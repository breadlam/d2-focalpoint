//! Preparing local working copies of the sources.
//!
//! The backups must never be modified, and SQLite must not be opened on the
//! network share they live on (locking over CIFS is unreliable, and opening a
//! WAL database writes `-shm` / checkpoints). So every SQLite source is first
//! copied (with its `-wal`, preserving committed-but-uncheckpointed data) or
//! decompressed into the work directory. A marker file records the source's
//! size and mtime, so re-runs skip sources that have not changed.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::layout::{Shard, SourceLayout};
use crate::progress::Progress;

/// Local working copies of every source.
#[derive(Debug, Clone)]
pub struct StagedSources {
    pub main_db: PathBuf,
    pub pgcr_db: PathBuf,
    /// Decompressed shards, in the same order as `SourceLayout::shards`.
    pub shards: Vec<(Shard, PathBuf)>,
}

/// Copy / decompress all sources into `work_dir`.
pub fn stage_sources(
    layout: &SourceLayout,
    work_dir: &Path,
    progress: &Progress,
) -> Result<StagedSources> {
    let live = work_dir.join("live");
    let shard_dir = work_dir.join("shards");
    fs::create_dir_all(&live)?;
    fs::create_dir_all(&shard_dir)?;

    let bar = progress.bar("stage sources", layout.shards.len() as u64 + 2);
    let main_db = copy_sqlite(&layout.main_db, &live)?;
    bar.inc(1);
    let pgcr_db = copy_sqlite(&layout.pgcr_db, &live)?;
    bar.inc(1);

    let shards = layout
        .shards
        .par_iter()
        .map(|shard| {
            let dest = shard_dir.join(&shard.stem);
            let r = gunzip_cached(&shard.path, &dest).map(|()| (shard.clone(), dest));
            bar.inc(1);
            r
        })
        .collect::<Result<Vec<_>>>()?;
    bar.finish();
    Ok(StagedSources {
        main_db,
        pgcr_db,
        shards,
    })
}

/// Identity of a source file, written next to its working copy.
fn fingerprint(src: &Path) -> Result<String> {
    let md = fs::metadata(src).with_context(|| format!("stat {}", src.display()))?;
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Ok(format!("{}\n{}\n{}\n", src.display(), md.len(), mtime))
}

fn marker_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".source");
    PathBuf::from(s)
}

fn is_fresh(dest: &Path, expected: &str) -> bool {
    dest.exists() && fs::read_to_string(marker_path(dest)).is_ok_and(|m| m == expected)
}

/// Copy a SQLite database and its WAL (if any) into `dir`.
fn copy_sqlite(src: &Path, dir: &Path) -> Result<PathBuf> {
    let name = src.file_name().context("source has no file name")?;
    let dest = dir.join(name);
    let wal_src = sibling(src, "-wal");
    let mut expected = fingerprint(src)?;
    if wal_src.exists() {
        expected.push_str(&fingerprint(&wal_src)?);
    }
    if is_fresh(&dest, &expected) {
        return Ok(dest);
    }
    // Remove any stale pieces first so an old WAL can never be replayed onto
    // a new copy.
    for suffix in ["", "-wal", "-shm"] {
        let p = sibling(&dest, suffix);
        if p.exists() {
            fs::remove_file(&p).with_context(|| format!("removing stale {}", p.display()))?;
        }
    }
    let _ = fs::remove_file(marker_path(&dest));
    fs::copy(src, &dest).with_context(|| format!("copying {}", src.display()))?;
    if wal_src.exists() {
        fs::copy(&wal_src, sibling(&dest, "-wal"))
            .with_context(|| format!("copying {}", wal_src.display()))?;
    }
    fs::write(marker_path(&dest), expected)?;
    Ok(dest)
}

/// Decompress `src` (gzip) to `dest` unless an up-to-date copy exists.
fn gunzip_cached(src: &Path, dest: &Path) -> Result<()> {
    let expected = fingerprint(src)?;
    if is_fresh(dest, &expected) {
        return Ok(());
    }
    let tmp = sibling(dest, ".tmp");
    {
        let input = File::open(src).with_context(|| format!("opening {}", src.display()))?;
        let mut decoder =
            flate2::read::MultiGzDecoder::new(BufReader::with_capacity(1 << 20, input));
        let mut out = BufWriter::with_capacity(1 << 20, File::create(&tmp)?);
        io::copy(&mut decoder, &mut out)
            .with_context(|| format!("decompressing {}", src.display()))?;
        out.into_inner()
            .map_err(io::IntoInnerError::into_error)?
            .sync_all()?;
    }
    fs::rename(&tmp, dest)?;
    fs::write(marker_path(dest), expected)?;
    Ok(())
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn gunzip_is_cached_until_the_source_changes() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.sqlite.gz");
        let write_gz = |content: &[u8]| {
            let mut enc = flate2::write::GzEncoder::new(
                File::create(&src).unwrap(),
                flate2::Compression::fast(),
            );
            enc.write_all(content).unwrap();
            enc.finish().unwrap();
        };
        write_gz(b"one");
        let dest = dir.path().join("a.sqlite");
        gunzip_cached(&src, &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"one");

        // Tamper with the copy: a fresh marker means it is not redone.
        fs::write(&dest, b"tampered").unwrap();
        gunzip_cached(&src, &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"tampered");

        // A changed source (size differs) is re-extracted.
        write_gz(b"two, longer");
        gunzip_cached(&src, &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"two, longer");
    }

    #[test]
    fn copies_database_with_wal_and_clears_stale_files() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let dst_dir = dir.path().join("dst");
        fs::create_dir_all(&src_dir).unwrap();
        fs::create_dir_all(&dst_dir).unwrap();
        fs::write(src_dir.join("x.sqlite"), b"db").unwrap();
        fs::write(src_dir.join("x.sqlite-wal"), b"wal").unwrap();
        fs::write(dst_dir.join("x.sqlite-shm"), b"stale").unwrap();

        let dest = copy_sqlite(&src_dir.join("x.sqlite"), &dst_dir).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"db");
        assert_eq!(fs::read(dst_dir.join("x.sqlite-wal")).unwrap(), b"wal");
        assert!(!dst_dir.join("x.sqlite-shm").exists());
    }
}
