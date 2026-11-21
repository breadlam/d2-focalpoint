//! The model's closed vocabulary (`data/sandbox/ontology.toml`): the events
//! rules fire on, the states they act on with the core verbs' parameters,
//! and the value lists every attribute is checked against.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

/// The repository's `data/sandbox/ontology.toml`, built in.
pub const ONTOLOGY: &str = include_str!("../../../data/sandbox/ontology.toml");

/// Value lists.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vocabulary {
    pub elements: Vec<String>,
    pub light: Vec<String>,
    pub darkness: Vec<String>,
    pub abilities: Vec<String>,
    pub ammo: Vec<String>,
    pub sources: Vec<String>,
    pub ranks: Vec<String>,
    pub champions: Vec<String>,
    pub tiers: Vec<u8>,
    pub damage_categories: Vec<String>,
    pub exclusive_categories: Vec<String>,
    pub targets: Vec<String>,
    /// Labels damage instances carry, for modifiers to select.
    pub damage_tags: Vec<String>,
    /// Parameters every ability has (`melee.charges`).
    pub ability_params: Vec<String>,
    /// Movement an event or condition may name.
    pub movement: Vec<String>,
    /// Character and weapon stats a modifier may change.
    pub stats: Vec<String>,
    /// Source families a citation may start with (`compendium:`).
    pub source_families: Vec<String>,
}

/// Something that happens, that rules fire on.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventDef {
    pub name: String,
    pub attributes: Vec<String>,
    pub description: String,
}

/// A core verb's parameter, with its `PvE` value when a source gives one.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Param {
    pub name: String,
    pub value: Option<f64>,
    pub unit: String,
    pub note: Option<String>,
}

/// A resource, buff, debuff, object, counter or verb.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateDef {
    pub name: String,
    /// resource | buff | debuff | object | verb
    pub kind: String,
    pub element: Option<String>,
    pub description: String,
    #[serde(default)]
    pub sources: Vec<String>,
    pub max_stacks: Option<u32>,
    /// The event fired when the stacks reach `max_stacks`.
    pub at_max: Option<String>,
    pub damage_category: Option<String>,
    #[serde(default)]
    pub params: Vec<Param>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    vocabulary: Vocabulary,
    #[serde(default)]
    event: Vec<EventDef>,
    #[serde(default)]
    state: Vec<StateDef>,
}

/// The vocabulary, indexed.
#[derive(Debug, Clone)]
pub struct Ontology {
    pub vocabulary: Vocabulary,
    pub events: BTreeMap<String, EventDef>,
    pub states: BTreeMap<String, StateDef>,
}

const STATE_KINDS: [&str; 5] = ["resource", "buff", "debuff", "object", "verb"];

impl Ontology {
    /// Parse and check an ontology file: names unique, kinds known, `at_max`
    /// events defined, elements and categories in the vocabulary.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let file: File = toml::from_str(text)?;
        let mut events = BTreeMap::new();
        for e in file.event {
            anyhow::ensure!(
                !events.contains_key(&e.name),
                "event {} defined twice",
                e.name
            );
            events.insert(e.name.clone(), e);
        }
        let v = &file.vocabulary;
        let mut states = BTreeMap::new();
        for s in file.state {
            anyhow::ensure!(
                !states.contains_key(&s.name),
                "state {} defined twice",
                s.name
            );
            anyhow::ensure!(
                STATE_KINDS.contains(&s.kind.as_str()),
                "state {}: kind {:?} is not one of {STATE_KINDS:?}",
                s.name,
                s.kind
            );
            if let Some(e) = &s.element {
                anyhow::ensure!(
                    e == "prismatic" || v.elements.contains(e),
                    "state {}: element {e:?} is not in the vocabulary",
                    s.name
                );
            }
            if let Some(ev) = &s.at_max {
                anyhow::ensure!(
                    events.contains_key(ev),
                    "state {}: at_max event {ev:?} is not defined",
                    s.name
                );
            }
            if let Some(c) = &s.damage_category {
                anyhow::ensure!(
                    v.damage_categories.contains(c),
                    "state {}: damage category {c:?} is not in the vocabulary",
                    s.name
                );
            }
            let mut names = BTreeSet::new();
            for p in &s.params {
                anyhow::ensure!(
                    names.insert(&p.name),
                    "state {}: parameter {} defined twice",
                    s.name,
                    p.name
                );
            }
            states.insert(s.name.clone(), s);
        }
        Ok(Self {
            vocabulary: file.vocabulary,
            events,
            states,
        })
    }

    /// The built-in ontology.
    pub fn builtin() -> anyhow::Result<Self> {
        Self::parse(ONTOLOGY)
    }

    /// Whether `path` names a parameter: `<state>.<param>` of a core verb,
    /// or `<ability>.<ability param>`.
    #[must_use]
    pub fn has_param(&self, path: &str) -> bool {
        let Some((head, param)) = path.split_once('.') else {
            return false;
        };
        if self.vocabulary.abilities.iter().any(|a| a == head) {
            return self.vocabulary.ability_params.iter().any(|p| p == param);
        }
        self.states
            .get(head)
            .is_some_and(|s| s.params.iter().any(|p| p.name == param))
    }

    /// A parameter's base value.
    #[must_use]
    pub fn param(&self, path: &str) -> Option<f64> {
        let (head, param) = path.split_once('.')?;
        self.states
            .get(head)?
            .params
            .iter()
            .find(|p| p.name == param)?
            .value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_ontology_parses() {
        let o = Ontology::builtin().unwrap();
        assert!(o.events.contains_key("ignition_hit"));
        assert_eq!(o.states["scorch"].at_max.as_deref(), Some("ignition"));
        assert_eq!(o.param("ignition.damage"), Some(676.0));
        assert!(o.has_param("melee.charges"));
        assert!(o.has_param("stasis_shard.melee_energy_small"));
        assert!(!o.has_param("stasis_shard.nonsense"));
        assert!(!o.has_param("nonsense"));
    }

    #[test]
    fn inconsistencies_are_rejected() {
        let base = ONTOLOGY;
        let bad_kind =
            format!("{base}\n[[state]]\nname = \"x\"\nkind = \"thing\"\ndescription = \"\"\n");
        assert!(Ontology::parse(&bad_kind).is_err());
        let bad_event = format!(
            "{base}\n[[state]]\nname = \"x\"\nkind = \"debuff\"\nat_max = \"nowhere\"\ndescription = \"\"\n"
        );
        assert!(Ontology::parse(&bad_event).is_err());
    }
}
