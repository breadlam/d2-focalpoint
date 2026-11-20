//! Ranking: how well a run went, and how much of that was the build.
//!
//! **Run performance** generalises Searchlight's operator-approved run
//! evaluator (`kills × speed × deaths`, ranked within one activity) so that
//! runs of different activities are comparable. Each factor is measured
//! against a [`Reference`] — a typical qualified clear of the same activity
//! by the same kind of player (solo, fireteam member, or partial
//! participant) — so a score of 1.0 is a typical clear anywhere. Within one
//! activity and kind, runs keep the original evaluator's order (the factors
//! differ from it only by constants), except where a factor hits its bounds
//! — kills are bounded here too, because the wider slate of activities can
//! be farmed. A partial participant, present for only part of a fireteam's
//! run, is measured by the evaluator's kill rate over time played instead
//! ([`ScoreParams::partial_performance`], ADR 0009).
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

/// A typical qualified clear: medians over the clears of one activity by
/// one kind of player. For partial participants `duration_s` is the median
/// time played.
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
    /// within [`ScoreParams::kill_bounds`]; for a partial participant, the
    /// kill rate over time played relative to the reference's, within
    /// [`ScoreParams::rate_bounds`].
    pub kill_index: f64,
    /// Reference duration over the run's, within [`ScoreParams::speed_bounds`];
    /// 1 for a partial participant, whose pace is in the kill rate.
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
    /// A solo (or fireteam, or partial) reference needs this many clears;
    /// rarer cases borrow the whole activity's reference.
    pub min_reference_clears: u64,
    /// A fireteam member whose time played is under this share of the run
    /// joined late or left early: a **partial participant**, scored by
    /// [`Self::partial_performance`]. Nearly all fireteam members play
    /// 98–100% of the run; 7% of fireteam clears fall under 90%.
    pub partial_presence: f64,
    /// Floor on a partial participant's time played (Searchlight's
    /// evaluator's 120 s): a rate over a short stint is noise.
    pub min_played_s: u32,
    /// Bounds on a partial participant's kill-rate index, at about its 1st
    /// and 99th percentiles (0.26 and 2.49).
    pub rate_bounds: (f64, f64),
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
            partial_presence: 0.9,
            min_played_s: 120,
            rate_bounds: (0.25, 2.5),
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

    /// Performance of a partial participant: Searchlight's kill rate over
    /// time played, relative to the reference's (`reference.duration_s` is
    /// the time played of a typical partial participant), with deaths
    /// measured against the reference's deaths over the same time played.
    /// The run's kills and length are the fireteam's, not theirs; measuring
    /// both per minute played makes the score indifferent to how much of
    /// the run they saw.
    #[must_use]
    pub fn partial_performance(
        &self,
        kills: u32,
        deaths: u32,
        played_s: u32,
        reference: &Reference,
    ) -> Option<Performance> {
        if kills == 0 || reference.kills <= 0.0 || reference.duration_s <= 0.0 {
            return None;
        }
        let played = f64::from(played_s.max(self.min_played_s));
        let reference_played = reference.duration_s.max(f64::from(self.min_played_s));
        let rate = f64::from(kills) / played / (reference.kills / reference_played);
        let haircut = |d: f64| 1.0 / (1.0 + self.death_rate * d);
        let expected_deaths = reference.deaths * played / reference_played;
        let survival = (haircut(f64::from(deaths)) / haircut(expected_deaths))
            .clamp(self.survival_bounds.0, self.survival_bounds.1);
        Some(Performance {
            kill_index: rate.clamp(self.rate_bounds.0, self.rate_bounds.1),
            speed: 1.0,
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
    /// with builds grouped by their core and solo clears credited (ADR 0006,
    /// ADR 0008, `focalpoint analyze priors`).
    fn default() -> Self {
        Self {
            player_prior: 1.0,
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
    /// Context-specific effects, one per cell (empty without contexts).
    pub specific: Vec<f64>,
    pub iterations: usize,
    pub converged: bool,
}

/// Contexts for [`fit_effects_in_context`]: the cell — one build group in
/// one context, such as an activity group — of every observation, and the
/// cells' ridge prior.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contexts<'a> {
    /// The cell of each observation, parallel to the observations.
    pub cell_of: &'a [u32],
    pub cells: usize,
    /// Pseudo-runs at no context-specific effect added to every cell.
    pub prior: f64,
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
    fit(baselines, n_groups, observations, None, params)
}

/// Fit `y = skill(player) + effect(group) + specific(group, context)`.
///
/// The context-specific effects absorb what a build group does in one
/// context (an activity group) beyond what it does everywhere, so `effect`
/// keeps only what holds across contexts: a group that helps by the same
/// amount in several contexts keeps more of it in `effect` than one with the
/// same runs in a single context. With an infinite context prior this is
/// [`fit_effects`].
///
/// # Panics
///
/// If an index is out of range, or `contexts.cell_of` is not parallel to
/// `observations`.
#[must_use]
pub fn fit_effects_in_context(
    baselines: &[Baseline],
    n_groups: usize,
    observations: &[Observation],
    contexts: &Contexts<'_>,
    params: &FitParams,
) -> Effects {
    assert_eq!(
        contexts.cell_of.len(),
        observations.len(),
        "one cell per observation"
    );
    fit(baselines, n_groups, observations, Some(contexts), params)
}

fn fit(
    baselines: &[Baseline],
    n_groups: usize,
    observations: &[Observation],
    contexts: Option<&Contexts<'_>>,
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

    let n_cells = contexts.map_or(0, |c| c.cells);
    let mut cell_den = vec![contexts.map_or(0.0, |c| c.prior); n_cells];
    if let Some(c) = contexts {
        for (o, &cell) in observations.iter().zip(c.cell_of) {
            cell_den[cell as usize] += o.weight;
        }
    }
    // The context-specific effect of observation `i` (none without contexts).
    let cell = |i: usize| contexts.map(|c| c.cell_of[i] as usize);

    let mut skill = vec![0.0; n_players];
    let mut effect = vec![0.0; n_groups];
    let mut specific = vec![0.0; n_cells];
    let mut player_num = vec![0.0; n_players];
    let mut group_num = vec![0.0; n_groups];
    let mut cell_num = vec![0.0; n_cells];
    let mut iterations = 0;
    let mut converged = false;
    while iterations < params.max_iterations {
        iterations += 1;
        let spec = |i: usize, specific: &[f64]| cell(i).map_or(0.0, |c| specific[c]);
        for (n, b) in player_num.iter_mut().zip(baselines) {
            *n = b.sum;
        }
        for (i, o) in observations.iter().enumerate() {
            player_num[o.player as usize] +=
                o.weight * (o.y - effect[o.group as usize] - spec(i, &specific));
        }
        let mut moved: f64 = 0.0;
        for ((s, n), d) in skill.iter_mut().zip(&player_num).zip(&player_den) {
            let new = n / d;
            moved = moved.max((new - *s).abs());
            *s = new;
        }

        group_num.fill(0.0);
        for (i, o) in observations.iter().enumerate() {
            group_num[o.group as usize] +=
                o.weight * (o.y - skill[o.player as usize] - spec(i, &specific));
        }
        for ((e, n), d) in effect.iter_mut().zip(&group_num).zip(&group_den) {
            let new = n / d;
            moved = moved.max((new - *e).abs());
            *e = new;
        }

        if n_cells > 0 {
            cell_num.fill(0.0);
            for (i, o) in observations.iter().enumerate() {
                if let Some(c) = cell(i) {
                    cell_num[c] +=
                        o.weight * (o.y - skill[o.player as usize] - effect[o.group as usize]);
                }
            }
            for ((s, n), d) in specific.iter_mut().zip(&cell_num).zip(&cell_den) {
                let new = n / d;
                moved = moved.max((new - *s).abs());
                *s = new;
            }
        }
        if moved < params.tolerance {
            converged = true;
            break;
        }
    }
    Effects {
        skill,
        effect,
        specific,
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

    fn partial(kills: u32, deaths: u32, played: u32) -> Performance {
        ScoreParams::default()
            .partial_performance(kills, deaths, played, &REF)
            .unwrap()
    }

    #[test]
    fn partial_participants_are_measured_per_minute_played() {
        // The typical rate and deaths per minute score one at any presence.
        assert!((partial(200, 4, 1800).score() - 1.0).abs() < 1e-12);
        assert!((partial(100, 2, 900).score() - 1.0).abs() < 1e-12);
        assert!((partial(50, 1, 450).score() - 1.0).abs() < 1e-12);
        // Twice the rate is twice the score, whatever the run's length.
        let p = partial(200, 2, 900);
        assert!((p.kill_index - 2.0).abs() < 1e-12);
        assert!((p.speed - 1.0).abs() < 1e-12);
        // Deaths count against the deaths expected in the time played.
        assert!(partial(100, 4, 900).survival < 1.0);
        assert!(partial(200, 2, 1800).survival > 1.0);
    }

    #[test]
    fn partial_rates_are_bounded_and_floored() {
        assert!((partial(2_000, 4, 1800).kill_index - 2.5).abs() < 1e-12);
        assert!((partial(1, 4, 1800).kill_index - 0.25).abs() < 1e-12);
        // 30 seconds counts as two minutes.
        assert!((partial(20, 0, 30).kill_index - partial(20, 0, 120).kill_index).abs() < 1e-12);
        let params = ScoreParams::default();
        assert!(params.partial_performance(0, 0, 600, &REF).is_none());
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

    /// Average players (well established) gain 0.3 with either build: build
    /// 0 in three runs in each of four contexts, build 1 in twelve runs in
    /// one context.
    fn spread_and_concentrated() -> (Vec<Baseline>, Vec<Observation>, Vec<u32>) {
        let baselines = vec![
            Baseline {
                weight: 1e6,
                sum: 0.0
            };
            8
        ];
        let mut o = Vec::new();
        let mut contexts = Vec::new();
        for i in 0..12_u32 {
            o.push(obs(i % 4, 0, 0.3));
            contexts.push(i % 4);
            o.push(obs(4 + i % 4, 1, 0.3));
            contexts.push(0);
        }
        (baselines, o, contexts)
    }

    /// Cells as `fit_effects_in_context` wants them: one per (group, context).
    fn cells(o: &[Observation], contexts: &[u32]) -> (Vec<u32>, usize) {
        let mut index = std::collections::HashMap::new();
        let cell_of = o
            .iter()
            .zip(contexts)
            .map(|(o, &c)| {
                let next = index.len() as u32;
                *index.entry((o.group, c)).or_insert(next)
            })
            .collect();
        (cell_of, index.len())
    }

    #[test]
    fn an_infinite_context_prior_is_the_plain_fit() {
        let (baselines, o, contexts) = spread_and_concentrated();
        let (cell_of, n) = cells(&o, &contexts);
        let params = FitParams::default();
        let plain = fit_effects(&baselines, 2, &o, &params);
        let ctx = Contexts {
            cell_of: &cell_of,
            cells: n,
            prior: 1e15,
        };
        let fit = fit_effects_in_context(&baselines, 2, &o, &ctx, &params);
        for (a, b) in plain.effect.iter().zip(&fit.effect) {
            assert!((a - b).abs() < 1e-9, "{plain:?} {fit:?}");
        }
        assert!(fit.specific.iter().all(|s| s.abs() < 1e-9));
    }

    #[test]
    fn help_in_many_contexts_counts_more_than_help_in_one() {
        let (baselines, o, contexts) = spread_and_concentrated();
        let (cell_of, n) = cells(&o, &contexts);
        let params = FitParams {
            player_prior: 1e-9,
            group_prior: 3.0,
            ..FitParams::default()
        };
        let plain = fit_effects(&baselines, 2, &o, &params);
        // Without contexts the two builds are the same: 12 runs at +0.3.
        assert!((plain.effect[0] - plain.effect[1]).abs() < 1e-9);
        let ctx = Contexts {
            cell_of: &cell_of,
            cells: n,
            prior: 3.0,
        };
        let fit = fit_effects_in_context(&baselines, 2, &o, &ctx, &params);
        assert!(fit.converged);
        assert!(
            fit.effect[0] > fit.effect[1] + 0.05,
            "spread {} vs concentrated {}",
            fit.effect[0],
            fit.effect[1]
        );
        // The concentrated build's lift is still explained, by its context.
        let concentrated = fit.effect[1] + fit.specific[cell_of[1] as usize];
        assert!((concentrated - plain.effect[1]).abs() < 0.05, "{fit:?}");
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
