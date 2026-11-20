# ADR 0007: Highlights — the neutral game and diverse, well-ranked picks

- **Status:** accepted
- **Date:** 2026-11-19

## Context

Readers want a short collection of builds per class that *really* stand
apart, as well ranked as possible: for an activity, and for the **neutral
game** — the all-round workhorses worth keeping in a loadout because they do
well almost anywhere, even though a build tailored to one activity usually
beats them there.

Two findings shaped the design (`focalpoint analyze …`, `docs/analysis.md`):

1. **The all-activities ranking does not reward breadth.** It is one fit
   over every run, each run counted the same whatever its activity, so a
   build's effect is roughly its average gain per run, shrunk by the
   evidence. Breadth enters only through the number of runs: the rank
   correlation of effect with the number of activity groups a build type was
   used in is +0.03 (Hunter), −0.11 (Titan), −0.07 (Warlock). 38% of Titan's
   top 10% ran ≥ 80% of their runs in one group (Solar Hammer and Burning
   Maul builds in Equilibrium), against 15% of the rest.
2. **Hard thresholds both over- and under-select.** The tiered build
   distance calls almost any two builds distinct (an exotic swap alone is
   0.4; weapons and mods rarely match: median pair 0.94), and artifact
   perks — 7 of about 20 kit elements — make one subclass setup look half
   different. Thresholds on kit distance without artifact perks work better
   (one per subclass at ≥ 0.95, 9–14 per class at ≥ 0.7), but combined with
   a player minimum and a "top half" pool they leave small scopes with
   almost nothing: Titan had 2 candidates in Pantheon and 6 in The Desert
   Perpetual at ≥ 3 players, and one or two picks. Every tracked player was
   in the top 1% of those monitored; a build type seen once is evidence too.

## Decision

### 1. The neutral game: effects that hold across activity groups

`RankOptions::context_prior` adds a build × activity group effect to the
model (`focalpoint_core::rank::fit_effects_in_context`):

```text
perf = skill(player) + effect(build type) + specific(build type, activity group) + noise
```

with ridge priors on each. The specific effects absorb what a build does in
one kind of activity beyond what it does everywhere, so `effect` keeps only
what holds across groups: in the limit of many runs, a build type used in
one group keeps ≈ 23% of its effect as neutral effect, one spread over four
groups ≈ 55%, over ten ≈ 75%. That is the neutral-game ranking.

The prior was chosen by five-fold cross-validation with the others
(`focalpoint analyze priors --contexts`):

| Model (core build types) | Explained | Builds add |
|---|---:|---:|
| player skill alone | 33.8% | — |
| skill + build (3, 10) | 35.9% | 2.1 points |
| skill + build + build × activity group (3, 10, **3**) | 38.0% | 4.2 points |

Activity-specific effects double what builds explain: much of a build's
value is where it is used. (Group prior 30 with context prior 3 is 0.04
points better; the shared group prior of 10 is kept.)
`NEUTRAL_CONTEXT_PRIOR = 3`.

The neutral top 10% is no longer dominated by specialists: builds with
≥ 80% of their runs in one group fall from 16% → 2% (Hunter), 38% → 7%
(Titan), 17% → 12% (Warlock) of it.

### 2. Picks: a determinantal point process, without thresholds

Per class and set (the neutral game, every activity group and activity),
the candidates are **every** ranked build type — no player minimum, no rank
cut-off — represented by its exemplar build (the "top build" of ADR 0006's
book), **if that build is complete**: two aspects, at least four fragments,
all four abilities and armor mods (`analysis::completeness`). An incomplete
capture looks unlike everything only because of what it lacks — Warlock's
neutral five held a Void Vortex build seen once, with no aspects or
fragments, sharing its exotic weapon with another pick. The line is in the
data: 99.2% of builds have two aspects and 4–6 fragments, no aspect pair is
usually run with fewer than four, and the other 0.8% lack armor mods ten
times as often. Without the rule, 31 of 682 five-pick highlights (in 29 of
138 sets) were incomplete, every one a build type of one or two players;
with it every set still fills and the distances and ranks are unchanged
(`focalpoint analyze completeness`). `focalpoint_core::highlight::select_diverse` greedily maximises

```text
2 · w · quality_i + ln(novelty_i)
```

where quality is the build type's effect in the set (the neutral effect for
the neutral game) and novelty is the part of the candidate's playstyle not
already spanned by the picks so far (the greedy MAP of a DPP with kernel
`q_i q_j S_ij`, `q = exp(w · quality)`, via an incremental Cholesky).
The first pick is the best-ranked; a near-copy of a pick needs a better rank
to beat something new; when every candidate is alike, the best of them are
still picked. Picks keep their greedy order, so the top 3 is a prefix of the
top 10 and the site can offer 3, 5 or 10.

**Playstyle similarity** `S` is a weighted Jaccard (Ružička, a positive
semi-definite kernel) over the elements that define how a build plays:

| Element | Weight |
|---|---:|
| exotic armor | 4 |
| exotic weapon, subclass, super | 3 |
| aspect (each) | 2 |
| exotic armor perk (exotic class items) | 1.5 |
| grenade, melee, class ability, movement | 1 |
| fragment (each) | 0.5 |

Artifact perks (seasonal), legendary weapons and rolls, mods and stats are
left out: they vary between nearly identical builds.

**Quality weight** `w = 2`, from `focalpoint analyze highlights --sweep`. At
2, a candidate half like a pick (similarity 0.45, novelty 0.8) must be about
6 points of effect better than an entirely new one; at 5 the same costs 2
points and exotic swaps of a pick get in (Warlock's neutral ten held three
Well of Radiance / Heat Rises builds). Against the plain top 10, `w = 2`
costs about one point of mean effect and cuts the mean pairwise similarity
of the picks from 0.15–0.25 to 0.03–0.05 (largest pair: ≈ 0.75 → ≈ 0.4).
Titan's Pantheon and Desert Perpetual sets now fill ten picks from 32 and
110 candidates.

### 3. Export

`focalpoint site` writes one `book/highlights.<set>.json` per set (the
neutral game replaces the all-activities set), ten picks per class with
their exact builds; the site's **Highlights** view shows 3, 5 or 10.

## Consequences

- Completeness is the one hard boundary. Incomplete builds stay in the
  ranked builds view and the catalog (1.8% of listings have an incomplete
  top build); only highlights leave them out.
- Low-evidence build types compete on their shrunk effect. Under the model
  a single exceptional run is weak evidence, but not none, and
  several neutral picks are build types one player used once. Each card
  shows its players, runs and (for the neutral game) the activity groups it
  was used in, so readers can weigh it. A lower confidence bound would
  favour proven builds instead; it is not used, by choice.
- The neutral ranking is computed only for highlights; the ranked builds
  view keeps the plain model (its numbers are per-run strength).
- Retune with `focalpoint analyze priors --contexts` and
  `focalpoint analyze highlights --sweep` after a re-import; the constants
  are `NEUTRAL_CONTEXT_PRIOR` (`crates/db/src/rank.rs`) and
  `QUALITY_WEIGHT` (`crates/db/src/highlight.rs`).

## Alternatives considered

- **Hard thresholds** (no shared exotic, kit distance ≥ τ, best-ranked
  first from a pool) — `focalpoint analyze diversity`; rejected for the
  reasons above.
- **Maximal marginal relevance** (`quality − λ · max similarity`): one
  number per pair only, so two half-similar picks do not add up; the DPP's
  novelty accounts for the picks jointly.
- **Averaging per-group rankings** for the neutral game: each group's
  effect is shrunk separately, so a build spread thinly over many groups
  is shrunk in each and punished for its breadth.
