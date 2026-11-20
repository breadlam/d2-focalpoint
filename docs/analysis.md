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
| `analyze completeness [--count K]` | Which captured builds are missing significant parts, how much evidence do they carry, and what do they do to highlights? | the completeness boundary of highlights (ADR 0007) |
| `analyze subclasses [--neutral-per N] [--prismatic N] [--activity-per N]` | What would a quota of highlights per subclass pick, and how distinct, well ranked and well evidenced are those picks? | how highlights are organised |
| `analyze solo [--min-players N] [--top N]` | How did Searchlight capture solo play, what does a solo clear take against the fireteam clears, do solo clears look like full clears, and what does crediting them do to the rankings? | the solo premium (ADR 0008) |
| `analyze breadth [--min-players N] [--top N]` | Does a build type's rank depend on how many activity groups it was used in? | the neutral-game ranking (ADR 0007) |
| `analyze diversity [--min-players N]` | How many well-ranked build types are distinct under hard thresholds? | why highlights have no thresholds (ADR 0007) |
| `analyze highlights [--set S] [--class C] [--sweep]` | Why was each highlight picked, and how do picks change with the quality weight? | `QUALITY_WEIGHT` (ADR 0007) |
| `analyze highlights --survey [--count K] [--quality-weight W]` | Across every activity and class, what distances, ranks and evidence do K picks have when the constraints are loosened? | how many highlights the data supports |

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

## `analyze completeness`

Counts builds by aspects and fragments (with how many of each cell lack
armor mods or artifact perks), checks two-aspect builds against their
aspect pair's usual (modal) fragment count, compares the evidence behind
complete and incomplete builds, counts ranked listings whose top build is
incomplete, and lists the highlight picks that would be incomplete without
the rule, with the survey summary with and without them.

Findings:

- Every captured build has a subclass, super, abilities, three weapons and
  five armor pieces. 99.2% have two aspects and 4–6 fragments; the other
  0.8% (4,785) lack armor mods and artifact perks far more often (e.g. 61%
  and 66% of the 1,088 builds with no aspects, against about 1%), the mark of
  a loadout read mid-change or sockets the API did not return.
- No aspect pair is usually run with fewer than four fragments, so two
  aspects and ≤ 3 fragments (2,940 builds) means unfilled slots. Four
  fragments where the pair usually has five (7,778) is kept: a slot left
  empty is plausible.
- Complete (two aspects, ≥ 4 fragments, four abilities, armor mods):
  586,193 builds; incomplete: 8,147. Exact builds are always one player's,
  so evidence is judged at the build type: every incomplete highlight pick
  was a build type of one or two players.
- Without the rule, 31 of 682 five-pick highlights (29 of 138 sets) were
  incomplete; leaving incomplete builds out changes neither how many sets
  fill nor the picks' distances and ranks.
- 737 of 41,892 ranked listings (1.8%) have an incomplete top build, some
  of build types with up to 64 players; 55 of 10,669 build types have no
  aspects at all.

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

### `--survey`: loosening the constraints

For every set (the neutral game, each activity group and activity) and
class, `K` picks three ways: the **strict** hard rules of the diversity
study (no shared exotic, kit distance ≥ 0.7, best-ranked first, from the
top half of build types with ≥ 3 players); the same rules **loosened** to
every build type; and the **soft** selection (no thresholds). It reports the
smallest kit and playstyle distances between the soft picks, whether they
meet the hard rule anyway, the effect given up against the plain top `K`,
the worst rank picked, and how many picks one player used.

Findings (138 sets × classes):

| | 3 picks | 5 picks | 10 picks |
|---|---|---|---|
| sets filled: strict / loosened hard rules / soft | 58 / 136 / 136 | 23 / 132 / 136 | 3 / 71 / 133 |
| loosened hard rules: median worst rank (sets with ≥ 50 build types) | 5 | 15 | 87 |
| soft, `w = 2`: median smallest kit distance (p10) | 1.00 (0.56) | 0.68 (0.40) | 0.47 (0.29) |
| soft, `w = 2`: sets meeting the hard rule anyway | 84 / 136 | 38 / 136 | 0 / 133 |
| soft, `w = 0.5`: median smallest kit distance; sets meeting the rule | — | 1.00; 78 / 136 | 0.59; 3 / 133 |
| soft: mean effect given up (median, `w = 2` / `0.5`) | 0.3 points | 0.8 / 1.4 points | 1.1 / 2.1 points |
| one-player picks: plain top K / loosened hard / soft `w = 2` | 40% / 50% / 46% | 46% / 60% / 54% | 54% / 72% / 68% |

- The strict study's empty sets came from its player minimum and top-half
  pool, not from the distinctness rule: loosened, the hard rule fills five
  picks in 132 of 138 sets (Titan: Pantheon from 32 build types, worst rank
  6; The Desert Perpetual from 110, worst rank 8).
- Three picks are mutually distinct almost everywhere; five usually are
  (more so at a lower quality weight); ten are not — the tenth pick shares
  its subclass and super with an earlier one, and the hard rule can fill
  ten in only half the sets, reaching rank ~87 to do it.
- Diversity costs about a point of mean effect throughout.
- Most collected build types were one player's: even the plain top five are
  46% one-player; loosening adds roughly another 10 points of that.

