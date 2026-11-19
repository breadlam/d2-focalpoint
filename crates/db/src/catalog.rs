//! The catalog: every collected build, exactly as equipped, for the site's
//! "Find any build" view — 594k builds, so it is laid out for a static host.
//!
//! * **Factored.** Players reuse a weapon roll, an armor piece or a kit
//!   across many builds, so builds refer to *parts* — kits, weapon
//!   configurations, armor-piece configurations, artifact perk sets, set
//!   combinations — stored once.
//! * **Sharded.** Build types (class, element, exotics, super, aspects) are
//!   sorted by name and packed into shards of about [`SHARD_BUILDS`] builds,
//!   each with its own part tables, so the browser downloads only the
//!   shards its filters need. A global index lists every type and its
//!   shards.
//! * **Plain text** with base-36 numbers: git compresses it (and delta
//!   compresses refreshes), and GitHub Pages serves it gzipped.
//!
//! The layout is documented in `docs/site.md`. No player names or ids are
//! exported.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use crate::rank::{BuildSort, RankOptions, rank_builds};
use crate::recipe::{ArmorPiece, Dict, NS_ITEM, Recipe, Weapon, components_sql};
use crate::search::GroupBy;
use crate::{Database, Result, meta_key};

/// Version of the text layout; the site checks it.
pub const CATALOG_FORMAT_VERSION: u32 = 1;

/// Builds per shard (a type larger than this spans several shards).
pub const SHARD_BUILDS: usize = 3000;

/// The catalog's files: the index and the shards, in order.
#[derive(Debug)]
pub struct Catalog {
    pub index: String,
    pub shards: Vec<String>,
    pub builds: usize,
    pub types: usize,
}

/// Base-36, the compact number format of the catalog.
fn b36(mut n: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII digits")
}

fn b36i(n: i64) -> String {
    if n < 0 {
        format!("-{}", b36(n.unsigned_abs()))
    } else {
        b36(n.unsigned_abs())
    }
}

fn opt(n: Option<u32>) -> String {
    n.map_or_else(|| "-".into(), |n| b36(u64::from(n)))
}

fn list(items: impl IntoIterator<Item = String>) -> String {
    let joined: Vec<String> = items.into_iter().collect();
    if joined.is_empty() {
        "-".into()
    } else {
        joined.join(",")
    }
}

fn ids(items: &[u32]) -> String {
    list(items.iter().map(|&i| b36(u64::from(i))))
}

/// The short id of a build: its id's low base-36 digits (8 of them).
#[must_use]
pub fn short_id(build_id: i64) -> String {
    #[allow(clippy::cast_sign_loss)]
    let n = build_id as u64 % 36_u64.pow(8);
    let s = b36(n);
    format!("{s:0>8}")
}

/// A build type by name, as the site's filters see it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct TypeKey {
    class: String,
    element: String,
    armor: String,
    weapon: String,
    super_name: String,
    aspects: String,
}

struct BuildInfo {
    id: i64,
    key: TypeKey,
    /// Dictionary entries of the type's names (from the build's hashes).
    names: (Option<u32>, Option<u32>, Option<u32>, Vec<u32>),
    stats: Option<[i32; 6]>,
    runs: i64,
    players: i64,
    clears: i64,
    qualified: i64,
    last_day: Option<i64>,
    effect: Option<f64>,
    activities: Vec<u32>,
    recipe: Recipe,
}

