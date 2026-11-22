//! The "D2 Ability Cooldowns and Factors" workbook (`cooldowns/`), for the
//! build model's energy economy (`docs/mechanics.md`):
//!
//! * the Grenades, Melees and Class Abilities tabs: each ability's cooldown
//!   at 0 stat, its flat-gain factor and its class-mod multiplier;
//! * Supers 2.0 (its CSV export: the sheet hides the tier, cooldown and
//!   multiplier columns, which leaves them out of its HTML view): each
//!   super's cooldown, and its best damage per cast;
//! * Armor 3.0 Stat Info: what each stat gives, every 10 points to 200;
//! * Regen Effects: parts that multiply an ability's regeneration;
//! * Flat Gain Sources: parts that grant a chunk of ability energy.
//!
//! The ability tabs' "T3 Cooldown" is the base before Edge of Fate: it is
//! the T0 cooldown over 1.45 for every ability, where the Armor 3.0 curve
//! gives 1.38 at 30 stat.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::names::{Kind, Names};
use crate::sheet::{Grid, value};

/// One ability's cooldown and factors.
#[derive(Debug, Clone, PartialEq)]
pub struct Ability {
    /// `grenade`, `melee`, `class_ability` or `super`.
    pub slot: &'static str,
    pub name: String,
    /// As the sheet gives it: an element, `Neutral`, `Exotic`, `Stasis Aspect`.
    pub element: String,
    /// Seconds at 0 stat; a super's base cooldown.
    pub cooldown_s: Option<f64>,
    /// The "T3 Cooldown" column of the ability tabs.
    pub pre_eof_base_s: Option<f64>,
    /// The share of a flat (chunk) energy gain the ability receives.
    pub flat_gain_factor: Option<f64>,
    /// Class abilities: the share of a class mod's energy they receive.
    pub class_mod_multiplier: Option<f64>,
    pub super_tier: Option<String>,
    pub hashes: Vec<u32>,
}

/// A row of the Supers tab: a super, often with an exotic, at its best.
#[derive(Debug, Clone, PartialEq)]
pub struct SuperDamage {
    pub element: String,
    pub class: String,
    /// As written: "Golden Gun (Nighthawk, pre-popped)".
    pub name: String,
    /// The manifest super the row is of, where its name resolves.
    pub super_name: Option<String>,
    /// Single-target, Add-Clear, Mix, Support.
    pub usage: Option<String>,
    pub exotic: Option<String>,
    pub damage: Option<f64>,
    pub cast_s: Option<f64>,
    pub dps: Option<f64>,
    pub tier: Option<String>,
    pub cooldown_s: Option<f64>,
    /// The sheet's "Active Multiplier"; what it multiplies is not stated.
    pub active_multiplier: Option<f64>,
}

/// One value of a stat's benefit, at a stat value.
#[derive(Debug, Clone, PartialEq)]
pub struct StatPoint {
    /// `health`, `melee`, `grenade`, `super`, `class`, `weapons`.
    pub stat: String,
    /// `cooldown_scalar`, `energy_gained`, `damage_increase`, ...
    pub effect: String,
    pub stat_value: u16,
    /// Fractions for percentages; 0 where the stat gives nothing.
    pub value: f64,
}

/// A part that speeds an ability's regeneration.
#[derive(Debug, Clone, PartialEq)]
pub struct Regen {
    /// As written: "Arc Soul x2", "Contraverse (Axion)".
    pub written: String,
    pub name: String,
    pub stacks: Option<u8>,
    pub variant: Option<String>,
    /// The sheet's kind: "Arc Aspect", "Hunter Exotic", ...
    pub source_kind: String,
    /// `grenade`, `melee` or `class_ability`.
    pub ability: &'static str,
    /// Extra regeneration over the 0-stat rate (1.6: +160%), at 0 and 100 stat.
    pub bonus_at_0: Option<f64>,
    pub bonus_at_100: Option<f64>,
    /// Before Edge of Fate, over the 30-stat base.
    pub pre_eof: Option<f64>,
    pub hashes: Vec<u32>,
}

