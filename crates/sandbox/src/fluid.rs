//! Fluid (mean-field) analysis: what an event leads to, on average, in a
//! scenario, and the gain of every loop (`docs/build-model.md` §5,
//! `docs/sandbox.md`).
//!
//! The analysis follows events through the compiled rules and the core
//! verbs, one **generation** at a time: from an ability cast, or from an
//! ignition that Scorch reaching x100 set off, until the next such event.
//! Those are the **boundaries**: an ability cast comes back when its energy
//! does, and an ignition comes back when its neighbours ignite. The
//! expected number of boundaries each boundary's generation reaches forms a
//! matrix whose spectral radius is the build's loop gain: above 1 a loop
//! feeds itself.
//!
//! The core verbs, written once here:
//! * **ignition**: hits its target and every neighbour within its radius.
//!   An ignition and what it sets off in the same place (a Fulmination) are
//!   one **burst**. A neighbour ignites in turn when the Scorch the burst
//!   puts on it reaches x100 and it survives the burst's damage;
//! * **Stasis crystals** freeze enemies within 2.6 m as they form, and are
//!   shattered; **frozen enemies** are shattered; every shatter leaves a
//!   **Stasis Shard** (large from elites), and a picked-up shard grants
//!   melee energy;
//! * **energy** arrives as the economy scales it, and returns as casts.
//!
//! What the analysis takes without data (the player's behaviour, unknown
//! radii) it says, in the report's notes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::compile::Compiled;
use crate::economy::{Economy, Scaling, stat_of};
use crate::ontology::Ontology;
use crate::rules::{Amount, Condition, Effect, Modifier, Rule, Trigger};
use crate::scenario::{Rank, Scenario};

/// An event as the analysis follows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub name: String,
    pub attrs: BTreeMap<String, String>,
    /// Damage and radius the event carries instead of its verb's own.
    pub damage: Option<f64>,
    pub radius: Option<f64>,
    pub tags: Vec<String>,
    pub element: Option<String>,
    /// Set off by an event of the same verb in the same place (a
    /// Fulmination by its ignition): part of that burst, not a new one.
    pub member: bool,
    /// Set off by a state reaching its maximum (an ignition by Scorch at
    /// x100): a boundary, where a chain's next generation starts.
    pub chained: bool,
}

impl Event {
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            attrs: BTreeMap::new(),
            damage: None,
            radius: None,
            tags: Vec::new(),
            element: None,
            member: false,
            chained: false,
        }
    }

    #[must_use]
    pub fn with(mut self, key: &str, value: &str) -> Self {
        self.attrs.insert(key.to_string(), value.to_string());
        self
    }

    /// An ability cast.
    #[must_use]
    pub fn cast(ability: &str) -> Self {
        Self::new("cast").with("ability", ability)
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)?;
        if !self.attrs.is_empty() {
            let a: Vec<String> = self.attrs.iter().map(|(k, v)| format!("{k}={v}")).collect();
            write!(f, "{{{}}}", a.join(", "))?;
        }
        if let Some(d) = self.damage {
            write!(f, " {d} dmg")?;
        }
        if let Some(r) = self.radius {
            write!(f, " {r} m")?;
        }
        if self.member {
            f.write_str(" (same burst)")?;
        }
        if self.chained {
            f.write_str(" (chained)")?;
        }
        Ok(())
    }
}

fn value_text(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Whether a trigger matches an event: same name, and every attribute it
/// names equal; an attribute the event lacks reads as `false`.
#[must_use]
pub fn matches(t: &Trigger, e: &Event) -> bool {
    t.event == e.name
        && t.attributes.iter().all(|(k, v)| {
            let want = value_text(v);
            e.attrs.get(k).map_or(want == "false", |have| *have == want)
        })
}

/// What a generation yields, per root event.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Yield {
    /// Damage dealt, before durability, and enemies killed.
    pub damage: f64,
    pub kills: f64,
    pub hits: f64,
    pub heal: f64,
    /// Charges of energy by ability (`melee`, `grenade`, `class`, `super`).
    pub energy: BTreeMap<String, f64>,
    /// Applications of buffs to the player, in stacks.
    pub buffs: BTreeMap<String, f64>,
    pub frozen: f64,
    pub pickups: f64,
    progress: BTreeMap<String, f64>,
    /// Firings of the event of a rule that spends a counter.
    spent_at: BTreeMap<String, f64>,
}

/// One generation: what a boundary event leads to before the next ones.
#[derive(Debug, Clone)]
pub struct Generation {
    pub root: Event,
    /// The boundary events reached, with their expected numbers.
    pub next: Vec<(Event, f64)>,
    pub yields: Yield,
}

/// A loop: boundaries that reach each other, and its gain.
#[derive(Debug, Clone)]
pub struct Loop {
    pub members: Vec<String>,
    pub gain: f64,
}

