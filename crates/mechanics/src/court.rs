//! Court's "Destiny 2 Damage Buffs/Debuffs" stacking guide (`court/`): every
//! damage buff and debuff by the guide's category, which decides how it
//! stacks (`docs/mechanics.md`). Global debuffs, empowering buffs and
//! weapon surges take only the highest of their category; everything else
//! multiplies.
//!
//! The guide is one tab of blocks under "Category: …" headings. A block is
//! either a table (name, trigger, `PvE` and `PvP` values and durations,
//! notes) or a list of names in columns. The exotic intrinsics, weapon
//! perks and world modifiers are only listed; their values are in the
//! Compendium. `PvP` values are not read.

use std::path::Path;

use anyhow::{Context, Result};
use regex::Regex;

use crate::names::{Kind, Names};
use crate::sheet::{Grid, value};

/// One buff or debuff, or one way of getting it.
#[derive(Debug, Clone, PartialEq)]
pub struct Buff {
    /// `global_debuff`, `empowering`, `surge`, `amplification`, `exotic`,
    /// `weapon_perk` or `world`, as in the sandbox vocabulary.
    pub category: &'static str,
    /// The guide's grouping within the category: "Weaken sandbox sources".
    pub section: Option<String>,
    pub name: String,
    /// What the guide says it is: "Void Hunter Super", "Titan Helmet".
    pub kind: Option<String>,
    pub detail: Option<String>,
    pub requirement: Option<String>,
    /// None for the guide's name lists.
    pub trigger: Option<String>,
    /// The largest `PvE` value the cell gives.
    pub pve: Option<f64>,
    pub pve_text: Option<String>,
    /// Values by stack, where the cell is a column of percentages.
    pub steps: Vec<f64>,
    pub duration_s: Option<f64>,
    pub duration_text: Option<String>,
    pub notes: Option<String>,
    pub introduced: Option<String>,
    pub hashes: Vec<u32>,
}

/// The guide's category headings, as vocabulary names.
fn category(heading: &str) -> Option<&'static str> {
    let h = heading.trim().to_ascii_lowercase();
    let h = h.strip_prefix("category:")?.trim();
    let h = h.lines().next().unwrap_or("").trim();
    Some(match h {
        "global debuffs" => "global_debuff",
        "empowering buffs" => "empowering",
        "weapon surges" => "surge",
        "amplification modifiers" => "amplification",
        "exotic intrinsics" => "exotic",
        "weapon perks" => "weapon_perk",
        "world & activity" => "world",
        _ => return None,
    })
}

/// What a guide name can refer to.
const KINDS: [Kind; 17] = [
    Kind::ExoticArmor,
    Kind::ExoticWeapon,
    Kind::Intrinsic,
    Kind::ArmorMod,
    Kind::ArtifactPerk,
    Kind::Super,
    Kind::ClassAbility,
    Kind::Grenade,
    Kind::Melee,
    Kind::Aspect,
    Kind::Fragment,
    Kind::WeaponPerk,
    Kind::OriginTrait,
    Kind::WeaponMod,
    Kind::Transcendence,
    Kind::SetBonus,
    Kind::Movement,
];

/// A name cell: "Name⏎Kind" or "Name⏎Qualifier⏎Kind⏎⏎Details". Footnote
/// marks (`*`, `^`) are dropped from the name.
fn name_cell(text: &str) -> (String, Option<String>, Option<String>) {
    let text = text.replace('\r', "");
    let (head, details) = match text.split_once("\n\n") {
        Some((h, d)) => (h.to_string(), Some(d.trim().to_string())),
        None => (text.clone(), None),
    };
    let lines: Vec<&str> = head
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let name = lines
        .first()
        .map(|l| l.trim_end_matches(['*', '^', ' ']).to_string())
        .unwrap_or_default();
    let kind = (lines.len() > 1).then(|| lines[lines.len() - 1].to_string());
    let middle = (lines.len() > 2).then(|| lines[1..lines.len() - 1].join(" "));
    let detail = match (middle, details) {
        (Some(m), Some(d)) => Some(format!("{m}\n{d}")),
        (m, d) => m.or(d).filter(|s| !s.is_empty()),
    };
    (name, kind, detail)
}

/// The guide's names for manifest items it does not name exactly.
const ALIASES: [(&str, &str); 3] = [
    ("Deadfall Tether", "Shadowshot: Deadfall"),
    ("Moebius Tether", "Shadowshot: Moebius Quiver"),
    ("Banner Shield", "Sentinel Shield"),
];

