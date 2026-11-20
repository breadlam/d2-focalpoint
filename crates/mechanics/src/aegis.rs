//! `TheAegisRelic`'s "Destiny 2: Boss Damage" workbook: measured boss damage
//! for Monument of Triumph, and the loadouts behind it.
//!
//! The `Ranking (S29)` sheet lists every tested damage rotation: its
//! category (Universal, Specialized, Support, …), class, boss, average DPS
//! after 10–60 seconds and at the rotation's end, the DPS of its repeating
//! core, artifact mods, other factors and notes — and links to the DIM
//! loadouts (dim.gg) it was run with. The loadouts are read from their
//! saved shares (`dim/<share id>.json`, the DIM loadout-share API's
//! answer): equipped items, the subclass's and items' socket choices,
//! armor mods and artifact unlocks. Notes link further variants.
//!
//! The sheet's figures are cached formula results; its own notes are
//! context-sensitive, so they are kept with each rotation.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::xlsx::{self, Sheet};

/// The sheet of rotations.
pub const RANKING: &str = "Ranking (S29)";

/// A measured damage rotation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rotation {
    pub section: String,
    pub category: String,
    pub class: Option<&'static str>,
    pub name: String,
    pub boss: Option<String>,
    pub core_dps: Option<f64>,
    /// Average DPS after 10, 20, … 60 seconds.
    pub dps_by_10s: [Option<f64>; 6],
    pub end_dps: Option<f64>,
    pub end_s: Option<f64>,
    pub end_damage: Option<f64>,
    pub continues: Option<bool>,
    pub segment_start_s: Option<f64>,
    pub segment_end_s: Option<f64>,
    pub artifact_mods: Option<String>,
    pub other_factors: Option<String>,
    pub notes: Option<String>,
    /// `(share id, role, label)`: `loadout 1`, `loadout 2`, or `note`
    /// (labelled by the note's line).
    pub loadouts: Vec<(String, String, Option<String>)>,
}

/// A loadout shared from DIM.
#[derive(Debug, Clone, PartialEq)]
pub struct Loadout {
    pub id: String,
    pub name: String,
    pub class: Option<&'static str>,
    pub items: Vec<LoadoutItem>,
}

/// One hash a loadout uses: an equipped item, a socket choice on one, an
/// armor mod or an artifact unlock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadoutItem {
    pub hash: u32,
    /// `equipped`, `socket` (a choice in `parent`'s socket `socket`),
    /// `mod` or `artifact`.
    pub role: &'static str,
    pub parent: Option<u32>,
    pub socket: Option<u16>,
}

/// What the workbook gives.
#[derive(Debug, Default)]
pub struct Aegis {
    pub rotations: Vec<Rotation>,
    pub loadouts: Vec<Loadout>,
    /// Share ids linked but not saved.
    pub missing: Vec<String>,
}

/// Read the workbook and the saved loadouts in `dir`.
pub fn read(dir: &Path) -> Result<Aegis> {
    let sheets = xlsx::read(&dir.join("boss-damage.xlsx"))?;
    let ranking = sheets
        .iter()
        .find(|s| s.name == RANKING)
        .with_context(|| format!("no sheet {RANKING:?} in boss-damage.xlsx"))?;
    let rotations = rotations(ranking);
    let mut ids: Vec<String> = rotations
        .iter()
        .flat_map(|r| r.loadouts.iter().map(|l| l.0.clone()))
        .collect();
    ids.sort();
    ids.dedup();
    let mut out = Aegis {
        rotations,
        ..Aegis::default()
    };
    for id in ids {
        let path = dir.join("dim").join(format!("{id}.json"));
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let v: Value = serde_json::from_str(&text)
                    .with_context(|| format!("parsing {}", path.display()))?;
                if let Some(l) = loadout(&id, &v) {
                    out.loadouts.push(l);
                }
            }
            Err(_) => out.missing.push(id),
        }
    }
    Ok(out)
}

/// The dim.gg share ids in a text, each with the line it is on.
fn shares(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut rest = line;
        while let Some(i) = rest.find("dim.gg/") {
            let id: String = rest[i + 7..]
                .chars()
                .take_while(char::is_ascii_alphanumeric)
                .collect();
            if !id.is_empty() {
                out.push((id, line.trim().to_string()));
            }
            rest = &rest[i + 7..];
        }
    }
    out
}