/// The analysis of a build in a scenario.
#[derive(Debug, Clone)]
pub struct Report {
    pub generations: Vec<Generation>,
    /// `gains[i][j]`: the expected boundary `j` events one boundary `i`
    /// event's generation reaches.
    pub gains: Vec<Vec<f64>>,
    pub loops: Vec<Loop>,
    /// The spectral radius of `gains`.
    pub gain: f64,
    /// Counter values at the moment they are spent (the operating point).
    pub counters: BTreeMap<String, f64>,
    /// The variables the result depends on.
    pub variables: BTreeSet<String>,
    pub notes: BTreeSet<String>,
}

const MAX_DEPTH: usize = 32;

/// The analysis engine.
#[derive(Debug)]
pub struct Engine<'a> {
    o: &'a Ontology,
    c: &'a Compiled<'a>,
    e: &'a Economy,
    s: &'a Scenario,
    /// Counters' values when spent.
    counters: BTreeMap<String, f64>,
    thresholds: BTreeMap<String, String>,
    notes: BTreeSet<String>,
}

impl<'a> Engine<'a> {
    #[must_use]
    pub fn new(o: &'a Ontology, c: &'a Compiled<'a>, e: &'a Economy, s: &'a Scenario) -> Self {
        let thresholds = o
            .states
            .values()
            .filter_map(|st| st.at_max.as_ref().map(|ev| (st.name.clone(), ev.clone())))
            .collect();
        Self {
            o,
            c,
            e,
            s,
            counters: BTreeMap::new(),
            thresholds,
            notes: BTreeSet::new(),
        }
    }

    fn note(&mut self, s: impl Into<String>) {
        self.notes.insert(s.into());
    }

    fn param(&self, path: &str) -> Option<f64> {
        self.c.param(self.o, path)
    }

    /// A variable's value (`data/sandbox/variables.toml`).
    fn var(&mut self, name: &str) -> f64 {
        if let Some(v) = self.c.vars.value(name) {
            v
        } else {
            self.note(format!("variable {name} is not defined; 0 taken"));
            0.0
        }
    }

    /// Ability casts, and events a state reaching its maximum set off,
    /// start generations.
    fn is_boundary(e: &Event) -> bool {
        e.name == "cast" || e.chained
    }

    /// Whether any granted rule applies a state.
    fn applies(&self, state: &str) -> bool {
        self.c
            .rules
            .iter()
            .flat_map(|r| r.effect.iter())
            .any(|e| e.apply.as_deref() == Some(state))
    }

    fn rank_share(&self, ranks: &[String]) -> f64 {
        Rank::ALL
            .iter()
            .filter(|r| ranks.iter().any(|x| x == r.name()))
            .map(|r| self.s.share(*r))
            .sum()
    }

    /// The probability a condition holds when its rule's event fires.
    fn holds(&mut self, c: &Condition) -> f64 {
        if let Some(b) = &c.has {
            self.note(format!("`has {b}`: held whenever the build applies it"));
            return f64::from(u8::from(self.applies(b)));
        }
        if let Some(b) = &c.lacks {
            return 1.0 - f64::from(u8::from(self.applies(b)));
        }
        if let Some(name) = &c.stacks {
            let min = c.min.unwrap_or(1.0);
            if let Some(v) = self.counters.get(name) {
                return f64::from(u8::from(*v + 1e-9 >= min));
            }
            if self.c.rules.iter().any(|r| {
                r.effect
                    .iter()
                    .any(|e| e.progress.as_deref() == Some(name.as_str()))
            }) {
                return 0.0;
            }
            return f64::from(u8::from(self.applies(name)));
        }
        if c.movement.is_some() {
            return self.var("policy.slide_form");
        }
        if let Some(v) = c.in_super {
            return if v { 0.0 } else { 1.0 };
        }
        if c.health_below.is_some() {
            return self.var("policy.critical_health");
        }
        if let Some(r) = c.enemies_within {
            let n = 1.0 + self.s.neighbours(r);
            return f64::from(u8::from(n >= c.min.unwrap_or(1.0)));
        }
        if let Some(st) = &c.target_has {
            self.note(format!(
                "`target_has {st}`: held whenever the build applies it"
            ));
            return f64::from(u8::from(self.applies(st)));
        }
        if let Some(ranks) = &c.target_rank {
            return self.rank_share(ranks);
        }
        if let Some(ranks) = &c.target_rank_not {
            return 1.0 - self.rank_share(ranks);
        }
        1.0
    }

    fn holds_all(&mut self, cs: &[Condition]) -> f64 {
        cs.iter().map(|c| self.holds(c)).product()
    }

