//! `searchlight search` — facet search over builds and popularity reports.

use std::path::Path;

use anyhow::Result;
use clap::ValueEnum;
use searchlight_db::Database;
use searchlight_db::search::{BuildSearch, GroupBy, SortBy, search_builds};

use super::filters::{FacetArgs, RunArgs};
use crate::output::{Format, render, thousands};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Group {
    Build,
    /// Class, subclass element, exotics, super and aspects
    Core,
    Player,
    Class,
    Element,
    ExoticArmor,
    ExoticWeapon,
    Super,
    Aspects,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Sort {
    Runs,
    Players,
    Clears,
    Kills,
    Recent,
}

#[derive(Debug, clap::Args)]
#[command(
    after_help = "Names match case-insensitively. Run filters decide which runs are counted.\n\n\
Examples:\n  \
searchlight search --class Warlock --exotic-armor \"Getaway Artist\" --activity conquest\n  \
searchlight search --class Hunter --activity \"master raid\" --clears --group-by exotic-armor"
)]
pub struct Args {
    #[command(flatten)]
    pub facets: FacetArgs,
    #[command(flatten)]
    pub runs: RunArgs,
    /// Only runs the player cleared
    #[arg(long)]
    pub clears: bool,
    /// Count zero-kill runs too (checkpoint bots, passengers)
    #[arg(long)]
    pub include_passengers: bool,

    /// Group results (popularity reports) instead of listing builds
    #[arg(long, value_enum, default_value_t = Group::Build)]
    pub group_by: Group,
    #[arg(long, value_enum, default_value_t = Sort::Runs)]
    pub sort: Sort,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    #[arg(long, value_enum, default_value_t)]
    pub format: Format,
}

impl Args {
    fn to_query(&self) -> BuildSearch {
        BuildSearch {
            facets: self.facets.to_facets(),
            runs: self.runs.to_filter(),
            clears_only: self.clears,
            include_passengers: self.include_passengers,
            group_by: match self.group_by {
                Group::Build => GroupBy::Build,
                Group::Core => GroupBy::Core,
                Group::Player => GroupBy::Player,
                Group::Class => GroupBy::Class,
                Group::Element => GroupBy::Element,
                Group::ExoticArmor => GroupBy::ExoticArmor,
                Group::ExoticWeapon => GroupBy::ExoticWeapon,
                Group::Super => GroupBy::Super,
                Group::Aspects => GroupBy::Aspects,
            },
            sort: match self.sort {
                Sort::Runs => SortBy::Runs,
                Sort::Players => SortBy::Players,
                Sort::Clears => SortBy::Clears,
                Sort::Kills => SortBy::Kills,
                Sort::Recent => SortBy::Recent,
            },
            limit: self.limit,
        }
    }
}

pub fn run(args: &Args, db: &Path) -> Result<()> {
    let db = Database::open_read_only(db)?;
    let result = search_builds(&db, &args.to_query())?;
    let grouped = args.group_by != Group::Build;

    let mut header: Vec<String> = result
        .key_columns
        .iter()
        .map(|c| (*c).to_string())
        .collect();
    if grouped {
        header.push("builds".into());
    }
    header.extend(
        [
            "runs",
            "players",
            "clears",
            "clear %",
            "avg kills",
            "last used",
        ]
        .map(String::from),
    );
    let rows: Vec<Vec<String>> = result
        .rows
        .iter()
        .map(|r| {
            let mut row = r.key.clone();
            if grouped {
                row.push(r.builds.to_string());
            }
            let clear_pct = if r.runs > 0 {
                100.0 * r.clears as f64 / r.runs as f64
            } else {
                0.0
            };
            row.extend([
                r.runs.to_string(),
                r.players.to_string(),
                r.clears.to_string(),
                format!("{clear_pct:.0}"),
                format!("{:.0}", r.avg_kills),
                r.last_used.clone(),
            ]);
            row
        })
        .collect();
    if args.format == Format::Table {
        println!(
            "{} {} match; showing {}",
            thousands(result.total_groups),
            if grouped { "groups" } else { "builds" },
            rows.len()
        );
    }
    println!("{}", render(args.format, &header, &rows));
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
    fn flags_map_onto_the_query() {
        let cli = Cli::try_parse_from([
            "x",
            "--class",
            "Warlock",
            "--aspect",
            "Feed the Void",
            "--aspect",
            "Chaos Accelerant",
            "--mod",
            "Firepower",
            "--super",
            "Song of Flame",
            "--clears",
            "--group-by",
            "exotic-armor",
            "--sort",
            "players",
        ])
        .unwrap();
        let q = cli.args.to_query();
        assert_eq!(q.facets.class.as_deref(), Some("Warlock"));
        assert_eq!(q.facets.aspects.len(), 2);
        assert_eq!(q.facets.mods, ["Firepower"]);
        assert_eq!(q.facets.super_name.as_deref(), Some("Song of Flame"));
        assert!(q.clears_only);
        assert_eq!(q.group_by, GroupBy::ExoticArmor);
        assert_eq!(q.sort, SortBy::Players);
    }
}
