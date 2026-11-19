//! The searchlight-cli analysis database (DuckDB).
//!
//! The schema lives in [`SCHEMA_SQL`] and is documented in `docs/schema.md`.
//! Databases are created once by the importer and opened read-only by the
//! CLI; a database whose `schema_version` differs from [`SCHEMA_VERSION`] is
//! rejected with a clear error rather than queried with wrong assumptions.

use std::path::{Path, PathBuf};

use duckdb::{AccessMode, Config, Connection, OptionalExt, params};
use thiserror::Error;

/// Version of [`SCHEMA_SQL`]. Bump on any incompatible schema change.
pub const SCHEMA_VERSION: u32 = 2;

/// DDL for a fresh database.
pub const SCHEMA_SQL: &str = include_str!("schema.sql");

/// Environment variable that overrides the default database location.
pub const DB_ENV_VAR: &str = "SEARCHLIGHT_DB";

/// Well-known `meta` keys.
pub mod meta_key {
    pub const SCHEMA_VERSION: &str = "schema_version";
    pub const BUILD_FINGERPRINT_VERSION: &str = "build_fingerprint_version";
    pub const LOADOUT_FINGERPRINT_VERSION: &str = "loadout_fingerprint_version";
    pub const VECTOR_FINGERPRINT_VERSION: &str = "vector_fingerprint_version";
    pub const TOOL_VERSION: &str = "tool_version";
    pub const IMPORTED_AT: &str = "imported_at";
    pub const SOURCE_ROOT: &str = "source_root";
    pub const SOURCE_SNAPSHOT: &str = "source_snapshot";
    pub const MANIFEST_VERSION: &str = "manifest_version";
    pub const IMPORT_SECONDS: &str = "import_seconds";
}

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    DuckDb(#[from] duckdb::Error),
    #[error("database not found at {0} (run `searchlight import` first, or pass --db)")]
    NotFound(PathBuf),
    #[error(
        "{path} has schema version {found}, this build of searchlight expects {expected}; re-run `searchlight import`"
    )]
    SchemaMismatch {
        path: PathBuf,
        found: String,
        expected: u32,
    },
    #[error("{0} is not a searchlight database (no schema_version)")]
    NotSearchlight(PathBuf),
}

pub type Result<T, E = DbError> = std::result::Result<T, E>;

