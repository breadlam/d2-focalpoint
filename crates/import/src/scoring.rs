//! Scoring stage (after finalize): measure every qualified clear against a
//! typical clear of its activity and solo status
//! ([`focalpoint_core::rank`]), producing `activity_reference` and
//! `run_score` for the ranking queries.
//!
//! The population is production's own definition of difficult content: runs
//! that met a Searchlight target's gates (`run.qualified_target`: fresh
//! start, difficulty tier, feat floor, post-2026-06-09), cleared, with kills.
//! A player who switched characters mid-run has one entry per character in
//! the report, so entries are summed per player and run.

use anyhow::Result;
use duckdb::{Connection, params};
use focalpoint_core::rank::{Reference, ScoreParams};
use rayon::prelude::*;

use crate::progress::Progress;
use crate::sink::appender;

/// Counters from the scoring stage.
#[derive(Debug, Default, Clone)]
pub struct ScoringStats {
    /// Scored player-runs.
    pub runs: u64,
    pub players: u64,
    pub activities: u64,
    /// Activities with qualified clears that are not scored because their
    /// median clear has too few kills.
    pub unscored_activities: u64,
}

struct Input {
    instance_id: i64,
    membership_id: i64,
    kills: u32,
    deaths: u32,
    duration_s: u32,
    reference: Reference,
}

/// Build the references, score every qualified clear, and write
/// `activity_reference` and `run_score`.
pub fn apply(conn: &Connection, params: &ScoreParams, progress: &Progress) -> Result<ScoringStats> {
    let spinner = progress.spinner("scoring");
    spinner.set_message("collecting qualified clears");
    conn.execute_batch(
        "CREATE TABLE stg.score_input AS
         SELECT p.instance_id, p.membership_id, r.activity_hash, r.started_at,
                coalesce(r.is_solo, false) AS is_solo,
                r.duration_s,
                sum(p.kills)::INTEGER     AS kills,
                sum(p.deaths)::INTEGER    AS deaths,
                bool_or(p.has_build)      AS has_build
         FROM run_player p JOIN run r USING (instance_id)
         WHERE r.qualified_target IS NOT NULL AND r.duration_s > 0
         GROUP BY ALL
         HAVING bool_or(p.is_clear) AND sum(p.kills) > 0
         ORDER BY p.instance_id, p.membership_id;",
    )?;

    spinner.set_message("activity references");
    // Parameters are plain numbers from `ScoreParams`, inlined.
    conn.execute_batch(&format!(
        "CREATE TABLE stg.reference AS
         WITH buckets AS (
             SELECT activity_hash, is_solo, count(*) AS clears, median(kills) AS kills,
                    median(duration_s) AS duration_s, median(deaths) AS deaths
             FROM stg.score_input GROUP BY ALL
         ), activities AS (
             SELECT activity_hash, median(kills) AS kills,
                    median(duration_s) AS duration_s, median(deaths) AS deaths
             FROM stg.score_input GROUP BY ALL
         )
         SELECT b.activity_hash, b.is_solo, b.clears,
                b.clears < {min_clears} AS is_borrowed,
                a.kills >= {min_kills}  AS is_scored,
                CASE WHEN b.clears < {min_clears} THEN a.kills      ELSE b.kills      END AS kills,
                CASE WHEN b.clears < {min_clears} THEN a.duration_s ELSE b.duration_s END AS duration_s,
                CASE WHEN b.clears < {min_clears} THEN a.deaths     ELSE b.deaths     END AS deaths
         FROM buckets b JOIN activities a USING (activity_hash);",
        min_clears = params.min_reference_clears,
        min_kills = params.min_reference_kills,
    ))?;

    spinner.set_message("scoring runs");
    let inputs: Vec<Input> = {
        let mut stmt = conn.prepare(
            "SELECT i.instance_id, i.membership_id, i.kills, i.deaths, i.duration_s,
                    r.kills, r.duration_s, r.deaths
             FROM stg.score_input i JOIN stg.reference r USING (activity_hash, is_solo)
             WHERE r.is_scored
             ORDER BY i.instance_id, i.membership_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Input {
                instance_id: r.get(0)?,
                membership_id: r.get(1)?,
                kills: r.get(2)?,
                deaths: r.get(3)?,
                duration_s: r.get(4)?,
                reference: Reference {
                    kills: r.get(5)?,
                    duration_s: r.get(6)?,
                    deaths: r.get(7)?,
                },
            })
        })?;
        rows.collect::<duckdb::Result<_>>()?
    };
    let performance: Vec<_> = inputs
        .par_iter()
        .map(|i| params.performance(i.kills, i.deaths, i.duration_s, &i.reference))
        .collect();

    spinner.set_message("writing");
    conn.execute_batch(
        "CREATE TABLE stg.run_perf (instance_id BIGINT, membership_id BIGINT,
                                    kill_index DOUBLE, speed DOUBLE, survival DOUBLE)",
    )?;
    {
        let mut a = appender(conn, "stg.run_perf")?;
        for (i, p) in inputs.iter().zip(&performance) {
            if let Some(p) = p {
                a.append_row(params![
                    i.instance_id,
                    i.membership_id,
                    p.kill_index,
                    p.speed,
                    p.survival
                ])?;
            }
        }
        a.flush()?;
    }
    conn.execute_batch(include_str!("sql/scoring.sql"))?;

    let stats = conn.query_row(
        "SELECT (SELECT count(*) FROM run_score),
                (SELECT count(DISTINCT membership_id) FROM run_score),
                (SELECT count(DISTINCT activity_hash) FROM activity_reference WHERE is_scored),
                (SELECT count(DISTINCT activity_hash) FROM activity_reference WHERE NOT is_scored)",
        [],
        |r| {
            Ok(ScoringStats {
                runs: r.get(0)?,
                players: r.get(1)?,
                activities: r.get(2)?,
                unscored_activities: r.get(3)?,
            })
        },
    )?;
    spinner.finish_with_message("scoring done");
    Ok(stats)
}
