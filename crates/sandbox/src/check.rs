//! Checking the rule base against the ontology, and its coverage of the
//! parts recorded builds use.

use std::collections::{BTreeMap, BTreeSet};

use crate::ontology::Ontology;
use crate::rules::{
    Amount, CONFIDENCE, Condition, Effect, Grant, Modifier, PART_KINDS, Part, Rule, RuleBase,
    Trigger,
};

/// Something wrong with a record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Problem {
    pub file: String,
    /// The record: `rule 2 (fragment:Ember of Char)`.
    pub record: String,
    pub message: String,
}

/// What the checks found.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub problems: Vec<Problem>,
    /// Parts whose own records all rest on Clarity alone.
    pub clarity_only: Vec<String>,
}

const SIZES: [&str; 3] = ["small", "medium", "large"];

struct Checker<'a> {
    o: &'a Ontology,
    counters: &'a BTreeSet<&'a str>,
    file: &'a str,
    record: String,
    out: Vec<Problem>,
}

impl<'a> Checker<'a> {
    fn new(
        o: &'a Ontology,
        counters: &'a BTreeSet<&'a str>,
        file: &'a str,
        record: String,
    ) -> Self {
        Self {
            o,
            counters,
            file,
            record,
            out: Vec::new(),
        }
    }

    fn problem(&mut self, message: impl Into<String>) {
        self.out.push(Problem {
            file: self.file.to_string(),
            record: self.record.clone(),
            message: message.into(),
        });
    }

    fn counter(&self, name: &str) -> bool {
        self.counters.contains(name)
    }

    fn part(&mut self, p: &Part) {
        if !PART_KINDS.contains(&p.kind.as_str()) {
            self.problem(format!(
                "part {p}: kind {:?} is not one of {PART_KINDS:?}",
                p.kind
            ));
        }
        if p.name.trim() != p.name {
            self.problem(format!("part {p}: the name has spaces around it"));
        }
    }

    fn grant(&mut self, g: &Grant) {
        if g.all.is_empty() {
            self.problem("granted by no part");
        }
        for p in &g.all {
            self.part(p);
        }
        if g.all.iter().collect::<BTreeSet<_>>().len() < g.all.len() {
            self.problem(format!("{g} names a part twice"));
        }
    }