fn lookup(names: &Names, name: &str, detail: Option<&str>) -> Vec<u32> {
    let mut candidates = vec![name.to_string()];
    if let Some((_, manifest)) = ALIASES.iter().find(|(n, _)| *n == name) {
        candidates.push((*manifest).to_string());
    }
    if let Some(stem) = name.strip_suffix("'s") {
        candidates.push(stem.to_string());
    }
    // "Kinetic Surge Mod" is the manifest's "Kinetic Weapon Surge".
    if let Some(element) = name.strip_suffix(" Surge Mod") {
        candidates.push(format!("{element} Weapon Surge"));
    }
    // "Blessing of the Sky (Boots of the Assembler)": the item in brackets.
    if let Some((_, inner)) = name.split_once(" (") {
        candidates.push(inner.trim_end_matches(')').to_string());
    }
    if let Some(d) = detail {
        candidates.extend(
            d.lines()
                .next()
                .map(|l| l.trim().trim_matches(['(', ')']).to_string()),
        );
    }
    let mut h: Vec<u32> = candidates
        .iter()
        .find_map(|c| {
            let found = names.lookup(c, &KINDS);
            (!found.is_empty()).then_some(found)
        })
        .unwrap_or_default()
        .iter()
        .map(|s| s.hash)
        .collect();
    h.sort_unstable();
    h.dedup();
    h
}

/// The percentages in a value cell, and the steps when the cell is only a
/// column of percentages ("10%⏎17%⏎22%⏎25%").
fn percentages(text: &str) -> (Option<f64>, Vec<f64>) {
    let pct = Regex::new(r"(\d+(?:\.\d+)?)%").expect("valid regex");
    let all: Vec<f64> = pct
        .captures_iter(text)
        .filter_map(|c| c[1].parse::<f64>().ok())
        .map(|x| x / 100.0)
        .collect();
    let max = all
        .iter()
        .copied()
        .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x))));
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let column = lines.len() > 1
        && lines
            .iter()
            .all(|l| value(l.trim_start_matches(['~', '+']).trim_end_matches('*')).is_some());
    (max, if column { all } else { Vec::new() })
}

fn clean(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty() && s != "-" && s != "^").then(|| s.to_string())
}

/// The table columns of a header row: NAME, TRIGGER and the rest.
#[derive(Debug, Clone, Copy)]
struct Table {
    requirement: Option<usize>,
    name: usize,
    trigger: usize,
    pve: Option<usize>,
    pve_duration: Option<usize>,
    notes: [Option<usize>; 3],
    introduced: Option<usize>,
}

enum Block {
    None,
    /// A table, and the section label found in its first column.
    Table(Table, Option<String>),
    /// The name columns of a list.
    List(Vec<usize>),
}

/// Read the guide's grid.
#[must_use]
pub fn buffs(g: &Grid, names: &Names) -> Vec<Buff> {
    let mut out = Vec::new();
    let mut cat: Option<&'static str> = None;
    let mut block = Block::None;
    for r in 0..g.rows() {
        let anchored: Vec<(usize, &str)> = (0..g.cols())
            .filter_map(|c| g.text(r, c).map(|t| (c, t.trim())))
            .collect();
        if let Some(c) = anchored.iter().find_map(|(_, t)| category(t)) {
            cat = Some(c);
            block = Block::None;
            continue;
        }
        // The worked examples after the last category are not entries.
        if anchored
            .iter()
            .any(|(_, t)| t.starts_with("Examples & Scenarios"))
        {
            cat = None;
            block = Block::None;
            continue;
        }
        let Some(category) = cat else { continue };
        let col_of = |label: &str| {
            anchored
                .iter()
                .find(|(_, t)| t.eq_ignore_ascii_case(label))
                .map(|(c, _)| *c)
        };
        let name_cols: Vec<usize> = anchored
            .iter()
            .filter(|(_, t)| t.eq_ignore_ascii_case("NAME"))
            .map(|(c, _)| *c)
            .collect();
        if !name_cols.is_empty() {
            block = match (col_of("TRIGGER"), name_cols.first()) {
                (Some(trigger), Some(&name)) => Block::Table(
                    Table {
                        requirement: col_of("FOUND/REQUIREMENT"),
                        name,
                        trigger,
                        pve: col_of("PVE %"),
                        pve_duration: col_of("PVE #s"),
                        notes: [
                            col_of("% / # NOTES"),
                            col_of("MISC NOTES"),
                            col_of("OTHER NOTES"),
                        ],
                        introduced: col_of("INTRODUCED"),
                    },
                    block_label(g, r),
                ),
                _ => Block::List(name_cols),
            };
            continue;
        }
        match &block {
            Block::None => {}
            Block::Table(t, label) => {
                if let Some(b) = table_row(g, r, t, label.as_deref(), category, names) {
                    out.push(b);
                }
            }
            Block::List(cols) => {
                for &c in cols {
                    let Some(text) = g.text(r, c) else { continue };
                    let Some((name, kind, detail)) = list_cell(names, text) else {
                        continue;
                    };
                    let note = g.text(r, c + 2).and_then(clean);
                    out.push(Buff {
                        category,
                        section: None,
                        hashes: lookup(names, &name, detail.as_deref()),
                        name,
                        kind,
                        detail,
                        requirement: None,
                        trigger: None,
                        pve: None,
                        pve_text: None,
                        steps: Vec::new(),
                        duration_s: None,
                        duration_text: None,
                        notes: note,
                        introduced: None,
                    });
                }
            }
        }
    }
    out
}

