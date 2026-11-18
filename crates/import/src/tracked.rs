//! Tracked stage: production's pre-parsed tables from the live main DB.
//!
//! * `run` / `run_entry` → staging (`src = tracked`), the fallback for runs
//!   without an archived raw PGCR (mostly before the archive began, Jun 24);
//! * `skull_catalog` → the `skull` dimension;
//! * `activity` → the curated target slate per activity hash;
//! * `player`, `seeded_player`, `random_player` → identity fallback and flags.
//!
//! `candidate` and `player_history` are not imported: the former is monitor
//! bookkeeping, the latter is derivable from runs (see docs/data-sources.md).

use std::path::Path;

use anyhow::{Context, Result};
use duckdb::{Connection, params};
use rayon::prelude::*;
use searchlight_core::pgcr::WeaponKills;

use crate::progress::Progress;
use crate::rows::{SRC_TRACKED, StgEntry, StgRun, StgWeapon};
use crate::sink::{Batcher, Sink, staging_writers};
use crate::sqlite;

const CHUNK_ROWS: i64 = 100_000;
const BATCH: usize = 8_192;

/// Counters from the tracked stage.
#[derive(Debug, Default, Clone)]
pub struct TrackedStats {
    pub runs: u64,
    pub entries: u64,
    pub weapons: u64,
    pub bad_weapon_json: u64,
    pub players: u64,
}

pub fn load(duck: &Connection, main_db: &Path, progress: &Progress) -> Result<TrackedStats> {
    let src = sqlite::open(main_db)?;
    let mut stats = TrackedStats::default();

    copy_skulls(&src, duck)?;
    copy_target_groups(&src, duck)?;
    stats.players = copy_players(&src, duck)?;
    stats.runs = copy_runs(main_db, duck, progress)?;
    let (entries, weapons, bad) = copy_entries(main_db, duck, progress)?;
    stats.entries = entries;
    stats.weapons = weapons;
    stats.bad_weapon_json = bad;
    Ok(stats)
}

fn copy_skulls(src: &rusqlite::Connection, duck: &Connection) -> Result<()> {
    let mut stmt = src.prepare(
        "SELECT skull_hash, name, kind, slot, connotation, description FROM skull_catalog",
    )?;
    let mut rows = stmt.query([])?;
    let mut a = duck.appender("skull")?;
    while let Some(r) = rows.next()? {
        a.append_row(params![
            r.get::<_, i64>(0)? as u32,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, Option<String>>(5)?,
        ])?;
    }
    a.flush()?;
    Ok(())
}

fn copy_target_groups(src: &rusqlite::Connection, duck: &Connection) -> Result<()> {
    let mut stmt = src.prepare(
        "SELECT director_activity_hash, target_name FROM activity WHERE target_name <> '?'",
    )?;
    let mut rows = stmt.query([])?;
    let mut a = duck.appender("stg_target_group")?;
    while let Some(r) = rows.next()? {
        a.append_row(params![r.get::<_, i64>(0)? as u32, r.get::<_, String>(1)?])?;
    }
    a.flush()?;
    Ok(())
}

fn copy_players(src: &rusqlite::Connection, duck: &Connection) -> Result<u64> {
    let mut n = 0;
    {
        let mut stmt = src.prepare(
            "SELECT membership_id, membership_type, display_name, global_name, global_name_code, first_seen FROM player",
        )?;
        let mut rows = stmt.query([])?;
        let mut a = duck.appender("stg_player")?;
        while let Some(r) = rows.next()? {
            a.append_row(params![
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<i64>>(5)?,
            ])?;
            n += 1;
        }
        a.flush()?;
    }
    let mut a = duck.appender("stg_player_flag")?;
    for (table, flag) in [
        ("seeded_player", "seeded"),
        ("random_player", "random_sample"),
    ] {
        if !sqlite::has_table(src, table)? {
            continue;
        }
        let mut stmt = src.prepare(&format!("SELECT membership_id FROM {table}"))?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            a.append_row(params![r.get::<_, i64>(0)?, flag])?;
        }
    }
    a.flush()?;
    Ok(n)
}

fn parse_u32_json_array(s: Option<String>) -> Vec<u32> {
    s.and_then(|s| serde_json::from_str::<Vec<u32>>(&s).ok())
        .unwrap_or_default()
}

