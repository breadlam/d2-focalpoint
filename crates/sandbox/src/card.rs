//! Part cards: what each build part does, as rules over the ontology's
//! vocabulary (`data/sandbox/parts/*.toml`; format in `docs/sandbox.md`).
//!
//! A card names its part (cards resolve to every manifest item of that name
//! and kind, so reissues share one card) and lists:
//! * `rule`s — *on* an event, *when* conditions hold, do *effects*;
//! * `modify` — passive changes to a core verb's or ability's parameters,
//!   a damage tag or a stat;
//! * `counter`s — the part's own progress counters.
//!
//! Every card cites its sources; rules and modifiers may cite more and
//! carry their own confidence.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

/// Kinds of part a card describes (`build_component.kind`; exotic armor
/// and weapons only for `armor` and `weapon`, whose legendaries are
/// modelled by archetype).
pub const PART_KINDS: [&str; 18] = [
    "super",
    "grenade",
    "melee",
    "class_ability",
    "movement",
    "aspect",
    "fragment",
    "transcendence",
    "armor",
    "weapon",
    "armor_perk",
    "set_bonus",
    "artifact_perk",
    "armor_mod",
    "weapon_perk",
    "weapon_intrinsic",
    "origin_trait",
    "weapon_mod",
];

/// How sure a number or rule is, as in `data/difficulty.toml`.
pub const CONFIDENCE: [&str; 3] = ["documented", "dated", "assumed"];

/// One file of cards.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardFile {
    #[serde(default)]
    pub part: Vec<Card>,
}

/// One part.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub name: String,
    pub kind: String,
    /// Restricts the card to these manifest hashes; empty for every item of
    /// the name and kind.
    #[serde(default)]
    pub hashes: Vec<u32>,
    pub sources: Vec<String>,
    #[serde(default = "documented")]
    pub confidence: String,
    pub note: Option<String>,
    #[serde(default)]
    pub rule: Vec<Rule>,
    #[serde(default)]
    pub modify: Vec<Modify>,
    #[serde(default)]
    pub counter: Vec<CounterDef>,
}

fn documented() -> String {
    "documented".into()
}

/// On an event, when conditions hold, do effects.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub on: Trigger,
    #[serde(default)]
    pub when: Vec<Condition>,
    #[serde(deserialize_with = "one_or_many")]
    pub effect: Vec<Effect>,
    /// Seconds before the rule can fire again.
    pub cooldown: Option<f64>,
    /// Probability the rule fires.
    pub chance: Option<f64>,
    /// Times the rule can fire per activation (per super, per buff).
    pub limit: Option<u32>,
    /// Field overrides of the (single) effect while another part is equipped.
    #[serde(default)]
    pub with: Vec<Variant>,
    pub confidence: Option<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    pub note: Option<String>,
}

/// An event and the attribute values it must have: `"kill"` or
/// `{ event = "kill", source = "weapon", element = "solar" }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Trigger {
    pub event: String,
    pub attributes: BTreeMap<String, toml::Value>,
}

impl<'de> Deserialize<'de> for Trigger {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        match toml::Value::deserialize(d)? {
            toml::Value::String(event) => Ok(Self {
                event,
                attributes: BTreeMap::new(),
            }),
            toml::Value::Table(mut t) => {
                let Some(toml::Value::String(event)) = t.remove("event") else {
                    return Err(D::Error::custom("a trigger table needs `event = \"...\"`"));
                };
                Ok(Self {
                    event,
                    attributes: t.into_iter().collect(),
                })
            }
            other => Err(D::Error::custom(format!(
                "a trigger is an event name or a table, not {other}"
            ))),
        }
    }
}

fn one_or_many<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<T>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany<T> {
        Many(Vec<T>),
        One(T),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::Many(v) => v,
        OneOrMany::One(x) => vec![x],
    })
}

