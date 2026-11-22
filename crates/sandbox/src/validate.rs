//! Validation against the recorded clears (`focalpoint analyze model`,
//! `docs/build-model.md` §7).
//!
//! Each recorded **kit** (the super, abilities, aspects, fragments,
//! Transcendence and exotic armor a build runs) is measured, per context,
//! by how its clears score against their players' usual level, as `analyze
//! escalation` measures parts. The model evaluates the same kits in a
//! scenario for each context: Grandmaster Conquest for the hard one,
//! Ultimate Fireteam Op for the easy one. The question is whether the
//! model's metrics say anything about the measured effects:
//!
//! * the rank correlation of each metric with the effect, per context;
//! * a ridge regression of the effect on all the metrics, scored on kits
//!   held out of its fit (five folds): the first calibrated layer, and a
//!   test on kits it has not seen.

// Numeric code: the usual short names (x, y, e, s, o, k) read best.
#![allow(clippy::many_single_char_names)]

use std::collections::BTreeSet;

use anyhow::Result;
use focalpoint_db::Database;
use focalpoint_db::analysis::{escalation, pearson, spearman};

use crate::compile::{Build, compile};
use crate::economy::Economy;
use crate::fluid::Engine;
use crate::ontology::Ontology;
use crate::rates::{rates, seeds_with};
use crate::rules::RuleBase;
use crate::scenario::Scenario;
use crate::variables::Variables;

/// The model's metrics of a build in a scenario, in this order.
pub const METRICS: [&str; 7] = [
    "ability kill capacity (log)",
    "ability damage (log)",
    "healing a second",
    "damage resistance",
    "loop gain",
    "casts a minute",
    "super casts a minute",
];

/// The scenario each measured context is evaluated in.
pub const SCENARIOS: [(&str, &str); 2] = [
    ("hard", "Grandmaster Conquest"),
    ("easy", "Ultimate Fireteam Op"),
];

/// A recorded kit's clears in one context.
#[derive(Debug, Clone)]
pub struct Kit {
    /// `kind:name` parts: the subclass kit and exotic armor.
    pub parts: Vec<String>,
    /// Modelled artifact perks and armor mods that most of the kit's clears
    /// ran (by build share): evaluated with the kit, not part of its key.
    pub accessories: Vec<String>,
    pub context: String,
    /// Clears, by build share.
    pub clears: f64,
    pub players: i64,
    /// Mean score against the players' usual level (log scale).
    pub effect: f64,
    /// The recorded share of the kit's kills by grenade, melee, super and
    /// other abilities (the activity report's split), by build share.
    pub ability_kill_share: Option<f64>,
}

impl Kit {
    /// The parts evaluated: the kit and its accessories.
    #[must_use]
    pub fn build(&self) -> Vec<String> {
        let mut out = self.parts.clone();
        out.extend(self.accessories.iter().cloned());
        out
    }
}