    /// An amount's expected value in the scenario.
    fn amount(&mut self, a: &Amount) -> f64 {
        match a {
            Amount::Number(x) => *x,
            Amount::Table(t) => {
                if let Some(v) = &t.var {
                    return self.var(v);
                }
                if let Some(rows) = &t.by_counter {
                    let at = t
                        .counter
                        .as_ref()
                        .and_then(|c| self.counters.get(c))
                        .copied()
                        .unwrap_or(0.0);
                    return rows
                        .iter()
                        .filter(|[v, _]| at + 1e-9 >= *v)
                        .map(|[_, amount]| *amount)
                        .next_back()
                        .unwrap_or(0.0);
                }
                if let Some(by) = &t.by_rank {
                    return Rank::ALL
                        .iter()
                        .map(|r| self.s.share(*r) * by.get(r.name()).copied().unwrap_or(0.0))
                        .sum();
                }
                if let Some(by) = &t.by_tier {
                    self.note("amounts by tier: minors read as tier 1, elites as tier 2");
                    let at = |i: usize| {
                        by.get(i.min(by.len().saturating_sub(1)))
                            .copied()
                            .unwrap_or(0.0)
                    };
                    return self.s.share(Rank::Minor) * at(0) + self.s.share(Rank::Elite) * at(1);
                }
                if let Some(by) = &t.by_count {
                    return by.first().copied().unwrap_or(0.0);
                }
                0.0
            }
        }
    }

    fn damage_multiplier(&mut self, tags: &[String], element: Option<&str>) -> f64 {
        let mods: Vec<&Modifier> = self.c.modifiers.clone();
        let p: Vec<f64> = mods.iter().map(|m| self.holds_all(&m.when)).collect();
        self.c.damage_multiplier(self.o, tags, element, &p)
    }

    /// How many enemies an effect lands on.
    fn targets(&mut self, eff: &Effect) -> f64 {
        match eff.target.as_deref() {
            None | Some("target" | "enemies_hit" | "nearest") => 1.0,
            Some("area") => {
                if let Some(a) = &eff.radius {
                    let r = self.amount(a);
                    1.0 + self.s.neighbours(r)
                } else {
                    self.note("an area effect without a radius lands on its target only");
                    1.0
                }
            }
            Some(_) => 0.0,
        }
    }

