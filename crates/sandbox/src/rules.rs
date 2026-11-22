//! The rule base: what build parts do, as rules and modifiers over the
//! ontology's vocabulary (`data/sandbox/rules/*.toml`; format in
//! `docs/sandbox.md`).
//!
//! Rules and modifiers are the unit of knowledge, not parts. Each record
//! says what grants it: one part, or every one of several together (Dawn
//! Chorus with Daybreak). A part does whatever is granted by it, alone or
//! with others, so behaviour that belongs to a combination needs no special
//! case in any part. A build is compiled by keeping the records its
//! loadout grants ([`RuleBase::granted`]); conditions on the loadout are
//! settled there, and only conditions on the moment (`when`) reach the
//! solvers.
//!
//! * `rule`: *on* an event, *when* conditions hold, do *effects*;
//! * `modifier`: a change to a core verb's or ability's parameter, to
//!   damage carrying a tag, or to a stat;
//! * `counter`: a named progress counter that rules advance and read;
//! * `gap`: a part's behaviour that is known and not yet modelled.
//!
//! Every record cites its sources and has a confidence.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

/// Kinds of build part (`build_component.kind`; exotic armor and weapons
/// only for `armor` and `weapon`, whose legendaries are modelled by
/// archetype).
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

/// A build part, written `kind:name` (`fragment:Ember of Char`). It stands
/// for every manifest item of that kind and name, so reissues share it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct Part {
    pub kind: String,
    pub name: String,
}

impl Part {
    #[must_use]
    pub fn new(kind: &str, name: &str) -> Self {
        Self {
            kind: kind.into(),
            name: name.into(),
        }
    }
}

impl TryFrom<String> for Part {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        match s.split_once(':') {
            Some((kind, name)) if !kind.is_empty() && !name.is_empty() => Ok(Self::new(kind, name)),
            _ => Err(format!("a part is written `kind:name`, not {s:?}")),
        }
    }
}

impl fmt::Display for Part {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind, self.name)
    }
}

/// What grants a record: every one of these parts, equipped together.
/// Written as one part, `"fragment:Ember of Char"`, or a list of all of
/// them, `["armor:Dawn Chorus", "super:Daybreak"]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub all: Vec<Part>,
}

impl Grant {
    /// Whether a loadout grants the record: it has every part.
    #[must_use]
    pub fn by(&self, loadout: &BTreeSet<Part>) -> bool {
        self.all.iter().all(|p| loadout.contains(p))
    }

    /// The part, when one part grants the record alone.
    #[must_use]
    pub fn sole(&self) -> Option<&Part> {
        match self.all.as_slice() {
            [p] => Some(p),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for Grant {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self {
            all: one_or_many(d)?,
        })
    }
}

impl fmt::Display for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, p) in self.all.iter().enumerate() {
            if i > 0 {
                f.write_str(" + ")?;
            }
            write!(f, "{p}")?;
        }
        Ok(())
    }
}

/// One file of the rule base.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleFile {
    #[serde(default)]
    pub rule: Vec<Rule>,
    #[serde(default)]
    pub modifier: Vec<Modifier>,
    #[serde(default)]
    pub counter: Vec<Counter>,
    #[serde(default)]
    pub gap: Vec<Gap>,
}

fn documented() -> String {
    "documented".into()
}

/// On an event, when conditions hold, do effects.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub granted_by: Grant,
    pub sources: Vec<String>,
    #[serde(default = "documented")]
    pub confidence: String,
    pub note: Option<String>,
    pub on: Trigger,
    #[serde(default)]
    pub when: Vec<Condition>,
    #[serde(deserialize_with = "one_or_many")]
    pub effect: Vec<Effect>,
    /// Seconds before the rule can fire again.
    pub cooldown: Option<Amount>,
    /// Probability the rule fires.
    pub chance: Option<Amount>,
    /// Times the rule can fire per activation (per super, per buff).
    pub limit: Option<u32>,
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

