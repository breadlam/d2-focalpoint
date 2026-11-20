//! The data behind the static build-ranking site (`site/`): a book of exact
//! recipes.
//!
//! Every listing on the site is a build a player actually used, published
//! exactly as equipped. The ranking tools only choose *which* builds to
//! publish:
//!
//! 1. **Build types** — class, element, exotics, super and aspects
//!    (`GroupBy::Core`) — are ranked by build effect (ADR 0006). This is the
//!    robust signal, pooled over every player of the type.
//! 2. Within a type, **recipe families** — the type plus an identical kit:
//!    exotic perks, abilities, aspects and fragments (`GroupBy::Recipe`) —
//!    and within the best family, **loadouts** — the kit plus the same
//!    legendary weapons (`GroupBy::Loadout`) — are ranked the same way.
//!    Pooling near-identical builds matters because most exact builds were
//!    used in a single clear.
//! 3. The best loadout's best-ranked **exact build** (`GroupBy::BuildId`)
//!    is the listing's exemplar, its "top recipe". Its **variants** are the
//!    loadout's other exact builds within `VARIANT_MAX_CHANGES` elements of
//!    it: near-identical builds that differ in a perk roll, a mod or an
//!    artifact perk. Each is published exactly.
//!
//! Rankings are computed per **scope** (all activities, each Searchlight
//! target group, each activity of a group with more than one), because the
//! model cannot run in a browser. Recipes are stored once and referenced by
//! index. The export holds no player names or ids.
//!
//! Each scope, and the neutral game, also gets per-class **highlights**
//! ([`crate::highlight`]): a few listings as well ranked and as unlike each
//! other as the data allows.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::Serialize;

use focalpoint_core::vector::ElementVector;

use crate::filter::RunFilter;
use crate::highlight::{
    ClassHighlights, Highlight, HighlightSet, QUALITY_WEIGHT, cap, highlight_sets,
};
use crate::rank::{BuildRank, BuildSort, NEUTRAL_CONTEXT_PRIOR, RankOptions, rank_builds};
use crate::recipe::{Dict, DictEntry, Recipe, recipes};
use crate::search::GroupBy;
use crate::{Database, Result, meta_key};

/// Version of the JSON layout; the site checks it.
pub const SITE_FORMAT_VERSION: u32 = 6;

/// Each scope's listings are split in two files: build types with at least
/// this many players (the page's default minimum) load with the page; the
/// rest only when the reader lowers the minimum.
pub const CORE_PLAYERS: usize = 3;

/// Variants listed per listing (the rest are counted).
pub const VARIANT_LIMIT: usize = 5;

/// Variants differ from the top recipe by at most this many elements of the
/// build vector (added plus removed, so 8 is four swaps): a perk roll, a mod
/// or an artifact perk, not a different build. Within a loadout, a quarter
/// of build pairs differ by 4 or fewer elements of ≈ 77; the median by 25.
pub const VARIANT_MAX_CHANGES: u32 = 8;

/// A top recipe should come from a loadout players came back to and that
/// beat their usual level: the best loadout with a positive effect used in
/// at least this many clears (in the scope), falling back to the next floor
/// when a build type has none, and finally to its best loadout.
pub const EVIDENCE_FLOORS: [f64; 3] = [3.0, 2.0, 0.0];

#[derive(Debug, Clone)]
pub struct SiteOptions {
    /// Build types need this many regular players in a scope to be listed.
    pub min_players: usize,
    pub rank: RankOptions,
    /// Compute only these scopes (ids such as `all` or `master-raid`); all
    /// of them when `None`. The neutral game needs `all`.
    pub scopes: Option<Vec<String>>,
    /// See [`crate::highlight::QUALITY_WEIGHT`].
    pub quality_weight: f64,
    /// See [`crate::rank::NEUTRAL_CONTEXT_PRIOR`].
    pub context_prior: f64,
}

impl Default for SiteOptions {
    fn default() -> Self {
        Self {
            min_players: 1,
            rank: RankOptions::default(),
            scopes: None,
            quality_weight: QUALITY_WEIGHT,
            context_prior: NEUTRAL_CONTEXT_PRIOR,
        }
    }
}

/// Everything the ranked builds view shows, before it is split into files
/// ([`split`]).
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
    /// Things recipes refer to by index: `[name, icon, type, damage type]`.
    pub dict: Vec<DictEntry>,
    /// The six armor stats in recipe order (Weapons, Health, Class, Grenade,
    /// Super, Melee): `[name, icon]`.
    pub stats: Vec<(String, Option<String>)>,
    /// Element glyph per damage type name ("Arc", "Solar", …).
    pub elements: BTreeMap<String, String>,
    /// Icon path per name shown in build types (exotics, supers, aspects).
    pub icons: BTreeMap<String, String>,
    pub types: Vec<BuildType>,
    pub recipes: Vec<Recipe>,
    pub scopes: Vec<Scope>,
    /// The neutral game first, then every scope.
    pub highlights: Vec<HighlightSet>,
    /// Per build type, its activity-independent effect over all activities
    /// (the neutral-game ranking); empty when `all` was not computed.
    #[serde(skip)]
    pub neutral: HashMap<u32, f64>,
    /// Published builds missing part of their kit or their armor mods
    /// ([`crate::analysis::completeness::Parts::complete`]): not highlight
    /// candidates.
    #[serde(skip)]
    pub incomplete: HashSet<i64>,
}

