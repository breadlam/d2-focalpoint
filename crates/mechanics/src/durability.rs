//! How much damage enemies take to kill, by enemy and activity
//! (`docs/durability.md`): the context parameter escalation loops turn on —
//! enemies that survive long enough sustain chains and damage-counted
//! ammo; enemies that die quickly feed kill-volume engines.
//!
//! * [`enemy_health`] — `MossyMax`'s Foetracer-measured health of every enemy
//!   archetype, by faction, for minor (rank-and-file) and elite variants at
//!   four activity health tiers (Damage Scaling Spreadsheet v1.9.2, from
//!   before Edge of Fate);
//! * [`measured`] — his measured bosses and minibosses;
//! * [`power_delta`] — outgoing damage by Power delta, Edge of Fate's
//!   curve, relative to a delta of 0 (it matches the Data Compendium's
//!   ×0.446 at −30 and ×0.144 at −50);
//! * [`champions`] — the Compendium's Champion health, shields and
//!   regeneration by activity difficulty.
//!
//! Health is in the game's health units at the activity's health tier,
//! before Power delta: an enemy's damage to kill is its health divided by
//! the damage multiplier of the delta it is fought at.

use std::collections::HashMap;

use crate::sheet::Grid;

/// An enemy's health at an activity health tier.
#[derive(Debug, Clone, PartialEq)]
pub struct EnemyHealth {
    pub faction: String,
    pub enemy: String,
    /// `minor` or `elite`.
    pub rank: &'static str,
    /// The sheet's base health class (50, 100, … 4400).
    pub base: f64,
    pub tier: u8,
    pub health: f64,
}

const FACTIONS: [&str; 6] = ["Cabal", "Fallen", "Hive", "Vex", "Taken", "Scorn"];

fn number(s: &str) -> Option<f64> {
    s.trim()
        .replace(',', "")
        .parse::<f64>()
        .ok()
        .filter(|x| x.is_finite())
}

fn text_at(g: &Grid, r: usize, c: usize) -> Option<&str> {
    let id = g.id_at(r, c)?;
    let (ar, ac, cell) = g.anchor(id);
    ((ar, ac) == (r, c))
        .then_some(cell.text.as_str())
        .filter(|t| !t.trim().is_empty())
}

/// The archetype table of the "foetracer enemy health summary" sheet: a
/// header row naming the six factions, the base health class and minor
/// and elite health at tiers 1–4 (the row above numbers the tiers).
#[must_use]
pub fn enemy_health(g: &Grid) -> Vec<EnemyHealth> {
    let Some(header) =
        (0..g.rows()).find(|&r| (0..6).all(|c| text_at(g, r, c) == Some(FACTIONS[c])))
    else {
        return Vec::new();
    };
    // Columns of minor/elite health per tier: the row above holds the tier
    // number over each minor column.
    let mut tiers: Vec<(usize, u8, &'static str)> = Vec::new();
    // The block of minor/elite columns after the base health class; the
    // sheet repeats the table further right, which is not read.
    for c in 7..g.cols() {
        let rank = match text_at(g, header, c).map(str::trim) {
            Some("minor") => "minor",
            Some("elite") => "elite",
            _ => break,
        };
        let tier = (0..=c)
            .rev()
            .find_map(|cc| text_at(g, header.saturating_sub(1), cc).and_then(number))
            .map_or(0, |t| t as u8);
        if (1..=4).contains(&tier) {
            tiers.push((c, tier, rank));
        }
    }
    let mut out = Vec::new();
    for r in header + 1..g.rows() {
        let Some(base) = text_at(g, r, 6).and_then(number) else {
            break;
        };
        for (c, faction) in FACTIONS.iter().enumerate() {
            let Some(names) = text_at(g, r, c) else {
                continue;
            };
            for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                let enemy = name.trim_end_matches("(?)").trim().to_string();
                for &(col, tier, rank) in &tiers {
                    if let Some(health) = text_at(g, r, col).and_then(number) {
                        out.push(EnemyHealth {
                            faction: (*faction).to_string(),
                            enemy: enemy.clone(),
                            rank,
                            base,
                            tier,
                            health,
                        });
                    }
                }
            }
        }
    }
    out
}

/// A measured boss or miniboss.
#[derive(Debug, Clone, PartialEq)]
pub struct Measured {
    /// `boss` or `miniboss`.
    pub kind: String,
    pub enemy: String,
    pub activity: String,
    pub tier: Option<u8>,
    /// In damage dealt, as measured.
    pub estimated: Option<f64>,
    /// In health units.
    pub baseline: Option<f64>,
}