/// A condition on the moment; exactly one of the leading fields is set
/// (with `min` for `stacks` and `enemies_within`). Conditions on the
/// loadout are not conditions: they are the record's grant.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    /// The player has this buff.
    pub has: Option<String>,
    /// The player does not have this buff.
    pub lacks: Option<String>,
    /// At least `min` stacks of this state on the player, or progress on a
    /// counter.
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

/// A number, a variable (`{ var = "howl_of_the_storm.freeze_radius" }`,
/// `data/sandbox/variables.toml`), or a table of numbers by enemy tier,
/// rank, a counter's value or the count of copies equipped.
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
    /// By copies of the granting part equipped, from one.
    pub by_count: Option<Vec<f64>>,
    /// A variable's value.
    pub var: Option<String>,
}

impl Amount {
    /// A variable amount.
    #[must_use]
    pub fn var(name: &str) -> Self {
        Self::Table(AmountTable {
            var: Some(name.to_string()),
            ..AmountTable::default()
        })
    }

    /// The variable an amount names, if it is one.
    #[must_use]
    pub fn var_name(&self) -> Option<&str> {
        match self {
            Amount::Table(t) => t.var.as_deref(),
            Amount::Number(_) => None,
        }
    }

    /// A plain number or a variable's value; tables need a scenario.
    #[must_use]
    pub fn fixed(&self, vars: &crate::variables::Variables) -> Option<f64> {
        match self {
            Amount::Number(x) => Some(*x),
            Amount::Table(t) => t.var.as_deref().and_then(|v| vars.value(v)),
        }
    }
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
    /// Advance a counter (`amount`).
    pub progress: Option<String>,

    pub amount: Option<Amount>,
    pub stacks: Option<Amount>,
    pub count: Option<Amount>,
    /// Seconds.
    pub duration: Option<Amount>,
    pub max_duration: Option<Amount>,
    /// Metres.
    pub radius: Option<Amount>,
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
    /// How energy is scaled: `chunk` (default: the ability's chunk scalar
    /// and the stat), `stat` (Clarity's "not affected by Chunk Energy
    /// Scalars") or `fixed` (neither).
    pub scaling: Option<String>,
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
}

/// A change to a parameter (`ignition.damage`, `melee.charges`), to damage
/// carrying a tag, or to a stat, passive or `when` conditions hold;
/// exactly one of `add`, `mul`, `set`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Modifier {
    pub granted_by: Grant,
    pub sources: Vec<String>,
    #[serde(default = "documented")]
    pub confidence: String,
    pub note: Option<String>,
    pub param: Option<String>,
    pub damage: Option<String>,
    pub stat: Option<String>,
    /// Restricts a damage modifier to damage of this element.
    pub element: Option<String>,
    pub add: Option<Amount>,
    pub mul: Option<Amount>,
    pub set: Option<Amount>,
    /// Court's category of a damage modifier.
    pub category: Option<String>,
    /// `up` to round the result up (Ember of Ashes' Scorch).
    pub round: Option<String>,
    #[serde(default)]
    pub when: Vec<Condition>,
}

/// A named counter: rules `progress` it towards `threshold`, read it and
/// `consume` it, and fire on its `counter_full`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counter {
    pub name: String,
    #[serde(default = "one")]
    pub threshold: f64,
    pub note: Option<String>,
}

fn one() -> f64 {
    1.0
}

/// Behaviour of a part that is known and not yet modelled: the part is
/// reported as partly modelled until the gap is closed.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gap {
    pub part: Part,
    pub note: String,
    #[serde(default)]
    pub sources: Vec<String>,
}

/// A record and where it was read: the file, and its place among the
/// records of its type there, from 1.
#[derive(Debug, Clone)]
pub struct Located<T> {
    pub file: String,
    pub index: usize,
    pub item: T,
}

