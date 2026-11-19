//! The data behind the static build-ranking site (`site/`).
//!
//! The site cannot fit the ranking model in a browser (it needs every
//! player's runs), so rankings are precomputed here for a set of
//! **scopes**: all activities, each Searchlight target group, and each
//! activity. Builds are grouped by their core (class, element, exotics,
//! super, aspects); every group also carries a description of how it is
//! usually run (abilities, fragments, weapons, mods, stat focus) from all
//! its captured runs.
//!
//! The export holds aggregates only: no player names or ids.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;

use crate::filter::RunFilter;
use crate::rank::{BuildSort, RankOptions, rank_builds};
use crate::search::GroupBy;
use crate::{Database, Result, meta_key};

/// Version of the JSON layout; the site checks it.
pub const SITE_FORMAT_VERSION: u32 = 1;

/// Components listed in a group's description: `(label, SQL predicate over
/// bc = build_component and i = the component's item)`.
const DETAIL_KINDS: [(&str, &str); 10] = [
    ("grenade", "bc.kind = 'grenade'"),
    ("melee", "bc.kind = 'melee'"),
    ("class_ability", "bc.kind = 'class_ability'"),
    ("movement", "bc.kind = 'movement'"),
    ("fragments", "bc.kind = 'fragment'"),
    ("weapons", "bc.kind = 'weapon' AND NOT i.is_exotic"),
    (
        "class_item_perks",
        "bc.kind = 'armor_perk' AND p.is_exotic AND p.slot = 'class_item'",
    ),
    ("armor_mods", "bc.kind = 'armor_mod'"),
    ("set_bonuses", "bc.kind = 'set_bonus'"),
    ("artifact_perks", "bc.kind = 'artifact_perk'"),
];
/// A component is listed when it appears in at least this share of the
/// group's runs, at most `DETAIL_LIMIT` per kind.
const DETAIL_MIN_SHARE: f64 = 0.1;
const DETAIL_LIMIT: usize = 12;

#[derive(Debug, Clone)]
pub struct SiteOptions {
    /// Groups need this many regular players in a scope to be listed there.
    pub min_players: usize,
    pub rank: RankOptions,
}

impl Default for SiteOptions {
    fn default() -> Self {
        Self {
            min_players: 2,
            rank: RankOptions::default(),
        }
    }
}

/// Everything the site loads.
#[derive(Debug, Serialize)]
pub struct SiteData {
    pub format: u32,
    /// When the database was imported.
    pub imported_at: String,
    /// Date range of the scored clears.
    pub first_run: String,
    pub last_run: String,
    /// Prefix for icon paths.
    pub icon_base: &'static str,
    /// Icon path per item or perk name.
    pub icons: BTreeMap<String, String>,
    /// Component names, referenced by index from group details.
    pub names: Vec<String>,
    pub groups: Vec<Group>,
    pub scopes: Vec<Scope>,
}

/// A build group: the build as players describe it.
#[derive(Debug, Serialize)]
pub struct Group {
    pub class: Option<String>,
    pub element: Option<String>,
    pub armor: Option<String>,
    pub weapon: Option<String>,
    #[serde(rename = "super")]
    pub super_name: Option<String>,
    pub aspects: Vec<String>,
    pub details: Details,
}

/// How a group is usually run, over all its captured qualified clears.
#[derive(Debug, Default, Serialize)]
pub struct Details {
    /// Runs described (a run split across builds counts by share).
    pub runs: f64,
    /// Mean stat investment level (0–4) per stat, in storage order
    /// (Weapons, Health, Class, Grenade, Super, Melee).
    pub stats: [f64; 6],
    /// Per kind: `[name index, percent of runs]`, most common first.
    pub components: BTreeMap<&'static str, Vec<(u32, u8)>>,
}

/// One scope's ranking. A target group with a single activity is listed
/// only as the group.
#[derive(Debug, Serialize)]
pub struct Scope {
    /// URL-friendly id: `all`, `<group>` or `<group>/<activity>`.
    pub id: String,
    pub name: String,
    /// `all`, `group` or `activity`.
    pub kind: &'static str,
    /// The target group an activity belongs to.
    pub parent: Option<String>,
    pub clears: usize,
    pub build_runs: usize,
    pub players: usize,
    pub rows: Vec<Row>,
}

/// `[group, effect, raw, players' skill, players, runs, last used]`;
/// effects are on the log scale (`exp(x) - 1` is the percentage).
#[derive(Debug, Serialize, PartialEq)]
pub struct Row(
    pub u32,
    pub f64,
    pub f64,
    pub f64,
    pub usize,
    pub f64,
    pub String,
);

