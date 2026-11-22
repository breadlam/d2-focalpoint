//! Variables: the values the model needs and no source gives exactly
//! (`data/sandbox/variables.toml`, `docs/measurements.md`).
//!
//! The model runs on each variable's guess. Any run can set one, and every
//! result says which variables it read, so the sensitivity of a result to
//! each unknown is one run away. Records reference a variable as
//! `{ var = "name" }`; a core verb's parameter without a value in the
//! ontology is the variable of the same name.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail, ensure};
use serde::Deserialize;

/// The repository's `data/sandbox/variables.toml`, built in.
pub const VARIABLES: &str = include_str!("../../../data/sandbox/variables.toml");

/// How a variable's value is known.
pub const STATUS: [&str; 3] = ["guess", "estimate", "measured"];

/// The variables the engine itself reads (policies, scenario priors, the
/// economy's readings), which must be defined.
pub const ENGINE: [&str; 23] = [
    "policy.jolt_chains_per_enemy",
    "jolt.chain_targets",
    "bolt_charge.discharge_radius",
    "policy.object_pickup",
    "policy.precision_share",
    "policy.weapon_element_match",
    "policy.airborne",
    "policy.sprint",
    "policy.other_movement",
    "scenario.packs_per_minute",
    "policy.min_cast_interval",
    "abilities.boss_scalar",
    "policy.shard_pickup",
    "policy.shatter_frozen",
    "policy.crystals_in_pack",
    "policy.slide_form",
    "policy.critical_health",
    "scenario.pack_size",
    "scenario.pack_radius",
    "scenario.elite_share",
    "build.default_stat",
    "economy.documented_chunk_scale",
    "economy.regen_relative_to_current",
];

/// One unknown value.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variable {
    pub name: String,
    pub guess: f64,
    pub low: f64,
    pub high: f64,
    pub unit: String,
    pub status: String,
    pub basis: String,
    pub measure: String,
    #[serde(default)]
    pub sources: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    variable: Vec<Variable>,
}

/// The variables, with any values set for a run, and the names read.
#[derive(Debug, Default)]
pub struct Variables {
    defs: BTreeMap<String, Variable>,
    set: BTreeMap<String, f64>,
    read: RefCell<BTreeSet<String>>,
}

impl Clone for Variables {
    fn clone(&self) -> Self {
        Self {
            defs: self.defs.clone(),
            set: self.set.clone(),
            read: RefCell::new(BTreeSet::new()),
        }
    }
}

impl Variables {
    /// Parse and check a variables file: names unique, `low <= guess <=
    /// high`, a known status, a basis and a way to measure.
    pub fn parse(text: &str) -> Result<Self> {
        let file: File = toml::from_str(text)?;
        let mut defs = BTreeMap::new();
        for v in file.variable {
            ensure!(!v.name.is_empty(), "a variable has no name");
            ensure!(
                v.low <= v.guess && v.guess <= v.high,
                "{}: the guess {} is outside {}..{}",
                v.name,
                v.guess,
                v.low,
                v.high
            );
            ensure!(
                STATUS.contains(&v.status.as_str()),
                "{}: status {:?} is not one of {STATUS:?}",
                v.name,
                v.status
            );
            ensure!(
                !v.basis.trim().is_empty() && !v.measure.trim().is_empty(),
                "{}: a variable needs a basis and a way to measure it",
                v.name
            );
            let name = v.name.clone();
            ensure!(
                defs.insert(name.clone(), v).is_none(),
                "variable {name} defined twice"
            );
        }
        Ok(Self {
            defs,
            ..Self::default()
        })
    }

    /// The built-in variables.
    pub fn builtin() -> Result<Self> {
        Self::parse(VARIABLES)
    }

    /// Whether a variable is defined.
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.defs.contains_key(name)
    }

    /// A variable's value for this run: as set, or its guess. The name is
    /// recorded as read.
    #[must_use]
    pub fn value(&self, name: &str) -> Option<f64> {
        let v = self
            .set
            .get(name)
            .copied()
            .or_else(|| self.defs.get(name).map(|d| d.guess))?;
        self.read.borrow_mut().insert(name.to_string());
        Some(v)
    }

    /// A variable the engine needs ([`ENGINE`]): its value, or `fallback`
    /// if a custom file leaves it out.
    #[must_use]
    pub fn engine(&self, name: &str, fallback: f64) -> f64 {
        self.value(name).unwrap_or(fallback)
    }

    /// Set a variable for this run.
    pub fn set(&mut self, name: &str, value: f64) -> Result<()> {
        if !self.defs.contains_key(name) {
            bail!("no variable {name:?} (see `focalpoint sandbox unknowns`)");
        }
        self.set.insert(name.to_string(), value);
        Ok(())
    }

    /// The variables set for this run.
    #[must_use]
    pub fn overrides(&self) -> &BTreeMap<String, f64> {
        &self.set
    }

    /// The names read since the last [`Variables::take_read`].
    pub fn take_read(&self) -> BTreeSet<String> {
        std::mem::take(&mut *self.read.borrow_mut())
    }

    /// Every variable.
    pub fn iter(&self) -> impl Iterator<Item = &Variable> {
        self.defs.values()
    }

    /// Keep only the variables `keep` accepts.
    pub fn retain(&mut self, keep: impl Fn(&Variable) -> bool) {
        self.defs.retain(|_, v| keep(v));
    }

    /// One variable's definition.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Variable> {
        self.defs.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_variables_parse_and_cover_the_engine() {
        let v = Variables::builtin().unwrap();
        for name in ENGINE {
            assert!(v.has(name), "{name} is not defined");
        }
        assert!((v.value("scenario.pack_size").unwrap() - 8.0).abs() < 1e-12);
        assert!(v.take_read().contains("scenario.pack_size"));
        assert!(v.take_read().is_empty());
    }

    #[test]
    fn values_can_be_set_and_bad_files_fail() {
        let mut v = Variables::builtin().unwrap();
        v.set("scenario.pack_size", 12.0).unwrap();
        assert!((v.value("scenario.pack_size").unwrap() - 12.0).abs() < 1e-12);
        assert!(v.set("nonsense", 1.0).is_err());
        let bad = "[[variable]]\nname = \"x\"\nguess = 5\nlow = 6\nhigh = 7\nunit = \"m\"\nstatus = \"guess\"\nbasis = \"b\"\nmeasure = \"m\"";
        assert!(Variables::parse(bad).is_err());
        let no_measure = "[[variable]]\nname = \"x\"\nguess = 6\nlow = 6\nhigh = 7\nunit = \"m\"\nstatus = \"guess\"\nbasis = \"b\"\nmeasure = \" \"";
        assert!(Variables::parse(no_measure).is_err());
    }
}