fn class_letter(s: &str) -> Option<&'static str> {
    match s.trim() {
        "H" | "Hunter" => Some("Hunter"),
        "T" | "Titan" => Some("Titan"),
        "W" | "Warlock" => Some("Warlock"),
        _ => None,
    }
}

fn number(s: Option<&str>) -> Option<f64> {
    s?.trim()
        .replace(',', "")
        .parse::<f64>()
        .ok()
        .filter(|x| x.is_finite())
}

fn text(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|s| !s.is_empty() && *s != "N/A" && *s != "/")
        .map(String::from)
}

/// The rotations of the ranking sheet: its header row has `Category` and
/// `core DPS`.
#[must_use]
pub fn rotations(sheet: &Sheet) -> Vec<Rotation> {
    let Some(header) = (0..sheet.rows()).find(|&r| {
        let row: Vec<&str> = sheet
            .cells
            .range((r, 0)..(r + 1, 0))
            .map(|(_, v)| v.trim())
            .collect();
        row.contains(&"Category") && row.contains(&"core DPS")
    }) else {
        return Vec::new();
    };
    let col: HashMap<String, u32> = sheet
        .cells
        .range((header, 0)..(header + 1, 0))
        .map(|((_, c), v)| (v.trim().to_string(), *c))
        .collect();
    let at = |r: u32, name: &str| col.get(name).and_then(|&c| sheet.get(r, c));
    let mut out = Vec::new();
    for r in header + 1..sheet.rows() {
        let (Some(category), Some(name)) = (text(at(r, "Category")), text(at(r, "Name"))) else {
            continue;
        };
        if category == "NOTE" {
            continue;
        }
        let mut loadouts = Vec::new();
        for (label, role) in [("Loadout 1", "loadout 1"), ("Loadout 2", "loadout 2")] {
            if let Some(url) = col.get(label).and_then(|&c| sheet.links.get(&(r, c))) {
                for (id, _) in shares(url) {
                    loadouts.push((id, role.to_string(), None));
                }
            }
        }
        let mut row_notes: Vec<(&(u32, u32), &String)> =
            sheet.notes.iter().filter(|((nr, _), _)| *nr == r).collect();
        row_notes.sort_by_key(|(pos, _)| **pos);
        for (_, note) in &row_notes {
            for (id, line) in shares(note) {
                if !loadouts.iter().any(|l| l.0 == id) {
                    loadouts.push((id, "note".to_string(), Some(line)));
                }
            }
        }
        let mut notes: Vec<String> = text(at(r, "Notes")).into_iter().collect();
        notes.extend(row_notes.iter().map(|(_, n)| (*n).clone()));
        out.push(Rotation {
            section: sheet.name.clone(),
            category,
            class: at(r, "Class").and_then(class_letter),
            name,
            boss: text(at(r, "Boss")),
            core_dps: number(at(r, "core DPS")),
            dps_by_10s: ["10.0", "20.0", "30.0", "40.0", "50.0", "60.0"].map(|h| number(at(r, h))),
            end_dps: number(at(r, "DPS @ end")),
            end_s: number(at(r, "time @ end")),
            end_damage: number(at(r, "total @ end")),
            continues: at(r, "continues?").and_then(|s| match s.trim() {
                "✓" => Some(true),
                "✗" => Some(false),
                _ => None,
            }),
            segment_start_s: number(at(r, "segment start")),
            segment_end_s: number(at(r, "segment end")),
            artifact_mods: text(at(r, "Artifact Mods")),
            other_factors: text(at(r, "Other Factors")),
            notes: (!notes.is_empty()).then(|| notes.join("\n")),
            loadouts,
        });
    }
    out
}

fn hash(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|h| u32::try_from(h).ok())
}