/// A condition; exactly one of the leading fields is set (with `min` for
/// `stacks` and `enemies_within`).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    /// The player has this buff.
    pub has: Option<String>,
    /// The player does not have this buff.
    pub lacks: Option<String>,
    /// A part of this name is equipped.
    pub equipped: Option<String>,
    /// At least `min` stacks of this state on the player, or progress on one
    /// of the card's counters.
    pub stacks: Option<String>,
    /// The player is moving this way (slide, airborne, ...).
    pub movement: Option<String>,
    /// The player's super is (or is not) active.
    pub in_super: Option<bool>,
    /// The player's health fraction is below this.
    pub health_below: Option<f64>,
    /// At least `min` enemies within this radius (m).
    pub enemies_within: Option<f64>,
    /// The event's target has this state.
    pub target_has: Option<String>,
    /// The event's target is of one of these ranks.
    pub target_rank: Option<Vec<String>>,
    /// The event's target is of none of these ranks.
    pub target_rank_not: Option<Vec<String>>,
    pub min: Option<f64>,
}

/// A number, or a table of numbers by enemy tier, rank, a counter's value
/// or the count of copies equipped.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Amount {
    Number(f64),
    Table(AmountTable),
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AmountTable {
    /// By combatant tier, from T1.
    pub by_tier: Option<Vec<f64>>,
    /// By rank (`minor`, `elite`, ...).
    pub by_rank: Option<BTreeMap<String, f64>>,
    /// `[[counter value, amount], ...]`, the highest row reached applies.
    pub by_counter: Option<Vec<[f64; 2]>>,
    /// The counter the rows read (with `by_counter`).
    pub counter: Option<String>,
    /// By copies equipped, from one.
    pub by_count: Option<Vec<f64>>,
}

/// One effect; exactly one of the leading (primary) fields is set.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Effect {
    /// Apply a buff or debuff (`stacks`, `duration`, `target`, `radius`).
    pub apply: Option<String>,
    /// Deal damage (`radius`, `falloff`, `element`, `tags`, `source`).
    pub damage: Option<Amount>,
    /// Grant flat ability energy, a fraction of a charge (`amount`).
    pub energy: Option<String>,
    /// Multiply an ability's regeneration (`amount`, `duration`, `max_duration`).
    pub regen: Option<String>,
    /// Create objects (`count`, `size` or `sizes`).
    pub spawn: Option<String>,
    /// Restore health.
    pub heal: Option<Amount>,
    /// Fire an event (`attributes`; `damage`/`radius` override the verb's).
    pub emit: Option<String>,
    /// Grant ability charges (`count`).
    pub charges: Option<String>,
    /// Remove a state or reset a counter.
    pub consume: Option<String>,
    /// Advance one of the card's counters (`amount`).
    pub progress: Option<String>,

    pub amount: Option<Amount>,
    pub stacks: Option<Amount>,
    pub count: Option<Amount>,
    /// Seconds.
    pub duration: Option<f64>,
    pub max_duration: Option<f64>,
    /// Metres.
    pub radius: Option<f64>,
    /// Damage at the edge of `radius`, as a fraction.
    pub falloff: Option<f64>,
    pub size: Option<String>,
    /// Counts by size: `{ small = 2, medium = 4, large = 2 }`.
    pub sizes: Option<BTreeMap<String, f64>>,
    pub target: Option<String>,
    /// Excluded from `target`: `origin` (the event's target).
    pub except: Option<String>,
    pub element: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// What the damage counts as (weapon, ability, melee, ...).
    pub source: Option<String>,
    /// Attributes of the emitted event.
    pub attributes: Option<BTreeMap<String, toml::Value>>,
}

impl Effect {
    /// The primary fields set, by name. With `emit`, `damage` qualifies the
    /// emitted verb (its damage instead of the verb's own).
    #[must_use]
    pub fn primaries(&self) -> Vec<&'static str> {
        let set = [
            ("apply", self.apply.is_some()),
            ("damage", self.damage.is_some() && self.emit.is_none()),
            ("energy", self.energy.is_some()),
            ("regen", self.regen.is_some()),
            ("spawn", self.spawn.is_some()),
            ("heal", self.heal.is_some()),
            ("emit", self.emit.is_some()),
            ("charges", self.charges.is_some()),
            ("consume", self.consume.is_some()),
            ("progress", self.progress.is_some()),
        ];
        set.into_iter()
            .filter(|(_, s)| *s)
            .map(|(n, _)| n)
            .collect()
    }

    /// This effect with a variant's overrides applied.
    pub fn with(&self, overrides: &BTreeMap<String, toml::Value>) -> anyhow::Result<Self> {
        let toml::Value::Table(mut table) = toml::Value::try_from(self)? else {
            unreachable!("an effect serialises to a table")
        };
        for (k, v) in overrides {
            table.insert(k.clone(), v.clone());
        }
        Ok(toml::Value::Table(table).try_into()?)
    }
}

