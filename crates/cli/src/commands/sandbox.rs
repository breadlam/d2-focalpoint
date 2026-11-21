//! `focalpoint sandbox` — the build model's vocabulary and part cards
//! (`docs/build-model.md`, `docs/sandbox.md`).

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use focalpoint_db::Database;
use focalpoint_sandbox::check::{self, Located};
use focalpoint_sandbox::ontology::Ontology;

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
    /// Check the part cards against the vocabulary, and their coverage of recorded builds
    #[command(after_help = "Examples:\n  \
focalpoint sandbox check                      # the built-in cards\n  \
focalpoint sandbox check --dir data/sandbox   # while editing (ontology.toml and parts/)\n  \
focalpoint sandbox check --top 50             # a longer list of uncarded parts\n\n\
With a database, also resolves the cards against the manifest and lists the most-used parts without a \
card: the drafting worklist.")]
    Check(CheckArgs),
}

#[derive(Debug, clap::Args)]
pub struct CheckArgs {
    /// A sandbox folder (`ontology.toml` and `parts/`) to check instead of the built-in one
    #[arg(long)]
    pub dir: Option<PathBuf>,

    /// Uncarded parts to list, most used first
    #[arg(long, default_value_t = 25)]
    pub top: usize,
}

pub fn run(args: &Args, db: &Path) -> Result<()> {
    match &args.what {
        What::Check(a) => check(a, db, args.format),
    }
}

fn check(a: &CheckArgs, db: &Path, format: Format) -> Result<()> {
    let (ontology, cards) = match &a.dir {
        Some(dir) => (
            Ontology::parse(&std::fs::read_to_string(dir.join("ontology.toml"))?)?,
            focalpoint_sandbox::cards_in(&dir.join("parts"))?,
        ),
        None => (Ontology::builtin()?, focalpoint_sandbox::builtin_cards()?),
    };
    let rules: usize = cards.iter().map(|c| c.card.rule.len()).sum();
    let modifiers: usize = cards.iter().map(|c| c.card.modify.len()).sum();
    println!(
        "Vocabulary: {} events, {} states; {} cards ({} rules, {} modifiers)",
        ontology.events.len(),
        ontology.states.len(),
        cards.len(),
        rules,
        modifiers
    );
    let report = check::check(&ontology, &cards);
    if !report.clarity_only.is_empty() {
        println!(
            "Resting on Clarity alone: {}",
            report.clarity_only.join(", ")
        );
    }
    if db.exists() {
        coverage(&Database::open_read_only(db)?, &cards, a.top, format)?;
    } else {
        println!("No database at {}: coverage skipped.", db.display());
    }
    if report.problems.is_empty() {
        println!("No problems.");
        return Ok(());
    }
    let header = ["file", "card", "problem"].map(String::from);
    let rows: Vec<Vec<String>> = report
        .problems
        .iter()
        .map(|p| vec![p.file.clone(), p.card.clone(), p.message.clone()])
        .collect();
    println!("{}", render(format, &header, &rows));
    bail!("{} problems", report.problems.len())
}

fn coverage(db: &Database, cards: &[Located], top: usize, format: Format) -> Result<()> {
    let (by_kind, uncarded, unresolved) = check::coverage(db, cards)?;
    println!("## Coverage of the parts recorded builds use");
    let pct = |a: i64, b: i64| {
        if b == 0 {
            "—".to_string()
        } else {
            format!("{:.1}%", 100.0 * a as f64 / b as f64)
        }
    };
    let header = ["kind", "parts", "carded", "uses", "carded uses"].map(String::from);
    let (mut parts, mut carded, mut uses, mut carded_uses) = (0, 0, 0, 0);
    let mut rows: Vec<Vec<String>> = by_kind
        .iter()
        .map(|c| {
            parts += c.parts;
            carded += c.carded_parts;
            uses += c.uses;
            carded_uses += c.carded_uses;
            vec![
                c.kind.clone(),
                thousands(c.parts as i64),
                thousands(c.carded_parts as i64),
                thousands(c.uses),
                pct(c.carded_uses, c.uses),
            ]
        })
        .collect();
    rows.push(vec![
        "all".into(),
        thousands(parts as i64),
        thousands(carded as i64),
        thousands(uses),
        pct(carded_uses, uses),
    ]);
    println!("{}", render(format, &header, &rows));
    if !unresolved.is_empty() {
        println!("Cards naming no manifest item: {}", unresolved.join(", "));
    }
    if top > 0 {
        println!("## The most-used parts without a card");
        let header = ["kind", "part", "uses"].map(String::from);
        let rows: Vec<Vec<String>> = uncarded
            .iter()
            .take(top)
            .map(|u| vec![u.kind.clone(), u.name.clone(), thousands(u.uses)])
            .collect();
        println!("{}", render(format, &header, &rows));
    }
    Ok(())
}