fn u32_list(v: duckdb::types::Value) -> Vec<u32> {
    match v {
        duckdb::types::Value::List(items) => items
            .into_iter()
            .filter_map(|x| match x {
                duckdb::types::Value::UInt(n) => Some(n),
                duckdb::types::Value::Int(n) => u32::try_from(n).ok(),
                duckdb::types::Value::BigInt(n) => u32::try_from(n).ok(),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn load_builds(db: &Database, dict: &mut Dict) -> Result<Vec<BuildInfo>> {
    let mut stmt = db.conn().prepare(
        "SELECT b.build_id, coalesce(b.class, ''), coalesce(b.element, ''),
                coalesce(item_name(b.exotic_armor_hash), ''), coalesce(item_name(b.exotic_weapon_hash), ''),
                coalesce(item_name(b.super_hash), ''),
                coalesce(array_to_string(list_sort(item_names(b.aspect_hashes)), ', '), ''),
                b.exotic_armor_hash, b.exotic_weapon_hash, b.super_hash, b.aspect_hashes,
                b.stat_weapons, b.stat_health, b.stat_class, b.stat_grenade, b.stat_super, b.stat_melee
         FROM build b ORDER BY b.build_id",
    )?;
    let mut out = Vec::new();
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let armor: Option<u32> = r.get(7)?;
        let weapon: Option<u32> = r.get(8)?;
        let super_hash: Option<u32> = r.get(9)?;
        let aspects = u32_list(r.get(10)?);
        let stats: [Option<i32>; 6] = std::array::from_fn(|i| r.get(11 + i).ok().flatten());
        out.push(BuildInfo {
            id: r.get(0)?,
            key: TypeKey {
                class: r.get(1)?,
                element: r.get(2)?,
                armor: r.get(3)?,
                weapon: r.get(4)?,
                super_name: r.get(5)?,
                aspects: r.get(6)?,
            },
            names: (
                armor.map(|h| dict.id(NS_ITEM, h)),
                weapon.map(|h| dict.id(NS_ITEM, h)),
                super_hash.map(|h| dict.id(NS_ITEM, h)),
                aspects.iter().map(|&h| dict.id(NS_ITEM, h)).collect(),
            ),
            stats: stats
                .iter()
                .all(Option::is_some)
                .then(|| stats.map(|s| s.unwrap_or(0))),
            runs: 0,
            players: 0,
            clears: 0,
            qualified: 0,
            last_day: None,
            effect: None,
            activities: Vec::new(),
            recipe: Recipe::default(),
        });
    }
    Ok(out)
}

/// Activities builds were used in, as `(display name, target group)`.
type ActivityNames = Vec<(String, String)>;

/// Activities builds were used in, and the index of each activity hash.
fn activities(db: &Database) -> Result<(ActivityNames, HashMap<u32, u32>)> {
    let mut stmt = db.conn().prepare(
        "SELECT DISTINCT a.activity_hash, coalesce(a.name, 'Unknown activity'), coalesce(a.target_group, '')
         FROM activity a WHERE a.activity_hash IN (SELECT r.activity_hash FROM run r
                                                   WHERE r.instance_id IN (SELECT instance_id FROM run_build))
         ORDER BY 3, 2, 1",
    )?;
    let mut names: Vec<(String, String)> = Vec::new();
    let mut by_name: HashMap<(String, String), u32> = HashMap::new();
    let mut of_hash = HashMap::new();
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let hash: u32 = r.get(0)?;
        let name: String = r.get(1)?;
        let display = name
            .strip_suffix(": Customize")
            .or_else(|| name.strip_suffix(": Standard"))
            .unwrap_or(&name)
            .to_string();
        let key = (display, r.get(2)?);
        let next = names.len() as u32;
        let id = *by_name.entry(key.clone()).or_insert_with(|| {
            names.push(key);
            next
        });
        of_hash.insert(hash, id);
    }
    Ok((names, of_hash))
}

/// Runs, players, clears, qualified clears, last use and activities of
/// every build, all over the runs it was used in.
fn usage(
    db: &Database,
    builds: &mut [BuildInfo],
    pos: &HashMap<i64, usize>,
    of_hash: &HashMap<u32, u32>,
) -> Result<()> {
    let mut stmt = db.conn().prepare(
        "WITH rp AS (
             SELECT instance_id, membership_id, bool_or(is_clear) AS clear
             FROM run_player WHERE has_build GROUP BY ALL
         )
         SELECT rb.build_id,
                count(*),
                count(DISTINCT rb.membership_id),
                count(*) FILTER (WHERE rp.clear),
                count(*) FILTER (WHERE s.instance_id IS NOT NULL),
                (epoch(max(r.started_at)) // 86400)::BIGINT,
                list(DISTINCT r.activity_hash)
         FROM run_build rb
         JOIN run r USING (instance_id)
         LEFT JOIN rp USING (instance_id, membership_id)
         LEFT JOIN run_score s USING (instance_id, membership_id)
         GROUP BY 1",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let Some(&i) = pos.get(&id) else { continue };
        let b = &mut builds[i];
        b.runs = r.get(1)?;
        b.players = r.get(2)?;
        b.clears = r.get(3)?;
        b.qualified = r.get(4)?;
        b.last_day = r.get(5)?;
        let mut acts: Vec<u32> = u32_list(r.get(6)?)
            .into_iter()
            .filter_map(|h| of_hash.get(&h).copied())
            .collect();
        acts.sort_unstable();
        acts.dedup();
        b.activities = acts;
    }
    Ok(())
}

/// Ranked builds: the exact build's effect over all qualified clears.
fn effects(db: &Database, builds: &mut [BuildInfo], pos: &HashMap<i64, usize>) -> Result<()> {
    let opts = RankOptions {
        group_by: GroupBy::BuildId,
        ..RankOptions::default()
    };
    let (_, _, ranked) = rank_builds(db, &opts, 1, BuildSort::Effect)?;
    for r in ranked {
        if let Some(&i) = r.key[0].parse::<i64>().ok().and_then(|id| pos.get(&id)) {
            builds[i].effect = Some(r.effect);
        }
    }
    Ok(())
}

fn recipes(
    db: &Database,
    builds: &mut [BuildInfo],
    pos: &HashMap<i64, usize>,
    dict: &mut Dict,
) -> Result<()> {
    let mut stmt = db.conn().prepare(&components_sql("TRUE"))?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let Some(&i) = pos.get(&id) else { continue };
        let kind: String = r.get(1)?;
        let slot: String = r.get(2)?;
        builds[i]
            .recipe
            .add(dict, &kind, &slot, r.get(3)?, r.get(4)?, r.get(5)?);
    }
    for b in builds.iter_mut() {
        b.recipe.finish();
    }
    Ok(())
}

/// Part tables local to one shard: each distinct line once, by first use.
#[derive(Default)]
struct Parts {
    index: HashMap<String, u32>,
    lines: Vec<String>,
}

impl Parts {
    fn id(&mut self, line: String) -> u32 {
        let next = self.lines.len() as u32;
        *self.index.entry(line.clone()).or_insert_with(|| {
            self.lines.push(line);
            next
        })
    }
}

fn kit_line(r: &Recipe) -> String {
    let k = &r.kit;
    format!(
        "{} {} {} {} {} {} {} {} {}",
        opt(k.subclass),
        opt(k.super_),
        opt(k.class_ability),
        opt(k.movement),
        opt(k.melee),
        opt(k.grenade),
        ids(&k.aspects),
        ids(&k.fragments),
        ids(&k.transcendence)
    )
}

fn weapon_line(w: &Weapon) -> String {
    format!(
        "{} {} {} {} {} {} {}",
        crate::recipe::WEAPON_SLOTS
            .iter()
            .position(|s| *s == w.slot)
            .unwrap_or(0),
        b36(u64::from(w.item)),
        u8::from(w.exotic),
        opt(w.frame),
        opt(w.origin),
        opt(w.mod_),
        ids(&w.perks)
    )
}

fn armor_line(a: &ArmorPiece) -> String {
    format!(
        "{} {} {} {} {} {} {} {}",
        crate::recipe::ARMOR_SLOTS
            .iter()
            .position(|s| *s == a.slot)
            .unwrap_or(0),
        b36(u64::from(a.item)),
        u8::from(a.exotic),
        opt(a.archetype),
        opt(a.tuning),
        ids(&a.stat_mods),
        ids(&a.mods),
        ids(&a.perks)
    )
}

fn sets_line(r: &Recipe) -> String {
    format!(
        "{} {}",
        list(
            r.sets
                .iter()
                .map(|&(s, n)| format!("{}:{}", b36(u64::from(s)), n))
        ),
        ids(&r.set_bonuses)
    )
}

/// The loadout of a build, for ordering similar builds together.
fn loadout_order(r: &Recipe) -> String {
    let mut weapons: Vec<u32> = r.weapons.iter().map(|w| w.item).collect();
    weapons.sort_unstable();
    format!("{} {weapons:?}", kit_line(r))
}

/// An effect in thousandths, or `-` when the build is not ranked.
#[allow(clippy::cast_possible_truncation)]
fn effect_text(effect: Option<f64>) -> String {
    effect.map_or_else(|| "-".into(), |e| b36i((e * 1000.0).round() as i64))
}

/// One shard's text.
fn shard_text(builds: &[&BuildInfo], type_index: &HashMap<TypeKey, u32>) -> String {
    let (mut kits, mut weapons, mut armor, mut artifact, mut sets) = (
        Parts::default(),
        Parts::default(),
        Parts::default(),
        Parts::default(),
        Parts::default(),
    );
    let mut lines = Vec::with_capacity(builds.len());
    for b in builds {
        let r = &b.recipe;
        let kit = kits.id(kit_line(r));
        let w: Vec<String> = r
            .weapons
            .iter()
            .map(|x| b36(u64::from(weapons.id(weapon_line(x)))))
            .collect();
        let a: Vec<String> = r
            .armor
            .iter()
            .map(|x| b36(u64::from(armor.id(armor_line(x)))))
            .collect();
        let art = artifact.id(ids(&r.artifact));
        let set = sets.id(sets_line(r));
        let stats = b.stats.map_or_else(
            || "-".to_string(),
            |s| list(s.iter().map(|&v| b36i(i64::from(v)))),
        );
        lines.push(format!(
            "{} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
            short_id(b.id),
            b36(u64::from(type_index[&b.key])),
            b36(u64::from(kit)),
            list(w),
            list(a),
            b36(u64::from(art)),
            b36(u64::from(set)),
            stats,
            b36i(b.runs),
            b36i(b.players),
            b36i(b.clears),
            b36i(b.qualified),
            b.last_day.map_or_else(|| "-".into(), b36i),
            effect_text(b.effect),
            list(b.activities.iter().map(|&x| b36(u64::from(x))))
        ));
    }
    let mut out = format!("focalpoint-shard {CATALOG_FORMAT_VERSION}\n");
    for (name, parts) in [
        ("kits", &kits.lines),
        ("weapons", &weapons.lines),
        ("armor", &armor.lines),
        ("artifact", &artifact.lines),
        ("sets", &sets.lines),
        ("builds", &lines),
    ] {
        let _ = writeln!(out, "@{name}");
        for l in parts {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

/// Build the catalog. `progress` is called with each step's name.
pub fn catalog(db: &Database, mut progress: impl FnMut(&str)) -> Result<Catalog> {
    let mut dict = Dict::default();
    progress("catalog: builds");
    let mut builds = load_builds(db, &mut dict)?;
    let pos: HashMap<i64, usize> = builds.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
    let (activity_names, of_hash) = activities(db)?;
    progress("catalog: usage");
    usage(db, &mut builds, &pos, &of_hash)?;
    progress("catalog: effects");
    effects(db, &mut builds, &pos)?;
    progress("catalog: recipes");
    recipes(db, &mut builds, &pos, &mut dict)?;

    // Types by name; builds by type, then loadout, then rank.
    let mut by_type: BTreeMap<TypeKey, Vec<&BuildInfo>> = BTreeMap::new();
    for b in &builds {
        by_type.entry(b.key.clone()).or_default().push(b);
    }
    let order: HashMap<i64, String> = builds
        .iter()
        .map(|b| (b.id, loadout_order(&b.recipe)))
        .collect();
    for list in by_type.values_mut() {
        list.sort_by(|a, b| {
            order[&a.id]
                .cmp(&order[&b.id])
                .then(
                    b.effect
                        .unwrap_or(f64::NEG_INFINITY)
                        .total_cmp(&a.effect.unwrap_or(f64::NEG_INFINITY)),
                )
                .then(b.clears.cmp(&a.clears))
                .then(b.runs.cmp(&a.runs))
                .then(a.id.cmp(&b.id))
        });
    }
    let type_index: HashMap<TypeKey, u32> = by_type
        .keys()
        .enumerate()
        .map(|(i, k)| (k.clone(), i as u32))
        .collect();

    progress("catalog: shards");
    let mut shards: Vec<Vec<&BuildInfo>> = vec![Vec::new()];
    let mut type_shards: Vec<Vec<usize>> = Vec::with_capacity(by_type.len());
    for list in by_type.values() {
        let current = shards.len() - 1;
        if !shards[current].is_empty() && shards[current].len() + list.len() > SHARD_BUILDS {
            shards.push(Vec::new());
        }
        let mut mine = Vec::new();
        for chunk in list.chunks(SHARD_BUILDS) {
            let current = shards.len() - 1;
            if shards[current].len() + chunk.len() > SHARD_BUILDS {
                shards.push(Vec::new());
            }
            let current = shards.len() - 1;
            shards[current].extend_from_slice(chunk);
            if mine.last() != Some(&current) {
                mine.push(current);
            }
        }
        type_shards.push(mine);
    }
    let shard_texts: Vec<String> = shards.iter().map(|s| shard_text(s, &type_index)).collect();

    // The index: metadata, names, activities, types.
    let resolved = dict.resolve(db)?;
    let (first, last): (Option<String>, Option<String>) = db.conn().query_row(
        "SELECT strftime(min(r.started_at), '%Y-%m-%d'), strftime(max(r.started_at), '%Y-%m-%d')
         FROM run r WHERE r.instance_id IN (SELECT instance_id FROM run_build)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut index = format!("focalpoint-catalog {CATALOG_FORMAT_VERSION}\n@meta\n");
    for (k, v) in [
        (
            "imported_at",
            db.meta(meta_key::IMPORTED_AT)?.unwrap_or_default(),
        ),
        ("first_run", first.unwrap_or_default()),
        ("last_run", last.unwrap_or_default()),
        ("icon_base", "https://www.bungie.net".to_string()),
        ("builds", builds.len().to_string()),
        ("shards", shard_texts.len().to_string()),
    ] {
        let _ = writeln!(index, "{k}\t{v}");
    }
    index.push_str("@dict\n");
    for (name, icon) in &resolved {
        let _ = writeln!(index, "{name}\t{}", icon.as_deref().unwrap_or(""));
    }
    index.push_str("@activities\n");
    for (name, group) in &activity_names {
        let _ = writeln!(index, "{name}\t{group}");
    }
    index.push_str("@types\n");
    for ((key, list), shard_ids) in by_type.iter().zip(&type_shards) {
        let first = list[0];
        let mut acts: Vec<u32> = list
            .iter()
            .flat_map(|b| b.activities.iter().copied())
            .collect();
        acts.sort_unstable();
        acts.dedup();
        let _ = writeln!(
            index,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            key.class,
            key.element,
            opt(first.names.0),
            opt(first.names.1),
            opt(first.names.2),
            ids(&first.names.3),
            b36(list.len() as u64),
            b36i(list.iter().map(|b| b.runs).sum()),
            b36i(list.iter().map(|b| b.players).sum()),
            list_of(shard_ids.iter().map(|&s| s as u64)),
            ids(&acts)
        );
    }
    Ok(Catalog {
        index,
        builds: builds.len(),
        types: by_type.len(),
        shards: shard_texts,
    })
}

fn list_of(items: impl IntoIterator<Item = u64>) -> String {
    list(items.into_iter().map(b36))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_build_is_catalogued_with_its_parts() {
        let (_d, db) = crate::site::tests::db();
        // The catalog reads runs and run players, which the book does not.
        db.conn()
            .execute_batch(
                "INSERT INTO run (instance_id, started_at, activity_hash, source, is_tracked)
                 SELECT DISTINCT instance_id, started_at, activity_hash, 'archive', true FROM run_score;
                 INSERT INTO run_player (instance_id, membership_id, character_id, started_at, activity_hash,
                     kills, deaths, assists, precision_kills, super_kills, melee_kills, grenade_kills,
                     ability_kills, completed, is_clear, has_build)
                 SELECT instance_id, membership_id, membership_id, started_at, activity_hash,
                        kills, 0, 0, 0, 0, 0, 0, 0, true, true, has_build FROM run_score;",
            )
            .unwrap();
        let cat = catalog(&db, |_| {}).unwrap();
        assert_eq!((cat.builds, cat.types, cat.shards.len()), (6, 2, 1));
        assert!(cat.index.starts_with("focalpoint-catalog 1\n"));
        let section = |text: &str, name: &str| -> Vec<String> {
            text.split(&format!("@{name}\n"))
                .nth(1)
                .unwrap()
                .lines()
                .take_while(|l| !l.starts_with('@'))
                .map(String::from)
                .collect()
        };
        let dict = section(&cat.index, "dict");
        let name = |id: &str| {
            dict[usize::from_str_radix(id, 36).unwrap()]
                .split('\t')
                .next()
                .unwrap()
                .to_string()
        };
        let types = section(&cat.index, "types");
        assert_eq!(types.len(), 2);
        let alpha: Vec<&str> = types[0].split('\t').collect();
        assert_eq!(
            (alpha[0], alpha[1], name(alpha[2]).as_str()),
            ("Warlock", "Solar", "Alpha Boots")
        );
        assert_eq!((alpha[6], alpha[9]), ("5", "0"), "five builds, in shard 0");

        let shard = &cat.shards[0];
        let builds = section(shard, "builds");
        assert_eq!(builds.len(), 6);
        let ten = builds
            .iter()
            .find(|l| l.starts_with(&short_id(10)))
            .unwrap();
        let f: Vec<&str> = ten.split(' ').collect();
        let stats: Vec<i64> = f[7]
            .split(',')
            .map(|x| i64::from_str_radix(x, 36).unwrap())
            .collect();
        assert_eq!(stats, [30, 10, 20, 200, 100, 15]);
        let armor = &section(shard, "armor")[usize::from_str_radix(f[4], 36).unwrap()];
        let a: Vec<&str> = armor.split(' ').collect();
        assert_eq!(
            (a[0], name(a[1]).as_str(), a[2]),
            ("3", "Alpha Boots", "1"),
            "legs, exotic"
        );
        let mods: Vec<String> = a[6].split(',').map(name).collect();
        assert_eq!(mods, ["Hands-On", "Hands-On"]);
        let sets = &section(shard, "sets")[usize::from_str_radix(f[6], 36).unwrap()];
        let (set, bonus) = sets.split_once(' ').unwrap();
        assert_eq!(
            (
                name(set.split(':').next().unwrap()).as_str(),
                set.split(':').nth(1)
            ),
            ("Iron Set", Some("2"))
        );
        assert_eq!(name(bonus), "Iron Will");
        assert_eq!(
            (f[8], f[9], f[10], f[11]),
            ("4", "2", "4", "4"),
            "four runs by two players, all qualified clears"
        );
        assert_ne!(f[13], "-", "build 10 is ranked");

        // Deterministic: a second export is identical.
        let again = catalog(&db, |_| {}).unwrap();
        assert_eq!((cat.index, cat.shards), (again.index, again.shards));
    }

    #[test]
    fn base36() {
        assert_eq!(b36(0), "0");
        assert_eq!(b36(35), "z");
        assert_eq!(b36(36), "10");
        assert_eq!(b36i(-37), "-11");
        assert_eq!(short_id(36_i64.pow(8) + 5), "00000005");
        assert_eq!(short_id(-1).len(), 8);
        assert_eq!(list(Vec::<String>::new()), "-");
        assert_eq!(ids(&[1, 36]), "1,10");
    }
}
