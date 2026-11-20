//! Failed attempts in content that cannot be resumed.
//!
//! The ranking scores clears only (ADR 0006). Raids and dungeons resume from
//! checkpoints, so an uncleared run there is mostly a session of a longer
//! clear: short, and often abandoned in the first minutes. Conquests and Ops
//! cannot be resumed ([`CHECKPOINT_SHARE`]): an uncleared run there is a
//! failed attempt, and in Ultimate Conquest most attempts fail. A build's
//! failures there are unseen by the ranking.
//!
//! Comparing a player's attempts with each other is biased: players try
//! until they clear and then stop, so a clear is almost always a sequence's
//! last attempt and whatever build a player switched to takes the credit.
//! A player's **first attempt** at an activity is free of that — its build
//! was chosen before any feedback there. [`first_attempts`] measures each
//! build type's first-attempt clear rate against players of the same level
//! (deciles of their scored clears in other activity groups), and
//! [`agreement`] compares that with the build-type ranking of the group.

use std::collections::{BTreeMap, HashMap};

use crate::filter::RunFilter;
use crate::rank::{BuildRank, BuildSort, CHECKPOINT_SHARE, RankOptions, rank_builds};
use crate::search::GroupBy;
use crate::{Database, Result};

/// Recorded runs shorter than this (time played) are bail-outs, not tries.
pub const MIN_ATTEMPT_S: i64 = 120;

/// Scored clears in other activity groups a player needs to be given a
/// level; players with fewer form a level of their own.
pub const LEVEL_CLEARS: i64 = 3;

/// Split-half reliability counts build types with at least this many first
/// attempts in each half of the players.
pub const HALF_ATTEMPTS: usize = 30;

/// [`agreement`] compares build types with at least this many first
/// attempts and this many ranked players.
pub const AGREEMENT_ATTEMPTS: usize = 100;
pub const AGREEMENT_PLAYERS: usize = 5;

/// Every recorded run of a target activity by a player with a recorded
/// build: `attempt (instance_id, membership_id, activity_hash, grp,
/// resumable, clear, played, started_at)`.
fn attempt_ctes() -> String {
    format!(
        "WITH checkpoints AS (
             SELECT activity_hash,
                    avg((NOT coalesce(started_from_beginning, true))::INTEGER) >= {CHECKPOINT_SHARE}
                        AS resumable
             FROM run GROUP BY 1),
         attempt AS (
             SELECT p.instance_id, p.membership_id, r.activity_hash, a.target_group AS grp,
                    c.resumable, bool_or(p.is_clear) AS clear,
                    coalesce(sum(p.time_played_s), 0) AS played, any_value(r.started_at) AS started_at
             FROM run_player p JOIN run r USING (instance_id)
             JOIN activity a ON a.activity_hash = r.activity_hash
             JOIN checkpoints c ON c.activity_hash = r.activity_hash
             WHERE p.has_build AND a.target_group IS NOT NULL
             GROUP BY ALL),
         tries AS (
             SELECT *, row_number() OVER w AS k,
                    coalesce(sum(clear::INTEGER) OVER (w ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING), 0)
                        AS clears_before
             FROM attempt
             WHERE NOT resumable AND played >= {MIN_ATTEMPT_S}
             WINDOW w AS (PARTITION BY membership_id, activity_hash ORDER BY started_at, instance_id))"
    )
}

/// An activity group's recorded runs (by players with a recorded build).
#[derive(Debug, Clone, PartialEq)]
pub struct GroupAttempts {
    pub group: String,
    /// Most of its runs are of activities with checkpoints.
    pub resumable: bool,
    pub runs: usize,
    pub cleared: f64,
    /// Uncleared runs: median minutes played, and the share under
    /// [`MIN_ATTEMPT_S`].
    pub uncleared_minutes: f64,
    pub uncleared_short: f64,
    pub cleared_minutes: Option<f64>,
}

