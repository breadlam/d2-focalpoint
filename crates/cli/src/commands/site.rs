//! `focalpoint site` — export the data behind the static build-ranking
//! site in `site/` (see `docs/site.md`).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use focalpoint_db::Database;
use focalpoint_db::site::{SiteOptions, site_data};

use crate::output::thousands;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory to write `focalpoint.json` into
    #[arg(long, default_value = "site/data")]
    pub out: PathBuf,

    /// Players a build needs in an activity to be listed for it
    #[arg(long, default_value_t = 2)]
    pub min_players: usize,
}

pub fn run(args: &Args, db: &Path) -> Result<()> {
    let db = Database::open_read_only(db)?;
    let started = Instant::now();
    let opts = SiteOptions {
        min_players: args.min_players,
        ..SiteOptions::default()
    };
    let data = site_data(&db, &opts, |scope| eprintln!("  {scope}"))?;

    fs::create_dir_all(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
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
    Ok(())
}