/// A part that grants a chunk of ability energy.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatGain {
    pub written: String,
    pub name: String,
    pub variant: Option<String>,
    pub source_kind: String,
    pub ability: &'static str,
    /// A fraction of a charge, before the ability's flat-gain factor.
    pub amount: f64,
    pub note: Option<String>,
    pub hashes: Vec<u32>,
}

/// Everything read from the workbook.
#[derive(Debug, Clone, Default)]
pub struct Workbook {
    pub abilities: Vec<Ability>,
    pub supers: Vec<SuperDamage>,
    pub stat_curve: Vec<StatPoint>,
    pub regen: Vec<Regen>,
    pub flat_gain: Vec<FlatGain>,
    /// Supers whose rows disagree on the cooldown; the most common is kept.
    pub conflicts: Vec<String>,
}

impl Workbook {
    /// Names that resolve to no manifest item.
    #[must_use]
    pub fn unmatched(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .abilities
            .iter()
            .filter(|a| a.hashes.is_empty())
            .map(|a| format!("{} ({})", a.name, a.slot))
            .chain(
                self.regen
                    .iter()
                    .filter(|r| r.hashes.is_empty())
                    .map(|r| r.written.clone()),
            )
            .chain(
                self.flat_gain
                    .iter()
                    .filter(|f| f.hashes.is_empty())
                    .map(|f| f.written.clone()),
            )
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

/// What a part's name in the Regen Effects and Flat Gain Sources tabs can
/// refer to.
const PART_KINDS: [Kind; 14] = [
    Kind::Aspect,
    Kind::Fragment,
    Kind::ExoticArmor,
    Kind::ExoticWeapon,
    Kind::ArmorMod,
    Kind::WeaponPerk,
    Kind::OriginTrait,
    Kind::Intrinsic,
    Kind::ArtifactPerk,
    Kind::ClassAbility,
    Kind::Grenade,
    Kind::Melee,
    Kind::Super,
    Kind::Transcendence,
];

/// The sheet's spellings of manifest names.
const ALIASES: [(&str, &str); 3] = [
    ("Heartof Inmost Light", "Heart of Inmost Light"),
    ("Sunbracer's", "Sunbracers"),
    ("Contraverse", "Contraverse Hold"),
];

fn hashes(names: &Names, name: &str, kinds: &[Kind]) -> Vec<u32> {
    let name = ALIASES
        .iter()
        .find(|(written, _)| *written == name)
        .map_or(name, |(_, manifest)| manifest);
    let mut h: Vec<u32> = names.lookup(name, kinds).iter().map(|s| s.hash).collect();
    h.sort_unstable();
    h.dedup();
    h
}

/// "Arc Soul x2" → ("Arc Soul", 2); "Contraverse (Axion)" → ("Contraverse",
/// "Axion"); an unclosed parenthesis is read as closed.
fn split_name(written: &str) -> (String, Option<u8>, Option<String>) {
    let (base, variant) = match written.split_once(" (") {
        Some((b, v)) => (
            b.trim().to_string(),
            Some(v.trim_end_matches(')').trim().to_string()),
        ),
        None => (written.trim().to_string(), None),
    };
    if let Some((b, n)) = base.rsplit_once(" x")
        && let Ok(n) = n.parse::<u8>()
    {
        return (b.trim().to_string(), Some(n), variant);
    }
    (base, None, variant)
}

/// The row holding a header cell, and each header's first column.
fn header(g: &Grid, marker: &str) -> Option<(usize, BTreeMap<String, usize>)> {
    let wanted = marker.to_ascii_lowercase();
    let row = (0..g.rows()).find(|&r| {
        (0..g.cols()).any(|c| {
            g.text(r, c)
                .is_some_and(|t| t.trim().to_ascii_lowercase() == wanted)
        })
    })?;
    let mut cols = BTreeMap::new();
    for c in 0..g.cols() {
        if let Some(t) = g.text(row, c) {
            cols.entry(t.trim().to_ascii_lowercase()).or_insert(c);
        }
    }
    Some((row, cols))
}

/// An ability tab: Grenades, Melees or Class Abilities.
#[must_use]
pub fn abilities(g: &Grid, slot: &'static str, names: &Names) -> Vec<Ability> {
    let Some((head, cols)) = header(g, "T0 Cooldown") else {
        return Vec::new();
    };
    let col = |k: &str| cols.get(k).copied();
    let name_col = match slot {
        "grenade" => col("grenade"),
        "melee" => col("melee"),
        _ => col("class"),
    };
    let (Some(name_col), Some(element_col)) = (name_col, col("element")) else {
        return Vec::new();
    };
    let kinds: &[Kind] = match slot {
        "grenade" => &[Kind::Grenade, Kind::ExoticArmor, Kind::Aspect],
        "melee" => &[Kind::Melee, Kind::ExoticArmor, Kind::Aspect],
        _ => &[Kind::ClassAbility, Kind::ExoticArmor, Kind::Aspect],
    };
    let num = |r: usize, k: &str| col(k).and_then(|c| g.text(r, c)).and_then(value);
    let mut out = Vec::new();
    for r in head + 1..g.rows() {
        let Some(name) = g.text(r, name_col).map(str::trim) else {
            continue;
        };
        let cooldown = num(r, "t0 cooldown");
        if cooldown.is_none() {
            continue;
        }
        out.push(Ability {
            slot,
            name: name.to_string(),
            element: g.text(r, element_col).unwrap_or("").trim().to_string(),
            cooldown_s: cooldown,
            pre_eof_base_s: num(r, "t3 cooldown"),
            flat_gain_factor: num(r, "flat gain factor"),
            class_mod_multiplier: num(r, "class mod mult"),
            super_tier: None,
            hashes: hashes(names, name, kinds),
        });
    }
    out
}

/// The Supers tab's rows, and each manifest super's cooldown and tier: the
/// most common among its rows.
pub fn supers(g: &Grid, names: &Names) -> (Vec<SuperDamage>, Vec<Ability>, Vec<String>) {
    let Some((head, cols)) = header(g, "Optimal Super Damage") else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let col = |k: &str| cols.get(k).copied();
    let txt = |r: usize, k: &str| {
        col(k)
            .and_then(|c| g.text(r, c))
            .map(|t| t.trim().to_string())
    };
    let num = |r: usize, k: &str| col(k).and_then(|c| g.text(r, c)).and_then(value);
    let mut rows = Vec::new();
    for r in head + 1..g.rows() {
        let (Some(name), Some(class)) = (txt(r, "super"), txt(r, "class")) else {
            continue;
        };
        let (base, _, _) = split_name(&name);
        let super_name = (!hashes(names, &base, &[Kind::Super]).is_empty()).then_some(base);
        rows.push(SuperDamage {
            element: txt(r, "element").unwrap_or_default(),
            class,
            name,
            super_name,
            usage: txt(r, "use"),
            exotic: txt(r, "damage text").map(|e| sort_prefix(&e)),
            damage: num(r, "optimal super damage"),
            cast_s: num(r, "cast time / lock"),
            dps: num(r, "optimal super dps"),
            tier: txt(r, "super tier"),
            cooldown_s: num(r, "t3 cooldown"),
            active_multiplier: num(r, "active multiplier"),
        });
    }
    // One cooldown per super: the most common of its rows'.
    let mut by_super: BTreeMap<&str, Vec<&SuperDamage>> = BTreeMap::new();
    for row in &rows {
        if let Some(s) = &row.super_name {
            by_super.entry(s).or_default().push(row);
        }
    }
    let mut abilities = Vec::new();
    let mut conflicts = Vec::new();
    for (name, rows) in by_super {
        let mut counts: BTreeMap<(i64, Option<&str>), usize> = BTreeMap::new();
        for r in &rows {
            if let Some(cd) = r.cooldown_s {
                let key = (cd.round() as i64, r.tier.as_deref());
                *counts.entry(key).or_default() += 1;
            }
        }
        let Some(((cd, tier), _)) = counts
            .iter()
            .max_by_key(|(k, n)| (**n, std::cmp::Reverse(**k)))
        else {
            continue;
        };
        if counts.len() > 1 {
            let seen: Vec<String> = counts
                .iter()
                .map(|((cd, t), n)| format!("{}s {} ×{n}", cd, t.unwrap_or("?")))
                .collect();
            conflicts.push(format!("{name}: {}", seen.join(", ")));
        }
        #[allow(clippy::cast_precision_loss)]
        abilities.push(Ability {
            slot: "super",
            name: name.to_string(),
            element: rows
                .iter()
                .find(|r| r.element != "Prismatic")
                .unwrap_or(&rows[0])
                .element
                .clone(),
            cooldown_s: Some(*cd as f64),
            pre_eof_base_s: None,
            flat_gain_factor: None,
            class_mod_multiplier: None,
            super_tier: tier.map(String::from),
            hashes: hashes(names, name, &[Kind::Super]),
        });
    }
    (rows, abilities, conflicts)
}

/// The sheet prefixes some names with `y` or `z` to sort them last:
/// "ySpirit of the Star-Eater", "zNone".
fn sort_prefix(s: &str) -> String {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some('y' | 'z'), Some(c)) if c.is_uppercase() => s[1..].to_string(),
        _ => s.to_string(),
    }
}

