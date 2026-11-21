//! What a documented build part needs and does, read from its text: the
//! **triggers** that set it off (a kill, a hit, an ability cast, a pickup,
//! …), the **payoffs** it gives (damage, ability energy, survival, ammo,
//! …), and the game **states** it produces or depends on (Radiant, Weaken,
//! Woven Mail, Orbs of Power, …) — the links a synergy evaluator credits
//! within a build, and the triggers a context makes plentiful or scarce
//! (`docs/effects.md`).
//!
//! Texts are split into **conditions** and **effects**. A line ending in a
//! colon is a condition for the lines under it (to the next blank line):
//! "On Weapon Kill:", "While Amplified:". In a sentence, the part before its
//! main verb is its condition and the rest its effect: "Weapon Kills |
//! grant 10% Grenade Ability Energy", "Picking up an Orb of Power | grants
//! Devour". Triggers are read from conditions (and from prepositional
//! phrases such as "from Grenade Kills" in effects), payoffs from effects; a
//! state is produced when an effect grants, inflicts, applies, creates or
//! spawns it, and depended on when a condition names it.
//!
//! Every tag comes with the sentence it was read from, so the reading can
//! be audited; the vocabulary is open (dotted tags), like context features.

use std::sync::LazyLock;

use regex::Regex;

/// What a tag records about a part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// What sets it off.
    Trigger,
    /// What it gives.
    Payoff,
    /// A state it puts on the player, allies or enemies.
    Produces,
    /// A state it needs.
    Depends,
}

impl Role {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Trigger => "trigger",
            Role::Payoff => "payoff",
            Role::Produces => "produces",
            Role::Depends => "depends",
        }
    }
}

/// One fact read from a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub role: Role,
    /// Dotted, open-ended: `kill.precision`, `energy.grenade`, `Radiant`.
    pub tag: String,
    /// The sentence it was read from.
    pub evidence: String,
}

fn re(pattern: &str) -> Regex {
    Regex::new(&format!("(?i){pattern}")).expect("valid pattern")
}

/// Trigger tags and their patterns, read from conditions.
static TRIGGERS: LazyLock<Vec<(&str, Regex)>> = LazyLock::new(|| {
    [
        ("kill.precision", r"\bprecision (final blows?|kills?)\b"),
        (
            "kill.ability",
            r"\b(grenade|melee|ability|abilities|super|class ability|powered melee|finisher|glaive melee)[- ]?(final blows?|kills?)\b|\bkills? with (your )?(an? )?(grenades?|melees?|abilit(y|ies)|supers?)\b",
        ),
        (
            "kill.debuffed",
            r"\bkills?\b[^.]{0,30}\b(debuffed|scorched|frozen|slowed|jolted|weakened|suppressed|severed|suspended|blinded|unraveling|volatile|ignited)\b|\b(debuffed|scorched|frozen|slowed|jolted|weakened|suppressed|severed|suspended|blinded|unraveling)[- ](enemy |enemies |target )?kills?\b",
        ),
        (
            "kill.elite",
            r"\b(elite|miniboss|minibosses|champion|champions|boss|bosses|powerful|major|majors)\b[^.]{0,25}\bkills?\b|\bkills?\b[^.]{0,15}\b(elite|miniboss|champion|powerful|major)s?\b",
        ),
        (
            "kill.multi",
            r"\b(rapid(ly)?|multi)[- ]?(kills?|final blows?)\b|\bkills? in (quick|rapid) succession\b|\b\d+ (enemies|kills) within\b",
        ),
        (
            "kill.weapon",
            r"\b(weapon|primary|special|heavy|kinetic|energy|power|sniper|shotgun|rocket|bow|sword|glaive|grenade launcher)[- ]kills?\b|\bkills? with this weapon\b",
        ),
        (
            "kill",
            r"\b(kills?|killing|final blows?|defeat(s|ing|ed)?|multikills?)\b",
        ),
        ("hit.precision", r"\bprecision (hits?|damage|shots?)\b|\bcrits?\b"),
        (
            "hit",
            r"\b(hits?|damage instances?|direct hits?|impacts?)\b|\b(dealing|deal|deals|damaging|damage dealt)\b[^.]{0,30}\bdamage\b|\bdamag(ing|e) (an? )?(enemy|enemies|target|combatant)",
        ),
        (
            "ability_use",
            r"\b(grenade|melee|class ability|super|barricade|rift|dodge|rally|charged melee)\b[^.]{0,20}\b(usage|use|uses|cast|casts|activation|thrown|throw|placement|placed)\b|\b(using|casting|throwing|activating|placing)\b[^.]{0,15}\b(grenade|melee|class ability|super|barricade|rift|dodge)\b|\bfinishers?\b|\bon super (cast|end)\b",
        ),
        (
            "pickup",
            r"\b(pick(ing)? up|pickups?|collect(ing|s)?|picks up)\b|\bon (orb of power|firesprite|ionic trace|void breach|tangle|stasis shard|ammo brick|elemental pickup)",
        ),
        (
            "damage_taken",
            r"\b(being hit|being damaged|taking damage|receiving damage|received damage|critical health|low health|shields? (is |are )?(broken|depleted)|when (you are )?damaged|when hit|upon being)\b",
        ),
        ("movement", r"\b(sprint(ing)?|slid(e|ing)|slides|airborne|jump(ing)?|crouch(ed|ing)?|glid(e|ing)|dodg(e|ing))\b"),
        (
            "handling",
            r"\b(reload(s|ing)?|ready(ing)?|stow(ing|ed)?|swap(ping)?|ads|aiming down sights|hipfir(e|ing)|emptying the magazine|magazine)\b",
        ),
        (
            "surrounded",
            r"\bwithin \d+(\.\d+)? ?(metres|meters|m)\b[^.]{0,6}\bof (\d+|an?|any|multiple|several)\b[^.]{0,12}\b(enem(y|ies)|combatants?|targets?)\b|\bsurrounded\b|\bnearby enemies\b|\benemies (are )?nearby\b",
        ),
        ("combat_time", r"\b(in combat|every \d+(\.\d+)? seconds?|after \d+(\.\d+)? seconds? of)\b"),
        ("passive", r"\b(passively|while equipped|always active|at all times)\b"),
    ]
    .into_iter()
    .map(|(t, p)| (t, re(p)))
    .collect()
});