/// A saved share: `{"loadout": {name, classType, equipped, unequipped, parameters}}`.
#[must_use]
pub fn loadout(id: &str, v: &Value) -> Option<Loadout> {
    let l = v.get("loadout")?;
    let mut items = Vec::new();
    for list in ["equipped", "unequipped"] {
        for e in l.get(list).and_then(Value::as_array).into_iter().flatten() {
            let Some(item) = e.get("hash").and_then(hash) else {
                continue;
            };
            items.push(LoadoutItem {
                hash: item,
                role: "equipped",
                parent: None,
                socket: None,
            });
            let mut overrides: Vec<(u16, u32)> = e
                .get("socketOverrides")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter_map(|(k, v)| Some((k.parse().ok()?, hash(v)?)))
                .collect();
            overrides.sort_unstable();
            items.extend(overrides.into_iter().map(|(socket, h)| LoadoutItem {
                hash: h,
                role: "socket",
                parent: Some(item),
                socket: Some(socket),
            }));
        }
    }
    let params = l.get("parameters");
    for h in params
        .and_then(|p| p.get("mods"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(hash)
    {
        items.push(LoadoutItem {
            hash: h,
            role: "mod",
            parent: None,
            socket: None,
        });
    }
    for h in params
        .and_then(|p| p.pointer("/artifactUnlocks/unlockedItemHashes"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(hash)
    {
        items.push(LoadoutItem {
            hash: h,
            role: "artifact",
            parent: None,
            socket: None,
        });
    }
    Some(Loadout {
        id: id.to_string(),
        name: l
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        class: match l.get("classType").and_then(Value::as_u64) {
            Some(0) => Some("Titan"),
            Some(1) => Some("Hunter"),
            Some(2) => Some("Warlock"),
            _ => None,
        },
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations_take_their_figures_links_and_noted_variants() {
        let mut s = Sheet {
            name: RANKING.into(),
            ..Sheet::default()
        };
        for (c, h) in [
            "Category",
            "Class",
            "Name",
            "10.0",
            "core DPS",
            "continues?",
            "Notes",
            "Loadout 1",
            "Boss",
        ]
        .iter()
        .enumerate()
        {
            s.cells.insert((1, c as u32), (*h).to_string());
        }
        for (c, v) in [
            "Universal",
            "H",
            "Turncoat Heretic",
            "14046.78",
            "15065.7",
            "✗",
            "pre-stacked",
            "LINK",
            "CARL",
        ]
        .iter()
        .enumerate()
        {
            s.cells.insert((2, c as u32), (*v).to_string());
        }
        s.cells.insert((3, 0), "NOTE".into());
        s.cells.insert((3, 2), "ignored".into());
        s.links.insert((2, 7), "https://dim.gg/7edunka/Void".into());
        s.notes
            .insert((2, 6), "Dawn:\nhttps://dim.gg/kry4xua/Nightfall".into());
        let r = rotations(&s);
        assert_eq!(r.len(), 1);
        let r = &r[0];
        assert_eq!(
            (r.class, r.boss.as_deref(), r.continues),
            (Some("Hunter"), Some("CARL"), Some(false))
        );
        assert_eq!(
            (r.core_dps, r.dps_by_10s[0], r.dps_by_10s[1]),
            (Some(15065.7), Some(14046.78), None)
        );
        assert_eq!(
            r.loadouts,
            [
                ("7edunka".to_string(), "loadout 1".to_string(), None),
                (
                    "kry4xua".to_string(),
                    "note".to_string(),
                    Some("https://dim.gg/kry4xua/Nightfall".to_string())
                )
            ]
        );
        assert_eq!(
            r.notes.as_deref(),
            Some("pre-stacked\nDawn:\nhttps://dim.gg/kry4xua/Nightfall")
        );
    }

    #[test]
    fn shares_give_items_socket_choices_mods_and_artifact_unlocks() {
        let v: Value = serde_json::from_str(
            r#"{"loadout": {"name": "Void", "classType": 1,
                "equipped": [{"hash": 10}, {"hash": 20, "socketOverrides": {"7": 72, "0": 70}}],
                "parameters": {"mods": [5, 5], "artifactUnlocks": {"unlockedItemHashes": [9], "seasonNumber": 29}}}}"#,
        )
        .unwrap();
        let l = loadout("abc", &v).unwrap();
        assert_eq!((l.name.as_str(), l.class), ("Void", Some("Hunter")));
        let got: Vec<(u32, &str, Option<u32>, Option<u16>)> = l
            .items
            .iter()
            .map(|i| (i.hash, i.role, i.parent, i.socket))
            .collect();
        assert_eq!(
            got,
            [
                (10, "equipped", None, None),
                (20, "equipped", None, None),
                (70, "socket", Some(20), Some(0)),
                (72, "socket", Some(20), Some(7)),
                (5, "mod", None, None),
                (5, "mod", None, None),
                (9, "artifact", None, None)
            ]
        );
    }
}
