//! Facet search over builds, with usage aggregated over matching runs.
//!
//! Build facets (class, exotics, abilities, aspects, perks, mods, ...) select
//! builds; run filters (activity, solo, clears, player, dates) select which
//! of their runs count. Results are grouped per build, or by a chosen key for
//! popularity reports. Names match case-insensitively against *every* item
//! with that name, because item names are not unique.

use std::fmt::Write as _;

use duckdb::params_from_iter;
use duckdb::types::Value;

use crate::{Database, DbError, Result};

/// What a search may group by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupBy {
    #[default]
    Build,
    Player,
    Class,
    Element,
    ExoticArmor,
    ExoticWeapon,
    Super,
    Aspects,
}

impl GroupBy {
    /// `(column label, SQL expression)` pairs forming the group key.
    fn key(self) -> &'static [(&'static str, &'static str)] {
        match self {
            GroupBy::Build => &[
                ("build_id", "b.build_id::VARCHAR"),
                ("class", "b.class"),
                ("element", "b.element"),
                ("exotic armor", "item_name(b.exotic_armor_hash)"),
                ("exotic weapon", "item_name(b.exotic_weapon_hash)"),
                ("super", "item_name(b.super_hash)"),
                (
                    "aspects",
                    "array_to_string(item_names(b.aspect_hashes), ', ')",
                ),
            ],
            GroupBy::Player => &[(
                "player",
                "coalesce(pl.bungie_name, pl.display_name, rb.membership_id::VARCHAR)",
            )],
            GroupBy::Class => &[("class", "b.class")],
            GroupBy::Element => &[("class", "b.class"), ("element", "b.element")],
            GroupBy::ExoticArmor => &[
                ("class", "b.class"),
                ("exotic armor", "item_name(b.exotic_armor_hash)"),
            ],
            GroupBy::ExoticWeapon => &[("exotic weapon", "item_name(b.exotic_weapon_hash)")],
            GroupBy::Super => &[("class", "b.class"), ("super", "item_name(b.super_hash)")],
            GroupBy::Aspects => &[
                ("class", "b.class"),
                (
                    "aspects",
                    "array_to_string(item_names(b.aspect_hashes), ', ')",
                ),
            ],
        }
    }
}

/// Result ordering (all descending).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    Runs,
    Players,
    Clears,
    Kills,
    Recent,
}

impl SortBy {
    fn sql(self) -> &'static str {
        match self {
            SortBy::Runs => "runs",
            SortBy::Players => "players",
            SortBy::Clears => "clears",
            SortBy::Kills => "avg_kills",
            SortBy::Recent => "last_used",
        }
    }
}

/// Search parameters. Empty facets impose no constraint.
#[derive(Debug, Clone, Default)]
pub struct BuildSearch {
    pub class: Option<String>,
    pub element: Option<String>,
    pub exotic_armor: Option<String>,
    pub exotic_weapon: Option<String>,
    pub super_name: Option<String>,
    /// Every listed name must be present (AND).
    pub aspects: Vec<String>,
    pub fragments: Vec<String>,
    /// Weapons in any slot.
    pub weapons: Vec<String>,
    /// Weapon perks, traits, origin traits and frames.
    pub perks: Vec<String>,
    /// Armor mods (general, stat, tuning) and weapon mods.
    pub mods: Vec<String>,
    pub artifact_perks: Vec<String>,
    /// Armor sets with at least two pieces equipped.
    pub armor_sets: Vec<String>,
    /// Substring of an activity name or Searchlight target group.
    pub activity: Option<String>,
    /// Substring of a player's Bungie name or display name.
    pub player: Option<String>,
    pub solo: bool,
    pub clears_only: bool,
    /// Count runs where the player got no kills (checkpoint bots, passengers).
    pub include_passengers: bool,
    /// Inclusive `YYYY-MM-DD` bounds on the run start date.
    pub since: Option<String>,
    pub until: Option<String>,
    pub group_by: GroupBy,
    pub sort: SortBy,
    pub limit: usize,
}

