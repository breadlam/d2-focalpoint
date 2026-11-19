//! Ranking: how well a run went, and how much of that was the build.
//!
//! **Run performance** generalises Searchlight's operator-approved run
//! evaluator (`kills × speed × deaths`, ranked within one activity) so that
//! runs of different activities are comparable. Each factor is measured
//! against a [`Reference`] — a typical qualified clear of the same activity
//! with the same solo status — so a score of 1.0 is a typical clear anywhere.
//! Within one activity and solo status, runs keep the original evaluator's
//! order (the factors differ from it only by constants), except where a
//! factor hits its bounds — kills are bounded here too, because the wider
//! slate of activities can be farmed.
//!
//! **Player skill and build effect.** A good build in a great player's hands
//! looks good for two reasons. To separate them, a run's log performance
//! (centred on its reference) is modelled as
//!
//! ```text
//! y = skill(player) + effect(build) + noise
//! ```
//!
//! and fit by ridge-regularised alternating least squares ([`fit_effects`]),
//! the "baseline predictor" of collaborative filtering. Shrinkage pulls
//! effects with little evidence toward zero (an average player, an average
//! build). Most of a player's runs have no captured build; those runs inform
//! only their skill, which is what lets a build be judged against how its
//! players usually perform. `exp(effect)` reads as "players perform this many
//! times their usual level with this build".
//!
//! See `docs/adr/0006-ranking.md` for the data behind the defaults.

/// A typical qualified clear: medians over the clears of one activity with
/// one solo status.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reference {
    pub kills: f64,
    pub duration_s: f64,
    pub deaths: f64,
}

/// The factors of one run's performance; their product is the score.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Performance {
    /// Kills relative to the reference (the evaluator's linear base),
    /// within [`ScoreParams::kill_bounds`].
    pub kill_index: f64,
    /// Reference duration over the run's, within [`ScoreParams::speed_bounds`].
    pub speed: f64,
    /// The evaluator's death haircut relative to the reference's deaths,
    /// within [`ScoreParams::survival_bounds`].
    pub survival: f64,
}

impl Performance {
    /// `kill_index × speed × survival`; 1.0 is a typical clear.
    #[must_use]
    pub fn score(&self) -> f64 {
        self.kill_index * self.speed * self.survival
    }
}

/// Parameters of run performance and of the references it is measured
/// against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreParams {
    /// Bounds on the kill index. The evaluator's curated activities could
    /// not be farmed; the wider slate can (a 157-minute Crota's End clear
    /// with 13,814 kills, 21× typical), so kills are bounded at about the
    /// 1st and 99th percentiles of fireteam clears (0.09 and 3.9). Kills
    /// keep the widest range of the three factors.
    pub kill_bounds: (f64, f64),
    /// Bounds on the speed factor (the evaluator's 0.5–2×).
    pub speed_bounds: (f64, f64),
    /// The evaluator's per-death haircut: `1 / (1 + death_rate × deaths)`.
    pub death_rate: f64,
    /// Bounds on the survival factor. Deaths rank below speed, so the
    /// range is narrower.
    pub survival_bounds: (f64, f64),
    /// Activities whose median clear has fewer kills are not scored: kills
    /// cannot measure a build there (Pantheon boss encounters: 1–3).
    pub min_reference_kills: f64,
    /// A solo (or fireteam) reference needs this many clears; rarer cases
    /// borrow the whole activity's reference.
    pub min_reference_clears: u64,
}

impl Default for ScoreParams {
    fn default() -> Self {
        Self {
            kill_bounds: (0.1, 4.0),
            speed_bounds: (0.5, 2.0),
            death_rate: 0.15,
            survival_bounds: (0.5, 1.5),
            min_reference_kills: 20.0,
            min_reference_clears: 50,
        }
    }
}

impl ScoreParams {
    /// Performance of a clear, or `None` when it cannot be scored (no kills
    /// — a passenger or checkpoint bot — or no usable reference).
    #[must_use]
    pub fn performance(
        &self,
        kills: u32,
        deaths: u32,
        duration_s: u32,
        reference: &Reference,
    ) -> Option<Performance> {
        if kills == 0 || reference.kills <= 0.0 || reference.duration_s <= 0.0 {
            return None;
        }
        let kill_index =
            (f64::from(kills) / reference.kills).clamp(self.kill_bounds.0, self.kill_bounds.1);
        let speed = (reference.duration_s / f64::from(duration_s.max(1)))
            .clamp(self.speed_bounds.0, self.speed_bounds.1);
        let haircut = |d: f64| 1.0 / (1.0 + self.death_rate * d);
        let survival = (haircut(f64::from(deaths)) / haircut(reference.deaths))
            .clamp(self.survival_bounds.0, self.survival_bounds.1);
        Some(Performance {
            kill_index,
            speed,
            survival,
        })
    }
}

/// A run with a captured build: the player, the build's group, the run's
/// centred log performance, and the build's share of the run (a run split
/// across builds contributes one observation per build).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Observation {
    pub player: u32,
    pub group: u32,
    pub y: f64,
    pub weight: f64,
}