fn locate<'a, T: 'a>(file: &'a str, items: Vec<T>) -> impl Iterator<Item = Located<T>> + 'a {
    items.into_iter().enumerate().map(move |(i, item)| Located {
        file: file.to_string(),
        index: i + 1,
        item,
    })
}

/// Every record, from every file.
#[derive(Debug, Clone, Default)]
pub struct RuleBase {
    pub rules: Vec<Located<Rule>>,
    pub modifiers: Vec<Located<Modifier>>,
    pub counters: Vec<Located<Counter>>,
    pub gaps: Vec<Located<Gap>>,
}

/// The rules and modifiers a loadout grants.
#[derive(Debug, Clone, Default)]
pub struct Granted<'a> {
    pub rules: Vec<&'a Rule>,
    pub modifiers: Vec<&'a Modifier>,
}

impl RuleBase {
    /// Add one file's records.
    pub fn add(&mut self, file: &str, text: &str) -> anyhow::Result<()> {
        let f: RuleFile = toml::from_str(text).map_err(|e| anyhow::anyhow!("{file}: {e}"))?;
        self.rules.extend(locate(file, f.rule));
        self.modifiers.extend(locate(file, f.modifier));
        self.counters.extend(locate(file, f.counter));
        self.gaps.extend(locate(file, f.gap));
        Ok(())
    }

    /// Parse one file on its own.
    pub fn parse(file: &str, text: &str) -> anyhow::Result<Self> {
        let mut base = Self::default();
        base.add(file, text)?;
        Ok(base)
    }

    fn grants(&self) -> impl Iterator<Item = &Grant> {
        self.rules
            .iter()
            .map(|r| &r.item.granted_by)
            .chain(self.modifiers.iter().map(|m| &m.item.granted_by))
    }

    /// Every part a grant or a gap names.
    #[must_use]
    pub fn parts(&self) -> BTreeSet<&Part> {
        self.grants()
            .flat_map(|g| g.all.iter())
            .chain(self.gaps.iter().map(|g| &g.item.part))
            .collect()
    }

    /// Every amount the records hold, with the grant of its record.
    #[must_use]
    pub fn amounts(&self) -> Vec<(&Grant, &Amount)> {
        let mut out: Vec<(&Grant, &Option<Amount>)> = Vec::new();
        for r in &self.rules {
            let g = &r.item.granted_by;
            out.extend([(g, &r.item.chance), (g, &r.item.cooldown)]);
            for e in &r.item.effect {
                out.extend(
                    [
                        &e.damage,
                        &e.heal,
                        &e.amount,
                        &e.stacks,
                        &e.count,
                        &e.radius,
                        &e.duration,
                        &e.max_duration,
                    ]
                    .map(|a| (g, a)),
                );
            }
        }
        for m in &self.modifiers {
            let g = &m.item.granted_by;
            out.extend([(g, &m.item.add), (g, &m.item.mul), (g, &m.item.set)]);
        }
        out.into_iter()
            .filter_map(|(g, a)| a.as_ref().map(|a| (g, a)))
            .collect()
    }

    /// Every variable the records reference.
    #[must_use]
    pub fn variables(&self) -> BTreeSet<&str> {
        self.amounts()
            .into_iter()
            .filter_map(|(_, a)| a.var_name())
            .collect()
    }

    /// The parts whose records reference each variable.
    #[must_use]
    pub fn variable_users(&self) -> BTreeMap<&str, BTreeSet<String>> {
        let mut out: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
        for (g, a) in self.amounts() {
            if let Some(v) = a.var_name() {
                out.entry(v).or_default().insert(g.to_string());
            }
        }
        out
    }

    /// The parts with behaviour of their own: each grants some record alone.
    /// A part named only in combinations (Daybreak, in Dawn Chorus's rule) is
    /// not modelled by them.
    #[must_use]
    pub fn modelled(&self) -> BTreeSet<&Part> {
        self.grants().filter_map(Grant::sole).collect()
    }

