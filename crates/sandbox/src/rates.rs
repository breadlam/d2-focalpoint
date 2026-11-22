//! Rates: a build's steady state in a scenario (`docs/sandbox.md`).
//!
//! The fluid analysis gives what each boundary event leads to per event
//! ([`crate::fluid::Report`]). Rates put time in:
//!
//! * **casts** come from passive regeneration (the economy's cooldowns at
//!   the build's stats, plus regeneration bonuses by their uptime) and from
//!   the energy the build returns;
//! * **weapon kills** take the enemies abilities leave: the scenario
//!   supplies `pack size × scenario.packs_per_minute` enemies a minute, and
//!   the player clears them;
//! * **chained events** (ignitions set off by Scorch) follow from the rest.
//!
//! The rates `x` solve `x = r(x) + Gᵀ x`, capped where a loop would outrun
//! the content: casts no faster than one per `policy.min_cast_interval`,
//! chained events no more than the enemies supplied.

use std::collections::BTreeMap;

use crate::compile::Compiled;
use crate::economy::{Economy, stat_of};
use crate::fluid::{Event, Report};
use crate::ontology::Ontology;
use crate::scenario::Scenario;

/// A buff's steady state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Uptime {
    /// The share of time it is active.
    pub uptime: f64,
    /// Its mean stacks, capped at its maximum.
    pub mean_stacks: f64,
}

/// A build's rates in a scenario.
#[derive(Debug, Clone, Default)]
pub struct Rates {
    /// Each boundary event's rate, per second.
    pub per_second: Vec<(Event, f64)>,
    /// Casts a minute by ability (`melee`, `grenade`, `class`, `super`).
    pub casts_per_min: BTreeMap<String, f64>,
    /// Charges a minute from passive regeneration, bonuses included.
    pub regen_per_min: BTreeMap<String, f64>,
    /// Charges a minute the build returns.
    pub returned_per_min: BTreeMap<String, f64>,
    /// Damage a second from what the model knows of the build (abilities
    /// and verbs; weapons' own damage is not modelled), before durability.
    pub damage_per_s: f64,
    pub ability_kills_per_min: f64,
    pub weapon_kills_per_min: f64,
    /// Enemies the scenario supplies a minute.
    pub supply_per_min: f64,
    pub heal_per_s: f64,
    pub buffs: BTreeMap<String, Uptime>,
    /// What hit a cap.
    pub capped: Vec<String>,
}

impl Rates {
    /// The share of kills abilities and verbs make.
    #[must_use]
    pub fn ability_kill_share(&self) -> f64 {
        let total = self.ability_kills_per_min + self.weapon_kills_per_min;
        if total > 0.0 {
            self.ability_kills_per_min / total
        } else {
            0.0
        }
    }
}

fn slot(ability: &str) -> &'static str {
    match ability {
        "grenade" => "grenade",
        "melee" => "melee",
        "class" => "class_ability",
        "super" => "super",
        _ => "movement",
    }
}

/// The weapon-kill seed: kills the player makes with weapons.
#[must_use]
pub fn weapon_kill() -> Event {
    Event::new("kill").with("source", "weapon")
}

/// The seeds of a build: each equipped ability's cast, and weapon kills.
#[must_use]
pub fn seeds(c: &Compiled<'_>) -> Vec<Event> {
    let mut out: Vec<Event> = [
        ("grenade", "grenade"),
        ("melee", "melee"),
        ("class_ability", "class"),
        ("super", "super"),
    ]
    .iter()
    .filter(|(s, _)| c.build.equipped(s).is_some())
    .map(|(_, a)| Event::cast(a))
    .collect();
    out.push(weapon_kill());
    out
}

