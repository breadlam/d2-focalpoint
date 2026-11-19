//! Choose the ranking model's ridge priors by cross-validation.
//!
//! For each build grouping, runs with a captured build are split into five
//! folds by run. Each fold is predicted from a fit on the other four (plus
//! every player's runs without a captured build), and the weighted squared
//! error of `skill + effect` is compared with two baselines: predicting an
//! average run (0) and predicting from player skill alone (no build effects).
//! The share of variance explained beyond player skill is how much the build
//! adds.
//!
//! ```text
//! cargo run --release -p searchlight-db --example rank_tuning [DB]
//! ```

use std::path::PathBuf;

use rayon::prelude::*;
use searchlight_core::rank::{FitParams, Observation, fit_effects};
use searchlight_db::Database;
use searchlight_db::rank::{ModelData, RankOptions, load_model};
use searchlight_db::search::GroupBy;

const FOLDS: u64 = 5;

type Indexed<'a> = Vec<(usize, &'a Observation)>;
const PLAYER_PRIORS: [f64; 5] = [1.0, 3.0, 10.0, 30.0, 100.0];
const GROUP_PRIORS: [f64; 6] = [1.0, 3.0, 10.0, 30.0, 100.0, 1e12];

fn fold(run: i64) -> u64 {
    // A fixed mix of the instance id: deterministic, and unrelated to time.
    ((run as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) % FOLDS
}

/// Weighted mean squared error over all folds.
fn cv_error(data: &ModelData, player_prior: f64, group_prior: f64) -> f64 {
    let params = FitParams {
        player_prior,
        group_prior,
        ..FitParams::default()
    };
    let (mut err, mut weight) = (0.0, 0.0);
    for k in 0..FOLDS {
        let (train, test): (Indexed<'_>, Indexed<'_>) = data
            .observations
            .iter()
            .enumerate()
            .partition(|(i, _)| fold(data.observation_runs[*i]) != k);
        let train: Vec<Observation> = train.into_iter().map(|(_, o)| *o).collect();
        let fit = fit_effects(&data.baselines, data.groups, &train, &params);
        for (_, o) in test {
            let predicted = fit.skill[o.player as usize] + fit.effect[o.group as usize];
            err += o.weight * (o.y - predicted).powi(2);
            weight += o.weight;
        }
    }
    err / weight
}

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).map_or_else(
        || {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".local/share/searchlight/searchlight.duckdb")
        },
        PathBuf::from,
    );
    let db = Database::open_read_only(&path)?;
    for (name, group_by) in [
        ("core", GroupBy::Core),
        ("exact build", GroupBy::Build),
        ("exotic armor", GroupBy::ExoticArmor),
    ] {
        let data = load_model(
            &db,
            &RankOptions {
                group_by,
                ..RankOptions::default()
            },
        )?;
        let (sum, w) = data.observations.iter().fold((0.0, 0.0), |(s, w), o| {
            (s + o.weight * o.y * o.y, w + o.weight)
        });
        let null = sum / w;
        println!(
            "\n== grouping: {name} — {} observations, {} groups; error of predicting 0: {null:.4}",
            data.observations.len(),
            data.groups
        );
        let grid: Vec<(f64, f64)> = PLAYER_PRIORS
            .iter()
            .flat_map(|&p| GROUP_PRIORS.iter().map(move |&g| (p, g)))
            .collect();
        let results: Vec<(f64, f64, f64)> = grid
            .par_iter()
            .map(|&(p, g)| (p, g, cv_error(&data, p, g)))
            .collect();
        println!("player prior  group prior   cv error   explained");
        for (p, g, e) in &results {
            let label = if *g > 1e9 {
                "∞ (no build)".to_string()
            } else {
                format!("{g}")
            };
            println!(
                "{p:>12}  {label:>11}   {e:.4}     {:>5.1}%",
                100.0 * (1.0 - e / null)
            );
        }
        let best = results
            .iter()
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .expect("grid is not empty");
        let player_only = results
            .iter()
            .filter(|r| r.1 > 1e9)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .expect("grid has a player-only column");
        println!(
            "best: player prior {}, group prior {} → explains {:.1}% (player skill alone: {:.1}%; builds add {:.1} points)",
            best.0,
            best.1,
            100.0 * (1.0 - best.2 / null),
            100.0 * (1.0 - player_only.2 / null),
            100.0 * (player_only.2 - best.2) / null,
        );
    }
    Ok(())
}