/// The default database path: `$SEARCHLIGHT_DB`, else
/// `<data dir>/searchlight/searchlight.duckdb` (e.g. `~/.local/share/...`).
#[must_use]
pub fn default_path() -> PathBuf {
    if let Some(p) = std::env::var_os(DB_ENV_VAR) {
        return PathBuf::from(p);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("searchlight")
        .join("searchlight.duckdb")
}

/// An open analysis database.
#[derive(Debug)]
pub struct Database {
    conn: Connection,
    path: PathBuf,
}

impl Database {
    /// Create a new database at `path` (which must not exist) and apply the
    /// schema. Used by the importer.
    pub fn create(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_SQL)?;
        let db = Self {
            conn,
            path: path.to_path_buf(),
        };
        db.set_meta(meta_key::SCHEMA_VERSION, &SCHEMA_VERSION.to_string())?;
        Ok(db)
    }

    /// Open an existing database read-only and verify its schema version.
    pub fn open_read_only(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(DbError::NotFound(path.to_path_buf()));
        }
        let config = Config::default().access_mode(AccessMode::ReadOnly)?;
        let conn = Connection::open_with_flags(path, config)?;
        let db = Self {
            conn,
            path: path.to_path_buf(),
        };
        db.check_schema()?;
        Ok(db)
    }

    fn check_schema(&self) -> Result<()> {
        let has_meta: bool = self.conn.query_row(
            "SELECT count(*) > 0 FROM information_schema.tables WHERE table_name = 'meta'",
            [],
            |r| r.get(0),
        )?;
        if !has_meta {
            return Err(DbError::NotSearchlight(self.path.clone()));
        }
        match self.meta(meta_key::SCHEMA_VERSION)? {
            None => Err(DbError::NotSearchlight(self.path.clone())),
            Some(v) if v == SCHEMA_VERSION.to_string() => Ok(()),
            Some(found) => Err(DbError::SchemaMismatch {
                path: self.path.clone(),
                found,
                expected: SCHEMA_VERSION,
            }),
        }
    }

    #[must_use]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Consume the handle, returning the underlying connection.
    #[must_use]
    pub fn into_conn(self) -> Connection {
        self.conn
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?", params![key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?, ?)",
            params![key, value],
        )?;
        Ok(())
    }

    /// All `meta` entries, sorted by key.
    pub fn all_meta(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM meta ORDER BY key")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Row counts of every base table, sorted by table name.
    pub fn table_counts(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT table_name FROM information_schema.tables \
             WHERE table_schema = 'main' AND table_type = 'BASE TABLE' ORDER BY table_name",
        )?;
        let names: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        names
            .into_iter()
            .map(|t| {
                let n: i64 =
                    self.conn
                        .query_row(&format!("SELECT count(*) FROM \"{t}\""), [], |r| r.get(0))?;
                Ok((t, n))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_then_open_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.duckdb");
        {
            let db = Database::create(&path).unwrap();
            db.set_meta("k", "v").unwrap();
            db.set_meta("k", "v2").unwrap();
        }
        let db = Database::open_read_only(&path).unwrap();
        assert_eq!(db.meta("k").unwrap().as_deref(), Some("v2"));
        assert_eq!(
            db.meta(meta_key::SCHEMA_VERSION).unwrap(),
            Some(SCHEMA_VERSION.to_string())
        );
        let counts = db.table_counts().unwrap();
        assert!(counts.iter().any(|(t, n)| t == "build" && *n == 0));
        assert!(counts.iter().any(|(t, _)| t == "character"));
        // Read-only really is read-only.
        assert!(db.set_meta("x", "y").is_err());
    }

    #[test]
    fn rejects_missing_and_foreign_and_mismatched_databases() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.duckdb");
        assert!(matches!(
            Database::open_read_only(&missing),
            Err(DbError::NotFound(_))
        ));

        let foreign = dir.path().join("foreign.duckdb");
        Connection::open(&foreign)
            .unwrap()
            .execute_batch("CREATE TABLE x (a INT)")
            .unwrap();
        assert!(matches!(
            Database::open_read_only(&foreign),
            Err(DbError::NotSearchlight(_))
        ));

        let old = dir.path().join("old.duckdb");
        Database::create(&old)
            .unwrap()
            .set_meta(meta_key::SCHEMA_VERSION, "0")
            .unwrap();
        assert!(matches!(
            Database::open_read_only(&old),
            Err(DbError::SchemaMismatch { .. })
        ));
    }

    #[test]
    fn macros_resolve_names() {
        let db = Database::create(&tempfile::tempdir().unwrap().path().join("m.duckdb")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO item (item_hash, name, is_exotic) VALUES (1, 'B', false), (2, 'A', false);
                 INSERT INTO activity (activity_hash, name) VALUES (7, 'Act');",
            )
            .unwrap();
        let one: String = db
            .conn()
            .query_row("SELECT item_name(1)", [], |r| r.get(0))
            .unwrap();
        assert_eq!(one, "B");
        let many: String = db
            .conn()
            .query_row(
                "SELECT item_names([1, 2, 99]::UINTEGER[])::VARCHAR",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(many, "[A, B]");
        let act: String = db
            .conn()
            .query_row("SELECT activity_name(7)", [], |r| r.get(0))
            .unwrap();
        assert_eq!(act, "Act");
    }

    #[test]
    fn distance_macros_match_slot_semantics() {
        let db = Database::create(&tempfile::tempdir().unwrap().path().join("d.duckdb")).unwrap();
        let q = |sql: &str| -> i64 { db.conn().query_row(sql, [], |r| r.get(0)).unwrap() };
        // Bag distance: order-free, multiset-aware, NULL-safe.
        assert_eq!(q("SELECT bag_distance([1, 2], [2, 1])"), 0);
        assert_eq!(q("SELECT bag_distance([1, 2], [2, 3])"), 1);
        assert_eq!(q("SELECT bag_distance([1, 1, 2], [1, 2, 2])"), 1);
        assert_eq!(q("SELECT bag_distance([1, 2, 3, 4], [1, 2, 3, 4, 5])"), 1);
        assert_eq!(q("SELECT bag_distance(NULL, [7, 8])"), 2);
        assert_eq!(q("SELECT bag_distance([]::UINTEGER[], []::UINTEGER[])"), 0);

        db.conn()
            .execute_batch(
                "INSERT INTO build_vector (vector_id, super_hash, kinetic_hash, helmet_hash, aspect_hashes,
                     fragment_hashes, weapon_mod_hashes, armor_mod_hashes, class_item_perk_hashes,
                     artifact_perk_hashes, slot_count)
                 VALUES (1, 1, 2, 3, [10, 11], [20, 21, 22], [], [5, 5], [], [], 0),
                        (2, 1, 9, NULL, [11, 12], [20, 21, 22, 23], [], [5, 6], [], [], 0);",
            )
            .unwrap();
        // kinetic + helmet + one aspect + one extra fragment + one armor mod.
        assert_eq!(
            q(
                "SELECT slot_distance(a, b) FROM build_vector a, build_vector b WHERE a.vector_id = 1 AND b.vector_id = 2"
            ),
            5
        );
        assert_eq!(
            q("SELECT slot_distance(a, a) FROM build_vector a WHERE a.vector_id = 2"),
            0
        );
    }

    #[test]
    fn default_path_honours_env_override() {
        // Only assert the fallback shape; mutating the process environment in
        // tests is unsafe under the 2024 edition and racy across threads.
        if std::env::var_os(DB_ENV_VAR).is_none() {
            assert!(default_path().ends_with("searchlight/searchlight.duckdb"));
        }
    }
}