/// One result row: the group key plus usage over the matching runs.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchRow {
    pub key: Vec<String>,
    pub builds: i64,
    pub runs: i64,
    pub players: i64,
    pub clears: i64,
    pub avg_kills: f64,
    pub last_used: String,
}

/// Search results, best first.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub key_columns: Vec<&'static str>,
    pub rows: Vec<SearchRow>,
    /// Groups matching before `limit` was applied.
    pub total_groups: i64,
}

/// Resolve a name to every hash carrying it (case-insensitive), or fail with
/// close suggestions.
fn resolve(db: &Database, table: &str, hash_col: &str, name: &str) -> Result<Vec<u32>> {
    let mut stmt = db.conn().prepare(&format!(
        "SELECT {hash_col} FROM {table} WHERE lower(name) = lower(?)"
    ))?;
    let hashes: Vec<u32> = stmt
        .query_map([name], |r| r.get(0))?
        .collect::<duckdb::Result<_>>()?;
    if !hashes.is_empty() {
        return Ok(hashes);
    }
    let mut stmt = db.conn().prepare(&format!(
        "SELECT DISTINCT name FROM {table} WHERE name ILIKE '%' || ? || '%' ORDER BY length(name), name LIMIT 5"
    ))?;
    let suggestions: Vec<String> = stmt
        .query_map([name], |r| r.get(0))?
        .collect::<duckdb::Result<_>>()?;
    Err(DbError::UnknownName {
        name: name.to_string(),
        suggestions,
    })
}