/// `Cooldown Reduction (Note 3)` → `cooldown_reduction`.
fn effect_key(s: &str) -> String {
    let s = s.split(" (").next().unwrap_or(s);
    let mut out = String::new();
    for ch in s.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

/// The Armor 3.0 Stat Info tab: a row of stat values (0 to 200 by 10), then
/// for each stat (named once, in the first column) its effects by row.
#[must_use]
pub fn stat_curve(g: &Grid) -> Vec<StatPoint> {
    // The header row numbers the stat values; take its run of 0, 10, ... 200.
    let mut found = None;
    'rows: for r in 0..g.rows() {
        for c in 0..g.cols() {
            if g.text(r, c).map(str::trim) == Some("0")
                && g.text(r, c + 1).map(str::trim) == Some("10")
            {
                let mut columns = Vec::new();
                let mut cc = c;
                while let Some(v) = g.text(r, cc).and_then(|t| t.trim().parse::<u16>().ok()) {
                    columns.push((cc, v));
                    cc += 1;
                }
                found = Some((r, c, columns));
                break 'rows;
            }
        }
    }
    let Some((head, first, columns)) = found else {
        return Vec::new();
    };
    let (Some(stat_col), Some(effect_col)) = (first.checked_sub(2), first.checked_sub(1)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut stat: Option<String> = None;
    for r in head + 1..g.rows() {
        if let Some(s) = g.text(r, stat_col) {
            stat = Some(effect_key(s));
        }
        let (Some(stat), Some(effect)) = (&stat, g.text(r, effect_col)) else {
            continue;
        };
        let cells: Vec<Option<&str>> = columns.iter().map(|(c, _)| g.text(r, *c)).collect();
        if !cells.iter().flatten().any(|t| value(t).is_some()) {
            continue;
        }
        for ((_, at), cell) in columns.iter().zip(cells) {
            out.push(StatPoint {
                stat: stat.clone(),
                effect: effect_key(effect),
                stat_value: *at,
                value: cell.and_then(value).unwrap_or(0.0),
            });
        }
    }
    out
}

fn ability_slot(s: &str) -> Option<&'static str> {
    match s.trim().to_ascii_lowercase().as_str() {
        "grenade" => Some("grenade"),
        "melee" => Some("melee"),
        "class" | "class ability" => Some("class_ability"),
        _ => None,
    }
}