/// The "All Measured Bosses and Minibosses" table: `Combatant Tier, Enemy,
/// Activity, potential Activity HP tier, AVERAGE of Estimated health,
/// AVERAGE of baseline health`; the kind and the enemy carry down blank
/// cells.
#[must_use]
pub fn measured(g: &Grid) -> Vec<Measured> {
    let Some(header) = (0..g.rows())
        .find(|&r| text_at(g, r, 0) == Some("Combatant Tier") && text_at(g, r, 1) == Some("Enemy"))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let (mut kind, mut enemy) = (String::new(), String::new());
    for r in header + 1..g.rows() {
        let activity = text_at(g, r, 2);
        let estimated = text_at(g, r, 4).and_then(number);
        if activity.is_none() && estimated.is_none() {
            if text_at(g, r, 1).is_none() {
                break;
            }
            continue;
        }
        if let Some(k) = text_at(g, r, 0) {
            kind = k.trim().to_string();
        }
        if let Some(e) = text_at(g, r, 1) {
            enemy = e.trim().to_string();
        }
        out.push(Measured {
            kind: kind.clone(),
            enemy: enemy.clone(),
            activity: activity.unwrap_or("").trim().to_string(),
            tier: text_at(g, r, 3).and_then(number).map(|t| t as u8),
            estimated,
            baseline: text_at(g, r, 5).and_then(number),
        });
    }
    out
}

/// Outgoing damage multiplier by Power delta, from the "PL Delta and
/// Activity Scaling" sheet's `EoF Standard` column, relative to a delta of
/// 0: `(delta, multiplier)`.
#[must_use]
pub fn power_delta(g: &Grid) -> Vec<(i32, f64)> {
    let Some((header, delta_col, eof_col)) = (0..g.rows()).find_map(|r| {
        let d = (0..g.cols()).find(|&c| text_at(g, r, c) == Some("Power Delta"))?;
        let e = (0..g.cols()).find(|&c| text_at(g, r, c) == Some("EoF Standard"))?;
        Some((r, d, e))
    }) else {
        return Vec::new();
    };
    let mut raw: Vec<(i32, f64)> = Vec::new();
    for r in header + 1..g.rows() {
        let (Some(d), Some(m)) = (
            text_at(g, r, delta_col).and_then(number),
            text_at(g, r, eof_col).and_then(number),
        ) else {
            continue;
        };
        if d.fract() == 0.0 {
            raw.push((d as i32, m));
        }
    }
    let Some(&(_, zero)) = raw.iter().find(|(d, _)| *d == 0) else {
        return Vec::new();
    };
    raw.into_iter().map(|(d, m)| (d, m / zero)).collect()
}

/// One property of a Champion type at one difficulty.
#[derive(Debug, Clone, PartialEq)]
pub struct ChampionStat {
    pub champion: String,
    pub property: String,
    /// The difficulty column: Standard, Raid (Expert/Master), Advanced, …
    pub difficulty: String,
    pub value: Option<f64>,
    pub text: String,
}

