//! Synthesis, first version (`focalpoint sandbox synthesize`,
//! `docs/build-model.md` §8): the best subclass kits for a class, element
//! and context, by the calibrated layer's predicted effect.
//!
//! * **Options** are what players run: each slot's parts from the recorded
//!   builds of the subclass, and each aspect pair's fragment slots from the
//!   builds that run it. Modelled artifact perks may be added.
//! * **Accessories** (artifact perks, armor mods, weapon perks, origin
//!   traits) are the subclass's most-used modelled ones, as many of each
//!   kind as its builds run on average. The search swaps them for others
//!   players run, keeping the count: the weights never penalise, so adding
//!   would stack every one.
//! * **Search** is a beam over one-part changes: another super, grenade,
//!   melee or class ability; another aspect; another fragment; another
//!   accessory. It starts from the subclass's most-used kit and
//!   accessories.
//! * **Score** is [`Calibrated::predict`]: the effect against a player's
//!   usual level the recorded kits' fit predicts from the model's metrics.
//!

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use focalpoint_db::Database;

use crate::economy::Economy;
use crate::ontology::Ontology;
use crate::rules::RuleBase;
use crate::validate::{Calibrated, metrics};

/// Slots with one part each.
const SINGLE: [&str; 5] = [
    "super",
    "grenade",
    "melee",
    "class_ability",
    "transcendence",
];

/// What a subclass can run, as recorded.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Parts by kind, most used first.
    pub parts: BTreeMap<String, Vec<String>>,
    /// Fragment slots by aspect pair (sorted names).
    pub fragment_slots: BTreeMap<(String, String), usize>,
    /// The most-used kit and accessories: kind → names.
    pub seed: BTreeMap<String, Vec<String>>,
}

/// The kinds of part swapped rather than chosen per slot.
const ACCESSORIES: [&str; 4] = ["artifact_perk", "armor_mod", "weapon_perk", "origin_trait"];

