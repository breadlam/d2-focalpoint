//! Builds the searchlight-cli analysis database from Searchlight backups.
//!
//! The import is a from-scratch rebuild (the sources are static backups):
//!
//! 1. **stage** — copy live SQLite DBs (+WAL) and decompress shards into a
//!    local work directory ([`stage`]); the backups are never opened in place;
//! 2. **extract** — parallel readers fill DuckDB staging tables from the
//!    manifest, production's tracked tables, the raw PGCR archive, the
//!    activity history, and the build timelines;
//! 3. **transform** — SQL merges, dedupes and reshapes staging into the final
//!    schema (`sql/transform.sql`);
//! 4. **validate** — invariant checks; any error aborts the import;
//! 5. **publish** — staging is dropped and the finished database is moved
//!    into place atomically.
//!
//! See `docs/importing.md` for operating instructions.

pub mod archive;
pub mod builds;
pub mod history;
pub mod layout;
pub mod manifest;
pub mod progress;
pub mod rows;
pub mod sink;
pub mod sqlite;
pub mod stage;
pub mod tracked;
pub mod transform;
pub mod validate;

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use searchlight_core::build::{BUILD_FINGERPRINT_VERSION, LOADOUT_FINGERPRINT_VERSION};
use searchlight_db::{Database, meta_key};

pub use layout::SourceLayout;
pub use progress::Progress;
use validate::{Check, Severity};

const STAGING_SQL: &str = include_str!("sql/staging.sql");
const STAGING_TABLES: &[&str] = &[
    "stg_run",
    "stg_entry",
    "stg_weapon",
    "stg_history",
    "stg_player",
    "stg_player_flag",
    "stg_target_group",
    "stg_activity_def",
];

/// Optional heavy stages that can be skipped for faster development runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OptionalStage {
    /// Raw PGCR archive (~17.7M reports).
    Archive,
    /// All-modes activity history (~250M rows before dedupe).
    History,
}

impl OptionalStage {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Archive => "archive",
            Self::History => "history",
        }
    }
}

/// Import configuration.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    /// Root of the Searchlight backups (contains `builds.rodeo/`, `shards/`).
    pub backups_root: PathBuf,
    /// Server snapshot directory name; defaults to the latest.
    pub snapshot: Option<String>,
    /// Where to publish the finished database.
    pub out: PathBuf,
    /// Local scratch space for staged sources and the database being built.
    pub work_dir: PathBuf,
    /// Replace `out` if it exists.
    pub force: bool,
    /// Worker threads (default: all cores).
    pub threads: Option<usize>,
    /// DuckDB memory limit, e.g. "64GB" (default: DuckDB's 80% of RAM).
    pub memory_limit: Option<String>,
    pub skip: Vec<OptionalStage>,
}

/// What an import did.
#[derive(Debug)]
pub struct ImportReport {
    pub out: PathBuf,
    pub elapsed: Duration,
    pub snapshot: String,
    pub manifest_version: Option<String>,
    pub tracked: tracked::TrackedStats,
    pub archive: Option<archive::ArchiveStats>,
    pub history: Option<history::HistoryStats>,
    pub builds: builds::BuildStats,
    pub checks: Vec<Check>,
    pub table_counts: Vec<(String, i64)>,
    pub timings: Vec<(String, Duration)>,
}

/// Run a full import.
pub fn run(opts: &ImportOptions, progress: &Progress) -> Result<ImportReport> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opts.threads.unwrap_or(0))
        .thread_name(|i| format!("import-{i}"))
        .build()?;
    pool.install(|| run_in_pool(opts, progress))
}

