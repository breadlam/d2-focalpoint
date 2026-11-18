//! `searchlight info` — what is in the database.

use std::path::Path;

use anyhow::Result;
use duckdb::Connection;
use searchlight_db::Database;

use crate::commands::sql::query_strings;
use crate::output::{Format, render, thousands};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Also show per-table row counts and import statistics
    #[arg(long)]
    pub detailed: bool,
}

/// Headline facts: (label, SQL returning one value).
const FACTS: &[(&str, &str)] = &[
    ("runs", "SELECT count(*) FROM run"),
    (
        "run date range",
        "SELECT min(started_at)::DATE || ' → ' || max(started_at)::DATE FROM run",
    ),
    (
        "runs from raw PGCRs",
        "SELECT count(*) FILTER (WHERE source = 'archive') FROM run",
    ),
    (
        "curated target runs",
        "SELECT count(*) FROM run r JOIN activity a USING (activity_hash) WHERE a.target_group IS NOT NULL",
    ),
    ("player-run rows", "SELECT count(*) FROM run_player"),
    ("players", "SELECT count(*) FROM player"),
    (
        "build timelines (player-runs)",
        "SELECT count(*) FROM (SELECT DISTINCT instance_id, membership_id FROM segment)",
    ),
    ("build segments", "SELECT count(*) FROM segment"),
    ("distinct builds", "SELECT count(*) FROM build"),
    (
        "players with builds",
        "SELECT count(*) FROM player WHERE build_timeline_count > 0",
    ),
    (
        "build date range",
        "SELECT min(r.started_at)::DATE || ' → ' || max(r.started_at)::DATE FROM run r WHERE r.instance_id IN (SELECT instance_id FROM segment)",
    ),
    (
        "activity history rows",
        "SELECT count(*) FROM activity_history",
    ),
];

pub fn run(args: &Args, path: &Path) -> Result<()> {
    let db = Database::open_read_only(path)?;
    let conn = db.conn();
    println!("Database: {}", path.display());
    for (k, v) in db.all_meta()? {
        println!("  {k:<28} {v}");
    }

    println!("\nContents:");
    for (label, sql) in FACTS {
        let v = scalar(conn, sql)?;
        println!("  {label:<30} {v}");
    }

    section(
        conn,
        "Build timelines by target group",
        "SELECT coalesce(a.target_group, '(not a target)') AS target_group, count(*) AS timelines, \
                count(DISTINCT p.membership_id) AS players \
         FROM run_player p JOIN activity a USING (activity_hash) \
         WHERE p.has_build GROUP BY 1 ORDER BY 2 DESC",
    )?;
    section(
        conn,
        "Builds by class and element",
        "SELECT class, element, count(*) AS builds FROM build GROUP BY ALL ORDER BY class, builds DESC",
    )?;

    if args.detailed {
        section(
            conn,
            "Tables",
            "SELECT table_name, estimated_size AS approx_rows FROM duckdb_tables() ORDER BY 1",
        )?;
        section(
            conn,
            "Import statistics",
            "SELECT stage, metric, value FROM import_stat ORDER BY 1, 2",
        )?;
    }
    Ok(())
}

fn scalar(conn: &Connection, sql: &str) -> Result<String> {
    let v: Option<duckdb::types::Value> = conn.query_row(sql, [], |r| r.get(0))?;
    Ok(match v {
        Some(duckdb::types::Value::BigInt(n)) => thousands(n),
        Some(duckdb::types::Value::Text(s)) => s,
        Some(other) => format!("{other:?}"),
        None => "—".into(),
    })
}

fn section(conn: &Connection, title: &str, sql: &str) -> Result<()> {
    let (header, rows, _) = query_strings(conn, sql, 50)?;
    println!("\n{title}:\n{}", render(Format::Table, &header, &rows));
    Ok(())
}
