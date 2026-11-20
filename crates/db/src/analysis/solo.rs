//! Soloing content made for a fireteam: how Searchlight captured it, what a
//! solo clear looks like against the fireteam clears of the same activity,
//! and what crediting it ([`SoloScoring::ActivityCredit`]) would do to the
//! build rankings.
//!
//! Every activity Searchlight collected is fireteam content (three- or
//! six-player). Searchlight doubled a solo clear's score ("soloing this
//! content is extraordinary"); ADR 0006 measures solo clears against other
//! solo clears instead, so a build that carries one player through a
//! fireteam's work earns nothing for it. Two things in how Searchlight
//! collected runs shape what the data can show: its qualification gates
//! (five Grandmaster Conquests qualified solo runs only), and its choice of
//! players to monitor — whose builds were captured — by an evaluator that
//! doubled solo scores.

use std::collections::HashMap;

use super::spearman;
use crate::filter::RunFilter;
use crate::rank::{
    BuildRank, BuildSort, RankOptions, SoloScoring, against_fireteam, rank_builds, solo_credit_sql,
    team_reference_sql,
};
use crate::{Database, Result};

/// How Searchlight qualified and captured one activity group's runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Qualification {
    pub group: String,
    /// Runs (attempts) recorded, and those that met the group's gates.
    pub runs: usize,
    pub qualified: usize,
    /// Solo share of qualified and of unqualified runs.
    pub solo_qualified: f64,
    pub solo_unqualified: f64,
    /// Share of qualified solo runs that started from the beginning.
    pub fresh_solo: f64,
    /// 99th percentile of entries in a report (players, with swaps).
    pub roster_p99: i64,
    /// Share of scored solo and fireteam clears with a captured build.
    pub build_solo: f64,
    pub build_fireteam: f64,
}