## `analyze subclasses`

Picks highlights per subclass (complete builds, chosen within each
subclass as everywhere else): by default three per classic subclass and six
Prismatic for the neutral game (21 per class), and one per subclass for each
activity. For each pick: its rank within the subclass and the class, its
effect, players, runs and the activity groups it was used in; per subclass:
the smallest kit and playstyle distances, distinct supers and aspect pairs,
and shared exotics; and a comparison with picking as many without a quota.

Findings, neutral game (21 per class):

- All 63 picks rank above zero; 39 had two or more players and 42 were used
  in two or more activity groups.
- Within a classic subclass the picks differ in aspects and exotics, often
  not in super (Hunter Arc's three are all Gathering Storm; Stasis and
  Strand picks share their super for every class): smallest kit distance
  0.33–0.70, against ≈ 1 across subclasses.
- Warlock Stasis has no strong all-rounders: its three picks are one player
  each, used in one activity group, ranked 307th–448th of the class (+1–2%).
  Without a quota the selection picks no Warlock Stasis build at all.
- Six Prismatic picks repeat for Hunter (four of six are Golden Gun:
  Marksman with Ascension; smallest kit distance 0.38, three pairs share an
  exotic); Titan's six span five supers.

Findings, activities (one per subclass):

- 97 of 135 activity × class sets have candidates in all six subclasses,
  28 in five, 10 in four or fewer (single activities with few builds).
- The quota costs a median 1.2 points of mean effect (p90 3.4) against six
  picks without it; 12% of its picks rank below zero for the activity
  (without the quota, 4%) — e.g. Titan Strand in Vault of Glass: −7%.

## `analyze solo`

Searchlight doubled a solo clear's score; ADR 0006 measures solo clears
against other solo clears of the same activity instead, so a build that
carries one player through a fireteam's work earns nothing for it. Every
activity Searchlight collected is fireteam content (three- or six-player).
The study: how Searchlight qualified and captured runs per activity group;
scored and solo clears; solo clears that started from the beginning against
the fireteam clears of their activity (kill index, speed, survival and the
**premium** — how much better a solo clear scores against the fireteam
clears than against other solo clears); the per-activity premium of
`SoloScoring::ActivityCredit`; and the all-activities build ranking with
solo clears credited, per class. `focalpoint rank builds --solo-scoring
credit|fireteam` ranks that way (`alone`, the default, is today's scoring).

Findings:

- **Searchlight's gates shape the data** (its target slate, see
  `docs/data-sources.md`). Five Grandmaster Conquests qualified solo runs
  only (`solo_only`): 280,989 of their 280,994 qualified runs are solo,
  while 482,433 of their 482,745 unqualified runs are fireteam runs. Their
  qualified clears are all solo clears of three-player content, and their
  fireteam baseline has to come from unqualified fireteam clears (cleared,
  started from the beginning, same period). Elsewhere qualification does not
  favour solo runs.
- **Solo play was captured far more often.** A solo clear had a captured
  build 5–13× as often as a fireteam clear in most groups (Equilibrium
  11.6% against 1.7%, Master dungeons 17.8% against 1.4%), 56× in Master
  raids and 135× in Pantheon: Searchlight chose the players it monitored with an evaluator that
  doubled solo scores, and builds were captured for monitored players. The
  build data leans solo.
- Fresh solo clears make 2.6–2.8× a fireteam member's kills at 0.64–0.95×
  the pace. The premium: ×1.2–1.5 in Grandmaster Conquests (soloists there
  are slower and die more than fireteams), ×1.5–2.2 in Ultimate Ops, ×2.8 in
  Equilibrium, ×3.0–3.1 in Ultimate Conquest, ×2.4–3.3 in Master dungeons
  and ×6.1 in Root of Nightmares (Master) — Searchlight's ×2, measured.
- Pantheon and The Desert Perpetual solo clears are mostly fresh (96% and
  84%) five-minute boss encounters with a tenth to a third of a fireteam
  member's kills: kills cannot measure soloing there (Searchlight captured
  Pantheon without using it to judge players), the measured premium is
  below one, and none is credited.
- **Not every flagged clear is a full clear.** Solo Root of Nightmares
  (Master) clears run a median 54 minutes, fastest 14 — a tenth under the
  24-minute Normal solo world record — with 931 kills against a fireteam's
  2,360; solo King's Fall (Master) a median 8 minutes. A fifth to two
  fifths of Master dungeon solo clears beat the fastest 1% of fireteam
  clears, some in a minute or two; Conquests, Ops and Equilibrium 0–7%. So
  the premium counts only solo clears at least as long as the fastest 1% of
  fireteam clears, and raids (activity mode 4) get none.
- Credited per activity (full-looking fresh solo clears only; never below
  ×1; no raids), the all-activities ranking keeps a rank correlation of +0.88–0.92 with
  today's; a third of each class's top 10% changes, and 88–96% of the
  credited top 10% have a solo clear (67–70% of the rest). The builds
  elite players take into fireteam content alone rise: Fortune's Favor +
  Wolfsbane Shadowshot (soloed by 139 players) from 29th to 1st of Hunter
  build types; Hallowfire Heart + Tractor Cannon Hammer of Sol from 13th
  to 1st of Titan's.
