//! `focalpoint sql` — ad-hoc SQL against the (read-only) database.

use std::path::Path;

use anyhow::{Context, Result};
use duckdb::Connection;
use focalpoint_db::Database;

use crate::output::{Format, render};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// SQL to run (DuckDB dialect). The database is opened read-only.
    pub query: String,

    /// Maximum rows to print
    #[arg(long, default_value_t = 100)]
    pub max_rows: usize,

    #[arg(long, value_enum, default_value_t)]
    pub format: Format,
}

pub fn run(args: &Args, db: &Path) -> Result<()> {
    let db = Database::open_read_only(db)?;
    let (header, rows, truncated) = query_strings(db.conn(), &args.query, args.max_rows)?;
    println!("{}", render(args.format, &header, &rows));
    if truncated {
        eprintln!(
            "(showing first {} rows; use --max-rows to see more)",
            args.max_rows
        );
    }
    Ok(())
}

/// Run `sql` and return its rows rendered as strings using DuckDB's own
/// formatting (every column cast to VARCHAR). Returns whether rows were cut off.
pub fn query_strings(
    conn: &Connection,
    sql: &str,
    max_rows: usize,
) -> Result<(Vec<String>, Vec<Vec<String>>, bool)> {
    let trimmed = sql.trim().trim_end_matches(';');
    // Wrapping lets DuckDB format every type (lists, timestamps, structs).
    // Statements that cannot be wrapped (SHOW, DESCRIBE, PRAGMA, ...) are
    // first materialised as a table function result.
    let wrapped = format!(
        "SELECT COLUMNS(*)::VARCHAR FROM ({trimmed}) LIMIT {}",
        max_rows + 1
    );
    let mut stmt = match conn.prepare(&wrapped) {
        Ok(s) => s,
        Err(_) => conn
            .prepare(&format!(
                "SELECT COLUMNS(*)::VARCHAR FROM query('{}') LIMIT {}",
                trimmed.replace('\'', "''"),
                max_rows + 1
            ))
            .with_context(|| format!("preparing query: {trimmed}"))?,
    };
    let mut rows_iter = stmt.query([])?;
    let mut rows = Vec::new();
    while let Some(r) = rows_iter.next()? {
        let n = r.as_ref().column_count();
        let mut row = Vec::with_capacity(n);
        for i in 0..n {
            row.push(
                r.get::<_, Option<String>>(i)?
                    .unwrap_or_else(|| "NULL".into()),
            );
        }
        rows.push(row);
    }
    drop(rows_iter);
    let header = stmt.column_names();
    let truncated = rows.len() > max_rows;
    rows.truncate(max_rows);
    Ok((header, rows, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_lists_timestamps_and_nulls() {
        let c = Connection::open_in_memory().unwrap();
        let (h, rows, truncated) = query_strings(
            &c,
            "SELECT [1, 2]::UINTEGER[] AS l, TIMESTAMP '2026-06-09 00:00:47' AS t, NULL AS n;",
            10,
        )
        .unwrap();
        assert_eq!(h, ["l", "t", "n"]);
        assert_eq!(
            rows,
            vec![vec![
                "[1, 2]".to_string(),
                "2026-06-09 00:00:47".into(),
                "NULL".into()
            ]]
        );
        assert!(!truncated);
    }

    #[test]
    fn truncates_and_reports() {
        let c = Connection::open_in_memory().unwrap();
        let (_, rows, truncated) = query_strings(&c, "SELECT * FROM range(10)", 3).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(truncated);
    }

    #[test]
    fn handles_statements_that_cannot_be_subqueries() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE t (a INT)").unwrap();
        let (_, rows, _) = query_strings(&c, "SHOW TABLES", 10).unwrap();
        assert_eq!(rows, vec![vec!["t".to_string()]]);
    }
}