/// The Compendium's Champion tables (Game Mechanics tab): a heading row
/// "<Type> Champions", an "Information" row naming the difficulty columns,
/// then a row per property.
#[must_use]
pub fn champions(g: &Grid) -> Vec<ChampionStat> {
    let mut out = Vec::new();
    let mut r = 0;
    while r < g.rows() {
        let heading = (0..g.cols()).find_map(|c| {
            text_at(g, r, c).filter(|t| t.trim().ends_with("Champions") && t.len() < 40)
        });
        let Some(heading) = heading.map(|h| h.trim().trim_end_matches('s').to_string()) else {
            r += 1;
            continue;
        };
        // The Information row within the next few rows.
        let Some(info) = (r + 1..(r + 4).min(g.rows()))
            .find(|&rr| (0..g.cols()).any(|c| text_at(g, rr, c) == Some("Information")))
        else {
            r += 1;
            continue;
        };
        let columns: Vec<(usize, String)> = (0..g.cols())
            .filter_map(|c| {
                let t = text_at(g, info, c)?;
                (t != "Information" && t.len() < 40).then(|| (c, t.replace('\n', " ")))
            })
            .take(5)
            .collect();
        let mut rr = info + 1;
        while rr < g.rows() {
            let Some(property) =
                (0..columns.first().map_or(0, |c| c.0)).find_map(|c| text_at(g, rr, c))
            else {
                break;
            };
            let mut last: Option<String> = None;
            for (c, difficulty) in &columns {
                // A merged cell spans the difficulties it applies to.
                let text = g
                    .id_at(rr, *c)
                    .map(|id| g.anchor(id).2.text.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .or_else(|| last.clone());
                let Some(text) = text else { continue };
                last = Some(text.clone());
                let value = text
                    .trim_start_matches('~')
                    .split_whitespace()
                    .next()
                    .and_then(|t| number(t.trim_end_matches('%')));
                out.push(ChampionStat {
                    champion: heading.clone(),
                    property: property.trim().to_string(),
                    difficulty: difficulty.clone(),
                    value,
                    text,
                });
            }
            rr += 1;
        }
        r = rr;
    }
    out
}

/// The repository's `data/difficulty.toml`, built in.
pub const DIFFICULTY: &str = include_str!("../../../data/difficulty.toml");

/// An activity group's Power delta and health tier, with its evidence.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Difficulty {
    pub group: String,
    pub power_delta: Option<i32>,
    pub health_tier: Option<u8>,
    pub champion_difficulty: Option<String>,
    /// documented | dated | assumed
    pub confidence: String,
    #[serde(default)]
    pub sources: Vec<String>,
    pub note: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DifficultyFile {
    #[serde(default)]
    difficulty: Vec<Difficulty>,
}

/// Parse a difficulty file in the format of `data/difficulty.toml`.
pub fn difficulties(text: &str) -> anyhow::Result<Vec<Difficulty>> {
    let file: DifficultyFile = toml::from_str(text)?;
    for d in &file.difficulty {
        anyhow::ensure!(
            ["documented", "dated", "assumed"].contains(&d.confidence.as_str()),
            "{}: confidence {:?} is not documented, dated or assumed",
            d.group,
            d.confidence
        );
    }
    Ok(file.difficulty)
}

/// The damage multiplier at a delta, interpolated on the curve.
#[must_use]
pub fn multiplier(curve: &[(i32, f64)], delta: i32) -> Option<f64> {
    curve.iter().find(|(d, _)| *d == delta).map(|(_, m)| *m)
}

/// Everything this module reads.
#[derive(Debug, Default)]
pub struct Tables {
    pub health: Vec<EnemyHealth>,
    pub measured: Vec<Measured>,
    pub curve: Vec<(i32, f64)>,
    pub champions: Vec<ChampionStat>,
}

/// Read `MossyMax`'s sheets in `scaling` and the Compendium's Game Mechanics
/// tab in `compendium`; sheets that are missing give nothing.
pub fn read(scaling: &std::path::Path, compendium: &std::path::Path) -> anyhow::Result<Tables> {
    let grid = |path: std::path::PathBuf| -> anyhow::Result<Option<Grid>> {
        if !path.exists() {
            return Ok(None);
        }
        let html = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        Ok(Some(Grid::parse(&html)))
    };
    let mut t = Tables::default();
    if let Some(g) = grid(scaling.join("foetracer_enemy_health_summary.html"))? {
        t.health = enemy_health(&g);
        t.measured = measured(&g);
    }
    if let Some(g) = grid(scaling.join("PL_Delta_and_Activity_Scaling.html"))? {
        t.curve = power_delta(&g);
    }
    if let Some(g) = grid(compendium.join("Game_Mechanics.html"))? {
        t.champions = champions(&g);
    }
    Ok(t)
}

/// Add each difficulty's features to its group's context, the group's
/// activities and their encounters: `power_delta`, `damage_multiplier`,
/// `durability` (its inverse), `health_tier`, `champion_difficulty`,
/// `difficulty_confidence` and `difficulty_evidence`. Returns the contexts given features.
pub fn apply(
    contexts: &mut [crate::context::Context],
    difficulties: &[Difficulty],
    curve: &[(i32, f64)],
) -> usize {
    let mut touched = 0;
    for d in difficulties {
        let Some(group) = contexts
            .iter()
            .find(|c| c.kind == "group" && c.name == d.group)
            .map(|c| c.id.clone())
        else {
            continue;
        };
        let mut ids = vec![group];
        for _ in 0..2 {
            let more: Vec<String> = contexts
                .iter()
                .filter(|c| {
                    c.parent.as_ref().is_some_and(|p| ids.contains(p)) && !ids.contains(&c.id)
                })
                .map(|c| c.id.clone())
                .collect();
            ids.extend(more);
        }
        let m = d.power_delta.and_then(|delta| multiplier(curve, delta));
        let evidence = match &d.note {
            Some(n) => format!("{}: {} ({n})", d.confidence, d.sources.join("; ")),
            None => format!("{}: {}", d.confidence, d.sources.join("; ")),
        };
        for c in contexts.iter_mut().filter(|c| ids.contains(&c.id)) {
            let s = "curated";
            c.number("power_delta", d.power_delta.map(f64::from), s);
            c.number("damage_multiplier", m, s);
            c.number("durability", m.map(|m| 1.0 / m), s);
            c.number("health_tier", d.health_tier.map(f64::from), s);
            c.text("champion_difficulty", d.champion_difficulty.as_deref(), s);
            c.text("difficulty_confidence", Some(&d.confidence), s);
            c.text("difficulty_evidence", Some(&evidence), s);
            touched += 1;
        }
    }
    touched
}

/// Health by enemy, rank and tier, for lookups.
#[must_use]
pub fn index(health: &[EnemyHealth]) -> HashMap<(String, &'static str, u8), f64> {
    health
        .iter()
        .map(|h| ((h.enemy.to_lowercase(), h.rank, h.tier), h.health))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(rows: &str) -> Grid {
        Grid::parse(&format!(
            r#"<table class="waffle"><tbody>{rows}</tbody></table>"#
        ))
    }

    #[test]
    fn archetypes_give_minor_and_elite_health_by_tier() {
        let g = page(
            "<tr><td></td><td></td><td></td><td></td><td></td><td></td><td></td><td>1</td><td></td><td>2</td><td></td><td>3</td><td></td><td>4</td><td></td></tr>
               <tr><td>Cabal</td><td>Fallen</td><td>Hive</td><td>Vex</td><td>Taken</td><td>Scorn</td><td>HP tier</td><td>minor</td><td>elite</td><td>minor</td><td>elite</td><td>minor</td><td>elite</td><td>minor</td><td>elite</td></tr>
               <tr><td></td><td>Shank</td><td></td><td></td><td></td><td></td><td>50</td><td>50</td><td>200</td><td>61</td><td>267</td><td>101</td><td>378</td><td>108</td><td>402</td></tr>
               <tr><td>Gladiator</td><td></td><td>Knight</td><td></td><td>Knight</td><td>Wraith</td><td>620</td><td>622</td><td>914</td><td>1235</td><td>1538</td><td>1549</td><td>2006</td><td>1668</td><td>2137</td></tr>
               <tr><td></td><td></td><td></td><td></td><td></td><td></td><td></td></tr>",
        );
        let h = enemy_health(&g);
        assert_eq!(h.len(), 8 + 4 * 8);
        let idx = index(&h);
        assert!((idx[&("shank".to_string(), "elite", 4)] - 402.0).abs() < 1e-9);
        assert!((idx[&("knight".to_string(), "minor", 3)] - 1549.0).abs() < 1e-9);
        assert!(
            h.iter()
                .any(|e| e.faction == "Taken" && e.enemy == "Knight")
        );
    }

    #[test]
    fn the_built_in_difficulty_file_parses() {
        let d = difficulties(DIFFICULTY).unwrap();
        let uc = d.iter().find(|d| d.group == "Ultimate Conquest").unwrap();
        assert_eq!(
            (uc.power_delta, uc.health_tier, uc.confidence.as_str()),
            (Some(-30), Some(4), "assumed")
        );
        assert!(difficulties("[[difficulty]]\ngroup = \"x\"\nconfidence = \"guessed\"").is_err());
    }

    #[test]
    fn the_delta_curve_is_relative_to_zero() {
        let g = page(
            "<tr><td>Power Delta</td><td>EoF Standard</td></tr>
               <tr><td>-50</td><td>0.129170147</td></tr><tr><td>-30</td><td>0.401670919</td></tr>
               <tr><td>0</td><td>0.9002192136</td></tr>",
        );
        let d = power_delta(&g);
        assert_eq!(d.len(), 3);
        assert!(
            (d[0].1 - 0.1435).abs() < 1e-3
                && (d[1].1 - 0.4462).abs() < 1e-3
                && (d[2].1 - 1.0).abs() < 1e-12
        );
    }

    #[test]
    fn champion_tables_spread_merged_cells_over_difficulties() {
        let g = page(
            r#"<tr><td colspan="3">Barrier Champions</td></tr>
               <tr><td>Information</td><td>Standard</td><td>Master<br>Grandmaster</td></tr>
               <tr><td>Champion Health</td><td>2200 HP</td><td>5475 HP</td></tr>
               <tr><td>Maximum Shield Duration</td><td colspan="2">5</td></tr>
               <tr><td></td><td></td><td></td></tr>"#,
        );
        let c = champions(&g);
        let get = |p: &str, d: &str| {
            c.iter()
                .find(|s| s.property == p && s.difficulty == d)
                .and_then(|s| s.value)
        };
        assert_eq!(c[0].champion, "Barrier Champion");
        assert_eq!(get("Champion Health", "Master Grandmaster"), Some(5475.0));
        assert_eq!(
            get("Maximum Shield Duration", "Master Grandmaster"),
            Some(5.0)
        );
    }
}
