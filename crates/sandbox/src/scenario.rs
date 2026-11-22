//! Scenarios: the contexts builds are evaluated in, as environments
//! (`docs/build-model.md` §4).
//!
//! A scenario is a pack of enemies, a mix of ranks with their health, the
//! context's durability (the damage to kill over health, from its Power
//! delta: `docs/durability.md`), and the player's stats.
//!
//! Positions are not in the data. Packs are modelled as enemies spread
//! uniformly over a disc, which gives the expected number of neighbours
//! within any radius: what every area effect and chain depends on. Pack
//! size and spread start from priors (here, not from data) and are to be
//! calibrated on the recorded clears.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use focalpoint_db::Database;

use crate::variables::Variables;

/// A combatant rank, as the scenario models it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    Minor,
    Elite,
}

impl Rank {
    pub const ALL: [Rank; 2] = [Rank::Minor, Rank::Elite];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Rank::Minor => "minor",
            Rank::Elite => "elite",
        }
    }
}

/// Enemies spread uniformly over a disc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pack {
    pub size: f64,
    pub radius_m: f64,
}

/// P(d ≤ r) for the distance d between two points uniform in a disc of
/// radius `radius`.
#[must_use]
pub fn disc_distance_cdf(r: f64, radius: f64) -> f64 {
    if r <= 0.0 || radius <= 0.0 {
        return 0.0;
    }
    if r >= 2.0 * radius {
        return 1.0;
    }
    let x = r / (2.0 * radius);
    let q = r * r / (radius * radius);
    let f = 1.0
        + 2.0 / std::f64::consts::PI
            * ((q - 1.0) * x.acos() - x * (1.0 + q / 2.0) * (1.0 - x * x).sqrt());
    f.clamp(0.0, 1.0)
}

/// An environment to evaluate a build in.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub name: String,
    pub pack: Pack,
    pub elite_share: f64,
    /// Health (before durability) of the archetypes of each rank: the
    /// scenario's enemies are drawn from them evenly.
    pub minor_health: Vec<f64>,
    pub elite_health: Vec<f64>,
    /// Damage to kill over health.
    pub durability: f64,
    /// Stat values by stat: `melee`, `grenade`, `class`, `super`, ...
    pub stats: BTreeMap<String, f64>,
    /// A stat's value where `stats` gives none.
    pub default_stat: f64,
    /// Enemies a minute the player kills in this context, where the
    /// recorded clears give it; otherwise pack size times
    /// `scenario.packs_per_minute`.
    pub supply_per_min: Option<f64>,
    /// The recorded share of kills by abilities, where known: what the
    /// model's should come close to.
    pub recorded_ability_kill_share: Option<f64>,
    /// What the scenario takes without data.
    pub assumptions: Vec<String>,
    /// The variables its priors came from.
    pub variables: Vec<String>,
}

impl Scenario {
    /// A scenario with the priors (`scenario.*` variables) and the given
    /// health and durability.
    #[must_use]
    pub fn new(
        name: &str,
        minor_health: Vec<f64>,
        elite_health: Vec<f64>,
        durability: f64,
        vars: &Variables,
    ) -> Self {
        Self {
            name: name.to_string(),
            pack: Pack {
                size: vars.engine("scenario.pack_size", 8.0),
                radius_m: vars.engine("scenario.pack_radius", 10.0),
            },
            elite_share: vars.engine("scenario.elite_share", 0.25),
            minor_health,
            elite_health,
            durability,
            stats: BTreeMap::new(),
            default_stat: vars.engine("build.default_stat", 100.0),
            supply_per_min: None,
            recorded_ability_kill_share: None,
            assumptions: Vec::new(),
            variables: [
                "scenario.pack_size",
                "scenario.pack_radius",
                "scenario.elite_share",
                "build.default_stat",
            ]
            .map(String::from)
            .to_vec(),
        }
    }