fn round(x: f64, digits: i32) -> f64 {
    let scale = 10_f64.powi(digits);
    (x * scale).round() / scale
}

/// Lowercase ASCII letters and digits, other runs of characters as `-`.
fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// "Ultimate Conquest: Lightblade: Customize" → "Ultimate Conquest: Lightblade".
fn display_name(activity: &str) -> &str {
    activity
        .strip_suffix(": Customize")
        .or_else(|| activity.strip_suffix(": Standard"))
        .unwrap_or(activity)
}

struct ScopeSpec {
    id: String,
    name: String,
    kind: &'static str,
    parent: Option<String>,
    filter: RunFilter,
}

fn scope_specs(db: &Database) -> Result<Vec<ScopeSpec>> {
    let mut stmt = db.conn().prepare(
        "SELECT a.target_group, a.name, count(*) FILTER (WHERE r.has_build) AS build_runs
         FROM run_score r JOIN activity a USING (activity_hash)
         WHERE a.target_group IS NOT NULL AND a.name IS NOT NULL
         GROUP BY ALL ORDER BY a.target_group, build_runs DESC, a.name",
    )?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<duckdb::Result<_>>()?;
    let mut specs = vec![ScopeSpec {
        id: "all".into(),
        name: "All activities".into(),
        kind: "all",
        parent: None,
        filter: RunFilter::default(),
    }];
    let mut activities_in: HashMap<&str, usize> = HashMap::new();
    for (group, _) in &rows {
        *activities_in.entry(group.as_str()).or_default() += 1;
    }
    let single: BTreeSet<String> = activities_in
        .into_iter()
        .filter(|&(_, n)| n == 1)
        .map(|(g, _)| g.to_string())
        .collect();
    let mut seen_groups = BTreeSet::new();
    for (group, activity) in rows {
        if seen_groups.insert(group.clone()) {
            specs.push(ScopeSpec {
                id: slug(&group),
                name: group.clone(),
                kind: "group",
                parent: None,
                filter: RunFilter {
                    target_group: Some(group.clone()),
                    ..RunFilter::default()
                },
            });
        }
        if single.contains(&group) {
            // The group scope is the same runs; don't repeat it.
            continue;
        }
        specs.push(ScopeSpec {
            id: format!("{}/{}", slug(&group), slug(display_name(&activity))),
            name: display_name(&activity).to_string(),
            kind: "activity",
            parent: Some(group.clone()),
            filter: RunFilter {
                target_group: Some(group),
                activity_name: Some(activity),
                ..RunFilter::default()
            },
        });
    }
    Ok(specs)
}

/// A group key as `GroupBy::Core` renders it.
type Key = Vec<String>;

fn opt(s: &str) -> Option<String> {
    (s != "—").then(|| s.to_string())
}

fn group_from_key(key: &Key) -> Group {
    Group {
        class: opt(&key[0]),
        element: opt(&key[1]),
        armor: opt(&key[2]),
        weapon: opt(&key[3]),
        super_name: opt(&key[4]),
        aspects: if key[5] == "—" || key[5].is_empty() {
            Vec::new()
        } else {
            key[5].split(", ").map(str::to_string).collect()
        },
        details: Details::default(),
    }
}

/// SQL producing one row per build used in a scored run: its core key
/// (fields joined by U+001F) and its run weight. Weights are exact decimals
/// so that DuckDB's parallel sums, and the order of tied components, are
/// the same on every export.
fn keyed_builds_sql() -> String {
    let key: Vec<String> = GroupBy::Core
        .key()
        .iter()
        .map(|(_, e)| format!("coalesce({e}, '—')"))
        .collect();
    format!(
        "WITH obs AS (
             SELECT rb.build_id,
                    coalesce(rb.share / nullif(sum(rb.share) OVER w, 0), 1.0 / count(*) OVER w)::DECIMAL(18, 9) AS w
             FROM run_score r JOIN run_build rb USING (instance_id, membership_id)
             WHERE r.has_build
             WINDOW w AS (PARTITION BY rb.instance_id, rb.membership_id)
         ), bw AS (SELECT build_id, sum(w) AS w FROM obs GROUP BY 1)
         SELECT b.build_id, concat_ws(chr(31), {key}) AS k, bw.w, b.stat_levels
         FROM build b JOIN bw USING (build_id)",
        key = key.join(", ")
    )
}

