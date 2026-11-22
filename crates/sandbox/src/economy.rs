//! The energy economy: ability cooldowns, how stats speed them, and what a
//! chunk of energy is worth (`docs/mechanics.md`, "Energy economy").
//!
//! From the ability cooldown workbook, imported into `ability_cooldown` and
//! `stat_curve`:
//!
//! * an ability recharges at `cooldown_scalar(stat) / cooldown_s` charges a
//!   second, where `cooldown_s` is its cooldown at 0 stat. Above 100 a stat
//!   adds damage, not cooldown, so the scalar is held at its value at 100;
//! * a chunk of energy the sources document (Whisper of Hunger's 16%) is
//!   their pre-Edge of Fate figure. It arrives as `amount × flat-gain
//!   factor × flat_energy_vs_pre_eof(stat)`: the ability's chunk scalar
//!   (Shiver Strike's 0.7) and 0.40 at 0 stat to 0.90 at 100. Gains Clarity
//!   marks as unaffected by chunk scalars skip the first, and fixed gains
//!   skip both;
//! * a documented regeneration bonus (Spark of Recharge's +400%) is scaled
//!   the same way by the stat, and is taken here as extra charges a second
//!   in units of the 0-stat rate, as the workbook states it ("relative to 0
//!   stat cooldown"). That reading is an assumption to measure in-game.

use std::collections::BTreeMap;

use anyhow::Result;
use focalpoint_db::Database;

use crate::variables::Variables;

/// The stat that drives an ability slot.
#[must_use]
pub fn stat_of(slot: &str) -> &'static str {
    match slot {
        "grenade" => "grenade",
        "melee" => "melee",
        "class_ability" | "class" => "class",
        "super" => "super",
        _ => "weapons",
    }
}

/// How a chunk of energy is scaled (`scaling` on an energy effect).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scaling {
    /// By the ability's chunk scalar and the stat: the default.
    Chunk,
    /// By the stat only ("not affected by Chunk Energy Scalars").
    Stat,
    /// Neither ("no chunk or stat scalar").
    Fixed,
}

impl Scaling {
    /// `chunk` (or none), `stat` or `fixed`.
    #[must_use]
    pub fn parse(s: Option<&str>) -> Option<Self> {
        match s {
            None | Some("chunk") => Some(Self::Chunk),
            Some("stat") => Some(Self::Stat),
            Some("fixed") => Some(Self::Fixed),
            Some(_) => None,
        }
    }
}

/// `(slot, name, cooldown_s, flat_gain_factor)`.
pub type AbilityRow<'a> = (&'a str, &'a str, f64, f64);
/// `(stat, effect, [(stat value, value)])`.
pub type CurveRow<'a> = (&'a str, &'a str, &'a [(f64, f64)]);

/// One ability's cooldown and chunk scalar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ability {
    pub cooldown_s: f64,
    /// The share of a chunk of energy it receives; 1 where not given.
    pub flat_gain_factor: f64,
}

/// The imported economy tables.
#[derive(Debug, Clone, Default)]
pub struct Economy {
    abilities: BTreeMap<(String, String), Ability>,
    curves: BTreeMap<(String, String), Vec<(f64, f64)>>,
}

