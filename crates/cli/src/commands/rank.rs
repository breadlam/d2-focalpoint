//! `focalpoint rank` — builds and players, with the build separated from
//! the player.

use std::path::Path;

use anyhow::Result;
use clap::ValueEnum;
use focalpoint_db::Database;
use focalpoint_db::rank::{
    BuildSort, Model, PlayerSort, RankOptions, SoloScoring, name_players, rank_builds, rank_players,
};
use focalpoint_db::search::GroupBy;

use super::filters::{FacetArgs, RunArgs};
use crate::output::{Format, render, thousands};

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub what: What,
}

#[derive(Debug, clap::Subcommand)]
pub enum What {
    /// Builds ranked by how their players perform with them, relative to
    /// those players' own usual level
    #[command(
        after_help = "Effects are relative to the average captured build. 'raw' is how \
the build's runs went without separating the player; 'players' is how good its players are.\n\n\
Examples:\n  \
focalpoint rank builds --class Warlock --activity conquest\n  \
focalpoint rank builds --group-by exotic-armor --class Hunter --min-players 10"
    )]
    Builds(BuildArgs),
    /// Players ranked by skill, net of the builds they used
    #[command(
        after_help = "Skill is performance relative to a typical clear of the same \
activity and solo status, shrunk toward average when a player has few runs.\n\n\
Examples:\n  \
focalpoint rank players --activity \"ultimate conquest\"\n  \
focalpoint rank players --exotic-armor \"Getaway Artist\" --min-runs 20"
    )]
    Players(PlayerArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Group {
    /// Class, subclass element, exotics, super and aspects
    Core,
    /// Exact builds (every component)
    Build,
    Class,
    Element,
    ExoticArmor,
    ExoticWeapon,
    Super,
    Aspects,
}

impl From<Group> for GroupBy {
    fn from(g: Group) -> Self {
        match g {
            Group::Core => GroupBy::Core,
            Group::Build => GroupBy::Build,
            Group::Class => GroupBy::Class,
            Group::Element => GroupBy::Element,
            Group::ExoticArmor => GroupBy::ExoticArmor,
            Group::ExoticWeapon => GroupBy::ExoticWeapon,
            Group::Super => GroupBy::Super,
            Group::Aspects => GroupBy::Aspects,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BuildOrder {
    Effect,
    Raw,
    Players,
    Runs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PlayerOrder {
    Skill,
    Runs,
    Best,
}

#[derive(Debug, clap::Args)]
pub struct BuildArgs {
    #[command(flatten)]
    pub facets: FacetArgs,
    #[command(flatten)]
    pub runs: RunArgs,
    #[arg(long, value_enum, default_value_t = Group::Core)]
    pub group_by: Group,
    /// Only list builds used by at least this many players
    #[arg(long, default_value_t = 3)]
    pub min_players: usize,
    #[arg(long, value_enum, default_value_t = BuildOrder::Effect)]
    pub sort: BuildOrder,
    /// What solo clears are measured against
    #[arg(long, value_enum, default_value_t = Solo::Alone)]
    pub solo_scoring: Solo,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    #[arg(long, value_enum, default_value_t)]
    pub format: Format,
}

/// What solo clears are measured against (see `focalpoint analyze solo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Solo {
    /// Other solo clears of the activity (default)
    Alone,
    /// Other solo clears, plus the activity's measured solo premium
    Credit,
    /// The activity's fireteam clears, run by run
    Fireteam,
}

impl From<Solo> for SoloScoring {
    fn from(s: Solo) -> Self {
        match s {
            Solo::Alone => Self::AgainstSolo,
            Solo::Credit => Self::ActivityCredit,
            Solo::Fireteam => Self::AgainstFireteam,
        }
    }
}

#[derive(Debug, clap::Args)]
pub struct PlayerArgs {
    #[command(flatten)]
    pub facets: FacetArgs,
    #[command(flatten)]
    pub runs: RunArgs,
    /// Only list players with at least this many scored clears
    #[arg(long, default_value_t = 10)]
    pub min_runs: i64,
    #[arg(long, value_enum, default_value_t = PlayerOrder::Skill)]
    pub sort: PlayerOrder,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    #[arg(long, value_enum, default_value_t)]
    pub format: Format,
}

/// A log-scale effect as a signed percentage: 0.1 → "+11%".
fn pct(x: f64) -> String {
    format!("{:+.0}%", (x.exp() - 1.0) * 100.0)
}

fn describe(model: &Model, format: Format) {
    if format != Format::Table {
        return;
    }
    println!(
        "Fit on {} qualified clears ({} with a captured build) by {} players; {} build groups{}",
        thousands(model.runs as i64),
        thousands(model.build_runs as i64),
        thousands(model.players as i64),
        thousands(model.groups as i64),
        if model.converged {
            ""
        } else {
            " (fit did not converge)"
        },
    );
}

pub fn run(args: &Args, db: &Path) -> Result<()> {
    let db = Database::open_read_only(db)?;
    match &args.what {
        What::Builds(a) => builds(&db, a),
        What::Players(a) => players(&db, a),
    }
}

fn builds(db: &Database, args: &BuildArgs) -> Result<()> {
    let opts = RankOptions {
        facets: args.facets.to_facets(),
        runs: args.runs.to_filter(),
        group_by: args.group_by.into(),
        solo: args.solo_scoring.into(),
        ..RankOptions::default()
    };
    let sort = match args.sort {
        BuildOrder::Effect => BuildSort::Effect,
        BuildOrder::Raw => BuildSort::Raw,
        BuildOrder::Players => BuildSort::Players,
        BuildOrder::Runs => BuildSort::Runs,
    };
    let (model, key_columns, rows) = rank_builds(db, &opts, args.min_players, sort)?;
    describe(&model, args.format);
    if args.format == Format::Table {
        println!(
            "{} groups used by at least {} players; showing {}\n",
            thousands(rows.len() as i64),
            args.min_players,
            rows.len().min(args.limit)
        );
    }
    let mut header: Vec<String> = key_columns.iter().map(|c| (*c).to_string()).collect();
    header.extend(
        [
            "effect",
            "raw",
            "players' skill",
            "players",
            "runs",
            "builds",
            "last used",
        ]
        .map(String::from),
    );
    let table: Vec<Vec<String>> = rows
        .iter()
        .take(args.limit)
        .map(|r| {
            let mut row = r.key.clone();
            row.extend([
                pct(r.effect),
                pct(r.raw),
                pct(r.player_skill),
                r.players.to_string(),
                format!("{:.0}", r.runs),
                r.builds.to_string(),
                r.last_used.clone(),
            ]);
            row
        })
        .collect();
    println!("{}", render(args.format, &header, &table));
    Ok(())
}

fn players(db: &Database, args: &PlayerArgs) -> Result<()> {
    let opts = RankOptions {
        facets: args.facets.to_facets(),
        runs: args.runs.to_filter(),
        ..RankOptions::default()
    };
    let sort = match args.sort {
        PlayerOrder::Skill => PlayerSort::Skill,
        PlayerOrder::Runs => PlayerSort::Runs,
        PlayerOrder::Best => PlayerSort::Best,
    };
    let (model, mut rows) = rank_players(db, &opts, args.min_runs, sort)?;
    describe(&model, args.format);
    if args.format == Format::Table {
        println!(
            "{} players with at least {} clears; showing {}\n",
            thousands(rows.len() as i64),
            args.min_runs,
            rows.len().min(args.limit)
        );
    }
    rows.truncate(args.limit);
    name_players(db, &mut rows)?;
    let header: Vec<String> = [
        "#",
        "player",
        "skill",
        "clears",
        "solo",
        "with build",
        "best score",
        "last clear",
    ]
    .map(String::from)
    .to_vec();
    let table: Vec<Vec<String>> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            vec![
                (i + 1).to_string(),
                r.name.clone(),
                pct(r.skill),
                r.runs.to_string(),
                r.solo_runs.to_string(),
                r.build_runs.to_string(),
                format!("{:.2}", r.best_score),
                r.last_run.clone(),
            ]
        })
        .collect();
    println!("{}", render(args.format, &header, &table));
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        args: Args,
    }

    #[test]
    fn percentages_read_naturally() {
        assert_eq!(pct(0.0), "+0%");
        assert_eq!(pct(0.1), "+11%");
        assert_eq!(pct(-0.5), "-39%");
    }

    #[test]
    fn subcommands_parse() {
        let cli = Cli::try_parse_from([
            "x",
            "builds",
            "--class",
            "Warlock",
            "--activity",
            "conquest",
            "--group-by",
            "exotic-armor",
            "--min-players",
            "5",
        ])
        .unwrap();
        let What::Builds(b) = cli.args.what else {
            panic!("expected builds")
        };
        assert_eq!(b.facets.class.as_deref(), Some("Warlock"));
        assert_eq!(b.group_by, Group::ExoticArmor);
        assert_eq!(b.min_players, 5);
        let cli = Cli::try_parse_from(["x", "players", "--solo", "--sort", "best"]).unwrap();
        let What::Players(p) = cli.args.what else {
            panic!("expected players")
        };
        assert!(p.runs.solo);
        assert_eq!(p.sort, PlayerOrder::Best);
    }
}
