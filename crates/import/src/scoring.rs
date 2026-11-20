//! Scoring stage (after finalize): measure every qualified clear against a
//! typical clear of its activity by the same kind of player — solo,
//! fireteam member, or partial participant ([`focalpoint_core::rank`], ADR
//! 0009) — producing `activity_reference` and `run_score` for the ranking
//! queries.
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
    played_s: u32,
    is_partial: bool,
    reference: Reference,
}

/// Build the references, score every qualified clear, and write
/// `activity_reference` and `run_score`.
pub fn apply(conn: &Connection, params: &ScoreParams, progress: &Progress) -> Result<ScoringStats> {
    let spinner = progress.spinner("scoring");
    spinner.set_message("collecting qualified clears");
    // A player's time played is unknown in no scored clear today; a missing
    // value counts as the whole run.
    conn.execute_batch(&format!(
        "CREATE TABLE stg.score_input AS
         WITH players AS (
             SELECT p.instance_id, p.membership_id, r.activity_hash, r.started_at,
                    coalesce(r.is_solo, false) AS is_solo,
                    r.duration_s,
                    coalesce(sum(p.time_played_s), any_value(r.duration_s))::INTEGER AS played_s,
                    sum(p.kills)::INTEGER     AS kills,
                    sum(p.deaths)::INTEGER    AS deaths,
                    bool_or(p.has_build)      AS has_build
             FROM run_player p JOIN run r USING (instance_id)
             WHERE r.qualified_target IS NOT NULL AND r.duration_s > 0
             GROUP BY ALL
             HAVING bool_or(p.is_clear) AND sum(p.kills) > 0)
         SELECT *, NOT is_solo AND played_s < {partial} * duration_s AS is_partial
         FROM players
         ORDER BY instance_id, membership_id;",
        partial = params.partial_presence,
    ))?;

    spinner.set_message("activity references");
    // Parameters are plain numbers from `ScoreParams`, inlined.
    conn.execute_batch(&format!(
        "CREATE TABLE stg.reference AS
         WITH buckets AS (
             SELECT activity_hash, is_solo, is_partial, count(*) AS clears, median(kills) AS kills,
                    median(CASE WHEN is_partial THEN played_s ELSE duration_s END) AS duration_s,
                    median(deaths) AS deaths
             FROM stg.score_input GROUP BY ALL
         ), activities AS (
             SELECT activity_hash, median(kills) AS kills,
                    median(duration_s) AS duration_s, median(deaths) AS deaths
             FROM stg.score_input GROUP BY ALL
         )
         SELECT b.activity_hash, b.is_solo, b.is_partial, b.clears,
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
                    i.played_s, i.is_partial, r.kills, r.duration_s, r.deaths
             FROM stg.score_input i JOIN stg.reference r USING (activity_hash, is_solo, is_partial)
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
                played_s: r.get(5)?,
                is_partial: r.get(6)?,
                reference: Reference {
                    kills: r.get(7)?,
                    duration_s: r.get(8)?,
                    deaths: r.get(9)?,
                },
            })
        })?;
        rows.collect::<duckdb::Result<_>>()?
    };
    let performance: Vec<_> = inputs
        .par_iter()
        .map(|i| {
            if i.is_partial {
                params.partial_performance(i.kills, i.deaths, i.played_s, &i.reference)
            } else {
                params.performance(i.kills, i.deaths, i.duration_s, &i.reference)
            }
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use focalpoint_db::Database;

    /// Sixty fireteam clears of one activity (20 minutes, three players).
    /// Two players play the whole run with 100 kills each; the third joined
    /// late: half of them for 5 minutes with 25 kills, half for 10 with 50
    /// — the same kill rate. A sixty-first latecomer kills twice as fast.
    fn scored() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(&dir.path().join("t.duckdb")).unwrap();
        let conn = db.conn();
        conn.execute_batch(
            "ATTACH ':memory:' AS stg;
             INSERT INTO run (instance_id, started_at, activity_hash, duration_s, roster_size, is_solo,
                              started_from_beginning, qualified_target, source, is_tracked)
             SELECT i, TIMESTAMP '2026-07-01', 7, 1200, 3, false, true, 'T', 'archive', false
             FROM range(0, 61) r(i);
             INSERT INTO run_player (instance_id, membership_id, character_id, started_at, activity_hash,
                                     kills, deaths, assists, precision_kills, super_kills, melee_kills,
                                     grenade_kills, ability_kills, completed, is_clear, time_played_s, start_s)
             SELECT i, m, m, TIMESTAMP '2026-07-01', 7,
                    CASE WHEN m < 3 THEN 100 WHEN i = 60 THEN 100 WHEN i % 2 = 0 THEN 25 ELSE 50 END,
                    0, 0, 0, 0, 0, 0, 0, true, true,
                    CASE WHEN m < 3 THEN 1200 WHEN i % 2 = 0 AND i < 60 THEN 300 ELSE 600 END,
                    CASE WHEN m < 3 THEN 0 ELSE 600 END
             FROM range(0, 61) r(i), range(1, 4) p(m);",
        )
        .unwrap();
        apply(conn, &ScoreParams::default(), &Progress::hidden()).unwrap();
        (dir, db)
    }

    fn get(db: &Database, sql: &str) -> f64 {
        db.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn partial_participants_have_their_own_reference() {
        let (_d, db) = scored();
        // Fireteam members keep a reference of their own: 100 kills in 20
        // minutes, not dragged down by the latecomers.
        let fireteam = "FROM activity_reference WHERE NOT is_solo AND NOT is_partial";
        assert!((get(&db, &format!("SELECT kills {fireteam}")) - 100.0).abs() < 1e-9);
        assert!((get(&db, &format!("SELECT duration_s {fireteam}")) - 1200.0).abs() < 1e-9);
        // Latecomers: median 50 kills in a median 10 minutes played.
        let partial = "FROM activity_reference WHERE is_partial";
        assert!((get(&db, &format!("SELECT clears {partial}")) - 61.0).abs() < 1e-9);
        assert!((get(&db, &format!("SELECT kills {partial}")) - 50.0).abs() < 1e-9);
        assert!((get(&db, &format!("SELECT duration_s {partial}")) - 600.0).abs() < 1e-9);
        assert!(get(&db, &format!("SELECT (NOT is_borrowed)::DOUBLE {partial}")) > 0.5);
    }

    #[test]
    fn partial_participants_are_scored_by_kill_rate() {
        let (_d, db) = scored();
        let partial = "FROM run_score WHERE is_partial";
        assert!((get(&db, &format!("SELECT count(*) {partial}")) - 61.0).abs() < 1e-9);
        // Five or ten minutes at the typical rate score the same; twice the
        // rate scores twice as well.
        let typical = format!("{partial} AND instance_id < 60");
        assert!((get(&db, &format!("SELECT min(kill_index) {typical}")) - 1.0).abs() < 1e-9);
        assert!((get(&db, &format!("SELECT max(kill_index) {typical}")) - 1.0).abs() < 1e-9);
        assert!(
            (get(
                &db,
                &format!("SELECT kill_index {partial} AND instance_id = 60")
            ) - 2.0)
                .abs()
                < 1e-9
        );
        assert!((get(&db, &format!("SELECT max(speed) {partial}")) - 1.0).abs() < 1e-9);
        assert!((get(&db, &format!("SELECT min(played_s) {partial}")) - 300.0).abs() < 1e-9);
        // Centred within their bucket.
        assert!(get(&db, &format!("SELECT abs(sum(perf)) {partial}")) < 1e-9);
        // Full participants are fireteam members, scored over the whole run.
        let full = "FROM run_score WHERE NOT is_partial";
        assert!((get(&db, &format!("SELECT count(*) {full}")) - 122.0).abs() < 1e-9);
        assert!((get(&db, &format!("SELECT min(score) {full}")) - 1.0).abs() < 1e-9);
    }
}