/// A build type: the build as players describe it.
#[derive(Debug, Serialize)]
pub struct BuildType {
    pub class: Option<String>,
    pub element: Option<String>,
    pub armor: Option<String>,
    pub weapon: Option<String>,
    #[serde(rename = "super")]
    pub super_name: Option<String>,
    pub aspects: Vec<String>,
}

/// One scope's listings, best build type first.
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

/// `[type, effect, raw, players' skill, players, runs, last used, top recipe,
/// variants, variants not listed, clears of the top recipe's loadout,
/// exact builds of that loadout]`.
/// Effects are on the log scale (`exp(x) - 1` is the percentage).
#[derive(Debug, Serialize, PartialEq)]
pub struct Row(
    pub u32,
    pub f64,
    pub f64,
    pub f64,
    pub usize,
    pub f64,
    pub String,
    pub Pick,
    pub Vec<Pick>,
    pub usize,
    pub f64,
    pub usize,
);

/// An exact build's record in a scope: `[recipe, effect, runs, players,
/// last used]`.
#[derive(Debug, Serialize, PartialEq)]
pub struct Pick(pub u32, pub f64, pub f64, pub usize, pub String);

/// The ranked builds view as files: an index loaded with the page, two
/// parts per scope and one highlights file per set, loaded on demand.
#[derive(Debug)]
pub struct Book {
    pub index: BookIndex,
    /// `(file name, contents)`.
    pub parts: Vec<(String, BookPart)>,
    pub highlights: Vec<(String, HighlightFile)>,
}

/// One highlight set in its own file, with its picks' recipes.
#[derive(Debug, Serialize)]
pub struct HighlightFile {
    pub format: u32,
    pub id: String,
    pub name: String,
    pub classes: Vec<ClassHighlights>,
    pub recipes: Vec<Recipe>,
}

/// A highlight set in the index: `[id, name, file]`.
#[derive(Debug, Serialize, PartialEq)]
pub struct HighlightMeta(pub String, pub String, pub String);

/// `book/index.json`: names, types and every scope without its listings.
#[derive(Debug, Serialize)]
pub struct BookIndex {
    pub format: u32,
    pub imported_at: String,
    pub first_run: String,
    pub last_run: String,
    pub icon_base: &'static str,
    pub dict: Vec<DictEntry>,
    pub stats: Vec<(String, Option<String>)>,
    pub elements: BTreeMap<String, String>,
    pub icons: BTreeMap<String, String>,
    pub types: Vec<BuildType>,
    pub scopes: Vec<ScopeMeta>,
    pub highlights: Vec<HighlightMeta>,
}