pub fn by_group(db: &Database) -> Result<Vec<GroupAttempts>> {
    let mut stmt = db.conn().prepare(&format!(
        "{}
         SELECT grp, avg(resumable::INTEGER) >= 0.5, count(*), avg(clear::DOUBLE),
                coalesce(median(played) FILTER (WHERE NOT clear), 0) / 60.0,
                coalesce(avg((played < {MIN_ATTEMPT_S})::DOUBLE) FILTER (WHERE NOT clear), 0),
                median(played) FILTER (WHERE clear) / 60.0
         FROM attempt GROUP BY grp ORDER BY count(*) DESC, grp",
        attempt_ctes()
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok(GroupAttempts {
            group: r.get(0)?,
            resumable: r.get(1)?,
            runs: usize::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
            cleared: r.get(3)?,
            uncleared_minutes: r.get(4)?,
            uncleared_short: r.get(5)?,
            cleared_minutes: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

/// A group's sequences of tries: per player and activity, the tries of at
/// least [`MIN_ATTEMPT_S`] in time order.
#[derive(Debug, Clone, PartialEq)]
pub struct Sequences {
    pub group: String,
    /// Player-activities.
    pub sequences: usize,
    pub mean_tries: f64,
    pub ever_cleared: f64,
    /// Of sequences with a clear, those whose last try is a clear: players
    /// stop when they clear.
    pub end_on_clear: f64,
    /// Clear rate of the 1st, 2nd, 3rd and 4th-or-later try, among tries up
    /// to a player's first clear.
    pub by_try: [Option<f64>; 4],
}

pub fn sequences(db: &Database) -> Result<Vec<Sequences>> {
    let mut stmt = db.conn().prepare(&format!(
        "{},
         seq AS (
             SELECT grp, count(*) AS n, sum(clear::INTEGER) AS clears, arg_max(clear, k) AS last_clear
             FROM tries GROUP BY grp, membership_id, activity_hash),
         hazard AS (
             SELECT grp,
                    avg(clear::DOUBLE) FILTER (WHERE k = 1) AS h1,
                    avg(clear::DOUBLE) FILTER (WHERE k = 2) AS h2,
                    avg(clear::DOUBLE) FILTER (WHERE k = 3) AS h3,
                    avg(clear::DOUBLE) FILTER (WHERE k >= 4) AS h4
             FROM tries WHERE clears_before = 0 GROUP BY grp)
         SELECT grp, count(*), avg(n), avg((clears > 0)::DOUBLE),
                coalesce(avg(last_clear::DOUBLE) FILTER (WHERE clears > 0), 0),
                any_value(h1), any_value(h2), any_value(h3), any_value(h4)
         FROM seq JOIN hazard USING (grp) GROUP BY grp ORDER BY count(*) DESC, grp",
        attempt_ctes()
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok(Sequences {
            group: r.get(0)?,
            sequences: usize::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
            mean_tries: r.get(2)?,
            ever_cleared: r.get(3)?,
            end_on_clear: r.get(4)?,
            by_try: [r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?],
        })
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

/// First tries: `graded (… attempt columns, decile, build_id)` with the
/// player's level decile (0 without a level), `expected (activity_hash,
/// decile, rate)` and `keys (build_id, type)`, the build type as the
/// ranking keys it ([`GroupBy::Core`], joined by U+001F).
fn first_ctes() -> String {
    let key: Vec<String> = GroupBy::Core
        .key()
        .iter()
        .map(|(_, e)| format!("coalesce({e}, '—')"))
        .collect();
    format!(
        "{},
         first AS (SELECT * FROM tries WHERE k = 1),
         level AS (
             SELECT f.instance_id, f.membership_id, avg(s.perf) AS level, count(s.perf) AS clears
             FROM first f LEFT JOIN (run_score s JOIN activity o ON o.activity_hash = s.activity_hash)
                 ON s.membership_id = f.membership_id AND o.target_group IS DISTINCT FROM f.grp
             GROUP BY ALL),
         graded AS (
             SELECT f.*, rb.build_id,
                    CASE WHEN l.clears >= {LEVEL_CLEARS}
                         THEN ntile(10) OVER (PARTITION BY f.grp, l.clears >= {LEVEL_CLEARS} ORDER BY l.level)
                         ELSE 0 END AS decile
             FROM first f JOIN level l USING (instance_id, membership_id)
             JOIN run_build rb USING (instance_id, membership_id)
             WHERE rb.is_primary),
         expected AS (SELECT activity_hash, decile, avg(clear::DOUBLE) AS rate FROM graded GROUP BY ALL),
         keys AS (
             SELECT b.build_id, concat_ws(chr(31), {}) AS type
             FROM build b WHERE b.build_id IN (SELECT build_id FROM graded))",
        attempt_ctes(),
        key.join(", ")
    )
}

/// One half of the players' first tries of a build type: summed clears and
/// summed excess.
#[derive(Debug, Clone, Copy, Default)]
struct Half {
    attempts: usize,
    clears: i64,
    excess: f64,
}

/// A group's first tries.
#[derive(Debug, Clone, PartialEq)]
pub struct FirstAttempts {
    pub group: String,
    pub attempts: usize,
    pub clear_rate: f64,
    /// Share of players with a level, and the clear rates of the top and
    /// bottom level deciles.
    pub with_level: f64,
    pub top_decile: Option<f64>,
    pub bottom_decile: Option<f64>,
    /// Build types with [`HALF_ATTEMPTS`] in both halves of the players,
    /// and the correlation of their excess between the halves: how much
    /// of a type's excess is the build type rather than chance.
    pub reliable_types: usize,
    pub split_half: f64,
}

/// A build type's first tries in a group.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAttempts {
    pub group: String,
    /// The ranking's key ([`GroupBy::Core`]).
    pub key: Vec<String>,
    pub attempts: usize,
    pub clear_rate: f64,
    /// Clear rate above the rate expected of the same players' level in the
    /// same activity.
    pub excess: f64,
}

pub fn first_attempts(db: &Database) -> Result<(Vec<FirstAttempts>, Vec<TypeAttempts>)> {
    let ctes = first_ctes();
    let mut stmt = db.conn().prepare(&format!(
        "{ctes}
         SELECT grp, count(*), avg(clear::DOUBLE), avg((decile > 0)::DOUBLE),
                avg(clear::DOUBLE) FILTER (WHERE decile = 10), avg(clear::DOUBLE) FILTER (WHERE decile = 1)
         FROM graded GROUP BY grp ORDER BY count(*) DESC, grp"
    ))?;
    let mut groups: Vec<FirstAttempts> = stmt
        .query_map([], |r| {
            Ok(FirstAttempts {
                group: r.get(0)?,
                attempts: usize::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
                clear_rate: r.get(2)?,
                with_level: r.get(3)?,
                top_decile: r.get(4)?,
                bottom_decile: r.get(5)?,
                reliable_types: 0,
                split_half: f64::NAN,
            })
        })?
        .collect::<duckdb::Result<_>>()?;

    // Per group, type and half of the players: attempts, clears, summed excess.
    let mut stmt = db.conn().prepare(&format!(
        "{ctes}
         SELECT g.grp, k.type, hash(g.membership_id) % 2 = 0, count(*), sum(g.clear::INTEGER),
                sum(g.clear::DOUBLE - e.rate)
         FROM graded g JOIN expected e USING (activity_hash, decile) JOIN keys k USING (build_id)
         GROUP BY ALL"
    ))?;
    let mut halves: BTreeMap<(String, String), [Half; 2]> = BTreeMap::new();
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let half = usize::from(!r.get::<_, bool>(2)?);
        let n = usize::try_from(r.get::<_, i64>(3)?).unwrap_or(0);
        halves.entry((r.get(0)?, r.get(1)?)).or_default()[half] = Half {
            attempts: n,
            clears: r.get(4)?,
            excess: r.get(5)?,
        };
    }

    let mut types = Vec::new();
    let mut split: HashMap<&str, (Vec<f64>, Vec<f64>)> = HashMap::new();
    for ((group, key), [a, b]) in &halves {
        let n = a.attempts + b.attempts;
        types.push(TypeAttempts {
            group: group.clone(),
            key: key.split('\u{1f}').map(String::from).collect(),
            attempts: n,
            clear_rate: (a.clears + b.clears) as f64 / n as f64,
            excess: (a.excess + b.excess) / n as f64,
        });
        if a.attempts >= HALF_ATTEMPTS && b.attempts >= HALF_ATTEMPTS {
            let s = split.entry(group).or_default();
            s.0.push(a.excess / a.attempts as f64);
            s.1.push(b.excess / b.attempts as f64);
        }
    }
    for g in &mut groups {
        if let Some((a, b)) = split.get(g.group.as_str()) {
            g.reliable_types = a.len();
            g.split_half = super::pearson(a, b);
        }
    }
    Ok((groups, types))
}

/// A group's build-type ranking against first-attempt clear rates.
#[derive(Debug, Clone)]
pub struct Agreement {
    pub group: String,
    /// Rank correlation of build effect and first-attempt excess over the
    /// types with [`AGREEMENT_ATTEMPTS`] first attempts and
    /// [`AGREEMENT_PLAYERS`] ranked players.
    pub spearman: f64,
    /// Those types, best ranked first.
    pub types: Vec<(BuildRank, TypeAttempts)>,
}

/// For each group with first attempts, rank its build types (clears only,
/// as the site does) and set them beside their first-attempt clear rates.
pub fn agreement(db: &Database, types: &[TypeAttempts]) -> Result<Vec<Agreement>> {
    let mut by_group: BTreeMap<&str, HashMap<&[String], &TypeAttempts>> = BTreeMap::new();
    for t in types.iter().filter(|t| t.attempts >= AGREEMENT_ATTEMPTS) {
        by_group
            .entry(t.group.as_str())
            .or_default()
            .insert(&t.key, t);
    }
    let mut out = Vec::new();
    for (group, attempts) in by_group {
        let opts = RankOptions {
            runs: RunFilter {
                target_group: Some(group.to_string()),
                ..RunFilter::default()
            },
            group_by: GroupBy::Core,
            ..RankOptions::default()
        };
        let (_, _, ranked) = rank_builds(db, &opts, AGREEMENT_PLAYERS, BuildSort::Effect)?;
        let matched: Vec<(BuildRank, TypeAttempts)> = ranked
            .into_iter()
            .filter_map(|r| {
                let t = (*attempts.get(r.key.as_slice())?).clone();
                Some((r, t))
            })
            .collect();
        let effects: Vec<f64> = matched.iter().map(|(r, _)| r.effect).collect();
        let excess: Vec<f64> = matched.iter().map(|(_, t)| t.excess).collect();
        out.push(Agreement {
            group: group.to_string(),
            spearman: if matched.len() >= 3 {
                super::spearman(&effects, &excess)
            } else {
                f64::NAN
            },
            types: matched,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ultimate Conquest (activity 100, never resumed) and a raid (200,
    /// resumed). Build 1 is the Alpha type, build 2 Beta.
    ///
    /// * player 1: Alpha fails on days 1 and 2, Beta clears on day 3;
    /// * player 2: Alpha clears on day 1;
    /// * player 3: Beta bails out after a minute on day 1, then fails on day 2;
    /// * player 1 also leaves a resumed raid run after 90 s.
    fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(&dir.path().join("a.duckdb")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO item (item_hash, name, is_exotic) VALUES
                    (1, 'Alpha Boots', true), (2, 'Beta Boots', true), (31, 'Glacial Quake', false);
                 INSERT INTO activity (activity_hash, name, target_group) VALUES
                    (100, 'Ultimate Conquest: Lightblade: Customize', 'Ultimate Conquest'),
                    (200, 'Raid: Master', 'Master Raid');
                 INSERT INTO build (build_id, class, element, exotic_armor_hash, super_hash, aspect_hashes,
                     fragment_hashes, artifact_perk_hashes, armor_mod_hashes, item_hashes, component_count,
                     unresolved_count, tier1_elements, tier2_elements, tier3_elements, tier4_elements)
                 VALUES (1, 'Titan', 'Stasis', 1, 31, [], [], [], [], [], 0, 0, [], [], [], []),
                        (2, 'Titan', 'Stasis', 2, 31, [], [], [], [], [], 0, 0, [], [], [], []);
                 INSERT INTO run (instance_id, started_at, activity_hash, started_from_beginning, source, is_tracked)
                 SELECT i, TIMESTAMP '2026-07-01' + INTERVAL (d) DAY, a, a = 100, 'archive', false
                 FROM (VALUES (11, 1, 100), (12, 2, 100), (13, 3, 100), (21, 1, 100), (31, 1, 100), (32, 2, 100),
                              (41, 1, 200)) v(i, d, a);
                 INSERT INTO run_player (instance_id, membership_id, character_id, started_at, activity_hash,
                     kills, deaths, assists, precision_kills, super_kills, melee_kills, grenade_kills, ability_kills,
                     completed, is_clear, time_played_s, has_build)
                 SELECT i, m, m, r.started_at, r.activity_hash, 0, 0, 0, 0, 0, 0, 0, 0, c, c, s, true
                 FROM (VALUES (11, 1, false, 600), (12, 1, false, 600), (13, 1, true, 1200), (21, 2, true, 1200),
                              (31, 3, false, 60), (32, 3, false, 600), (41, 1, false, 90)) v(i, m, c, s)
                 JOIN run r ON r.instance_id = v.i;
                 INSERT INTO run_build VALUES
                    (11, 1, 1, 1, 0, 600, 1.0, true), (12, 1, 1, 1, 0, 600, 1.0, true),
                    (13, 1, 2, 1, 0, 1200, 1.0, true), (21, 2, 1, 1, 0, 1200, 1.0, true),
                    (31, 3, 2, 1, 0, 60, 1.0, true), (32, 3, 2, 1, 0, 600, 1.0, true),
                    (41, 1, 1, 1, 0, 90, 1.0, true);",
            )
            .unwrap();
        (dir, db)
    }

    #[test]
    fn uncleared_runs_are_failures_only_where_runs_cannot_be_resumed() {
        let (_dir, db) = db();
        let groups = by_group(&db).unwrap();
        let uc = &groups[0];
        assert_eq!(
            (uc.group.as_str(), uc.resumable, uc.runs),
            ("Ultimate Conquest", false, 6)
        );
        assert!((uc.cleared - 2.0 / 6.0).abs() < 1e-9);
        assert!(
            (uc.uncleared_short - 0.25).abs() < 1e-9,
            "one bail-out of four"
        );
        let raid = &groups[1];
        assert!(raid.resumable && raid.cleared_minutes.is_none());

        let seqs = sequences(&db).unwrap();
        assert_eq!(seqs.len(), 1, "only content that cannot be resumed");
        let s = &seqs[0];
        assert_eq!(s.sequences, 3);
        assert!(
            (s.mean_tries - 5.0 / 3.0).abs() < 1e-9,
            "the bail-out is not a try"
        );
        assert!((s.ever_cleared - 2.0 / 3.0).abs() < 1e-9);
        assert!((s.end_on_clear - 1.0).abs() < 1e-9);
        let [h1, h2, h3, h4] = s.by_try;
        assert!((h1.unwrap() - 1.0 / 3.0).abs() < 1e-9);
        assert!(h2.unwrap().abs() < 1e-9 && (h3.unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(h4, None, "nobody made a fourth try");
    }

    #[test]
    fn first_tries_are_measured_against_the_expected_rate() {
        let (_dir, db) = db();
        let (groups, types) = first_attempts(&db).unwrap();
        assert_eq!((groups[0].attempts, groups[0].with_level), (3, 0.0));
        assert!((groups[0].clear_rate - 1.0 / 3.0).abs() < 1e-9);
        let alpha = types.iter().find(|t| t.key[2] == "Alpha Boots").unwrap();
        assert_eq!(
            alpha.key,
            ["Titan", "Stasis", "Alpha Boots", "—", "Glacial Quake", "—"]
        );
        assert_eq!(
            alpha.attempts, 2,
            "players 1 and 2; player 1's Beta clear was not a first try"
        );
        assert!((alpha.excess - 1.0 / 6.0).abs() < 1e-9);
        let beta = types.iter().find(|t| t.key[2] == "Beta Boots").unwrap();
        assert!((beta.excess + 1.0 / 3.0).abs() < 1e-9);
    }
}