    /// The slot (`melee`, `class_ability`, ...) of an ability name in the
    /// vocabulary (`melee`, `class`, ...).
    fn slot(ability: &str) -> &'static str {
        match ability {
            "grenade" => "grenade",
            "melee" => "melee",
            "class" => "class_ability",
            "super" => "super",
            _ => "movement",
        }
    }

    /// Energy for an ability, in charges, and the cast it returns as.
    fn energy(&mut self, ability: &str, amount: f64, scaling: Scaling) -> Option<(Event, f64)> {
        let slot = Self::slot(ability);
        let Some(name) = self.c.build.equipped(slot).map(str::to_string) else {
            self.note(format!("energy for {ability}: no {slot} is equipped"));
            return None;
        };
        if self.e.ability(slot, &name).is_none() {
            self.note(format!(
                "{name}: no chunk scalar in the economy tables; 1 taken"
            ));
        }
        let stat = self.s.stat(stat_of(slot));
        let documented = if scaling == Scaling::Fixed {
            1.0
        } else {
            self.var("economy.documented_chunk_scale")
        };
        let charges = self
            .e
            .chunk(slot, &name, stat, amount * documented, scaling);
        Some((Event::cast(ability), charges))
    }

    fn pickup_chance(&mut self) -> f64 {
        if self.c.modifies("stasis_shard.pickup_radius") {
            1.0
        } else {
            self.var("policy.shard_pickup")
        }
    }

    /// The children of one effect of a rule fired by `parent`.
    #[allow(clippy::too_many_lines, clippy::many_single_char_names)]
    fn effect(
        &mut self,
        eff: &Effect,
        parent: &Event,
        p: f64,
        y: &mut Yield,
        w: f64,
    ) -> Vec<(Event, f64)> {
        let mut out = Vec::new();
        let n = self.targets(eff);
        if let Some(state) = &eff.apply {
            if state == "frozen" {
                out.push((Event::new("freeze"), p * n));
            } else if self.thresholds.contains_key(state) {
                if parent.name != "ignition_hit" {
                    self.note(format!(
                        "{state} from separate applications adds up over time; not modelled"
                    ));
                }
            } else if matches!(eff.target.as_deref(), Some("self" | "allies" | "fireteam")) {
                let stacks = eff.stacks.as_ref().map_or(1.0, |a| self.amount(a));
                *y.buffs.entry(state.clone()).or_default() += w * p * stacks;
            }
        }
        if let (Some(a), None) = (&eff.damage, &eff.emit) {
            let base = self.amount(a);
            let mult = self.damage_multiplier(&eff.tags, eff.element.as_deref());
            let dmg = base * mult;
            y.damage += w * p * n * dmg;
            y.hits += w * p * n;
            let mut hit = Event::new("hit");
            if let Some(s) = &eff.source {
                hit = hit.with("source", s);
            }
            if let Some(el) = &eff.element {
                hit = hit.with("element", el);
            }
            out.push((hit.clone(), p * n));
            let kills = p * n * (1.0 - self.s.survives_mixed(dmg));
            y.kills += w * kills;
            if kills > 0.0 {
                let mut kill = hit;
                kill.name = "kill".into();
                out.push((kill, kills));
            }
        }
        if let Some(ability) = &eff.energy {
            let amount = eff.amount.as_ref().map_or(0.0, |a| self.amount(a));
            let scaling = Scaling::parse(eff.scaling.as_deref()).unwrap_or(Scaling::Chunk);
            if let Some((cast, charges)) = self.energy(ability, amount, scaling) {
                *y.energy.entry(ability.clone()).or_default() += w * p * charges;
                out.push((cast, p * charges));
            }
        }
        if let Some(ability) = &eff.charges {
            let count = eff.count.as_ref().map_or(1.0, |a| self.amount(a));
            *y.energy.entry(ability.clone()).or_default() += w * p * count;
            out.push((Event::cast(ability), p * count));
        }
        if eff.regen.is_some() {
            self.note("regeneration bonuses change rates, not generations; not in the gains");
        }
        if let Some(obj) = &eff.spawn {
            let sizes: Vec<(String, f64)> = if let Some(s) = &eff.sizes {
                s.iter().map(|(k, v)| (k.clone(), *v)).collect()
            } else {
                let count = eff.count.as_ref().map_or(1.0, |a| self.amount(a));
                vec![(eff.size.clone().unwrap_or_else(|| "small".into()), count)]
            };
            for (size, count) in sizes {
                if obj == "stasis_crystal" {
                    out.push((Event::new("crystal_created").with("size", &size), p * count));
                } else {
                    let chance = self.pickup_chance();
                    let pickup = Event::new("pickup").with("object", obj).with("size", &size);
                    out.push((pickup, p * count * chance));
                }
            }
        }
        if let Some(h) = &eff.heal {
            y.heal += w * p * self.amount(h);
        }
        if let Some(ev) = &eff.emit {
            let mut child = Event::new(ev);
            for (k, v) in eff.attributes.iter().flatten() {
                child.attrs.insert(k.clone(), value_text(v));
            }
            child.damage = eff.damage.as_ref().map(|a| self.amount(a));
            child.radius = eff.radius.as_ref().map(|a| self.amount(a));
            child.tags.clone_from(&eff.tags);
            child.element.clone_from(&eff.element);
            child.member = *ev == parent.name;
            out.push((child, p));
        }
        if let Some(c) = &eff.progress {
            let amount = eff.amount.as_ref().map_or(1.0, |a| self.amount(a));
            *y.progress.entry(c.clone()).or_default() += w * p * amount;
            let threshold = self.threshold(c);
            out.push((
                Event::new("counter_full").with("counter", c),
                p * amount / threshold,
            ));
        }
        out
    }

    fn threshold(&self, counter: &str) -> f64 {
        self.c
            .counters
            .get(counter)
            .copied()
            .unwrap_or(1.0)
            .max(1e-9)
    }

    /// The rules an event fires, with the probability each fires.
    fn fired(&mut self, e: &Event) -> Vec<(&'a Rule, f64)> {
        let rules: Vec<&'a Rule> = self
            .c
            .rules
            .iter()
            .copied()
            .filter(|r| matches(&r.on, e))
            .collect();
        let mut out = Vec::new();
        for r in rules {
            let chance = r.chance.as_ref().map_or(1.0, |a| self.amount(a));
            let p = self.holds_all(&r.when) * chance;
            if r.cooldown.is_some() || r.limit.is_some() {
                self.note(format!(
                    "{}: its cooldown or limit is not modelled in the gains",
                    r.granted_by
                ));
            }
            if p > 0.0 {
                out.push((r, p));
            }
        }
        out
    }

    /// The Scorch (and other threshold states) one hit puts on an enemy
    /// that is not the burst's origin: state → stacks.
    fn stacks_from(&mut self, hit: &Event) -> BTreeMap<String, f64> {
        let mut out = BTreeMap::new();
        for (r, p) in self.fired(hit) {
            for eff in &r.effect {
                let Some(state) = &eff.apply else { continue };
                if !self.thresholds.contains_key(state) {
                    continue;
                }
                let stacks = eff.stacks.as_ref().map_or(0.0, |a| self.amount(a));
                let applied = self.c.applied_stacks(self.o, state, stacks);
                *out.entry(state.clone()).or_default() += p * applied;
            }
        }
        out
    }

    /// An ignition: its hits, and for a burst's first ignition, the
    /// neighbours it kills and the ones it ignites.
    #[allow(clippy::many_single_char_names)]
    fn ignition(&mut self, e: &Event, y: &mut Yield, w: f64) -> Vec<(Event, f64)> {
        let mut out = Vec::new();
        let hit = |origin: bool| {
            let mut h =
                Event::new("ignition_hit").with("origin", if origin { "true" } else { "false" });
            if let Some(f) = e.attrs.get("fulmination") {
                h = h.with("fulmination", f);
            }
            h
        };
        let radius = e
            .radius
            .or_else(|| self.param("ignition.radius"))
            .unwrap_or(8.0);
        let neighbours = self.s.neighbours(radius);
        out.push((hit(true), 1.0));
        out.push((hit(false), neighbours));
        if e.member {
            return out;
        }
        // The burst: this ignition and what it sets off in the same place.
        let mut members = vec![(e.clone(), 1.0)];
        for (r, p) in self.fired(e) {
            for eff in &r.effect {
                if eff.emit.as_deref() == Some(e.name.as_str()) {
                    let mut m = Event::new(&e.name);
                    for (k, v) in eff.attributes.iter().flatten() {
                        m.attrs.insert(k.clone(), value_text(v));
                    }
                    m.damage = eff.damage.as_ref().map(|a| self.amount(a));
                    m.radius = eff.radius.as_ref().map(|a| self.amount(a));
                    m.tags.clone_from(&eff.tags);
                    m.member = true;
                    members.push((m, p));
                }
            }
        }
        // Each member's reach, damage and Scorch on a neighbour.
        let mut reach = Vec::new();
        for (m, p) in &members {
            let r = m
                .radius
                .or_else(|| self.param("ignition.radius"))
                .unwrap_or(8.0);
            let mut tags = m.tags.clone();
            if !tags.iter().any(|t| t == "ignition") {
                tags.push("ignition".into());
            }
            let base = m
                .damage
                .or_else(|| self.param("ignition.damage"))
                .unwrap_or(0.0);
            let dmg = base * self.damage_multiplier(&tags, Some("solar"));
            let mut h = Event::new("ignition_hit").with("origin", "false");
            if let Some(f) = m.attrs.get("fulmination") {
                h = h.with("fulmination", f);
            }
            let stacks = self.stacks_from(&h);
            reach.push((r, *p, dmg, stacks));
        }
        let origin_damage: f64 = reach.iter().map(|(_, p, d, _)| p * d).sum();
        y.damage += w * origin_damage;
        let mut radii: Vec<f64> = reach.iter().map(|(r, ..)| *r).collect();
        radii.sort_by(f64::total_cmp);
        radii.dedup();
        let mut inner = 0.0;
        let mut ignited: BTreeMap<String, f64> = BTreeMap::new();
        let mut killed = 1.0 - self.s.survives_mixed(origin_damage);
        for r in radii {
            let band = self.s.neighbours(r) - self.s.neighbours(inner);
            inner = r;
            let covering: Vec<&(f64, f64, f64, BTreeMap<String, f64>)> =
                reach.iter().filter(|(mr, ..)| *mr + 1e-9 >= r).collect();
            let damage: f64 = covering.iter().map(|(_, p, d, _)| p * d).sum();
            y.damage += w * band * damage;
            let alive = self.s.survives_mixed(damage);
            killed += band * (1.0 - alive);
            let mut stacks: BTreeMap<&str, f64> = BTreeMap::new();
            for (_, _, _, s) in &covering {
                for (state, n) in s {
                    *stacks.entry(state.as_str()).or_default() += n;
                }
            }
            for (state, n) in stacks {
                let max = self.o.states[state]
                    .max_stacks
                    .map_or(f64::INFINITY, f64::from);
                if n + 1e-9 >= max {
                    let at_max = self.thresholds[state].clone();
                    *ignited.entry(at_max).or_default() += band * alive;
                }
            }
        }
        y.kills += w * killed;
        if killed > 0.0 {
            out.push((
                Event::new("kill")
                    .with("source", "verb")
                    .with("element", "solar"),
                killed,
            ));
        }
        for (ev, n) in ignited {
            let mut child = Event::new(&ev);
            child.chained = true;
            out.push((child, n));
        }
        out
    }

    /// Everything one event leads to directly: the core verb's own
    /// consequences, then the rules it fires.
    fn children(&mut self, e: &Event, y: &mut Yield, w: f64) -> Vec<(Event, f64)> {
        let mut out = Vec::new();
        match e.name.as_str() {
            "ignition" => out.extend(self.ignition(e, y, w)),
            "crystal_created" => {
                let r = self.param("stasis_crystal.freeze_radius").unwrap_or(2.6);
                let in_pack = self.var("policy.crystals_in_pack");
                out.push((Event::new("freeze"), in_pack * self.s.around(r)));
                out.push((Event::new("shatter").with("of", "crystal"), 1.0));
            }
            "shatter" => {
                let chance = self.pickup_chance();
                let shard = |size: &str| {
                    Event::new("pickup")
                        .with("object", "stasis_shard")
                        .with("size", size)
                };
                if e.attrs.get("of").map(String::as_str) == Some("enemy") {
                    out.push((shard("small"), chance * self.s.share(Rank::Minor)));
                    out.push((shard("large"), chance * self.s.share(Rank::Elite)));
                } else {
                    out.push((shard("small"), chance));
                }
            }
            "pickup" => {
                y.pickups += w;
                if e.attrs.get("object").map(String::as_str) == Some("stasis_shard") {
                    let size = e.attrs.get("size").map_or("small", String::as_str);
                    let amount = self
                        .param(&format!("stasis_shard.melee_energy_{size}"))
                        .unwrap_or(0.0);
                    if let Some((cast, charges)) = self.energy("melee", amount, Scaling::Chunk) {
                        *y.energy.entry("melee".into()).or_default() += w * charges;
                        out.push((cast, charges));
                    }
                }
            }
            _ => {}
        }
        // A counter's value when spent: what it gained since the last time
        // the spending rule's event fired, whether or not it then held.
        for r in self.c.rules.iter().filter(|r| matches(&r.on, e)) {
            for c in r.effect.iter().filter_map(|eff| eff.consume.as_ref()) {
                if self.c.counters.contains_key(c) {
                    *y.spent_at.entry(c.clone()).or_default() += w;
                }
            }
        }
        for (r, p) in self.fired(e) {
            for eff in &r.effect {
                let kids = self.effect(eff, e, p, y, w);
                out.extend(kids);
            }
        }
        out
    }

    /// One generation from a boundary event.
    pub fn generation(&mut self, root: &Event) -> Generation {
        let mut y = Yield::default();
        let mut next: Vec<(Event, f64)> = Vec::new();
        let mut freezes = 0.0;
        let mut stack = vec![(root.clone(), 1.0, 0usize)];
        for phase in 0..2 {
            while let Some((e, w, depth)) = stack.pop() {
                if depth > 0 && Self::is_boundary(&e) {
                    match next.iter_mut().find(|(n, _)| *n == e) {
                        Some((_, c)) => *c += w,
                        None => next.push((e, w)),
                    }
                    continue;
                }
                if e.name == "freeze" && phase == 0 {
                    freezes += w;
                    continue;
                }
                if depth > MAX_DEPTH {
                    self.note("a chain of events deeper than 32 was cut short");
                    continue;
                }
                for (child, n) in self.children(&e, &mut y, w) {
                    if n > 1e-12 {
                        stack.push((child, w * n, depth + 1));
                    }
                }
            }
            // Freezes overlap: the enemies frozen are a share of the pack.
            if phase == 0 && freezes > 0.0 {
                let pack = self.s.pack.size.max(1.0);
                let frozen = pack * (1.0 - (-freezes / pack).exp());
                y.frozen = frozen;
                let shattered = frozen * self.var("policy.shatter_frozen");
                stack.push((Event::new("shatter").with("of", "enemy"), shattered, 1));
            }
        }
        Generation {
            root: root.clone(),
            next,
            yields: y,
        }
    }

    /// Analyse from seed events: every boundary they reach, generation by
    /// generation, with counters at their operating point.
    pub fn analyse(&mut self, seeds: &[Event]) -> Report {
        let mut gens = Vec::new();
        for _ in 0..12 {
            gens = self.explore(seeds);
            // A counter spent once a generation holds what a generation adds.
            let mut changed = false;
            for g in &gens {
                for (c, spent) in &g.yields.spent_at {
                    if *spent <= 1e-12 {
                        continue;
                    }
                    let added = g.yields.progress.get(c).copied().unwrap_or(0.0);
                    let v = added / spent;
                    let old = self.counters.insert(c.clone(), v);
                    changed |= old.is_none_or(|o| (o - v).abs() > 1e-9);
                }
            }
            if !changed {
                break;
            }
        }
        let keys: Vec<Event> = gens.iter().map(|g| g.root.clone()).collect();
        let gains: Vec<Vec<f64>> = gens
            .iter()
            .map(|g| {
                keys.iter()
                    .map(|k| g.next.iter().filter(|(e, _)| e == k).map(|(_, n)| n).sum())
                    .collect()
            })
            .collect();
        let loops = loops(&keys, &gains);
        let gain = spectral_radius(&gains);
        Report {
            generations: gens,
            gains,
            loops,
            gain,
            counters: self.counters.clone(),
            variables: self
                .c
                .vars
                .take_read()
                .into_iter()
                .chain(self.s.variables.iter().cloned())
                .collect(),
            notes: std::mem::take(&mut self.notes),
        }
    }

    fn explore(&mut self, seeds: &[Event]) -> Vec<Generation> {
        let mut gens: Vec<Generation> = Vec::new();
        let mut queue: Vec<Event> = seeds.to_vec();
        while let Some(e) = queue.pop() {
            if gens.iter().any(|g| g.root == e) {
                continue;
            }
            let g = self.generation(&e);
            for (n, _) in &g.next {
                if !gens.iter().any(|g| g.root == *n) && !queue.contains(n) {
                    queue.push(n.clone());
                }
            }
            gens.push(g);
        }
        gens
    }
}