/// Trigger phrases recognised inside effects: "… from Grenade Kills", "… on kill".
static EFFECT_TRIGGER: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"\b(from|on|upon|after|when|for)\b[^.,;:]{0,30}\b(kills?|final blows?|hits?|pickups?|usage|cast|finishers?|damage dealt)\b",
    )
});

/// Payoff tags and their patterns, read from effects.
static PAYOFFS: LazyLock<Vec<(&str, Regex)>> = LazyLock::new(|| {
    [
        (
            "damage.ability",
            r"\b(grenade|melee|ability|abilities|super|class ability)\b[^.]{0,15}\bdamage\b[^.]{0,15}\b(increase|increased|bonus|more|multiplier)|\b(increase|increased|bonus|more)\b[^.]{0,20}\b(grenade|melee|ability|super)\b[^.]{0,5}\bdamage\b",
        ),
        (
            "damage.weapon",
            r"\b(increases?|increased|bonus|more|extra)\b[^.]{0,25}\bdamage\b|\bdamage (increase|bonus|buff|multiplier)\b|\d+(\.\d+)?% (increased )?damage\b|\bdamage\b[^.]{0,10}\bincreased by\b",
        ),
        (
            "damage.direct",
            r"\b(deals?|dealing) (up to )?\d+|\b(explosion|explodes?|detonat\w*|shockwaves?|blasts?|projectiles?|burst)\b|\bdamage over time\b",
        ),
        ("energy.grenade", r"\bgrenade (ability )?(energy|regen\w*|recharge|cooldown|charge)"),
        ("energy.melee", r"\bmelee (ability )?(energy|regen\w*|recharge|cooldown|charge)"),
        ("energy.class", r"\bclass ability (energy|regen\w*|recharge|cooldown|charge)"),
        ("energy.super", r"\bsuper (ability )?(energy|regen\w*|gain)"),
        ("energy.ability", r"\b(ability|abilities) (energy|regen\w*|cooldowns?)\b|\bability energy\b"),
        (
            "survival",
            r"\bdamage resist\w*|\bresist(ance)?\b|\bheal(s|ing|th regen\w*)?\b|\brestores? (\d+ )?(hp|health)|\bhealth\b|\bovershields?\b",
        ),
        ("ammo", r"\b(ammo|reserves|refill\w*|magazine|reload\w*)\b"),
        (
            "stats",
            r"\+\s?\d+(\.\d+)? (handling|range|stability|reload|aim assist|airborne|mobility|zoom|charge time|draw time|weapons|health|class|grenade|super|melee)\b|\b(handling|reload speed|stability|range|aim assist)\b",
        ),
        ("speed", r"\b(movement speed|sprint speed|speed booster|faster|move faster)\b"),
        ("spawn", r"\b(spawns?|creates?|generates?|drops?|summons?|releases?)\b"),
    ]
    .into_iter()
    .map(|(t, p)| (t, re(p)))
    .collect()
});