/// Describe every group in `keys` from all its captured runs.
fn describe(
    db: &Database,
    keys: &HashMap<Key, u32>,
    groups: &mut [Group],
    names: &mut Vec<String>,
) -> Result<()> {
    let index: HashMap<String, u32> = keys.iter().map(|(k, &g)| (k.join("\u{1f}"), g)).collect();

    // Runs and mean stat levels.
    {
        let sums: Vec<String> = (1..=6)
            .map(|i| {
                format!(
                    "coalesce(sum(w * stat_levels[{i}]) FILTER (WHERE stat_levels IS NOT NULL), 0)"
                )
            })
            .collect();
        let sql = format!(
            "WITH kb AS ({}) SELECT k, sum(w), sum(w) FILTER (WHERE stat_levels IS NOT NULL), {}
             FROM kb GROUP BY 1",
            keyed_builds_sql(),
            sums.join(", ")
        );
        let mut stmt = db.conn().prepare(&sql)?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let k: String = r.get(0)?;
            let Some(&g) = index.get(&k) else { continue };
            let d = &mut groups[g as usize].details;
            d.runs = round(r.get(1)?, 1);
            let w: Option<f64> = r.get(2)?;
            if let Some(w) = w.filter(|w| *w > 0.0) {
                for i in 0..6 {
                    let sum: f64 = r.get(3 + i)?;
                    d.stats[i] = round(sum / w, 2);
                }
            }
        }
    }

    // Components: share of the group's runs that include each one.
    let mut name_index: HashMap<String, u32> = names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i as u32))
        .collect();
    for (label, predicate) in DETAIL_KINDS {
        let sql = format!(
            "WITH kb AS ({kb}),
             totals AS (SELECT k, sum(w) AS total FROM kb GROUP BY 1),
             comp AS (
                 SELECT kb.k, coalesce(i.name, sp.name) AS name, sum(kb.w) AS w
                 FROM kb JOIN build_component bc USING (build_id)
                 LEFT JOIN item i ON i.item_hash = bc.component_hash
                 LEFT JOIN item p ON p.item_hash = bc.parent_hash
                 LEFT JOIN sandbox_perk sp ON bc.kind = 'set_bonus' AND sp.perk_hash = bc.component_hash
                 WHERE {predicate}
                 GROUP BY ALL
             )
             SELECT k, name, w / total AS share
             FROM comp JOIN totals USING (k)
             WHERE name IS NOT NULL AND w / total >= {DETAIL_MIN_SHARE}
             QUALIFY row_number() OVER (PARTITION BY k ORDER BY w DESC, name) <= {DETAIL_LIMIT}
             ORDER BY k, share DESC, name",
            kb = keyed_builds_sql()
        );
        let mut stmt = db.conn().prepare(&sql)?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let k: String = r.get(0)?;
            let Some(&g) = index.get(&k) else { continue };
            let name: String = r.get(1)?;
            let share: f64 = r.get(2)?;
            let next = names.len() as u32;
            let n = *name_index.entry(name.clone()).or_insert_with(|| {
                names.push(name);
                next
            });
            groups[g as usize]
                .details
                .components
                .entry(label)
                .or_default()
                .push((n, (share * 100.0).round().clamp(0.0, 100.0) as u8));
        }
    }
    Ok(())
}

/// Icons for every item or perk name the site shows.
fn icons(db: &Database, wanted: &BTreeSet<String>) -> Result<BTreeMap<String, String>> {
    let mut stmt = db.conn().prepare(
        "SELECT name, min(icon) FROM (
             SELECT name, icon FROM item WHERE icon IS NOT NULL
             UNION ALL SELECT name, icon FROM sandbox_perk WHERE icon IS NOT NULL
         ) GROUP BY 1",
    )?;
    let mut out = BTreeMap::new();
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let name: String = r.get(0)?;
        if wanted.contains(&name) {
            out.insert(name, r.get(1)?);
        }
    }
    Ok(out)
}

