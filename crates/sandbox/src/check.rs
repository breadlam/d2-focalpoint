//! Checking cards against the ontology, and their coverage of the parts
//! recorded builds use.

use std::collections::{BTreeMap, BTreeSet};

use crate::card::{Amount, CONFIDENCE, Card, Condition, Effect, PART_KINDS, Rule, Trigger};
use crate::ontology::Ontology;

/// A card and the file it came from.
#[derive(Debug, Clone)]
pub struct Located {
    pub file: String,
    pub card: Card,
}

/// Something wrong with a card.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Problem {
    pub file: String,
    pub card: String,
    pub message: String,
}

/// What the checks found.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub problems: Vec<Problem>,
    /// Cards, rules and modifiers whose only sources are Clarity's.
    pub clarity_only: Vec<String>,
}

const SIZES: [&str; 3] = ["small", "medium", "large"];

struct Checker<'a> {
    o: &'a Ontology,
    file: &'a str,
    card: &'a Card,
    out: &'a mut Vec<Problem>,
}

impl Checker<'_> {
    fn problem(&mut self, message: impl Into<String>) {
        self.out.push(Problem {
            file: self.file.to_string(),
            card: format!("{} ({})", self.card.name, self.card.kind),
            message: message.into(),
        });
    }

    fn in_list(&mut self, what: &str, value: &str, list: &[String]) {
        if !list.iter().any(|x| x == value) {
            self.problem(format!("{what} {value:?} is not in the vocabulary"));
        }
    }

    fn state(&mut self, what: &str, name: &str, kinds: &[&str]) {
        match self.o.states.get(name) {
            None => self.problem(format!("{what}: state {name:?} is not in the ontology")),
            Some(s) if !kinds.is_empty() && !kinds.contains(&s.kind.as_str()) => {
                self.problem(format!(
                    "{what}: {name:?} is a {}, not a {}",
                    s.kind,
                    kinds.join(" or ")
                ));
            }
            Some(_) => {}
        }
    }

    fn sources(&mut self, sources: &[String]) {
        for s in sources {
            match s.split_once(':') {
                Some((family, rest)) if !rest.is_empty() => {
                    if !self
                        .o
                        .vocabulary
                        .source_families
                        .iter()
                        .any(|f| f == family)
                    {
                        self.problem(format!("source {s:?}: family {family:?} is not known"));
                    }
                }
                _ => self.problem(format!("source {s:?} is not `family:reference`")),
            }
        }
    }

    fn confidence(&mut self, c: Option<&String>) {
        if let Some(c) = c
            && !CONFIDENCE.contains(&c.as_str())
        {
            self.problem(format!("confidence {c:?} is not one of {CONFIDENCE:?}"));
        }
    }

    /// An attribute value of a trigger or an emitted event.
    fn attribute(&mut self, event: &str, name: &str, value: &toml::Value) {
        let v = &self.o.vocabulary;
        let text = value.as_str();
        let list: Option<Vec<String>> = match name {
            "source" | "by" => Some(v.sources.clone()),
            "element" => Some(
                v.elements
                    .iter()
                    .cloned()
                    .chain(["prismatic".to_string()])
                    .collect(),
            ),
            "rank" => Some(v.ranks.clone()),
            "ability" => Some(v.abilities.clone()),
            "ammo" => Some(v.ammo.clone()),
            "kind" => Some(v.movement.clone()),
            "champion" => Some(v.champions.clone()),
            "size" => Some(SIZES.iter().map(|s| (*s).to_string()).collect()),
            "of" => Some(vec!["enemy".into(), "crystal".into()]),
            "from" => Some(vec!["combatant".into(), "guardian".into(), "any".into()]),
            _ => None,
        };
        if let Some(list) = list {
            match text {
                Some(t) => self.in_list(&format!("{event}.{name}"), t, &list),
                None => self.problem(format!("{event}.{name} must be a name, not {value}")),
            }
            return;
        }
        match name {
            "state" | "target_has" | "object" => match text {
                Some(t) => self.state(&format!("{event}.{name}"), t, &[]),
                None => self.problem(format!("{event}.{name} must be a state name")),
            },
            "tier" | "interval" | "multikill" => {
                if !(value.is_integer() || value.is_float()) {
                    self.problem(format!("{event}.{name} must be a number"));
                }
            }
            "precision" | "powered" | "origin" | "fulmination" | "melee" => {
                if !value.is_bool() {
                    self.problem(format!("{event}.{name} must be true or false"));
                }
            }
            "counter" => {
                if !text.is_some_and(|c| self.card.counter.iter().any(|d| d.name == c)) {
                    self.problem(format!("{event}.counter names no counter of this card"));
                }
            }
            _ => self.problem(format!(
                "{event}: attribute {name:?} is not checked; add it to the checker"
            )),
        }
    }

    fn trigger(&mut self, t: &Trigger) {
        let Some(def) = self.o.events.get(&t.event) else {
            self.problem(format!("event {:?} is not in the ontology", t.event));
            return;
        };
        let allowed = def.attributes.clone();
        for (k, v) in &t.attributes {
            if allowed.iter().any(|a| a == k) {
                self.attribute(&t.event, k, v);
            } else {
                self.problem(format!(
                    "event {:?} has no attribute {k:?} (it has {allowed:?})",
                    t.event
                ));
            }
        }
    }

    fn condition(&mut self, c: &Condition) {
        let set = [
            ("has", c.has.is_some()),
            ("lacks", c.lacks.is_some()),
            ("equipped", c.equipped.is_some()),
            ("stacks", c.stacks.is_some()),
            ("movement", c.movement.is_some()),
            ("in_super", c.in_super.is_some()),
            ("health_below", c.health_below.is_some()),
            ("enemies_within", c.enemies_within.is_some()),
            ("target_has", c.target_has.is_some()),
            ("target_rank", c.target_rank.is_some()),
            ("target_rank_not", c.target_rank_not.is_some()),
        ];
        let n = set.iter().filter(|(_, s)| *s).count();
        if n != 1 {
            self.problem(format!(
                "a condition sets {n} of its kinds; it must set exactly one"
            ));
        }
        if let Some(s) = &c.has {
            self.state("has", s, &["buff", "resource"]);
        }
        if let Some(s) = &c.lacks {
            self.state("lacks", s, &["buff"]);
        }
        if let Some(s) = &c.stacks {
            if !self.card.counter.iter().any(|d| &d.name == s) {
                self.state("stacks", s, &[]);
            }
            if c.min.is_none() {
                self.problem("a `stacks` condition needs `min`");
            }
        }
        if let Some(m) = &c.movement {
            let list = self.o.vocabulary.movement.clone();
            self.in_list("movement", m, &list);
        }
        if c.enemies_within.is_some() && c.min.is_none() {
            self.problem("an `enemies_within` condition needs `min`");
        }
        if let Some(s) = &c.target_has {
            self.state("target_has", s, &["debuff", "buff"]);
        }
        for r in c
            .target_rank
            .iter()
            .chain(c.target_rank_not.iter())
            .flatten()
        {
            let list = self.o.vocabulary.ranks.clone();
            self.in_list("rank", r, &list);
        }
    }

    fn amount(&mut self, what: &str, a: &Amount) {
        let Amount::Table(t) = a else { return };
        let n = [
            t.by_tier.is_some(),
            t.by_rank.is_some(),
            t.by_counter.is_some(),
            t.by_count.is_some(),
        ]
        .iter()
        .filter(|s| **s)
        .count();
        if n != 1 {
            self.problem(format!("{what}: a table sets {n} of by_tier, by_rank, by_counter, by_count; it must set one"));
        }
        if let Some(r) = &t.by_rank {
            for k in r.keys() {
                let list = self.o.vocabulary.ranks.clone();
                self.in_list(&format!("{what} rank"), k, &list);
            }
        }
        if t.by_counter.is_some() {
            match &t.counter {
                Some(c) if self.card.counter.iter().any(|d| &d.name == c) => {}
                _ => self.problem(format!(
                    "{what}: by_counter needs `counter`, one of this card's counters"
                )),
            }
        }
    }

    fn effect(&mut self, e: &Effect) {
        let primaries = e.primaries();
        if primaries.len() != 1 {
            self.problem(format!(
                "an effect sets {primaries:?}; it must set exactly one primary field"
            ));
            return;
        }
        let v = self.o.vocabulary.clone();
        if let Some(s) = &e.apply {
            self.state("apply", s, &["buff", "debuff"]);
        }
        for a in [&e.energy, &e.regen, &e.charges].into_iter().flatten() {
            self.in_list("ability", a, &v.abilities);
        }
        if let Some(s) = &e.spawn {
            self.state("spawn", s, &["object"]);
        }
        if let Some(ev) = &e.emit {
            let t = Trigger {
                event: ev.clone(),
                attributes: e.attributes.clone().unwrap_or_default(),
            };
            self.trigger(&t);
        } else if e.attributes.is_some() {
            self.problem("`attributes` only belongs to an `emit` effect");
        }
        for c in [&e.consume, &e.progress].into_iter().flatten() {
            let own = self.card.counter.iter().any(|d| &d.name == c);
            if e.progress.is_some() && !own {
                self.problem(format!(
                    "progress: {c:?} is not one of this card's counters"
                ));
            } else if !own && !self.o.states.contains_key(c) {
                self.problem(format!(
                    "consume: {c:?} is neither a state nor one of this card's counters"
                ));
            }
        }
        for (name, a) in [
            ("damage", &e.damage),
            ("heal", &e.heal),
            ("amount", &e.amount),
            ("stacks", &e.stacks),
            ("count", &e.count),
        ] {
            if let Some(a) = a {
                self.amount(name, a);
            }
        }
        if let Some(t) = &e.target {
            self.in_list("target", t, &v.targets);
        }
        if let Some(x) = &e.except
            && x != "origin"
        {
            self.problem(format!("except {x:?}: only `origin` is known"));
        }
        if let Some(el) = &e.element {
            self.in_list("element", el, &v.elements);
        }
        for t in &e.tags {
            self.in_list("damage tag", t, &v.damage_tags);
        }
        if let Some(s) = &e.source {
            self.in_list("source", s, &v.sources);
        }
        if let Some(s) = &e.size
            && !SIZES.contains(&s.as_str())
        {
            self.problem(format!("size {s:?} is not small, medium or large"));
        }
        if let Some(sizes) = &e.sizes {
            for s in sizes.keys() {
                if !SIZES.contains(&s.as_str()) {
                    self.problem(format!("size {s:?} is not small, medium or large"));
                }
            }
        }
        if let Some(f) = e.falloff
            && !(0.0..=1.0).contains(&f)
        {
            self.problem(format!("falloff {f} is not a fraction"));
        }
    }

    fn rule(&mut self, r: &Rule) {
        self.trigger(&r.on);
        for c in &r.when {
            self.condition(c);
        }
        if r.effect.is_empty() {
            self.problem("a rule has no effect");
        }
        for e in &r.effect {
            self.effect(e);
        }
        if let Some(p) = r.chance
            && !(0.0..=1.0).contains(&p)
        {
            self.problem(format!("chance {p} is not a probability"));
        }
        if !r.with.is_empty() && r.effect.len() != 1 {
            self.problem("`with` variants need a rule with a single effect");
        }
        for w in &r.with {
            if w.part.trim().is_empty() {
                self.problem("a `with` variant names no part");
            }
            if let Some(e) = r.effect.first() {
                match e.with(&w.overrides) {
                    Ok(merged) => self.effect(&merged),
                    Err(err) => self.problem(format!("with {:?}: {err}", w.part)),
                }
            }
        }
        self.confidence(r.confidence.as_ref());
        self.sources(&r.sources);
    }
}

