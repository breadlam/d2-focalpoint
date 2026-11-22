//! Synthesis, first version (`focalpoint sandbox synthesize`,
//! `docs/build-model.md` §8): the best subclass kits for a class, element
//! and context, by the calibrated layer's predicted effect.
//!
//! * **Options** are what players run: each slot's parts from the recorded
//!   builds of the subclass, and each aspect pair's fragment slots from the
//!   builds that run it. Modelled artifact perks may be added.
//! * **Search** is a beam over one-part changes: another super, grenade,
//!   melee or class ability; another aspect; another fragment; an artifact
//!   perk added. It starts from the subclass's most-used kit.
//! * **Score** is [`Calibrated::predict`]: the effect against a player's
//!   usual level the recorded kits' fit predicts from the model's metrics.
//!
//! Weapons, armor mods and most exotic armor are not modelled yet, so the
//! search is over the kit alone.

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
    /// The most-used kit: kind → names.
    pub seed: BTreeMap<String, Vec<String>>,
}

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
        next.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));
        next.truncate(beam);
        best.extend(next.iter().map(|(_, c)| c.clone()));
        best.sort_by(|a, b| b.score.total_cmp(&a.score));
        best.truncate(keep.max(beam));
        frontier = next;
    }
    best.truncate(keep);
    Ok((best, start))
}
