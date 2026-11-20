//! The Destiny Data Compendium (a Google Sheet; one saved `htmlview/sheet`
//! page per tab): numeric, community-tested descriptions of perks, mods,
//! set bonuses, abilities, exotics and game mechanics, keyed by name.
//!
//! Tabs are laid out for reading, each its own way, so entries are found
//! by their names. A cell is a **name** when its first line (or the part
//! after a `|`, as in "2 Piece | Force Absorption") is the manifest name
//! of a kind the tab lists ([`Tab::kinds`]); fragments are also listed by
//! their short name on their element's tab ("Ashes"). A short cell that is
//! not a manifest name but has a long text beside it is a name too, with
//! no subject: the glossary's game states (Radiant, Scorch, ...) and the
//! game-mechanics tab's topics.
//!
//! An entry's text is the rest of its name cell, then the cells beside it
//! in the tab's reading direction ([`Look`]) over the rows the name spans,
//! up to the next name or the next column where the tab's names start;
//! failing that, the cell below it. A name cell whose own further lines
//! are long carries its text itself (set bonuses).

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::names::{Kind, Names, Subject};
use crate::sheet::Grid;
use crate::{Entry, SubjectRef};

/// Where a tab puts an entry's text relative to its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Right,
    Left,
}

/// A tab read: its file (from `manifest.json`'s `file`), the kinds of
/// manifest names it lists, its element (for fragments' short names) and
/// reading direction.
#[derive(Debug, Clone, Copy)]
pub struct Tab {
    pub file: &'static str,
    pub section: &'static str,
    pub kinds: &'static [Kind],
    pub element: Option<&'static str>,
    pub look: Look,
}

const SUBCLASS: &[Kind] = &[
    Kind::Aspect,
    Kind::Fragment,
    Kind::Super,
    Kind::Grenade,
    Kind::Melee,
    Kind::ClassAbility,
    Kind::Movement,
    Kind::Transcendence,
];

const fn subclass(file: &'static str) -> Tab {
    Tab {
        file,
        section: file,
        kinds: SUBCLASS,
        element: Some(file),
        look: Look::Right,
    }
}

const fn tab(file: &'static str, section: &'static str, kinds: &'static [Kind], look: Look) -> Tab {
    Tab {
        file,
        section,
        kinds,
        element: None,
        look,
    }
}

/// The current tabs; the `OLD …` tabs (archived pre-Monument values) and
/// the landing page are not read.
pub const TABS: [Tab; 15] = [
    tab(
        "Weapon_Perks",
        "Weapon Perks",
        &[
            Kind::WeaponPerk,
            Kind::OriginTrait,
            Kind::Intrinsic,
            Kind::WeaponMod,
        ],
        Look::Right,
    ),
    tab(
        "Armor_Perks",
        "Armor Perks",
        &[
            Kind::SetBonus,
            Kind::ArmorSet,
            Kind::ArmorArchetype,
            Kind::ArmorTuning,
        ],
        Look::Right,
    ),
    tab(
        "Artifact_Perks",
        "Artifact Perks",
        &[Kind::ArtifactPerk],
        Look::Left,
    ),
    tab("Armor_Mods", "Armor Mods", &[Kind::ArmorMod], Look::Right),
    subclass("Arc"),
    subclass("Solar"),
    subclass("Void"),
    subclass("Stasis"),
    subclass("Strand"),
    subclass("Prismatic"),
    tab(
        "Exotic_Class",
        "Exotic Class",
        &[Kind::Intrinsic, Kind::ExoticArmor],
        Look::Right,
    ),
    tab(
        "Class_Abilities",
        "Class Abilities",
        &[Kind::ClassAbility, Kind::Movement],
        Look::Right,
    ),
    tab(
        "Exotic_Weapons",
        "Exotic Weapons",
        &[Kind::ExoticWeapon, Kind::Intrinsic, Kind::WeaponPerk],
        Look::Right,
    ),
    tab(
        "Exotic_Armors",
        "Exotic Armors",
        &[Kind::ExoticArmor, Kind::Intrinsic],
        Look::Right,
    ),
    tab("Game_Mechanics", "Game Mechanics", &[], Look::Right),
];

/// A name cell's own further lines at least this long carry its text.
const OWN_TEXT: usize = 40;
/// A cell this long or longer is a description, not a name.
const LONG: usize = 30;
/// A name is at most this long.
const NAME_MAX: usize = 48;
/// A column is where a tab's names start when this many matched names do.
const NAME_COLUMN: usize = 3;

/// Read every tab in `dir`.
pub fn read(dir: &Path, names: &Names) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    for tab in &TABS {
        let path = dir.join(format!("{}.html", tab.file));
        let html = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        out.extend(extract(&Grid::parse(&html), tab, names));
    }
    Ok(out)
}

