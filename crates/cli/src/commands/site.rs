//! `focalpoint site` — export the data behind the static site in `site/`
//! (see `docs/site.md`): the ranked book (`focalpoint.json`) and the catalog
//! of every collected build (`catalog/`).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use focalpoint_db::Database;
use focalpoint_db::catalog::catalog;
use focalpoint_db::site::{SiteOptions, site_data};

use crate::output::thousands;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory to write `focalpoint.json` and `catalog/` into
    #[arg(long, default_value = "site/data")]
    pub out: PathBuf,

    /// Players a build type needs in an activity to be listed for it
    #[arg(long, default_value_t = 2)]
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
    let path = args.out.join("focalpoint.json");
    let tmp = args.out.join(".focalpoint.json.tmp");
    let json = serde_json::to_vec(&data)?;
    fs::write(&tmp, &json).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("replacing {}", path.display()))?;
    let rows: usize = data.scopes.iter().map(|s| s.rows.len()).sum();
    println!(
        "Wrote {} ({} KB) in {:.1}s: {} scopes, {} build types, {} listings, {} exact recipes",
        path.display(),
        thousands((json.len() / 1024) as i64),
        started.elapsed().as_secs_f64(),
        data.scopes.len(),
        thousands(data.types.len() as i64),
        thousands(rows as i64),
        thousands(data.recipes.len() as i64),
    );

    let started = Instant::now();
    let cat = catalog(&db, |step| eprintln!("  {step}"))?;
    let dir = args.out.join("catalog");
    let tmp = args.out.join(".catalog.tmp");
    if tmp.exists() {
        fs::remove_dir_all(&tmp).with_context(|| format!("clearing {}", tmp.display()))?;
    }
    fs::create_dir_all(&tmp)?;
    fs::write(tmp.join("index.txt"), &cat.index)?;
    let mut bytes = cat.index.len();
    for (i, shard) in cat.shards.iter().enumerate() {
        fs::write(tmp.join(format!("s{i:03}.txt")), shard)?;
        bytes += shard.len();
    }
    if dir.exists() {
        fs::remove_dir_all(&dir).with_context(|| format!("replacing {}", dir.display()))?;
    }
    fs::rename(&tmp, &dir).with_context(|| format!("replacing {}", dir.display()))?;
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
