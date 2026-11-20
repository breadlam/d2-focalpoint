//! The ranking model's ridge priors, chosen by cross-validation.
//!
//! Runs with a captured build are split into five folds by run. Each fold is
//! predicted from a fit on the other four (plus every player's runs without
//! a captured build), and the weighted squared error of the prediction is
//! compared with predicting an average run (0). An infinite group prior
//! leaves builds out entirely (player skill alone); with a context prior the
//! fit also has build × activity group effects, and the prediction includes
//! the run's cell.

use focalpoint_core::rank::{Contexts, FitParams, Observation, fit_effects_in_context};
use rayon::prelude::*;

use crate::rank::ModelData;

pub const FOLDS: u64 = 5;

/// A group prior this large leaves builds out of the model.
pub const NO_BUILDS: f64 = 1e12;

/// One point of the grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Priors {
    pub player: f64,
    pub group: f64,
    /// `None`: no build × activity group effects.
    pub context: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    pub priors: Priors,
    /// Weighted mean squared error over all folds.
    pub error: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub observations: usize,
    pub groups: usize,
    /// The error of predicting 0 for every run.
    pub null_error: f64,
    pub scores: Vec<Score>,
}

impl Report {
    /// Share of the variance explained by an error.
    #[must_use]
    pub fn explained(&self, error: f64) -> f64 {
        1.0 - error / self.null_error
    }

    /// The best point of the grid.
    #[must_use]
    pub fn best(&self) -> Option<&Score> {
        self.scores
            .iter()
            .min_by(|a, b| a.error.total_cmp(&b.error))
    }

    /// The best point without build effects (player skill alone).
    #[must_use]
    pub fn best_without_builds(&self) -> Option<&Score> {
        self.scores
            .iter()
            .filter(|s| s.priors.group >= NO_BUILDS)
            .min_by(|a, b| a.error.total_cmp(&b.error))
    }

    /// The best point without context effects.
    #[must_use]
    pub fn best_without_contexts(&self) -> Option<&Score> {
        self.scores
            .iter()
            .filter(|s| s.priors.context.is_none())
            .min_by(|a, b| a.error.total_cmp(&b.error))
    }
}

/// The fold of a run: a fixed mix of its instance id, deterministic and
/// unrelated to time.
#[must_use]
pub fn fold(run: i64) -> u64 {
    ((run as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) % FOLDS
}

/// The weighted mean squared error of `priors` over all folds.
#[must_use]
pub fn cv_error(data: &ModelData, cell_of: &[u32], cells: usize, priors: Priors) -> f64 {
    let params = FitParams {
        player_prior: priors.player,
        group_prior: priors.group,
        ..FitParams::default()
    };
    let (mut err, mut weight) = (0.0, 0.0);
    for k in 0..FOLDS {
        let (mut train, mut train_cells) = (Vec::<Observation>::new(), Vec::new());
        let mut test = Vec::new();
        for (i, o) in data.observations.iter().enumerate() {
            if fold(data.observation_runs[i]) == k {
                test.push(i);
            } else {
                train.push(*o);
                train_cells.push(cell_of[i]);
            }
        }
        // An infinite context prior is the plain fit, with no specific effects.
        let contexts = Contexts {
            cell_of: &train_cells,
            cells,
            prior: priors.context.unwrap_or(f64::INFINITY),
        };
        let fit = fit_effects_in_context(&data.baselines, data.groups, &train, &contexts, &params);
        for i in test {
            let o = &data.observations[i];
            let specific = if priors.context.is_some() {
                fit.specific[cell_of[i] as usize]
            } else {
                0.0
            };
            let predicted = fit.skill[o.player as usize] + fit.effect[o.group as usize] + specific;
            err += o.weight * (o.y - predicted).powi(2);
            weight += o.weight;
        }
    }
    err / weight
}

/// Score every point of `grid`, in parallel.
#[must_use]
pub fn cross_validate(data: &ModelData, grid: &[Priors]) -> Report {
    let (sum, w) = data.observations.iter().fold((0.0, 0.0), |(s, w), o| {
        (s + o.weight * o.y * o.y, w + o.weight)
    });
    let (cell_of, cells) = data.cells();
    let scores = grid
        .par_iter()
        .map(|&priors| Score {
            priors,
            error: cv_error(data, &cell_of, cells, priors),
        })
        .collect();
    Report {
        observations: data.observations.len(),
        groups: data.groups,
        null_error: sum / w,
        scores,
    }
}

/// Every combination of the given priors.
#[must_use]
pub fn grid(player: &[f64], group: &[f64], context: &[Option<f64>]) -> Vec<Priors> {
    let mut out = Vec::new();
    for &p in player {
        for &g in group {
            for &c in context {
                // Contexts mean nothing without builds.
                if g >= NO_BUILDS && c.is_some() {
                    continue;
                }
                out.push(Priors {
                    player: p,
                    group: g,
                    context: c,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_are_balanced_and_fixed() {
        let mut counts = [0; FOLDS as usize];
        for run in 0..10_000 {
            counts[fold(run) as usize] += 1;
        }
        assert!(
            counts.iter().all(|&c| (1_800..2_200).contains(&c)),
            "{counts:?}"
        );
        assert_eq!(fold(123_456), fold(123_456));
    }

    #[test]
    fn the_grid_skips_contexts_without_builds() {
        let g = grid(&[3.0], &[10.0, NO_BUILDS], &[None, Some(10.0)]);
        assert_eq!(g.len(), 3);
        assert!(g.iter().all(|p| p.group < NO_BUILDS || p.context.is_none()));
    }
}
