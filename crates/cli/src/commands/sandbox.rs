//! `focalpoint sandbox` — the build model's vocabulary and rule base
//! (`docs/build-model.md`, `docs/sandbox.md`).

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use focalpoint_db::Database;
use focalpoint_sandbox::check;
use focalpoint_sandbox::compile::{Build, compile};
use focalpoint_sandbox::economy::Economy;
use focalpoint_sandbox::fluid::{Engine, Event, Knob, Setup, threshold};
use focalpoint_sandbox::ontology::Ontology;
use focalpoint_sandbox::rules::RuleBase;
use focalpoint_sandbox::scenario::Scenario;

use crate::output::{Format, render, thousands};

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub what: What,

    /// Output format for tables
    #[arg(long, value_enum, default_value_t = Format::Table, global = true)]
    pub format: Format,
}

#[derive(Debug, clap::Subcommand)]
pub enum What {
    /// Check the rule base against the vocabulary, and its coverage of recorded builds
    #[command(after_help = "Examples:\n  \
focalpoint sandbox check                      # the built-in rule base\n  \
focalpoint sandbox check --dir data/sandbox   # while editing (ontology.toml and rules/)\n  \
focalpoint sandbox check --top 50             # a longer list of parts not yet modelled\n\n\
With a database, also resolves the parts the records name against the manifest and lists the most-used \
parts not yet modelled: the drafting worklist.")]
    Check(CheckArgs),
    /// Analyse a build in a context: its loops, their gains and thresholds
    #[command(after_help = "Examples:\n  \
focalpoint sandbox analyze --part 'fragment:Ember of Char' --part 'fragment:Ember of Ashes' \\\n    \
--part 'artifact_perk:Solar Fulmination' --part 'armor:Dawn Chorus' --seed ignition \\\n    \
--context 'Ultimate Conquest'\n  \
focalpoint sandbox analyze --part 'aspect:Howl of the Storm' --part 'melee:Shiver Strike' \\\n    \
--part 'fragment:Whisper of Hunger' --stat melee=70\n\n\
Parts are `kind:name`, as the rule base writes them. Each equipped ability's cast is a seed; --seed adds \
events (ignition). The scenario is the context's durability and health tier with the pack priors; \
without --context, durability 1. Needs `focalpoint mechanics import`.")]
    Analyze(AnalyzeArgs),
}

#[derive(Debug, clap::Args)]
pub struct AnalyzeArgs {
    /// A part of the build, `kind:name` (repeat)
    #[arg(long = "part", required = true)]
    pub parts: Vec<String>,

    /// An extra event to start from, as well as the abilities' casts: `ignition`
    #[arg(long = "seed")]
    pub seeds: Vec<String>,

    /// The context: an id or a name (`focalpoint mechanics context`)
    #[arg(long)]
    pub context: Option<String>,

    /// Enemies per pack
    #[arg(long)]
    pub pack: Option<f64>,

    /// The pack's radius, in metres
    #[arg(long)]
    pub spread: Option<f64>,

    /// The share of elites
    #[arg(long)]
    pub elites: Option<f64>,

    /// A stat's value, `melee=70` (repeat; 100 where not given)
    #[arg(long = "stat")]
    pub stats: Vec<String>,
}

#[derive(Debug, clap::Args)]
pub struct CheckArgs {
    /// A sandbox folder (`ontology.toml` and `rules/`) to check instead of the built-in one
    #[arg(long)]
    pub dir: Option<PathBuf>,

    /// Parts not yet modelled to list, most used first
    #[arg(long, default_value_t = 25)]
    pub top: usize,
}

pub fn run(args: &Args, db: &Path) -> Result<()> {
    match &args.what {
        What::Check(a) => check(a, db, args.format),
        What::Analyze(a) => analyze(a, db, args.format),
    }
}