/// The Regen Effects tab: one row per part and ability.
#[must_use]
pub fn regen(g: &Grid, names: &Names) -> Vec<Regen> {
    let Some((head, cols)) = header(g, "Effect") else {
        return Vec::new();
    };
    let col = |prefix: &str| {
        cols.iter()
            .find(|(k, _)| k.starts_with(prefix))
            .map(|(_, c)| *c)
    };
    let (Some(effect), Some(source), Some(stat)) = (col("effect"), col("source"), col("stat"))
    else {
        return Vec::new();
    };
    let at_0 = col("relative to 0 stat");
    let at_100 = col("current value at 100");
    let pre = col("pre-eof value");
    let num = |r: usize, c: Option<usize>| c.and_then(|c| g.text(r, c)).and_then(value);
    let mut out = Vec::new();
    for r in head + 1..g.rows() {
        let (Some(written), Some(ability)) = (
            g.text(r, effect).map(str::trim),
            g.text(r, stat).and_then(ability_slot),
        ) else {
            continue;
        };
        let (name, stacks, variant) = split_name(written);
        out.push(Regen {
            written: written.to_string(),
            hashes: hashes(names, &name, &PART_KINDS),
            name,
            stacks,
            variant,
            source_kind: g.text(r, source).unwrap_or("").trim().to_string(),
            ability,
            bonus_at_0: num(r, at_0),
            bonus_at_100: num(r, at_100),
            pre_eof: num(r, pre),
        });
    }
    out
}

