//! `focalpoint site` — export the data behind the static site in `site/`
//! (see `docs/site.md`): the ranked builds (`book/`) and the catalog of
//! every collected build (`catalog/`).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use focalpoint_db::Database;
use focalpoint_db::catalog::catalog;
use focalpoint_db::site::{SiteOptions, site_data, split};

use crate::output::thousands;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory to write `book/` and `catalog/` into
    #[arg(long, default_value = "site/data")]
    pub out: PathBuf,

    /// Players a build type needs in an activity to be listed for it
    #[arg(long, default_value_t = 1)]
    pub min_players: usize,
}

pub fn run(args: &Args, db: &Path) -> Result<()> {
    let db = Database::open_read_only(db)?;
    fs::create_dir_all(&args.out).with_context(|| format!("creating {}", args.out.display()))?;

    let started = Instant::now();
    let opts = SiteOptions {
        min_players: args.min_players,
        ..SiteOptions::default()
    };
    let data = site_data(&db, &opts, |scope| eprintln!("  {scope}"))?;
    let rows: usize = data.scopes.iter().map(|s| s.rows.len()).sum();
    let (types, recipes) = (data.types.len(), data.recipes.len());
    let book = split(data);
    let mut files = vec![("index.json".to_string(), serde_json::to_vec(&book.index)?)];
    for (name, part) in &book.parts {
        files.push((name.clone(), serde_json::to_vec(part)?));
    }
    let bytes = replace_dir(&args.out, "book", &files)?;
    // The single-file book of earlier versions.
    let legacy = args.out.join("focalpoint.json");
    if legacy.exists() {
        fs::remove_file(&legacy).with_context(|| format!("removing {}", legacy.display()))?;
    }
    println!(
        "Wrote {} ({} KB) in {:.1}s: {} scopes in {} files, {} build types, {} listings, {} exact builds",
        args.out.join("book").display(),
        thousands((bytes / 1024) as i64),
        started.elapsed().as_secs_f64(),
        book.index.scopes.len(),
        files.len(),
        thousands(types as i64),
        thousands(rows as i64),
        thousands(recipes as i64),
    );

    let started = Instant::now();
    let cat = catalog(&db, |step| eprintln!("  {step}"))?;
    let mut files = vec![("index.txt".to_string(), cat.index.clone().into_bytes())];
    for (i, shard) in cat.shards.iter().enumerate() {
        files.push((format!("s{i:03}.txt"), shard.clone().into_bytes()));
    }
    let bytes = replace_dir(&args.out, "catalog", &files)?;
    let dir = args.out.join("catalog");
    println!(
        "Wrote {} ({} MB) in {:.1}s: {} builds of {} types in {} shards",
        dir.display(),
        thousands((bytes / (1024 * 1024)) as i64),
        started.elapsed().as_secs_f64(),
        thousands(cat.builds as i64),
        thousands(cat.types as i64),
        cat.shards.len(),
    );
    Ok(())
}

/// Write `files` into `<out>/<name>/`, replacing what was there, via a
/// temporary directory renamed into place. Returns the bytes written.
fn replace_dir(out: &Path, name: &str, files: &[(String, Vec<u8>)]) -> Result<usize> {
    let dir = out.join(name);
    let tmp = out.join(format!(".{name}.tmp"));
    if tmp.exists() {
        fs::remove_dir_all(&tmp).with_context(|| format!("clearing {}", tmp.display()))?;
    }
    fs::create_dir_all(&tmp)?;
    let mut bytes = 0;
    for (file, contents) in files {
        fs::write(tmp.join(file), contents).with_context(|| format!("writing {file}"))?;
        bytes += contents.len();
    }
    if dir.exists() {
        fs::remove_dir_all(&dir).with_context(|| format!("replacing {}", dir.display()))?;
    }
    fs::rename(&tmp, &dir).with_context(|| format!("replacing {}", dir.display()))?;
    Ok(bytes)
}