/// The spectral radius of a non-negative matrix: the power iteration of
/// `M + I`, whose leading eigenvalue is the Perron root plus one and which
/// is aperiodic, so the iteration settles where `M`'s would oscillate.
#[must_use]
pub fn spectral_radius(m: &[Vec<f64>]) -> f64 {
    let n = m.len();
    if n == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let mut v = vec![1.0 / n as f64; n];
    let mut rho = 0.0;
    for _ in 0..10_000 {
        let mut next = v.clone();
        for (i, row) in m.iter().enumerate() {
            for (j, a) in row.iter().enumerate() {
                next[j] += a * v[i];
            }
        }
        let norm: f64 = next.iter().sum();
        for x in &mut next {
            *x /= norm;
        }
        let converged = (norm - 1.0 - rho).abs() < 1e-12;
        rho = norm - 1.0;
        v = next;
        if converged {
            break;
        }
    }
    rho.max(0.0)
}

/// The loops of a gain matrix: its strongly connected components that
/// reach themselves, each with its own spectral radius.
fn loops(keys: &[Event], m: &[Vec<f64>]) -> Vec<Loop> {
    let n = keys.len();
    let reach = |i: usize, j: usize| m[i][j] > 1e-12;
    // Transitive closure (the graphs are small).
    let mut r = vec![vec![false; n]; n];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, x) in row.iter_mut().enumerate() {
            *x = reach(i, j);
        }
    }
    for k in 0..n {
        let via = r[k].clone();
        for row in &mut r {
            if row[k] {
                for (x, v) in row.iter_mut().zip(&via) {
                    *x |= *v;
                }
            }
        }
    }
    let mut seen = vec![false; n];
    let mut out = Vec::new();
    for i in 0..n {
        if seen[i] || !r[i][i] {
            continue;
        }
        let members: Vec<usize> = (0..n).filter(|&j| r[i][j] && r[j][i]).collect();
        for &j in &members {
            seen[j] = true;
        }
        let sub: Vec<Vec<f64>> = members
            .iter()
            .map(|&a| members.iter().map(|&b| m[a][b]).collect())
            .collect();
        out.push(Loop {
            members: members.iter().map(|&j| keys[j].to_string()).collect(),
            gain: spectral_radius(&sub),
        });
    }
    out.sort_by(|a, b| b.gain.total_cmp(&a.gain));
    out
}