/// The Flat Gain Sources tab: per part, the energy it grants each ability.
/// Rows without a number (not yet measured) give nothing.
#[must_use]
pub fn flat_gain(g: &Grid, names: &Names) -> Vec<FlatGain> {
    let Some((head, cols)) = header(g, "Items Affected") else {
        return Vec::new();
    };
    let col = |k: &str| cols.get(k).copied();
    let (Some(item), Some(source)) = (col("items affected"), col("source")) else {
        return Vec::new();
    };
    let notes = col("notes");
    let abilities: Vec<(usize, &'static str)> = [
        ("grenade", "grenade"),
        ("melee", "melee"),
        ("class ability", "class_ability"),
    ]
    .iter()
    .filter_map(|(k, slot)| col(k).map(|c| (c, *slot)))
    .collect();
    let mut out = Vec::new();
    for r in head + 1..g.rows() {
        let Some(written) = g.text(r, item).map(str::trim) else {
            continue;
        };
        let (name, _, variant) = split_name(written);
        let found = hashes(names, &name, &PART_KINDS);
        for (c, ability) in &abilities {
            let Some(amount) = g.text(r, *c).and_then(value) else {
                continue;
            };
            out.push(FlatGain {
                written: written.to_string(),
                name: name.clone(),
                variant: variant.clone(),
                source_kind: g.text(r, source).unwrap_or("").trim().to_string(),
                ability,
                amount,
                note: notes
                    .and_then(|n| g.text(r, n))
                    .map(|t| t.trim().to_string()),
                hashes: found.clone(),
            });
        }
    }
    out
}