/// Sufficient statistics of a player's runs *without* a captured build:
/// their count and the sum of their centred log performance.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Baseline {
    pub weight: f64,
    pub sum: f64,
}

/// Ridge priors and convergence settings for [`fit_effects`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitParams {
    /// Pseudo-runs at an average skill added to every player.
    pub player_prior: f64,
    /// Pseudo-runs at no effect added to every build group.
    pub group_prior: f64,
    pub max_iterations: usize,
    /// Stop when no effect moves by more than this between iterations.
    pub tolerance: f64,
}

impl Default for FitParams {
    /// Priors chosen by five-fold cross-validation on the Searchlight data
    /// with builds grouped by their core (ADR 0006,
    /// `crates/db/examples/rank_tuning.rs`).
    fn default() -> Self {
        Self {
            player_prior: 3.0,
            group_prior: 10.0,
            max_iterations: 500,
            tolerance: 1e-7,
        }
    }
}

/// Fitted effects on the log scale; `exp` turns them into multipliers.
#[derive(Debug, Clone, PartialEq)]
pub struct Effects {
    pub skill: Vec<f64>,
    pub effect: Vec<f64>,
    pub iterations: usize,
    pub converged: bool,
}

/// Fit `y = skill(player) + effect(group)` by alternating ridge updates.
///
/// `baselines` has one entry per player (its length fixes the player count);
/// observation indices must be in range. Players and groups without data get
/// zero.
///
/// # Panics
///
/// If an observation's player or group index is out of range.
#[must_use]
pub fn fit_effects(
    baselines: &[Baseline],
    n_groups: usize,
    observations: &[Observation],
    params: &FitParams,
) -> Effects {
    let n_players = baselines.len();
    let mut player_den: Vec<f64> = baselines
        .iter()
        .map(|b| params.player_prior + b.weight)
        .collect();
    let mut group_den = vec![params.group_prior; n_groups];
    for o in observations {
        player_den[o.player as usize] += o.weight;
        group_den[o.group as usize] += o.weight;
    }

    let mut skill = vec![0.0; n_players];
    let mut effect = vec![0.0; n_groups];
    let mut player_num = vec![0.0; n_players];
    let mut group_num = vec![0.0; n_groups];
    let mut iterations = 0;
    let mut converged = false;
    while iterations < params.max_iterations {
        iterations += 1;
        for (n, b) in player_num.iter_mut().zip(baselines) {
            *n = b.sum;
        }
        for o in observations {
            player_num[o.player as usize] += o.weight * (o.y - effect[o.group as usize]);
        }
        let mut moved: f64 = 0.0;
        for ((s, n), d) in skill.iter_mut().zip(&player_num).zip(&player_den) {
            let new = n / d;
            moved = moved.max((new - *s).abs());
            *s = new;
        }

        group_num.fill(0.0);
        for o in observations {
            group_num[o.group as usize] += o.weight * (o.y - skill[o.player as usize]);
        }
        for ((e, n), d) in effect.iter_mut().zip(&group_num).zip(&group_den) {
            let new = n / d;
            moved = moved.max((new - *e).abs());
            *e = new;
        }
        if moved < params.tolerance {
            converged = true;
            break;
        }
    }
    Effects {
        skill,
        effect,
        iterations,
        converged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REF: Reference = Reference {
        kills: 200.0,
        duration_s: 1800.0,
        deaths: 4.0,
    };

    fn perf(kills: u32, deaths: u32, secs: u32) -> Performance {
        ScoreParams::default()
            .performance(kills, deaths, secs, &REF)
            .unwrap()
    }

    #[test]
    fn a_typical_clear_scores_one() {
        let p = perf(200, 4, 1800);
        assert!((p.score() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn unscorable_runs() {
        let params = ScoreParams::default();
        assert!(params.performance(0, 0, 600, &REF).is_none(), "no kills");
        let no_ref = Reference { kills: 0.0, ..REF };
        assert!(params.performance(10, 0, 600, &no_ref).is_none());
    }

    #[test]
    fn factors_are_bounded() {
        assert!((perf(200, 4, 60).speed - 2.0).abs() < 1e-12);
        assert!((perf(200, 4, 99_999).speed - 0.5).abs() < 1e-12);
        assert!((perf(200, 0, 1800).survival - 1.5).abs() < 1e-12);
        assert!((perf(200, 99, 1800).survival - 0.5).abs() < 1e-12);
        // 600 kills is three typical clears; a farm session is capped.
        assert!((perf(600, 4, 1800).kill_index - 3.0).abs() < 1e-12);
        assert!((perf(13_814, 4, 1800).kill_index - 4.0).abs() < 1e-12);
        assert!((perf(1, 4, 1800).kill_index - 0.1).abs() < 1e-12);
    }

    /// The original evaluator (`crates/core/src/evaluator.rs` in Searchlight),
    /// without its solo multiplier (solo runs have their own reference here).
    fn original(kills: u32, deaths: u32, secs: u32, reference_secs: f64) -> f64 {
        let speed = (reference_secs / f64::from(secs.max(1))).clamp(0.5, 2.0);
        f64::from(kills) * speed / (1.0 + f64::from(deaths) * 0.15)
    }

    #[test]
    fn within_an_activity_the_original_order_is_kept() {
        let runs = [
            (300, 2, 1500),
            (150, 0, 1200),
            (220, 5, 2400),
            (90, 1, 700),
            (400, 6, 2000),
        ];
        let mut ours: Vec<usize> = (0..runs.len()).collect();
        let mut theirs = ours.clone();
        let score = |&(k, d, s): &(u32, u32, u32)| perf(k, d, s).score();
        ours.sort_by(|&a, &b| score(&runs[b]).total_cmp(&score(&runs[a])));
        theirs.sort_by(|&a, &b| {
            let o = |r: &(u32, u32, u32)| original(r.0, r.1, r.2, REF.duration_s);
            o(&runs[b]).total_cmp(&o(&runs[a]))
        });
        assert_eq!(ours, theirs);
        // ...because, inside the bounds, the scores are proportional.
        let ratio = |r: &(u32, u32, u32)| score(r) / original(r.0, r.1, r.2, REF.duration_s);
        assert!((ratio(&runs[0]) - ratio(&runs[2])).abs() < 1e-12);
    }

    fn obs(player: u32, group: u32, y: f64) -> Observation {
        Observation {
            player,
            group,
            y,
            weight: 1.0,
        }
    }

    fn exact() -> FitParams {
        FitParams {
            player_prior: 1e-9,
            group_prior: 1e-9,
            ..FitParams::default()
        }
    }

    #[test]
    fn strong_players_do_not_make_a_build_look_good() {
        // Players 0-1 are strong (+0.5 in their runs without a captured
        // build), players 2-3 weak (-0.5). Strong players use build 0, weak
        // ones build 1, and both perform at their usual level: neither build
        // helps. Raw averages would rank build 0 a full point above build 1.
        let baselines = [
            Baseline {
                weight: 50.0,
                sum: 25.0,
            },
            Baseline {
                weight: 50.0,
                sum: 25.0,
            },
            Baseline {
                weight: 50.0,
                sum: -25.0,
            },
            Baseline {
                weight: 50.0,
                sum: -25.0,
            },
        ];
        let o = [
            obs(0, 0, 0.5),
            obs(1, 0, 0.5),
            obs(2, 1, -0.5),
            obs(3, 1, -0.5),
        ];
        let fit = fit_effects(&baselines, 2, &o, &exact());
        assert!(fit.converged);
        assert!(
            fit.effect.iter().all(|e| e.abs() < 1e-6),
            "{:?}",
            fit.effect
        );
        assert!((fit.skill[0] - 0.5).abs() < 1e-6);
        assert!((fit.skill[3] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_build_that_lifts_its_players_is_found() {
        // Every player is average without a captured build and 0.3 better
        // with build 0; build 1 changes nothing.
        let baselines = vec![
            Baseline {
                weight: 20.0,
                sum: 0.0
            };
            6
        ];
        let mut o = Vec::new();
        for p in 0..6 {
            o.push(obs(p, 0, 0.3));
            o.push(obs(p, 1, 0.0));
        }
        let fit = fit_effects(&baselines, 2, &o, &exact());
        assert!(fit.converged);
        // Skill absorbs a little of the lift (players run build 0 in 1 of 22
        // runs), so the effect is slightly under 0.3 but well clear of build 1.
        assert!(
            fit.effect[0] > 0.27 && fit.effect[0] <= 0.3,
            "{:?}",
            fit.effect
        );
        assert!(fit.effect[1].abs() < 0.03);
    }

    #[test]
    fn little_evidence_is_shrunk_toward_zero() {
        let params = FitParams {
            player_prior: 1e-9,
            group_prior: 3.0,
            ..FitParams::default()
        };
        // One run 0.8 above an average player's level (well established).
        let baselines = [Baseline {
            weight: 1e6,
            sum: 0.0,
        }];
        let fit = fit_effects(&baselines, 1, &[obs(0, 0, 0.8)], &params);
        assert!((fit.effect[0] - 0.8 / 4.0).abs() < 1e-3, "{:?}", fit.effect);
    }

    #[test]
    fn players_without_data_stay_average() {
        let baselines = [
            Baseline::default(),
            Baseline {
                weight: 5.0,
                sum: 2.0,
            },
        ];
        let params = FitParams {
            player_prior: 10.0,
            ..FitParams::default()
        };
        let fit = fit_effects(&baselines, 0, &[], &params);
        assert!(fit.skill[0].abs() < f64::EPSILON);
        // Five runs summing to 2, shrunk by ten pseudo-runs at zero.
        assert!((fit.skill[1] - 2.0 / 15.0).abs() < 1e-12);
        assert!(fit.converged);
    }
}