/// Check every card against the ontology.
#[must_use]
pub fn check(o: &Ontology, cards: &[Located]) -> Report {
    let mut report = Report::default();
    let mut seen: BTreeMap<(String, String), String> = BTreeMap::new();
    for Located { file, card } in cards {
        let mut c = Checker {
            o,
            file,
            card,
            out: &mut report.problems,
        };
        if !PART_KINDS.contains(&card.kind.as_str()) {
            c.problem(format!("kind {:?} is not one of {PART_KINDS:?}", card.kind));
        }
        if let Some(first) = seen.insert((card.name.clone(), card.kind.clone()), file.clone()) {
            c.problem(format!("also carded in {first}"));
        }
        if card.sources.is_empty() {
            c.problem("a card needs at least one source");
        }
        c.sources(&card.sources);
        c.confidence(Some(&card.confidence));
        let mut counters = BTreeSet::new();
        for d in &card.counter {
            if !counters.insert(&d.name) {
                c.problem(format!("counter {:?} defined twice", d.name));
            }
        }
        for r in &card.rule {
            c.rule(r);
        }
        for m in &card.modify {
            let targets = [m.param.is_some(), m.damage.is_some(), m.stat.is_some()]
                .iter()
                .filter(|s| **s)
                .count();
            if targets != 1 {
                c.problem("a modifier sets one of `param`, `damage`, `stat`");
            }
            let ops = [m.add.is_some(), m.mul.is_some(), m.set.is_some()]
                .iter()
                .filter(|s| **s)
                .count();
            if ops != 1 {
                c.problem("a modifier sets one of `add`, `mul`, `set`");
            }
            if let Some(p) = &m.param
                && !o.has_param(p)
            {
                c.problem(format!("parameter {p:?} is not in the ontology"));
            }
            if let Some(t) = &m.damage {
                let list = o.vocabulary.damage_tags.clone();
                c.in_list("damage tag", t, &list);
            }
            if let Some(s) = &m.stat {
                let list = o.vocabulary.stats.clone();
                c.in_list("stat", s, &list);
            }
            if let Some(el) = &m.element {
                if m.damage.is_none() {
                    c.problem("`element` restricts a damage modifier; it needs `damage`");
                }
                let list = o.vocabulary.elements.clone();
                c.in_list("element", el, &list);
            }
            if let Some(cat) = &m.category {
                let list = o.vocabulary.damage_categories.clone();
                c.in_list("damage category", cat, &list);
            }
            if let Some(r) = &m.round
                && r != "up"
            {
                c.problem(format!("round {r:?}: only `up` is known"));
            }
            for w in &m.when {
                c.condition(w);
            }
            c.confidence(m.confidence.as_ref());
            c.sources(&m.sources);
        }
        let only_clarity =
            |s: &[String]| !s.is_empty() && s.iter().all(|x| x.starts_with("clarity:"));
        if only_clarity(&card.sources)
            && card
                .rule
                .iter()
                .all(|r| r.sources.is_empty() || only_clarity(&r.sources))
            && card
                .modify
                .iter()
                .all(|m| m.sources.is_empty() || only_clarity(&m.sources))
        {
            report
                .clarity_only
                .push(format!("{} ({})", card.name, card.kind));
        }
    }
    report.problems.sort();
    report
}

