//! History stage: the all-modes `player_activity` corpus, from the live table
//! (src 1) and its archive shards (src 0). Duplicates across sources (failed
//! export deletes, re-exports) are removed in the transform step.

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
    let mut dbs: Vec<(PathBuf, u8)> = vec![(staged.main_db.clone(), 1)];
    dbs.extend(
        staged
            .shards
            .iter()
            .filter(|(s, _)| s.kind == ShardKind::PlayerActivity)
            .map(|(_, p)| (p.clone(), 0)),
    );

    let mut tasks = Vec::new();
    let mut total = 0;
    for (db, src) in &dbs {
        let conn = sqlite::open(db)?;
        total += sqlite::count(&conn, "player_activity")?;
        for (lo, hi) in sqlite::rowid_ranges(&conn, "player_activity", CHUNK_ROWS)? {
            tasks.push((db.clone(), *src, lo, hi));
        }
    }

    let bar = progress.bar("activity history", total);
    let sink = Sink::<StgHistory>::spawn_parallel(duck, "stg.history", staging_writers())?;
    let sender = sink.sender();
    let produced = tasks
        .par_iter()
        .try_for_each(|(db, src, lo, hi)| -> Result<()> {
            let conn = sqlite::open(db)?;
            let mut stmt = conn.prepare(
                "SELECT membership_id, character_id, instance_id, period, director_activity_hash, mode, \
                        kills, deaths, assists, duration_seconds, completion_reason, completed, player_count \
                 FROM player_activity WHERE rowid BETWEEN ?1 AND ?2",
            )?;
            let mut rows = stmt.query([lo, hi])?;
            let mut out = Batcher::new(&sender, BATCH);
            let mut n = 0;
            while let Some(r) = rows.next()? {
                out.push(StgHistory {
                    src: *src,
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
