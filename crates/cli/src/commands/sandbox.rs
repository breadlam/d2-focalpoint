//! `focalpoint sandbox` — the build model's vocabulary and rule base
//! (`docs/build-model.md`, `docs/sandbox.md`).

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use focalpoint_db::Database;
use focalpoint_sandbox::check;
use focalpoint_sandbox::ontology::Ontology;
use focalpoint_sandbox::rules::RuleBase;

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