/// A cell of a name list: "Name⏎Kind", or a name the sheet wrapped
/// ("Armor-Piercing⏎Rounds"). Labels in capitals ("HEAVY SLOT") and notes
/// are not names.
fn list_cell(names: &Names, text: &str) -> Option<(String, Option<String>, Option<String>)> {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.is_empty() || joined.len() > 60 || !joined.chars().any(char::is_lowercase) {
        return None;
    }
    if !names.lookup(&joined, &KINDS).is_empty() {
        return Some((joined, None, None));
    }
    let cell = name_cell(text);
    (!cell.0.is_empty()).then_some(cell)
}

/// A section label: its lines up to the first blank one ("POWERFUL WEAKEN",
/// not the "Scroll down for…" under it).
fn label(text: &str) -> Option<String> {
    let head: Vec<&str> = text
        .lines()
        .map(str::trim)
        .take_while(|l| !l.is_empty())
        .collect();
    (!head.is_empty()).then(|| head.join(" "))
}

/// The first label in the first column of the table under the header row
/// `head`, up to the next header or category heading. A label is centred on
/// its block, so it need not cover the rows above it.
fn block_label(g: &Grid, head: usize) -> Option<String> {
    for r in head + 1..g.rows() {
        let ends = (0..g.cols()).any(|c| {
            g.text(r, c)
                .is_some_and(|t| t.trim().eq_ignore_ascii_case("NAME") || category(t).is_some())
        });
        if ends {
            return None;
        }
        if let Some(t) = g.text(r, 0).and_then(label) {
            return Some(t);
        }
    }
    None
}

/// A table row: a name anchored in the name column on this row, or a
/// further trigger of the name above (its name cell covering this row).
fn table_row(
    g: &Grid,
    r: usize,
    t: &Table,
    block_label: Option<&str>,
    category: &'static str,
    names: &Names,
) -> Option<Buff> {
    let (nr, nc, name_text) = g.covering(r, t.name)?;
    if nc != t.name {
        return None;
    }
    let own = |c: usize| g.text(r, c).is_some();
    if nr != r && !own(t.trigger) && !t.pve.is_some_and(own) {
        return None;
    }
    // Values a merged cell shares with the rows below it read on each row.
    let at = |c: Option<usize>| c.and_then(|c| g.covering(r, c)).map(|(_, _, s)| s);
    let (name, kind, detail) = name_cell(name_text);
    if name.is_empty() {
        return None;
    }
    let pve_text = at(t.pve).and_then(clean);
    let (pve, steps) = pve_text.as_deref().map_or((None, Vec::new()), percentages);
    let duration_text = at(t.pve_duration).and_then(clean);
    let duration_s = duration_text
        .as_deref()
        .and_then(|d| d.lines().next())
        .and_then(|l| value(l.trim_end_matches('*')));
    let notes: Vec<String> = t
        .notes
        .iter()
        .filter_map(|c| at(*c).and_then(clean))
        .collect();
    // The label covering the row, or else the block's.
    let section = g
        .covering(r, 0)
        .and_then(|(_, _, s)| label(s))
        .or_else(|| block_label.map(String::from));
    Some(Buff {
        category,
        section,
        hashes: lookup(names, &name, detail.as_deref()),
        name,
        kind,
        detail,
        requirement: t
            .requirement
            .into_iter()
            .flat_map(|c| [c, c + 1])
            .find_map(|c| g.text(r, c).and_then(clean)),
        trigger: at(Some(t.trigger)).and_then(clean),
        pve,
        pve_text,
        steps,
        duration_s,
        duration_text,
        notes: (!notes.is_empty()).then(|| notes.join("\n")),
        introduced: at(t.introduced).and_then(clean),
    })
}