impl Economy {
    /// Read `ability_cooldown` and `stat_curve`. Abilities the cooldown
    /// workbook lacks take the Compendium's base cooldown and chunk scalar;
    /// a chunk scalar it leaves unknown is `economy.unknown_chunk_scalar`.
    pub fn load(db: &Database, vars: &Variables) -> Result<Self> {
        let mut economy = Self::default();
        let mut query = db.conn().prepare(
            "SELECT slot, name, cooldown_s, coalesce(flat_gain_factor, 1.0)
             FROM ability_cooldown WHERE cooldown_s IS NOT NULL",
        )?;
        let rows = query.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, f64>(3)?,
            ))
        })?;
        for row in rows {
            let (slot, name, cooldown_s, flat_gain_factor) = row?;
            economy.abilities.insert(
                (slot, name),
                Ability {
                    cooldown_s,
                    flat_gain_factor,
                },
            );
        }
        let mut query = db.conn().prepare(
            "SELECT CASE WHEN section LIKE '%Grenade Abilities' THEN 'grenade'
                         WHEN section LIKE '%Melee Abilities' THEN 'melee'
                         ELSE 'class_ability' END AS slot,
                    name,
                    try_cast(regexp_extract(text, 'Base Cooldown: ([0-9.]+)', 1) AS DOUBLE),
                    try_cast(regexp_extract(text, 'Chunk Scalar: ([0-9.]+)x', 1) AS DOUBLE)
             FROM mechanic_entry
             WHERE source = 'compendium'
               AND (section LIKE '%Grenade Abilities' OR section LIKE '%Melee Abilities'
                    OR section LIKE '%Class Abilities')
               AND text LIKE '%Base Cooldown:%'",
        )?;
        let rows = query.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<f64>>(2)?,
                r.get::<_, Option<f64>>(3)?,
            ))
        })?;
        let unknown_chunk = vars.engine("economy.unknown_chunk_scalar", 0.8);
        for row in rows {
            let (slot, name, cooldown_s, chunk) = row?;
            let Some(cooldown_s) = cooldown_s else {
                continue;
            };
            economy.abilities.entry((slot, name)).or_insert(Ability {
                cooldown_s,
                flat_gain_factor: chunk.unwrap_or(unknown_chunk),
            });
        }
        let mut query = db
            .conn()
            .prepare("SELECT stat, effect, stat_value, value FROM stat_curve ORDER BY ALL")?;
        let rows = query.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                f64::from(r.get::<_, u16>(2)?),
                r.get::<_, f64>(3)?,
            ))
        })?;
        for row in rows {
            let (stat, effect, at, value) = row?;
            economy
                .curves
                .entry((stat, effect))
                .or_default()
                .push((at, value));
        }
        Ok(economy)
    }

    /// Build from rows, for tests and hand-made scenarios: abilities as
    /// `(slot, name, cooldown_s, flat_gain_factor)`, curves as `(stat,
    /// effect, points)`.
    #[must_use]
    pub fn from_rows(abilities: &[AbilityRow<'_>], curves: &[CurveRow<'_>]) -> Self {
        let mut e = Self::default();
        for (slot, name, cooldown_s, flat_gain_factor) in abilities {
            e.abilities.insert(
                ((*slot).to_string(), (*name).to_string()),
                Ability {
                    cooldown_s: *cooldown_s,
                    flat_gain_factor: *flat_gain_factor,
                },
            );
        }
        for (stat, effect, points) in curves {
            e.curves.insert(
                ((*stat).to_string(), (*effect).to_string()),
                points.to_vec(),
            );
        }
        e
    }

    /// An ability, by slot (`grenade`, `melee`, `class_ability`, `super`).
    #[must_use]
    pub fn ability(&self, slot: &str, name: &str) -> Option<Ability> {
        self.abilities
            .get(&(slot.to_string(), name.to_string()))
            .copied()
    }

    /// A stat curve at a stat value, interpolated linearly between its
    /// points and held at its ends.
    #[must_use]
    pub fn curve(&self, stat: &str, effect: &str, at: f64) -> Option<f64> {
        let points = self.curves.get(&(stat.to_string(), effect.to_string()))?;
        let (first, last) = (points.first()?, points.last()?);
        if at <= first.0 {
            return Some(first.1);
        }
        if at >= last.0 {
            return Some(last.1);
        }
        points.windows(2).find_map(|w| {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            (at >= x0 && at <= x1).then(|| y0 + (y1 - y0) * (at - x0) / (x1 - x0))
        })
    }

    /// How much faster than at 0 stat an ability recharges.
    #[must_use]
    pub fn cooldown_scalar(&self, stat: &str, value: f64) -> f64 {
        self.curve(stat, "cooldown_scalar", value.min(100.0))
            .unwrap_or(1.0)
    }

    /// The share of a documented chunk (or regeneration bonus) a stat value
    /// gives: 0.40 at 0 to 0.90 at 100. Supers use their active regeneration
    /// curve.
    #[must_use]
    pub fn stat_scale(&self, stat: &str, value: f64) -> f64 {
        let effect = if stat == "super" {
            "active_regen_vs_pre_eof"
        } else {
            "flat_energy_vs_pre_eof"
        };
        self.curve(stat, effect, value).unwrap_or(1.0)
    }

    /// Charges a second from passive regeneration.
    #[must_use]
    pub fn charges_per_second(&self, slot: &str, name: &str, stat_value: f64) -> Option<f64> {
        let a = self.ability(slot, name)?;
        Some(self.cooldown_scalar(stat_of(slot), stat_value) / a.cooldown_s)
    }

    /// What a documented chunk of energy is worth, in charges.
    #[must_use]
    pub fn chunk(
        &self,
        slot: &str,
        name: &str,
        stat_value: f64,
        amount: f64,
        scaling: Scaling,
    ) -> f64 {
        let ces = self.ability(slot, name).map_or(1.0, |a| a.flat_gain_factor);
        let stat = self.stat_scale(stat_of(slot), stat_value);
        match scaling {
            Scaling::Chunk => amount * ces * stat,
            Scaling::Stat => amount * stat,
            Scaling::Fixed => amount,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The workbook's numbers for the abilities the tests use.
    pub(crate) fn economy() -> Economy {
        let stat_points: &[(f64, f64)] =
            &[(0.0, 0.40), (70.0, 0.797), (100.0, 0.90), (200.0, 0.90)];
        let scalar: &[(f64, f64)] = &[
            (0.0, 1.0),
            (30.0, 1.38),
            (70.0, 2.56),
            (100.0, 2.75),
            (110.0, 2.17),
        ];
        Economy::from_rows(
            &[
                ("melee", "Shiver Strike", 146.0, 0.70),
                ("melee", "Incinerator Snap", 120.0, 0.90),
                ("super", "Daybreak", 556.0, 1.0),
            ],
            &[
                ("melee", "flat_energy_vs_pre_eof", stat_points),
                ("melee", "cooldown_scalar", scalar),
                ("super", "active_regen_vs_pre_eof", stat_points),
            ],
        )
    }

    #[test]
    fn cooldowns_follow_the_stat_and_stop_at_100() {
        let e = economy();
        let per_s = e
            .charges_per_second("melee", "Shiver Strike", 100.0)
            .unwrap();
        assert!((1.0 / per_s - 146.0 / 2.75).abs() < 1e-9);
        // The sheet's 2.17 above 100 is not used.
        assert!((e.cooldown_scalar("melee", 150.0) - 2.75).abs() < 1e-12);
        assert!((e.cooldown_scalar("melee", 50.0) - (1.38 + (2.56 - 1.38) * 0.5)).abs() < 1e-12);
    }

    #[test]
    fn chunks_take_the_chunk_scalar_and_the_stat() {
        let e = economy();
        let hunger_small = 0.16;
        let got = e.chunk(
            "melee",
            "Shiver Strike",
            100.0,
            hunger_small,
            Scaling::Chunk,
        );
        assert!((got - 0.16 * 0.7 * 0.9).abs() < 1e-12);
        assert!(
            (e.chunk("melee", "Shiver Strike", 0.0, 0.16, Scaling::Stat) - 0.064).abs() < 1e-12
        );
        assert!(
            (e.chunk("melee", "Shiver Strike", 0.0, 0.16, Scaling::Fixed) - 0.16).abs() < 1e-12
        );
        assert_eq!(Scaling::parse(Some("stat")), Some(Scaling::Stat));
        assert_eq!(Scaling::parse(Some("other")), None);
    }
}
