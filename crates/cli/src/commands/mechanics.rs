//! `focalpoint mechanics` — the community's documentation of game mechanics
//! as a layer of the database (`docs/mechanics.md`).

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use focalpoint_db::Database;

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
    /// Read the raw sources into the `mechanic_*` tables, replacing them
    #[command(
        after_help = "The sources are raw downloads kept outside the repository, one folder per source \
with a manifest.json (see docs/mechanics.md). Prints what was read and how much of the recorded builds' \
components each source documents."
    )]
    Import(ImportArgs),
}

#[derive(Debug, clap::Args)]
pub struct ImportArgs {
    /// Folder of raw sources [default: focalpoint/sources in the user data dir]
    #[arg(long)]
    pub sources: Option<PathBuf>,
}

pub fn run(args: &Args, db: &Path) -> Result<()> {
    match &args.what {
        What::Import(a) => import(a, db, args.format),
    }
}

/// `sources/` beside the default database.
fn default_sources() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("focalpoint")
        .join("sources")
}

fn import(a: &ImportArgs, db: &Path, format: Format) -> Result<()> {
    let dir = a.sources.clone().unwrap_or_else(default_sources);
    let db = Database::open_for_update(db)?;
    let started = Instant::now();
    let summaries = focalpoint_mechanics::import(&db, &dir, |s| eprintln!("  {s}"))
        .with_context(|| format!("importing the sources in {}", dir.display()))?;
    println!(
        "Read {} sources from {} in {:.1}s",
        summaries.len(),
        dir.display(),
        started.elapsed().as_secs_f64()
    );
    let header = [
        "source",
        "fetched",
        "entries",
        "with a subject",
        "subjects",
        "keywords",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = summaries
        .iter()
        .map(|s| {
            vec![
                format!("{} ({})", s.source, s.provenance.title),
                s.provenance.fetched.clone().unwrap_or_default(),
                thousands(s.entries as i64),
                thousands(s.with_subject as i64),
                thousands(s.subjects as i64),
                thousands(s.keywords as i64),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));

    println!("\nShare of the recorded builds' component uses each source documents:");
    let mut stmt = db.conn().prepare(
        "WITH used AS (
             SELECT bc.kind, bc.component_hash AS h, sum(b.timeline_count) AS uses
             FROM build_component bc JOIN build b USING (build_id)
             WHERE bc.kind NOT IN ('armor', 'weapon', 'armor_stat_mod', 'subclass', 'artifact')
             GROUP BY ALL),
         doc AS (SELECT DISTINCT source, hash FROM mechanic_subject)
         SELECT kind, count(*),
                sum(uses) FILTER (WHERE h IN (SELECT hash FROM doc WHERE source = 'clarity')) / sum(uses),
                sum(uses) FILTER (WHERE h IN (SELECT hash FROM doc WHERE source = 'compendium')) / sum(uses),
                sum(uses) FILTER (WHERE h IN (SELECT hash FROM doc)) / sum(uses)
         FROM used GROUP BY kind ORDER BY sum(uses) DESC",
    )?;
    let pct = |x: Option<f64>| x.map_or("0%".to_string(), |x| format!("{:.0}%", 100.0 * x));
    let rows: Vec<Vec<String>> = stmt
        .query_map([], |r| {
            Ok(vec![
                r.get::<_, String>(0)?,
                thousands(r.get::<_, i64>(1)?),
                pct(r.get(2)?),
                pct(r.get(3)?),
                pct(r.get(4)?),
            ])
        })?
        .collect::<duckdb::Result<_>>()?;
    let header = ["component", "distinct", "Clarity", "Compendium", "either"].map(String::from);
    println!("{}", render(format, &header, &rows));
    Ok(())
}
