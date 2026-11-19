# ADR 0006: Ranking builds and players, separating the build from the player

- **Status:** accepted
- **Date:** 2026-11-19

## Context

Rankings should find good builds and good players. A build's raw results
mostly reflect who plays it: top players gravitate to strong builds, so a
build used by great players looks great whether or not it helps. The
value of the build has to be separated from the value of the player.

Searchlight already had an operator-approved run evaluator
(`searchlight/crates/core/src/evaluator.rs`, 2026-06-14) for choosing players
to monitor: `score = kills × speed × death haircut × solo`, genuine clears
only, ranked within one activity. Priority is kills ≫ speed > deaths; speed is
`reference time / duration` bounded to 0.5–2×; the death haircut is
`1 / (1 + 0.15·deaths)`; solo doubles the score.

## Decision

### 1. Population: production's qualified clears

Runs count when they met a Searchlight target's gates
(`run.qualified_target`: fresh start, difficulty tier, feat floor, after
2026-06-09), the player cleared, and the player had kills: about 3.0 M
player-runs by 625 k players. A player who switched characters mid-run has one
report entry per character; they are summed.

### 2. Run performance: the evaluator, normalised per activity

Activities differ wildly (median kills per clear range from 1 to 538, median
durations from 4 to 164 minutes), so each factor is measured against a
**reference**: the median kills, duration and deaths of qualified clears of
the same activity *and solo status* (`activity_reference`).

- `kill_index = kills / reference kills`, bounded to 0.1–4×;
- `speed = reference duration / duration`, bounded to 0.5–2× (as before);
- `survival = haircut(deaths) / haircut(reference deaths)`, bounded to
  0.5–1.5× — the evaluator's haircut, made relative so that activities with
  many deaths (Master raids: 14–19 per clear) are not penalised;
- `score = kill_index × speed × survival`; 1.0 is a typical clear anywhere.

Within one activity and solo status, runs keep the evaluator's order except
where a bound applies (a test checks the proportionality). Differences:

- **Solo** runs are compared with solo runs instead of doubling the score.
  Doubling on top of kills measured against fireteams would make every solo
  run outrank every fireteam run; `--solo` ranks soloists among themselves.
  Buckets with fewer than 50 clears (e.g. solo Master raids) borrow the
  activity-wide reference, so those rare feats keep their advantage.
- **Kills are bounded** because the wider slate can be farmed: a 157-minute
  Crota's End clear with 13,814 kills is 21× typical. The bounds are about the
  1st and 99th percentiles of fireteam clears (0.09× and 3.9×); kills keep the
  widest range of the three factors.
- **Activities where kills cannot measure a build** — median clear under 20
  kills: the Pantheon Morgeth and Insurrection Prime boss encounters (1–3) —
  are not scored.

### 3. Separating build from player: a two-way model

Each run's `perf = ln(score) − mean ln(score) of its reference population` is
modelled as

```text
perf = skill(player) + effect(build group) + noise
```

and fit by ridge-regularised alternating least squares — the "baseline
predictor" of collaborative filtering (`searchlight_core::rank::fit_effects`).
The ridge priors add pseudo-runs at zero, so players and builds with little
evidence are shrunk toward average.

The separation works because **most of a player's runs have no captured
build** (2.96 M of 3.03 M). Those runs pin down how good the player usually
is; the 66 k runs with a captured build then show how much better or worse
they do with a particular build. A run used with several builds is split by
each build's share of it. `exp(effect)` reads as "players perform this many
times their usual level with this build"; effects are reported relative to the
run-weighted average captured build (tracked players were chosen for standout
runs, so their untracked runs flatter their skill slightly and raw effects sit
just below zero).

The model is fit at query time (≈ 1.5 s) on the runs the run filters select,
so `--activity "ultimate conquest"` estimates skills and effects within that
content. Build facets and the player filter only choose what is listed;
skills are always net of every build their players used.

### 4. Priors from cross-validation

`crates/db/examples/rank_tuning.rs` predicts held-out runs with a captured
build (five folds by run) from a fit on the rest:

| Grouping | Player skill alone | Skill + build (best priors) | Builds add |
|---|---:|---:|---:|
| core: class, element, exotics, super, aspects | 33.8 % | 35.9 % (3, 10) | 2.1 points |
| exotic armor only | 33.8 % | 35.2 % (3, 30) | 1.4 points |
| exact build | 33.8 % | 35.2 % (1, 1) | 1.4 points |

(Share of the variance of held-out `perf` explained; priors as
(player, group) pseudo-runs. The remaining ~64 % is run-to-run variation:
teammates, modifiers, luck.) Before kills were bounded, a few farmers'
extreme runs made player skill look like 58 %.

Defaults: player prior 3, group prior 10 (best for the default grouping).
Exact builds do no better than core builds: nearly every exact build is one
player's.

## Consequences

- **Builds matter less than players.** Of the variation in run performance
  that can be predicted, player skill accounts for about 94 % and the build
  for about 6 % (2.1 of 35.9 points). Build effects are real but modest (the strongest are +15–35 %), and
  only builds used by many players are estimated well.
- **Effects are within-player comparisons**: what switching to a build does
  for players who also play other builds. A player's habitual build is
  partly absorbed into their skill. A build tied to one activity can also
  absorb a player's affinity for that activity; filter by `--activity` to
  compare within one.
- **Kill-based scoring undervalues support builds** whose value is in their
  team's kills (their contribution shows only through speed and survival).
- Schema v5 adds `activity_reference` and `run_score`; the import gains a
  ~7 s scoring stage. `meta` now records `investment_params` (previously
  defined but never written) and `score_params`.
