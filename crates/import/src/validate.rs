//! Post-transform validation.
//!
//! *Errors* are invariants the importer guarantees (natural keys unique, every
//! row linked to its run); any violation is a bug and fails the import.
//! *Warnings* describe known gaps in the source data and are recorded.
//! *Info* checks record cross-source consistency (archive vs tracked parsing).

use std::fmt;

use anyhow::Result;
use duckdb::Connection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub name: &'static str,
    pub severity: Severity,
    pub value: i64,
    pub description: &'static str,
}

impl Check {
    /// An error/warning check passes when its value is zero.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.severity == Severity::Info || self.value == 0
    }
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag = match (self.severity, self.passed()) {
            (Severity::Info, _) => "info",
            (_, true) => "ok",
            (Severity::Error, false) => "ERROR",
            (Severity::Warning, false) => "warn",
        };
        write!(
            f,
            "[{tag:>5}] {:<32} {:>12}  {}",
            self.name, self.value, self.description
        )
    }
}

const CHECKS: &[(&str, Severity, &str, &str)] = &[
    (
        "dup_run",
        Severity::Error,
        "duplicate run instance ids",
        "SELECT count(*) - count(DISTINCT instance_id) FROM run",
    ),
    (
        "dup_run_player",
        Severity::Error,
        "duplicate (instance, membership, character)",
        "SELECT count(*) - (SELECT count(*) FROM (SELECT DISTINCT instance_id, membership_id, character_id FROM run_player)) FROM run_player",
    ),
    (
        "dup_segment",
        Severity::Error,
        "duplicate (instance, membership, segment_index)",
        "SELECT count(*) - (SELECT count(*) FROM (SELECT DISTINCT instance_id, membership_id, segment_index FROM segment)) FROM segment",
    ),
    (
        "dup_activity_history",
        Severity::Error,
        "duplicate (membership, instance)",
        "SELECT count(*) - (SELECT count(*) FROM (SELECT DISTINCT membership_id, instance_id FROM activity_history)) FROM activity_history",
    ),
    (
        "run_player_orphans",
        Severity::Error,
        "run_player rows without a run",
        "SELECT count(*) FROM run_player p WHERE NOT EXISTS (SELECT 1 FROM run r WHERE r.instance_id = p.instance_id)",
    ),
    (
        "segment_unknown_build",
        Severity::Error,
        "segments whose build is missing",
        "SELECT count(*) FROM segment s WHERE NOT EXISTS (SELECT 1 FROM build b WHERE b.build_id = s.build_id)",
    ),
    (
        "timeline_without_run",
        Severity::Warning,
        "build timelines whose run is unknown",
        "SELECT count(*) FROM (SELECT DISTINCT instance_id FROM segment) s WHERE NOT EXISTS (SELECT 1 FROM run r WHERE r.instance_id = s.instance_id)",
    ),
    (
        "timeline_without_player_row",
        Severity::Warning,
        "build timelines with no run_player row",
        "SELECT count(*) FROM (SELECT DISTINCT instance_id, membership_id FROM segment) s WHERE NOT EXISTS (SELECT 1 FROM run_player p WHERE p.instance_id = s.instance_id AND p.membership_id = s.membership_id)",
    ),
    (
        "runs_without_players",
        Severity::Warning,
        "runs whose report lists no players",
        "SELECT count(*) FROM run WHERE roster_size = 0",
    ),
    (
        "unparseable_periods",
        Severity::Warning,
        "staged runs dropped for an invalid timestamp",
        "SELECT (SELECT count(DISTINCT instance_id) FROM stg.run) - (SELECT count(*) FROM run)",
    ),
    (
        "unknown_weapon_hashes",
        Severity::Warning,
        "distinct weapon hashes not in the manifest",
        "SELECT count(DISTINCT item_hash) FROM run_player_weapon w WHERE NOT EXISTS (SELECT 1 FROM item i WHERE i.item_hash = w.item_hash)",
    ),
    (
        "unknown_component_hashes",
        Severity::Warning,
        "distinct build item hashes not in the manifest",
        "SELECT count(DISTINCT component_hash) FROM build_component c WHERE kind NOT IN ('armor_set', 'set_bonus') AND NOT EXISTS (SELECT 1 FROM item i WHERE i.item_hash = c.component_hash)",
    ),
    (
        "unnamed_activities",
        Severity::Warning,
        "activities seen in data but not in the manifest",
        "SELECT count(*) FROM activity WHERE name IS NULL AND run_count > 0",
    ),
    (
        "tracked_archive_overlap",
        Severity::Info,
        "player rows parsed from both sources",
        "SELECT count(*) FROM stg.entry t JOIN stg.entry a ON a.src = 0 AND t.src = 1 AND a.instance_id = t.instance_id AND a.membership_id = t.membership_id AND a.character_id = t.character_id",
    ),
    (
        "tracked_archive_mismatch",
        Severity::Info,
        "of those, rows whose stats disagree",
        "SELECT count(*) FROM stg.entry t JOIN stg.entry a ON a.src = 0 AND t.src = 1 AND a.instance_id = t.instance_id AND a.membership_id = t.membership_id AND a.character_id = t.character_id \
      WHERE (a.kills, a.deaths, a.assists, a.precision_kills, a.super_kills, a.melee_kills, a.grenade_kills, a.ability_kills, a.completed, a.completion_reason, a.time_played_s) \
         IS DISTINCT FROM (t.kills, t.deaths, t.assists, t.precision_kills, t.super_kills, t.melee_kills, t.grenade_kills, t.ability_kills, t.completed, t.completion_reason, t.time_played_s)",
    ),
];

/// Run all checks. Must run before staging tables are dropped.
pub fn run(conn: &Connection) -> Result<Vec<Check>> {
    CHECKS
        .iter()
        .map(|&(name, severity, description, sql)| {
            let value: i64 = conn.query_row(sql, [], |r| r.get(0))?;
            Ok(Check {
                name,
                severity,
                value,
                description,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh database with an in-memory staging catalog, as the importer
    /// sets it up.
    fn staged_db(dir: &std::path::Path) -> searchlight_db::Database {
        let db = searchlight_db::Database::create(&dir.join("v.duckdb")).unwrap();
        db.conn()
            .execute_batch("ATTACH ':memory:' AS stg;")
            .unwrap();
        db.conn()
            .execute_batch(include_str!("sql/staging.sql"))
            .unwrap();
        db
    }

    #[test]
    fn checks_run_against_an_empty_database() {
        let dir = tempfile::tempdir().unwrap();
        let db = staged_db(dir.path());
        let checks = run(db.conn()).unwrap();
        assert_eq!(checks.len(), CHECKS.len());
        assert!(checks.iter().all(Check::passed), "{checks:#?}");
    }

    #[test]
    fn detects_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let db = staged_db(dir.path());
        db.conn()
            .execute_batch(
                "INSERT INTO run (instance_id, started_at, activity_hash, source, is_tracked)
                 VALUES (1, '2026-01-01', 5, 'archive', false), (1, '2026-01-01', 5, 'archive', false);",
            )
            .unwrap();
        let checks = run(db.conn()).unwrap();
        let dup = checks.iter().find(|c| c.name == "dup_run").unwrap();
        assert_eq!(dup.value, 1);
        assert!(!dup.passed());
    }
}