/// Read the stacking guide from `dir`; a missing tab gives nothing.
pub fn read(dir: &Path, names: &Names) -> Result<Vec<Buff>> {
    let path = dir.join("STACKING_GUIDE.html");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let html =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(buffs(&Grid::parse(&html), names))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[&str]) -> String {
        format!("<tr><td>{}</td></tr>", cells.join("</td><td>"))
    }

    #[test]
    fn name_cells_split_into_name_kind_and_detail() {
        assert_eq!(
            name_cell("Buried Bloodline*\nSpecial Sidearm\n\n*Catalyst"),
            (
                "Buried Bloodline".into(),
                Some("Special Sidearm".into()),
                Some("*Catalyst".into())
            )
        );
        assert_eq!(
            name_cell("Lumina's\nBlessing of the Sky\nHand Cannon"),
            (
                "Lumina's".into(),
                Some("Hand Cannon".into()),
                Some("Blessing of the Sky".into())
            )
        );
        assert_eq!(name_cell("Bipod"), ("Bipod".into(), None, None));
        let mut names = Names::default();
        names.insert_for_test("Armor-Piercing Rounds", 1, Kind::WeaponPerk);
        assert_eq!(
            list_cell(&names, "Armor-Piercing\nRounds"),
            Some(("Armor-Piercing Rounds".into(), None, None))
        );
        assert_eq!(list_cell(&names, "HEAVY SLOT"), None);
        assert_eq!(
            list_cell(&names, "Athrys's Embrace\nHunter Arms").map(|c| c.1),
            Some(Some("Hunter Arms".into()))
        );
        names.insert_for_test("Kinetic Weapon Surge", 2, Kind::ArmorMod);
        assert_eq!(lookup(&names, "Kinetic Surge Mod", None), [2]);
    }

    #[test]
    fn value_cells_give_the_largest_and_their_steps() {
        assert_eq!(percentages("35%"), (Some(0.35), vec![]));
        let (max, steps) = percentages("10%\n17%\n22%\n25%");
        assert_eq!(max, Some(0.25));
        assert_eq!(steps, vec![0.10, 0.17, 0.22, 0.25]);
        assert_eq!(percentages("20%*\nat max").1, Vec::<f64>::new());
        assert_eq!(percentages("tbc"), (None, vec![]));
    }

    #[test]
    fn tables_and_lists_read_under_their_category() {
        let rows = [
            row(&["", "", "Category: Global Debuffs\n\nOverview"]),
            row(&["", "", "ICON", "FOUND/REQUIREMENT", "", "NAME", "", "TRIGGER", "", "PVE %", "PVE #s", "PVP %", "PVP #s", "% / # NOTES", "", "", "MISC NOTES", "", "", "OTHER NOTES", "", "INTRODUCED"]),
            "<tr><td rowspan=\"2\">WEAKEN SANDBOX SOURCES</td><td></td><td></td><td></td><td></td><td>Snare Bomb\nVoid Hunter Melee</td><td></td><td>Smoke Bomb damage applies Weaken</td><td></td><td>15%</td><td>8s</td><td>7.5%</td><td>2.5s</td><td>-</td><td></td><td></td><td>-</td><td></td><td></td><td></td><td></td><td>S16</td></tr>"
                .to_string(),
            row(&["", "", "", "", "Kinetic Surge Mod\nArmor Mod", "", "Adds 1x Surge stack", "", "10%\n17%\n22%", "~", "3%", "~"]),
            row(&["", "", "Category: Weapon Perks"]),
            row(&["", "", "ICON", "NAME", "", "", "", "ICON", "NAME"]),
            row(&["", "", "", "Bipod", "", "-25%", "", "", "Frenzy"]),
        ]
        .concat();
        let g = Grid::parse(&format!(
            r#"<table class="waffle"><tbody>{rows}</tbody></table>"#
        ));
        let b = buffs(&g, &Names::default());
        let names: Vec<&str> = b.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(
            names,
            ["Snare Bomb", "Kinetic Surge Mod", "Bipod", "Frenzy"]
        );
        let snare = &b[0];
        assert_eq!(snare.category, "global_debuff");
        assert_eq!(snare.section.as_deref(), Some("WEAKEN SANDBOX SOURCES"));
        assert_eq!(snare.kind.as_deref(), Some("Void Hunter Melee"));
        assert_eq!(snare.pve, Some(0.15));
        assert_eq!(snare.duration_s, Some(8.0));
        assert_eq!(snare.notes, None);
        assert_eq!(snare.introduced.as_deref(), Some("S16"));
        // The section label is merged down over the next row.
        let surge = &b[1];
        assert_eq!(surge.section.as_deref(), Some("WEAKEN SANDBOX SOURCES"));
        assert_eq!(surge.steps, vec![0.10, 0.17, 0.22]);
        assert_eq!(surge.duration_s, None);
        assert_eq!(b[2].category, "weapon_perk");
        assert_eq!(b[2].notes.as_deref(), Some("-25%"));
        assert_eq!(b[3].trigger, None);
    }
}