/// `run` (keyed by instance id, so its rowids are sparse but evenly spread)
/// is read in parallel rowid ranges like `run_entry`.
fn copy_runs(main_db: &Path, duck: &Connection, progress: &Progress) -> Result<u64> {
    let (ranges, total) = {
        let src = sqlite::open(main_db)?;
        (
            sqlite::rowid_ranges(&src, "run", CHUNK_ROWS)?,
            sqlite::count(&src, "run")?,
        )
    };
    let bar = progress.bar("tracked runs", total);
    let sink = Sink::<StgRun>::spawn_parallel(duck, "stg_run", staging_writers())?;
    let sender = sink.sender();
    let produced = ranges.par_iter().try_for_each(|&(lo, hi)| -> Result<()> {
        let src = sqlite::open(main_db)?;
        let mut stmt = src.prepare(
            "SELECT instance_id, period, director_activity_hash, reference_id, mode, \
                    activity_difficulty_tier, duration_seconds, player_count, \
                    started_from_beginning, selected_skull_hashes, target_name \
             FROM run WHERE rowid BETWEEN ?1 AND ?2",
        )?;
        let mut rows = stmt.query([lo, hi])?;
        let mut out = Batcher::new(&sender, BATCH);
        let mut n = 0;
        while let Some(r) = rows.next()? {
            out.push(StgRun {
                src: SRC_TRACKED,
                instance_id: r.get(0)?,
                period: r.get(1)?,
                activity_hash: r.get::<_, i64>(2)? as u32,
                reference_hash: r.get::<_, i64>(3)? as u32,
                mode: r.get(4)?,
                modes: None,
                difficulty_tier: r.get(5)?,
                duration_s: r.get::<_, i64>(6)? as u32,
                player_count: r.get::<_, i64>(7)? as u32,
                started_from_beginning: r.get::<_, Option<i64>>(8)?.map(|v| v != 0),
                is_private: None,
                starting_phase_index: None,
                skull_hashes: parse_u32_json_array(r.get(9)?),
                tracked_target: r.get(10)?,
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
    let n = sink.finish()?;
    produced.context("reading run")?;
    bar.finish();
    Ok(n)
}

/// `run_entry` is the largest tracked table; read it in parallel rowid ranges.
fn copy_entries(main_db: &Path, duck: &Connection, progress: &Progress) -> Result<(u64, u64, u64)> {
    let ranges = {
        let src = sqlite::open(main_db)?;
        sqlite::rowid_ranges(&src, "run_entry", CHUNK_ROWS)?
    };
    let total = sqlite::count(&sqlite::open(main_db)?, "run_entry")?;
    let bar = progress.bar("tracked entries", total);
    let entries = Sink::<StgEntry>::spawn_parallel(duck, "stg_entry", staging_writers())?;
    let weapons = Sink::<StgWeapon>::spawn_parallel(duck, "stg_weapon", staging_writers())?;
    let (es, ws) = (entries.sender(), weapons.sender());

    let produced = ranges
        .par_iter()
        .map(|&(lo, hi)| -> Result<u64> {
            let src = sqlite::open(main_db)?;
            let mut stmt = src.prepare_cached(
                "SELECT instance_id, membership_id, character_id, membership_type, display_name, global_name, \
                        global_name_code, character_class, light_level, kills, deaths, assists, \
                        precision_kills, super_kills, melee_kills, grenade_kills, ability_kills, \
                        completed, completion_reason, time_played_seconds, weapons_used \
                 FROM run_entry WHERE rowid BETWEEN ?1 AND ?2",
            )?;
            let mut rows = stmt.query([lo, hi])?;
            let mut eb = Batcher::new(&es, BATCH);
            let mut wb = Batcher::new(&ws, BATCH);
            let mut bad = 0;
            let mut n = 0;
            while let Some(r) = rows.next()? {
                let e = StgEntry {
                    src: SRC_TRACKED,
                    instance_id: r.get(0)?,
                    membership_id: r.get(1)?,
                    character_id: r.get(2)?,
                    membership_type: r.get(3)?,
                    display_name: r.get(4)?,
                    global_name: r.get(5)?,
                    global_name_code: r.get::<_, Option<i64>>(6)?.map(|c| c as u16),
                    class: r.get(7)?,
                    light_level: r.get(8)?,
                    character_level: None,
                    emblem_hash: None,
                    kills: r.get(9)?,
                    deaths: r.get(10)?,
                    assists: r.get(11)?,
                    precision_kills: r.get(12)?,
                    super_kills: r.get(13)?,
                    melee_kills: r.get(14)?,
                    grenade_kills: r.get(15)?,
                    ability_kills: r.get(16)?,
                    completed: r.get::<_, i64>(17)? != 0,
                    completion_reason: r.get(18)?,
                    time_played_s: r.get(19)?,
                    start_s: None,
                };
                let weapons_json: String = r.get(20)?;
                match serde_json::from_str::<Vec<WeaponKills>>(&weapons_json) {
                    Ok(list) => {
                        for w in list {
                            wb.push(StgWeapon {
                                src: SRC_TRACKED,
                                instance_id: e.instance_id,
                                membership_id: e.membership_id,
                                character_id: e.character_id,
                                item_hash: w.item_hash,
                                kills: w.kills,
                                precision_kills: w.precision_kills,
                            })?;
                        }
                    }
                    Err(_) => bad += 1,
                }
                eb.push(e)?;
                n += 1;
            }
            eb.flush()?;
            wb.flush()?;
            bar.inc(n);
            Ok(bad)
        })
        .sum::<Result<u64>>();
    drop((es, ws));
    // Settle writers before producers: a writer's error is the root cause of
    // a producer's "writer stopped" error.
    let (ne, nw) = (entries.finish(), weapons.finish());
    let (ne, nw) = (ne?, nw?);
    let bad = produced.context("reading run_entry")?;
    bar.finish();
    Ok((ne, nw, bad))
}
