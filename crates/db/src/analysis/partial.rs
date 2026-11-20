//! Partial clears and partial participation: which content allows them,
//! what they look like, and how the run score treats them.
//!
//! Searchlight's later evaluator scored a player's **kill rate over their own
//! time played**, divided by lives used, so a late joiner or a short run is
//! judged on the pace it sustained; its target slate required fresh starts
//! for checkpointed content (raids, dungeons) and needed no gate for
//! Conquests and Ops, which cannot be resumed. `run_score` (ADR 0006)
//! instead measures kills against the activity's reference kills and pace
//! against the whole run's length, so a player present for part of a run is
//! scored as if present for all of it.

use crate::rank::CHECKPOINT_SHARE;
use crate::{Database, Result};

/// How often an activity group's recorded runs were resumed from a
/// checkpoint or joined in progress (all runs, qualified or not).
#[derive(Debug, Clone, PartialEq)]
pub struct Resumption {
    pub group: String,
    pub activities: usize,
    pub runs: usize,
    /// Shares of runs not started from the beginning, and with a player who
    /// joined more than a minute in.
    pub not_fresh: f64,
    pub late_join: f64,
    /// Activities with checkpoints ([`CHECKPOINT_SHARE`]).
    pub with_checkpoints: usize,
}

/// Checkpoints and late joins by activity group.
pub fn resumption(db: &Database) -> Result<Vec<Resumption>> {
    let mut stmt = db.conn().prepare(&format!(
        "WITH runs AS (
             SELECT r.instance_id, r.activity_hash,
                    NOT coalesce(r.started_from_beginning, true) AS not_fresh,
                    coalesce(max(rp.start_s), 0) > 60 AS late
             FROM run r JOIN run_player rp USING (instance_id) GROUP BY ALL),
         per AS (
             SELECT activity_hash, count(*) AS runs, avg(not_fresh::INTEGER) AS nf,
                    avg(late::INTEGER) AS lj
             FROM runs GROUP BY 1)
         SELECT a.target_group, count(*), sum(per.runs),
                sum(per.nf * per.runs) / sum(per.runs), sum(per.lj * per.runs) / sum(per.runs),
                count(*) FILTER (WHERE per.nf >= {CHECKPOINT_SHARE})
         FROM per JOIN activity a USING (activity_hash)
         WHERE a.target_group IS NOT NULL
         GROUP BY 1 ORDER BY 1"
    ))?;
    let rows = stmt.query_map([], |r| {
        let n = |i: usize| r.get::<_, i64>(i).map(|v| usize::try_from(v).unwrap_or(0));
        Ok(Resumption {
            group: r.get(0)?,
            activities: n(1)?,
            runs: usize::try_from(r.get::<_, i128>(2)?).unwrap_or(0),
            not_fresh: r.get(3)?,
            late_join: r.get(4)?,
            with_checkpoints: n(5)?,
        })
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

/// Scored clears of one group and kind: kill rates, and the run score of
/// players present for all and for part of the run.
#[derive(Debug, Clone, PartialEq)]
pub struct Pace {
    pub group: String,
    /// "fireteam", "solo, full length" or "solo, faster than the fastest
    /// fireteams" (only for content with checkpoints).
    pub kind: &'static str,
    pub clears: usize,
    /// Median kills per minute played, and the same per life.
    pub kills_per_minute: f64,
    pub per_life: f64,
}

/// Kill rates by group: fireteam clears, full-length solo clears and solo
/// clears faster than the fastest 1% of fireteam clears.
pub fn pace(db: &Database) -> Result<Vec<Pace>> {
    let mut stmt = db.conn().prepare(
        "WITH p AS (
             SELECT s.activity_hash, s.is_solo, s.duration_s, sum(rp.kills) AS kills,
                    sum(rp.deaths) AS deaths, greatest(sum(rp.time_played_s), 120) AS played
             FROM run_score s JOIN run_player rp USING (instance_id, membership_id)
             GROUP BY s.instance_id, s.membership_id, s.activity_hash, s.is_solo, s.duration_s),
         fast AS (SELECT activity_hash, quantile_cont(duration_s, 0.01) AS p1 FROM p
                  WHERE NOT is_solo GROUP BY 1)
         SELECT a.target_group,
                CASE WHEN NOT p.is_solo THEN 0 WHEN p.duration_s < fast.p1 THEN 2 ELSE 1 END AS kind,
                count(*), median(60.0 * p.kills / p.played),
                median(60.0 * p.kills / p.played / (1 + p.deaths))
         FROM p JOIN fast USING (activity_hash) JOIN activity a ON a.activity_hash = p.activity_hash
         WHERE a.target_group IS NOT NULL
         GROUP BY ALL ORDER BY 1, 2",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Pace {
            group: r.get(0)?,
            kind: match r.get::<_, i32>(1)? {
                0 => "fireteam",
                1 => "solo, full length",
                _ => "solo, faster than the fastest fireteams",
            },
            clears: usize::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
            kills_per_minute: r.get(3)?,
            per_life: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

/// Fireteam clears of one group, by presence: the run score now, and the
/// score a kill rate over time played would give.
#[derive(Debug, Clone, PartialEq)]
pub struct Presence {
    pub group: String,
    /// Present for at least 90% of the run, or less.
    pub partial: bool,
    pub clears: usize,
    /// Median share of the run the player was present for.
    pub played_share: f64,
    /// Median `run_score.score` (1 = a typical clear).
    pub score: f64,
    /// Median of the same score with kills and pace measured over the
    /// player's time played: `(kills / played) / (reference kills /
    /// reference duration)`, bounded like kills × speed, times survival.
    pub rate_score: f64,
}

/// Fireteam clears by presence, per group with checkpoints or late joins.
pub fn presence(db: &Database) -> Result<Vec<Presence>> {
    let p = focalpoint_core::rank::ScoreParams::default();
    let lo = p.kill_bounds.0 * p.speed_bounds.0;
    let hi = p.kill_bounds.1 * p.speed_bounds.1;
    let mut stmt = db.conn().prepare(&format!(
        "WITH p AS (
             SELECT s.activity_hash, s.instance_id, s.membership_id, any_value(s.duration_s) AS duration_s,
                    any_value(s.score) AS score, any_value(s.survival) AS survival,
                    sum(rp.kills) AS kills, greatest(sum(rp.time_played_s), 120) AS played
             FROM run_score s JOIN run_player rp USING (instance_id, membership_id)
             WHERE NOT s.is_solo GROUP BY 1, 2, 3)
         SELECT a.target_group, p.played < 0.9 * p.duration_s AS partial, count(*),
                median(least(p.played * 1.0 / p.duration_s, 1)), median(p.score),
                median(least(greatest((p.kills / p.played) / (t.kills / t.duration_s), {lo}), {hi})
                       * p.survival)
         FROM p JOIN activity a USING (activity_hash)
         JOIN activity_reference t ON t.activity_hash = p.activity_hash AND NOT t.is_solo
         WHERE a.target_group IS NOT NULL AND t.kills > 0
         GROUP BY ALL ORDER BY 1, 2"
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok(Presence {
            group: r.get(0)?,
            partial: r.get(1)?,
            clears: usize::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
            played_share: r.get(3)?,
            score: r.get(4)?,
            rate_score: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_studies_read_the_database() {
        let (_d, db) = crate::site::tests::db();
        assert!(resumption(&db).is_ok());
        assert!(pace(&db).is_ok());
        assert!(presence(&db).is_ok());
    }
}