fn hash_list(hashes: &[u32]) -> String {
    hashes
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Run a facet search.
pub fn search_builds(db: &Database, q: &BuildSearch) -> Result<SearchResult> {
    let mut params: Vec<Value> = Vec::new();
    let mut build_where = String::from("TRUE");
    if let Some(c) = &q.class {
        build_where.push_str(" AND lower(b.class) = lower(?)");
        params.push(Value::Text(c.clone()));
    }
    if let Some(e) = &q.element {
        build_where.push_str(" AND lower(b.element) = lower(?)");
        params.push(Value::Text(e.clone()));
    }
    for (name, column) in [
        (&q.exotic_armor, "exotic_armor_hash"),
        (&q.exotic_weapon, "exotic_weapon_hash"),
        (&q.super_name, "super_hash"),
    ] {
        if let Some(n) = name {
            let hashes = resolve(db, "item", "item_hash", n)?;
            let _ = write!(build_where, " AND b.{column} IN ({})", hash_list(&hashes));
        }
    }
    // Component facets: each listed name must be present among the kinds.
    let component_facets: [(&Vec<String>, &str); 7] = [
        (&q.aspects, "'aspect'"),
        (&q.fragments, "'fragment'"),
        (&q.weapons, "'weapon'"),
        (
            &q.perks,
            "'weapon_perk', 'origin_trait', 'weapon_intrinsic'",
        ),
        (
            &q.mods,
            "'armor_mod', 'armor_stat_mod', 'armor_tuning', 'weapon_mod'",
        ),
        (&q.artifact_perks, "'artifact_perk'"),
        (&q.armor_sets, "'armor_set'"),
    ];
    for (names, kinds) in component_facets {
        for n in names {
            let (hashes, extra) = if kinds == "'armor_set'" {
                (
                    resolve(db, "item_set", "set_hash", n)?,
                    " AND quantity >= 2",
                )
            } else {
                (resolve(db, "item", "item_hash", n)?, "")
            };
            let _ = write!(
                build_where,
                " AND b.build_id IN (SELECT build_id FROM build_component WHERE kind IN ({kinds}) \
                 AND component_hash IN ({}){extra})",
                hash_list(&hashes)
            );
        }
    }

    let mut run_where = String::from("TRUE");
    if !q.include_passengers {
        run_where.push_str(" AND p.kills > 0");
    }
    if q.clears_only {
        run_where.push_str(" AND p.is_clear");
    }
    if q.solo {
        run_where.push_str(" AND r.is_solo");
    }
    if let Some(a) = &q.activity {
        run_where.push_str(
            " AND (a.name ILIKE '%' || ? || '%' OR a.target_group ILIKE '%' || ? || '%')",
        );
        params.push(Value::Text(a.clone()));
        params.push(Value::Text(a.clone()));
    }
    if let Some(p) = &q.player {
        run_where.push_str(
            " AND (pl.bungie_name ILIKE '%' || ? || '%' OR pl.display_name ILIKE '%' || ? || '%')",
        );
        params.push(Value::Text(p.clone()));
        params.push(Value::Text(p.clone()));
    }
    if let Some(d) = &q.since {
        run_where.push_str(" AND r.started_at >= CAST(? AS DATE)");
        params.push(Value::Text(d.clone()));
    }
    if let Some(d) = &q.until {
        run_where.push_str(" AND r.started_at < CAST(? AS DATE) + INTERVAL 1 DAY");
        params.push(Value::Text(d.clone()));
    }

    let key = q.group_by.key();
    let key_select: Vec<String> = key
        .iter()
        .enumerate()
        .map(|(i, (_, e))| format!("{e} AS k{i}"))
        .collect();
    let sql = format!(
        "SELECT {keys},
                count(DISTINCT b.build_id) AS builds,
                count(*) AS runs,
                count(DISTINCT rb.membership_id) AS players,
                count(*) FILTER (WHERE p.is_clear) AS clears,
                avg(p.kills) AS avg_kills,
                strftime(max(r.started_at), '%Y-%m-%d') AS last_used,
                count(*) OVER () AS total_groups
         FROM build b
         JOIN run_build rb ON rb.build_id = b.build_id
         JOIN run_player p ON p.instance_id = rb.instance_id AND p.membership_id = rb.membership_id AND p.has_build
         JOIN run r ON r.instance_id = rb.instance_id
         JOIN activity a ON a.activity_hash = r.activity_hash
         JOIN player pl ON pl.membership_id = rb.membership_id
         WHERE {build_where} AND {run_where}
         GROUP BY ALL
         ORDER BY {sort} DESC NULLS LAST, runs DESC, k0
         LIMIT {limit}",
        keys = key_select.join(", "),
        sort = q.sort.sql(),
        limit = q.limit.max(1),
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let n = key.len();
    let mut total_groups = 0;
    let rows = stmt
        .query_map(params_from_iter(params.iter()), |r| {
            let mut k = Vec::with_capacity(n);
            for i in 0..n {
                k.push(r.get::<_, Option<String>>(i)?.unwrap_or_else(|| "—".into()));
            }
            Ok((
                SearchRow {
                    key: k,
                    builds: r.get(n)?,
                    runs: r.get(n + 1)?,
                    players: r.get(n + 2)?,
                    clears: r.get(n + 3)?,
                    avg_kills: r.get(n + 4)?,
                    last_used: r.get(n + 5)?,
                },
                r.get::<_, i64>(n + 6)?,
            ))
        })?
        .map(|row| {
            row.map(|(r, total)| {
                total_groups = total;
                r
            })
        })
        .collect::<duckdb::Result<Vec<_>>>()?;
    Ok(SearchResult {
        key_columns: key.iter().map(|(l, _)| *l).collect(),
        rows,
        total_groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds (all Warlock unless noted):
    ///   1: Nothing Manacles (hash 1) + Chaos Accelerant, Feed the Void
    ///   2: Nothing Manacles (hash 6, same name) + Chaos Accelerant
    ///   3: Getaway Artist + Chaos Accelerant, Feed the Void
    ///   4: Titan, Nothing Manacles hash 1 (an impossible but useful case)
    /// Runs: 10/11 in Ultimate Conquest, 12 in Vault of Glass; run 13 is a
    /// zero-kill passenger run of build 1.
    fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(&dir.path().join("q.duckdb")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO item (item_hash, name, is_exotic) VALUES
                    (1, 'Nothing Manacles', true), (6, 'Nothing Manacles', true), (2, 'Getaway Artist', true),
                    (3, 'Chaos Accelerant', false), (4, 'Feed the Void', false);
                 INSERT INTO activity (activity_hash, name, target_group) VALUES
                    (100, 'Ultimate Conquest: Lightblade', 'Ultimate Conquest'), (200, 'Vault of Glass: Master', 'Master Raid');
                 INSERT INTO player (membership_id, bungie_name) VALUES (7, 'Alpha#0001'), (8, 'Bravo#0002');
                 INSERT INTO build (build_id, class, exotic_armor_hash, aspect_hashes, fragment_hashes,
                     artifact_perk_hashes, armor_mod_hashes, item_hashes, component_count, unresolved_count,
                     tier1_elements, tier2_elements, tier3_elements, tier4_elements)
                 VALUES (1, 'Warlock', 1, [3, 4], [], [], [], [], 0, 0, [], [], [], []),
                        (2, 'Warlock', 6, [3], [], [], [], [], 0, 0, [], [], [], []),
                        (3, 'Warlock', 2, [3, 4], [], [], [], [], 0, 0, [], [], [], []),
                        (4, 'Titan', 1, [], [], [], [], [], 0, 0, [], [], [], []);
                 INSERT INTO build_component (build_id, kind, component_hash, quantity, position) VALUES
                    (1, 'aspect', 3, 1, 0), (1, 'aspect', 4, 1, 1), (2, 'aspect', 3, 1, 0),
                    (3, 'aspect', 3, 1, 0), (3, 'aspect', 4, 1, 1);
                 INSERT INTO run (instance_id, started_at, activity_hash, is_solo, source, is_tracked) VALUES
                    (10, '2026-07-01', 100, true, 'archive', true), (11, '2026-07-02', 100, false, 'archive', true),
                    (12, '2026-07-03', 200, false, 'archive', true), (13, '2026-07-04', 100, false, 'archive', true),
                    (14, '2026-07-05', 100, false, 'archive', true), (15, '2026-07-06', 100, false, 'archive', true);
                 INSERT INTO run_build VALUES (10, 7, 1, 1, 0, 60, 1.0, true), (11, 8, 1, 1, 0, 60, 1.0, true),
                    (12, 7, 1, 1, 0, 60, 1.0, true), (13, 8, 1, 1, 0, 60, 1.0, true),
                    (14, 7, 2, 1, 0, 60, 1.0, true), (15, 8, 3, 1, 0, 60, 1.0, true);
                 INSERT INTO run_player (instance_id, membership_id, character_id, started_at, activity_hash,
                     kills, deaths, assists, precision_kills, super_kills, melee_kills, grenade_kills,
                     ability_kills, completed, is_clear, has_build) VALUES
                    (10, 7, 1, '2026-07-01', 100, 40, 0, 0, 0, 0, 0, 0, 0, true, true, true),
                    (11, 8, 2, '2026-07-02', 100, 20, 0, 0, 0, 0, 0, 0, 0, true, false, true),
                    (12, 7, 1, '2026-07-03', 200, 90, 0, 0, 0, 0, 0, 0, 0, true, true, true),
                    (13, 8, 2, '2026-07-04', 100, 0, 0, 0, 0, 0, 0, 0, 0, false, false, true),
                    (14, 7, 1, '2026-07-05', 100, 10, 0, 0, 0, 0, 0, 0, 0, true, false, true),
                    (15, 8, 2, '2026-07-06', 100, 30, 0, 0, 0, 0, 0, 0, 0, true, true, true);",
            )
            .unwrap();
        (dir, db)
    }

    fn ids(r: &SearchResult) -> Vec<String> {
        r.rows.iter().map(|row| row.key[0].clone()).collect()
    }

    fn search(db: &Database, q: BuildSearch) -> SearchResult {
        search_builds(db, &BuildSearch { limit: 50, ..q }).unwrap()
    }

    #[test]
    fn names_match_every_hash_case_insensitively() {
        let (_d, db) = db();
        let r = search(
            &db,
            BuildSearch {
                class: Some("warlock".into()),
                exotic_armor: Some("nothing manacles".into()),
                ..Default::default()
            },
        );
        let mut got = ids(&r);
        got.sort();
        assert_eq!(got, ["1", "2"]);
    }

    #[test]
    fn repeated_facets_are_and() {
        let (_d, db) = db();
        let r = search(
            &db,
            BuildSearch {
                aspects: vec!["Chaos Accelerant".into(), "Feed the Void".into()],
                ..Default::default()
            },
        );
        let mut got = ids(&r);
        got.sort();
        assert_eq!(got, ["1", "3"]);
    }

    #[test]
    fn run_filters_shape_the_usage() {
        let (_d, db) = db();
        // Build 1: runs 10, 11 (Conquest), 12 (raid), 13 (passenger, excluded).
        let all = search(
            &db,
            BuildSearch {
                exotic_armor: Some("Nothing Manacles".into()),
                class: Some("Warlock".into()),
                ..Default::default()
            },
        );
        let b1 = all.rows.iter().find(|r| r.key[0] == "1").unwrap();
        assert_eq!((b1.runs, b1.players, b1.clears), (3, 2, 2));
        assert!((b1.avg_kills - 50.0).abs() < 1e-9);
        assert_eq!(b1.last_used, "2026-07-03");

        let conquest = search(
            &db,
            BuildSearch {
                activity: Some("conquest".into()),
                exotic_armor: Some("Nothing Manacles".into()),
                class: Some("Warlock".into()),
                ..Default::default()
            },
        );
        let b1 = conquest.rows.iter().find(|r| r.key[0] == "1").unwrap();
        assert_eq!(b1.runs, 2);

        let with_passengers = search(
            &db,
            BuildSearch {
                include_passengers: true,
                activity: Some("conquest".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            with_passengers
                .rows
                .iter()
                .find(|r| r.key[0] == "1")
                .unwrap()
                .runs,
            3
        );

        let solo = search(
            &db,
            BuildSearch {
                solo: true,
                ..Default::default()
            },
        );
        assert_eq!(ids(&solo), ["1"]);
        let clears = search(
            &db,
            BuildSearch {
                clears_only: true,
                player: Some("bravo".into()),
                ..Default::default()
            },
        );
        assert_eq!(ids(&clears), ["3"]);
        let dated = search(
            &db,
            BuildSearch {
                since: Some("2026-07-05".into()),
                until: Some("2026-07-05".into()),
                ..Default::default()
            },
        );
        assert_eq!(ids(&dated), ["2"]);
    }

    #[test]
    fn grouping_reports_popularity() {
        let (_d, db) = db();
        let r = search(
            &db,
            BuildSearch {
                class: Some("Warlock".into()),
                group_by: GroupBy::ExoticArmor,
                ..Default::default()
            },
        );
        assert_eq!(r.key_columns, ["class", "exotic armor"]);
        assert_eq!(r.total_groups, 2);
        // Nothing Manacles: builds 1 and 2, runs 10, 11, 12, 14.
        let nm = r
            .rows
            .iter()
            .find(|row| row.key[1] == "Nothing Manacles")
            .unwrap();
        assert_eq!((nm.builds, nm.runs), (2, 4));
        assert_eq!(r.rows[0].key[1], "Nothing Manacles", "sorted by runs");
    }

    #[test]
    fn unknown_names_suggest_alternatives() {
        let (_d, db) = db();
        let err = search_builds(
            &db,
            &BuildSearch {
                exotic_armor: Some("Manacles".into()),
                limit: 5,
                ..Default::default()
            },
        )
        .unwrap_err();
        match err {
            DbError::UnknownName { name, suggestions } => {
                assert_eq!(name, "Manacles");
                assert_eq!(suggestions, ["Nothing Manacles"]);
            }
            other => panic!("unexpected {other}"),
        }
    }
}
