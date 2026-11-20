//! Armor set bonuses: how often recorded builds run without full set
//! bonuses, what full bonuses are worth to the same player, whether the
//! players who skip them are stronger, and how often a listing's top build
//! lacks them (ADR 0010).
//!
//! A build is **polished** with at least [`POLISHED_SET_BONUSES`] active set
//! bonuses — a 4-piece set or two 2-piece sets — and **lesser** otherwise.
//! Armor is always captured whole, so a lesser build really ran without
//! them. Each scored clear counts once, by its main build (the one used
//! longest).

use std::collections::HashSet;
use std::hash::BuildHasher;

use crate::site::{CORE_PLAYERS, POLISHED_SET_BONUSES, SiteData};
use crate::{Database, Result};

/// Recorded scored clears' main builds with their set bonuses: the CTEs
/// `bonus (build_id, bonuses)` and `main (membership_id, activity_hash,
/// perf, started_at, bonuses)`.
const MAIN_BUILDS: &str = "
    WITH bonus AS (
        SELECT build_id, count(*) FILTER (WHERE kind = 'set_bonus') AS bonuses
        FROM build_component GROUP BY build_id),
    main AS (
        SELECT r.membership_id, r.activity_hash, r.perf, r.started_at,
               coalesce(b.bonuses, 0) AS bonuses
        FROM run_score r JOIN run_build rb USING (instance_id, membership_id)
        LEFT JOIN bonus b ON b.build_id = rb.build_id
        WHERE r.has_build AND rb.is_primary)";

/// Recorded scored clears by their main build's active set bonuses.
#[derive(Debug, Clone, PartialEq)]
pub struct Prevalence {
    /// Active set bonuses, the last row standing for this many or more.
    pub bonuses: usize,
    pub clears: usize,
    pub share: f64,
    /// Mean `perf`: the clears' scores, players' skill included.
    pub mean_perf: f64,
}

/// Recorded scored clears by set bonuses (0, 1, 2 or more).
pub fn prevalence(db: &Database) -> Result<Vec<Prevalence>> {
    let mut stmt = db.conn().prepare(&format!(
        "{MAIN_BUILDS}
         SELECT least(bonuses, {POLISHED_SET_BONUSES}), count(*), avg(perf)
         FROM main GROUP BY 1 ORDER BY 1"
    ))?;
    let rows: Vec<(i64, i64, f64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<duckdb::Result<_>>()?;
    let total: i64 = rows.iter().map(|r| r.1).sum();
    Ok(rows
        .into_iter()
        .map(|(bonuses, clears, mean_perf)| Prevalence {
            bonuses: usize::try_from(bonuses).unwrap_or(0),
            clears: usize::try_from(clears).unwrap_or(0),
            share: clears as f64 / total.max(1) as f64,
            mean_perf,
        })
        .collect())
}

/// What full set bonuses are worth to the same player in the same
/// activity: per player and activity with both lesser and polished clears,
/// the difference of their mean `perf` (polished minus lesser).
#[derive(Debug, Clone, PartialEq)]
pub struct Gain {
    /// Player-activities with clears of both kinds.
    pub pairs: usize,
    pub mean: f64,
    /// Standard error of the mean.
    pub se: f64,
    pub median: f64,
    /// How many days later the polished clears were, on average, and how
    /// the gain correlates with that: learning over time would show as a
    /// positive correlation.
    pub later_days: f64,
    pub lateness_correlation: f64,
}

pub fn gain(db: &Database) -> Result<Gain> {
    let sql = format!(
        "{MAIN_BUILDS},
         cell AS (
             SELECT membership_id, activity_hash, bonuses >= {POLISHED_SET_BONUSES} AS polished,
                    avg(perf) AS perf, avg(epoch(started_at)) / 86400 AS day
             FROM main GROUP BY ALL),
         pairs AS (
             SELECT p.perf - l.perf AS gain, p.day - l.day AS later
             FROM cell l JOIN cell p USING (membership_id, activity_hash)
             WHERE NOT l.polished AND p.polished)
         SELECT count(*), avg(gain), coalesce(stddev(gain) / sqrt(count(*)), 0), median(gain),
                avg(later), coalesce(corr(gain, later), 0)
         FROM pairs"
    );
    Ok(db.conn().query_row(&sql, [], |r| {
        Ok(Gain {
            pairs: usize::try_from(r.get::<_, i64>(0)?).unwrap_or(0),
            mean: r.get::<_, Option<f64>>(1)?.unwrap_or(f64::NAN),
            se: r.get(2)?,
            median: r.get::<_, Option<f64>>(3)?.unwrap_or(f64::NAN),
            later_days: r.get::<_, Option<f64>>(4)?.unwrap_or(f64::NAN),
            lateness_correlation: r.get(5)?,
        })
    })?)
}

/// How often a player's recorded clears ran lesser builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Habit {
    Never,
    UnderHalf,
    OverHalf,
    Always,
}

impl Habit {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Habit::Never => "never",
            Habit::UnderHalf => "under half",
            Habit::OverHalf => "half or more",
            Habit::Always => "always",
        }
    }
}

/// Players by how many recorded clears they have and how often those ran
/// lesser builds, with the mean `perf` of all their scored clears
/// (recorded or not) as their level.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerGroup {
    /// The fewest recorded clears in the band (1, 2–4, 5–9, 10+).
    pub recorded_from: usize,
    pub habit: Habit,
    pub players: usize,
    pub mean_perf: f64,
    pub se: f64,
}