/// A scenario setting a threshold is searched along.
#[derive(Debug, Clone, PartialEq)]
pub enum Knob {
    /// Enemies per pack.
    PackSize,
    /// Damage to kill over health.
    Durability,
    /// A stat's value (`melee`, `grenade`, ...).
    Stat(String),
}

impl Knob {
    /// The scenario with the knob set to `x`.
    #[must_use]
    pub fn set(&self, s: &Scenario, x: f64) -> Scenario {
        let mut s = s.clone();
        match self {
            Knob::PackSize => s.pack.size = x,
            Knob::Durability => s.durability = x,
            Knob::Stat(name) => {
                s.stats.insert(name.clone(), x);
            }
        }
        s
    }
}

impl fmt::Display for Knob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Knob::PackSize => f.write_str("pack size"),
            Knob::Durability => f.write_str("durability"),
            Knob::Stat(s) => write!(f, "{s} stat"),
        }
    }
}

/// The gain of the loop one of whose members starts with `member` (its
/// display), or 0.
#[must_use]
pub fn loop_gain(r: &Report, member: &str) -> f64 {
    r.loops
        .iter()
        .find(|l| l.members.iter().any(|m| m.starts_with(member)))
        .map_or(0.0, |l| l.gain)
}

/// What a threshold search runs: the build, its economy and scenario, and
/// the seeds.
#[derive(Debug, Clone, Copy)]
pub struct Setup<'a> {
    pub ontology: &'a Ontology,
    pub compiled: &'a Compiled<'a>,
    pub economy: &'a Economy,
    pub scenario: &'a Scenario,
    pub seeds: &'a [Event],
}

