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
use focalpoint_sandbox::variables::Variables;

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
    /// The values no source gives: each variable's guess, range, what reads it and how to measure it
    #[command(after_help = "Examples:\n  \
focalpoint sandbox unknowns              # the catalog (data/sandbox/variables.toml)\n  \
focalpoint sandbox unknowns --measure    # with how to measure each in-game\n  \
focalpoint sandbox unknowns --status guess")]
    Unknowns(UnknownsArgs),
}

#[derive(Debug, clap::Args)]
pub struct UnknownsArgs {
    /// Show how to measure each, and the basis of its guess
    #[arg(long)]
    pub measure: bool,

    /// Only variables of this status: guess, estimate, measured
    #[arg(long)]
    pub status: Option<String>,

    /// A sandbox folder (`ontology.toml`, `variables.toml`, `rules/`) instead of the built-in one
    #[arg(long)]
    pub dir: Option<PathBuf>,
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

    /// A stat's value, `melee=70` (repeat; `build.default_stat` where not given)
    #[arg(long = "stat")]
    pub stats: Vec<String>,

    /// Set a variable for this run, `howl_of_the_storm.freeze_radius=4` (repeat)
    #[arg(long = "set")]
    pub set: Vec<String>,

    /// Also report each loop's gain at the low and high end of every variable it reads
    #[arg(long)]
    pub sensitivity: bool,
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
        What::Unknowns(a) => unknowns(a, args.format),
    }
}

