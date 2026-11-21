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
    /// Show context profiles: every context, or those whose id or name contains QUERY
    #[command(after_help = "Examples:\n  \
focalpoint mechanics context                    # every context, briefly\n  \
focalpoint mechanics context atheon             # an encounter's profile\n  \
focalpoint mechanics context group:master-raid")]
    Context(ContextArgs),
}

#[derive(Debug, clap::Args)]
pub struct ImportArgs {
    /// Folder of raw sources [default: focalpoint/sources in the user data dir]
    #[arg(long)]
    pub sources: Option<PathBuf>,

    /// Hand-maintained context detail [default: the built-in data/contexts.toml]
    #[arg(long)]
    pub contexts: Option<PathBuf>,

    /// Power delta and health tier by activity group [default: the built-in data/difficulty.toml]
    #[arg(long)]
    pub difficulty: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
pub struct ContextArgs {
    /// Part of a context's id or name (case-insensitive)
    pub query: Option<String>,
}

pub fn run(args: &Args, db: &Path) -> Result<()> {
    match &args.what {
        What::Import(a) => import(a, db, args.format),
        What::Context(a) => context(a, db, args.format),
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
    let imported = focalpoint_mechanics::import(
        &db,
        &dir,
        &focalpoint_mechanics::Options {
            contexts: a.contexts.clone(),
            difficulty: a.difficulty.clone(),
        },
        |s| eprintln!("  {s}"),
    )
    .with_context(|| format!("importing the sources in {}", dir.display()))?;
    let summaries = &imported.sources;
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
        "rotations",
        "loadouts",
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
                thousands(s.rotations as i64),
                if s.missing_loadouts > 0 {
                    format!(
                        "{} ({} missing)",
                        thousands(s.loadouts as i64),
                        s.missing_loadouts
                    )
                } else {
                    thousands(s.loadouts as i64)
                },
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
    println!(
        "\nContext profiles: {} ({} encounters), {} features, {} timed events",
        thousands(imported.contexts as i64),
        imported.encounters,
        thousands(imported.features as i64),
        imported.events
    );
    let d = &imported.durability;
    println!(
        "Enemy durability: {} enemy archetypes ({} health values), {} measured bosses and minibosses, {} points of the Power delta curve, {} Champion values; {} contexts given a Power delta and health tier",
        d.enemies, d.health_rows, d.measured, d.curve_points, d.champion_stats, d.contexts
    );
    if !imported.unplaced.is_empty() {
        println!(
            "Encounters of activities without scored clears (no parent): {}",
            imported.unplaced.join(", ")
        );
    }
    Ok(())
}

fn context(a: &ContextArgs, db: &Path, format: Format) -> Result<()> {
    let db = Database::open_read_only(db)?;
    let query = a.query.as_deref().unwrap_or("").to_lowercase();
    let mut stmt = db.conn().prepare(
        "SELECT c.context_id, c.kind, coalesce(c.parent_id, ''), c.name,
                (SELECT count(*) FROM context_feature f WHERE f.context_id = c.context_id),
                (SELECT count(*) FROM context_event e WHERE e.context_id = c.context_id)
         FROM context c
         WHERE strpos(lower(c.context_id), ?) > 0 OR strpos(lower(c.name), ?) > 0
         ORDER BY CASE c.kind WHEN 'group' THEN 0 WHEN 'activity' THEN 1 ELSE 2 END, c.context_id",
    )?;
    let found: Vec<(String, String, String, String, i64, i64)> = stmt
        .query_map([&query, &query], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<duckdb::Result<_>>()
        .context("reading contexts (run `focalpoint mechanics import` first)")?;
    if found.len() != 1 {
        let header = ["context", "kind", "parent", "name", "features", "events"].map(String::from);
        let rows: Vec<Vec<String>> = found
            .into_iter()
            .map(|(id, kind, parent, name, f, e)| {
                vec![id, kind, parent, name, f.to_string(), e.to_string()]
            })
            .collect();
        println!("{}", render(format, &header, &rows));
        return Ok(());
    }
    let (id, kind, parent, name, _, _) = &found[0];
    println!(
        "{name}  ({kind} {id}{})",
        if parent.is_empty() {
            String::new()
        } else {
            format!(", in {parent}")
        }
    );
    let mut stmt = db.conn().prepare(
        "SELECT feature, value, coalesce(text, ''), source FROM context_feature WHERE context_id = ?
         ORDER BY feature, source",
    )?;
    let rows: Vec<Vec<String>> = stmt
        .query_map([id], |r| {
            let value: Option<f64> = r.get(1)?;
            Ok(vec![
                r.get(0)?,
                value.map_or(String::new(), |v| {
                    if v.fract() == 0.0 && v.abs() < 1e15 {
                        format!("{v:.0}")
                    } else {
                        format!("{v:.3}")
                    }
                }),
                r.get::<_, String>(2)?.replace('\n', " / "),
                r.get(3)?,
            ])
        })?
        .collect::<duckdb::Result<_>>()?;
    let header = ["feature", "value", "text", "source"].map(String::from);
    println!("{}", render(format, &header, &rows));
    let mut stmt = db.conn().prepare(
        "SELECT kind, start_s, length_s, coalesce(detail, ''), source FROM context_event WHERE context_id = ? ORDER BY seq",
    )?;
    let secs = |v: Option<f64>| v.map_or(String::new(), |v| format!("{v:.1}"));
    let rows: Vec<Vec<String>> = stmt
        .query_map([id], |r| {
            Ok(vec![
                r.get(0)?,
                secs(r.get(1)?),
                secs(r.get(2)?),
                r.get(3)?,
                r.get(4)?,
            ])
        })?
        .collect::<duckdb::Result<_>>()?;
    if !rows.is_empty() {
        let header = ["event", "start s", "length s", "detail", "source"].map(String::from);
        println!("{}", render(format, &header, &rows));
    }
    Ok(())
}
