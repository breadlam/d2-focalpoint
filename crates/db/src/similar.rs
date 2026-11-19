//! Nearest builds by the tiered build distance (ADR 0005).

use std::fmt::Write as _;

use duckdb::types::Value;
use duckdb::{OptionalExt, params};
use focalpoint_core::vector::{ElementVector, TierWeights, TieredDistance};
use rayon::prelude::*;

use crate::{Database, DbError, Result};

/// Options for [`nearest_builds`].
#[derive(Debug, Clone)]
pub struct SimilarOptions {
    pub limit: usize,
    /// Only compare against builds of the same class (default).
    pub same_class: bool,
    /// Include builds only ever used by players who never got a kill with
    /// them (checkpoint bots and passengers; excluded by default).
    pub include_passengers: bool,
    /// Skip builds used by any player of the target build, to find who
    /// *else* plays something similar.
    pub other_players_only: bool,
    pub weights: TierWeights,
}

impl Default for SimilarOptions {
    fn default() -> Self {
        Self {
            limit: 10,
            same_class: true,
            include_passengers: false,
            other_players_only: false,
            weights: TierWeights::default(),
        }
    }
}

/// A human-readable summary of a build.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildSummary {
    pub build_id: i64,
    pub class: Option<String>,
    pub element: Option<String>,
    pub exotic_armor: Option<String>,
    pub exotic_weapon: Option<String>,
    pub super_name: Option<String>,
    pub aspects: Vec<String>,
    /// Investment level per stat (Weapons, Health, Class, Grenade, Super, Melee).
    pub stat_levels: Vec<u8>,
    pub stats_promoted: Vec<String>,
    pub timeline_count: i64,
    pub player_count: i64,
}

/// One neighbour of the target build.
#[derive(Debug, Clone, PartialEq)]
pub struct Neighbour {
    pub summary: BuildSummary,
    pub distance: TieredDistance,
    pub slot_edits: u32,
}

/// The target build and its nearest neighbours, closest first.
#[derive(Debug, Clone, PartialEq)]
pub struct Similar {
    pub target: BuildSummary,
    pub neighbours: Vec<Neighbour>,
    /// How many candidate builds were compared.
    pub candidates: usize,
}