fn check(a: &CheckArgs, db: &Path, format: Format) -> Result<()> {
    let (ontology, base) = match &a.dir {
        Some(dir) => (
            Ontology::parse(&std::fs::read_to_string(dir.join("ontology.toml"))?)?,
            focalpoint_sandbox::rules_in(&dir.join("rules"))?,
        ),
        None => (Ontology::builtin()?, focalpoint_sandbox::builtin_rules()?),
    };
    println!(
        "Vocabulary: {} events, {} states. Rule base: {} rules, {} modifiers, {} counters, {} gaps; {} parts modelled",
        ontology.events.len(),
        ontology.states.len(),
        base.rules.len(),
        base.modifiers.len(),
        base.counters.len(),
        base.gaps.len(),
        base.modelled().len(),
    );
    let report = check::check(&ontology, &base);
    if !report.clarity_only.is_empty() {
        println!(
            "Resting on Clarity alone: {}",
            report.clarity_only.join(", ")
        );
    }
    if db.exists() {
        coverage(&Database::open_read_only(db)?, &base, a.top, format)?;
    } else {
        println!("No database at {}: coverage skipped.", db.display());
    }
    if !base.gaps.is_empty() {
        println!("## Gaps: behaviour known and not yet modelled");
        let header = ["part", "gap"].map(String::from);
        let rows: Vec<Vec<String>> = base
            .gaps
            .iter()
            .map(|g| vec![g.item.part.to_string(), g.item.note.clone()])
            .collect();
        println!("{}", render(format, &header, &rows));
    }
    if report.problems.is_empty() {
        println!("No problems.");
        return Ok(());
    }
    let header = ["file", "record", "problem"].map(String::from);
    let rows: Vec<Vec<String>> = report
        .problems
        .iter()
        .map(|p| vec![p.file.clone(), p.record.clone(), p.message.clone()])
        .collect();
    println!("{}", render(format, &header, &rows));
    bail!("{} problems", report.problems.len())
}

fn coverage(db: &Database, base: &RuleBase, top: usize, format: Format) -> Result<()> {
    let (by_kind, unmodelled, unresolved) = check::coverage(db, base)?;
    println!("## Coverage of the parts recorded builds use");
    let pct = |a: i64, b: i64| {
        if b == 0 {
            "—".to_string()
        } else {
            format!("{:.1}%", 100.0 * a as f64 / b as f64)
        }
    };
    let header = [
        "kind",
        "parts",
        "modelled",
        "partly",
        "uses",
        "modelled uses",
    ]
    .map(String::from);
    let (mut parts, mut modelled, mut partial, mut uses, mut modelled_uses) = (0, 0, 0, 0, 0);
    let mut rows: Vec<Vec<String>> = by_kind
        .iter()
        .map(|c| {
            parts += c.parts;
            modelled += c.modelled;
            partial += c.partial;
            uses += c.uses;
            modelled_uses += c.modelled_uses;
            vec![
                c.kind.clone(),
                thousands(c.parts as i64),
                thousands(c.modelled as i64),
                thousands(c.partial as i64),
                thousands(c.uses),
                pct(c.modelled_uses, c.uses),
            ]
        })
        .collect();
    rows.push(vec![
        "all".into(),
        thousands(parts as i64),
        thousands(modelled as i64),
        thousands(partial as i64),
        thousands(uses),
        pct(modelled_uses, uses),
    ]);
    println!("{}", render(format, &header, &rows));
    if !unresolved.is_empty() {
        println!(
            "Parts named that no manifest item has the name of: {}",
            unresolved.join(", ")
        );
    }
    if top > 0 {
        println!("## The most-used parts not yet modelled");
        let header = ["kind", "part", "uses"].map(String::from);
        let rows: Vec<Vec<String>> = unmodelled
            .iter()
            .take(top)
            .map(|u| vec![u.kind.clone(), u.name.clone(), thousands(u.uses)])
            .collect();
        println!("{}", render(format, &header, &rows));
    }
    Ok(())
}

fn scenario(a: &AnalyzeArgs, db: &Database) -> Result<Scenario> {
    let mut s = match &a.context {
        Some(q) => Scenario::from_context(db, q)?,
        None => Scenario::baseline(db)?,
    };
    if let Some(n) = a.pack {
        s.pack.size = n;
    }
    if let Some(r) = a.spread {
        s.pack.radius_m = r;
    }
    if let Some(e) = a.elites {
        s.elite_share = e;
    }
    for kv in &a.stats {
        let Some((k, v)) = kv.split_once('=') else {
            bail!("--stat {kv:?}: expected name=value");
        };
        s.stats.insert(k.trim().to_string(), v.trim().parse()?);
    }
    Ok(s)
}