/// Solve a build's rates from its analysis.
#[must_use]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
pub fn rates(o: &Ontology, c: &Compiled<'_>, e: &Economy, s: &Scenario, report: &Report) -> Rates {
    let vars = c.vars;
    let gens = &report.generations;
    let n = gens.len();
    let supply = s.pack.size * vars.engine("scenario.packs_per_minute", 1.5) / 60.0;
    let min_interval = vars.engine("policy.min_cast_interval", 1.5).max(0.1);
    let regen_mode = vars.engine("economy.regen_relative_to_current", 0.0);
    // Passive regeneration per cast boundary, and the 0-stat rate bonuses
    // scale from.
    let mut base = vec![0.0; n];
    let mut zero_stat = vec![0.0; n];
    let mut ability: Vec<Option<String>> = vec![None; n];
    let mut weapon = None;
    for (i, g) in gens.iter().enumerate() {
        if g.root == weapon_kill() {
            weapon = Some(i);
        }
        if g.root.name != "cast" {
            continue;
        }
        let Some(a) = g.root.attrs.get("ability") else {
            continue;
        };
        ability[i] = Some(a.clone());
        let sl = slot(a);
        if let Some(name) = c.build.equipped(sl) {
            let stat = s.stat(stat_of(sl));
            base[i] = e.charges_per_second(sl, name, stat).unwrap_or(0.0);
            zero_stat[i] = e.charges_per_second(sl, name, 0.0).unwrap_or(0.0);
        }
    }
    let mut x = base.clone();
    let mut capped = Vec::new();
    for _ in 0..500 {
        // Regeneration bonuses, by their uptime: bonus-seconds a second.
        let mut bonus: BTreeMap<&str, f64> = BTreeMap::new();
        for (i, g) in gens.iter().enumerate() {
            for (a, b) in &g.yields.regen {
                *bonus.entry(a.as_str()).or_default() += x[i] * b;
            }
        }
        let ability_kills: f64 = gens
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != weapon)
            .map(|(i, g)| x[i] * g.yields.kills)
            .sum();
        let mut next = vec![0.0; n];
        for i in 0..n {
            next[i] = if Some(i) == weapon {
                (supply - ability_kills).max(0.0)
            } else if let Some(a) = &ability[i] {
                let b = bonus.get(a.as_str()).copied().unwrap_or(0.0);
                let sl = slot(a);
                let scale = e.stat_scale(stat_of(sl), s.stat(stat_of(sl)));
                let unit = if regen_mode >= 0.5 {
                    base[i]
                } else {
                    zero_stat[i]
                };
                base[i] + b * scale * unit
            } else {
                0.0
            };
        }
        for (i, row) in report.gains.iter().enumerate() {
            for (j, g) in row.iter().enumerate() {
                next[j] += x[i] * g;
            }
        }
        for (i, g) in gens.iter().enumerate() {
            if ability[i].is_some() && next[i] > 1.0 / min_interval {
                next[i] = 1.0 / min_interval;
            }
            if g.root.chained && next[i] > supply {
                next[i] = supply;
            }
        }
        let mut change = 0.0f64;
        for i in 0..n {
            let v = 0.5 * x[i] + 0.5 * next[i];
            change = change.max((v - x[i]).abs());
            x[i] = v;
        }
        if change < 1e-12 {
            break;
        }
    }
    for (i, g) in gens.iter().enumerate() {
        if ability[i].is_some() && x[i] >= 1.0 / min_interval - 1e-9 {
            capped.push(format!("{}: one cast every {min_interval} s", g.root));
        }
        if g.root.chained && x[i] >= supply - 1e-9 && supply > 0.0 {
            capped.push(format!("{}: every enemy supplied", g.root));
        }
    }
    let mut r = Rates {
        supply_per_min: supply * 60.0,
        capped,
        ..Rates::default()
    };
    let mut stack_seconds: BTreeMap<String, f64> = BTreeMap::new();
    for (i, g) in gens.iter().enumerate() {
        let y = &g.yields;
        r.per_second.push((g.root.clone(), x[i]));
        if let Some(a) = &ability[i] {
            *r.casts_per_min.entry(a.clone()).or_default() += 60.0 * x[i];
            *r.regen_per_min.entry(a.clone()).or_default() += 60.0 * base[i];
        }
        r.damage_per_s += x[i] * y.damage;
        r.heal_per_s += x[i] * y.heal;
        if Some(i) == weapon {
            r.weapon_kills_per_min += 60.0 * x[i];
        } else {
            r.ability_kills_per_min += 60.0 * x[i] * y.kills;
        }
        for (a, v) in &y.energy {
            *r.returned_per_min.entry(a.clone()).or_default() += 60.0 * x[i] * v;
        }
        for (b, v) in &y.buff_seconds {
            *stack_seconds.entry(b.clone()).or_default() += x[i] * v;
        }
    }
    // Abilities cannot kill more than the scenario supplies.
    if r.ability_kills_per_min > r.supply_per_min {
        r.capped.push(format!(
            "kills: {:.1} a minute by abilities, {:.1} supplied",
            r.ability_kills_per_min, r.supply_per_min
        ));
        r.ability_kills_per_min = r.supply_per_min;
    }
    // Regeneration bonuses count as regeneration.
    for (a, casts) in &r.casts_per_min {
        let returned = r.returned_per_min.get(a).copied().unwrap_or(0.0);
        let regen = r.regen_per_min.entry(a.clone()).or_default();
        *regen = regen.max(casts - returned);
    }
    for (b, ss) in stack_seconds {
        let max = o
            .states
            .get(&b)
            .and_then(|st| st.max_stacks)
            .map_or(1.0, f64::from);
        let max = c.param(o, &format!("{b}.max_stacks")).unwrap_or(max);
        r.buffs.insert(
            b,
            Uptime {
                uptime: 1.0 - (-ss).exp(),
                mean_stacks: ss.min(max),
            },
        );
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::{Build, compile};
    use crate::economy::tests::economy;
    use crate::fluid::Engine;
    use crate::scenario::tests::scenario;
    use crate::variables::Variables;

    #[test]
    fn the_shard_loop_casts_faster_than_regeneration_alone() {
        let o = Ontology::builtin().unwrap();
        let base = crate::builtin_rules().unwrap();
        let vars = Variables::builtin().unwrap();
        let run = |parts: &[&str]| {
            let build = Build::parse(parts.iter().copied()).unwrap();
            let c = compile(&o, &base, &build, &vars);
            let e = economy();
            let s = scenario(6.97);
            let report = Engine::new(&o, &c, &e, &s).analyse(&seeds(&c));
            rates(&o, &c, &e, &s, &report)
        };
        let plain = run(&["melee:Shiver Strike"]);
        // No records for Shiver Strike itself: regeneration only.
        let regen = 60.0 * 2.75 / 146.0;
        assert!((plain.casts_per_min["melee"] - regen).abs() < 1e-6);
        assert!((plain.weapon_kills_per_min - plain.supply_per_min).abs() < 1e-6);
        let howl = run(&[
            "melee:Shiver Strike",
            "aspect:Howl of the Storm",
            "aspect:Tectonic Harvest",
            "fragment:Whisper of Hunger",
            "fragment:Whisper of Conduction",
            "fragment:Whisper of Rime",
        ]);
        assert!(
            howl.casts_per_min["melee"] > 2.0 * regen,
            "{:?}",
            howl.casts_per_min
        );
        // Tectonic Harvest's shards keep Frost Armor up.
        let frost = howl.buffs["frost_armor"];
        assert!(frost.uptime > 0.9 && frost.mean_stacks > 1.0, "{frost:?}");
        assert!(howl.heal_per_s > 0.0);
    }
}