/// Game states: a name and the pattern of its mentions.
pub static STATES: LazyLock<Vec<(&str, Regex)>> = LazyLock::new(|| {
    [
        ("Radiant", r"\bradiant\b"),
        ("Restoration", r"\brestoration\b"),
        ("Cure", r"\bcure[ds]?\b"),
        ("Scorch", r"\bscorch(ed|es|ing)?\b"),
        ("Ignition", r"\bignit(e|es|ed|ion|ions)\b"),
        ("Firesprite", r"\bfiresprites?\b"),
        ("Weaken", r"\bweaken(s|ed|ing)?\b"),
        ("Volatile", r"\bvolatile\b"),
        ("Devour", r"\bdevour\b"),
        ("Invisibility", r"\binvisib(le|ility)\b"),
        ("Overshield", r"\bovershields?\b"),
        ("Suppression", r"\bsuppress(ion|ed|es|ing)?\b"),
        ("Void Breach", r"\bvoid breach(es)?\b"),
        ("Amplified", r"\bamplif(ied|y)\b"),
        ("Jolt", r"\bjolt(s|ed|ing)?\b"),
        ("Blind", r"\bblind(s|ed|ing)?\b"),
        ("Ionic Trace", r"\bionic traces?\b"),
        ("Bolt Charge", r"\bbolt charges?\b"),
        ("Slow", r"\bslow(s|ed|ing)?\b"),
        ("Freeze", r"\b(freez(e|es|ing)|frozen)\b"),
        ("Shatter", r"\bshatter(s|ed|ing)?\b"),
        ("Frost Armor", r"\bfrost armor\b"),
        ("Stasis Crystal", r"\bstasis crystals?\b"),
        ("Stasis Shard", r"\bstasis shards?\b"),
        ("Sever", r"\bsever(s|ed|ing)?\b"),
        ("Suspend", r"\bsuspend(s|ed|ing)?\b"),
        ("Unravel", r"\bunravel(s|ed|ing)?\b"),
        ("Woven Mail", r"\bwoven mail\b"),
        ("Tangle", r"\btangles?\b"),
        ("Threadling", r"\bthreadlings?\b"),
        ("Orb of Power", r"\borbs? of power\b"),
        ("Armor Charge", r"\barmor charges?\b"),
        ("Transcendence", r"\btranscend(ent|ence)\b"),
        ("Speed Booster", r"\bspeed booster\b"),
        ("Arc Debuff", r"\barc[- ](debuff(ed|s)?|-debuffed)\b"),
        ("Solar Debuff", r"\bsolar[- ]debuff(ed|s)?\b"),
        ("Void Debuff", r"\bvoid[- ]debuff(ed|s)?\b"),
        ("Stasis Debuff", r"\bstasis[- ]debuff(ed|s)?\b"),
        ("Strand Debuff", r"\bstrand[- ]debuff(ed|s)?\b"),
    ]
    .into_iter()
    .map(|(s, p)| (s, re(p)))
    .collect()
});

/// Verbs ending a sentence's condition and beginning its effect.
static EFFECT_VERB: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"\b(grants?|granting|triggers?|spawns?|appl(y|ies)|refills?|increases?|creates?|emits?|restores?|inflicts?|causes?|deals?|reduces?|gives?|starts?|consumes?|becomes?|returns?|refunds?|generates?|drops?|releases?|unleashes?|fires?|summons?|heals?|makes?|extends?|regenerates?)\b",
    )
});

/// Verbs that put a state into play.
static PRODUCE_VERB: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"\b(grants?|granting|inflicts?|inflicting|appl(y|ies|ying)|creates?|creating|spawns?|spawning|generates?|drops?|becomes?|triggers?|causes?|emits?|makes?|releases?|leaves?|summons?|turns?|gains?|gaining|receives?)\b",
    )
});