fn check(a: &CheckArgs, db: &Path, format: Format) -> Result<()> {
    let (ontology, base, vars) = load(a.dir.as_deref())?;
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
    let report = check::check(&ontology, &base, &vars);
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

/// The vocabulary, rule base and variables: built in, or from a folder.
fn load(dir: Option<&Path>) -> Result<(Ontology, RuleBase, Variables)> {
    Ok(match dir {
        Some(dir) => (
            Ontology::parse(&std::fs::read_to_string(dir.join("ontology.toml"))?)?,
            focalpoint_sandbox::rules_in(&dir.join("rules"))?,
            Variables::parse(&std::fs::read_to_string(dir.join("variables.toml"))?)?,
        ),
        None => (
            Ontology::builtin()?,
            focalpoint_sandbox::builtin_rules()?,
            Variables::builtin()?,
        ),
    })
}

fn pairs(list: &[String], what: &str) -> Result<Vec<(String, f64)>> {
    list.iter()
        .map(|kv| {
            let Some((k, v)) = kv.split_once('=') else {
                bail!("--{what} {kv:?}: expected name=value");
            };
            Ok((k.trim().to_string(), v.trim().parse()?))
        })
        .collect()
}

fn unknowns(a: &UnknownsArgs, format: Format) -> Result<()> {
    let (ontology, base, vars) = load(a.dir.as_deref())?;
    let users = base.variable_users();
    let engine: std::collections::BTreeSet<&str> = focalpoint_sandbox::variables::ENGINE
        .iter()
        .copied()
        .collect();
    let read_by = |name: &str| -> String {
        let mut who: Vec<String> = users
            .get(name)
            .map(|u| u.iter().cloned().collect())
            .unwrap_or_default();
        if engine.contains(name) {
            who.push("the engine".into());
        }
        if ontology.has_param(name) {
            who.push("the core verb".into());
        }
        who.join(", ")
    };
    let rows: Vec<&focalpoint_sandbox::variables::Variable> = vars
        .iter()
        .filter(|v| a.status.as_ref().is_none_or(|s| *s == v.status))
        .collect();
    let mut header = vec!["variable", "guess", "range", "unit", "status", "read by"];
    if a.measure {
        header.extend(["basis", "how to measure"]);
    }
    let header: Vec<String> = header.into_iter().map(String::from).collect();
    let table: Vec<Vec<String>> = rows
        .iter()
        .map(|v| {
            let mut row = vec![
                v.name.clone(),
                format!("{}", v.guess),
                format!("{}–{}", v.low, v.high),
                v.unit.clone(),
                v.status.clone(),
                read_by(&v.name),
            ];
            if a.measure {
                row.extend([v.basis.clone(), v.measure.clone()]);
            }
            row
        })
        .collect();
    println!(
        "{} variables ({} guesses, {} estimates, {} measured)",
        rows.len(),
        rows.iter().filter(|v| v.status == "guess").count(),
        rows.iter().filter(|v| v.status == "estimate").count(),
        rows.iter().filter(|v| v.status == "measured").count()
    );
    println!("{}", render(format, &header, &table));
    Ok(())
}

fn scenario(a: &AnalyzeArgs, db: &Database, vars: &Variables) -> Result<Scenario> {
    let mut s = match &a.context {
        Some(q) => Scenario::from_context(db, q, vars)?,
        None => Scenario::baseline(db, vars)?,
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
    for (k, v) in pairs(&a.stats, "stat")? {
        s.stats.insert(k, v);
    }
    Ok(s)
}

fn analyze(a: &AnalyzeArgs, db: &Path, format: Format) -> Result<()> {
    let (ontology, base, mut vars) = load(None)?;
    for (k, v) in pairs(&a.set, "set")? {
        vars.set(&k, v)?;
    }
    let db = Database::open_read_only(db)?;
    let economy = Economy::load(&db)?;
    let build = Build::parse(a.parts.iter().map(String::as_str))?;
    let unknown: Vec<String> = build
        .parts
        .iter()
        .filter(|p| !base.parts().contains(p))
        .map(ToString::to_string)
        .collect();
    let compiled = compile(&ontology, &base, &build, &vars);
    let s = scenario(a, &db, &vars)?;
    let mut seeds = focalpoint_sandbox::rates::seeds(&compiled);
    seeds.extend(a.seeds.iter().map(|e| Event::new(e)));
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

    let r = focalpoint_sandbox::rates::rates(&ontology, &compiled, &economy, &s, &report);
    println!("## Per minute");
    let header = ["", "value"].map(String::from);
    let mut rows: Vec<Vec<String>> = r
        .casts_per_min
        .iter()
        .map(|(a, n)| {
            let returned = r.returned_per_min.get(a).copied().unwrap_or(0.0);
            vec![
                format!("{a} casts"),
                format!("{n:.1} ({:.1} paid by returned energy)", returned.min(*n)),
            ]
        })
        .collect();
    rows.extend([
        vec![
            "kills by abilities and verbs".into(),
            format!(
                "{:.1} of {:.1} ({:.0}%)",
                r.ability_kills_per_min,
                r.supply_per_min,
                100.0 * r.ability_kill_share()
            ),
        ],
        vec![
            "damage a second (abilities, verbs)".into(),
            format!("{:.0}", r.damage_per_s),
        ],
        vec!["healing a second".into(), format!("{:.1}", r.heal_per_s)],
    ]);
    for (b, u) in &r.buffs {
        rows.push(vec![
            format!("{b} uptime"),
            format!("{:.0}% (mean x{:.1})", 100.0 * u.uptime, u.mean_stacks),
        ]);
    }
    println!("{}", render(format, &header, &rows));
    for c in &r.capped {
        println!("Capped: {c}");
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
    let read: Vec<&focalpoint_sandbox::variables::Variable> = report
        .variables
        .iter()
        .filter_map(|n| vars.get(n))
        .collect();
    if !read.is_empty() {
        println!("## Variables this result reads (`focalpoint sandbox unknowns`)");
        let mut header = vec!["variable", "value", "status"];
        if a.sensitivity {
            header.extend(["at low", "at high"]);
        }
        let header: Vec<String> = header.into_iter().map(String::from).collect();
        let main = report.loops.first().map(|l| l.members[0].clone());
        let rows: Vec<Vec<String>> = read
            .iter()
            .map(|v| {
                let value = vars.overrides().get(&v.name).copied().unwrap_or(v.guess);
                let mut row = vec![
                    v.name.clone(),
                    format!("{value} {}", v.unit),
                    v.status.clone(),
                ];
                if a.sensitivity {
                    for x in [v.low, v.high] {
                        row.push(match &main {
                            Some(member) => {
                                let gain = gain_with(
                                    &ontology, &base, &build, &vars, &economy, a, &db, &seeds,
                                    &v.name, x, member,
                                );
                                format!(
                                    "{x} → {}",
                                    gain.map_or_else(|e| e.to_string(), |g| format!("{g:.2}"))
                                )
                            }
                            None => "—".into(),
                        });
                    }
                }
                row
            })
            .collect();
        println!("{}", render(format, &header, &rows));
        if a.sensitivity
            && let Some(m) = &main
        {
            println!("(gains are of the loop through {m})");
        }
    }
    Ok(())
}

/// The gain of the loop through `member` with one variable set to `x`.
#[allow(clippy::too_many_arguments)]
fn gain_with(
    ontology: &Ontology,
    base: &RuleBase,
    build: &Build,
    vars: &Variables,
    economy: &Economy,
    a: &AnalyzeArgs,
    db: &Database,
    seeds: &[Event],
    name: &str,
    x: f64,
    member: &str,
) -> Result<f64> {
    let mut vars = vars.clone();
    vars.set(name, x)?;
    let compiled = compile(ontology, base, build, &vars);
    let s = scenario(a, db, &vars)?;
    let report = Engine::new(ontology, &compiled, economy, &s).analyse(seeds);
    Ok(focalpoint_sandbox::fluid::loop_gain(&report, member))
}
