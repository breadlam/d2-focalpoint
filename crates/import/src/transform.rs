//! Runs `sql/transform.sql` (staging → final tables) step by step.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use duckdb::Connection;

use crate::progress::Progress;

const TRANSFORM_SQL: &str = include_str!("sql/transform.sql");
const FINALIZE_SQL: &str = include_str!("sql/finalize.sql");
const STEP_MARKER: &str = "-- step: ";

/// Split a script into `(step name, sql)` pairs on lines starting with
/// `-- step: <name>`. Anything before the first marker is a preamble and is
/// ignored; the marker text elsewhere in a line (e.g. in a comment) is not a
/// step boundary.
fn steps(script: &str) -> Vec<(&str, String)> {
    let mut out: Vec<(&str, String)> = Vec::new();
    for line in script.lines() {
        if let Some(name) = line.strip_prefix(STEP_MARKER) {
            out.push((name.trim(), String::new()));
        } else if let Some((_, body)) = out.last_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    for (_, body) in &mut out {
        *body = body.trim().to_string();
    }
    out
}

/// Execute every transform step, returning per-step timings.
pub fn run(conn: &Connection, progress: &Progress) -> Result<Vec<(String, Duration)>> {
    run_script(conn, TRANSFORM_SQL, progress)
}

/// Execute the finalize steps (after the investment stage).
pub fn finalize(conn: &Connection, progress: &Progress) -> Result<Vec<(String, Duration)>> {
    run_script(conn, FINALIZE_SQL, progress)
}

fn run_script(
    conn: &Connection,
    script: &str,
    progress: &Progress,
) -> Result<Vec<(String, Duration)>> {
    let mut timings = Vec::new();
    for (name, sql) in steps(script) {
        let spinner = progress.spinner("transform");
        spinner.set_message(name.to_string());
        let t = Instant::now();
        conn.execute_batch(&sql)
            .with_context(|| format!("transform step '{name}'"))?;
        spinner.finish_with_message(format!("{name} done"));
        timings.push((name.to_string(), t.elapsed()));
    }
    Ok(timings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_step_markers_at_line_start_only() {
        let s = "-- preamble mentions `-- step: x` inline\n-- step: one\nSELECT 1;\n\n-- step: two words\nSELECT 2;\nSELECT 3;\n";
        let st = steps(s);
        assert_eq!(
            st,
            vec![
                ("one", "SELECT 1;".to_string()),
                ("two words", "SELECT 2;\nSELECT 3;".to_string())
            ]
        );
    }

    #[test]
    fn real_script_has_expected_steps() {
        let names: Vec<_> = steps(TRANSFORM_SQL).into_iter().map(|(n, _)| n).collect();
        for expected in [
            "run",
            "run_player",
            "run_player_weapon",
            "activity_history",
            "run_build",
            "activity",
            "player",
            "character",
        ] {
            assert!(
                names.contains(&expected),
                "missing step {expected}: {names:?}"
            );
        }
    }
}