/// Words before a state that make it a requirement.
static DEPEND_BEFORE: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"\b(while|against|on|when|whenever|after|if|during|from|with|by|upon|near|affected by|inside|in)\s+(an?\s+|the\s+|your\s+|any\s+|\w+\s+){0,2}$",
    )
});

/// Words after a state that make it a requirement ("Scorched enemies", "Tangle pickup").
static DEPEND_AFTER: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"^(\s|-)*(enem(y|ies)|targets?|combatants?|pickups?|is active|are active|stacks? (is|are) active|kills?)\b",
    )
});

/// A sentence split into its condition and its effect.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Clause<'a> {
    sentence: &'a str,
    /// The heading it is under, if any.
    heading: Option<&'a str>,
    condition: &'a str,
    effect: &'a str,
}

/// Sentences of a line: split at ". " before a capital, not after a digit.
fn sentences(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut start = 0;
    for i in 0..bytes.len() {
        if bytes[i] == b'.'
            && i + 2 < bytes.len()
            && bytes[i + 1] == b' '
            && bytes[i + 2].is_ascii_uppercase()
            && i > 0
            && !bytes[i - 1].is_ascii_digit()
        {
            out.push(line[start..=i].trim());
            start = i + 2;
        }
    }
    let rest = line[start..].trim();
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

/// A text's clauses, headings carried to the lines under them.
fn clauses(text: &str) -> Vec<Clause<'_>> {
    let mut out = Vec::new();
    let mut heading: Option<&str> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            heading = None;
            continue;
        }
        let body = line.trim_start_matches(['•', '-', '*', '↑', '🡅']).trim();
        if body.ends_with(':') && body.chars().count() < 120 {
            heading = Some(body.trim_end_matches(':'));
            continue;
        }
        for s in sentences(body) {
            let (condition, effect) = match EFFECT_VERB.find(s) {
                Some(m) => (s[..m.start()].trim(), s[m.start()..].trim()),
                None => ("", s),
            };
            out.push(Clause {
                sentence: s,
                heading,
                condition,
                effect,
            });
        }
    }
    out
}

fn add(out: &mut Vec<Tag>, role: Role, tag: &str, evidence: &str) {
    if !out.iter().any(|t| t.role == role && t.tag == tag) {
        out.push(Tag {
            role,
            tag: tag.to_string(),
            evidence: evidence.to_string(),
        });
    }
}