/// The recorded options of a class and element (`Prismatic` included),
/// keeping only parts in `modelled` (`kind:name`): a part the model knows
/// nothing about scores as nothing, which a search would mistake for safe.
pub fn options(
    db: &Database,
    class: &str,
    element: &str,
    top: usize,
    modelled: &BTreeSet<String>,
) -> Result<Options> {
    let mut o = Options::default();
    let mut stmt = db.conn().prepare(
        "SELECT bc.kind, i.name, count(*) AS uses
         FROM build b JOIN build_component bc USING (build_id)
         JOIN item i ON i.item_hash = bc.component_hash
         WHERE b.class = ?1 AND b.element = ?2
           AND bc.kind IN ('super', 'grenade', 'melee', 'class_ability', 'aspect', 'fragment', 'transcendence')
         GROUP BY ALL ORDER BY bc.kind, uses DESC, i.name",
    )?;
    let rows: Vec<(String, String)> = stmt
        .query_map([class, element], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<duckdb::Result<_>>()?;
    if rows.is_empty() {
        bail!("no recorded builds of {class} {element}");
    }
    for (kind, name) in rows {
        if !modelled.contains(&format!("{kind}:{name}")) {
            continue;
        }
        let list = o.parts.entry(kind).or_default();
        if list.len() < top {
            list.push(name);
        }
    }
    let mut stmt = db.conn().prepare(
        "WITH a AS (
             SELECT bc.build_id, list_sort(list(i.name)) AS aspects
             FROM build b JOIN build_component bc USING (build_id)
             JOIN item i ON i.item_hash = bc.component_hash
             WHERE b.class = ?1 AND b.element = ?2 AND bc.kind = 'aspect'
             GROUP BY 1 HAVING count(*) = 2),
         f AS (SELECT build_id, count(*) AS n FROM build_component WHERE kind = 'fragment' GROUP BY 1)
         SELECT a.aspects[1], a.aspects[2], max(f.n)
         FROM a JOIN f USING (build_id) GROUP BY 1, 2 HAVING count(*) >= 10",
    )?;
    let pairs: Vec<(String, String, i64)> = stmt
        .query_map([class, element], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<duckdb::Result<_>>()?;
    for (a, b, n) in pairs {
        o.fragment_slots
            .insert((a, b), usize::try_from(n).unwrap_or(4));
    }
    // The most-used exact kit.
    let mut stmt = db.conn().prepare(
        "WITH k AS (
             SELECT bc.build_id, array_to_string(list_sort(list(bc.kind || ':' || i.name)), '|') AS kit
             FROM build b JOIN build_component bc USING (build_id)
             JOIN item i ON i.item_hash = bc.component_hash
             WHERE b.class = ?1 AND b.element = ?2
               AND bc.kind IN ('super', 'grenade', 'melee', 'class_ability', 'aspect', 'fragment', 'transcendence')
             GROUP BY 1)
         SELECT kit FROM k GROUP BY 1 ORDER BY count(*) DESC, 1 LIMIT 1",
    )?;
    let kit: String = stmt.query_row([class, element], |r| r.get(0))?;
    for p in kit.split('|') {
        if let Some((kind, name)) = p.split_once(':') {
            o.seed
                .entry(kind.to_string())
                .or_default()
                .push(name.to_string());
        }
    }
    // Accessories: the most used modelled ones, as many of each kind as a
    // build runs on average.
    let mut stmt = db.conn().prepare(
        "SELECT bc.kind, i.name, count(*) AS uses,
                (SELECT count(*) FROM build WHERE class = ?1 AND element = ?2) AS builds
         FROM build b JOIN build_component bc USING (build_id)
         JOIN item i ON i.item_hash = bc.component_hash
         WHERE b.class = ?1 AND b.element = ?2
           AND bc.kind IN ('artifact_perk', 'armor_mod', 'weapon_perk', 'origin_trait')
         GROUP BY bc.kind, i.name ORDER BY bc.kind, uses DESC, i.name",
    )?;
    let rows: Vec<(String, String, i64, i64)> = stmt
        .query_map([class, element], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<duckdb::Result<_>>()?;
    let mut per_build: BTreeMap<String, f64> = BTreeMap::new();
    for (kind, name, uses, builds) in rows {
        if !modelled.contains(&format!("{kind}:{name}")) {
            continue;
        }
        #[allow(clippy::cast_precision_loss)]
        let share = uses as f64 / builds.max(1) as f64;
        *per_build.entry(kind.clone()).or_default() += share;
        let list = o.parts.entry(kind).or_default();
        if list.len() < top {
            list.push(name);
        }
    }
    for (kind, n) in per_build {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let k = n.round() as usize;
        let chosen: Vec<String> = o
            .parts
            .get(&kind)
            .into_iter()
            .flatten()
            .take(k)
            .cloned()
            .collect();
        if !chosen.is_empty() {
            o.seed.insert(kind, chosen);
        }
    }
    Ok(o)
}

/// A scored kit.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// `kind:name` parts.
    pub parts: Vec<String>,
    pub score: f64,
    pub metrics: Vec<f64>,
}

fn flatten(kit: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let mut out: Vec<String> = kit
        .iter()
        .flat_map(|(k, names)| names.iter().map(move |n| format!("{k}:{n}")))
        .collect();
    out.sort();
    out
}

fn slots(o: &Options, kit: &BTreeMap<String, Vec<String>>) -> usize {
    let mut a = kit.get("aspect").cloned().unwrap_or_default();
    a.sort();
    if let [x, y] = a.as_slice() {
        o.fragment_slots
            .get(&(x.clone(), y.clone()))
            .copied()
            .unwrap_or(4)
    } else {
        4
    }
}

