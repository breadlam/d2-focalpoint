//! The discrete-event simulator (`docs/build-model.md` §5): a seeded Monte
//! Carlo of a build fighting packs of enemies, on the same compiled records
//! as the fluid analysis ([`crate::fluid`]).
//!
//! Where the analysis takes expected values, the simulator draws them:
//!
//! * **Enemies** are concrete: each pack's enemies stand at sampled points
//!   of a disc with health drawn from the scenario's archetypes. Packs
//!   arrive at the scenario's rate of kills.
//! * **Weapons** wear the enemies down at a steady rate, sized so they
//!   alone could kill what the scenario supplies (`sim.weapon_headroom`
//!   times over). Ability damage that kills nothing outright adds up with
//!   theirs on the same health bars, and the final blow decides the kill.
//! * **The player** regenerates energy by the economy and casts each
//!   ability when charged, at most once per `policy.min_cast_interval`,
//!   at a live enemy.
//! * **Rules** fire on concrete events. Chances, conditions, a weapon
//!   kill's precision and element and the player's movement are drawn;
//!   cooldowns and per-activation limits are kept per rule.
//! * **Core verbs** act on enemies and positions: ignitions, freezes and
//!   shatters, crystals, Jolt chains, Volatile explosions, Unravel threads,
//!   Bolt Charge discharges, Threadlings, Tangles, Scorch's damage over
//!   time and decay, Stasis Shards and other pickups.
//!
//! Damage counts towards kills by the same efficiencies as the analysis
//! (`scenario.ability_efficiency`, `scenario.direct_hit_efficiency`) and
//! spreads by `scenario.damage_spread`, so the two differ only by what
//! averaging leaves out.

#![allow(clippy::many_single_char_names)]

use std::collections::BTreeMap;

use crate::compile::{Build, Compiled, compile};
use crate::economy::{Economy, Scaling, stat_of};
use crate::fluid::{attr_eq, value_text};
use crate::ontology::Ontology;
use crate::rules::{Amount, Condition, Effect, RuleBase, Trigger};
use crate::scenario::{Rank, Scenario};
use crate::variables::Variables;

/// The time step, seconds.
const DT: f64 = 0.05;
/// Events one step may process: a runaway loop stops here.
const MAX_EVENTS: usize = 50_000;
/// Abilities in the vocabulary's names, in casting priority.
const ABILITIES: [&str; 4] = ["super", "grenade", "melee", "class"];

/// How long and how often to simulate.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// Seconds a run lasts.
    pub seconds: f64,
    /// Independent runs, spread over the machine's cores.
    pub runs: usize,
    pub seed: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            seconds: 600.0,
            runs: 16,
            seed: 1,
        }
    }
}

/// What the runs did, per minute or second over all of them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    pub minutes: f64,
    pub runs: usize,
    /// Casts a minute by ability.
    pub casts_per_min: BTreeMap<String, f64>,
    pub ability_kills_per_min: f64,
    pub weapon_kills_per_min: f64,
    /// Kills a minute by what landed the final blow (`grenade`, `verb:ignition`, ...).
    pub kills_by: BTreeMap<String, f64>,
    /// Ability and verb damage a second, before efficiency and durability
    /// (as the analysis counts it).
    pub damage_per_s: f64,
    pub heal_per_s: f64,
    /// Core events a minute (ignitions, freezes, shatters, pickups, ...).
    pub events_per_min: BTreeMap<String, f64>,
    /// Share of the time each buff on the player is up.
    pub uptime: BTreeMap<String, f64>,
}

impl Outcome {
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

    fn add(&mut self, o: &Outcome) {
        fn merge(a: &mut BTreeMap<String, f64>, b: &BTreeMap<String, f64>) {
            for (k, v) in b {
                *a.entry(k.clone()).or_default() += v;
            }
        }
        self.minutes += o.minutes;
        self.runs += o.runs;
        merge(&mut self.casts_per_min, &o.casts_per_min);
        self.ability_kills_per_min += o.ability_kills_per_min;
        self.weapon_kills_per_min += o.weapon_kills_per_min;
        merge(&mut self.kills_by, &o.kills_by);
        self.damage_per_s += o.damage_per_s;
        self.heal_per_s += o.heal_per_s;
        merge(&mut self.events_per_min, &o.events_per_min);
        merge(&mut self.uptime, &o.uptime);
    }

    /// Totals into rates: per minute, per second, shares of time.
    fn normalise(&mut self) {
        let minutes = self.minutes.max(1e-9);
        for v in self.casts_per_min.values_mut() {
            *v /= minutes;
        }
        self.ability_kills_per_min /= minutes;
        self.weapon_kills_per_min /= minutes;
        for v in self.kills_by.values_mut() {
            *v /= minutes;
        }
        self.damage_per_s /= 60.0 * minutes;
        self.heal_per_s /= 60.0 * minutes;
        for v in self.events_per_min.values_mut() {
            *v /= minutes;
        }
        for v in self.uptime.values_mut() {
            *v /= 60.0 * minutes;
        }
    }
}

/// Simulate a build in a scenario: `config.runs` runs of `config.seconds`,
/// in parallel, merged.
#[must_use]
pub fn simulate(
    o: &Ontology,
    base: &RuleBase,
    build: &Build,
    vars: &Variables,
    e: &Economy,
    s: &Scenario,
    config: Config,
) -> Outcome {
    let threads = std::thread::available_parallelism()
        .map_or(4, std::num::NonZeroUsize::get)
        .min(config.runs.max(1));
    let mut total = Outcome::default();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                // Variables keep a record of what is read, so each thread
                // compiles its own copy.
                let vars = vars.clone();
                scope.spawn(move || {
                    let c = compile(o, base, build, &vars);
                    let mut out = Outcome::default();
                    for run in (t..config.runs).step_by(threads) {
                        let seed = config.seed ^ (run as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                        let mut sim = Sim::new(o, &c, e, s, seed);
                        sim.run(config.seconds);
                        out.add(&sim.out);
                    }
                    out
                })
            })
            .collect();
        for h in handles {
            if let Ok(o) = h.join() {
                total.add(&o);
            }
        }
    });
    total.normalise();
    total
}

/// The mean number of `event`s each one directly sets off, one generation
/// on, in a fresh pack with no weapons or casts: the simulator's measure of
/// a chain's branching, against the analysis's loop gain.
#[must_use]
pub fn offspring(
    o: &Ontology,
    c: &Compiled<'_>,
    e: &Economy,
    s: &Scenario,
    event: &str,
    trials: usize,
    seed: u64,
) -> f64 {
    let mut children = 0.0;
    for i in 0..trials {
        let mut sim = Sim::new(
            o,
            c,
            e,
            s,
            seed ^ (i as u64).wrapping_mul(0xA24B_AED4_963E_E407),
        );
        sim.spawn_pack();
        let Some(target) = sim.random_alive() else {
            continue;
        };
        let pos = sim.enemies[target].pos;
        sim.process(Ev::new(event, Some(target), pos, 0));
        // The delayed events (chained ignitions, shatters) of a few seconds.
        for _ in 0..200 {
            sim.t += DT;
            sim.due();
        }
        children += sim
            .generations
            .get(&(event.to_string(), 1))
            .copied()
            .unwrap_or(0.0);
    }
    #[allow(clippy::cast_precision_loss)]
    let n = trials.max(1) as f64;
    children / n
}

