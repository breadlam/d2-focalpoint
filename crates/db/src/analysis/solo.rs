//! Soloing content made for a fireteam: how much of it was captured, what a
//! solo clear looks like against the fireteam clears of the same activity,
//! and what crediting it ([`SoloScoring::AgainstFireteam`]) would do to the
//! build rankings.
//!
//! Searchlight doubled a solo clear's score ("soloing this content is
//! extraordinary"); ADR 0006 measures solo clears against other solo clears
//! instead, so a build that carries one player through a fireteam's work
//! earns nothing for it.

use std::collections::HashMap;

use super::spearman;
use crate::filter::RunFilter;
use crate::rank::{
    BuildRank, BuildSort, RankOptions, SoloScoring, against_fireteam, rank_builds, solo_credit_sql,
};
use crate::{Database, Result};

/// Qualified clears of one activity group.
#[derive(Debug, Clone, PartialEq)]
pub struct Volume {
    pub group: String,
    pub clears: usize,
    pub solo_clears: usize,
    pub solo_players: usize,
    /// Solo clears with a captured build.
    pub solo_build_runs: usize,
}

/// Scored clears by activity group, most solo clears first.
pub fn volume(db: &Database) -> Result<Vec<Volume>> {
    let mut stmt = db.conn().prepare(
        "SELECT coalesce(a.target_group, '(untargeted)'), count(*),
                count(*) FILTER (WHERE r.is_solo),
                count(DISTINCT r.membership_id) FILTER (WHERE r.is_solo),
                count(*) FILTER (WHERE r.is_solo AND r.has_build)
         FROM run_score r JOIN activity a USING (activity_hash)
         GROUP BY 1 ORDER BY 3 DESC, 1",
    )?;
    let rows = stmt.query_map([], |r| {
        let n = |i: usize| r.get::<_, i64>(i).map(|v| usize::try_from(v).unwrap_or(0));
        Ok(Volume {
            group: r.get(0)?,
            clears: n(1)?,
            solo_clears: n(2)?,
            solo_players: n(3)?,
            solo_build_runs: n(4)?,
        })
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

/// What solo clears of one activity group look like against the fireteam
/// clears of the same activities (medians over solo clears).
#[derive(Debug, Clone, PartialEq)]
pub struct Premium {
    pub group: String,
    pub solo_clears: usize,
    /// The factors of `run_score`, against the fireteam reference.
    pub kill_index: f64,
    pub speed: f64,
    pub survival: f64,
    /// The credit a solo clear would earn: `exp` of the median difference
    /// between its fireteam-referenced and solo-referenced performance.
    pub credit: f64,
    /// … its quartiles.
    pub credit_q1: f64,
    pub credit_q3: f64,
    /// Share of solo clears at the kill index's upper bound.
    pub at_kill_cap: f64,
}

/// Solo clears against the fireteam reference, by activity group.
pub fn premium(db: &Database) -> Result<Vec<Premium>> {
    let (log_score, kill_index) = against_fireteam("r", "t");
    let p = focalpoint_core::rank::ScoreParams::default();
    let mut stmt = db.conn().prepare(&format!(
        "WITH s AS (
             SELECT coalesce(a.target_group, '(untargeted)') AS grp,
                    {kill_index} AS kill_index,
                    least(greatest(t.duration_s / greatest(r.duration_s, 1), {s0}), {s1}) AS speed,
                    least(greatest((1 + {d} * t.deaths) / (1 + {d} * r.deaths), {v0}), {v1}) AS survival,
                    {log_score} - t.log_score_mean - r.perf AS log_credit
             FROM run_score r JOIN activity a USING (activity_hash)
             JOIN activity_reference t
                  ON t.activity_hash = r.activity_hash AND NOT t.is_solo AND t.is_scored
             WHERE r.is_solo AND t.kills > 0 AND t.log_score_mean IS NOT NULL)
         SELECT grp, count(*), median(kill_index), median(speed), median(survival),
                quantile_cont(log_credit, [0.25, 0.5, 0.75]),
                avg((kill_index >= {k1})::INTEGER)
         FROM s GROUP BY 1 ORDER BY 2 DESC, 1",
        k1 = p.kill_bounds.1,
        s0 = p.speed_bounds.0,
        s1 = p.speed_bounds.1,
        d = p.death_rate,
        v0 = p.survival_bounds.0,
        v1 = p.survival_bounds.1,
    ))?;
    let rows = stmt.query_map([], |r| {
        let q: Vec<f64> = match r.get::<_, duckdb::types::Value>(5)? {
            duckdb::types::Value::List(v) => v
                .into_iter()
                .filter_map(|x| match x {
                    duckdb::types::Value::Double(f) => Some(f),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let at = |i: usize| q.get(i).copied().map_or(f64::NAN, f64::exp);
        Ok(Premium {
            group: r.get(0)?,
            solo_clears: usize::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
            kill_index: r.get(2)?,
            speed: r.get(3)?,
            survival: r.get(4)?,
            credit_q1: at(0),
            credit: at(1),
            credit_q3: at(2),
            at_kill_cap: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

/// One activity's solo premium under [`SoloScoring::ActivityCredit`].
#[derive(Debug, Clone, PartialEq)]
pub struct Credit {
    pub group: String,
    pub activity: String,
    pub solo_clears: usize,
    pub fireteam_clears: usize,
    /// The fireteam reference is borrowed (too few fireteam clears).
    pub borrowed: bool,
    /// Median kill index of the solo clears against the fireteam reference.
    pub kill_index: f64,
    /// The median premium as measured, and as credited (1 when the activity
    /// does not qualify).
    pub measured: f64,
    pub credited: f64,
}

/// Every scored activity with solo clears, most solo clears first.
pub fn credits(db: &Database) -> Result<Vec<Credit>> {
    let (log_score, kill_index) = against_fireteam("s", "t");
    let mut stmt = db.conn().prepare(&format!(
        "WITH m AS (
             SELECT s.activity_hash, count(*) AS solo_clears, median({kill_index}) AS kill_index,
                    median({log_score} - t.log_score_mean - s.perf) AS log_credit,
                    any_value(t.clears) AS fireteam_clears, any_value(t.is_borrowed) AS borrowed
             FROM run_score s
             JOIN activity_reference t
                  ON t.activity_hash = s.activity_hash AND NOT t.is_solo AND t.is_scored
             WHERE s.is_solo AND t.kills > 0 AND t.log_score_mean IS NOT NULL
             GROUP BY 1)
         SELECT coalesce(a.target_group, '(untargeted)'), a.name, m.solo_clears, m.fireteam_clears,
                m.borrowed, m.kill_index, exp(m.log_credit), exp(coalesce(c.log_credit, 0))
         FROM m JOIN activity a USING (activity_hash)
         LEFT JOIN ({credit}) c USING (activity_hash)
         ORDER BY m.solo_clears DESC, a.name",
        credit = solo_credit_sql(),
    ))?;
    let rows = stmt.query_map([], |r| {
        let n = |i: usize| r.get::<_, i64>(i).map(|v| usize::try_from(v).unwrap_or(0));
        Ok(Credit {
            group: r.get(0)?,
            activity: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            solo_clears: n(2)?,
            fireteam_clears: n(3)?,
            borrowed: r.get(4)?,
            kill_index: r.get(5)?,
            measured: r.get(6)?,
            credited: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

/// One build type under both scorings.
#[derive(Debug, Clone, PartialEq)]
pub struct Shift {
    /// The core key: class, element, exotic armor, exotic weapon, super, aspects.
    pub key: Vec<String>,
    /// Effect with solo clears against solo clears (now), and against
    /// fireteam clears.
    pub effect: f64,
    pub credited: f64,
    pub runs: f64,
    pub players: usize,
    /// Solo clears with the build type, and players who soloed with it.
    pub solo_runs: f64,
    pub solo_players: usize,
}

/// Every build type of the all-activities ranking now and under `scoring`.
pub fn shifts(db: &Database, scoring: SoloScoring) -> Result<Vec<Shift>> {
    let rank = |solo: SoloScoring, runs: RunFilter| -> Result<Vec<BuildRank>> {
        let o = RankOptions {
            runs,
            solo,
            ..RankOptions::default()
        };
        Ok(rank_builds(db, &o, 1, BuildSort::Effect)?.2)
    };
    let now = rank(SoloScoring::AgainstSolo, RunFilter::default())?;
    let credited: HashMap<Vec<String>, f64> = rank(scoring, RunFilter::default())?
        .into_iter()
        .map(|b| (b.key, b.effect))
        .collect();
    let solo: HashMap<Vec<String>, (f64, usize)> = rank(
        SoloScoring::AgainstSolo,
        RunFilter {
            solo: true,
            ..RunFilter::default()
        },
    )?
    .into_iter()
    .map(|b| (b.key, (b.runs, b.players)))
    .collect();
    Ok(now
        .into_iter()
        .filter_map(|b| {
            let credited = *credited.get(&b.key)?;
            let (solo_runs, solo_players) = solo.get(&b.key).copied().unwrap_or((0.0, 0));
            Some(Shift {
                effect: b.effect,
                credited,
                runs: b.runs,
                players: b.players,
                solo_runs,
                solo_players,
                key: b.key,
            })
        })
        .collect())
}

/// How one class's ranking changes when solo clears are credited.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassShift {
    pub class: String,
    /// Build types with at least the minimum of players, best-credited first.
    pub types: Vec<Shift>,
    /// Their rank (1 = best) before crediting, parallel to `types`.
    pub rank_before: Vec<usize>,
    /// Rank correlation of the two effects.
    pub correlation: f64,
    /// Build types in both top 10% lists, and the size of the list.
    pub top_overlap: (usize, usize),
    /// Share of the credited top 10% with a solo clear, and of the rest.
    pub top_with_solo: f64,
    pub rest_with_solo: f64,
}

/// The shift of each class's build types with at least `min_players`.
#[must_use]
pub fn by_class(shifts: &[Shift], min_players: usize) -> Vec<ClassShift> {
    let mut classes: Vec<String> = shifts.iter().map(|s| s.key[0].clone()).collect();
    classes.sort();
    classes.dedup();
    classes
        .into_iter()
        .filter(|c| c != "—")
        .map(|class| {
            let mut types: Vec<Shift> = shifts
                .iter()
                .filter(|s| s.key[0] == class && s.players >= min_players)
                .cloned()
                .collect();
            let mut before: Vec<usize> = (0..types.len()).collect();
            before.sort_by(|&a, &b| types[b].effect.total_cmp(&types[a].effect));
            let mut rank_of = vec![0; types.len()];
            for (r, &i) in before.iter().enumerate() {
                rank_of[i] = r + 1;
            }
            let mut order: Vec<usize> = (0..types.len()).collect();
            order.sort_by(|&a, &b| types[b].credited.total_cmp(&types[a].credited));
            let rank_before: Vec<usize> = order.iter().map(|&i| rank_of[i]).collect();
            types = order.iter().map(|&i| types[i].clone()).collect();
            let n = types.len();
            let decile = n.div_ceil(10);
            let effect: Vec<f64> = types.iter().map(|t| t.effect).collect();
            let credited: Vec<f64> = types.iter().map(|t| t.credited).collect();
            let share = |range: std::ops::Range<usize>| {
                let k = range.len().max(1) as f64;
                range.filter(|&i| types[i].solo_runs > 0.0).count() as f64 / k
            };
            ClassShift {
                correlation: spearman(&effect, &credited),
                top_overlap: (
                    rank_before[..decile.min(n)]
                        .iter()
                        .filter(|&&r| r <= decile)
                        .count(),
                    decile,
                ),
                top_with_solo: share(0..decile.min(n)),
                rest_with_solo: share(decile.min(n)..n),
                class,
                types,
                rank_before,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shift(class: &str, effect: f64, credited: f64, solo_runs: f64) -> Shift {
        Shift {
            key: vec![class.into()],
            effect,
            credited,
            runs: 10.0,
            players: 5,
            solo_runs,
            solo_players: usize::from(solo_runs > 0.0),
        }
    }

    #[test]
    fn credited_rankings_are_compared_per_class() {
        let shifts = [
            shift("Titan", 0.3, 0.2, 0.0),
            shift("Titan", 0.1, 0.4, 5.0),
            shift("Titan", 0.0, 0.0, 0.0),
            shift("Hunter", 0.2, 0.2, 0.0),
        ];
        let by = by_class(&shifts, 3);
        let titan = by.iter().find(|c| c.class == "Titan").unwrap();
        // The soloed build type rises from second to first.
        assert_eq!(titan.rank_before, [2, 1, 3]);
        assert!((titan.types[0].credited - 0.4).abs() < 1e-12);
        assert_eq!(titan.top_overlap, (0, 1));
        assert!((titan.top_with_solo - 1.0).abs() < 1e-12);
    }

    #[test]
    fn solo_volume_and_premium_read_the_database() {
        let (_d, db) = crate::site::tests::db();
        assert!(volume(&db).is_ok());
        assert!(premium(&db).is_ok());
        assert!(credits(&db).is_ok());
    }
}