/// Recorded kits with at least `min_clears` clears and `min_players`
/// players in a context.
pub fn kits(
    db: &Database,
    min_clears: f64,
    min_players: i64,
    accessories: &[String],
) -> Result<Vec<Kit>> {
    let listed = accessories
        .iter()
        .map(|p| format!("'{}'", p.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");
    let listed = if listed.is_empty() {
        "''".to_string()
    } else {
        listed
    };
    let sql = format!(
        "WITH {clears},
         kit AS (
             SELECT bc.build_id,
                    array_to_string(list_sort(list_distinct(list(bc.kind || ':' || i.name))), '|') AS kit
             FROM build_component bc JOIN item i ON i.item_hash = bc.component_hash
             WHERE bc.kind IN ('super', 'grenade', 'melee', 'class_ability', 'aspect', 'fragment', 'transcendence')
                OR (bc.kind = 'armor' AND i.is_exotic)
             GROUP BY 1),
         acc AS (
             SELECT DISTINCT bc.build_id, bc.kind || ':' || i.name AS part
             FROM build_component bc JOIN item i ON i.item_hash = bc.component_hash
             WHERE bc.kind IN ('artifact_perk', 'armor_mod')
               AND bc.kind || ':' || i.name IN ({listed})),
         runs AS (
             SELECT k.kit, c.ctx, c.build_id, c.share, c.resid, c.instance_id, c.membership_id
             FROM c JOIN kit k USING (build_id) WHERE c.ctx IN ('hard', 'easy')),
         pk AS (
             SELECT instance_id, membership_id, sum(kills) AS kills,
                    sum(grenade_kills + melee_kills + super_kills + ability_kills) AS abilities
             FROM run_player SEMI JOIN runs USING (instance_id, membership_id) GROUP BY ALL),
         agg AS (
             SELECT kit, ctx, sum(share) AS w, sum(share * resid) / sum(share) AS effect,
                    count(DISTINCT membership_id) AS players,
                    sum(share * pk.abilities) / nullif(sum(share * pk.kills), 0) AS ability_share
             FROM runs LEFT JOIN pk USING (instance_id, membership_id) GROUP BY ALL
             HAVING sum(share) >= ? AND count(DISTINCT membership_id) >= ?),
         accs AS (
             SELECT r.kit, r.ctx, a.part, sum(r.share) AS w
             FROM runs r JOIN acc a USING (build_id) GROUP BY ALL)
         SELECT agg.kit, agg.ctx, agg.w, agg.effect, agg.players,
                coalesce(array_to_string(list_sort(list(accs.part) FILTER (WHERE accs.w >= 0.5 * agg.w)), '|'), ''),
                agg.ability_share
         FROM agg LEFT JOIN accs USING (kit, ctx)
         GROUP BY agg.kit, agg.ctx, agg.w, agg.effect, agg.players, agg.ability_share
         ORDER BY 1, 2",
        clears = escalation::clears_cte(),
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map(duckdb::params![min_clears, min_players], |r| {
        let kit: String = r.get(0)?;
        let acc: String = r.get(5)?;
        Ok(Kit {
            parts: kit.split('|').map(String::from).collect(),
            accessories: acc
                .split('|')
                .filter(|p| !p.is_empty())
                .map(String::from)
                .collect(),
            context: r.get(1)?,
            clears: r.get(2)?,
            effect: r.get(3)?,
            players: r.get(4)?,
            ability_kill_share: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

/// The modelled parts evaluated with a kit rather than keying it: artifact
/// perks and armor mods.
fn accessories(base: &RuleBase) -> Vec<String> {
    base.modelled()
        .iter()
        .filter(|p| p.kind == "artifact_perk" || p.kind == "armor_mod")
        .map(ToString::to_string)
        .collect()
}

/// The model's [`METRICS`] for a build in a scenario.
pub fn metrics(
    o: &Ontology,
    base: &RuleBase,
    vars: &Variables,
    e: &Economy,
    s: &Scenario,
    parts: &[String],
) -> Result<Vec<f64>> {
    let build = Build::parse(parts.iter().map(String::as_str))?;
    let c = compile(o, base, &build, vars);
    let seeds = seeds_with(o, &c);
    let report = Engine::new(o, &c, e, s).analyse(&seeds);
    let r = rates(o, &c, e, s, &report);
    let casts: f64 = r
        .casts_per_min
        .iter()
        .filter(|(a, _)| *a != "super")
        .map(|(_, n)| n)
        .sum();
    let capacity = r.ability_kill_capacity_per_min / r.supply_per_min.max(1e-9);
    Ok(vec![
        capacity.ln_1p(),
        r.damage_per_s.max(0.0).ln_1p(),
        r.heal_per_s,
        r.damage_resist,
        report.loops.first().map_or(0.0, |l| l.gain.min(3.0)),
        casts,
        r.casts_per_min.get("super").copied().unwrap_or(0.0),
    ])
}

/// What validation found in one context.
#[derive(Debug, Clone)]
pub struct ContextReport {
    pub context: String,
    pub scenario: String,
    pub kits: usize,
    /// Each metric's rank correlation with the measured effect.
    pub correlations: Vec<(&'static str, f64)>,
    /// The ridge regression's held-out predictions against the effects.
    pub held_out_pearson: f64,
    pub held_out_spearman: f64,
    /// Its weights on the standardised metrics, fitted on every kit.
    pub weights: Vec<(&'static str, f64)>,
    /// The share of the kits' part uses the rule base models.
    pub modelled_share: f64,
    /// The model's mean share of kills by abilities over the kits, and the
    /// recorded one for the context: a calibration moment.
    pub model_ability_kill_share: f64,
    pub recorded_ability_kill_share: Option<f64>,
    /// The ability efficiency calibrated to the recorded share, if fitted.
    pub fitted_efficiency: Option<f64>,
    /// The model's share of kills by abilities against each kit's recorded
    /// share: how well the model orders kits by a mechanism it predicts
    /// directly, not through the effect.
    pub kill_share_pearson: f64,
    pub kill_share_spearman: f64,
    /// The kits' mean recorded share.
    pub kits_recorded_kill_share: Option<f64>,
}

/// The mean share of kills by abilities the model gives a set of kits.
fn mean_kill_share(
    o: &Ontology,
    base: &RuleBase,
    vars: &Variables,
    e: &Economy,
    s: &Scenario,
    kits: &[&Kit],
) -> Result<f64> {
    let mut total = 0.0;
    for k in kits {
        let m = metrics(o, base, vars, e, s, &k.build())?;
        total += m[0].exp_m1().min(1.0);
    }
    #[allow(clippy::cast_precision_loss)]
    Ok(total / kits.len().max(1) as f64)
}

/// Calibrate a scenario to the recorded clears (one moment, one
/// parameter): the `scenario.ability_efficiency` at which the kits' mean
/// share of kills by abilities matches the recorded share. None when no
/// value in 0.002-1 does.
pub fn fit_efficiency(
    o: &Ontology,
    base: &RuleBase,
    vars: &Variables,
    e: &Economy,
    s: &Scenario,
    kits: &[&Kit],
) -> Result<Option<f64>> {
    // The kits' own recorded share where known, else the context's.
    let recorded: Vec<f64> = kits.iter().filter_map(|k| k.ability_kill_share).collect();
    #[allow(clippy::cast_precision_loss)]
    let kits_share =
        (!recorded.is_empty()).then(|| recorded.iter().sum::<f64>() / recorded.len() as f64);
    let Some(target) = kits_share.or(s.recorded_ability_kill_share) else {
        return Ok(None);
    };
    let at = |x: f64| -> Result<f64> {
        let mut v = vars.clone();
        v.set("scenario.ability_efficiency", x)?;
        Ok(mean_kill_share(o, base, &v, e, s, kits)? - target)
    };
    // Bisect on the log scale.
    let (mut lo, mut hi) = (0.002f64.ln(), 0.0f64);
    let (flo, fhi) = (at(lo.exp())?, at(hi.exp())?);
    if flo.signum() == fhi.signum() {
        return Ok(None);
    }
    for _ in 0..30 {
        let mid = f64::midpoint(lo, hi);
        if at(mid.exp())?.signum() == flo.signum() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok(Some(f64::midpoint(lo, hi).exp()))
}

/// Validate the model on the recorded kits; with `fit`, each scenario is
/// first calibrated ([`fit_efficiency`]).
pub fn validate(
    db: &Database,
    o: &Ontology,
    base: &RuleBase,
    vars: &Variables,
    min_clears: f64,
    min_players: i64,
    fit: bool,
) -> Result<Vec<ContextReport>> {
    let e = Economy::load(db, vars)?;
    let modelled: BTreeSet<String> = base.modelled().iter().map(ToString::to_string).collect();
    let all = kits(db, min_clears, min_players, &accessories(base))?;
    let mut out = Vec::new();
    for (context, scenario_name) in SCENARIOS {
        let s = Scenario::from_context(db, scenario_name, vars)?;
        let kits: Vec<&Kit> = all.iter().filter(|k| k.context == context).collect();
        let fitted = if fit {
            fit_efficiency(o, base, vars, &e, &s, &kits)?
        } else {
            None
        };
        let mut fitted_vars = vars.clone();
        if let Some(x) = fitted {
            fitted_vars.set("scenario.ability_efficiency", x)?;
        }
        let vars = &fitted_vars;
        let mut x = Vec::with_capacity(kits.len());
        let mut y = Vec::with_capacity(kits.len());
        let (mut parts, mut known) = (0usize, 0usize);
        let mut share = 0.0;
        let (mut model_share, mut recorded_share) = (Vec::new(), Vec::new());
        for k in &kits {
            let build = k.build();
            let m = metrics(o, base, vars, &e, &s, &build)?;
            share += (m[0].exp_m1()).min(1.0);
            if let Some(r) = k.ability_kill_share {
                model_share.push(m[0].exp_m1().min(1.0));
                recorded_share.push(r);
            }
            x.push(m);
            y.push(k.effect);
            parts += k.parts.len();
            known += k.parts.iter().filter(|p| modelled.contains(*p)).count();
        }
        let column = |j: usize| x.iter().map(|row| row[j]).collect::<Vec<f64>>();
        let correlations = METRICS
            .iter()
            .enumerate()
            .map(|(j, m)| (*m, spearman(&column(j), &y)))
            .collect();
        let held_out = cross_validate(&x, &y, 5);
        let weights = ridge(&standardise(&x).0, &y)
            .into_iter()
            .zip(METRICS)
            .map(|(w, m)| (m, w))
            .collect();
        #[allow(clippy::cast_precision_loss)]
        let modelled_share = if parts > 0 {
            known as f64 / parts as f64
        } else {
            0.0
        };
        out.push(ContextReport {
            context: context.to_string(),
            scenario: s.name.clone(),
            kits: kits.len(),
            correlations,
            held_out_pearson: pearson(&held_out, &y),
            held_out_spearman: spearman(&held_out, &y),
            weights,
            modelled_share,
            #[allow(clippy::cast_precision_loss)]
            model_ability_kill_share: share / (kits.len().max(1) as f64),
            recorded_ability_kill_share: s.recorded_ability_kill_share,
            fitted_efficiency: fitted,
            kill_share_pearson: pearson(&model_share, &recorded_share),
            kill_share_spearman: spearman(&model_share, &recorded_share),
            #[allow(clippy::cast_precision_loss)]
            kits_recorded_kill_share: (!recorded_share.is_empty())
                .then(|| recorded_share.iter().sum::<f64>() / recorded_share.len() as f64),
        });
    }
    Ok(out)
}

/// A context's calibrated layer: the scenario with its fitted efficiency,
/// and a ridge regression from the model's metrics to the measured effect
/// of recorded kits. It scores builds no one has played.
#[derive(Debug, Clone)]
pub struct Calibrated {
    pub context: String,
    pub scenario: Scenario,
    pub vars: Variables,
    /// Each metric's mean and deviation over the recorded kits.
    pub stats: Vec<(f64, f64)>,
    pub weights: Vec<f64>,
    /// The kits' mean effect.
    pub mean: f64,
    pub kits: usize,
}

impl Calibrated {
    /// The predicted effect (log scale against a player's usual level) of
    /// a build with these metrics.
    #[must_use]
    pub fn predict(&self, m: &[f64]) -> f64 {
        self.mean
            + m.iter()
                .zip(&self.stats)
                .zip(&self.weights)
                .map(|((v, (mu, sd)), w)| if *sd > 1e-12 { (v - mu) / sd * w } else { 0.0 })
                .sum::<f64>()
    }
}

/// Calibrate a context (`hard` or `easy`, [`SCENARIOS`]) on the recorded
/// kits with at least `min_clears` clears and `min_players` players.
#[allow(clippy::too_many_arguments)]
pub fn calibrate(
    db: &Database,
    o: &Ontology,
    base: &RuleBase,
    vars: &Variables,
    e: &Economy,
    context: &str,
    min_clears: f64,
    min_players: i64,
) -> Result<Calibrated> {
    let Some((_, name)) = SCENARIOS.iter().find(|(c, _)| *c == context) else {
        anyhow::bail!("no calibrated context {context:?}: one of hard, easy");
    };
    let s = Scenario::from_context(db, name, vars)?;
    let all = kits(db, min_clears, min_players, &accessories(base))?;
    let kits: Vec<&Kit> = all.iter().filter(|k| k.context == context).collect();
    let mut fitted = vars.clone();
    if let Some(x) = fit_efficiency(o, base, vars, e, &s, &kits)? {
        fitted.set("scenario.ability_efficiency", x)?;
    }
    let mut x = Vec::with_capacity(kits.len());
    let mut y = Vec::with_capacity(kits.len());
    for k in &kits {
        x.push(metrics(o, base, &fitted, e, &s, &k.build())?);
        y.push(k.effect);
    }
    let (z, stats) = standardise(&x);
    let weights = ridge_monotone(&z, &y);
    #[allow(clippy::cast_precision_loss)]
    let mean = y.iter().sum::<f64>() / y.len().max(1) as f64;
    Ok(Calibrated {
        context: context.to_string(),
        scenario: s,
        vars: fitted,
        stats,
        weights,
        mean,
        kits: kits.len(),
    })
}

/// A part's measured lift (`analyze escalation`) against the model's
/// predicted change when the part is removed from the kits that run it.
#[derive(Debug, Clone)]
pub struct PartCheck {
    pub subclass: String,
    pub part: String,
    pub measured_hard: f64,
    pub measured_easy: f64,
    /// Kits the prediction averages over.
    pub kits: usize,
    /// Per metric, the mean change the part makes in the hard and the easy
    /// scenario.
    pub hard: Vec<f64>,
    pub easy: Vec<f64>,
}

/// Compare the model's marginal value of each modelled part with its
/// measured lift. Kits come from `kits(db, 10, 3)`; each part is judged on
/// up to `max_kits` of the kits that run it, most clears first.
pub fn part_checks(
    db: &Database,
    o: &Ontology,
    base: &RuleBase,
    vars: &Variables,
    min_clears: f64,
    min_players: i64,
    max_kits: usize,
) -> Result<Vec<PartCheck>> {
    let e = Economy::load(db, vars)?;
    let scenarios: Vec<Scenario> = SCENARIOS
        .iter()
        .map(|(_, name)| Scenario::from_context(db, name, vars))
        .collect::<Result<_>>()?;
    let mut pool = kits(db, 10.0, 3, &accessories(base))?;
    pool.sort_by(|a, b| b.clears.total_cmp(&a.clears));
    let modelled: BTreeSet<String> = base.modelled().iter().map(ToString::to_string).collect();
    let mut out = Vec::new();
    for lift in escalation::lifts(db, min_clears, min_players)? {
        let key = format!("{}:{}", lift.kind, lift.part);
        if !modelled.contains(&key) {
            continue;
        }
        let mut seen = BTreeSet::new();
        let with: Vec<&Kit> = pool
            .iter()
            .filter(|k| k.build().contains(&key) && seen.insert(k.build()))
            .take(max_kits)
            .collect();
        if with.is_empty() {
            continue;
        }
        let mut deltas = [vec![0.0; METRICS.len()], vec![0.0; METRICS.len()]];
        for k in &with {
            let build = k.build();
            let without: Vec<String> = build.iter().filter(|p| **p != key).cloned().collect();
            for (si, s) in scenarios.iter().enumerate() {
                let a = metrics(o, base, vars, &e, s, &build)?;
                let b = metrics(o, base, vars, &e, s, &without)?;
                for (j, d) in deltas[si].iter_mut().enumerate() {
                    *d += a[j] - b[j];
                }
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let n = with.len() as f64;
        let [hard, easy] = deltas.map(|d| d.into_iter().map(|x| x / n).collect::<Vec<f64>>());
        out.push(PartCheck {
            subclass: lift.subclass,
            part: key,
            measured_hard: lift.hard,
            measured_easy: lift.easy,
            kits: with.len(),
            hard,
            easy,
        });
    }
    Ok(out)
}

/// For each metric, the rank correlation of the parts' predicted change
/// with their measured lift in hard content, and of the predicted hard
/// minus easy change with the measured hard minus easy lift.
#[must_use]
pub fn part_correlations(checks: &[PartCheck]) -> Vec<(&'static str, f64, f64)> {
    let measured: Vec<f64> = checks.iter().map(|c| c.measured_hard).collect();
    let measured_diff: Vec<f64> = checks
        .iter()
        .map(|c| c.measured_hard - c.measured_easy)
        .collect();
    METRICS
        .iter()
        .enumerate()
        .map(|(j, m)| {
            let hard: Vec<f64> = checks.iter().map(|c| c.hard[j]).collect();
            let diff: Vec<f64> = checks.iter().map(|c| c.hard[j] - c.easy[j]).collect();
            (
                *m,
                spearman(&hard, &measured),
                spearman(&diff, &measured_diff),
            )
        })
        .collect()
}

/// Columns standardised to mean 0 and deviation 1 (constant columns to 0),
/// with each column's mean and deviation.
fn standardise(x: &[Vec<f64>]) -> (Vec<Vec<f64>>, Vec<(f64, f64)>) {
    let n = x.len().max(1);
    let k = x.first().map_or(0, Vec::len);
    #[allow(clippy::cast_precision_loss)]
    let nf = n as f64;
    let stats: Vec<(f64, f64)> = (0..k)
        .map(|j| {
            let mean = x.iter().map(|r| r[j]).sum::<f64>() / nf;
            let var = x.iter().map(|r| (r[j] - mean).powi(2)).sum::<f64>() / nf;
            (mean, var.sqrt())
        })
        .collect();
    let z = x
        .iter()
        .map(|r| {
            r.iter()
                .zip(&stats)
                .map(|(v, (m, sd))| if *sd > 1e-12 { (v - m) / sd } else { 0.0 })
                .collect()
        })
        .collect();
    (z, stats)
}

/// Ridge regression weights for standardised `x` and centred `y`
/// (penalty: one per kit's worth of data, a tenth of the kits).
fn ridge(x: &[Vec<f64>], y: &[f64]) -> Vec<f64> {
    let k = x.first().map_or(0, Vec::len);
    #[allow(clippy::cast_precision_loss)]
    let n = y.len() as f64;
    let my = y.iter().sum::<f64>() / n.max(1.0);
    let lambda = 0.1 * n;
    let mut a = vec![vec![0.0; k + 1]; k];
    for (i, row) in a.iter_mut().enumerate() {
        for j in 0..k {
            row[j] = x.iter().map(|r| r[i] * r[j]).sum::<f64>() + if i == j { lambda } else { 0.0 };
        }
        row[k] = x.iter().zip(y).map(|(r, v)| r[i] * (v - my)).sum();
    }
    solve(a)
}

/// Ridge weights constrained to be non-negative: more of what the model
/// measures (kill capacity, loop gain, survival) never scores worse. A
/// metric whose weight comes out negative is dropped and the rest refitted.
fn ridge_monotone(x: &[Vec<f64>], y: &[f64]) -> Vec<f64> {
    let k = x.first().map_or(0, Vec::len);
    let mut active: Vec<bool> = vec![true; k];
    loop {
        let cols: Vec<usize> = (0..k).filter(|j| active[*j]).collect();
        let sub: Vec<Vec<f64>> = x
            .iter()
            .map(|r| cols.iter().map(|&j| r[j]).collect())
            .collect();
        let w = ridge(&sub, y);
        let mut full = vec![0.0; k];
        for (c, &j) in cols.iter().enumerate() {
            full[j] = w[c];
        }
        match cols
            .iter()
            .zip(&w)
            .filter(|(_, v)| **v < 0.0)
            .min_by(|a, b| a.1.total_cmp(b.1))
        {
            Some((&j, _)) => active[j] = false,
            None => return full,
        }
    }
}

/// Solve an augmented linear system by Gaussian elimination with partial
/// pivoting; singular directions get 0.
fn solve(mut a: Vec<Vec<f64>>) -> Vec<f64> {
    let k = a.len();
    for col in 0..k {
        let pivot = (col..k)
            .max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))
            .unwrap_or(col);
        a.swap(col, pivot);
        let p = a[col][col];
        if p.abs() < 1e-12 {
            continue;
        }
        let pivot_row = a[col].clone();
        for (r, row) in a.iter_mut().enumerate() {
            if r != col {
                let f = row[col] / p;
                if f != 0.0 {
                    for (x, v) in row.iter_mut().zip(&pivot_row).skip(col) {
                        *x -= f * v;
                    }
                }
            }
        }
    }
    (0..k)
        .map(|i| {
            if a[i][i].abs() < 1e-12 {
                0.0
            } else {
                a[i][k] / a[i][i]
            }
        })
        .collect()
}

/// Held-out predictions: each kit predicted by a ridge fit on the other
/// folds (kits assigned to folds in turn).
fn cross_validate(x: &[Vec<f64>], y: &[f64], folds: usize) -> Vec<f64> {
    let n = y.len();
    let mut pred = vec![0.0; n];
    for f in 0..folds {
        let train: Vec<usize> = (0..n).filter(|i| i % folds != f).collect();
        let test: Vec<usize> = (0..n).filter(|i| i % folds == f).collect();
        if train.len() < 3 || test.is_empty() {
            continue;
        }
        let xt: Vec<Vec<f64>> = train.iter().map(|&i| x[i].clone()).collect();
        let yt: Vec<f64> = train.iter().map(|&i| y[i]).collect();
        let (z, stats) = standardise(&xt);
        let w = ridge(&z, &yt);
        #[allow(clippy::cast_precision_loss)]
        let my = yt.iter().sum::<f64>() / yt.len() as f64;
        for &i in &test {
            let p: f64 = x[i]
                .iter()
                .zip(&stats)
                .zip(&w)
                .map(|((v, (m, sd)), b)| if *sd > 1e-12 { (v - m) / sd * b } else { 0.0 })
                .sum();
            pred[i] = my + p;
        }
    }
    pred
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ridge_recovers_a_linear_effect_held_out() {
        // y = 2 x0 - x1 + noise-free; x2 is irrelevant.
        let x: Vec<Vec<f64>> = (0..40)
            .map(|i| {
                let f = f64::from(i);
                vec![f, (f * 7.0) % 11.0, (f * 3.0) % 5.0]
            })
            .collect();
        let y: Vec<f64> = x.iter().map(|r| 2.0 * r[0] - r[1]).collect();
        let held = cross_validate(&x, &y, 5);
        assert!(pearson(&held, &y) > 0.95, "{}", pearson(&held, &y));
        let w = ridge(&standardise(&x).0, &y);
        assert!(
            w[0] > 0.0 && w[1] < 0.0 && w[2].abs() < w[0].abs() / 5.0,
            "{w:?}"
        );
    }

    #[test]
    fn solve_handles_a_singular_direction() {
        let a = vec![vec![2.0, 0.0, 4.0], vec![0.0, 0.0, 0.0]];
        assert_eq!(solve(a), vec![2.0, 0.0]);
    }
}