/// `SplitMix64`: small, fast and seedable.
#[derive(Debug, Clone)]
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    #[allow(clippy::cast_precision_loss)]
    fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn chance(&mut self, p: f64) -> bool {
        self.f64() < p
    }

    /// A standard normal draw (Box-Muller).
    fn normal(&mut self) -> f64 {
        let u = self.f64().max(1e-12);
        let v = self.f64();
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn index(&mut self, n: usize) -> usize {
        ((self.f64() * n as f64) as usize).min(n.saturating_sub(1))
    }

    /// `x` rounded up or down at random, so the mean is `x`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn round(&mut self, x: f64) -> usize {
        let x = x.max(0.0);
        let whole = x.floor();
        whole as usize + usize::from(self.chance(x - whole))
    }
}

/// A state on an enemy or the player.
#[derive(Debug, Clone, Copy)]
struct Held {
    stacks: f64,
    /// When it ends (or, for a stacking state, when its next stack does).
    until: f64,
    /// When it was last applied (Scorch decays after a linger).
    last: f64,
}

#[derive(Debug, Clone)]
struct Enemy {
    pos: (f64, f64),
    health: f64,
    rank: Rank,
    alive: bool,
    states: BTreeMap<String, Held>,
    /// When it last chained lightning or released threads.
    last_chain: f64,
    last_thread: f64,
    next_scorch_tick: f64,
}

/// A concrete event.
#[derive(Debug, Clone)]
struct Ev {
    name: String,
    attrs: BTreeMap<String, String>,
    target: Option<usize>,
    pos: (f64, f64),
    damage: Option<f64>,
    radius: Option<f64>,
    tags: Vec<String>,
    element: Option<String>,
    /// Chained generations since the seed (an ignition set off by one is
    /// one on).
    generation: u32,
    /// Set off by an event of the same verb in the same place (a
    /// Fulmination by its ignition): part of that burst, not a new one.
    member: bool,
}

impl Ev {
    fn new(name: &str, target: Option<usize>, pos: (f64, f64), generation: u32) -> Self {
        Self {
            name: name.to_string(),
            attrs: BTreeMap::new(),
            target,
            pos,
            damage: None,
            radius: None,
            tags: Vec::new(),
            element: None,
            generation,
            member: false,
        }
    }

    fn with(mut self, k: &str, v: &str) -> Self {
        self.attrs.insert(k.to_string(), v.to_string());
        self
    }
}

/// How damage counts towards a kill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// A single-target hit: `scenario.direct_hit_efficiency`.
    Direct,
    /// Area and verb damage: `scenario.ability_efficiency`.
    Area,
    /// Weapons: whole.
    Weapon,
}

/// Damage being dealt.
#[derive(Debug, Clone)]
struct Blow {
    amount: f64,
    source: String,
    element: Option<String>,
    /// The verb dealing it (`ignition`, `jolt_chain`, ...).
    by: Option<String>,
    reach: Reach,
    /// Whether it is a hit rules fire on.
    hit: bool,
    powered: bool,
    generation: u32,
}

struct Sim<'a> {
    o: &'a Ontology,
    c: &'a Compiled<'a>,
    e: &'a Economy,
    s: &'a Scenario,
    rng: Rng,
    t: f64,
    enemies: Vec<Enemy>,
    next_pack: f64,
    /// Ability energy, in charges.
    energy: BTreeMap<&'static str, f64>,
    last_cast: BTreeMap<&'static str, f64>,
    buffs: BTreeMap<String, Held>,
    /// Regeneration bonuses by `(ability, thousandths)`: when each ends.
    regen: BTreeMap<(String, u32), f64>,
    counters: BTreeMap<String, f64>,
    /// When each rule may fire again, by its index.
    ready_at: BTreeMap<usize, f64>,
    /// Firings of limited rules since the last super.
    fired: BTreeMap<usize, u32>,
    delayed: Vec<(f64, Ev)>,
    weapon_target: Option<usize>,
    /// Events by `(name, generation)`.
    generations: BTreeMap<(String, u32), f64>,
    out: Outcome,
    weapons: bool,
}

impl<'a> Sim<'a> {
    fn new(
        o: &'a Ontology,
        c: &'a Compiled<'a>,
        e: &'a Economy,
        s: &'a Scenario,
        seed: u64,
    ) -> Self {
        Self {
            o,
            c,
            e,
            s,
            rng: Rng(seed),
            t: 0.0,
            enemies: Vec::new(),
            next_pack: 0.0,
            energy: ABILITIES.iter().map(|a| (*a, 0.0)).collect(),
            last_cast: BTreeMap::new(),
            buffs: BTreeMap::new(),
            regen: BTreeMap::new(),
            counters: BTreeMap::new(),
            ready_at: BTreeMap::new(),
            fired: BTreeMap::new(),
            delayed: Vec::new(),
            weapon_target: None,
            generations: BTreeMap::new(),
            out: Outcome {
                runs: 1,
                ..Outcome::default()
            },
            weapons: true,
        }
    }

    fn var(&self, name: &str) -> f64 {
        self.c.vars.value(name).unwrap_or(0.0)
    }

    fn param(&self, path: &str) -> Option<f64> {
        self.c.param(self.o, path)
    }

    /// Enemies supplied a second.
    fn supply(&self) -> f64 {
        self.s.supply_per_min.map_or_else(
            || self.s.pack.size * self.c.vars.engine("scenario.packs_per_minute", 1.5) / 60.0,
            |k| k / 60.0,
        )
    }