/// A name cell: its display name, subjects and own text.
#[derive(Debug, Clone)]
struct Named {
    name: String,
    subjects: Vec<SubjectRef>,
    own: Option<String>,
}

fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

/// The ways a cell may name something: its first line, the part after a
/// `|`, and its lines up to a bracketed annotation joined.
fn candidates(text: &str) -> Vec<(String, bool)> {
    let first = first_line(text);
    let mut out = vec![(first.to_string(), true)];
    if let Some((_, after)) = first.rsplit_once('|') {
        out.push((after.trim().to_string(), true));
    }
    let joined: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take_while(|l| !l.starts_with('['))
        .collect();
    if joined.len() > 1 {
        out.push((joined.join(" "), false));
    }
    out
}

fn match_cell(text: &str, tab: &Tab, names: &Names) -> Option<Named> {
    for (candidate, from_first) in candidates(text) {
        if candidate.is_empty() || candidate.chars().count() > NAME_MAX {
            continue;
        }
        let mut found: Vec<(Subject, &'static str)> = names
            .lookup(&candidate, tab.kinds)
            .into_iter()
            .map(|s| (s, "name"))
            .collect();
        if found.is_empty()
            && let Some(element) = tab.element
        {
            found = names
                .fragment(element, &candidate)
                .into_iter()
                .map(|s| (s, "alias"))
                .collect();
        }
        if found.is_empty() && tab.kinds == [Kind::ArmorMod] {
            found = names
                .family(&candidate, tab.kinds)
                .into_iter()
                .map(|s| (s, "family"))
                .collect();
        }
        if found.is_empty() {
            continue;
        }
        let rest: Vec<&str> = text
            .lines()
            .map(str::trim)
            .skip_while(|l| l.is_empty())
            .skip(1)
            .collect();
        let rest = rest.join("\n").trim().to_string();
        let own = (from_first && rest.chars().count() >= OWN_TEXT).then_some(rest);
        return Some(Named {
            name: candidate,
            subjects: found
                .into_iter()
                .map(|(s, rule)| SubjectRef {
                    hash: s.hash,
                    hash_kind: s.hash_kind,
                    rule,
                })
                .collect(),
            own,
        });
    }
    None
}

/// Column headings, never names.
const HEADINGS: [&str; 7] = [
    "effect",
    "description",
    "notes",
    "information",
    "icon",
    "stat changes",
    "perk",
];

fn name_like(text: &str) -> bool {
    let first = first_line(text);
    let n = first.chars().count();
    (2..=NAME_MAX).contains(&n)
        && first.split_whitespace().count() <= 6
        && !first.ends_with(['.', ':'])
        && !HEADINGS.contains(&crate::names::normalize(first).as_str())
        && first.chars().any(char::is_alphabetic)
        && text.chars().count() <= NAME_MAX + 20
}

/// The cells holding a name cell's text, in reading order; with `below`,
/// the cell under it when nothing is beside it.
fn gather(
    grid: &Grid,
    id: usize,
    look: Look,
    name_cells: &HashMap<usize, Named>,
    starts: &[usize],
    below: bool,
) -> Vec<usize> {
    let (r, c, cell) = grid.anchor(id);
    let mut out = Vec::new();
    let next_start = starts
        .iter()
        .copied()
        .find(|&s| s >= c + cell.cols)
        .unwrap_or(grid.cols());
    let prev_start = starts
        .iter()
        .rev()
        .copied()
        .find(|&s| s < c)
        .map_or(0, |s| s + 1);
    for row in r..r + cell.rows {
        let cols: Vec<usize> = match look {
            Look::Right => (c + cell.cols..grid.cols()).collect(),
            Look::Left => (0..c).rev().collect(),
        };
        // Past the next column where names start, only once something of
        // the text has been found in the row: a description may itself
        // start in such a column.
        let beyond = |col: usize| match look {
            Look::Right => col >= next_start,
            Look::Left => col < prev_start,
        };
        let mut found = false;
        for col in cols {
            if found && beyond(col) {
                break;
            }
            let Some(other) = grid.id_at(row, col) else {
                continue;
            };
            if other == id {
                continue;
            }
            if name_cells.contains_key(&other) {
                break;
            }
            let text = &grid.anchor(other).2.text;
            if text.is_empty() || text == "-" {
                continue;
            }
            found = true;
            if !out.contains(&other) {
                out.push(other);
            }
        }
    }
    if out.is_empty() && below {
        let under = r + cell.rows;
        for col in c..c + cell.cols {
            if let Some(other) = grid.id_at(under, col)
                && other != id
                && !name_cells.contains_key(&other)
                && !grid.anchor(other).2.text.is_empty()
                && !out.contains(&other)
            {
                out.push(other);
            }
        }
    }
    out
}

/// The entries of one tab.
#[must_use]
pub fn extract(grid: &Grid, tab: &Tab, names: &Names) -> Vec<Entry> {
    let mut name_cells: HashMap<usize, Named> = HashMap::new();
    for (id, _, _, cell) in grid.anchors() {
        if let Some(n) = match_cell(&cell.text, tab, names) {
            name_cells.insert(id, n);
        }
    }
    // Columns where the tab's names start bound an entry's text.
    let mut counts: HashMap<usize, usize> = HashMap::new();
    for id in name_cells.keys() {
        *counts.entry(grid.anchor(*id).1).or_default() += 1;
    }
    let mut starts: Vec<usize> = counts
        .into_iter()
        .filter(|&(_, n)| n >= NAME_COLUMN)
        .map(|(c, _)| c)
        .collect();
    starts.sort_unstable();

    // Short cells with a long text beside them: game states and topics.
    let ids: Vec<usize> = grid.anchors().map(|(id, ..)| id).collect();
    for &id in &ids {
        let text = &grid.anchor(id).2.text;
        if name_cells.contains_key(&id) || !name_like(text) {
            continue;
        }
        let beside = gather(grid, id, tab.look, &name_cells, &starts, false);
        if beside
            .iter()
            .any(|&d| grid.anchor(d).2.text.chars().count() >= LONG)
        {
            name_cells.insert(
                id,
                Named {
                    name: first_line(text).to_string(),
                    subjects: Vec::new(),
                    own: None,
                },
            );
        }
    }

    // Header rows (only short cells, none a name) name the part of the tab.
    let mut header_of_row: Vec<Option<String>> = vec![None; grid.rows()];
    let mut current: Option<String> = None;
    for (r, slot) in header_of_row.iter_mut().enumerate() {
        let cells: Vec<usize> = (0..grid.cols())
            .filter_map(|c| grid.id_at(r, c))
            .filter(|&id| grid.anchor(id).0 == r && !grid.anchor(id).2.text.is_empty())
            .fold(Vec::new(), |mut v, id| {
                if !v.contains(&id) {
                    v.push(id);
                }
                v
            });
        if !cells.is_empty()
            && cells.iter().all(|&id| {
                !name_cells.contains_key(&id) && grid.anchor(id).2.text.chars().count() < LONG
            })
        {
            current = Some(first_line(&grid.anchor(cells[0]).2.text).to_string());
        }
        slot.clone_from(&current);
    }

    let mut out = Vec::new();
    for &id in &ids {
        let Some(n) = name_cells.get(&id) else {
            continue;
        };
        let (r, _, cell) = grid.anchor(id);
        let mut parts = Vec::new();
        let mut keywords = Vec::new();
        if let Some(own) = &n.own {
            parts.push(own.clone());
            keywords.extend(
                cell.marks
                    .iter()
                    .filter(|(k, _)| !k.contains(&n.name))
                    .cloned(),
            );
        } else {
            let rest: Vec<&str> = cell
                .text
                .lines()
                .map(str::trim)
                .skip_while(|l| l.is_empty())
                .skip(1)
                .filter(|l| !l.is_empty())
                .collect();
            if !rest.is_empty() {
                parts.push(rest.join("\n"));
            }
            for d in gather(grid, id, tab.look, &name_cells, &starts, true) {
                let other = grid.anchor(d).2;
                parts.push(other.text.clone());
                keywords.extend(other.marks.iter().cloned());
            }
        }
        let text = parts.join("\n").trim().to_string();
        if text.is_empty() {
            continue;
        }
        let section = match &header_of_row[r] {
            Some(h) if h != tab.section => format!("{} / {h}", tab.section),
            _ => tab.section.to_string(),
        };
        out.push(Entry {
            section,
            name: n.name.clone(),
            text,
            subjects: n.subjects.clone(),
            keywords,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Names {
        let mut n = Names::default();
        n.insert_for_test("Ember of Ashes", 1, Kind::Fragment);
        n.insert_for_test("Force Absorption", 2, Kind::SetBonus);
        n.insert_for_test("AION Adapter", 3, Kind::ArmorSet);
        n.insert_for_test("Press The Advantage", 4, Kind::ArtifactPerk);
        n.insert_for_test("Dielectric", 5, Kind::ArtifactPerk);
        n.insert_for_test("Reactive Shock", 6, Kind::SetBonus);
        n
    }

    fn page(rows: &str) -> Grid {
        Grid::parse(&format!(
            r#"<table class="waffle"><tbody>{rows}</tbody></table>"#
        ))
    }

    #[test]
    fn subclass_tabs_name_fragments_short_and_define_states() {
        let g = page(
            r#"<tr><td colspan="4">Glossary</td></tr>
               <tr><td>Radiant</td><td></td><td colspan="2">Increases Weapon Damage by 20% for 10 seconds.</td></tr>
               <tr><td>Fragments</td><td>Icon</td><td>Information</td><td>Stat Changes</td></tr>
               <tr><td>Ashes</td><td></td><td>Scorch Stacks inflicted are increased by 50%.</td><td>-</td></tr>"#,
        );
        let e = extract(&g, &subclass("Solar"), &names());
        assert_eq!(e.len(), 2);
        assert_eq!(
            (e[0].name.as_str(), e[0].section.as_str()),
            ("Radiant", "Solar / Glossary")
        );
        assert!(e[0].subjects.is_empty(), "a game state documents no item");
        assert_eq!(e[1].name, "Ashes");
        assert_eq!(e[1].section, "Solar / Fragments");
        assert_eq!(e[1].text, "Scorch Stacks inflicted are increased by 50%.");
        assert_eq!((e[1].subjects[0].hash, e[1].subjects[0].rule), (1, "alias"));
    }

    #[test]
    fn a_description_may_start_in_a_column_where_names_start() {
        use std::fmt::Write as _;
        // Names start in column 0 and, further down, in column 1: the
        // column every description of column 0 starts in.
        let mut rows = String::new();
        for name in ["Ember of Ashes", "Press The Advantage", "Dielectric"] {
            let _ = write!(
                rows,
                "<tr><td>{name}</td><td>Some fifty characters of description text go here.</td></tr>"
            );
        }
        for _ in 0..3 {
            rows.push_str("<tr><td></td><td>Dielectric</td><td>Grants Bolt Charge on Arc-debuffed kills.</td></tr>");
        }
        let mut n = names();
        n.insert_for_test("Press The Advantage", 4, Kind::Fragment);
        n.insert_for_test("Dielectric", 5, Kind::Fragment);
        let e = extract(&page(&rows), &subclass("Solar"), &n);
        assert_eq!(
            e[0].text,
            "Some fifty characters of description text go here."
        );
    }

    #[test]
    fn sentence_starts_and_headings_are_not_names() {
        assert!(!name_like("Upon stunning a Champion:"));
        assert!(!name_like("Effect"));
        assert!(name_like("Woven Mail"));
    }

    #[test]
    fn set_bonus_cells_carry_their_own_text() {
        let g = page(
            r#"<tr><td rowspan="2">AION Adapter<br><br>Kepler</td><td><span style="color:#81cf82">2 Piece | Force Absorption</span><br>Upon scoring a Rocket Launcher kill: 20% resist for 16 seconds.</td></tr>
               <tr><td>4 Piece | Reactive Shock<br>While Force Absorption is active: disorient nearby enemies.</td></tr>"#,
        );
        let tab = TABS[1];
        let e = extract(&g, &tab, &names());
        let set = e.iter().find(|x| x.name == "AION Adapter").unwrap();
        assert_eq!(
            set.text, "Kepler",
            "the set keeps only its own lines; its bonuses are entries"
        );
        let bonus = e.iter().find(|x| x.name == "Force Absorption").unwrap();
        assert_eq!(
            bonus.text,
            "Upon scoring a Rocket Launcher kill: 20% resist for 16 seconds."
        );
        assert_eq!(bonus.subjects[0].hash_kind, "sandbox_perk");
        assert_eq!(
            bonus.keywords,
            [],
            "the bonus's own coloured name is not a keyword"
        );
        let four = e.iter().find(|x| x.name == "Reactive Shock").unwrap();
        assert_eq!(
            four.text,
            "While Force Absorption is active: disorient nearby enemies."
        );
    }

    #[test]
    fn artifact_perks_read_leftwards_and_stop_at_the_previous_name() {
        let g = page(
            r#"<tr><td></td><td rowspan="2">Press The Advantage</td><td></td><td rowspan="2">Dielectric</td></tr>
               <tr><td>Upon breaking a Combatant's Shield: +20 Stability.</td><td>On Arc-Debuffed Enemy Kill: Grants Bolt Charge.</td></tr>"#,
        );
        let e = extract(&g, &TABS[2], &names());
        let text = |n: &str| e.iter().find(|x| x.name == n).unwrap().text.clone();
        assert_eq!(
            text("Press The Advantage"),
            "Upon breaking a Combatant's Shield: +20 Stability."
        );
        assert_eq!(
            text("Dielectric"),
            "On Arc-Debuffed Enemy Kill: Grants Bolt Charge."
        );
    }
}