pub fn players(db: &Database) -> Result<Vec<PlayerGroup>> {
    let mut stmt = db.conn().prepare(&format!(
        "{MAIN_BUILDS},
         per AS (
             SELECT membership_id, avg((bonuses < {POLISHED_SET_BONUSES})::DOUBLE) AS lesser,
                    count(*) AS n
             FROM main GROUP BY 1),
         level AS (SELECT membership_id, avg(perf) AS perf FROM run_score GROUP BY 1)
         SELECT CASE WHEN n = 1 THEN 1 WHEN n <= 4 THEN 2 WHEN n <= 9 THEN 5 ELSE 10 END,
                CASE WHEN lesser = 0 THEN 0 WHEN lesser < 0.5 THEN 1 WHEN lesser < 1 THEN 2 ELSE 3 END,
                count(*), avg(perf), coalesce(stddev(perf) / sqrt(count(*)), 0)
         FROM per JOIN level USING (membership_id)
         GROUP BY ALL ORDER BY 1, 2"
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok(PlayerGroup {
            recorded_from: usize::try_from(r.get::<_, i64>(0)?).unwrap_or(0),
            habit: match r.get::<_, i64>(1)? {
                0 => Habit::Never,
                1 => Habit::UnderHalf,
                2 => Habit::OverHalf,
                _ => Habit::Always,
            },
            players: usize::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
            mean_perf: r.get(3)?,
            se: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

/// A scope's listings and how many of their top builds are lesser: under
/// the polished rule (`crate::site`), those are the build types with no
/// polished build in the scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopBuilds {
    pub scope: String,
    /// Listings with at least [`CORE_PLAYERS`] players, or fewer.
    pub core: bool,
    pub listings: usize,
    pub lesser: usize,
    /// Of those, in the scope's top 10% of listings.
    pub lesser_in_top_tenth: usize,
}

#[must_use]
pub fn top_builds<S: BuildHasher>(data: &SiteData, polished: &HashSet<i64, S>) -> Vec<TopBuilds> {
    let mut out = Vec::new();
    for scope in &data.scopes {
        for core in [true, false] {
            let rows: Vec<_> = scope
                .rows
                .iter()
                .filter(|r| (r.4 >= CORE_PLAYERS) == core)
                .collect();
            let lesser: Vec<bool> = rows
                .iter()
                .map(|r| {
                    let id = &data.recipes[r.7.0 as usize].id;
                    !id.parse::<i64>().is_ok_and(|id| polished.contains(&id))
                })
                .collect();
            out.push(TopBuilds {
                scope: scope.id.clone(),
                core,
                listings: rows.len(),
                lesser: lesser.iter().filter(|&&l| l).count(),
                lesser_in_top_tenth: lesser[..rows.len() / 10].iter().filter(|&&l| l).count(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Player 1 clears activity 100 twice with a lesser build (perf 0.0,
    /// 0.2) and twice with a polished one (0.4, 0.6); player 2 only with
    /// the lesser build.
    fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(&dir.path().join("s.duckdb")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO build_component (build_id, kind, slot, component_hash, parent_hash, quantity, position)
                 VALUES (1, 'set_bonus', NULL, 901, 900, 1, 0),
                        (2, 'set_bonus', NULL, 901, 900, 1, 0), (2, 'set_bonus', NULL, 902, 900, 1, 1);
                 INSERT INTO run_score (instance_id, membership_id, activity_hash, started_at, is_solo, is_partial,
                     has_build, kills, deaths, duration_s, played_s, kill_index, speed, survival, score, perf)
                 SELECT i, m, 100, TIMESTAMP '2026-07-01' + INTERVAL (i % 10) DAY, false, false, true,
                        100, 0, 600, 600, 1, 1, 1, 1, p
                 FROM (VALUES (1, 1, 0.0), (2, 1, 0.2), (3, 1, 0.4), (4, 1, 0.6), (11, 2, -0.2)) v(i, m, p);
                 INSERT INTO run_build VALUES
                    (1, 1, 1, 1, 0, 600, 1.0, true), (2, 1, 1, 1, 0, 600, 1.0, true),
                    (3, 1, 2, 1, 0, 600, 1.0, true), (4, 1, 2, 1, 0, 600, 1.0, true),
                    (11, 2, 1, 1, 0, 600, 1.0, true);",
            )
            .unwrap();
        (dir, db)
    }

    #[test]
    fn full_set_bonuses_are_compared_within_player() {
        let (_dir, db) = db();
        let prevalence = prevalence(&db).unwrap();
        let counts: Vec<(usize, usize)> =
            prevalence.iter().map(|p| (p.bonuses, p.clears)).collect();
        assert_eq!(counts, [(1, 3), (2, 2)]);
        let gain = gain(&db).unwrap();
        assert_eq!(gain.pairs, 1, "only player 1 has both kinds");
        assert!((gain.mean - 0.4).abs() < 1e-9, "{gain:?}");
        assert!(gain.later_days > 0.0);

        let groups = players(&db).unwrap();
        let habits: Vec<(usize, Habit, usize)> = groups
            .iter()
            .map(|g| (g.recorded_from, g.habit, g.players))
            .collect();
        assert_eq!(habits, [(1, Habit::Always, 1), (2, Habit::OverHalf, 1)]);
        assert!((groups[1].mean_perf - 0.3).abs() < 1e-9);
    }
}