/// Read the workbook's tabs from `dir`; a missing tab gives nothing.
pub fn read(dir: &Path, names: &Names) -> Result<Workbook> {
    let grid = |file: &str| -> Result<Option<Grid>> {
        let path = dir.join(file);
        if !path.exists() {
            return Ok(None);
        }
        let html = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Ok(Some(Grid::parse(&html)))
    };
    let mut w = Workbook::default();
    for (file, slot) in [
        ("Grenades.html", "grenade"),
        ("Melees.html", "melee"),
        ("Class_Abilities.html", "class_ability"),
    ] {
        if let Some(g) = grid(file)? {
            w.abilities.extend(abilities(&g, slot, names));
        }
    }
    // The Supers tab hides its tier, cooldown and multiplier columns: only
    // the CSV export has them.
    let supers_csv = dir.join("Supers_2_0.csv");
    if supers_csv.exists() {
        let text = std::fs::read_to_string(&supers_csv)
            .with_context(|| format!("reading {}", supers_csv.display()))?;
        let g =
            Grid::from_csv(&text).with_context(|| format!("parsing {}", supers_csv.display()))?;
        let (rows, abilities, conflicts) = supers(&g, names);
        w.supers = rows;
        w.abilities.extend(abilities);
        w.conflicts = conflicts;
    }
    if let Some(g) = grid("Armor_3_0_Stat_Info.html")? {
        w.stat_curve = stat_curve(&g);
    }
    if let Some(g) = grid("Regen_Effects.html")? {
        w.regen = regen(&g, names);
    }
    if let Some(g) = grid("Flat_Gain_Sources.html")? {
        w.flat_gain = flat_gain(&g, names);
    }
    Ok(w)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(rows: &str) -> Grid {
        Grid::parse(&format!(
            r#"<table class="waffle"><tbody>{rows}</tbody></table>"#
        ))
    }

    fn row(cells: &[&str]) -> String {
        format!("<tr><td>{}</td></tr>", cells.join("</td><td>"))
    }

    #[test]
    fn names_split_into_part_stacks_and_variant() {
        assert_eq!(
            split_name("Arc Soul x2"),
            ("Arc Soul".into(), Some(2), None)
        );
        assert_eq!(
            split_name("Contraverse (Axion)"),
            ("Contraverse".into(), None, Some("Axion".into()))
        );
        assert_eq!(
            split_name("Devour (Feed the Void"),
            ("Devour".into(), None, Some("Feed the Void".into()))
        );
        assert_eq!(split_name("Fr0st-EE5"), ("Fr0st-EE5".into(), None, None));
        assert_eq!(
            effect_key("Cooldown Reduction (Note 3)"),
            "cooldown_reduction"
        );
        assert_eq!(
            effect_key("Primary/Special Boss Damage"),
            "primary_special_boss_damage"
        );
        assert_eq!(
            sort_prefix("ySpirit of the Star-Eater"),
            "Spirit of the Star-Eater"
        );
        assert_eq!(sort_prefix("zNone"), "None");
        assert_eq!(sort_prefix("Synthoceps"), "Synthoceps");
    }

    #[test]
    fn ability_tabs_give_cooldowns_and_factors() {
        let g = page(
            &[
                row(&[
                    "",
                    "Element",
                    "Icon",
                    "Grenade",
                    "T0 Cooldown",
                    "T3 Cooldown",
                    "Flat Gain Factor",
                    "",
                    "Flat Gain Factor",
                ]),
                row(&[
                    "",
                    "Arc",
                    "",
                    "Lightning Grenade",
                    "220s",
                    "152s",
                    "50.0%",
                    "",
                    "62.5%",
                ]),
                row(&[
                    "",
                    "Exotic",
                    "",
                    "Mothkeeper's Wraps",
                    "106s",
                    "73s",
                    "100.0%",
                ]),
                row(&["", "", "", "", "", "", ""]),
            ]
            .concat(),
        );
        let a = abilities(&g, "grenade", &Names::default());
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].name, "Lightning Grenade");
        assert_eq!(a[0].cooldown_s, Some(220.0));
        assert_eq!(a[0].pre_eof_base_s, Some(152.0));
        assert_eq!(a[0].flat_gain_factor, Some(0.5));
        assert_eq!(a[1].element, "Exotic");
    }

    #[test]
    fn the_stat_curve_carries_each_stat_down_its_rows() {
        let g = page(
            &[
                row(&["", "Armor 3.0 Stat Benefits"]),
                row(&["", "", "", "0", "10", "20"]),
                row(&[
                    "",
                    "Melee",
                    "Cooldown Reduction (Note 3)",
                    "---",
                    "-7%",
                    "-17%",
                ]),
                row(&["", "", "Cooldown Scalar (Note 3)", "1.00", "1.07", "1.20"]),
                row(&["", "", "Notes:", "", "", ""]),
                row(&["", "Health", "Health per orb", "+0", "+7", "+14"]),
            ]
            .concat(),
        );
        let c = stat_curve(&g);
        assert_eq!(c.len(), 9);
        let at = |stat: &str, effect: &str, v: u16| {
            c.iter()
                .find(|p| p.stat == stat && p.effect == effect && p.stat_value == v)
                .map(|p| p.value)
        };
        assert_eq!(at("melee", "cooldown_reduction", 0), Some(0.0));
        assert!((at("melee", "cooldown_reduction", 20).unwrap() + 0.17).abs() < 1e-12);
        assert_eq!(at("melee", "cooldown_scalar", 10), Some(1.07));
        assert_eq!(at("health", "health_per_orb", 20), Some(14.0));
    }

    #[test]
    fn regen_and_flat_gains_read_one_row_per_ability() {
        let g = page(
            &[
                row(&[
                    "",
                    "Effect",
                    "Source",
                    "Stat",
                    "Relative to 0 stat Cooldown Current Value at 0 stat",
                    "Current Value at 100 stat",
                    "Current Value vs old base",
                    "Renegades Value vs old base",
                    "Pre-Eof Value (vs 30 stat base)",
                ]),
                row(&[
                    "",
                    "Arc Soul x2",
                    "Arc Aspect",
                    "Class",
                    "32.0%",
                    "72%",
                    "50.4%",
                    "65.0%",
                    "80.0%",
                ]),
                row(&[
                    "",
                    "Spark of Recharge",
                    "Arc Fragment",
                    "Grenade",
                    "160.0%",
                    "360%",
                    "252.0%",
                    "324.8%",
                    "400.0%",
                ]),
            ]
            .concat(),
        );
        let r = regen(&g, &Names::default());
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].name.as_str(), r[0].stacks), ("Arc Soul", Some(2)));
        assert_eq!(r[1].ability, "grenade");
        assert_eq!(r[1].bonus_at_0, Some(1.6));
        assert_eq!(r[1].bonus_at_100, Some(3.6));
        assert_eq!(r[1].pre_eof, Some(4.0));

        let g = page(
            &[
                row(&[
                    "",
                    "Items Affected",
                    "Source",
                    "Grenade",
                    "Melee",
                    "Class Ability",
                    "Notes",
                ]),
                row(&["", "Absolution", "Leg Mod", "5.0%", "5.0%", "5.0%", ""]),
                row(&[
                    "",
                    "Demolitionist (special)",
                    "Weapon Perk",
                    "20.0%",
                    "---",
                    "---",
                    "",
                ]),
                row(&["", "Firesprites", "Elemental Pickup", "", "", "", ""]),
            ]
            .concat(),
        );
        let f = flat_gain(&g, &Names::default());
        assert_eq!(f.len(), 4);
        assert_eq!(f[3].name, "Demolitionist");
        assert_eq!(f[3].variant.as_deref(), Some("special"));
        assert!((f[3].amount - 0.2).abs() < 1e-12);
    }

    #[test]
    fn supers_take_the_most_common_cooldown() {
        let g = page(
            &[
                row(&[
                    "",
                    "Element",
                    "Class",
                    "Icon",
                    "Super",
                    "Use",
                    "Uptime text",
                    "Uptime Exotic",
                    "Damage Text",
                    "Damage Exotic",
                    "Optimal Super Damage",
                    "Cast Time / Lock",
                    "Optimal Super DPS",
                    "Super Tier",
                    "T3 Cooldown",
                    "Active Multiplier",
                ]),
                row(&[
                    "",
                    "Stasis",
                    "Warlock",
                    "",
                    "Winter's Wrath",
                    "Add-Clear",
                    "",
                    "",
                    "Ballidorse Wrathweavers",
                    "",
                    "35,160",
                    "15.25s",
                    "2,306/s",
                    "T2",
                    "556s",
                    "270%",
                ]),
                row(&[
                    "",
                    "Prismatic",
                    "Warlock",
                    "",
                    "Winter's Wrath",
                    "Add-clear",
                    "",
                    "",
                    "ySpirit of the Star-Eater",
                    "",
                    "14,018",
                    "15.25s",
                    "919/s",
                    "T3",
                    "556s",
                    "270%",
                ]),
                row(&[
                    "",
                    "Stasis",
                    "Warlock",
                    "",
                    "Winter's Wrath (other)",
                    "Add-clear",
                    "",
                    "",
                    "zNone",
                    "",
                    "1",
                    "1s",
                    "1/s",
                    "T2",
                    "556s",
                    "270%",
                ]),
            ]
            .concat(),
        );
        let mut names = Names::default();
        names.insert_for_test("Winter's Wrath", 7, Kind::Super);
        let (rows, abilities, conflicts) = supers(&g, &names);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].damage, Some(35_160.0));
        assert_eq!(rows[0].dps, Some(2_306.0));
        assert_eq!(rows[1].exotic.as_deref(), Some("Spirit of the Star-Eater"));
        assert!(
            rows.iter()
                .all(|r| r.super_name.as_deref() == Some("Winter's Wrath"))
        );
        assert_eq!(abilities.len(), 1);
        let a = &abilities[0];
        assert_eq!(
            (a.cooldown_s, a.super_tier.as_deref()),
            (Some(556.0), Some("T2"))
        );
        assert_eq!(
            (a.element.as_str(), a.hashes.as_slice()),
            ("Stasis", &[7][..])
        );
        assert_eq!(conflicts, ["Winter's Wrath: 556s T2 ×2, 556s T3 ×1"]);
    }
}