/// Build the site data. `progress` is called with each scope's name.
pub fn site_data(
    db: &Database,
    opts: &SiteOptions,
    mut progress: impl FnMut(&str),
) -> Result<SiteData> {
    let mut keys: HashMap<Key, u32> = HashMap::new();
    let mut groups: Vec<Group> = Vec::new();
    let mut scopes = Vec::new();
    for spec in scope_specs(db)? {
        progress(&spec.name);
        let rank = RankOptions {
            runs: spec.filter.clone(),
            group_by: GroupBy::Core,
            ..opts.rank.clone()
        };
        let (model, _, ranked) = rank_builds(db, &rank, opts.min_players, BuildSort::Effect)?;
        let rows = ranked
            .into_iter()
            .map(|r| {
                let next = groups.len() as u32;
                let g = *keys.entry(r.key.clone()).or_insert_with(|| {
                    groups.push(group_from_key(&r.key));
                    next
                });
                Row(
                    g,
                    round(r.effect, 3),
                    round(r.raw, 3),
                    round(r.player_skill, 3),
                    r.players,
                    round(r.runs, 1),
                    r.last_used,
                )
            })
            .collect();
        scopes.push(Scope {
            id: spec.id,
            name: spec.name,
            kind: spec.kind,
            parent: spec.parent,
            clears: model.runs,
            build_runs: model.build_runs,
            players: model.players,
            rows,
        });
    }

    progress("describing builds");
    let mut names = Vec::new();
    describe(db, &keys, &mut groups, &mut names)?;

    let mut wanted: BTreeSet<String> = names.iter().cloned().collect();
    for g in &groups {
        wanted.extend(g.armor.iter().cloned());
        wanted.extend(g.weapon.iter().cloned());
        wanted.extend(g.super_name.iter().cloned());
        wanted.extend(g.aspects.iter().cloned());
    }
    let icons = icons(db, &wanted)?;
    let (first_run, last_run): (Option<String>, Option<String>) = db.conn().query_row(
        "SELECT strftime(min(started_at), '%Y-%m-%d'), strftime(max(started_at), '%Y-%m-%d') FROM run_score",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(SiteData {
        format: SITE_FORMAT_VERSION,
        imported_at: db.meta(meta_key::IMPORTED_AT)?.unwrap_or_default(),
        first_run: first_run.unwrap_or_default(),
        last_run: last_run.unwrap_or_default(),
        icon_base: "https://www.bungie.net",
        icons,
        names,
        groups,
        scopes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four players with ten untracked clears each in Ultimate Conquest:
    /// Lightblade, and two tracked clears each: players 1-2 in Lightblade on
    /// build 10 (Alpha Boots), players 3-4 in Seraph's Shield on build 20
    /// (Beta Boots). Equilibrium has one activity and one untracked clear.
    fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(&dir.path().join("s.duckdb")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO item (item_hash, name, is_exotic, icon) VALUES
                    (1, 'Alpha Boots', true, '/a.jpg'), (2, 'Beta Boots', true, NULL),
                    (3, 'Ember of Char', false, '/e.png'), (5, 'Solar Grenade', false, NULL),
                    (6, 'The Mountaintop', false, NULL), (7, 'Exotic Cannon', true, NULL);
                 INSERT INTO activity (activity_hash, name, target_group) VALUES
                    (100, 'Ultimate Conquest: Lightblade: Customize', 'Ultimate Conquest'),
                    (101, 'Ultimate Conquest: Operation: Seraph''s Shield: Customize', 'Ultimate Conquest'),
                    (200, 'Equilibrium: Standard', 'Equilibrium');
                 INSERT INTO build (build_id, class, element, exotic_armor_hash, aspect_hashes, fragment_hashes,
                     artifact_perk_hashes, armor_mod_hashes, item_hashes, component_count, unresolved_count,
                     stat_levels, tier1_elements, tier2_elements, tier3_elements, tier4_elements)
                 VALUES (10, 'Warlock', 'Solar', 1, [], [], [], [], [], 0, 0, [1, 1, 1, 4, 2, 0], [], [], [], []),
                        (20, 'Warlock', 'Solar', 2, [], [], [], [], [], 0, 0, NULL, [], [], [], []);
                 INSERT INTO build_component (build_id, kind, component_hash, quantity, position) VALUES
                    (10, 'fragment', 3, 1, 0), (10, 'grenade', 5, 1, 1), (10, 'weapon', 6, 1, 2),
                    (10, 'weapon', 7, 1, 3), (20, 'fragment', 3, 1, 0);
                 INSERT INTO run_score
                 SELECT 1000 * m + i, m, CASE WHEN i < 10 OR m <= 2 THEN 100 ELSE 101 END,
                        TIMESTAMP '2026-07-01' + INTERVAL (i) DAY, false, i >= 10,
                        100, 0, 600, 1, 1, 1, 1, CASE WHEN m <= 2 THEN 0.5 ELSE -0.5 END
                 FROM range(1, 5) p(m), range(0, 12) r(i);
                 INSERT INTO run_score VALUES (9999, 1, 200, TIMESTAMP '2026-07-01', false, false,
                        100, 0, 600, 1, 1, 1, 1, 0.0);
                 INSERT INTO run_build
                 SELECT 1000 * m + i, m, CASE WHEN m <= 2 THEN 10 ELSE 20 END, 1, 0, 600, 1.0, true
                 FROM range(1, 5) p(m), range(10, 12) r(i);",
            )
            .unwrap();
        (dir, db)
    }

    #[test]
    fn exports_scopes_groups_and_details() {
        let (_d, db) = db();
        let opts = SiteOptions {
            min_players: 1,
            ..SiteOptions::default()
        };
        let mut seen = Vec::new();
        let data = site_data(&db, &opts, |s| seen.push(s.to_string())).unwrap();
        assert_eq!(data.format, SITE_FORMAT_VERSION);
        assert_eq!(
            (data.first_run.as_str(), data.last_run.as_str()),
            ("2026-07-01", "2026-07-12")
        );

        let ids: Vec<&str> = data.scopes.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "all",
                "equilibrium",
                "ultimate-conquest",
                "ultimate-conquest/ultimate-conquest-lightblade",
                "ultimate-conquest/ultimate-conquest-operation-seraph-s-shield",
            ],
            "single-activity groups are listed once"
        );
        let seraph = &data.scopes[4];
        assert_eq!(
            (seraph.kind, seraph.parent.as_deref()),
            ("activity", Some("Ultimate Conquest"))
        );
        assert_eq!(seraph.name, "Ultimate Conquest: Operation: Seraph's Shield");
        assert_eq!(seraph.build_runs, 4);
        assert_eq!(seraph.rows.len(), 1, "only Beta Boots was run there");
        assert!(seen.contains(&"describing builds".to_string()));

        let all = &data.scopes[0];
        assert_eq!((all.clears, all.build_runs, all.players), (49, 8, 4));
        assert_eq!(all.rows.len(), 2);
        let alpha = data
            .groups
            .iter()
            .position(|g| g.armor.as_deref() == Some("Alpha Boots"))
            .unwrap();
        let g = &data.groups[alpha];
        assert_eq!(
            (g.class.as_deref(), g.element.as_deref()),
            (Some("Warlock"), Some("Solar"))
        );
        assert!(g.aspects.is_empty());
        assert!((g.details.runs - 4.0).abs() < 1e-9);
        let expected = [1.0, 1.0, 1.0, 4.0, 2.0, 0.0];
        assert!(
            g.details
                .stats
                .iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < 1e-9)
        );
        let named = |kind: &str| -> Vec<(String, u8)> {
            g.details.components[kind]
                .iter()
                .map(|&(i, p)| (data.names[i as usize].clone(), p))
                .collect()
        };
        assert_eq!(named("fragments"), [("Ember of Char".to_string(), 100)]);
        assert_eq!(
            named("weapons"),
            [("The Mountaintop".to_string(), 100)],
            "exotic weapons are not repeated"
        );
        assert_eq!(named("grenade"), [("Solar Grenade".to_string(), 100)]);
        assert_eq!(
            data.icons.get("Alpha Boots").map(String::as_str),
            Some("/a.jpg")
        );
        assert_eq!(
            data.icons.get("Ember of Char").map(String::as_str),
            Some("/e.png")
        );
        assert!(!data.icons.contains_key("Beta Boots"), "no icon, no entry");

        // The export is aggregates only: nothing identifies a player.
        let json = serde_json::to_string(&data).unwrap();
        assert!(!json.contains("membership"));
    }

    #[test]
    fn slugs_and_display_names() {
        assert_eq!(slug("Ultimate Conquest"), "ultimate-conquest");
        assert_eq!(
            slug("Operation: Seraph's Shield"),
            "operation-seraph-s-shield"
        );
        assert_eq!(slug("//node.ovrd.AVALON//"), "node-ovrd-avalon");
        assert_eq!(
            display_name("Ultimate Conquest: Lightblade: Customize"),
            "Ultimate Conquest: Lightblade"
        );
        assert_eq!(display_name("Equilibrium: Standard"), "Equilibrium");
        assert_eq!(
            display_name("Vault of Glass: Master"),
            "Vault of Glass: Master"
        );
    }

    #[test]
    fn keys_become_groups() {
        let key: Key = [
            "Warlock",
            "Prismatic",
            "Getaway Artist",
            "—",
            "Song of Flame",
            "Hellion, Weaver's Call",
        ]
        .map(String::from)
        .to_vec();
        let g = group_from_key(&key);
        assert_eq!(g.armor.as_deref(), Some("Getaway Artist"));
        assert_eq!(g.weapon, None);
        assert_eq!(g.aspects, ["Hellion", "Weaver's Call"]);
        let rows =
            serde_json::to_string(&Row(3, 0.1, 0.2, 0.3, 4, 5.5, "2026-07-01".into())).unwrap();
        assert_eq!(rows, r#"[3,0.1,0.2,0.3,4,5.5,"2026-07-01"]"#);
    }
}
