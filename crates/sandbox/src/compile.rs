//! Compiling a loadout: the records it grants, and the core verbs'
//! parameters with its passive modifiers applied.
//!
//! A passive modifier (no `when`) changes a parameter once, here: every
//! `set` (the largest wins), then every `add`, then every `mul`. Modifiers
//! with conditions, and those on damage or stats, are kept for the solvers.

use std::collections::{BTreeMap, BTreeSet};

use crate::ontology::Ontology;
use crate::rules::{Modifier, Part, Rule, RuleBase};

/// A loadout: the parts equipped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Build {
    pub parts: BTreeSet<Part>,
}

impl Build {
    /// A build from `kind:name` parts.
    pub fn parse<'a>(parts: impl IntoIterator<Item = &'a str>) -> anyhow::Result<Self> {
        let mut out = BTreeSet::new();
        for p in parts {
            out.insert(Part::try_from(p.to_string()).map_err(anyhow::Error::msg)?);
        }
        Ok(Self { parts: out })
    }

    /// The equipped part of a kind (`grenade`, `melee`, `class_ability`,
    /// `super`).
    #[must_use]
    pub fn equipped(&self, kind: &str) -> Option<&str> {
        self.parts
            .iter()
            .find(|p| p.kind == kind)
            .map(|p| p.name.as_str())
    }
}

/// A compiled build.
#[derive(Debug, Clone)]
pub struct Compiled<'a> {
    pub build: Build,
    pub rules: Vec<&'a Rule>,
    /// Modifiers with conditions, on damage or on stats.
    pub modifiers: Vec<&'a Modifier>,
    /// Every declared counter's threshold.
    pub counters: BTreeMap<String, f64>,
    params: BTreeMap<String, f64>,
    round_up: BTreeSet<String>,
}

/// Compile a build against the rule base.
#[must_use]
pub fn compile<'a>(o: &Ontology, base: &'a RuleBase, build: &Build) -> Compiled<'a> {
    let granted = base.granted(&build.parts);
    let mut by_param: BTreeMap<&str, Vec<&Modifier>> = BTreeMap::new();
    let mut modifiers = Vec::new();
    for m in granted.modifiers {
        match &m.param {
            Some(p) if m.when.is_empty() => by_param.entry(p.as_str()).or_default().push(m),
            _ => modifiers.push(m),
        }
    }
    let mut params = BTreeMap::new();
    let mut round_up = BTreeSet::new();
    for (param, mods) in by_param {
        let sets = mods.iter().filter_map(|m| m.set);
        let mut v = sets
            .fold(None, |acc: Option<f64>, x| {
                Some(acc.map_or(x, |a| a.max(x)))
            })
            .or_else(|| o.param(param))
            .unwrap_or(0.0);
        v += mods.iter().filter_map(|m| m.add).sum::<f64>();
        v *= mods.iter().filter_map(|m| m.mul).product::<f64>();
        params.insert(param.to_string(), v);
        if mods.iter().any(|m| m.round.as_deref() == Some("up")) {
            round_up.insert(param.to_string());
        }
    }
    Compiled {
        build: build.clone(),
        rules: granted.rules,
        modifiers,
        counters: base
            .counters
            .iter()
            .map(|c| (c.item.name.clone(), c.item.threshold))
            .collect(),
        params,
        round_up,
    }
}

impl Compiled<'_> {
    /// A parameter after the build's passive modifiers, or the ontology's.
    #[must_use]
    pub fn param(&self, o: &Ontology, path: &str) -> Option<f64> {
        self.params.get(path).copied().or_else(|| o.param(path))
    }

    /// Whether the build modifies a parameter at all.
    #[must_use]
    pub fn modifies(&self, path: &str) -> bool {
        self.params.contains_key(path)
    }

    /// Stacks of a state as applied: the documented stacks times the
    /// state's `applied_multiplier`, rounded up where a modifier says so
    /// (Ember of Ashes).
    #[must_use]
    pub fn applied_stacks(&self, o: &Ontology, state: &str, stacks: f64) -> f64 {
        let path = format!("{state}.applied_multiplier");
        let m = self.param(o, &path).unwrap_or(1.0);
        let v = stacks * m;
        if self.round_up.contains(&path) {
            (v - 1e-9).ceil()
        } else {
            v
        }
    }

    /// The expected damage multiplier for damage carrying `tags`, of
    /// `element`. `p[i]` is the probability `modifiers[i]`'s conditions
    /// hold (1 where missing). Within the exclusive categories only the
    /// largest applies; the rest multiply.
    #[must_use]
    pub fn damage_multiplier(
        &self,
        o: &Ontology,
        tags: &[String],
        element: Option<&str>,
        p: &[f64],
    ) -> f64 {
        let mut exclusive: BTreeMap<&str, f64> = BTreeMap::new();
        let mut total = 1.0;
        for (i, m) in self.modifiers.iter().enumerate() {
            let Some(tag) = &m.damage else { continue };
            if tag != "any" && !tags.iter().any(|t| t == tag) {
                continue;
            }
            if let (Some(want), Some(have)) = (&m.element, element)
                && want != have
            {
                continue;
            }
            if m.element.is_some() && element.is_none() {
                continue;
            }
            let factor = 1.0 + p.get(i).copied().unwrap_or(1.0) * (m.mul.unwrap_or(1.0) - 1.0);
            match m.category.as_deref() {
                Some(c) if o.vocabulary.exclusive_categories.iter().any(|x| x == c) => {
                    let e = exclusive.entry(c).or_insert(1.0);
                    *e = e.max(factor);
                }
                _ => total *= factor,
            }
        }
        total * exclusive.values().product::<f64>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passive_modifiers_resolve_and_ashes_rounds_up() {
        let o = Ontology::builtin().unwrap();
        let base = crate::builtin_rules().unwrap();
        let b = Build::parse([
            "fragment:Ember of Ashes",
            "fragment:Whisper of Hunger",
            "fragment:Whisper of Rime",
            "armor:Dawn Chorus",
        ])
        .unwrap();
        let c = compile(&o, &base, &b);
        assert!((c.param(&o, "stasis_shard.melee_energy_small").unwrap() - 0.16).abs() < 1e-12);
        assert_eq!(c.param(&o, "frost_armor.max_stacks"), Some(7.0));
        assert!((c.applied_stacks(&o, "scorch", 40.0) - 60.0).abs() < 1e-12);
        assert!((c.applied_stacks(&o, "scorch", 15.0) - 23.0).abs() < 1e-12);
        let ignition = ["ignition".to_string()];
        assert!((c.damage_multiplier(&o, &ignition, Some("solar"), &[]) - 2.0).abs() < 1e-12);
        assert!((c.damage_multiplier(&o, &[], Some("solar"), &[]) - 1.0).abs() < 1e-12);
        assert_eq!(c.build.equipped("armor"), Some("Dawn Chorus"));
    }
}