/// A scope in the index.
#[derive(Debug, Serialize)]
pub struct ScopeMeta {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub parent: Option<String>,
    pub clears: usize,
    pub build_runs: usize,
    pub players: usize,
    /// The scope's files: `[least players, most players or null, file, listings]`.
    pub parts: Vec<PartMeta>,
    /// Every listing in brief, `[type, effect, players]`, so a listing can
    /// say where else its build type ranks without loading other scopes.
    pub summary: Vec<(u32, f64, usize)>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct PartMeta(pub usize, pub Option<usize>, pub String, pub usize);

/// One part of a scope: its listings, with picks indexing `recipes`.
#[derive(Debug, Serialize)]
pub struct BookPart {
    pub format: u32,
    pub scope: String,
    pub rows: Vec<Row>,
    pub recipes: Vec<Recipe>,
}

/// Split [`SiteData`] into the index, per-scope parts and highlight files.
/// A file's recipes are its own (a recipe listed in several scopes is
/// repeated), so each file stands alone.
#[must_use]
pub fn split(data: SiteData) -> Book {
    let mut highlights = Vec::new();
    let mut highlight_metas = Vec::new();
    for set in data.highlights {
        let file = format!("highlights.{}.json", set.id.replace('/', "--"));
        let mut local: HashMap<u32, u32> = HashMap::new();
        let mut recipes = Vec::new();
        let classes = set
            .classes
            .into_iter()
            .map(|c| ClassHighlights {
                class: c.class,
                picks: c
                    .picks
                    .into_iter()
                    .map(|mut h: Highlight| {
                        let next = local.len() as u32;
                        h.5 = *local.entry(h.5).or_insert_with(|| {
                            recipes.push(data.recipes[h.5 as usize].clone());
                            next
                        });
                        h
                    })
                    .collect(),
            })
            .collect();
        highlight_metas.push(HighlightMeta(
            set.id.clone(),
            set.name.clone(),
            file.clone(),
        ));
        highlights.push((
            file,
            HighlightFile {
                format: SITE_FORMAT_VERSION,
                id: set.id,
                name: set.name,
                classes,
                recipes,
            },
        ));
    }
    let mut parts = Vec::new();
    let mut scopes = Vec::with_capacity(data.scopes.len());
    for scope in data.scopes {
        let summary = scope.rows.iter().map(|r| (r.0, r.1, r.4)).collect();
        let base = scope.id.replace('/', "--");
        let (core, more): (Vec<Row>, Vec<Row>) =
            scope.rows.into_iter().partition(|r| r.4 >= CORE_PLAYERS);
        let mut metas = Vec::new();
        for (rows, least, most, file) in [
            (core, CORE_PLAYERS, None, format!("{base}.json")),
            (more, 1, Some(CORE_PLAYERS - 1), format!("{base}.more.json")),
        ] {
            metas.push(PartMeta(least, most, file.clone(), rows.len()));
            if rows.is_empty() {
                continue;
            }
            let mut local: HashMap<u32, u32> = HashMap::new();
            let mut recipes = Vec::new();
            let mut map = |i: u32| -> u32 {
                let next = local.len() as u32;
                *local.entry(i).or_insert_with(|| {
                    recipes.push(data.recipes[i as usize].clone());
                    next
                })
            };
            let rows = rows
                .into_iter()
                .map(|mut r| {
                    r.7.0 = map(r.7.0);
                    for v in &mut r.8 {
                        v.0 = map(v.0);
                    }
                    r
                })
                .collect();
            parts.push((
                file,
                BookPart {
                    format: SITE_FORMAT_VERSION,
                    scope: scope.id.clone(),
                    rows,
                    recipes,
                },
            ));
        }
        scopes.push(ScopeMeta {
            id: scope.id,
            name: scope.name,
            kind: scope.kind,
            parent: scope.parent,
            clears: scope.clears,
            build_runs: scope.build_runs,
            players: scope.players,
            parts: metas,
            summary,
        });
    }
    Book {
        index: BookIndex {
            format: data.format,
            imported_at: data.imported_at,
            first_run: data.first_run,
            last_run: data.last_run,
            icon_base: data.icon_base,
            dict: data.dict,
            stats: data.stats,
            elements: data.elements,
            icons: data.icons,
            types: data.types,
            scopes,
            highlights: highlight_metas,
        },
        parts,
        highlights,
    }
}

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
    let mut activities_in: HashMap<&str, usize> = HashMap::new();
    for (group, _) in &rows {
        *activities_in.entry(group.as_str()).or_default() += 1;
    }
    let single: BTreeSet<String> = activities_in
        .into_iter()
        .filter(|&(_, n)| n == 1)
        .map(|(g, _)| g.to_string())
        .collect();
    let mut specs = vec![ScopeSpec {
        id: "all".into(),
        name: "All activities".into(),
        kind: "all",
        parent: None,
        filter: RunFilter::default(),
    }];
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

/// A rank key joined with U+001F, as `keys_of_builds` renders it in SQL.
fn joined(key: &[String]) -> String {
    key.join("\u{1f}")
}

fn opt(s: &str) -> Option<String> {
    (s != "—").then(|| s.to_string())
}

fn type_from_key(key: &[String]) -> BuildType {
    BuildType {
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
    }
}

/// The element vector of every build used in a scored run.
pub fn build_vectors(db: &Database) -> Result<HashMap<i64, ElementVector>> {
    let mut stmt = db.conn().prepare(
        "SELECT b.build_id, list_concat(b.tier1_elements, b.tier2_elements, b.tier3_elements, b.tier4_elements)
         FROM build b
         WHERE b.build_id IN (SELECT rb.build_id FROM run_build rb
                              JOIN run_score r USING (instance_id, membership_id))",
    )?;
    let mut out = HashMap::new();
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let keys: Vec<i64> = match r.get::<_, duckdb::types::Value>(1)? {
            duckdb::types::Value::List(v) => v
                .into_iter()
                .filter_map(|x| match x {
                    duckdb::types::Value::BigInt(k) => Some(k),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        out.insert(id, ElementVector::from_keys(keys));
    }
    Ok(out)
}

/// The loadout key of every build used in a scored run.
fn loadout_of_builds(db: &Database) -> Result<HashMap<i64, String>> {
    let parts: Vec<String> = GroupBy::Loadout
        .key()
        .iter()
        .map(|(_, e)| format!("coalesce({e}, '—')"))
        .collect();
    let sql = format!(
        "SELECT b.build_id, concat_ws(chr(31), {}) FROM build b
         WHERE b.build_id IN (SELECT rb.build_id FROM run_build rb
                              JOIN run_score r USING (instance_id, membership_id))",
        parts.join(", ")
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

fn by_effect(a: &BuildRank, b: &BuildRank) -> std::cmp::Ordering {
    b.effect
        .total_cmp(&a.effect)
        .then(b.runs.total_cmp(&a.runs))
        .then_with(|| a.key.cmp(&b.key))
}

/// The chosen loadout of one build type in one scope and its exact builds
/// by effect (the first is the exemplar): at the highest evidence floor any
/// loadout with a positive effect meets, the best family's best such
/// loadout; failing all floors, the best family's best loadout.
fn choose<'a>(
    families: &[&'a BuildRank],
    loadouts_of: &HashMap<String, Vec<&'a BuildRank>>,
    members: &HashMap<String, Vec<(i64, &'a BuildRank)>>,
) -> Option<(&'a BuildRank, Vec<(i64, &'a BuildRank)>)> {
    let mut families = families.to_vec();
    families.sort_by(|a, b| by_effect(a, b));
    for floor in EVIDENCE_FLOORS {
        for family in &families {
            let Some(loadouts) = loadouts_of.get(&joined(&family.key)) else {
                continue;
            };
            let mut loadouts: Vec<&BuildRank> = loadouts
                .iter()
                .copied()
                .filter(|l| floor <= 0.0 || (l.runs + 1e-9 >= floor && l.effect > 0.0))
                .collect();
            loadouts.sort_by(|a, b| by_effect(a, b));
            for loadout in loadouts {
                if let Some(builds) = members.get(&joined(&loadout.key)) {
                    let mut builds = builds.clone();
                    builds.sort_by(|(ia, a), (ib, b)| by_effect(a, b).then(ia.cmp(ib)));
                    return Some((loadout, builds));
                }
            }
        }
    }
    None
}

/// The six armor stats in recipe order, and the element glyphs.
pub(crate) type StatIcons = Vec<(String, Option<String>)>;

pub(crate) fn stat_and_element_icons(
    db: &Database,
) -> Result<(StatIcons, BTreeMap<String, String>)> {
    let mut stats = Vec::new();
    for stat in focalpoint_core::stats::ArmorStat::ALL {
        let (name, icon): (String, Option<String>) = db
            .conn()
            .query_row(
                "SELECT name, icon FROM stat WHERE stat_hash = ?",
                [stat.hash()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or_else(|_| (stat.as_str().to_string(), None));
        stats.push((name, icon));
    }
    let mut elements = BTreeMap::new();
    let mut stmt = db.conn().prepare(
        "SELECT name, icon FROM damage_type WHERE icon IS NOT NULL ORDER BY damage_type_hash",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        elements.entry(r.get(0)?).or_insert(r.get(1)?);
    }
    Ok((stats, elements))
}

/// Icons for the names shown in build types.
fn type_icons(db: &Database, types: &[BuildType]) -> Result<BTreeMap<String, String>> {
    let mut wanted = BTreeSet::new();
    for t in types {
        wanted.extend(t.armor.iter().cloned());
        wanted.extend(t.weapon.iter().cloned());
        wanted.extend(t.super_name.iter().cloned());
        wanted.extend(t.aspects.iter().cloned());
    }
    let mut stmt = db
        .conn()
        .prepare("SELECT name, min(icon) FROM item WHERE icon IS NOT NULL GROUP BY 1")?;
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

fn pick(recipe: u32, r: &BuildRank) -> Pick {
    Pick(
        recipe,
        round(r.effect, 3),
        round(r.runs, 1),
        r.players,
        r.last_used.clone(),
    )
}

/// Build the site data. `progress` is called with each scope's name.
pub fn site_data(
    db: &Database,
    opts: &SiteOptions,
    mut progress: impl FnMut(&str),
) -> Result<SiteData> {
    let loadout_of_build = loadout_of_builds(db)?;
    let vectors = build_vectors(db)?;
    let mut type_index: HashMap<String, u32> = HashMap::new();
    let mut types: Vec<BuildType> = Vec::new();
    let mut recipe_index: HashMap<i64, u32> = HashMap::new();
    let mut recipe_ids: Vec<i64> = Vec::new();
    let mut intern = |id: i64| -> u32 {
        let next = recipe_ids.len() as u32;
        *recipe_index.entry(id).or_insert_with(|| {
            recipe_ids.push(id);
            next
        })
    };

    let mut scopes = Vec::new();
    let wanted = |id: &str| {
        opts.scopes
            .as_ref()
            .is_none_or(|ids| ids.iter().any(|w| w == id))
    };
    for spec in scope_specs(db)?.into_iter().filter(|s| wanted(&s.id)) {
        progress(&spec.name);
        let rank = |group_by: GroupBy, min_players: usize| {
            let o = RankOptions {
                runs: spec.filter.clone(),
                group_by,
                ..opts.rank.clone()
            };
            rank_builds(db, &o, min_players, BuildSort::Effect)
        };
        let (model, _, ranked_types) = rank(GroupBy::Core, opts.min_players)?;
        let (_, _, ranked_families) = rank(GroupBy::Recipe, 1)?;
        let (_, _, ranked_loadouts) = rank(GroupBy::Loadout, 1)?;
        let (_, _, ranked_builds) = rank(GroupBy::BuildId, 1)?;

        // Families per type, loadouts per family, exact builds per loadout.
        let mut families_of: HashMap<String, Vec<&BuildRank>> = HashMap::new();
        for f in &ranked_families {
            families_of.entry(joined(&f.key[..6])).or_default().push(f);
        }
        let mut loadouts_of: HashMap<String, Vec<&BuildRank>> = HashMap::new();
        for l in &ranked_loadouts {
            loadouts_of.entry(joined(&l.key[..7])).or_default().push(l);
        }
        let mut members: HashMap<String, Vec<(i64, &BuildRank)>> = HashMap::new();
        for b in &ranked_builds {
            let Ok(id) = b.key[0].parse::<i64>() else {
                continue;
            };
            if let Some(loadout) = loadout_of_build.get(&id) {
                members.entry(loadout.clone()).or_default().push((id, b));
            }
        }

        let mut rows = Vec::new();
        for t in &ranked_types {
            let Some(families) = families_of.get(&joined(&t.key)) else {
                continue;
            };
            let Some((loadout, builds)) = choose(families, &loadouts_of, &members) else {
                continue;
            };
            let next = types.len() as u32;
            let type_id = *type_index.entry(joined(&t.key)).or_insert_with(|| {
                types.push(type_from_key(&t.key));
                next
            });
            let (top_id, top) = builds[0];
            let near: Vec<&(i64, &BuildRank)> = match vectors.get(&top_id) {
                Some(v) => builds[1..]
                    .iter()
                    .filter(|(id, _)| {
                        vectors
                            .get(id)
                            .is_some_and(|w| v.l1_distance(w) <= VARIANT_MAX_CHANGES)
                    })
                    .collect(),
                None => Vec::new(),
            };
            let variants: Vec<Pick> = near
                .iter()
                .take(VARIANT_LIMIT)
                .map(|(id, b)| pick(intern(*id), b))
                .collect();
            rows.push(Row(
                type_id,
                round(t.effect, 3),
                round(t.raw, 3),
                round(t.player_skill, 3),
                t.players,
                round(t.runs, 1),
                t.last_used.clone(),
                pick(intern(top_id), top),
                variants,
                near.len().saturating_sub(VARIANT_LIMIT),
                round(loadout.runs, 1),
                builds.len(),
            ));
        }
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

    // The neutral game: all activities, with build × activity group effects.
    let mut neutral = HashMap::new();
    if wanted("all") {
        progress("neutral game");
        let o = RankOptions {
            context_prior: Some(opts.context_prior),
            ..opts.rank.clone()
        };
        let (_, _, ranked) = rank_builds(db, &o, 1, BuildSort::Effect)?;
        for t in ranked {
            if let Some(&id) = type_index.get(&joined(&t.key)) {
                neutral.insert(id, t.effect);
            }
        }
    }

    let parts = crate::analysis::completeness::parts(db)?;
    let incomplete: HashSet<i64> = recipe_ids
        .iter()
        .copied()
        .filter(|id| {
            !parts
                .get(id)
                .is_some_and(crate::analysis::completeness::Parts::complete)
        })
        .collect();

    progress("loading recipes");
    let mut dict = Dict::default();
    let recipes = recipes(db, &recipe_ids, &mut dict)?;
    let dict = dict.resolve(db)?;
    let icons = type_icons(db, &types)?;
    let (stats, elements) = stat_and_element_icons(db)?;
    let (first_run, last_run): (Option<String>, Option<String>) = db.conn().query_row(
        "SELECT strftime(min(started_at), '%Y-%m-%d'), strftime(max(started_at), '%Y-%m-%d') FROM run_score",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    progress("highlights");
    let mut data = SiteData {
        format: SITE_FORMAT_VERSION,
        imported_at: db.meta(meta_key::IMPORTED_AT)?.unwrap_or_default(),
        first_run: first_run.unwrap_or_default(),
        last_run: last_run.unwrap_or_default(),
        icon_base: "https://www.bungie.net",
        dict,
        stats,
        elements,
        icons,
        types,
        recipes,
        scopes,
        highlights: Vec::new(),
        neutral,
        incomplete,
    };
    data.highlights = highlight_sets(&data, &vectors, &cap, opts.quality_weight);
    Ok(data)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Two build types (Alpha Boots, Beta Boots). Alpha has two recipe
    /// families: builds 10, 11 and 13 share a kit; 11 differs from 10 only
    /// in a weapon perk (a variant), 13 carries another weapon (not a
    /// variant); build 12 has another fragment. Players 1–2 are strong
    /// (+0.5 usually), 3–4 weak (−0.5).
    ///
    /// * player 1: build 10 twice at +1.0 (Lightblade), build 11 once at
    ///   +0.6 (Seraph's Shield), build 13 once at +0.7;
    /// * player 2: build 10 twice at +1.0, build 12 once at 0.0, build 14
    ///   (10's loadout, but nine more armor mods: not near-identical) once
    ///   at +0.5;
    /// * players 3–4: build 20 twice each at −0.5 (Seraph's Shield).
    pub(crate) fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(&dir.path().join("s.duckdb")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO item (item_hash, name, is_exotic, icon) VALUES
                    (1, 'Alpha Boots', true, '/a.jpg'), (2, 'Beta Boots', true, NULL),
                    (3, 'Ember of Char', false, '/e.png'), (9, 'Ember of Ashes', false, NULL),
                    (5, 'Solar Grenade', false, NULL), (6, 'The Mountaintop', false, NULL),
                    (8, 'Fatebringer', false, NULL), (30, 'Dawnblade', false, NULL),
                    (31, 'Well of Radiance', false, NULL), (32, 'Heat Rises', false, NULL),
                    (40, 'Outlaw', false, NULL), (41, 'Adaptive Frame', false, NULL),
                    (42, 'Hands-On', false, NULL), (43, '+Grenade / -Health', false, NULL),
                    (44, 'Grenadier', false, NULL), (45, 'Elemental Siphon', false, NULL),
                    (46, 'Kill Clip', false, NULL);
                 INSERT INTO item_set (set_hash, name) VALUES (900, 'Iron Set');
                 INSERT INTO sandbox_perk (perk_hash, name, icon) VALUES (901, 'Iron Will', '/p.png');
                 INSERT INTO activity (activity_hash, name, target_group) VALUES
                    (100, 'Ultimate Conquest: Lightblade: Customize', 'Ultimate Conquest'),
                    (101, 'Ultimate Conquest: Operation: Seraph''s Shield: Customize', 'Ultimate Conquest'),
                    (200, 'Equilibrium: Standard', 'Equilibrium');
                 INSERT INTO build (build_id, class, element, exotic_armor_hash, super_hash, aspect_hashes,
                     fragment_hashes, artifact_perk_hashes, armor_mod_hashes, item_hashes, component_count,
                     unresolved_count, stat_weapons, stat_health, stat_class, stat_grenade, stat_super, stat_melee,
                     tier1_elements, tier2_elements, tier3_elements, tier4_elements)
                 VALUES (10, 'Warlock', 'Solar', 1, 31, [32], [], [], [], [], 0, 0, 30, 10, 20, 200, 100, 15,
                         [4294967297], [21474836511, 47244640259], [], [68719476742]),
                        (11, 'Warlock', 'Solar', 1, 31, [32], [], [], [], [], 0, 0, 30, 10, 20, 200, 100, 15,
                         [4294967297], [21474836511, 47244640259], [], [68719476742]),
                        (13, 'Warlock', 'Solar', 1, 31, [32], [], [], [], [], 0, 0, NULL, NULL, NULL, NULL, NULL, NULL,
                         [4294967297], [21474836511, 47244640259], [], [68719476744]),
                        (14, 'Warlock', 'Solar', 1, 31, [32], [], [], [], [], 0, 0, NULL, NULL, NULL, NULL, NULL, NULL,
                         [4294967297], [21474836511, 47244640259],
                         [64424509441, 64424509442, 64424509443, 64424509444, 64424509445,
                          64424509446, 64424509447, 64424509448, 64424509449], [68719476742]),
                        (12, 'Warlock', 'Solar', 1, 31, [32], [], [], [], [], 0, 0, NULL, NULL, NULL, NULL, NULL, NULL,
                         [4294967297], [21474836511, 47244640265], [], []),
                        (20, 'Warlock', 'Solar', 2, 31, [32], [], [], [], [], 0, 0, NULL, NULL, NULL, NULL, NULL, NULL,
                         [4294967298], [21474836511, 47244640259], [], []);
                 INSERT INTO build_component (build_id, kind, slot, component_hash, parent_hash, quantity, position)
                 SELECT b, kind, slot, hash, parent, quantity, position FROM (VALUES
                    ('subclass', 'subclass', 30, NULL, 1, 0), ('super', 'subclass', 31, NULL, 1, 1),
                    ('grenade', 'subclass', 5, NULL, 1, 2), ('aspect', 'subclass', 32, NULL, 1, 3),
                    ('fragment', 'subclass', 3, NULL, 1, 4),
                    ('armor', 'legs', 1, NULL, 1, 0), ('armor_mod', 'legs', 42, 1, 2, 1),
                    ('armor_tuning', 'legs', 43, 1, 1, 2), ('armor_archetype', 'legs', 44, 1, 1, 3),
                    ('armor_set', NULL, 900, NULL, 2, 0), ('set_bonus', NULL, 901, 900, 1, 0),
                    ('artifact_perk', 'artifact', 45, NULL, 7, 0)
                 ) c(kind, slot, hash, parent, quantity, position), (VALUES (10), (11), (12), (13), (14)) bs(b);
                 INSERT INTO build_component (build_id, kind, slot, component_hash, parent_hash, quantity, position) VALUES
                    (10, 'weapon', 'energy', 6, NULL, 1, 0), (10, 'weapon_intrinsic', 'energy', 41, 6, 1, 0),
                    (10, 'weapon_perk', 'energy', 40, 6, 1, 1),
                    (11, 'weapon', 'energy', 6, NULL, 1, 0), (11, 'weapon_perk', 'energy', 46, 6, 1, 1),
                    (13, 'weapon', 'energy', 8, NULL, 1, 0), (13, 'weapon_perk', 'energy', 40, 8, 1, 1),
                    (12, 'fragment', 'subclass', 9, NULL, 1, 5),
                    (20, 'armor', 'legs', 2, NULL, 1, 0);
                 INSERT INTO run_score
                 SELECT 1000 * m + i, m, 100, TIMESTAMP '2026-07-01' + INTERVAL (i) DAY, false, false, false,
                        100, 0, 600, 600, 1, 1, 1, 1, CASE WHEN m <= 2 THEN 0.5 ELSE -0.5 END
                 FROM range(1, 5) p(m), range(0, 10) r(i);
                 INSERT INTO run_score (instance_id, membership_id, activity_hash, started_at, is_solo, is_partial,
                     has_build, kills, deaths, duration_s, played_s, kill_index, speed, survival, score, perf) VALUES
                    (1010, 1, 100, '2026-07-11', false, false, true, 100, 0, 600, 600, 1, 1, 1, 1, 1.0),
                    (1011, 1, 100, '2026-07-12', false, false, true, 100, 0, 600, 600, 1, 1, 1, 1, 1.0),
                    (1012, 1, 101, '2026-07-13', false, false, true, 100, 0, 600, 600, 1, 1, 1, 1, 0.6),
                    (1013, 1, 100, '2026-07-14', false, false, true, 100, 0, 600, 600, 1, 1, 1, 1, 0.7),
                    (2013, 2, 100, '2026-07-14', false, false, true, 100, 0, 600, 600, 1, 1, 1, 1, 0.5),
                    (2010, 2, 100, '2026-07-11', false, false, true, 100, 0, 600, 600, 1, 1, 1, 1, 1.0),
                    (2011, 2, 100, '2026-07-12', false, false, true, 100, 0, 600, 600, 1, 1, 1, 1, 1.0),
                    (2012, 2, 100, '2026-07-13', false, false, true, 100, 0, 600, 600, 1, 1, 1, 1, 0.0),
                    (3010, 3, 101, '2026-07-11', false, false, true, 100, 0, 600, 600, 1, 1, 1, 1, -0.5),
                    (3011, 3, 101, '2026-07-12', false, false, true, 100, 0, 600, 600, 1, 1, 1, 1, -0.5),
                    (4010, 4, 101, '2026-07-11', false, false, true, 100, 0, 600, 600, 1, 1, 1, 1, -0.5),
                    (4011, 4, 101, '2026-07-12', false, false, true, 100, 0, 600, 600, 1, 1, 1, 1, -0.5),
                    (9999, 1, 200, '2026-07-01', false, false, false, 100, 0, 600, 600, 1, 1, 1, 1, 0.0);
                 INSERT INTO run_build VALUES
                    (1010, 1, 10, 1, 0, 600, 1.0, true), (1011, 1, 10, 1, 0, 600, 1.0, true),
                    (1012, 1, 11, 1, 0, 600, 1.0, true), (1013, 1, 13, 1, 0, 600, 1.0, true),
                    (2013, 2, 14, 1, 0, 600, 1.0, true),
                    (2010, 2, 10, 1, 0, 600, 1.0, true),
                    (2011, 2, 10, 1, 0, 600, 1.0, true), (2012, 2, 12, 1, 0, 600, 1.0, true),
                    (3010, 3, 20, 1, 0, 600, 1.0, true), (3011, 3, 20, 1, 0, 600, 1.0, true),
                    (4010, 4, 20, 1, 0, 600, 1.0, true), (4011, 4, 20, 1, 0, 600, 1.0, true);",
            )
            .unwrap();
        (dir, db)
    }

    fn export() -> SiteData {
        let (_d, db) = db();
        let opts = SiteOptions {
            min_players: 1,
            ..SiteOptions::default()
        };
        site_data(&db, &opts, |_| {}).unwrap()
    }

    fn row<'a>(data: &'a SiteData, scope: &str, armor: &str) -> &'a Row {
        let s = data.scopes.iter().find(|s| s.id == scope).unwrap();
        s.rows
            .iter()
            .find(|r| data.types[r.0 as usize].armor.as_deref() == Some(armor))
            .unwrap()
    }

    #[test]
    fn scopes_list_single_activity_groups_once() {
        let data = export();
        assert_eq!(data.format, SITE_FORMAT_VERSION);
        let ids: Vec<&str> = data.scopes.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "all",
                "equilibrium",
                "ultimate-conquest",
                "ultimate-conquest/ultimate-conquest-lightblade",
                "ultimate-conquest/ultimate-conquest-operation-seraph-s-shield",
            ]
        );
        let seraph = &data.scopes[4];
        assert_eq!(seraph.name, "Ultimate Conquest: Operation: Seraph's Shield");
        assert_eq!(
            (seraph.kind, seraph.parent.as_deref()),
            ("activity", Some("Ultimate Conquest"))
        );
    }

    #[test]
    fn listings_publish_the_best_exact_build_and_its_variants() {
        let data = export();
        let alpha = row(&data, "all", "Alpha Boots");
        let id = |p: &Pick| data.recipes[p.0 as usize].id.as_str();
        assert_eq!(id(&alpha.7), "10", "the best build of the best family");
        let variants: Vec<&str> = alpha.8.iter().map(id).collect();
        assert_eq!(
            variants,
            ["11"],
            "11 is near-identical; 14 differs by 9 elements; 13 carries another weapon; \
             12 is another family"
        );
        assert_eq!(alpha.9, 0);
        assert!(
            (alpha.10 - 6.0).abs() < 1e-9,
            "the loadout of 10, 11 and 14: six clears"
        );
        assert_eq!(alpha.11, 3, "three exact builds in that loadout");
        assert!(alpha.7.1 > alpha.8[0].1, "listed by effect");
        assert_eq!(
            (alpha.7.2, alpha.7.3),
            (4.0, 2),
            "runs and players of the exact build"
        );

        // In Seraph's Shield only build 11 of the family was used.
        let seraph = row(
            &data,
            "ultimate-conquest/ultimate-conquest-operation-seraph-s-shield",
            "Alpha Boots",
        );
        assert_eq!(id(&seraph.7), "11");
        assert!(seraph.8.is_empty());
    }

    #[test]
    fn recipes_are_exact() {
        let data = export();
        let name = |i: u32| data.dict[i as usize].name.as_str();
        let r = data.recipes.iter().find(|r| r.id == "10").unwrap();
        assert_eq!(r.stats, Some([30, 10, 20, 200, 100, 15]));
        assert_eq!(r.kit.subclass.map(name), Some("Dawnblade"));
        assert_eq!(r.kit.super_.map(name), Some("Well of Radiance"));
        assert_eq!(r.kit.grenade.map(name), Some("Solar Grenade"));
        assert_eq!(
            r.kit.aspects.iter().map(|&i| name(i)).collect::<Vec<_>>(),
            ["Heat Rises"]
        );
        assert_eq!(
            r.kit.fragments.iter().map(|&i| name(i)).collect::<Vec<_>>(),
            ["Ember of Char"]
        );

        let legs = &r.armor[0];
        assert_eq!(
            (legs.slot, name(legs.item), legs.exotic),
            ("legs", "Alpha Boots", true)
        );
        assert_eq!(
            legs.mods.iter().map(|&i| name(i)).collect::<Vec<_>>(),
            ["Hands-On", "Hands-On"]
        );
        assert_eq!(legs.tuning.map(name), Some("+Grenade / -Health"));
        assert_eq!(legs.archetype.map(name), Some("Grenadier"));

        let w = &r.weapons[0];
        assert_eq!((w.slot, name(w.item)), ("energy", "The Mountaintop"));
        assert_eq!(w.frame.map(name), Some("Adaptive Frame"));
        assert_eq!(
            w.perks.iter().map(|&i| name(i)).collect::<Vec<_>>(),
            ["Outlaw"]
        );

        let sets: Vec<(&str, u16, Vec<&str>)> = r
            .sets
            .iter()
            .map(|e| {
                (
                    name(e.set),
                    e.pieces,
                    e.bonuses.iter().map(|&b| name(b)).collect(),
                )
            })
            .collect();
        assert_eq!(
            sets,
            [("Iron Set", 2, vec!["Iron Will"])],
            "bonuses under their set"
        );
        assert_eq!(
            r.artifact.iter().map(|&i| name(i)).collect::<Vec<_>>(),
            ["Elemental Siphon"],
            "a perk is listed once"
        );
        assert_eq!(
            data.dict
                .iter()
                .find(|d| d.name == "Iron Will")
                .unwrap()
                .icon
                .as_deref(),
            Some("/p.png")
        );
        assert_eq!(
            data.icons.get("Alpha Boots").map(String::as_str),
            Some("/a.jpg")
        );

        let variant = data.recipes.iter().find(|r| r.id == "11").unwrap();
        assert_eq!(name(variant.weapons[0].item), "The Mountaintop");
        assert_eq!(
            variant.weapons[0]
                .perks
                .iter()
                .map(|&i| name(i))
                .collect::<Vec<_>>(),
            ["Kill Clip"]
        );
        for unpublished in ["13", "14"] {
            assert!(
                !data.recipes.iter().any(|r| r.id == unpublished),
                "build {unpublished} is neither exemplar nor variant"
            );
        }

        // Aggregates and recipes only: nothing identifies a player.
        let json = serde_json::to_string(&data).unwrap();
        assert!(!json.contains("membership"));
    }

    #[test]
    fn incomplete_builds_are_not_highlighted() {
        // The fixture's builds have one aspect and one fragment.
        let data = export();
        assert!(!data.incomplete.is_empty());
        assert!(
            data.highlights
                .iter()
                .all(|s| s.classes.iter().all(|c| c.picks.is_empty())),
            "{:?}",
            data.highlights
        );
    }

    #[test]
    fn highlights_cover_the_neutral_game_and_every_activity() {
        let (_d, db) = db();
        let mut data = site_data(&db, &SiteOptions::default(), |_| {}).unwrap();
        // Treat the fixture's partial kits as complete to see the picks.
        data.incomplete.clear();
        let vectors = build_vectors(&db).unwrap();
        data.highlights = highlight_sets(&data, &vectors, &cap, QUALITY_WEIGHT);
        let ids: Vec<&str> = data.highlights.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids[0], "neutral");
        assert!(
            !ids.contains(&"all"),
            "the neutral game stands in for all activities"
        );
        assert_eq!(ids.len(), data.scopes.len());
        assert_eq!(
            data.neutral.len(),
            2,
            "both build types have a neutral effect"
        );
        let warlock = &data.highlights[0].classes[0];
        assert_eq!(warlock.class, "Warlock");
        // Alpha and Beta are both Solar Well of Radiance with Heat Rises:
        // the second would only swap the first's exotic armor.
        assert_eq!(warlock.picks.len(), 1, "{:?}", warlock.picks);
        assert!(warlock.picks[0].1 > 0.0, "picks rank above average");
        assert!(
            (warlock.picks[0].7 - 1.0).abs() < 1e-9,
            "the first pick is all new"
        );
        // Without that rule the cap would allow both.
        let cands = crate::highlight::subclass_candidates(&data, "neutral", "Warlock", "Solar");
        assert_eq!(cands.len(), 2);
        let book = split(data);
        assert_eq!(
            book.index.highlights[0],
            HighlightMeta(
                "neutral".into(),
                "Neutral game".into(),
                "highlights.neutral.json".into()
            )
        );
        let (_, file) = &book.highlights[0];
        for pick in &file.classes[0].picks {
            assert!(
                (pick.5 as usize) < file.recipes.len(),
                "picks index the file's own recipes"
            );
        }
    }

    #[test]
    fn the_book_splits_into_an_index_and_parts() {
        let data = export();
        let alpha_type = data
            .types
            .iter()
            .position(|t| t.armor.as_deref() == Some("Alpha Boots"))
            .unwrap() as u32;
        let top = data.scopes[0]
            .rows
            .iter()
            .find(|r| r.0 == alpha_type)
            .map(|r| data.recipes[r.7.0 as usize].id.clone())
            .unwrap();
        let book = split(data);
        let all = &book.index.scopes[0];
        assert_eq!(all.id, "all");
        // Alpha has 2 players, Beta 2: both in the "more" part (1–2 players).
        assert_eq!(
            all.parts,
            [
                PartMeta(3, None, "all.json".into(), 0),
                PartMeta(1, Some(2), "all.more.json".into(), 2)
            ]
        );
        assert_eq!(all.summary.len(), 2);
        let files: Vec<&str> = book.parts.iter().map(|(f, _)| f.as_str()).collect();
        assert!(files.contains(&"all.more.json"));
        assert!(!files.contains(&"all.json"), "empty parts are not written");
        assert!(
            files.iter().any(|f| f.contains("--")),
            "activity ids become file names"
        );
        let (_, part) = book
            .parts
            .iter()
            .find(|(f, _)| f == "all.more.json")
            .unwrap();
        let row = part.rows.iter().find(|r| r.0 == alpha_type).unwrap();
        assert_eq!(
            part.recipes[row.7.0 as usize].id, top,
            "picks index the part's own recipes"
        );
        assert!(row.8.iter().all(|v| (v.0 as usize) < part.recipes.len()));
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
    fn rows_serialize_as_arrays() {
        let row = Row(
            3,
            0.1,
            0.2,
            0.3,
            4,
            5.5,
            "2026-07-01".into(),
            Pick(7, 0.4, 2.0, 1, "2026-07-02".into()),
            vec![],
            0,
            2.0,
            3,
        );
        assert_eq!(
            serde_json::to_string(&row).unwrap(),
            r#"[3,0.1,0.2,0.3,4,5.5,"2026-07-01",[7,0.4,2.0,1,"2026-07-02"],[],0,2.0,3]"#
        );
    }
}