/// While the named part is equipped, the rule's effect has these fields.
#[derive(Debug, Clone, Deserialize)]
pub struct Variant {
    pub part: String,
    #[serde(flatten)]
    pub overrides: BTreeMap<String, toml::Value>,
}

/// A passive change: to a parameter (`ignition.damage`, `melee.charges`), to
/// damage carrying a tag, or to a stat; exactly one of `add`, `mul`, `set`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Modify {
    pub param: Option<String>,
    pub damage: Option<String>,
    pub stat: Option<String>,
    /// Restricts a damage modifier to damage of this element.
    pub element: Option<String>,
    pub add: Option<f64>,
    pub mul: Option<f64>,
    pub set: Option<f64>,
    /// Court's category of a damage modifier.
    pub category: Option<String>,
    /// `up` to round the result up (Ember of Ashes' Scorch).
    pub round: Option<String>,
    #[serde(default)]
    pub when: Vec<Condition>,
    pub confidence: Option<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    pub note: Option<String>,
}

/// A counter of the part's own: progress accumulates to `threshold`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CounterDef {
    pub name: String,
    #[serde(default = "one")]
    pub threshold: f64,
    pub note: Option<String>,
}

fn one() -> f64 {
    1.0
}

/// Parse a card file.
pub fn parse(text: &str) -> anyhow::Result<Vec<Card>> {
    Ok(toml::from_str::<CardFile>(text)?.part)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_parse_with_short_and_long_forms() {
        let cards = parse(
            r#"
            [[part]]
            name = "Ember of Char"
            kind = "fragment"
            sources = ["compendium:Solar/Fragments/Char"]

            [[part.rule]]
            on = { event = "ignition_hit", origin = false }
            effect = { apply = "scorch", stacks = 40, target = "target" }
            with = [{ part = "Ember of Ashes", stacks = 60 }]

            [[part.rule]]
            on = "kill"
            effect = [{ energy = "melee", amount = { by_tier = [0.08, 0.15] } }, { spawn = "firesprite" }]
            "#,
        )
        .unwrap();
        let c = &cards[0];
        assert_eq!(c.rule[0].on.event, "ignition_hit");
        assert_eq!(
            c.rule[0].on.attributes["origin"],
            toml::Value::Boolean(false)
        );
        assert_eq!(c.rule[0].effect[0].stacks, Some(Amount::Number(40.0)));
        assert_eq!(c.rule[1].effect.len(), 2);
        let with = c.rule[0].effect[0]
            .with(&c.rule[0].with[0].overrides)
            .unwrap();
        assert_eq!(with.stacks, Some(Amount::Number(60.0)));
        assert_eq!(with.apply.as_deref(), Some("scorch"));
    }

    #[test]
    fn an_emitted_verb_may_carry_its_damage() {
        let e = Effect {
            emit: Some("ignition".into()),
            damage: Some(Amount::Number(171.0)),
            ..Effect::default()
        };
        assert_eq!(e.primaries(), ["emit"]);
        let d = Effect {
            damage: Some(Amount::Number(1.0)),
            ..Effect::default()
        };
        assert_eq!(d.primaries(), ["damage"]);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        assert!(
            parse("[[part]]\nname = \"x\"\nkind = \"fragment\"\nsources = []\ncolour = \"red\"")
                .is_err()
        );
        assert!(parse(
            "[[part]]\nname = \"x\"\nkind = \"fragment\"\nsources = []\n[[part.rule]]\non = \"kill\"\neffect = { aply = \"scorch\" }"
        )
        .is_err());
    }
}