/// How much of the recorded part use the cards cover, for one kind.
#[derive(Debug, Clone, PartialEq)]
pub struct Coverage {
    pub kind: String,
    pub parts: usize,
    pub carded_parts: usize,
    pub uses: i64,
    pub carded_uses: i64,
}

/// A part recorded builds use that no card describes.
#[derive(Debug, Clone, PartialEq)]
pub struct Uncarded {
    pub kind: String,
    pub name: String,
    pub uses: i64,
}

/// Coverage of the recorded builds' parts by the cards, by kind, and the
/// most-used parts without a card (the drafting worklist); cards that
/// name no manifest item of their kind are listed too.
pub fn coverage(
    db: &focalpoint_db::Database,
    cards: &[Located],
) -> anyhow::Result<(Vec<Coverage>, Vec<Uncarded>, Vec<String>)> {
    let kinds = PART_KINDS
        .iter()
        .map(|k| format!("'{k}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT bc.kind, coalesce(i.name, p.name, bc.component_hash::VARCHAR) AS name, count(*) AS uses
         FROM build_component bc
         LEFT JOIN item i ON i.item_hash = bc.component_hash
         LEFT JOIN sandbox_perk p ON p.perk_hash = bc.component_hash
         WHERE bc.kind IN ({kinds})
           AND (bc.kind NOT IN ('armor', 'weapon') OR i.is_exotic)
         GROUP BY ALL"
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let rows: Vec<(String, String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<duckdb::Result<_>>()?;
    let carded: BTreeSet<(&str, &str)> = cards
        .iter()
        .map(|c| (c.card.kind.as_str(), c.card.name.as_str()))
        .collect();
    let mut by_kind: BTreeMap<String, Coverage> = BTreeMap::new();
    let mut uncarded = Vec::new();
    let mut used: BTreeSet<(String, String)> = BTreeSet::new();
    for (kind, name, uses) in rows {
        let has = carded.contains(&(kind.as_str(), name.as_str()));
        let c = by_kind.entry(kind.clone()).or_insert(Coverage {
            kind: kind.clone(),
            parts: 0,
            carded_parts: 0,
            uses: 0,
            carded_uses: 0,
        });
        c.parts += 1;
        c.uses += uses;
        if has {
            c.carded_parts += 1;
            c.carded_uses += uses;
        } else {
            uncarded.push(Uncarded {
                kind: kind.clone(),
                name: name.clone(),
                uses,
            });
        }
        used.insert((kind, name));
    }
    uncarded.sort_by(|a, b| b.uses.cmp(&a.uses).then_with(|| a.name.cmp(&b.name)));
    // Cards naming nothing: no manifest item (or set bonus perk) of the name.
    let mut stmt = db
        .conn()
        .prepare("SELECT name FROM item UNION SELECT name FROM sandbox_perk")?;
    let names: BTreeSet<String> = stmt
        .query_map([], |r| r.get::<_, Option<String>>(0))?
        .filter_map(|r| r.ok().flatten())
        .collect();
    let mut unresolved: Vec<String> = cards
        .iter()
        .filter(|c| !names.contains(&c.card.name))
        .map(|c| format!("{} ({})", c.card.name, c.card.kind))
        .collect();
    // Parts named inside rules and modifiers: `equipped` and `with`.
    for c in cards {
        for name in referenced_parts(&c.card) {
            if !names.contains(name) {
                unresolved.push(format!("{name} (named by {})", c.card.name));
            }
        }
    }
    Ok((by_kind.into_values().collect(), uncarded, unresolved))
}

/// The parts a card's rules and modifiers name (`equipped`, `with`).
fn referenced_parts(card: &Card) -> BTreeSet<&str> {
    let mut out = BTreeSet::new();
    let conditions = card
        .rule
        .iter()
        .flat_map(|r| r.when.iter())
        .chain(card.modify.iter().flat_map(|m| m.when.iter()));
    out.extend(conditions.filter_map(|c| c.equipped.as_deref()));
    out.extend(
        card.rule
            .iter()
            .flat_map(|r| r.with.iter().map(|w| w.part.as_str())),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::parse;

    fn located(text: &str) -> Vec<Located> {
        parse(text)
            .unwrap()
            .into_iter()
            .map(|card| Located {
                file: "t.toml".into(),
                card,
            })
            .collect()
    }

    #[test]
    fn good_cards_pass_and_bad_names_fail() {
        let o = Ontology::builtin().unwrap();
        let good = located(
            r#"
            [[part]]
            name = "Ember of Char"
            kind = "fragment"
            sources = ["compendium:Solar/Fragments/Char"]
            [[part.rule]]
            on = { event = "ignition_hit", origin = false }
            effect = { apply = "scorch", stacks = 40, target = "target" }
            "#,
        );
        let r = check(&o, &good);
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let bad = located(
            r#"
            [[part]]
            name = "Nonsense"
            kind = "doohickey"
            sources = ["wiki:x", "clarity:1"]
            [[part.rule]]
            on = { event = "ignite", origin = false }
            effect = [{ apply = "scorchh" }, { energy = "melee", spawn = "firesprite" }]
            [[part.modify]]
            param = "ignition.colour"
            mul = 2
            "#,
        );
        let msgs: Vec<String> = check(&o, &bad)
            .problems
            .into_iter()
            .map(|p| p.message)
            .collect();
        for needle in [
            "kind \"doohickey\"",
            "family \"wiki\"",
            "event \"ignite\"",
            "state \"scorchh\"",
            "exactly one primary",
            "parameter \"ignition.colour\"",
        ] {
            assert!(
                msgs.iter().any(|m| m.contains(needle)),
                "missing {needle}: {msgs:?}"
            );
        }
    }

    #[test]
    fn variants_are_checked_merged_and_clarity_only_cards_reported() {
        let o = Ontology::builtin().unwrap();
        let cards = located(
            r#"
            [[part]]
            name = "Whisper of Hunger"
            kind = "fragment"
            sources = ["clarity:2483898431"]
            [[part.rule]]
            on = { event = "pickup", object = "stasis_shard" }
            effect = { energy = "melee", amount = 0.1 }
            with = [{ part = "X", energy = "elbow" }]
            "#,
        );
        let r = check(&o, &cards);
        assert!(
            r.problems.iter().any(|p| p.message.contains("\"elbow\"")),
            "{:?}",
            r.problems
        );
        assert_eq!(r.clarity_only, ["Whisper of Hunger (fragment)"]);
    }
}
