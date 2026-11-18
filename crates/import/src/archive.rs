//! Archive stage: every raw PGCR (day shards, never-pulled outbox shards, and
//! the live `pgcr_raw` table) → staging rows with `src = archive`.
//!
//! Shards overlap: whenever the nightly export's delete step failed, the same
//! rows were exported again the next night (see docs/data-sources.md). Reports
//! are immutable, so the first copy of each instance id wins and later copies
//! are skipped before they are even decompressed.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use duckdb::Connection;
use rayon::prelude::*;
use rusqlite::types::ValueRef;
use searchlight_core::pgcr::{self, Pgcr};

use crate::layout::ShardKind;
use crate::progress::Progress;
use crate::rows::{SRC_ARCHIVE, StgEntry, StgRun, StgWeapon};
use crate::sink::{BatchSender, Batcher, Sink, staging_writers};
use crate::sqlite;
use crate::stage::StagedSources;

const CHUNK_ROWS: i64 = 20_000;
const BATCH: usize = 8_192;
const SHARDS: usize = 256;

/// Counters from the archive stage.
#[derive(Debug, Default, Clone)]
pub struct ArchiveStats {
    pub files: u64,
    pub rows_read: u64,
    pub duplicates: u64,
    pub parse_errors: u64,
    pub runs: u64,
    pub entries: u64,
    pub weapons: u64,
    /// Up to a handful of `(instance_id, error)` examples.
    pub error_examples: Vec<(i64, String)>,
}

/// A concurrent set of instance ids, sharded to keep lock contention low.
struct SeenSet(Vec<Mutex<HashSet<i64>>>);

impl SeenSet {
    fn new() -> Self {
        Self((0..SHARDS).map(|_| Mutex::new(HashSet::new())).collect())
    }

    fn shard(&self, id: i64) -> &Mutex<HashSet<i64>> {
        &self.0[(id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) as usize % SHARDS]
    }

    fn contains(&self, id: i64) -> bool {
        self.shard(id).lock().expect("seen-set lock").contains(&id)
    }

    /// Claim `id`; returns true only for the first claimant.
    fn first_sight(&self, id: i64) -> bool {
        self.shard(id).lock().expect("seen-set lock").insert(id)
    }
}

struct Task {
    db: PathBuf,
    table: &'static str,
    lo: i64,
    hi: i64,
}

struct Senders {
    runs: BatchSender<StgRun>,
    entries: BatchSender<StgEntry>,
    weapons: BatchSender<StgWeapon>,
}