    /// The rules and modifiers a loadout grants: those whose parts it has.
    #[must_use]
    pub fn granted(&self, loadout: &BTreeSet<Part>) -> Granted<'_> {
        Granted {
            rules: self
                .rules
                .iter()
                .filter(|r| r.item.granted_by.by(loadout))
                .map(|r| &r.item)
                .collect(),
            modifiers: self
                .modifiers
                .iter()
                .filter(|m| m.item.granted_by.by(loadout))
                .map(|m| &m.item)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAWN_CHORUS: &str = r#"
        [[rule]]
        granted_by = ["armor:Dawn Chorus", "super:Daybreak"]
        sources = ["compendium:Exotic Armors/Armor/Dawn Chorus"]
        on = { event = "hit", source = "super" }
        effect = { emit = "ignition", attributes = { by = "super" } }

        [[rule]]
        granted_by = "armor:Dawn Chorus"
        sources = ["compendium:Exotic Armors/Armor/Dawn Chorus"]
        on = "ignition_hit"
        effect = [{ energy = "melee", amount = { by_tier = [0.08, 0.15] } }, { spawn = "firesprite" }]

        [[modifier]]
        granted_by = "armor:Dawn Chorus"
        sources = ["compendium:Exotic Armors/Armor/Dawn Chorus"]
        damage = "ignition"
        mul = 2.0
    "#;

    #[test]
    fn records_parse_with_short_and_long_forms() {
        let base = RuleBase::parse("t.toml", DAWN_CHORUS).unwrap();
        let [daybreak, chorus] = [&base.rules[0], &base.rules[1]];
        assert_eq!(
            daybreak.item.granted_by.all,
            [
                Part::new("armor", "Dawn Chorus"),
                Part::new("super", "Daybreak")
            ]
        );
        assert_eq!(daybreak.item.granted_by.sole(), None);
        assert_eq!(daybreak.item.on.attributes["source"], "super".into());
        assert_eq!(chorus.index, 2);
        assert_eq!(chorus.item.on.event, "ignition_hit");
        assert_eq!(chorus.item.effect.len(), 2);
        assert_eq!(chorus.item.confidence, "documented");
        assert_eq!(base.modifiers[0].item.mul, Some(Amount::Number(2.0)));
    }

    #[test]
    fn a_loadout_grants_what_all_its_parts_grant() {
        let base = RuleBase::parse("t.toml", DAWN_CHORUS).unwrap();
        let chorus = BTreeSet::from([Part::new("armor", "Dawn Chorus")]);
        let g = base.granted(&chorus);
        assert_eq!((g.rules.len(), g.modifiers.len()), (1, 1));
        let mut with_daybreak = chorus.clone();
        with_daybreak.insert(Part::new("super", "Daybreak"));
        assert_eq!(base.granted(&with_daybreak).rules.len(), 2);
        let daybreak = BTreeSet::from([Part::new("super", "Daybreak")]);
        assert!(base.granted(&daybreak).rules.is_empty());
        // Daybreak is named, but only Dawn Chorus has behaviour of its own.
        assert_eq!(base.parts().len(), 2);
        assert_eq!(
            base.modelled().into_iter().collect::<Vec<_>>(),
            [&Part::new("armor", "Dawn Chorus")]
        );
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
    fn malformed_records_are_rejected() {
        let rule = |extra: &str| {
            format!(
                "[[rule]]\ngranted_by = \"fragment:x\"\nsources = []\non = \"kill\"\neffect = {{ apply = \"scorch\" }}\n{extra}"
            )
        };
        assert!(RuleBase::parse("t", &rule("")).is_ok());
        // Unknown fields, the loadout as a condition, a part without its kind,
        // a rule without sources.
        assert!(RuleBase::parse("t", &rule("colour = \"red\"")).is_err());
        assert!(RuleBase::parse("t", &rule("when = [{ equipped = \"y\" }]")).is_err());
        assert!(RuleBase::parse("t", &rule("").replace("fragment:x", "Ember of Char")).is_err());
        assert!(RuleBase::parse("t", &rule("").replace("sources = []\n", "")).is_err());
        assert!(
            RuleBase::parse("t", "[[part]]\nname = \"x\"")
                .unwrap_err()
                .to_string()
                .contains("unknown field")
        );
    }
}
