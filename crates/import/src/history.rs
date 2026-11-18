//! History stage: the all-modes `player_activity` corpus, from the live table
//! and its archive shards. The same (membership, instance) can appear in
//! several sources with different content (failed export deletes, re-mined
//! history); the transform keeps the newest copy by source recency.

use std::path::PathBuf;

use anyhow::{Context, Result};
use duckdb::Connection;
use rayon::prelude::*;

use crate::layout::ShardKind;
use crate::progress::Progress;
use crate::rows::StgHistory;
use crate::sink::{Batcher, Sink, staging_writers};
use crate::sqlite;
use crate::stage::StagedSources;

const CHUNK_ROWS: i64 = 250_000;
const BATCH: usize = 16_384;

/// Counters from the history stage.
#[derive(Debug, Default, Clone)]
pub struct HistoryStats {
    pub files: u64,
    pub rows: u64,
}

pub fn load(
    duck: &Connection,
    staged: &StagedSources,
    progress: &Progress,
) -> Result<HistoryStats> {
    // Ordered oldest → newest: shards by name (day, then export run id), then
    // the live table. A row's recency is its source's position here, and the
    // transform keeps the newest copy of each (membership, instance).
    let mut dbs: Vec<(PathBuf, &str)> = staged
        .shards
        .iter()
        .filter(|(s, _)| s.kind == ShardKind::PlayerActivity)
        .map(|(_, p)| (p.clone(), "orig_rowid"))
        .collect();
    dbs.push((staged.main_db.clone(), "rowid"));

    let mut tasks = Vec::new();
    let mut total = 0;
    for (recency, (db, key)) in dbs.iter().enumerate() {
        let conn = sqlite::open(db)?;
        total += sqlite::count(&conn, "player_activity")?;
        for (lo, hi) in sqlite::rowid_ranges(&conn, "player_activity", CHUNK_ROWS)? {
            tasks.push((db.clone(), recency as i32, *key, lo, hi));
        }
    }

    let bar = progress.bar("activity history", total);
    let sink = Sink::<StgHistory>::spawn_parallel(duck, "stg.history", staging_writers())?;
    let sender = sink.sender();
    let produced = tasks
        .par_iter()
        .try_for_each(|(db, recency, key, lo, hi)| -> Result<()> {
            let conn = sqlite::open(db)?;
            // `key` is the row's insertion rowid in the live table (shards
            // carry it as orig_rowid), a deterministic final tie-breaker.
            let mut stmt = conn.prepare(&format!(
                "SELECT membership_id, character_id, instance_id, period, director_activity_hash, mode, \
                        kills, deaths, assists, duration_seconds, completion_reason, completed, player_count, \
                        {key} \
                 FROM player_activity WHERE rowid BETWEEN ?1 AND ?2"
            ))?;
            let mut rows = stmt.query([lo, hi])?;
            let mut out = Batcher::new(&sender, BATCH);
            let mut n = 0;
            while let Some(r) = rows.next()? {
                out.push(StgHistory {
                    recency: *recency,
                    row_key: r.get(13)?,
                    membership_id: r.get(0)?,
                    character_id: r.get(1)?,
                    instance_id: r.get(2)?,
                    period: r.get(3)?,
                    activity_hash: r.get::<_, Option<i64>>(4)?.map(|h| h as u32),
                    mode: r.get(5)?,
                    kills: r.get(6)?,
                    deaths: r.get(7)?,
                    assists: r.get(8)?,
                    duration_s: r.get(9)?,
                    completion_reason: r.get(10)?,
                    completed: r.get::<_, Option<i64>>(11)?.map(|v| v != 0),
                    player_count: r.get(12)?,
                })?;
                n += 1;
            }
            out.flush()?;
            bar.inc(n);
            Ok(())
        });
    drop(sender);
    // Settle writers before producers: a writer's error is the root cause of
    // a producer's "writer stopped" error.
    let rows = sink.finish()?;
    produced.context("reading player_activity")?;
    bar.finish();
    Ok(HistoryStats {
        files: dbs.len() as u64,
        rows,
    })
}