    /// A context's scenario: its durability and health tier from the
    /// database (`focalpoint mechanics import`), with the priors.
    pub fn from_context(db: &Database, query: &str, vars: &Variables) -> Result<Self> {
        let mut stmt = db.conn().prepare(
            "SELECT c.context_id, c.name, d.value, t.value
             FROM context c
             JOIN context_feature d ON d.context_id = c.context_id AND d.feature = 'durability'
             LEFT JOIN context_feature t ON t.context_id = c.context_id AND t.feature = 'health_tier'
             WHERE c.context_id = ?1 OR lower(c.name) = lower(?1)
                OR (lower(c.name) LIKE '%' || lower(?1) || '%')
             ORDER BY c.context_id = ?1 DESC, lower(c.name) = lower(?1) DESC, c.kind = 'group' DESC, c.name",
        )?;
        let found: Vec<(String, String, f64, Option<f64>)> = stmt
            .query_map([query], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<duckdb::Result<_>>()?;
        let Some((id, name, durability, tier)) = found.into_iter().next() else {
            bail!(
                "no context with a durability matches {query:?} (see `focalpoint mechanics context`)"
            );
        };
        #[allow(clippy::cast_sign_loss)]
        let tier = tier.unwrap_or(2.0).round() as u8;
        let health = |rank: &str| -> Result<Vec<f64>> {
            let mut stmt = db.conn().prepare(
                "SELECT health FROM enemy_health WHERE health_tier = ?1 AND rank = ?2 ORDER BY health",
            )?;
            let v: Vec<f64> = stmt
                .query_map(duckdb::params![tier, rank], |r| r.get(0))?
                .collect::<duckdb::Result<_>>()
                .with_context(|| format!("reading {rank} health at tier {tier}"))?;
            Ok(v)
        };
        let mut s = Self::new(
            &format!("{name} ({id})"),
            health("minor")?,
            health("elite")?,
            durability,
            vars,
        );
        s.assumptions.push(format!(
            "health tier {tier}, every archetype of a rank equally likely"
        ));
        // The recorded clears' kill rate and kill mix (context profiles).
        let feature = |name: &str| -> Result<Option<f64>> {
            let mut stmt = db.conn().prepare(
                "SELECT value FROM context_feature WHERE context_id = ?1 AND feature = ?2",
            )?;
            let v: Option<Option<f64>> = stmt
                .query_map(duckdb::params![id, name], |r| r.get(0))?
                .next()
                .transpose()?;
            Ok(v.flatten())
        };
        if let Some(kpm) = feature("kills_per_min")? {
            s.supply_per_min = Some(kpm);
            s.assumptions.push(format!(
                "{kpm:.1} kills a minute, as the recorded clears make"
            ));
        }
        s.recorded_ability_kill_share = feature("ability_kill_share")?;
        Ok(s)
    }

    /// Durability 1 (a Power delta of 0) with health tier 2: the scenario
    /// without a context.
    pub fn baseline(db: &Database, vars: &Variables) -> Result<Self> {
        let mut stmt = db.conn().prepare(
            "SELECT rank, health FROM enemy_health WHERE health_tier = 2 ORDER BY health",
        )?;
        let rows: Vec<(String, f64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<duckdb::Result<_>>()?;
        let of = |rank: &str| -> Vec<f64> {
            rows.iter()
                .filter(|(r, _)| r == rank)
                .map(|(_, h)| *h)
                .collect()
        };
        let mut s = Self::new(
            "durability 1, health tier 2",
            of("minor"),
            of("elite"),
            1.0,
            vars,
        );
        s.assumptions
            .push("no context: durability 1 (a Power delta of 0)".into());
        Ok(s)
    }

    /// A stat's value, or the default.
    #[must_use]
    pub fn stat(&self, stat: &str) -> f64 {
        self.stats.get(stat).copied().unwrap_or(self.default_stat)
    }

    /// The share of a pack of a rank.
    #[must_use]
    pub fn share(&self, rank: Rank) -> f64 {
        match rank {
            Rank::Minor => 1.0 - self.elite_share,
            Rank::Elite => self.elite_share,
        }
    }

    /// The expected number of other pack members within `r` metres of one.
    #[must_use]
    pub fn neighbours(&self, r: f64) -> f64 {
        (self.pack.size - 1.0).max(0.0) * disc_distance_cdf(r, self.pack.radius_m)
    }

    /// The expected number of pack members within `r` metres of a point in
    /// the pack (a crystal, an explosion that is not on an enemy).
    #[must_use]
    pub fn around(&self, r: f64) -> f64 {
        self.pack.size * disc_distance_cdf(r, self.pack.radius_m)
    }

    /// The share of enemies of a rank that survive `damage` (before
    /// durability: the damage the sources document).
    #[must_use]
    pub fn survives(&self, rank: Rank, damage: f64) -> f64 {
        let health = match rank {
            Rank::Minor => &self.minor_health,
            Rank::Elite => &self.elite_health,
        };
        if health.is_empty() {
            return 1.0;
        }
        #[allow(clippy::cast_precision_loss)]
        let n = health.len() as f64;
        #[allow(clippy::cast_precision_loss)]
        let alive = health
            .iter()
            .filter(|h| *h * self.durability > damage)
            .count() as f64;
        alive / n
    }

    /// The share of a pack that survives `damage`, ranks mixed.
    #[must_use]
    pub fn survives_mixed(&self, damage: f64) -> f64 {
        Rank::ALL
            .iter()
            .map(|r| self.share(*r) * self.survives(*r, damage))
            .sum()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Tier-2 health like `MossyMax`'s: minors 120–550, elites 550–2,250.
    pub(crate) fn scenario(durability: f64) -> Scenario {
        Scenario::new(
            "test",
            vec![120.0, 215.0, 323.0, 525.0],
            vec![550.0, 1470.0, 1825.0, 2250.0],
            durability,
            &Variables::builtin().unwrap(),
        )
    }

    #[test]
    fn the_disc_distance_cdf_matches_sampling() {
        // A fixed-seed linear congruential sampler: no test dependency.
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            #[allow(clippy::cast_precision_loss)]
            let u = (seed >> 11) as f64 / (1u64 << 53) as f64;
            u
        };
        let mut point = || loop {
            let (x, y) = (2.0 * next() - 1.0, 2.0 * next() - 1.0);
            if x * x + y * y <= 1.0 {
                return (x * 10.0, y * 10.0);
            }
        };
        let n = 200_000;
        let mut within = [0usize; 3];
        for _ in 0..n {
            let (a, b) = (point(), point());
            let d = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
            for (i, r) in [4.0, 8.0, 12.0].iter().enumerate() {
                within[i] += usize::from(d <= *r);
            }
        }
        for (i, r) in [4.0, 8.0, 12.0].iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let sampled = within[i] as f64 / f64::from(n);
            let exact = disc_distance_cdf(*r, 10.0);
            assert!(
                (sampled - exact).abs() < 0.005,
                "r {r}: {sampled} vs {exact}"
            );
        }
        assert!(disc_distance_cdf(0.0, 10.0).abs() < 1e-12);
        assert!((disc_distance_cdf(25.0, 10.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn survival_scales_with_durability() {
        let easy = scenario(1.0);
        let hard = scenario(6.97);
        // 1,352 damage (a Dawn Chorus ignition) kills every minor at a
        // durability of 1 and only the frailest at 6.97.
        assert!(easy.survives(Rank::Minor, 1352.0).abs() < 1e-12);
        assert!((hard.survives(Rank::Minor, 1352.0) - 0.75).abs() < 1e-12);
        assert!((hard.survives(Rank::Elite, 1352.0) - 1.0).abs() < 1e-12);
        assert!((hard.survives_mixed(1352.0) - (0.75 * 0.75 + 0.25)).abs() < 1e-12);
        assert!((easy.neighbours(8.0) - 7.0 * disc_distance_cdf(8.0, 10.0)).abs() < 1e-12);
    }
}