/// Read a text's tags, each once (with the first sentence it came from).
#[must_use]
pub fn read(text: &str) -> Vec<Tag> {
    let mut out: Vec<Tag> = Vec::new();
    for c in clauses(text) {
        let evidence = match c.heading {
            Some(h) => format!("{h}: {}", c.sentence),
            None => c.sentence.to_string(),
        };
        // Triggers: the heading and the condition; trigger phrases in the effect.
        let mut conditions: Vec<&str> = c.heading.into_iter().collect();
        conditions.push(c.condition);
        let in_effect: Vec<&str> = EFFECT_TRIGGER
            .find_iter(c.effect)
            .map(|m| m.as_str())
            .collect();
        for part in conditions.iter().chain(in_effect.iter()) {
            for (tag, pattern) in TRIGGERS.iter() {
                if pattern.is_match(part) {
                    add(&mut out, Role::Trigger, tag, &evidence);
                }
            }
        }
        for (tag, pattern) in PAYOFFS.iter() {
            if pattern.is_match(c.effect) {
                add(&mut out, Role::Payoff, tag, &evidence);
            }
        }
        // States: required where conditions name them; produced or
        // required in the effect by the words around them.
        for part in &conditions {
            for (state, pattern) in STATES.iter() {
                if pattern.is_match(part) {
                    add(&mut out, Role::Depends, state, &evidence);
                }
            }
        }
        for (state, pattern) in STATES.iter() {
            for m in pattern.find_iter(c.effect) {
                let before = &c.effect[..m.start()];
                let after = &c.effect[m.end()..];
                let window_start = before.char_indices().rev().nth(40).map_or(0, |(i, _)| i);
                let near_before = &before[window_start..];
                if DEPEND_BEFORE.is_match(before) || DEPEND_AFTER.is_match(after) {
                    add(&mut out, Role::Depends, state, &evidence);
                } else if PRODUCE_VERB.is_match(near_before) {
                    add(&mut out, Role::Produces, state, &evidence);
                }
            }
        }
    }
    // A part that needs nothing in particular works all the time.
    if !out.iter().any(|t| t.role == Role::Trigger)
        && let Some(first) = out
            .iter()
            .find(|t| t.role == Role::Payoff)
            .map(|t| t.evidence.clone())
    {
        add(&mut out, Role::Trigger, "passive", &first);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(text: &str, role: Role) -> Vec<String> {
        let mut v: Vec<String> = read(text)
            .into_iter()
            .filter(|t| t.role == role)
            .map(|t| t.tag)
            .collect();
        v.sort();
        v
    }

    #[test]
    fn sentences_split_before_capitals_not_decimals() {
        assert_eq!(
            sentences("Stacks expire every 4.5 seconds. Grants a 10% damage increase."),
            [
                "Stacks expire every 4.5 seconds.",
                "Grants a 10% damage increase."
            ]
        );
    }

    #[test]
    fn kill_engines_read_as_kill_triggers_with_energy_payoffs() {
        let demolitionist = "Weapon Kills grant 10% Grenade Ability Energy.\n\nUsing your Grenade Ability refills the Magazine with a 3 second cooldown.";
        let t = tags(demolitionist, Role::Trigger);
        assert!(
            t.contains(&"kill.weapon".to_string()) && t.contains(&"ability_use".to_string()),
            "{t:?}"
        );
        let p = tags(demolitionist, Role::Payoff);
        assert!(
            p.contains(&"energy.grenade".to_string()) && p.contains(&"ammo".to_string()),
            "{p:?}"
        );
    }

    #[test]
    fn states_are_produced_by_effects_and_depended_on_by_conditions() {
        let starvation = "Picking up an Orb of Power or Void Breach grants Devour for 5 seconds.";
        assert_eq!(tags(starvation, Role::Trigger), ["pickup"]);
        assert_eq!(tags(starvation, Role::Produces), ["Devour"]);
        assert_eq!(
            tags(starvation, Role::Depends),
            ["Orb of Power", "Void Breach"]
        );

        let repulsor =
            "Kills against Void Debuffed enemies grant a 45 HP Void Overshield for 8 seconds.";
        assert!(tags(repulsor, Role::Trigger).contains(&"kill.debuffed".to_string()));
        assert_eq!(tags(repulsor, Role::Depends), ["Void Debuff"]);
        assert_eq!(tags(repulsor, Role::Produces), ["Overshield"]);

        let incandescent = "Weapon Kills trigger a Solar explosion that applies 30 Scorch Stacks and deals up to 30 damage to enemies.";
        assert_eq!(tags(incandescent, Role::Produces), ["Scorch"]);
        assert!(tags(incandescent, Role::Payoff).contains(&"damage.direct".to_string()));
    }

    #[test]
    fn headings_condition_the_lines_under_them() {
        let text =
            "On Orb of Power Pickup:\n10% Grenade Energy.\n\nWhile Amplified:\n+20 Handling.";
        assert_eq!(tags(text, Role::Trigger), ["pickup"]);
        assert_eq!(tags(text, Role::Depends), ["Amplified", "Orb of Power"]);
        assert_eq!(tags(text, Role::Payoff), ["energy.grenade", "stats"]);
    }

    #[test]
    fn hit_driven_and_passive_parts_are_not_kill_triggered() {
        let bait = "Dealing damage with all 3 of your weapons without letting the timer run out grants you 'Bait and Switch' for 10 seconds, granting 30% increased damage.";
        assert_eq!(tags(bait, Role::Trigger), ["hit"]);
        assert!(tags(bait, Role::Payoff).contains(&"damage.weapon".to_string()));
        let synthoceps = "While within 15 metres of 3 enemies:\nGrants Biotic Enhancements.\n165% Increased Melee Damage.";
        assert_eq!(tags(synthoceps, Role::Trigger), ["surrounded"]);
        let passive = "Passively grants +30 Airborne Effectiveness.";
        assert_eq!(tags(passive, Role::Trigger), ["passive"]);
        let unconditioned = "Increases Weapon Damage by 20% for 10 seconds.";
        assert_eq!(tags(unconditioned, Role::Trigger), ["passive"]);
    }
}
