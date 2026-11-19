//! `searchlight search` — facet search over builds and popularity reports.

use std::path::Path;

use anyhow::Result;
use clap::ValueEnum;
use searchlight_db::Database;
use searchlight_db::search::{BuildSearch, GroupBy, SortBy, search_builds};

use crate::output::{Format, render, thousands};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Group {
    Build,
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
    after_help = "Facets combine with AND; repeat a flag to require several (--aspect A --aspect B).\n\
Names match case-insensitively. Run filters decide which runs are counted.\n\n\
Examples:\n  \
searchlight search --class Warlock --exotic-armor \"Getaway Artist\" --activity conquest\n  \
searchlight search --class Hunter --activity \"master raid\" --clears --group-by exotic-armor"
)]
pub struct Args {
    /// Titan, Hunter or Warlock
    #[arg(long)]
    pub class: Option<String>,
    /// Arc, Solar, Void, Stasis, Strand or Prismatic
    #[arg(long)]
    pub element: Option<String>,
    #[arg(long, value_name = "NAME")]
    pub exotic_armor: Option<String>,
    #[arg(long, value_name = "NAME")]
    pub exotic_weapon: Option<String>,
    #[arg(long = "super", value_name = "NAME")]
    pub super_name: Option<String>,
    #[arg(long, value_name = "NAME")]
    pub aspect: Vec<String>,
    #[arg(long, value_name = "NAME")]
    pub fragment: Vec<String>,
    /// A weapon in any slot
    #[arg(long, value_name = "NAME")]
    pub weapon: Vec<String>,
    /// A weapon perk, trait, origin trait or frame
    #[arg(long, value_name = "NAME")]
    pub perk: Vec<String>,
    /// An armor mod (general, stat or tuning) or weapon mod
    #[arg(long = "mod", value_name = "NAME")]
    pub mods: Vec<String>,
    #[arg(long, value_name = "NAME")]
    pub artifact_perk: Vec<String>,
    /// An armor set with at least two pieces equipped
    #[arg(long = "set", value_name = "NAME")]
    pub sets: Vec<String>,

    /// Only runs whose activity name or target group contains TEXT
    #[arg(long, value_name = "TEXT")]
    pub activity: Option<String>,
    /// Only runs by players whose Bungie or display name contains TEXT
    #[arg(long, value_name = "TEXT")]
    pub player: Option<String>,
    /// Only solo runs
    #[arg(long)]
    pub solo: bool,
    /// Only runs the player cleared
    #[arg(long)]
    pub clears: bool,
    /// Count zero-kill runs too (checkpoint bots, passengers)
    #[arg(long)]
    pub include_passengers: bool,
    /// Only runs on or after this date (YYYY-MM-DD)
    #[arg(long, value_name = "DATE")]
    pub since: Option<String>,
    /// Only runs on or before this date (YYYY-MM-DD)
    #[arg(long, value_name = "DATE")]
    pub until: Option<String>,

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
            class: self.class.clone(),
            element: self.element.clone(),
            exotic_armor: self.exotic_armor.clone(),
            exotic_weapon: self.exotic_weapon.clone(),
            super_name: self.super_name.clone(),
            aspects: self.aspect.clone(),
            fragments: self.fragment.clone(),
            weapons: self.weapon.clone(),
            perks: self.perk.clone(),
            mods: self.mods.clone(),
            artifact_perks: self.artifact_perk.clone(),
            armor_sets: self.sets.clone(),
            activity: self.activity.clone(),
            player: self.player.clone(),
            solo: self.solo,
            clears_only: self.clears,
            include_passengers: self.include_passengers,
            since: self.since.clone(),
            until: self.until.clone(),
            group_by: match self.group_by {
                Group::Build => GroupBy::Build,
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
        assert_eq!(q.class.as_deref(), Some("Warlock"));
        assert_eq!(q.aspects.len(), 2);
        assert_eq!(q.mods, ["Firepower"]);
        assert_eq!(q.super_name.as_deref(), Some("Song of Flame"));
        assert!(q.clears_only);
        assert_eq!(q.group_by, GroupBy::ExoticArmor);
        assert_eq!(q.sort, SortBy::Players);
    }
}
