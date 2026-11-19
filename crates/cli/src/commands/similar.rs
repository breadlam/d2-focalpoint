//! `searchlight similar` — a build's nearest neighbours by tiered distance.

use std::path::Path;

use anyhow::Result;
use searchlight_db::Database;
use searchlight_db::similar::{BuildSummary, SimilarOptions, nearest_builds};

use crate::output::{Format, render, thousands};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The build to compare against (`build.build_id`)
    pub build_id: i64,

    /// How many neighbours to list
    #[arg(long, default_value_t = 10)]
    pub limit: usize,

    /// Compare against builds of every class, not just the target's
    #[arg(long)]
    pub any_class: bool,

    /// Include builds only used by players who never got a kill with them
    /// (checkpoint bots and passengers)
    #[arg(long)]
    pub include_passengers: bool,

    #[arg(long, value_enum, default_value_t)]
    pub format: Format,
}

const STATS: [&str; 6] = ["Weapons", "Health", "Class", "Grenade", "Super", "Melee"];

/// "Super 4, Grenade 4*, Weapons 2" — invested stats by level; `*` marks a
/// stat read as held back by budget.
fn stat_summary(s: &BuildSummary) -> String {
    let mut stats: Vec<(u8, &str)> = s
        .stat_levels
        .iter()
        .zip(STATS)
        .filter(|(l, _)| **l > 0)
        .map(|(l, n)| (*l, n))
        .collect();
    stats.sort_by_key(|&(level, _)| std::cmp::Reverse(level));
    if stats.is_empty() {
        return "—".into();
    }
    stats
        .iter()
        .map(|(l, n)| {
            let mark = if s.stats_promoted.iter().any(|p| p == n) {
                "*"
            } else {
                ""
            };
            format!("{n} {l}{mark}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn or_dash(v: Option<&String>) -> String {
    v.cloned().unwrap_or_else(|| "—".into())
}

pub fn run(args: &Args, db: &Path) -> Result<()> {
    let db = Database::open_read_only(db)?;
    let opts = SimilarOptions {
        limit: args.limit,
        same_class: !args.any_class,
        include_passengers: args.include_passengers,
        ..SimilarOptions::default()
    };
    let result = nearest_builds(&db, args.build_id, &opts)?;
    let t = &result.target;
    println!(
        "Build {} — {} {} · exotics: {} / {} · super: {}",
        t.build_id,
        or_dash(t.class.as_ref()),
        or_dash(t.element.as_ref()),
        or_dash(t.exotic_armor.as_ref()),
        or_dash(t.exotic_weapon.as_ref()),
        or_dash(t.super_name.as_ref()),
    );
    println!(
        "  aspects: {}",
        if t.aspects.is_empty() {
            "—".into()
        } else {
            t.aspects.join(", ")
        }
    );
    println!("  stats: {}   (* = held back by budget)", stat_summary(t));
    println!(
        "  used in {} player-runs by {} players; compared with {} builds",
        thousands(t.timeline_count),
        thousands(t.player_count),
        thousands(result.candidates as i64)
    );
    println!(
        "  distance = 0.4·exotics + 0.3·kit + 0.2·armor + 0.1·weapons (each a Jaccard distance, 0–1)\n"
    );

    let header: Vec<String> = [
        "build_id",
        "distance",
        "exotics",
        "kit",
        "armor",
        "weapons",
        "slots",
        "runs",
        "players",
        "exotic armor",
        "exotic weapon",
        "super",
        "stats",
    ]
    .map(String::from)
    .to_vec();
    let rows: Vec<Vec<String>> = result
        .neighbours
        .iter()
        .map(|n| {
            let d = n.distance;
            let s = &n.summary;
            vec![
                s.build_id.to_string(),
                format!("{:.3}", d.total),
                format!("{:.2}", d.per_tier[0]),
                format!("{:.2}", d.per_tier[1]),
                format!("{:.2}", d.per_tier[2]),
                format!("{:.2}", d.per_tier[3]),
                n.slot_edits.to_string(),
                s.timeline_count.to_string(),
                s.player_count.to_string(),
                or_dash(s.exotic_armor.as_ref()),
                or_dash(s.exotic_weapon.as_ref()),
                or_dash(s.super_name.as_ref()),
                stat_summary(s),
            ]
        })
        .collect();
    println!("{}", render(args.format, &header, &rows));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_summary_orders_by_level_and_marks_budget_limited() {
        let s = BuildSummary {
            build_id: 1,
            class: None,
            element: None,
            exotic_armor: None,
            exotic_weapon: None,
            super_name: None,
            aspects: vec![],
            stat_levels: vec![2, 0, 0, 4, 4, 0],
            stats_promoted: vec!["Grenade".into()],
            timeline_count: 0,
            player_count: 0,
        };
        assert_eq!(stat_summary(&s), "Grenade 4*, Super 4, Weapons 2");
        assert_eq!(
            stat_summary(&BuildSummary {
                stat_levels: vec![],
                ..s
            }),
            "—"
        );
    }
}
