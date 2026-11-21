//! How much harder each difficulty step makes the content, measured on the
//! same players in the same activities: a player's kill rate and deaths at
//! each step of an activity's difficulty ladder (`difficulty_tier`) against
//! their own clears of that activity at a base step (Master by default).
//!
//! Comparing players with themselves removes the selection that hides the
//! effect across players — the players who clear the hardest steps are the
//! strongest. The kill rate is what enemy durability does to a build's
//! throughput: it falls by less than enemies' health rises, as fights are
//! not all shooting, and red-bar enemies gain health more slowly with Power
//! delta than majors and bosses (Edge of Fate). See `docs/durability.md`.

use duckdb::params;

use crate::{Database, Result};

/// One difficulty step of an activity group, against the base step.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub group: String,
    /// The step's name in the manifest (Normal, Master, Ultimate, ...).
    pub step: String,
    /// The step's median `activityLevel` across the group's ladders.
    pub activity_level: f64,
    /// Player-activity pairs with clears at both this step and the base.
    pub pairs: i64,
    pub players: i64,
    /// Geometric mean of the pairs' kill-rate ratios to the base step.
    pub kill_rate: f64,
    /// Deaths per 10 minutes played, minus the same pairs' at the base step.
    pub deaths_10m: f64,
}

/// Every step with at least `min_pairs` player-activity pairs that also
/// cleared the activity at `base`, by group and activity level. Clears
/// count when the player played over three minutes and got a kill.
pub fn steps(db: &Database, base: &str, min_pairs: i64) -> Result<Vec<Step>> {
    let sql = "
        WITH clears AS (
            SELECT p.membership_id, r.activity_hash, a.target_group AS grp, t.name AS step,
                   t.activity_level AS lvl,
                   p.kills * 60.0 / p.time_played_s AS kpm,
                   p.deaths * 600.0 / p.time_played_s AS dpm
            FROM run_player p
            JOIN run r USING (instance_id)
            JOIN activity a ON a.activity_hash = r.activity_hash
            JOIN difficulty_tier t
              ON t.collection_hash = a.difficulty_collection_hash AND t.tier = r.difficulty_tier
            WHERE p.is_clear AND p.time_played_s > 180 AND p.kills > 0
              AND a.target_group IS NOT NULL),
        pairs AS (
            SELECT grp, membership_id, activity_hash, step, median(lvl) AS lvl,
                   exp(avg(ln(kpm))) AS kpm, avg(dpm) AS dpm
            FROM clears GROUP BY ALL),
        base AS (
            SELECT grp, membership_id, activity_hash, kpm AS kpm0, dpm AS dpm0
            FROM pairs WHERE step = ?)
        SELECT p.grp, p.step, median(p.lvl), count(*), count(DISTINCT p.membership_id),
               exp(avg(ln(p.kpm / b.kpm0))), avg(p.dpm - b.dpm0)
        FROM pairs p JOIN base b USING (grp, membership_id, activity_hash)
        GROUP BY ALL HAVING count(*) >= ?
        ORDER BY p.grp, median(p.lvl), p.step";
    let mut stmt = db.conn().prepare(sql)?;
    let rows = stmt.query_map(params![base, min_pairs], |r| {
        Ok(Step {
            group: r.get(0)?,
            step: r.get(1)?,
            activity_level: r.get(2)?,
            pairs: r.get(3)?,
            players: r.get(4)?,
            kill_rate: r.get(5)?,
            deaths_10m: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn players_are_measured_against_their_own_base_step() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(&dir.path().join("d.duckdb")).unwrap();
        // Two players clear the same Op at Master (step 0) and Ultimate (step 1):
        // 10 → 8 and 20 → 18 kills a minute; 1 → 3 and 0 → 1 deaths in ten.
        db.conn()
            .execute_batch(
                "INSERT INTO activity (activity_hash, name, target_group, difficulty_collection_hash)
                 VALUES (1, 'Op: Customize', 'Ultimate Fireteam Op', 7);
                 INSERT INTO difficulty_tier VALUES (7, 0, 'Master', 40, 40, 300, 0), (7, 1, 'Ultimate', 50, 60, 400, -100);
                 INSERT INTO run (instance_id, started_at, activity_hash, difficulty_tier, source, is_tracked)
                 SELECT i, TIMESTAMP '2026-07-01', 1, t, 'archive', false
                 FROM (VALUES (1, 0), (2, 1), (3, 0), (4, 1)) v(i, t);
                 INSERT INTO run_player (instance_id, membership_id, character_id, started_at, activity_hash,
                     kills, deaths, assists, precision_kills, super_kills, melee_kills, grenade_kills, ability_kills,
                     completed, is_clear, time_played_s, has_build)
                 SELECT i, m, m, TIMESTAMP '2026-07-01', 1, k, d, 0, 0, 0, 0, 0, 0, true, true, 600, false
                 FROM (VALUES (1, 1, 100, 1), (2, 1, 80, 3), (3, 2, 200, 0), (4, 2, 180, 1)) v(i, m, k, d);",
            )
            .unwrap();
        let s = steps(&db, "Master", 1).unwrap();
        let names: Vec<&str> = s.iter().map(|s| s.step.as_str()).collect();
        assert_eq!(names, ["Master", "Ultimate"]);
        assert!((s[0].kill_rate - 1.0).abs() < 1e-12);
        let u = &s[1];
        assert_eq!((u.pairs, u.players), (2, 2));
        assert!((u.activity_level - 50.0).abs() < 1e-12);
        assert!((u.kill_rate - (0.8f64 * 0.9).sqrt()).abs() < 1e-9);
        assert!((u.deaths_10m - 1.5).abs() < 1e-9);
        assert!(steps(&db, "Master", 3).unwrap().is_empty());
    }
}