pub fn load(
    duck: &Connection,
    staged: &StagedSources,
    progress: &Progress,
) -> Result<ArchiveStats> {
    // The live table first, then shards in name (≈ date) order.
    let mut dbs: Vec<PathBuf> = vec![staged.pgcr_db.clone()];
    dbs.extend(
        staged
            .shards
            .iter()
            .filter(|(s, _)| s.kind == ShardKind::PgcrRaw)
            .map(|(_, p)| p.clone()),
    );

    let mut tasks = Vec::new();
    let mut total = 0;
    for db in &dbs {
        let conn = sqlite::open(db)?;
        total += sqlite::count(&conn, "pgcr_raw")?;
        for (lo, hi) in sqlite::rowid_ranges(&conn, "pgcr_raw", CHUNK_ROWS)? {
            tasks.push(Task {
                db: db.clone(),
                table: "pgcr_raw",
                lo,
                hi,
            });
        }
    }

    let bar = progress.bar("archived PGCRs", total);
    let run_sink = Sink::<StgRun>::spawn_parallel(duck, "stg_run", staging_writers())?;
    let entry_sink = Sink::<StgEntry>::spawn_parallel(duck, "stg_entry", staging_writers())?;
    let weapon_sink = Sink::<StgWeapon>::spawn_parallel(duck, "stg_weapon", staging_writers())?;
    let senders = Senders {
        runs: run_sink.sender(),
        entries: entry_sink.sender(),
        weapons: weapon_sink.sender(),
    };

    let seen = SeenSet::new();
    let read = AtomicU64::new(0);
    let dups = AtomicU64::new(0);
    let errors = AtomicU64::new(0);
    let examples = Mutex::new(Vec::new());

    let produced = tasks.par_iter().try_for_each(|t| -> Result<()> {
        let conn = sqlite::open(&t.db)?;
        let mut stmt = conn.prepare(&format!(
            "SELECT instance_id, raw FROM \"{}\" WHERE rowid BETWEEN ?1 AND ?2",
            t.table
        ))?;
        let mut rows = stmt.query([t.lo, t.hi])?;
        let mut out = (
            Batcher::new(&senders.runs, BATCH),
            Batcher::new(&senders.entries, BATCH),
            Batcher::new(&senders.weapons, BATCH),
        );
        let mut buf = Vec::with_capacity(64 * 1024);
        let mut n = 0;
        while let Some(r) = rows.next()? {
            n += 1;
            let instance_id: i64 = r.get(0)?;
            // Cheap early skip for copies already imported; the id is only
            // claimed after a successful parse, so a corrupt copy never
            // shadows a good duplicate.
            if seen.contains(instance_id) {
                dups.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            let raw = match r.get_ref(1)? {
                ValueRef::Blob(b) | ValueRef::Text(b) => b,
                _ => &[][..],
            };
            match pgcr::parse_stored(raw, &mut buf) {
                Ok(p) if seen.first_sight(p.instance_id) => emit(&p, &mut out)?,
                Ok(_) => {
                    dups.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    errors.fetch_add(1, Ordering::Relaxed);
                    let mut ex = examples.lock().expect("examples lock");
                    if ex.len() < 10 {
                        ex.push((instance_id, e.to_string()));
                    }
                }
            }
        }
        out.0.flush()?;
        out.1.flush()?;
        out.2.flush()?;
        read.fetch_add(n, Ordering::Relaxed);
        bar.inc(n);
        Ok(())
    });
    drop(senders);
    // Settle writers before producers: a writer's error is the root cause of
    // a producer's "writer stopped" error.
    let (runs, entries, weapons) = (run_sink.finish(), entry_sink.finish(), weapon_sink.finish());
    let (runs, entries, weapons) = (runs?, entries?, weapons?);
    produced.context("reading archived PGCRs")?;

    let stats = ArchiveStats {
        files: dbs.len() as u64,
        rows_read: read.into_inner(),
        duplicates: dups.into_inner(),
        parse_errors: errors.into_inner(),
        runs,
        entries,
        weapons,
        error_examples: examples.into_inner().expect("examples lock"),
    };
    bar.finish();
    Ok(stats)
}

type Outputs = (Batcher<StgRun>, Batcher<StgEntry>, Batcher<StgWeapon>);

fn emit(p: &Pgcr, out: &mut Outputs) -> Result<()> {
    out.0.push(StgRun {
        src: SRC_ARCHIVE,
        instance_id: p.instance_id,
        period: p.period.clone(),
        activity_hash: p.director_activity_hash,
        reference_hash: p.reference_id,
        mode: p.mode,
        modes: Some(p.modes.clone()),
        difficulty_tier: p.difficulty_tier,
        duration_s: p.duration_s,
        player_count: p.player_count,
        started_from_beginning: Some(p.started_from_beginning),
        is_private: Some(p.is_private),
        starting_phase_index: p.starting_phase_index,
        skull_hashes: p.skull_hashes.clone(),
        tracked_target: None,
    })?;
    for e in &p.entries {
        for w in &e.weapons {
            out.2.push(StgWeapon {
                src: SRC_ARCHIVE,
                instance_id: p.instance_id,
                membership_id: e.membership_id,
                character_id: e.character_id,
                item_hash: w.item_hash,
                kills: w.kills,
                precision_kills: w.precision_kills,
            })?;
        }
        out.1.push(StgEntry {
            src: SRC_ARCHIVE,
            instance_id: p.instance_id,
            membership_id: e.membership_id,
            character_id: e.character_id,
            membership_type: e.membership_type,
            display_name: Some(e.display_name.clone()).filter(|s| !s.is_empty()),
            global_name: e.global_name.clone(),
            global_name_code: e.global_name_code,
            class: e.class_name.clone(),
            light_level: e.light_level,
            character_level: Some(e.character_level),
            emblem_hash: Some(e.emblem_hash).filter(|&h| h != 0),
            kills: e.kills,
            deaths: e.deaths,
            assists: e.assists,
            precision_kills: e.precision_kills,
            super_kills: e.super_kills,
            melee_kills: e.melee_kills,
            grenade_kills: e.grenade_kills,
            ability_kills: e.ability_kills,
            completed: e.completed,
            completion_reason: e.completion_reason,
            time_played_s: e.time_played_s,
            start_s: Some(e.start_s),
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seen_set_reports_first_sight_once() {
        let s = SeenSet::new();
        assert!(!s.contains(17_000_000_001));
        assert!(s.first_sight(17_000_000_001));
        assert!(s.contains(17_000_000_001));
        assert!(!s.first_sight(17_000_000_001));
        assert!(s.first_sight(-5));
        let firsts = (0..10_000i64)
            .into_par_iter()
            .filter(|&i| s.first_sight(i % 1000))
            .count();
        assert_eq!(firsts, 1000);
    }
}