fn analyze(a: &AnalyzeArgs, db: &Path, format: Format) -> Result<()> {
    let ontology = Ontology::builtin()?;
    let base = focalpoint_sandbox::builtin_rules()?;
    let db = Database::open_read_only(db)?;
    let economy = Economy::load(&db)?;
    let build = Build::parse(a.parts.iter().map(String::as_str))?;
    let unknown: Vec<String> = build
        .parts
        .iter()
        .filter(|p| !base.parts().contains(p))
        .map(ToString::to_string)
        .collect();
    let compiled = compile(&ontology, &base, &build);
    let s = scenario(a, &db)?;
    let mut seeds: Vec<Event> = [
        ("grenade", "grenade"),
        ("melee", "melee"),
        ("class_ability", "class"),
        ("super", "super"),
    ]
    .iter()
    .filter(|(slot, _)| build.equipped(slot).is_some())
    .map(|(_, ability)| Event::cast(ability))
    .collect();
    seeds.extend(a.seeds.iter().map(|e| Event::new(e)));
    if seeds.is_empty() {
        bail!("nothing to start from: equip an ability, or give --seed");
    }
    println!(
        "{}: durability ×{:.2}, packs of {} within {} m, {:.0}% elites",
        s.name,
        s.durability,
        s.pack.size,
        s.pack.radius_m,
        s.elite_share * 100.0
    );
    println!(
        "Build: {} rules, {} modifiers granted",
        compiled.rules.len(),
        compiled.modifiers.len()
    );
    if !unknown.is_empty() {
        println!("Not modelled (no records): {}", unknown.join(", "));
    }
    let report = Engine::new(&ontology, &compiled, &economy, &s).analyse(&seeds);

    println!("## Loops");
    if report.loops.is_empty() {
        println!("None: nothing this build does comes back to itself.");
    } else {
        let knobs = [
            (Knob::PackSize, (1.0, 40.0)),
            (Knob::Durability, (0.25, 12.0)),
        ];
        let header = ["loop", "gain", "crosses 1 at"].map(String::from);
        let mut rows = Vec::new();
        for l in &report.loops {
            let member = l.members[0].clone();
            let mut at = Vec::new();
            let stats: Vec<Knob> = ["grenade", "melee", "class", "super"]
                .iter()
                .filter(|st| {
                    l.members
                        .iter()
                        .any(|m| m.contains(&format!("ability={st}")))
                })
                .map(|st| Knob::Stat((*st).to_string()))
                .collect();
            for (knob, range) in knobs
                .iter()
                .cloned()
                .chain(stats.into_iter().map(|k| (k, (0.0, 200.0))))
            {
                let setup = Setup {
                    ontology: &ontology,
                    compiled: &compiled,
                    economy: &economy,
                    scenario: &s,
                    seeds: &seeds,
                };
                if let Some(x) = threshold(setup, &member, &knob, range) {
                    at.push(format!("{knob} {x:.1}"));
                }
            }
            rows.push(vec![
                l.members.join(" → "),
                format!("{:.2}", l.gain),
                if at.is_empty() {
                    "—".into()
                } else {
                    at.join("; ")
                },
            ]);
        }
        println!("{}", render(format, &header, &rows));
    }

    println!("## Generations");
    let header = [
        "from", "leads to", "damage", "kills", "energy", "pickups", "frozen",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = report
        .generations
        .iter()
        .map(|g| {
            let y = &g.yields;
            vec![
                g.root.to_string(),
                g.next
                    .iter()
                    .map(|(e, n)| format!("{n:.2} {e}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                format!("{:.0}", y.damage),
                format!("{:.2}", y.kills),
                y.energy
                    .iter()
                    .map(|(k, v)| format!("{k} {v:.2}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                format!("{:.1}", y.pickups),
                format!("{:.1}", y.frozen),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    if !report.counters.is_empty() {
        let c: Vec<String> = report
            .counters
            .iter()
            .map(|(k, v)| format!("{k} {v:.1}"))
            .collect();
        println!("Counters when spent: {}", c.join(", "));
    }
    println!("## Taken without data");
    for n in s.assumptions.iter().chain(report.notes.iter()) {
        println!("- {n}");
    }
    Ok(())
}
