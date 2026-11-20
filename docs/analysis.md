# Analyses

The analyses behind Focalpoint's design decisions are part of the toolkit,
so they can be rerun after a re-import and extended:
`focalpoint analyze <analysis>` (code in `crates/db/src/analysis/`, the CLI
in `crates/cli/src/commands/analyze.rs`). Each prints tables (`--format tsv`
for further processing). The numbers below are from the import of
2026-11-19 (594,340 builds; 159,532 build observations in qualified clears).

| Command | Question | Drives |
|---|---|---|
| `analyze priors [--contexts] [--grouping …]` | Which ridge priors predict held-out runs best? | `FitParams::default`, `NEUTRAL_CONTEXT_PRIOR` (ADR 0006, 0007) |
| `analyze breadth [--min-players N] [--top N]` | Does a build type's rank depend on how many activity groups it was used in? | the neutral-game ranking (ADR 0007) |
| `analyze diversity [--min-players N]` | How many well-ranked build types are distinct under hard thresholds? | why highlights have no thresholds (ADR 0007) |
| `analyze highlights [--set S] [--class C] [--sweep]` | Why was each highlight picked, and how do picks change with the quality weight? | `QUALITY_WEIGHT` (ADR 0007) |

## `analyze priors`

Five-fold cross-validation by run: each fold of runs with a captured build is
predicted from a fit on the others (plus every player's runs without one),
over a grid of player, group and — with `--contexts` — build × activity
group priors. "Explained" is the share of the variance of held-out
performance; an infinite group prior is player skill alone.

Findings (core build types):

- player skill alone explains 33.8%; build effects add 2.1 points (best:
  player 3, group 10);
- with build × activity group effects the best model explains 38.0% (player
  3, group 10–30, context 3): **activity-specific effects double what builds
  explain**. The remaining ~62% is run-to-run variation (teammates,
  modifiers, luck).

## `analyze breadth`

Breadth is the effective number of activity groups a build type was used
in, `(Σ runs)² / Σ runs²`. For each class: rank correlations of the
all-activities effect and the neutral effect with breadth and runs; bands by
breadth with their share of the top 10%; how concentrated the top 10% is;
how the top 10% fares in each group's own ranking; and the top listings.

Findings (≥ 3 players):

| | Hunter | Titan | Warlock |
|---|---:|---:|---:|
| effect ~ breadth (rank correlation) | +0.03 | −0.11 | −0.07 |
| effect ~ runs | +0.34 | +0.25 | +0.26 |
| top 10% with ≥ 80% of runs in one group | 16% | 38% | 17% |
| … of the rest | 15% | 15% | 19% |
| neutral top 10% with ≥ 80% of runs in one group | 2% | 7% | 12% |
| top 10% above zero in their groups' own rankings | 121 / 167 | 73 / 88 | 79 / 102 |

The all-activities ranking measures per-run strength anywhere: breadth only
matters through the amount of evidence, and Titan's top is dominated by
Equilibrium specialists. The neutral ranking moves them out (Hallowfire
Heart Burning Maul: +16% overall, +6% neutral) and lifts proven
all-rounders (Relativism + Khvostov Golden Gun, used in 8 of 10 groups:
+14% overall, +16% neutral).

## `analyze diversity`

Candidates are each build type's exemplar with at least `--min-players`
players. For six criteria of "distinct" — the weighted tiered distance
≥ 0.9 with and without artifact perks, and no shared exotic plus a kit
distance (without artifact perks) of at least 0.5, 0.7, 0.85 or 0.95 — it
counts *rank-first* sets (best-ranked first, keeping each candidate distinct
from all kept) and *most-distinct* sets (a greedy maximum independent set),
in the whole list, its top half and its top quarter; then, per scope, the
rank-first set at kit ≥ 0.7 and how its picks relate to the class's
all-activities set (a new playstyle, the same playstyle with other exotics,
or a near repeat).

Findings (≥ 3 players, all activities, top half; rank-first / most):

| Distinct when | Hunter | Titan | Warlock |
|---|---|---|---|
| weighted distance ≥ 0.9 | 10 / 15 | 11 / 12 | 11 / 15 |
| no shared exotic, kit ≥ 0.7 | 11 / 14 | 9 / 14 | 12 / 16 |
| no shared exotic, kit ≥ 0.85 | 7 / 9 | 7 / 8 | 7 / 8 |
| no shared exotic, kit ≥ 0.95 | 6 / 6 | 6 / 6 | 6 / 6 |

- The weighted distance is saturated (median pair 0.94): an exotic swap
  alone is 0.4, and weapon rolls and mods rarely match, so its "distinct"
  sets hold near-copies.
- Kit ≥ 0.95 is one build per subclass element; 0.85 adds a second super
  for a subclass or two.
- Per activity, the hard rules leave small scopes nearly empty (Titan: one
  pick in Pantheon, two in The Desert Perpetual), and of 311 per-activity
  picks only 8 were new playstyles; 95 were a class playstyle with other
  exotics and 208 near repeats.

## `analyze highlights`

Lists a set's picks per class (`--set neutral` or a scope id; `--sets`
lists them) with each pick's rank among the candidates, effect, players,
runs, novelty and its most similar earlier pick; `--sweep` compares quality
weights (`--weights 0.5,1,2,…,inf`; `inf` is the plain ranking) by the
picks' mean effect, mean and largest pairwise similarity, and median and
worst rank.

Findings (ten picks):

| Neutral game | Hunter | Titan | Warlock |
|---|---|---|---|
| plain top 10: mean effect · mean / max similarity | +13% · 0.25 / 0.75 | +11% · 0.13 / 0.77 | +16% · 0.15 / 0.53 |
| `w = 2`: mean effect · mean / max similarity | +11% · 0.03 / 0.38 | +10% · 0.03 / 0.33 | +14% · 0.05 / 0.38 |
| `w = 2`: worst rank of a pick (of thousands) | 37 | 30 | 32 |

Diversity is cheap: about one point of mean effect. Weights of 5 and above
let exotic swaps of a pick in (largest similarity 0.45–0.69).
