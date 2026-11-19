//! `searchlight` — query and analyse Destiny 2 builds collected by Searchlight.

mod commands;
mod output;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "searchlight", version, about, long_about = None)]
struct Cli {
    /// Analysis database to use [default: searchlight/searchlight.duckdb in the user data dir]
    #[arg(long, global = true, env = searchlight_db::DB_ENV_VAR)]
    db: Option<PathBuf>,

    /// Increase log verbosity (-v, -vv)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Build the analysis database from Searchlight backups
    Import(commands::import::Args),
    /// Summarise the analysis database
    Info(commands::info::Args),
    /// Search builds by class, exotics, abilities, perks, mods, activity, ...
    Search(Box<commands::search::Args>),
    /// List a build's nearest neighbours by the tiered build distance
    Similar(commands::similar::Args),
    /// Run an ad-hoc SQL query against the database
    #[command(
        after_help = "Example:\n  searchlight sql \"SELECT item_name(exotic_armor_hash) AS exotic, count(*) AS builds\n                   FROM build GROUP BY 1 ORDER BY 2 DESC LIMIT 10\""
    )]
    Sql(commands::sql::Args),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging(cli.verbose);
    let db = cli.db.unwrap_or_else(searchlight_db::default_path);
    let result = match cli.command {
        Command::Import(args) => commands::import::run(&args, &db),
        Command::Info(args) => commands::info::run(&args, &db),
        Command::Search(args) => commands::search::run(&args, &db),
        Command::Similar(args) => commands::similar::run(&args, &db),
        Command::Sql(args) => commands::sql::run(&args, &db),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_import_skip_list() {
        let cli = Cli::try_parse_from([
            "searchlight",
            "import",
            "--backups",
            "/b",
            "--skip",
            "archive,history",
        ])
        .unwrap();
        let Command::Import(args) = cli.command else {
            panic!("expected import")
        };
        assert_eq!(args.skip.len(), 2);
    }
}