/// Qualification and build capture by activity group.
pub fn qualification(db: &Database) -> Result<Vec<Qualification>> {
    let mut stmt = db.conn().prepare(
        "WITH q AS (
             SELECT a.target_group AS grp, count(*) AS runs,
                    count(*) FILTER (WHERE r.qualified_target IS NOT NULL) AS qualified,
                    coalesce(avg(r.is_solo::INT) FILTER (WHERE r.qualified_target IS NOT NULL), 0) AS sq,
                    coalesce(avg(r.is_solo::INT) FILTER (WHERE r.qualified_target IS NULL), 0) AS su,
                    coalesce(avg(r.started_from_beginning::INT)
                        FILTER (WHERE r.qualified_target IS NOT NULL AND r.is_solo), 0) AS fresh,
                    quantile_disc(r.roster_size, 0.99) AS p99
             FROM run r JOIN activity a USING (activity_hash)
             WHERE a.target_group IS NOT NULL GROUP BY 1),
         b AS (
             SELECT a.target_group AS grp,
                    coalesce(avg(s.has_build::INT) FILTER (WHERE s.is_solo), 0) AS bs,
                    coalesce(avg(s.has_build::INT) FILTER (WHERE NOT s.is_solo), 0) AS bf
             FROM run_score s JOIN activity a USING (activity_hash) GROUP BY 1)
         SELECT q.grp, q.runs, q.qualified, q.sq, q.su, q.fresh, q.p99,
                coalesce(b.bs, 0), coalesce(b.bf, 0)
         FROM q LEFT JOIN b USING (grp) ORDER BY q.grp",
    )?;
    let rows = stmt.query_map([], |r| {
        let n = |i: usize| r.get::<_, i64>(i).map(|v| usize::try_from(v).unwrap_or(0));
        Ok(Qualification {
            group: r.get(0)?,
            runs: n(1)?,
            qualified: n(2)?,
            solo_qualified: r.get(3)?,
            solo_unqualified: r.get(4)?,
            fresh_solo: r.get(5)?,
            roster_p99: r.get::<_, Option<i64>>(6)?.unwrap_or(0),
            build_solo: r.get(7)?,
            build_fireteam: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

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

/// What fresh solo clears of one activity group look like against the
/// fireteam clears of the same activities (medians).
#[derive(Debug, Clone, PartialEq)]
pub struct Premium {
    pub group: String,
    /// Solo clears that started from the beginning, and all solo clears.
    pub solo_clears: usize,
    pub all_solo_clears: usize,
    /// The factors of `run_score`, against the fireteam reference.
    pub kill_index: f64,
    pub speed: f64,
    pub survival: f64,
    /// How much better a solo clear scores against the fireteam clears than
    /// against other solo clears: median and quartiles.
    pub credit: f64,
    pub credit_q1: f64,
    pub credit_q3: f64,
    /// Median duration in seconds.
    pub duration: f64,
}

/// Fresh solo clears against the fireteam reference, by activity group.
pub fn premium(db: &Database) -> Result<Vec<Premium>> {
    let (log_score, kill_index) = against_fireteam("r", "t");
    let p = focalpoint_core::rank::ScoreParams::default();
    let mut stmt = db.conn().prepare(&format!(
        "WITH s AS (
             SELECT coalesce(a.target_group, '(untargeted)') AS grp, run.started_from_beginning AS fresh,
                    r.duration_s,
                    {kill_index} AS kill_index,
                    least(greatest(t.duration_s / greatest(r.duration_s, 1), {s0}), {s1}) AS speed,
                    least(greatest((1 + {d} * t.deaths) / (1 + {d} * r.deaths), {v0}), {v1}) AS survival,
                    {log_score} - t.centre - r.perf AS log_credit
             FROM run_score r JOIN activity a USING (activity_hash)
             JOIN run USING (instance_id)
             JOIN ({team}) t ON t.activity_hash = r.activity_hash
             WHERE r.is_solo)
         SELECT grp, count(*) FILTER (WHERE fresh), count(*),
                median(kill_index) FILTER (WHERE fresh), median(speed) FILTER (WHERE fresh),
                median(survival) FILTER (WHERE fresh),
                quantile_cont(log_credit, [0.25, 0.5, 0.75]) FILTER (WHERE fresh),
                median(duration_s) FILTER (WHERE fresh)
         FROM s GROUP BY 1 ORDER BY 2 DESC, 1",
        team = team_reference_sql(),
        s0 = p.speed_bounds.0,
        s1 = p.speed_bounds.1,
        d = p.death_rate,
        v0 = p.survival_bounds.0,
        v1 = p.survival_bounds.1,
    ))?;
    let rows = stmt.query_map([], |r| {
        let q: Vec<f64> = match r.get::<_, duckdb::types::Value>(6)? {
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
        let n = |i: usize| r.get::<_, i64>(i).map(|v| usize::try_from(v).unwrap_or(0));
        Ok(Premium {
            group: r.get(0)?,
            solo_clears: n(1)?,
            all_solo_clears: n(2)?,
            kill_index: r.get::<_, Option<f64>>(3)?.unwrap_or(f64::NAN),
            speed: r.get::<_, Option<f64>>(4)?.unwrap_or(f64::NAN),
            survival: r.get::<_, Option<f64>>(5)?.unwrap_or(f64::NAN),
            credit_q1: at(0),
            credit: at(1),
            credit_q3: at(2),
            duration: r.get::<_, Option<f64>>(7)?.unwrap_or(f64::NAN),
        })
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

/// One activity's solo premium under [`SoloScoring::ActivityCredit`].
#[derive(Debug, Clone, PartialEq)]
pub struct Credit {
    pub group: String,
    pub activity: String,
    /// Solo clears that started from the beginning.
    pub solo_clears: usize,
    /// Where the fireteam reference comes from: qualified fireteam clears,
    /// or (when too few qualified) unqualified ones.
    pub from_qualified: bool,
    /// Median kill index of the solo clears against the fireteam reference.
    pub kill_index: f64,
    /// The credited premium (never below 1).
    pub credited: f64,
}

/// Every activity with a premium, most fresh solo clears first.
pub fn credits(db: &Database) -> Result<Vec<Credit>> {
    let mut stmt = db.conn().prepare(&format!(
        "SELECT coalesce(a.target_group, '(untargeted)'), a.name, c.solo_clears, t.from_qualified,
                c.kill_index, exp(c.log_credit)
         FROM ({credit}) c JOIN activity a USING (activity_hash)
         JOIN ({team}) t USING (activity_hash)
         ORDER BY c.solo_clears DESC, a.name",
        credit = solo_credit_sql(),
        team = team_reference_sql(),
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok(Credit {
            group: r.get(0)?,
            activity: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            solo_clears: usize::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
            from_qualified: r.get(3)?,
            kill_index: r.get(4)?,
            credited: r.get(5)?,
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
        assert!(qualification(&db).is_ok());
        assert!(volume(&db).is_ok());
        assert!(premium(&db).is_ok());
        assert!(credits(&db).is_ok());
    }
}