    fn run(&mut self, seconds: f64) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let steps = (seconds / DT).ceil() as usize;
        let supply = self.supply().max(1e-6);
        let interval = self.s.pack.size.max(1.0) / supply;
        let headroom = self.c.vars.engine("sim.weapon_headroom", 1.3);
        let weapon_dps = headroom * supply * self.s.mean_health();
        for _ in 0..steps {
            self.t += DT;
            let alive = self.enemies.iter().filter(|e| e.alive).count();
            if self.t >= self.next_pack {
                #[allow(clippy::cast_precision_loss)]
                if (alive as f64) < 3.0 * self.s.pack.size {
                    self.spawn_pack();
                }
                self.next_pack += interval;
            }
            self.expire();
            self.scorch_ticks();
            self.regenerate();
            self.due();
            if self.weapons {
                self.fire_weapons(weapon_dps * DT);
            }
            self.cast();
            for (name, held) in &self.buffs {
                if held.stacks > 0.0 {
                    *self.out.uptime.entry(name.clone()).or_default() += DT;
                }
            }
            // Forget the dead, now and then.
            if self.enemies.len() > 200 {
                self.compact();
            }
        }
        self.out.minutes = seconds / 60.0;
    }

    fn compact(&mut self) {
        let mut map = Vec::with_capacity(self.enemies.len());
        let mut kept = Vec::new();
        for (i, e) in self.enemies.iter().enumerate() {
            if e.alive {
                map.push(Some(kept.len()));
                kept.push(e.clone());
            } else {
                map.push(None);
            }
            let _ = i;
        }
        self.enemies = kept;
        self.weapon_target = self.weapon_target.and_then(|i| map[i]);
        self.delayed.retain_mut(|(_, ev)| match ev.target {
            Some(i) => match map[i] {
                Some(j) => {
                    ev.target = Some(j);
                    true
                }
                None => false,
            },
            None => true,
        });
    }

    fn spawn_pack(&mut self) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = self.s.pack.size.round().max(1.0) as usize;
        let r = self.s.pack.radius_m;
        for _ in 0..n {
            let rank = if self.rng.chance(self.s.elite_share) {
                Rank::Elite
            } else {
                Rank::Minor
            };
            let healths = match rank {
                Rank::Minor => &self.s.minor_health,
                Rank::Elite => &self.s.elite_health,
            };
            let health = if healths.is_empty() {
                1.0
            } else {
                healths[self.rng.index(healths.len())] * self.s.durability
            };
            let d = r * self.rng.f64().sqrt();
            let a = std::f64::consts::TAU * self.rng.f64();
            self.enemies.push(Enemy {
                pos: (d * a.cos(), d * a.sin()),
                health,
                rank,
                alive: true,
                states: BTreeMap::new(),
                last_chain: f64::NEG_INFINITY,
                last_thread: f64::NEG_INFINITY,
                next_scorch_tick: 0.0,
            });
        }
    }

    fn random_alive(&mut self) -> Option<usize> {
        let alive: Vec<usize> = (0..self.enemies.len())
            .filter(|i| self.enemies[*i].alive)
            .collect();
        if alive.is_empty() {
            None
        } else {
            Some(alive[self.rng.index(alive.len())])
        }
    }

    fn within(&self, pos: (f64, f64), r: f64) -> Vec<usize> {
        (0..self.enemies.len())
            .filter(|i| {
                let e = &self.enemies[*i];
                e.alive && dist(e.pos, pos) <= r
            })
            .collect()
    }

    fn nearest(&self, pos: (f64, f64), r: f64, except: Option<usize>, n: usize) -> Vec<usize> {
        let mut v: Vec<(f64, usize)> = (0..self.enemies.len())
            .filter(|i| Some(*i) != except && self.enemies[*i].alive)
            .map(|i| (dist(self.enemies[i].pos, pos), i))
            .filter(|(d, _)| *d <= r)
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v.into_iter().take(n).map(|(_, i)| i).collect()
    }

    // ------------------------------------------------------------ time

    /// States running out, and decaying stacks.
    fn expire(&mut self) {
        let t = self.t;
        let mut buffs = std::mem::take(&mut self.buffs);
        for (name, h) in &mut buffs {
            if h.stacks <= 0.0 || t < h.until {
                continue;
            }
            // A stacking buff loses one stack per `stack_duration`.
            if let Some(per) = self.param(&format!("{name}.stack_duration")) {
                h.stacks -= 1.0;
                h.until = t + per;
            } else {
                h.stacks = 0.0;
            }
        }
        buffs.retain(|_, h| h.stacks > 0.0);
        self.buffs = buffs;
        let decays: BTreeMap<String, (f64, f64)> = ["scorch", "slow"]
            .iter()
            .filter_map(|s| {
                let rate = self.param(&format!("{s}.decay_per_s"))?;
                let linger = self.param(&format!("{s}.linger")).unwrap_or(0.0);
                Some(((*s).to_string(), (rate, linger)))
            })
            .collect();
        for e in &mut self.enemies {
            if !e.alive {
                continue;
            }
            e.states.retain(|name, h| {
                if let Some((rate, linger)) = decays.get(name) {
                    if t > h.last + linger {
                        h.stacks -= rate * DT;
                    }
                    h.stacks > 0.0
                } else {
                    t < h.until
                }
            });
        }
    }

    /// Scorch's damage over time.
    fn scorch_ticks(&mut self) {
        let Some(interval) = self.param("scorch.tick_interval") else {
            return;
        };
        let base = self.param("scorch.tick_base").unwrap_or(0.0);
        let per = self.param("scorch.tick_per_stack").unwrap_or(0.0);
        let bonus = 1.0 + self.param("scorch.non_boss_bonus").unwrap_or(0.0);
        let mult = self.multiplier(&["scorch".to_string()], Some("solar"), None);
        let mut ticks = Vec::new();
        for (i, e) in self.enemies.iter_mut().enumerate() {
            let Some(h) = e.states.get("scorch") else {
                continue;
            };
            if !e.alive || self.t < e.next_scorch_tick {
                continue;
            }
            e.next_scorch_tick = self.t + interval;
            ticks.push((i, (base + per * h.stacks) * bonus * mult));
        }
        for (i, dmg) in ticks {
            let blow = Blow {
                amount: dmg,
                source: "verb".into(),
                element: Some("solar".into()),
                by: Some("scorch".into()),
                reach: Reach::Area,
                hit: false,
                powered: false,
                generation: 0,
            };
            let kids = self.strike(i, &blow);
            self.drain(kids);
        }
    }

    fn regenerate(&mut self) {
        let regen_mode = self.c.vars.engine("economy.regen_relative_to_current", 0.0);
        let t = self.t;
        self.regen.retain(|_, until| *until > t);
        for a in ABILITIES {
            let slot = slot(a);
            let Some(name) = self.c.build.equipped(slot).map(str::to_string) else {
                continue;
            };
            let stat_name = stat_of(slot);
            let stat = self.s.stat(stat_name) + self.stat_bonus(stat_name);
            let base = self.e.charges_per_second(slot, &name, stat).unwrap_or(0.0);
            let zero = self.e.charges_per_second(slot, &name, 0.0).unwrap_or(0.0);
            let mut bonus: f64 = self
                .regen
                .keys()
                .filter(|(ab, _)| ab == a)
                .map(|(_, level)| f64::from(*level) / 1000.0)
                .sum();
            bonus += self.passive_regen(a);
            let unit = if regen_mode >= 0.5 { base } else { zero };
            let rate = base + bonus * self.e.stat_scale(stat_name, stat) * unit;
            let max = self.param(&format!("{a}.charges")).unwrap_or(1.0).max(1.0);
            let en = self.energy.entry(a).or_default();
            *en = (*en + rate * DT).min(max);
        }
    }

    fn passive_regen(&mut self, ability: &str) -> f64 {
        let path = format!("{ability}.regen");
        let mut b = if self.c.modifies(&path) {
            self.param(&path).unwrap_or(0.0)
        } else {
            0.0
        };
        let mods: Vec<_> = self
            .c
            .modifiers
            .iter()
            .copied()
            .filter(|m| m.param.as_deref() == Some(path.as_str()))
            .collect();
        for m in mods {
            if self.holds_all(&m.when, None) {
                b += m.add.as_ref().map_or(0.0, |a| self.amount(a, None));
            }
        }
        b
    }

    fn stat_bonus(&mut self, stat: &str) -> f64 {
        let mods: Vec<_> = self
            .c
            .modifiers
            .iter()
            .copied()
            .filter(|m| m.stat.as_deref() == Some(stat))
            .collect();
        let mut total = 0.0;
        for m in mods {
            if self.holds_all(&m.when, None) {
                total += m.add.as_ref().map_or(0.0, |a| self.amount(a, None));
            }
        }
        total
    }

    /// Delayed events now due.
    fn due(&mut self) {
        let t = self.t;
        let (now, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.delayed)
            .into_iter()
            .partition(|(at, _)| *at <= t);
        self.delayed = later;
        for (_, ev) in now {
            if ev.target.is_some_and(|i| !self.enemies[i].alive) {
                continue;
            }
            self.process(ev);
        }
    }

    fn fire_weapons(&mut self, damage: f64) {
        if self
            .weapon_target
            .is_none_or(|i| !self.enemies.get(i).is_some_and(|e| e.alive))
        {
            self.weapon_target = self.random_alive();
        }
        let Some(i) = self.weapon_target else {
            return;
        };
        let blow = Blow {
            amount: damage,
            source: "weapon".into(),
            element: None,
            by: None,
            reach: Reach::Weapon,
            hit: false,
            powered: false,
            generation: 0,
        };
        let kids = self.strike(i, &blow);
        self.drain(kids);
    }

    fn cast(&mut self) {
        let interval = self.c.vars.engine("policy.min_cast_interval", 1.5);
        for a in ABILITIES {
            if self.c.build.equipped(slot(a)).is_none() {
                continue;
            }
            if self.energy.get(a).copied().unwrap_or(0.0) < 1.0 {
                continue;
            }
            if self.t - self.last_cast.get(a).copied().unwrap_or(f64::NEG_INFINITY) < interval {
                continue;
            }
            let Some(target) = self.random_alive() else {
                return;
            };
            *self.energy.entry(a).or_default() -= 1.0;
            self.last_cast.insert(a, self.t);
            *self.out.casts_per_min.entry(a.to_string()).or_default() += 1.0;
            if a == "super" {
                self.fired.clear();
            }
            let pos = self.enemies[target].pos;
            self.process(Ev::new("cast", Some(target), pos, 0).with("ability", a));
        }
    }

    // ---------------------------------------------------------- events

    fn drain(&mut self, mut stack: Vec<Ev>) {
        let mut n = 0;
        while let Some(ev) = stack.pop() {
            n += 1;
            if n > MAX_EVENTS {
                break;
            }
            let kids = self.children(&ev);
            stack.extend(kids);
        }
    }

    fn process(&mut self, ev: Ev) {
        self.drain(vec![ev]);
    }

    /// An event's consequences: its core verb's, then its rules'.
    fn children(&mut self, ev: &Ev) -> Vec<Ev> {
        if !ev.member {
            *self
                .generations
                .entry((ev.name.clone(), ev.generation))
                .or_default() += 1.0;
        }
        if !matches!(ev.name.as_str(), "hit" | "apply" | "gain" | "cast" | "kill") {
            *self.out.events_per_min.entry(ev.name.clone()).or_default() += 1.0;
        }
        let mut out = self.verb(ev);
        self.fire(ev, &mut out);
        out
    }

    fn verb(&mut self, ev: &Ev) -> Vec<Ev> {
        let mut out = Vec::new();
        let g = ev.generation;
        match ev.name.as_str() {
            "ignition" => {
                let radius = ev
                    .radius
                    .or_else(|| self.param("ignition.radius"))
                    .unwrap_or(8.0);
                let base = ev
                    .damage
                    .or_else(|| self.param("ignition.damage"))
                    .unwrap_or(0.0);
                let mut tags = ev.tags.clone();
                if !tags.iter().any(|t| t == "ignition") {
                    tags.push("ignition".into());
                }
                for i in self.within(ev.pos, radius) {
                    let mult = self.multiplier(&tags, Some("solar"), Some(i));
                    let blow = verb_blow(base * mult, "solar", "ignition", g);
                    let mut hit = Ev::new("ignition_hit", Some(i), self.enemies[i].pos, g).with(
                        "origin",
                        if Some(i) == ev.target {
                            "true"
                        } else {
                            "false"
                        },
                    );
                    if let Some(f) = ev.attrs.get("fulmination") {
                        hit = hit.with("fulmination", f);
                    }
                    out.push(hit);
                    out.extend(self.strike(i, &blow));
                }
            }
            "freeze" => {
                if let Some(i) = ev.target.filter(|i| self.enemies[*i].alive) {
                    let d = self.param("frozen.duration").unwrap_or(6.0);
                    self.enemies[i].states.insert(
                        "frozen".into(),
                        Held {
                            stacks: 1.0,
                            until: self.t + d,
                            last: self.t,
                        },
                    );
                    if self.rng.chance(self.var("policy.shatter_frozen")) {
                        let at = self.t + 0.5;
                        self.delayed.push((
                            at,
                            Ev::new("shatter", Some(i), ev.pos, g).with("of", "enemy"),
                        ));
                    }
                }
            }
            "shatter" => {
                let of_enemy = ev.attrs.get("of").map(String::as_str) == Some("enemy");
                let tags = vec!["shatter".to_string(), "crystal".to_string()];
                if of_enemy {
                    let Some(i) = ev.target else { return out };
                    if !self.enemies[i].alive || self.enemies[i].states.remove("frozen").is_none() {
                        return out;
                    }
                    let r = self.param("shatter.radius").unwrap_or(5.0);
                    let dmg = self.param("shatter.damage").unwrap_or(0.0);
                    let pos = self.enemies[i].pos;
                    let rank = self.enemies[i].rank;
                    for j in self.within(pos, r) {
                        let mult = self.multiplier(&tags, Some("stasis"), Some(j));
                        out.extend(self.strike(j, &verb_blow(dmg * mult, "stasis", "shatter", g)));
                    }
                    let size = if rank == Rank::Elite {
                        "large"
                    } else {
                        "small"
                    };
                    out.extend(self.pickup("stasis_shard", size, pos, g));
                } else {
                    let r = self.param("stasis_crystal.shatter_radius").unwrap_or(8.0);
                    let dmg = self.param("stasis_crystal.shatter_damage").unwrap_or(0.0);
                    for j in self.within(ev.pos, r) {
                        let d = dist(self.enemies[j].pos, ev.pos);
                        let falloff = 1.0 - 0.55 * (d / r).min(1.0);
                        let mult = self.multiplier(&tags, Some("stasis"), Some(j));
                        out.extend(
                            self.strike(
                                j,
                                &verb_blow(dmg * mult * falloff, "stasis", "shatter", g),
                            ),
                        );
                    }
                    out.extend(self.pickup("stasis_shard", "small", ev.pos, g));
                }
            }
            "crystal_created" => {
                // In the pack, or somewhere it freezes no one.
                if self.rng.chance(self.var("policy.crystals_in_pack")) {
                    let r = self.param("stasis_crystal.freeze_radius").unwrap_or(2.6);
                    for i in self.within(ev.pos, r) {
                        out.push(Ev::new("freeze", Some(i), self.enemies[i].pos, g));
                    }
                }
                let at = self.t + 1.0;
                self.delayed.push((
                    at,
                    Ev::new("shatter", None, ev.pos, g).with("of", "crystal"),
                ));
            }
            "jolt_chain" => {
                let r = self.param("jolt.chain_radius").unwrap_or(12.0);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = self.var("jolt.chain_targets").round().max(0.0) as usize;
                let dmg = self.param("jolt.chain_damage").unwrap_or(0.0);
                for j in self.nearest(ev.pos, r, ev.target, n) {
                    let mult = self.multiplier(&["jolt".to_string()], Some("arc"), Some(j));
                    out.extend(self.strike(j, &verb_blow(dmg * mult, "arc", "jolt_chain", g)));
                }
            }
            "volatile_explosion" => {
                let r = self.param("volatile.radius").unwrap_or(5.0);
                let dmg = self.param("volatile.damage").unwrap_or(0.0);
                for j in self.within(ev.pos, r) {
                    let mult = self.multiplier(&["volatile".to_string()], Some("void"), Some(j));
                    out.extend(self.strike(j, &verb_blow(dmg * mult, "void", "volatile", g)));
                }
            }
            "unravel_thread" => {
                let r = self.param("unravel.radius").unwrap_or(10.0);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = self.param("unravel.threads").unwrap_or(3.0).round() as usize;
                let dmg = self.param("unravel.thread_damage").unwrap_or(0.0);
                for j in self.nearest(ev.pos, r, ev.target, n) {
                    let mult = self.multiplier(&["unravel".to_string()], Some("strand"), Some(j));
                    out.extend(
                        self.strike(j, &verb_blow(dmg * mult, "strand", "unravel_thread", g)),
                    );
                }
            }
            "bolt_discharge" => {
                let r = self.var("bolt_charge.discharge_radius");
                let dmg = self.param("bolt_charge.discharge_damage").unwrap_or(0.0);
                let pos = ev
                    .target
                    .map(|i| self.enemies[i].pos)
                    .or_else(|| self.random_alive().map(|i| self.enemies[i].pos))
                    .unwrap_or(ev.pos);
                for j in self.within(pos, r) {
                    let mult = self.multiplier(&["bolt".to_string()], Some("arc"), Some(j));
                    out.extend(self.strike(j, &verb_blow(dmg * mult, "arc", "bolt", g)));
                }
            }
            "threadling_hit" => {
                let dmg = self.param("threadling.damage").unwrap_or(0.0);
                let r = self.param("threadling.seek_radius").unwrap_or(20.0);
                if let Some(&j) = self.nearest(ev.pos, r, None, 1).first() {
                    let mult =
                        self.multiplier(&["threadling".to_string()], Some("strand"), Some(j));
                    out.extend(self.strike(j, &verb_blow(dmg * mult, "strand", "threadling", g)));
                }
            }
            "tangle_detonated" => {
                let r = self.param("tangle.radius").unwrap_or(8.0);
                let dmg = self.param("tangle.damage").unwrap_or(0.0);
                if let Some(c) = self.random_alive() {
                    let pos = self.enemies[c].pos;
                    for j in self.within(pos, r) {
                        let mult =
                            self.multiplier(&["tangle".to_string()], Some("strand"), Some(j));
                        out.extend(self.strike(j, &verb_blow(dmg * mult, "strand", "tangle", g)));
                    }
                }
            }
            "pickup" => {
                let object = ev.attrs.get("object").cloned().unwrap_or_default();
                let gains: Vec<(String, f64)> = self
                    .o
                    .states
                    .get(&object)
                    .map(|st| {
                        st.params
                            .iter()
                            .filter_map(|p| {
                                let ability = p.name.strip_suffix("_energy")?;
                                Some((ability.to_string(), format!("{object}.{}", p.name)))
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|(a, path)| self.param(&path).map(|v| (a, v)))
                    .collect();
                for (ability, amount) in gains {
                    self.gain_energy(&ability, amount, Scaling::Chunk);
                }
                if object == "stasis_shard" {
                    let size = ev.attrs.get("size").map_or("small", String::as_str);
                    let amount = self
                        .param(&format!("stasis_shard.melee_energy_{size}"))
                        .unwrap_or(0.0);
                    self.gain_energy("melee", amount, Scaling::Chunk);
                }
            }
            _ => {}
        }
        out
    }

    /// Deal damage to an enemy: the verbs its states set off, the hit, and
    /// the kill if it dies.
    fn strike(&mut self, i: usize, b: &Blow) -> Vec<Ev> {
        let mut out = Vec::new();
        if !self.enemies[i].alive || b.amount <= 0.0 {
            return out;
        }
        let landed = match b.reach {
            Reach::Weapon => b.amount,
            reach => {
                let eff = if reach == Reach::Direct {
                    self.var("scenario.direct_hit_efficiency")
                } else {
                    self.var("scenario.ability_efficiency")
                };
                let sigma = self.s.damage_spread;
                self.out.damage_per_s += b.amount;
                b.amount * eff * (sigma * self.rng.normal()).exp()
            }
        };
        let pos = self.enemies[i].pos;
        let g = b.generation;
        let t = self.t;
        let by = b.by.as_deref().unwrap_or("");
        // A Jolted, Volatile or Unraveled enemy damaged again.
        if by != "jolt_chain" && self.enemies[i].states.contains_key("jolt") {
            let cd = self.param("jolt.chain_cooldown").unwrap_or(0.8);
            if t - self.enemies[i].last_chain >= cd {
                self.enemies[i].last_chain = t;
                out.push(Ev::new("jolt_chain", Some(i), pos, g));
            }
        }
        if by != "volatile" && self.enemies[i].states.remove("volatile").is_some() {
            out.push(Ev::new("volatile_explosion", Some(i), pos, g));
        }
        if by != "unravel_thread" && self.enemies[i].states.contains_key("unravel") {
            let cd = self.param("unravel.cooldown").unwrap_or(1.2);
            if t - self.enemies[i].last_thread >= cd {
                self.enemies[i].last_thread = t;
                out.push(Ev::new("unravel_thread", Some(i), pos, g));
            }
        }
        if b.hit {
            let mut hit = Ev::new("hit", Some(i), pos, g).with("source", &b.source);
            if let Some(el) = &b.element {
                hit = hit.with("element", el);
            }
            if b.powered {
                hit = hit.with("powered", "true");
            }
            out.push(hit);
        }
        self.enemies[i].health -= landed;
        if self.enemies[i].health <= 0.0 {
            self.enemies[i].alive = false;
            let frozen = self.enemies[i].states.contains_key("frozen");
            let mut kill = Ev::new("kill", Some(i), pos, g).with("source", &b.source);
            if let Some(el) = &b.element {
                kill = kill.with("element", el);
            }
            if let Some(v) = &b.by {
                kill = kill.with("by", v);
            }
            if b.source == "weapon" {
                self.out.weapon_kills_per_min += 1.0;
            } else {
                self.out.ability_kills_per_min += 1.0;
            }
            let key = match &b.by {
                Some(v) => format!("verb:{v}"),
                None => b.source.clone(),
            };
            *self.out.kills_by.entry(key).or_default() += 1.0;
            // A frozen enemy's death shatters it: a shard.
            if frozen {
                let size = if self.enemies[i].rank == Rank::Elite {
                    "large"
                } else {
                    "small"
                };
                out.extend(self.pickup("stasis_shard", size, pos, g));
            }
            out.push(kill);
        }
        out
    }

    fn pickup(&mut self, object: &str, size: &str, pos: (f64, f64), g: u32) -> Vec<Ev> {
        let chance = if object == "stasis_shard" {
            if self.c.modifies("stasis_shard.pickup_radius") {
                1.0
            } else {
                self.var("policy.shard_pickup")
            }
        } else if self.c.modifies(&format!("{object}.pickup_radius")) {
            1.0
        } else {
            self.var("policy.object_pickup")
        };
        if self.rng.chance(chance) {
            vec![
                Ev::new("pickup", None, pos, g)
                    .with("object", object)
                    .with("size", size),
            ]
        } else {
            Vec::new()
        }
    }

    fn gain_energy(&mut self, ability: &str, amount: f64, scaling: Scaling) {
        let sl = slot(ability);
        let Some(name) = self.c.build.equipped(sl).map(str::to_string) else {
            return;
        };
        let stat = self.s.stat(stat_of(sl)) + self.stat_bonus(stat_of(sl));
        let documented = if scaling == Scaling::Fixed {
            1.0
        } else {
            self.c.vars.engine("economy.documented_chunk_scale", 1.0)
        };
        let charges = self.e.chunk(sl, &name, stat, amount * documented, scaling);
        let key = ABILITIES.iter().find(|a| **a == ability).copied();
        if let Some(a) = key {
            let max = self.param(&format!("{a}.charges")).unwrap_or(1.0).max(1.0);
            let en = self.energy.entry(a).or_default();
            *en = (*en + charges).min(max);
        }
    }

    // ----------------------------------------------------------- rules

    fn fire(&mut self, ev: &Ev, out: &mut Vec<Ev>) {
        let rules: Vec<(usize, &'a crate::rules::Rule)> = self
            .c
            .rules
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, r)| r.on.event == ev.name)
            .collect();
        for (k, r) in rules {
            if !self.trigger(&r.on, ev) || !self.holds_all(&r.when, Some(ev)) {
                continue;
            }
            if let Some(ch) = &r.chance {
                let p = self.amount(ch, Some(ev));
                if !self.rng.chance(p) {
                    continue;
                }
            }
            if let Some(cd) = &r.cooldown {
                if self.t < self.ready_at.get(&k).copied().unwrap_or(f64::NEG_INFINITY) {
                    continue;
                }
                let d = self.amount(cd, Some(ev));
                self.ready_at.insert(k, self.t + d);
            }
            if let Some(limit) = r.limit {
                let n = self.fired.entry(k).or_default();
                if *n >= limit {
                    continue;
                }
                *n += 1;
            }
            for eff in &r.effect {
                self.effect(eff, ev, out);
            }
        }
    }

    fn trigger(&mut self, t: &Trigger, ev: &Ev) -> bool {
        for (k, v) in &t.attributes {
            let want = value_text(v);
            let ok = match ev.attrs.get(k) {
                Some(have) => attr_eq(k, &want, have),
                None => match k.as_str() {
                    "target_has" => self.target_has(ev, &want),
                    "rank" => ev
                        .target
                        .is_some_and(|i| self.enemies[i].rank.name() == want),
                    "precision" => {
                        let p = self.var("policy.precision_share");
                        self.rng.chance(p) == (want == "true")
                    }
                    "element" => {
                        let p = self.var("policy.weapon_element_match");
                        self.rng.chance(p)
                    }
                    _ => want == "false",
                },
            };
            if !ok {
                return false;
            }
        }
        true
    }

    fn target_has(&self, ev: &Ev, state: &str) -> bool {
        ev.target.is_some_and(|i| {
            self.enemies[i]
                .states
                .get(state)
                .is_some_and(|h| h.stacks > 0.0)
        })
    }

    fn holds_all(&mut self, cs: &[Condition], ev: Option<&Ev>) -> bool {
        cs.iter().all(|c| self.holds(c, ev))
    }

    fn holds(&mut self, c: &Condition, ev: Option<&Ev>) -> bool {
        if let Some(b) = &c.has {
            return self.buffs.get(b).is_some_and(|h| h.stacks > 0.0);
        }
        if let Some(b) = &c.lacks {
            return !self.buffs.get(b).is_some_and(|h| h.stacks > 0.0);
        }
        if let Some(name) = &c.stacks {
            let min = c.min.unwrap_or(1.0);
            let have = self
                .counters
                .get(name)
                .copied()
                .or_else(|| self.buffs.get(name).map(|h| h.stacks))
                .unwrap_or(0.0);
            return have + 1e-9 >= min;
        }
        if let Some(m) = &c.movement {
            let policy = match m.as_str() {
                "slide" => "policy.slide_form",
                "airborne" | "glide" => "policy.airborne",
                "sprint" => "policy.sprint",
                _ => "policy.other_movement",
            };
            let p = self.var(policy);
            return self.rng.chance(p);
        }
        if let Some(v) = c.in_super {
            return !v;
        }
        if c.health_below.is_some() {
            let p = self.var("policy.critical_health");
            return self.rng.chance(p);
        }
        if let Some(r) = c.enemies_within {
            #[allow(clippy::cast_precision_loss)]
            let n = self.within((0.0, 0.0), r).len() as f64;
            return n >= c.min.unwrap_or(1.0);
        }
        if let Some(st) = &c.target_has {
            return ev.is_some_and(|e| self.target_has(e, st));
        }
        let rank = ev
            .and_then(|e| e.target)
            .map(|i| self.enemies[i].rank.name());
        if let Some(ranks) = &c.target_rank {
            return rank.is_some_and(|r| ranks.iter().any(|x| x == r));
        }
        if let Some(ranks) = &c.target_rank_not {
            return !rank.is_some_and(|r| ranks.iter().any(|x| x == r));
        }
        true
    }

    fn amount(&mut self, a: &Amount, ev: Option<&Ev>) -> f64 {
        match a {
            Amount::Number(x) => *x,
            Amount::Table(t) => {
                if let Some(v) = &t.var {
                    return self.var(v) * t.scale.unwrap_or(1.0);
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
                let rank = ev
                    .and_then(|e| e.target)
                    .map_or(Rank::Minor, |i| self.enemies[i].rank);
                if let Some(by) = &t.by_rank {
                    return by.get(rank.name()).copied().unwrap_or(0.0);
                }
                if let Some(by) = &t.by_tier {
                    let i = usize::from(rank == Rank::Elite).min(by.len().saturating_sub(1));
                    return by.get(i).copied().unwrap_or(0.0);
                }
                if let Some(by) = &t.by_count {
                    return by.first().copied().unwrap_or(0.0);
                }
                0.0
            }
        }
    }

    /// The damage multiplier for a hit on enemy `target`: each conditional
    /// modifier whole if its conditions hold, else not at all.
    fn multiplier(&mut self, tags: &[String], element: Option<&str>, target: Option<usize>) -> f64 {
        let ev = target.map(|i| Ev::new("hit", Some(i), self.enemies[i].pos, 0));
        let mods: Vec<_> = self.c.modifiers.clone();
        let p: Vec<f64> = mods
            .iter()
            .map(|m| f64::from(u8::from(self.holds_all(&m.when, ev.as_ref()))))
            .collect();
        self.c.damage_multiplier(self.o, tags, element, &p)
    }

    /// The enemies an effect lands on, or `None` for the player.
    fn targets(&mut self, eff: &Effect, ev: &Ev) -> Option<Vec<usize>> {
        match eff.target.as_deref() {
            Some("self" | "allies" | "fireteam") => None,
            Some("area") => {
                let r = eff
                    .radius
                    .as_ref()
                    .map_or(0.0, |a| self.amount(a, Some(ev)));
                let mut v = self.within(ev.pos, r);
                if let Some(i) = ev.target
                    && self.enemies[i].alive
                    && !v.contains(&i)
                {
                    v.push(i);
                }
                if eff.except.as_deref() == Some("origin") {
                    v.retain(|i| Some(*i) != ev.target);
                }
                Some(v)
            }
            _ => {
                // A dead target takes nothing, but repeated hits (an Arc
                // Soul's rounds) move on to the nearest enemy.
                let t = match ev.target {
                    Some(i) if self.enemies[i].alive => Some(i),
                    Some(_) if eff.times.is_none() => None,
                    _ => self.nearest(ev.pos, 15.0, None, 1).first().copied(),
                };
                Some(t.into_iter().collect())
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn effect(&mut self, eff: &Effect, ev: &Ev, out: &mut Vec<Ev>) {
        let times = eff.times.as_ref().map_or(1.0, |a| self.amount(a, Some(ev)));
        let n = self.rng.round(times);
        let g = ev.generation;
        for _ in 0..n {
            if let Some(state) = &eff.extend
                && let Some(h) = self.buffs.get_mut(state)
            {
                let d = eff.duration.as_ref().map_or(0.0, |a| match a {
                    Amount::Number(x) => *x,
                    Amount::Table(_) => 0.0,
                });
                h.until += d;
            }
            if let Some(state) = &eff.apply {
                self.apply(state, eff, ev, out);
            }
            if let Some(a) = &eff.damage
                && eff.emit.is_none()
            {
                let base = self.amount(a, Some(ev));
                let single = !matches!(eff.target.as_deref(), Some("area"));
                let source = eff.source.clone().unwrap_or_else(|| "ability".into());
                let powered = eff.tags.iter().any(|t| t == "powered_melee");
                for i in self.targets(eff, ev).unwrap_or_default() {
                    let mult = self.multiplier(&eff.tags, eff.element.as_deref(), Some(i));
                    let blow = Blow {
                        amount: base * mult,
                        source: source.clone(),
                        element: eff.element.clone(),
                        by: None,
                        reach: if single { Reach::Direct } else { Reach::Area },
                        hit: true,
                        powered,
                        generation: g,
                    };
                    out.extend(self.strike(i, &blow));
                }
            }
            if let Some(ability) = &eff.energy {
                let amount = eff
                    .amount
                    .as_ref()
                    .map_or(0.0, |a| self.amount(a, Some(ev)));
                let scaling = Scaling::parse(eff.scaling.as_deref()).unwrap_or(Scaling::Chunk);
                self.gain_energy(ability, amount, scaling);
            }
            if let Some(ability) = &eff.charges {
                let count = eff.count.as_ref().map_or(1.0, |a| self.amount(a, Some(ev)));
                if let Some(a) = ABILITIES.iter().find(|a| *a == ability).copied() {
                    let max = self.param(&format!("{a}.charges")).unwrap_or(1.0).max(1.0);
                    let en = self.energy.entry(a).or_default();
                    *en = (*en + count).min(max);
                }
            }
            if let Some(ability) = &eff.regen {
                let bonus = eff
                    .amount
                    .as_ref()
                    .map_or(0.0, |a| self.amount(a, Some(ev)));
                let d = eff
                    .duration
                    .as_ref()
                    .or(eff.max_duration.as_ref())
                    .map_or(0.0, |a| self.amount(a, Some(ev)));
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let level = (bonus * 1000.0).round().max(0.0) as u32;
                let until = self.regen.entry((ability.clone(), level)).or_default();
                *until = until.max(self.t + d);
            }
            if let Some(obj) = &eff.spawn {
                let sizes: Vec<(String, f64)> = if let Some(s) = &eff.sizes {
                    s.iter().map(|(k, v)| (k.clone(), *v)).collect()
                } else {
                    let count = eff.count.as_ref().map_or(1.0, |a| self.amount(a, Some(ev)));
                    let size = eff.size.clone().unwrap_or_else(|| {
                        if obj == "orb_of_power" {
                            "large"
                        } else {
                            "small"
                        }
                        .to_string()
                    });
                    vec![(size, count)]
                };
                for (size, count) in sizes {
                    for _ in 0..self.rng.round(count) {
                        match obj.as_str() {
                            "stasis_crystal" => {
                                out.push(
                                    Ev::new("crystal_created", None, ev.pos, g).with("size", &size),
                                );
                            }
                            "threadling" => out.push(Ev::new("threadling_hit", None, ev.pos, g)),
                            "tangle" => {
                                if self.rng.chance(self.var("policy.tangle_use")) {
                                    out.push(Ev::new("tangle_detonated", None, ev.pos, g));
                                }
                            }
                            _ => out.extend(self.pickup(obj, &size, ev.pos, g)),
                        }
                    }
                }
            }
            if let Some(h) = &eff.heal {
                self.out.heal_per_s += self.amount(h, Some(ev));
            }
            if let Some(name) = &eff.emit {
                let attrs: BTreeMap<String, String> = eff
                    .attributes
                    .iter()
                    .flatten()
                    .map(|(k, v)| (k.clone(), value_text(v)))
                    .collect();
                let damage = eff.damage.as_ref().map(|a| self.amount(a, Some(ev)));
                let radius = eff.radius.as_ref().map(|a| self.amount(a, Some(ev)));
                let at: Vec<Option<usize>> = if eff.target.as_deref() == Some("area") {
                    self.targets(eff, ev)
                        .unwrap_or_default()
                        .into_iter()
                        .map(Some)
                        .collect()
                } else {
                    vec![ev.target]
                };
                for target in at {
                    let pos = target.map_or(ev.pos, |i| self.enemies[i].pos);
                    let mut child = Ev::new(name, target, pos, g);
                    child.attrs.clone_from(&attrs);
                    child.damage = damage;
                    child.radius = radius;
                    child.tags.clone_from(&eff.tags);
                    child.element.clone_from(&eff.element);
                    child.member = *name == ev.name;
                    out.push(child);
                }
            }
            if let Some(name) = &eff.consume {
                if self.counters.contains_key(name) || self.c.counters.contains_key(name) {
                    self.counters.insert(name.clone(), 0.0);
                } else {
                    self.buffs.remove(name);
                }
            }
            if let Some(name) = &eff.progress {
                let amount = eff
                    .amount
                    .as_ref()
                    .map_or(1.0, |a| self.amount(a, Some(ev)));
                let threshold = self.c.counters.get(name).copied().unwrap_or(1.0).max(1e-9);
                let v = self.counters.entry(name.clone()).or_default();
                let before = *v;
                *v = (*v + amount).min(threshold);
                if before < threshold && *v + 1e-9 >= threshold {
                    out.push(Ev::new("counter_full", ev.target, ev.pos, g).with("counter", name));
                }
            }
        }
    }

    fn apply(&mut self, state: &str, eff: &Effect, ev: &Ev, out: &mut Vec<Ev>) {
        let g = ev.generation;
        let def = self.o.states.get(state);
        let max = def
            .and_then(|d| d.max_stacks)
            .map_or(f64::INFINITY, f64::from);
        let at_max = def.and_then(|d| d.at_max.clone());
        let stacks = eff
            .stacks
            .as_ref()
            .map_or(1.0, |a| self.amount(a, Some(ev)));
        let duration = self.duration(state, eff, ev);
        let t = self.t;
        match self.targets(eff, ev) {
            None => {
                // On the player.
                let max = self.param(&format!("{state}.max_stacks")).unwrap_or(max);
                if def.is_some_and(|d| d.kind == "verb") {
                    // An instant verb (Cure) heals by its stacks.
                    #[allow(clippy::cast_possible_truncation)]
                    let tier = format!("{state}.heal_x{}", stacks.round().clamp(1.0, 3.0) as i64);
                    self.out.heal_per_s += self.param(&tier).unwrap_or(0.0);
                } else {
                    let h = self.buffs.entry(state.to_string()).or_insert(Held {
                        stacks: 0.0,
                        until: t,
                        last: t,
                    });
                    h.stacks = (h.stacks + stacks).min(max);
                    h.until = h.until.max(t + duration);
                    h.last = t;
                    if let Some(next) = at_max
                        && h.stacks + 1e-9 >= max
                        && max.is_finite()
                    {
                        h.stacks = 0.0;
                        out.push(Ev::new(&next, ev.target, ev.pos, g));
                    }
                }
                out.push(Ev::new("gain", None, ev.pos, g).with("state", state));
            }
            Some(targets) => {
                let applied = self.c.applied_stacks(self.o, state, stacks);
                for i in targets {
                    let pos = self.enemies[i].pos;
                    if state == "frozen" {
                        if !self.enemies[i].states.contains_key("frozen") {
                            out.push(Ev::new("freeze", Some(i), pos, g));
                        }
                        continue;
                    }
                    let h = self.enemies[i]
                        .states
                        .entry(state.to_string())
                        .or_insert(Held {
                            stacks: 0.0,
                            until: t,
                            last: t,
                        });
                    h.stacks = (h.stacks + applied).min(max);
                    h.until = h.until.max(t + duration.max(DT));
                    h.last = t;
                    if let Some(next) = &at_max
                        && max.is_finite()
                        && h.stacks + 1e-9 >= max
                    {
                        h.stacks = 0.0;
                        // It goes off a moment later, if the enemy lives.
                        self.delayed
                            .push((t + 0.1, Ev::new(next, Some(i), pos, g + 1)));
                    }
                    out.push(Ev::new("apply", Some(i), pos, g).with("state", state));
                }
            }
        }
    }

    fn duration(&mut self, state: &str, eff: &Effect, ev: &Ev) -> f64 {
        let scale = self
            .param(&format!("{state}.duration_multiplier"))
            .unwrap_or(1.0);
        if let Some(a) = eff.duration.as_ref().or(eff.max_duration.as_ref()) {
            return scale * self.amount(a, Some(ev));
        }
        for p in ["duration", "stack_duration"] {
            if let Some(d) = self.param(&format!("{state}.{p}")) {
                return scale * d;
            }
        }
        // Jolt, Volatile and the like last a while.
        10.0
    }
}

fn verb_blow(amount: f64, element: &str, by: &str, generation: u32) -> Blow {
    Blow {
        amount,
        source: "verb".into(),
        element: Some(element.into()),
        by: Some(by.into()),
        reach: Reach::Area,
        hit: false,
        powered: false,
        generation,
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

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::tests::economy;
    use crate::scenario::tests::scenario;

    fn pinned() -> Variables {
        let mut v = Variables::builtin().unwrap();
        for (name, x) in [
            ("scenario.pack_size", 8.0),
            ("scenario.pack_radius", 10.0),
            ("scenario.elite_share", 0.25),
            ("build.default_stat", 70.0),
            ("scenario.ability_efficiency", 1.0),
            ("scenario.direct_hit_efficiency", 1.0),
            ("economy.documented_chunk_scale", 1.0),
        ] {
            v.set(name, x).unwrap();
        }
        v
    }

    #[test]
    fn the_ignition_chain_branches_with_fulmination_where_neighbours_survive() {
        let o = Ontology::builtin().unwrap();
        let base = crate::builtin_rules().unwrap();
        let vars = pinned();
        let e = economy();
        let solar = [
            "fragment:Ember of Char",
            "fragment:Ember of Ashes",
            "armor:Dawn Chorus",
        ];
        let run = |extra: &[&str], durability: f64| {
            let parts: Vec<&str> = solar.iter().chain(extra).copied().collect();
            let build = Build::parse(parts.iter().copied()).unwrap();
            let c = compile(&o, &base, &build, &vars);
            let mut s = scenario(durability);
            s.pack = crate::scenario::Pack {
                size: 8.0,
                radius_m: 10.0,
            };
            offspring(&o, &c, &e, &s, "ignition", 400, 7)
        };
        let with = run(&["artifact_perk:Solar Fulmination"], 6.97);
        let without = run(&[], 6.97);
        let easy = run(&["artifact_perk:Solar Fulmination"], 1.0);
        assert!(with > 1.0, "with Fulmination, hard: {with}");
        assert!(without < 0.2, "without: {without}");
        assert!(easy < with, "easy {easy} against hard {with}");
    }

    #[test]
    fn weapons_alone_kill_about_the_supply() {
        let o = Ontology::builtin().unwrap();
        let base = crate::builtin_rules().unwrap();
        let vars = pinned();
        let e = economy();
        let mut s = scenario(3.47);
        s.supply_per_min = Some(9.0);
        let build = Build::parse(std::iter::empty::<&str>()).unwrap();
        let out = simulate(
            &o,
            &base,
            &build,
            &vars,
            &e,
            &s,
            Config {
                seconds: 600.0,
                runs: 4,
                seed: 3,
            },
        );
        assert!(out.ability_kills_per_min.abs() < 1e-9);
        assert!(
            (out.weapon_kills_per_min - 9.0).abs() < 1.0,
            "{}",
            out.weapon_kills_per_min
        );
    }

    #[test]
    fn runs_are_reproducible() {
        let o = Ontology::builtin().unwrap();
        let base = crate::builtin_rules().unwrap();
        let vars = pinned();
        let e = economy();
        let s = scenario(3.47);
        let build = Build::parse(["melee:Shiver Strike", "aspect:Howl of the Storm"]).unwrap();
        let cfg = Config {
            seconds: 120.0,
            runs: 3,
            seed: 11,
        };
        let a = simulate(&o, &base, &build, &vars, &e, &s, cfg);
        let b = simulate(&o, &base, &build, &vars, &e, &s, cfg);
        assert_eq!(a, b);
        assert!(a.casts_per_min.get("melee").copied().unwrap_or(0.0) > 0.0);
    }
}