fn run_in_pool(opts: &ImportOptions, progress: &Progress) -> Result<ImportReport> {
    let started = Instant::now();
    if opts.out.exists() && !opts.force {
        bail!(
            "{} already exists (use --force to replace it)",
            opts.out.display()
        );
    }
    let layout = SourceLayout::discover(&opts.backups_root, opts.snapshot.as_deref())?;
    progress.println(format!(
        "sources: snapshot {} · {} shards · manifest {}",
        layout.snapshot,
        layout.shards.len(),
        layout.manifest_dir.display()
    ));

    fs::create_dir_all(&opts.work_dir)
        .with_context(|| format!("creating work dir {}", opts.work_dir.display()))?;
    let mut timings = Vec::new();
    let mut timed = |name: &str, t: Instant| timings.push((name.to_string(), t.elapsed()));

    let t = Instant::now();
    let staged = stage::stage_sources(&layout, &opts.work_dir, progress)?;
    timed("stage sources", t);

    let partial = opts.work_dir.join("searchlight.duckdb.partial");
    remove_db_files(&partial)?;
    let db = Database::create(&partial)?;
    let conn = db.conn();
    configure_duckdb(conn, opts)?;
    conn.execute_batch(STAGING_SQL)?;

    let t = Instant::now();
    let manifest = manifest::load(conn, &layout.manifest_dir)?;
    timed("manifest", t);

    let t = Instant::now();
    let tracked = tracked::load(conn, &staged.main_db, progress)?;
    timed("tracked", t);

    let archive = if opts.skip.contains(&OptionalStage::Archive) {
        None
    } else {
        let t = Instant::now();
        let s = archive::load(conn, &staged, progress)?;
        timed("archive", t);
        Some(s)
    };

    let history = if opts.skip.contains(&OptionalStage::History) {
        None
    } else {
        let t = Instant::now();
        let s = history::load(conn, &staged, progress)?;
        timed("history", t);
        Some(s)
    };

    let t = Instant::now();
    let builds = builds::load(conn, &staged.main_db, &manifest.catalog, progress)?;
    timed("builds", t);

    let mut step_timings = transform::run(conn, progress)?;
    timings.append(&mut step_timings);

    let t = Instant::now();
    let checks = validate::run(conn)?;
    timings.push(("validate".into(), t.elapsed()));
    let failed: Vec<&Check> = checks
        .iter()
        .filter(|c| c.severity == Severity::Error && !c.passed())
        .collect();
    if !failed.is_empty() {
        let list: Vec<String> = failed.iter().map(ToString::to_string).collect();
        bail!(
            "validation failed; the partial database is kept at {} for inspection:\n{}",
            partial.display(),
            list.join("\n")
        );
    }

    let report_stats = StatRecorder::new(conn);
    report_stats.record_all(
        &tracked,
        archive.as_ref(),
        history.as_ref(),
        &builds,
        &checks,
    )?;

    for t in STAGING_TABLES {
        conn.execute_batch(&format!("DROP TABLE {t}"))?;
    }
    write_meta(
        &db,
        &layout,
        manifest.version.as_deref(),
        opts,
        started.elapsed(),
    )?;
    let table_counts = db.table_counts()?;
    conn.execute_batch("CHECKPOINT")?;
    drop(db);

    publish(&partial, &opts.out)?;
    Ok(ImportReport {
        out: opts.out.clone(),
        elapsed: started.elapsed(),
        snapshot: layout.snapshot,
        manifest_version: manifest.version,
        tracked,
        archive,
        history,
        builds,
        checks,
        table_counts,
        timings,
    })
}

fn configure_duckdb(conn: &duckdb::Connection, opts: &ImportOptions) -> Result<()> {
    let tmp = opts.work_dir.join("duckdb_tmp");
    fs::create_dir_all(&tmp)?;
    conn.execute_batch(&format!(
        "SET preserve_insertion_order = false; SET temp_directory = '{}';",
        tmp.display().to_string().replace('\'', "''")
    ))?;
    if let Some(n) = opts.threads {
        conn.execute_batch(&format!("SET threads = {n};"))?;
    }
    if let Some(m) = &opts.memory_limit {
        conn.execute_batch(&format!("SET memory_limit = '{}';", m.replace('\'', "''")))?;
    }
    Ok(())
}

fn write_meta(
    db: &Database,
    layout: &SourceLayout,
    manifest_version: Option<&str>,
    opts: &ImportOptions,
    elapsed: Duration,
) -> Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let imported_at: String = db.conn().query_row(
        "SELECT strftime(make_timestamp(?::BIGINT * 1000000), '%Y-%m-%dT%H:%M:%SZ')",
        [now as i64],
        |r| r.get(0),
    )?;
    db.set_meta(meta_key::IMPORTED_AT, &imported_at)?;
    db.set_meta(meta_key::TOOL_VERSION, env!("CARGO_PKG_VERSION"))?;
    db.set_meta(
        meta_key::BUILD_FINGERPRINT_VERSION,
        &BUILD_FINGERPRINT_VERSION.to_string(),
    )?;
    db.set_meta(
        meta_key::LOADOUT_FINGERPRINT_VERSION,
        &LOADOUT_FINGERPRINT_VERSION.to_string(),
    )?;
    db.set_meta(meta_key::SOURCE_ROOT, &layout.root.display().to_string())?;
    db.set_meta(meta_key::SOURCE_SNAPSHOT, &layout.snapshot)?;
    if let Some(v) = manifest_version {
        db.set_meta(meta_key::MANIFEST_VERSION, v)?;
    }
    db.set_meta(meta_key::IMPORT_SECONDS, &elapsed.as_secs().to_string())?;
    let skipped: Vec<&str> = opts.skip.iter().map(|s| s.as_str()).collect();
    db.set_meta("skipped_stages", &skipped.join(","))?;
    Ok(())
}