    /// A record's provenance: at least one source, of a known family, and
    /// a known confidence.
    fn provenance(&mut self, sources: &[String], confidence: &str) {
        if sources.is_empty() {
            self.problem("a record needs at least one source");
        }
        self.sources(sources);
        if !CONFIDENCE.contains(&confidence) {
            self.problem(format!(
                "confidence {confidence:?} is not one of {CONFIDENCE:?}"
            ));
        }
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
                if !text.is_some_and(|c| self.counter(c)) {
                    self.problem(format!("{event}.counter names no declared counter"));
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
            if !self.counter(s) {
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
                Some(c) if self.counter(c) => {}
                _ => self.problem(format!(
                    "{what}: by_counter needs `counter`, a declared counter"
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
            let counter = self.counter(c);
            if e.progress.is_some() && !counter {
                self.problem(format!("progress: {c:?} is not a declared counter"));
            } else if !counter && !self.o.states.contains_key(c) {
                self.problem(format!(
                    "consume: {c:?} is neither a state nor a declared counter"
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
        if let Some(s) = &e.scaling {
            if e.energy.is_none() {
                self.problem("`scaling` belongs to an `energy` effect");
            }
            if crate::economy::Scaling::parse(Some(s)).is_none() {
                self.problem(format!("scaling {s:?} is not chunk, stat or fixed"));
            }
        }
    }

    fn rule(&mut self, r: &Rule) {
        self.grant(&r.granted_by);
        self.provenance(&r.sources, &r.confidence);
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
    }

    fn modifier(&mut self, m: &Modifier) {
        self.grant(&m.granted_by);
        self.provenance(&m.sources, &m.confidence);
        let targets = [m.param.is_some(), m.damage.is_some(), m.stat.is_some()]
            .iter()
            .filter(|s| **s)
            .count();
        if targets != 1 {
            self.problem("a modifier sets one of `param`, `damage`, `stat`");
        }
        let ops = [m.add.is_some(), m.mul.is_some(), m.set.is_some()]
            .iter()
            .filter(|s| **s)
            .count();
        if ops != 1 {
            self.problem("a modifier sets one of `add`, `mul`, `set`");
        }
        let v = self.o.vocabulary.clone();
        if let Some(p) = &m.param
            && !self.o.has_param(p)
        {
            self.problem(format!("parameter {p:?} is not in the ontology"));
        }
        if let Some(t) = &m.damage {
            self.in_list("damage tag", t, &v.damage_tags);
        }
        if let Some(s) = &m.stat {
            self.in_list("stat", s, &v.stats);
        }
        if let Some(el) = &m.element {
            if m.damage.is_none() {
                self.problem("`element` restricts a damage modifier; it needs `damage`");
            }
            self.in_list("element", el, &v.elements);
        }
        if let Some(cat) = &m.category {
            self.in_list("damage category", cat, &v.damage_categories);
        }
        if let Some(r) = &m.round
            && r != "up"
        {
            self.problem(format!("round {r:?}: only `up` is known"));
        }
        for w in &m.when {
            self.condition(w);
        }
    }
}

/// Check the rule base against the ontology.
#[must_use]
pub fn check(o: &Ontology, base: &RuleBase) -> Report {
    let mut report = Report::default();
    let counters: BTreeSet<&str> = base.counters.iter().map(|c| c.item.name.as_str()).collect();
    let checker = |file, record| Checker::new(o, &counters, file, record);
    let mut problems = Vec::new();
    for r in &base.rules {
        let mut c = checker(&r.file, format!("rule {} ({})", r.index, r.item.granted_by));
        c.rule(&r.item);
        problems.append(&mut c.out);
    }
    for m in &base.modifiers {
        let mut c = checker(
            &m.file,
            format!("modifier {} ({})", m.index, m.item.granted_by),
        );
        c.modifier(&m.item);
        problems.append(&mut c.out);
    }
    let progressed: BTreeSet<&str> = base
        .rules
        .iter()
        .flat_map(|r| r.item.effect.iter())
        .filter_map(|e| e.progress.as_deref())
        .collect();
    let mut declared = BTreeSet::new();
    for k in &base.counters {
        let name = k.item.name.as_str();
        let mut c = checker(&k.file, format!("counter {name:?}"));
        if !declared.insert(name) {
            c.problem("declared twice");
        }
        if k.item.threshold <= 0.0 {
            c.problem("its threshold must be positive");
        }
        if !progressed.contains(name) {
            c.problem("no rule progresses it");
        }
        problems.append(&mut c.out);
    }
    for g in &base.gaps {
        let mut c = checker(&g.file, format!("gap {} ({})", g.index, g.item.part));
        c.part(&g.item.part);
        if g.item.note.trim().is_empty() {
            c.problem("a gap needs a note saying what is not modelled");
        }
        c.sources(&g.item.sources);
        problems.append(&mut c.out);
    }
    problems.sort();
    report.problems = problems;
    // A part rests on Clarity alone when every record naming it does.
    let only_clarity = |s: &[String]| !s.is_empty() && s.iter().all(|x| x.starts_with("clarity:"));
    let mut by_part: BTreeMap<&Part, bool> = BTreeMap::new();
    let records = base
        .rules
        .iter()
        .map(|r| (&r.item.granted_by, &r.item.sources))
        .chain(
            base.modifiers
                .iter()
                .map(|m| (&m.item.granted_by, &m.item.sources)),
        );
    for (grant, sources) in records {
        for p in &grant.all {
            *by_part.entry(p).or_insert(true) &= only_clarity(sources);
        }
    }
    report.clarity_only = by_part
        .into_iter()
        .filter(|(_, only)| *only)
        .map(|(p, _)| p.to_string())
        .collect();
    report
}

/// How much of the recorded part use the rule base models, for one kind.
#[derive(Debug, Clone, PartialEq)]
pub struct Coverage {
    pub kind: String,
    pub parts: usize,
    /// Parts granting some record alone.
    pub modelled: usize,
    /// Modelled parts with a gap.
    pub partial: usize,
    pub uses: i64,
    pub modelled_uses: i64,
}

/// A part recorded builds use that the rule base does not model.
#[derive(Debug, Clone, PartialEq)]
pub struct Unmodelled {
    pub kind: String,
    pub name: String,
    pub uses: i64,
}

/// Coverage of the recorded builds' parts by the rule base, by kind; the
/// most-used parts not modelled (the drafting worklist); and the parts the
/// records name that no manifest item has the name of.
pub fn coverage(
    db: &focalpoint_db::Database,
    base: &RuleBase,
) -> anyhow::Result<(Vec<Coverage>, Vec<Unmodelled>, Vec<String>)> {
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
    let key = |p: &Part| (p.kind.clone(), p.name.clone());
    let modelled: BTreeSet<(String, String)> = base.modelled().into_iter().map(key).collect();
    let partial: BTreeSet<(String, String)> = base.gaps.iter().map(|g| key(&g.item.part)).collect();
    let mut by_kind: BTreeMap<String, Coverage> = BTreeMap::new();
    let mut unmodelled = Vec::new();
    for (kind, name, uses) in rows {
        let part = (kind, name);
        let c = by_kind.entry(part.0.clone()).or_insert(Coverage {
            kind: part.0.clone(),
            parts: 0,
            modelled: 0,
            partial: 0,
            uses: 0,
            modelled_uses: 0,
        });
        c.parts += 1;
        c.uses += uses;
        if modelled.contains(&part) {
            c.modelled += 1;
            c.modelled_uses += uses;
            c.partial += usize::from(partial.contains(&part));
        } else {
            let (kind, name) = part;
            unmodelled.push(Unmodelled { kind, name, uses });
        }
    }
    unmodelled.sort_by(|a, b| b.uses.cmp(&a.uses).then_with(|| a.name.cmp(&b.name)));
    let mut stmt = db
        .conn()
        .prepare("SELECT name FROM item UNION SELECT name FROM sandbox_perk")?;
    let names: BTreeSet<String> = stmt
        .query_map([], |r| r.get::<_, Option<String>>(0))?
        .filter_map(|r| r.ok().flatten())
        .collect();
    let unresolved = base
        .parts()
        .into_iter()
        .filter(|p| !names.contains(&p.name))
        .map(ToString::to_string)
        .collect();
    Ok((by_kind.into_values().collect(), unmodelled, unresolved))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(text: &str) -> (Vec<String>, Report) {
        let o = Ontology::builtin().unwrap();
        let r = check(&o, &RuleBase::parse("t.toml", text).unwrap());
        (r.problems.iter().map(|p| p.message.clone()).collect(), r)
    }

    fn assert_has(msgs: &[String], needles: &[&str]) {
        for needle in needles {
            assert!(
                msgs.iter().any(|m| m.contains(needle)),
                "missing {needle}: {msgs:?}"
            );
        }
    }

    #[test]
    fn good_records_pass_and_bad_names_fail() {
        let (msgs, _) = messages(
            r#"
            [[rule]]
            granted_by = "fragment:Ember of Char"
            sources = ["compendium:Solar/Fragments/Char"]
            on = { event = "ignition_hit", origin = false }
            effect = { apply = "scorch", stacks = 40, target = "target" }
            "#,
        );
        assert!(msgs.is_empty(), "{msgs:?}");
        let (msgs, _) = messages(
            r#"
            [[rule]]
            granted_by = ["doohickey:Nonsense", "fragment:Ember of Char", "fragment:Ember of Char"]
            sources = ["wiki:x", "clarity:1"]
            confidence = "certain"
            on = { event = "ignite", origin = false }
            effect = [{ apply = "scorchh" }, { energy = "melee", spawn = "firesprite" }]

            [[modifier]]
            granted_by = "fragment:Ember of Char"
            sources = []
            param = "ignition.colour"
            mul = 2
            "#,
        );
        assert_has(
            &msgs,
            &[
                "kind \"doohickey\"",
                "names a part twice",
                "family \"wiki\"",
                "confidence \"certain\"",
                "event \"ignite\"",
                "state \"scorchh\"",
                "exactly one primary",
                "parameter \"ignition.colour\"",
                "at least one source",
            ],
        );
    }

    #[test]
    fn counters_and_gaps_are_checked_and_clarity_only_parts_reported() {
        let (msgs, r) = messages(
            r#"
            [[counter]]
            name = "shards"
            threshold = 15

            [[counter]]
            name = "shards"

            [[counter]]
            name = "idle"

            [[rule]]
            granted_by = "fragment:Whisper of Hunger"
            sources = ["clarity:2483898431"]
            on = { event = "pickup", object = "stasis_shard" }
            effect = { progress = "shards", amount = 1 }

            [[rule]]
            granted_by = "fragment:Whisper of Hunger"
            sources = ["clarity:2483898431"]
            on = "kill"
            when = [{ stacks = "nothing", min = 1 }]
            effect = [{ consume = "shards" }, { progress = "unknown" }]

            [[gap]]
            part = "fragment:Whisper of Hunger"
            note = " "
            "#,
        );
        assert_has(
            &msgs,
            &[
                "declared twice",
                "no rule progresses it",
                "state \"nothing\"",
                "\"unknown\" is not a declared counter",
                "needs a note",
            ],
        );
        assert_eq!(r.clarity_only, ["fragment:Whisper of Hunger"]);
        assert!(
            r.problems.iter().any(|p| p.record == "counter \"idle\""),
            "{:?}",
            r.problems
        );
    }
}