/// Every kit one change away, fragments fitted to the aspects' slots.
fn neighbours(
    o: &Options,
    kit: &BTreeMap<String, Vec<String>>,
    extras: &[String],
) -> Vec<BTreeMap<String, Vec<String>>> {
    let mut out = Vec::new();
    for kind in SINGLE {
        let Some(current) = kit.get(kind).and_then(|v| v.first()) else {
            continue;
        };
        for alt in o.parts.get(kind).into_iter().flatten() {
            if alt != current {
                let mut k = kit.clone();
                k.insert(kind.to_string(), vec![alt.clone()]);
                out.push(k);
            }
        }
    }
    let aspects = kit.get("aspect").cloned().unwrap_or_default();
    for (i, _) in aspects.iter().enumerate() {
        for alt in o.parts.get("aspect").into_iter().flatten() {
            if !aspects.contains(alt) {
                let mut k = kit.clone();
                let mut a = aspects.clone();
                a[i].clone_from(alt);
                k.insert("aspect".into(), a);
                out.push(k);
            }
        }
    }
    let fragments = kit.get("fragment").cloned().unwrap_or_default();
    for (i, _) in fragments.iter().enumerate() {
        for alt in o.parts.get("fragment").into_iter().flatten() {
            if !fragments.contains(alt) {
                let mut k = kit.clone();
                let mut f = fragments.clone();
                f[i].clone_from(alt);
                k.insert("fragment".into(), f);
                out.push(k);
            }
        }
    }
    for kind in ACCESSORIES {
        let current = kit.get(kind).cloned().unwrap_or_default();
        for (i, _) in current.iter().enumerate() {
            for alt in o.parts.get(kind).into_iter().flatten() {
                if !current.contains(alt) {
                    let mut k = kit.clone();
                    let mut c = current.clone();
                    c[i].clone_from(alt);
                    k.insert(kind.into(), c);
                    out.push(k);
                }
            }
        }
    }
    for extra in extras {
        let perks = kit.get("artifact_perk").cloned().unwrap_or_default();
        if !perks.contains(extra) {
            let mut k = kit.clone();
            let mut p = perks;
            p.push(extra.clone());
            k.insert("artifact_perk".into(), p);
            out.push(k);
        }
    }
    // Fit the fragments to the slots: drop the last or add the most used.
    for k in &mut out {
        let n = slots(o, k);
        let f = k.entry("fragment".into()).or_default();
        f.truncate(n);
        for alt in o.parts.get("fragment").into_iter().flatten() {
            if f.len() >= n {
                break;
            }
            if !f.contains(alt) {
                f.push(alt.clone());
            }
        }
    }
    out
}

/// Search for the best kits; returns the best `keep`, best first, and the
/// seed kit's score for comparison.
#[allow(clippy::too_many_arguments)]
pub fn search(
    o: &Ontology,
    base: &RuleBase,
    e: &Economy,
    model: &Calibrated,
    options: &Options,
    extras: &[String],
    beam: usize,
    rounds: usize,
    keep: usize,
) -> Result<(Vec<Candidate>, Candidate)> {
    let score = |kit: &BTreeMap<String, Vec<String>>| -> Result<Candidate> {
        let parts = flatten(kit);
        let m = metrics(o, base, &model.vars, e, &model.scenario, &parts)?;
        Ok(Candidate {
            score: model.predict(&m),
            metrics: m,
            parts,
        })
    };
    let start = score(&options.seed)?;
    let mut seen: BTreeSet<Vec<String>> = BTreeSet::new();
    seen.insert(start.parts.clone());
    let mut frontier = vec![(options.seed.clone(), start.clone())];
    let mut best: Vec<Candidate> = vec![start.clone()];
    for _ in 0..rounds {
        let mut next = Vec::new();
        for (kit, _) in &frontier {
            for n in neighbours(options, kit, extras) {
                let parts = flatten(&n);
                if !seen.insert(parts) {
                    continue;
                }
                let c = score(&n)?;
                next.push((n, c));
            }
        }
        if next.is_empty() {
            break;
        }
        // Ties go to the kit closest to the start: a change that scores
        // nothing is not worth making.
        let changes = |c: &Candidate| {
            let a: BTreeSet<&String> = c.parts.iter().collect();
            let b: BTreeSet<&String> = start.parts.iter().collect();
            a.symmetric_difference(&b).count()
        };
        let order = |a: &Candidate, b: &Candidate| {
            if (a.score - b.score).abs() < 1e-9 {
                changes(a).cmp(&changes(b))
            } else {
                b.score.total_cmp(&a.score)
            }
        };
        next.sort_by(|a, b| order(&a.1, &b.1));
        next.truncate(beam);
        best.extend(next.iter().map(|(_, c)| c.clone()));
        best.sort_by(order);
        best.truncate(keep.max(beam));
        frontier = next;
    }
    best.truncate(keep);
    Ok((best, start))
}