fn ints(v: Value) -> Vec<i64> {
    match v {
        Value::List(items) => items
            .into_iter()
            .filter_map(|x| match x {
                Value::BigInt(n) => Some(n),
                Value::Int(n) => Some(i64::from(n)),
                Value::UTinyInt(n) => Some(i64::from(n)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn texts(v: Value) -> Vec<String> {
    match v {
        Value::List(items) => items
            .into_iter()
            .filter_map(|x| match x {
                Value::Text(s) => Some(s),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

const VECTOR_SQL: &str =
    "list_concat(tier1_elements, tier2_elements, tier3_elements, tier4_elements)";

/// Rank every candidate build by its distance to `build_id`.
pub fn nearest_builds(db: &Database, build_id: i64, opts: &SimilarOptions) -> Result<Similar> {
    let conn = db.conn();
    let (class, target_keys): (Option<String>, Value) = conn
        .query_row(
            &format!("SELECT class, {VECTOR_SQL} FROM build WHERE build_id = ?"),
            params![build_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(DbError::BuildNotFound(build_id))?;
    let target = ElementVector::from_keys(ints(target_keys));

    let mut sql = format!("SELECT build_id, {VECTOR_SQL} FROM build b WHERE build_id <> ?");
    if opts.same_class {
        sql.push_str(" AND class IS NOT DISTINCT FROM ?");
    }
    if !opts.include_passengers {
        sql.push_str(
            " AND build_id IN (SELECT DISTINCT rb.build_id FROM run_build rb
                 JOIN run_player p ON p.instance_id = rb.instance_id AND p.membership_id = rb.membership_id
                 WHERE p.has_build AND p.kills > 0)",
        );
    }
    if opts.other_players_only {
        // The target id is a trusted integer, inlined to keep binding simple.
        let _ = write!(
            sql,
            " AND build_id NOT IN (SELECT build_id FROM run_build WHERE membership_id IN
                 (SELECT membership_id FROM run_build WHERE build_id = {build_id}))"
        );
    }
    let mut stmt = conn.prepare(&sql)?;
    let row = |r: &duckdb::Row<'_>| Ok((r.get(0)?, ElementVector::from_keys(ints(r.get(1)?))));
    let candidates: Vec<(i64, ElementVector)> = if opts.same_class {
        stmt.query_map(params![build_id, class], row)?
            .collect::<duckdb::Result<_>>()?
    } else {
        stmt.query_map(params![build_id], row)?
            .collect::<duckdb::Result<_>>()?
    };

    let mut scored: Vec<(i64, TieredDistance, u32)> = candidates
        .par_iter()
        .map(|(id, v)| {
            (
                *id,
                target.tiered_distance(v, &opts.weights),
                target.slot_edits(v),
            )
        })
        .collect();
    scored.sort_by(|a, b| {
        a.1.total
            .total_cmp(&b.1.total)
            .then(a.2.cmp(&b.2))
            .then(a.0.cmp(&b.0))
    });
    scored.truncate(opts.limit);

    let mut ids: Vec<i64> = scored.iter().map(|s| s.0).collect();
    ids.push(build_id);
    let mut summaries = summaries(db, &ids)?;
    let target_summary = summaries
        .remove(&build_id)
        .ok_or(DbError::BuildNotFound(build_id))?;
    let neighbours = scored
        .into_iter()
        .filter_map(|(id, distance, slot_edits)| {
            summaries.remove(&id).map(|summary| Neighbour {
                summary,
                distance,
                slot_edits,
            })
        })
        .collect();
    Ok(Similar {
        target: target_summary,
        neighbours,
        candidates: candidates.len(),
    })
}

/// Summaries for a set of builds, keyed by id.
pub fn summaries(
    db: &Database,
    ids: &[i64],
) -> Result<std::collections::HashMap<i64, BuildSummary>> {
    let list = Value::List(ids.iter().map(|&i| Value::BigInt(i)).collect());
    let mut stmt = db.conn().prepare(
        "SELECT build_id, class, element, item_name(exotic_armor_hash), item_name(exotic_weapon_hash),
                item_name(super_hash), item_names(aspect_hashes), stat_levels, stats_promoted,
                timeline_count, player_count
         FROM build WHERE list_contains(?, build_id)",
    )?;
    let rows = stmt.query_map(params![list], |r| {
        Ok(BuildSummary {
            build_id: r.get(0)?,
            class: r.get(1)?,
            element: r.get(2)?,
            exotic_armor: r.get(3)?,
            exotic_weapon: r.get(4)?,
            super_name: r.get(5)?,
            aspects: texts(r.get(6)?),
            stat_levels: ints(r.get(7)?).into_iter().map(|l| l as u8).collect(),
            stats_promoted: texts(r.get(8)?),
            timeline_count: r.get(9)?,
            player_count: r.get(10)?,
        })
    })?;
    rows.map(|r| r.map(|s| (s.build_id, s)).map_err(DbError::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds: 1 (target), 2 (one weapon perk away), 3 (exotic swapped),
    /// 4 (identical to 1 but a different class), 5 (identical to 1, but only
    /// ever used by a zero-kill passenger).
    fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(&dir.path().join("s.duckdb")).unwrap();
        let ex = |k: i64| (1_i64 << 32) | k; // exotic_armor
        let fr = |k: i64| (11_i64 << 32) | k; // fragment
        let wp = |k: i64| (19_i64 << 32) | k; // weapon_perk
        let row = |id: i64, class: &str, t1: i64, perk: i64| {
            format!(
                "({id}, '{class}', [], [], [], [], [], 0, 0, [{}], [{}, {}], [], [{}])",
                ex(t1),
                fr(1),
                fr(2),
                wp(perk)
            )
        };
        db.conn()
            .execute_batch(&format!(
                "INSERT INTO build (build_id, class, aspect_hashes, fragment_hashes, artifact_perk_hashes,
                     armor_mod_hashes, item_hashes, component_count, unresolved_count,
                     tier1_elements, tier2_elements, tier3_elements, tier4_elements)
                 VALUES {}, {}, {}, {}, {};
                 INSERT INTO run_build VALUES (10, 7, 2, 1, 0, 60, 1.0, true), (11, 7, 3, 1, 0, 60, 1.0, true),
                                              (12, 7, 4, 1, 0, 60, 1.0, true), (13, 8, 5, 1, 0, 60, 1.0, true);
                 INSERT INTO run_player (instance_id, membership_id, character_id, started_at, activity_hash,
                     kills, deaths, assists, precision_kills, super_kills, melee_kills, grenade_kills,
                     ability_kills, completed, is_clear, has_build)
                 VALUES (10, 7, 1, '2026-07-01', 1, 50, 0, 0, 0, 0, 0, 0, 0, true, true, true),
                        (11, 7, 1, '2026-07-01', 1, 50, 0, 0, 0, 0, 0, 0, 0, true, true, true),
                        (12, 7, 1, '2026-07-01', 1, 50, 0, 0, 0, 0, 0, 0, 0, true, true, true),
                        (13, 8, 2, '2026-07-01', 1, 0, 0, 0, 0, 0, 0, 0, 0, false, false, true);",
                row(1, "Warlock", 100, 500),
                row(2, "Warlock", 100, 501),
                row(3, "Warlock", 101, 500),
                row(4, "Titan", 100, 500),
                row(5, "Warlock", 100, 500),
            ))
            .unwrap();
        (dir, db)
    }

    #[test]
    fn ranks_by_tiered_distance_within_class_without_passengers() {
        let (_dir, db) = db();
        let s = nearest_builds(&db, 1, &SimilarOptions::default()).unwrap();
        assert_eq!(s.target.build_id, 1);
        let order: Vec<i64> = s.neighbours.iter().map(|n| n.summary.build_id).collect();
        // Perk swap (0.1) before exotic swap (0.4); Titan and passenger excluded.
        assert_eq!(order, [2, 3]);
        assert!((s.neighbours[0].distance.total - 0.1).abs() < 1e-12);
        assert!((s.neighbours[1].distance.total - 0.4).abs() < 1e-12);
        assert_eq!(s.neighbours[0].slot_edits, 1);
    }

    #[test]
    fn other_players_only_skips_the_target_players_builds() {
        let (_dir, db) = db();
        // Member 7 also used build 1, so their builds 2-4 are skipped; only
        // the passenger's build 5 remains (when passengers are included).
        db.conn()
            .execute_batch("INSERT INTO run_build VALUES (14, 7, 1, 1, 0, 60, 1.0, true)")
            .unwrap();
        let opts = SimilarOptions {
            other_players_only: true,
            include_passengers: true,
            same_class: false,
            ..SimilarOptions::default()
        };
        let s = nearest_builds(&db, 1, &opts).unwrap();
        let order: Vec<i64> = s.neighbours.iter().map(|n| n.summary.build_id).collect();
        assert_eq!(order, [5]);
    }

    #[test]
    fn options_widen_the_candidates() {
        let (_dir, db) = db();
        let opts = SimilarOptions {
            same_class: false,
            include_passengers: true,
            ..SimilarOptions::default()
        };
        let s = nearest_builds(&db, 1, &opts).unwrap();
        let order: Vec<i64> = s.neighbours.iter().map(|n| n.summary.build_id).collect();
        // Identical builds (distance 0) first, ordered by id.
        assert_eq!(order, [4, 5, 2, 3]);
        assert!(matches!(
            nearest_builds(&db, 999, &opts),
            Err(DbError::BuildNotFound(999))
        ));
    }
}