/// Where the gain of the loop through `member` crosses 1 as `knob` moves
/// from `lo` to `hi`.
#[must_use]
pub fn threshold(setup: Setup<'_>, member: &str, knob: &Knob, (lo, hi): (f64, f64)) -> Option<f64> {
    crossing(lo, hi, |x| {
        let scenario = knob.set(setup.scenario, x);
        let report = Engine::new(setup.ontology, setup.compiled, setup.economy, &scenario)
            .analyse(setup.seeds);
        loop_gain(&report, member)
    })
}

/// Where `f` crosses 1 between `lo` and `hi`, by bisection; None when it
/// does not change side.
pub fn crossing(lo: f64, hi: f64, mut f: impl FnMut(f64) -> f64) -> Option<f64> {
    let (mut a, mut b) = (lo, hi);
    let (fa, fb) = (f(a) - 1.0, f(b) - 1.0);
    if fa.signum() == fb.signum() {
        return None;
    }
    for _ in 0..60 {
        let mid = f64::midpoint(a, b);
        if (f(mid) - 1.0).signum() == fa.signum() {
            a = mid;
        } else {
            b = mid;
        }
    }
    Some(f64::midpoint(a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::{Build, compile};
    use crate::economy::tests::economy;
    use crate::scenario::tests::scenario;

    #[allow(clippy::many_single_char_names)]
    fn gain(parts: &[&str], seed: Event, durability: f64) -> Report {
        let o = Ontology::builtin().unwrap();
        let base = crate::builtin_rules().unwrap();
        let build = Build::parse(parts.iter().copied()).unwrap();
        // The acceptance cases are pinned to stated values, not to the
        // variables' current guesses.
        let mut v = crate::variables::Variables::builtin().unwrap();
        for (name, x) in [
            ("scenario.pack_size", 8.0),
            ("scenario.pack_radius", 10.0),
            ("scenario.elite_share", 0.25),
            ("build.default_stat", 70.0),
            ("howl_of_the_storm.freeze_radius", 3.0),
            ("policy.shard_pickup", 0.75),
            ("policy.shatter_frozen", 1.0),
            ("policy.crystals_in_pack", 1.0),
            ("policy.slide_form", 1.0),
            ("economy.documented_chunk_scale", 1.0),
        ] {
            v.set(name, x).unwrap();
        }
        let vars = Box::leak(Box::new(v));
        let c = compile(&o, &base, &build, vars);
        let e = economy();
        let mut s = scenario(durability);
        s.pack = crate::scenario::Pack {
            size: 8.0,
            radius_m: 10.0,
        };
        Engine::new(&o, &c, &e, &s).analyse(&[seed])
    }

    const SOLAR: [&str; 4] = [
        "fragment:Ember of Char",
        "fragment:Ember of Ashes",
        "armor:Dawn Chorus",
        "melee:Incinerator Snap",
    ];

    #[test]
    fn the_ignition_chain_needs_fulmination_and_survivors() {
        let with: Vec<&str> = SOLAR
            .iter()
            .copied()
            .chain(["artifact_perk:Solar Fulmination"])
            .collect();
        // Ultimate Conquest: neighbours take 60 + 60 Scorch and survive.
        let hard = gain(&with, Event::new("ignition"), 6.97);
        let r_hard = loop_gain(&hard, "ignition");
        assert!(r_hard > 1.0, "hard: {r_hard} {:?}", hard.loops);
        // Without Fulmination a neighbour takes 60: no chain.
        let without = gain(&SOLAR, Event::new("ignition"), 6.97);
        assert!(
            loop_gain(&without, "ignition") < 1e-9,
            "{:?}",
            without.loops
        );
        // Easy content: the burst kills the neighbours first.
        let easy = gain(&with, Event::new("ignition"), 1.0);
        let r_easy = loop_gain(&easy, "ignition");
        assert!(r_easy < 1.0, "easy: {r_easy}");
        // Dawn Chorus's melee energy: every ignition hit, Fulminations too.
        let g = &hard
            .generations
            .iter()
            .find(|g| g.root.name == "ignition")
            .unwrap();
        assert!(g.yields.energy["melee"] > 0.0);
    }

    #[test]
    fn the_shard_loop_crosses_one_with_hunger_and_converter() {
        let base = [
            "aspect:Howl of the Storm",
            "melee:Shiver Strike",
            "fragment:Whisper of Conduction",
        ];
        let run = |extra: &[&str]| {
            let parts: Vec<&str> = base.iter().chain(extra).copied().collect();
            let r = gain(&parts, Event::cast("melee"), 6.97);
            (loop_gain(&r, "cast"), r)
        };
        let (none, _) = run(&[]);
        let (hunger, _) = run(&["fragment:Whisper of Hunger"]);
        let (converter, rc) = run(&["artifact_perk:Crystalline Converter"]);
        let (both, rb) = run(&[
            "fragment:Whisper of Hunger",
            "artifact_perk:Crystalline Converter",
        ]);
        assert!(none < 1.0, "none {none}");
        assert!(both > 1.0, "both {both}");
        assert!(none < hunger && hunger < both, "{none} {hunger} {both}");
        assert!(
            none < converter && converter < both,
            "{none} {converter} {both}"
        );
        // Converter's count at the melee hit that spends it.
        assert!(rc.counters["converter_shards"] >= 8.0, "{:?}", rc.counters);
        assert!(rb.counters["converter_shards"] >= rc.counters["converter_shards"]);
    }

    #[test]
    fn spectral_radius_and_crossing() {
        assert!((spectral_radius(&[vec![0.0, 2.0], vec![0.5, 0.0]]) - 1.0).abs() < 1e-6);
        assert!((spectral_radius(&[vec![1.5]]) - 1.5).abs() < 1e-9);
        let x = crossing(0.0, 10.0, |x| x / 4.0).unwrap();
        assert!((x - 4.0).abs() < 1e-9);
        assert!(crossing(0.0, 1.0, |_| 2.0).is_none());
    }
}
