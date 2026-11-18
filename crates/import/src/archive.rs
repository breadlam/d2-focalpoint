//! Archive stage: every raw PGCR (day shards, never-pulled outbox shards, and
//! the live `pgcr_raw` table) → staging rows with `src = archive`.
//!
//! Shards overlap: whenever the nightly export's delete step failed, the same
//! rows were exported again the next night, and some reports were fetched
//! again weeks later (see docs/data-sources.md). About 10 % of duplicate copies
//! are not byte-identical: Bungie re-renders player identity (names, emblems,
//! light level) at fetch time, and a few rosters differ. To stay
//! deterministic, a cheap pre-pass picks one copy per instance — the most
//! recently fetched, ties broken by source order — and only that copy is
//! decompressed and parsed.

use std::cmp::Reverse;
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

/// One stored copy of a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StoredCopy {
    instance_id: i64,
    fetched_at: i64,
    file: u32,
    rowid: i64,
}

/// Choose the copy of each instance to import: the most recently fetched
/// (current player identities), ties broken by source order, then rowid.
/// Returns, for each file, the sorted rowids of its winning copies.
fn select_copies(mut copies: Vec<StoredCopy>, files: usize) -> Vec<Vec<i64>> {
    copies.par_sort_unstable_by_key(|c| (c.instance_id, Reverse(c.fetched_at), c.file, c.rowid));
    let mut winners = vec![Vec::new(); files];
    let mut last = None;
    for c in copies {
        if last != Some(c.instance_id) {
            winners[c.file as usize].push(c.rowid);
            last = Some(c.instance_id);
        }
    }
    winners.par_iter_mut().for_each(|w| w.sort_unstable());
    winners
}

/// Read `(rowid, instance_id, fetched_at)` of every stored copy.
fn scan_copies(dbs: &[PathBuf]) -> Result<Vec<StoredCopy>> {
    let per_file = dbs
        .par_iter()
        .enumerate()
        .map(|(file, db)| -> Result<Vec<StoredCopy>> {
            let conn = sqlite::open(db)?;
            let mut stmt =
                conn.prepare("SELECT rowid, instance_id, coalesce(fetched_at, 0) FROM pgcr_raw")?;
            let rows = stmt.query_map([], |r| {
                Ok(StoredCopy {
                    rowid: r.get(0)?,
                    instance_id: r.get(1)?,
                    fetched_at: r.get(2)?,
                    file: file as u32,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(per_file.into_iter().flatten().collect())
}

struct Task {
    file: usize,
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

    let spinner = progress.spinner("archived PGCRs");
    spinner.set_message("choosing one copy per report");
    let copies = scan_copies(&dbs)?;
    let total = copies.len() as u64;
    let winners = select_copies(copies, dbs.len());
    spinner.finish_and_clear();

    let mut tasks = Vec::new();
    for (file, db) in dbs.iter().enumerate() {
        let conn = sqlite::open(db)?;
        for (lo, hi) in sqlite::rowid_ranges(&conn, "pgcr_raw", CHUNK_ROWS)? {
            tasks.push(Task { file, lo, hi });
        }
    }

    let bar = progress.bar("archived PGCRs", total);
    let run_sink = Sink::<StgRun>::spawn_parallel(duck, "stg.run", staging_writers())?;
    let entry_sink = Sink::<StgEntry>::spawn_parallel(duck, "stg.entry", staging_writers())?;
    let weapon_sink = Sink::<StgWeapon>::spawn_parallel(duck, "stg.weapon", staging_writers())?;
    let senders = Senders {
        runs: run_sink.sender(),
        entries: entry_sink.sender(),
        weapons: weapon_sink.sender(),
    };

    let read = AtomicU64::new(0);
    let dups = AtomicU64::new(0);
    let errors = AtomicU64::new(0);
    let examples = Mutex::new(Vec::new());

    let produced = tasks.par_iter().try_for_each(|t| -> Result<()> {
        let conn = sqlite::open(&dbs[t.file])?;
        let mut stmt = conn.prepare(
            "SELECT rowid, instance_id, raw FROM pgcr_raw WHERE rowid BETWEEN ?1 AND ?2",
        )?;
        let chosen = &winners[t.file];
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
            if chosen.binary_search(&r.get::<_, i64>(0)?).is_err() {
                dups.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            let instance_id: i64 = r.get(1)?;
            let raw = match r.get_ref(2)? {
                ValueRef::Blob(b) | ValueRef::Text(b) => b,
                _ => &[][..],
            };
            match pgcr::parse_stored(raw, &mut buf) {
                Ok(p) => emit(&p, &mut out)?,
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

    fn copy(instance_id: i64, fetched_at: i64, file: u32, rowid: i64) -> StoredCopy {
        StoredCopy {
            instance_id,
            fetched_at,
            file,
            rowid,
        }
    }

    #[test]
    fn latest_fetch_wins_then_source_order() {
        let copies = vec![
            copy(1, 100, 0, 10), // older fetch of 1
            copy(1, 200, 2, 5),  // latest fetch of 1 -> wins
            copy(2, 50, 1, 7),   // tie on fetched_at: file 1 beats file 2
            copy(2, 50, 2, 3),
            copy(3, 10, 2, 9), // unique
        ];
        let winners = select_copies(copies, 3);
        assert_eq!(winners, vec![vec![], vec![7], vec![5, 9]]);
    }

    #[test]
    fn selection_is_independent_of_input_order() {
        let mut copies: Vec<StoredCopy> = (0..1000)
            .map(|i| copy(i % 97, i % 5, (i % 3) as u32, i))
            .collect();
        let a = select_copies(copies.clone(), 3);
        copies.reverse();
        assert_eq!(select_copies(copies, 3), a);
        assert_eq!(a.iter().map(Vec::len).sum::<usize>(), 97);
    }
}
