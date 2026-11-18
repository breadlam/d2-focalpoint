//! Locating the Searchlight backup sources on disk.
//!
//! Expected layout (see `docs/data-sources.md`):
//!
//! ```text
//! <root>/
//!   builds.rodeo/snapshot-YYYY-MM-DD/opt/searchlight/searchlight/data/
//!       searchlight.sqlite[-wal]      live main DB (runs, builds, history, ...)
//!       pgcr_raw.sqlite[-wal]         last ~7 days of raw PGCRs
//!       archive/outbox/*.sqlite.gz    day shards exported but never pulled
//!       manifest/Destiny*.json        cached Bungie manifest tables
//!   shards/*.sqlite.gz                day shards pulled by the backup host
//!   snapshots/                        older logical dumps (superseded; unused)
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Data directory of the production deployment inside a server snapshot.
const SNAPSHOT_DATA_DIR: &str = "opt/searchlight/searchlight/data";

/// Kind of rows a shard holds (from its file-name prefix).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShardKind {
    PgcrRaw,
    PlayerActivity,
}

/// One gzipped day shard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shard {
    pub kind: ShardKind,
    pub path: PathBuf,
    /// File name without `.gz` (unique across shards and the outbox).
    pub stem: String,
}

/// All source locations for one import.
#[derive(Debug, Clone)]
pub struct SourceLayout {
    pub root: PathBuf,
    /// Name of the server snapshot directory used (e.g. `snapshot-2026-09-20`).
    pub snapshot: String,
    pub data_dir: PathBuf,
    pub main_db: PathBuf,
    pub pgcr_db: PathBuf,
    pub manifest_dir: PathBuf,
    /// Pulled shards plus the never-pulled outbox, deduplicated by file name.
    pub shards: Vec<Shard>,
}

impl SourceLayout {
    /// Discover sources under `root`, using the latest server snapshot unless
    /// `snapshot` names one.
    pub fn discover(root: &Path, snapshot: Option<&str>) -> Result<Self> {
        let snapshots_dir = root.join("builds.rodeo");
        let snapshot = match snapshot {
            Some(s) => s.to_string(),
            None => latest_snapshot(&snapshots_dir)?,
        };
        let data_dir = snapshots_dir.join(&snapshot).join(SNAPSHOT_DATA_DIR);
        let main_db = data_dir.join("searchlight.sqlite");
        let pgcr_db = data_dir.join("pgcr_raw.sqlite");
        let manifest_dir = data_dir.join("manifest");
        for p in [&main_db, &pgcr_db, &manifest_dir] {
            if !p.exists() {
                bail!(
                    "expected {} to exist (is {} a Searchlight backup root?)",
                    p.display(),
                    root.display()
                );
            }
        }

        let mut shards = Vec::new();
        collect_shards(&root.join("shards"), &mut shards)?;
        collect_shards(&data_dir.join("archive").join("outbox"), &mut shards)?;
        shards.sort_by(|a, b| a.stem.cmp(&b.stem));
        shards.dedup_by(|a, b| a.stem == b.stem);

        Ok(Self {
            root: root.to_path_buf(),
            snapshot,
            data_dir,
            main_db,
            pgcr_db,
            manifest_dir,
            shards,
        })
    }

    pub fn shards_of(&self, kind: ShardKind) -> impl Iterator<Item = &Shard> {
        self.shards.iter().filter(move |s| s.kind == kind)
    }
}

fn latest_snapshot(dir: &Path) -> Result<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .with_context(|| format!("listing server snapshots in {}", dir.display()))?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("snapshot-"))
        .collect();
    names.sort();
    names
        .pop()
        .with_context(|| format!("no snapshot-* directory in {}", dir.display()))
}

fn collect_shards(dir: &Path, out: &mut Vec<Shard>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir).with_context(|| format!("listing {}", dir.display()))? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".gz") else {
            continue;
        };
        if !stem.ends_with(".sqlite") {
            continue;
        }
        let kind = if stem.starts_with("pgcr_raw_") {
            ShardKind::PgcrRaw
        } else if stem.starts_with("player_activity_") {
            ShardKind::PlayerActivity
        } else {
            continue;
        };
        out.push(Shard {
            kind,
            stem: stem.to_string(),
            path,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"").unwrap();
    }

    #[test]
    fn discovers_latest_snapshot_and_dedupes_shards() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        for snap in ["snapshot-2026-08-01", "snapshot-2026-09-20"] {
            let data = r.join("builds.rodeo").join(snap).join(SNAPSHOT_DATA_DIR);
            touch(&data.join("searchlight.sqlite"));
            touch(&data.join("pgcr_raw.sqlite"));
            std::fs::create_dir_all(data.join("manifest")).unwrap();
        }
        let data = r
            .join("builds.rodeo/snapshot-2026-09-20")
            .join(SNAPSHOT_DATA_DIR);
        touch(&r.join("shards/pgcr_raw_2026-06-01_1.sqlite.gz"));
        touch(&r.join("shards/player_activity_2026-06-09_1.sqlite.gz"));
        touch(&r.join("shards/unrelated.txt"));
        touch(&data.join("archive/outbox/pgcr_raw_2026-09-01_2.sqlite.gz"));
        // The same shard in both places counts once.
        touch(&data.join("archive/outbox/pgcr_raw_2026-06-01_1.sqlite.gz"));

        let l = SourceLayout::discover(r, None).unwrap();
        assert_eq!(l.snapshot, "snapshot-2026-09-20");
        assert_eq!(l.shards_of(ShardKind::PgcrRaw).count(), 2);
        assert_eq!(l.shards_of(ShardKind::PlayerActivity).count(), 1);

        let older = SourceLayout::discover(r, Some("snapshot-2026-08-01")).unwrap();
        assert_eq!(older.snapshot, "snapshot-2026-08-01");
    }

    #[test]
    fn explains_a_wrong_root() {
        let root = tempfile::tempdir().unwrap();
        let err = SourceLayout::discover(root.path(), None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("snapshot"), "{err}");
    }
}
