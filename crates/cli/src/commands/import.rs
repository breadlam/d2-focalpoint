//! `searchlight import`

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::ValueEnum;
use searchlight_import::{ImportOptions, OptionalStage, Progress};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Skip {
    /// The raw PGCR archive (~17.7M reports)
    Archive,
    /// The all-modes activity history (~250M rows)
    History,
}

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Root of the Searchlight backups (contains builds.rodeo/ and shards/)
    #[arg(long, value_name = "DIR")]
    pub backups: PathBuf,

    /// Server snapshot to import (e.g. snapshot-2026-09-20) [default: latest]
    #[arg(long)]
    pub snapshot: Option<String>,

    /// Local scratch directory for staged sources (reused between runs as a cache)
    /// [default: <user cache dir>/searchlight/import]
    #[arg(long, value_name = "DIR")]
    pub work_dir: Option<PathBuf>,

    /// Replace an existing database
    #[arg(long)]
    pub force: bool,

    /// Worker threads [default: all cores]
    #[arg(long)]
    pub threads: Option<usize>,

    /// DuckDB memory limit, e.g. 64GB [default: 80% of RAM]
    #[arg(long)]
    pub memory_limit: Option<String>,

    /// Skip heavy optional stages (faster development imports)
    #[arg(long, value_delimiter = ',')]
    pub skip: Vec<Skip>,
}

pub fn run(args: &Args, db: &Path) -> Result<()> {
    let work_dir = args.work_dir.clone().unwrap_or_else(|| {
        dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("searchlight")
            .join("import")
    });
    let opts = ImportOptions {
        backups_root: args.backups.clone(),
        snapshot: args.snapshot.clone(),
        out: db.to_path_buf(),
        work_dir,
        force: args.force,
        threads: args.threads,
        memory_limit: args.memory_limit.clone(),
        skip: args
            .skip
            .iter()
            .map(|s| match s {
                Skip::Archive => OptionalStage::Archive,
                Skip::History => OptionalStage::History,
            })
            .collect(),
    };
    let report = searchlight_import::run(&opts, &Progress::stderr())?;
    println!("{report}");
    Ok(())
}