/// Writes counters into `import_stat`.
struct StatRecorder<'a>(&'a duckdb::Connection);

impl<'a> StatRecorder<'a> {
    fn new(conn: &'a duckdb::Connection) -> Self {
        Self(conn)
    }

    fn put(&self, stage: &str, metric: &str, value: u64) -> Result<()> {
        self.0.execute(
            "INSERT INTO import_stat (stage, metric, value) VALUES (?, ?, ?)",
            duckdb::params![stage, metric, value as i64],
        )?;
        Ok(())
    }

    fn record_all(
        &self,
        tracked: &tracked::TrackedStats,
        archive: Option<&archive::ArchiveStats>,
        history: Option<&history::HistoryStats>,
        builds: &builds::BuildStats,
        checks: &[Check],
    ) -> Result<()> {
        self.put("tracked", "runs", tracked.runs)?;
        self.put("tracked", "entries", tracked.entries)?;
        self.put("tracked", "weapons", tracked.weapons)?;
        self.put("tracked", "bad_weapon_json", tracked.bad_weapon_json)?;
        self.put("tracked", "players", tracked.players)?;
        if let Some(a) = archive {
            self.put("archive", "files", a.files)?;
            self.put("archive", "rows_read", a.rows_read)?;
            self.put("archive", "duplicates", a.duplicates)?;
            self.put("archive", "parse_errors", a.parse_errors)?;
            self.put("archive", "runs", a.runs)?;
            self.put("archive", "entries", a.entries)?;
            self.put("archive", "weapons", a.weapons)?;
        }
        if let Some(h) = history {
            self.put("history", "files", h.files)?;
            self.put("history", "rows_read", h.rows)?;
        }
        self.put("builds", "segments", builds.segments)?;
        self.put("builds", "bad_loadouts", builds.bad_loadouts)?;
        self.put("builds", "builds", builds.builds)?;
        self.put("builds", "components", builds.components)?;
        self.put("builds", "loadouts", builds.loadouts)?;
        self.put("builds", "unresolved_hashes", builds.unresolved_hashes)?;
        for c in checks {
            self.0.execute(
                "INSERT INTO import_stat (stage, metric, value) VALUES ('validate', ?, ?)",
                duckdb::params![c.name, c.value],
            )?;
        }
        Ok(())
    }
}

fn remove_db_files(path: &Path) -> Result<()> {
    for suffix in ["", ".wal"] {
        let mut p = path.as_os_str().to_owned();
        p.push(suffix);
        let p = PathBuf::from(p);
        if p.exists() {
            fs::remove_file(&p).with_context(|| format!("removing {}", p.display()))?;
        }
    }
    Ok(())
}

/// Move the finished database to `out` atomically: rename when on the same
/// filesystem, otherwise copy next to `out` and rename into place.
fn publish(partial: &Path, out: &Path) -> Result<()> {
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    remove_db_files(out)?;
    if fs::rename(partial, out).is_ok() {
        return Ok(());
    }
    let mut tmp = out.as_os_str().to_owned();
    tmp.push(".partial");
    let tmp = PathBuf::from(tmp);
    fs::copy(partial, &tmp).with_context(|| format!("copying database to {}", tmp.display()))?;
    fs::rename(&tmp, out)?;
    fs::remove_file(partial)?;
    Ok(())
}

impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Imported {} in {:.1?}", self.out.display(), self.elapsed)?;
        writeln!(
            f,
            "  snapshot {} · manifest {}",
            self.snapshot,
            self.manifest_version.as_deref().unwrap_or("?")
        )?;
        if let Some(a) = &self.archive {
            writeln!(
                f,
                "  archive: {} stored PGCRs read, {} duplicates skipped, {} unparseable",
                a.rows_read, a.duplicates, a.parse_errors
            )?;
            for (id, e) in &a.error_examples {
                writeln!(f, "    e.g. instance {id}: {e}")?;
            }
        }
        writeln!(
            f,
            "  builds: {} segments → {} distinct builds, {} distinct loadouts ({} unparseable)",
            self.builds.segments,
            self.builds.builds,
            self.builds.loadouts,
            self.builds.bad_loadouts
        )?;
        writeln!(f, "\nTables:")?;
        for (t, n) in &self.table_counts {
            writeln!(f, "  {t:<20} {n:>14}")?;
        }
        writeln!(f, "\nValidation:")?;
        for c in &self.checks {
            writeln!(f, "  {c}")?;
        }
        writeln!(f, "\nTimings:")?;
        for (name, d) in &self.timings {
            writeln!(f, "  {name:<24} {d:>8.1?}")?;
        }
        Ok(())
    }
}
