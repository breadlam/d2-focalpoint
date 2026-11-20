# ADR 0009: Partial participants are scored by kill rate over time played

- **Status:** accepted
- **Date:** 2026-11-20
- **Amends:** ADR 0006 §2 (references per activity and solo status), ADR
  0008 (the fireteam clears a solo clear is compared with)

## Context

ADR 0006 measures a clear's kills against the activity's reference kills
and its pace against the run's length. A fireteam member who joined late
or left early is measured against the whole run though they saw only part
of it. In dungeons, raids, Equilibrium, Pantheon and The Desert Perpetual,
runs resume from checkpoints (22–80% of recorded runs) and take players in
progress (16–48%); Conquests and Ops cannot be resumed (0.01%) and almost
never take late joiners into qualified runs (`focalpoint analyze partial`).

Searchlight's later evaluator scored a player's kill rate over their own
time played, `(kills / time played) / (1 + deaths) × 2 if solo`, with a
120-second floor on time played. A late joiner was judged on the pace they
sustained.

What the data shows (`focalpoint analyze partial`, `docs/analysis.md`):

- Nearly every fireteam member plays 98–100% of the run (2.56 M of 2.87 M
  scored fireteam clears). Below that the tail is smooth: 208 k clears (7.3%
  of fireteam clears) fall under 90%, spread from a few minutes to most of
  the run.
- Whole-run scoring penalises presence, not play. Players present for
  80–90% of a run score 0.80 of a typical fireteam member, those present
  for under 10% score 0.17.
- Searchlight's rate measured against the fireteam reference overshoots
  the other way (1.08–1.42 by activity group). A stretch someone joins for
  is not a typical stretch: short stints at a checkpoint are add-dense.
- Measured against other partial participants of the same activity, the
  kill rate is close to neutral at every presence level: 0.95–1.14
  (geometric means). Their deaths have to be measured against the deaths
  expected in the time played, too. Raw deaths against the bucket's median
  deaths favour short stints, scoring 0.86 at 70–80% presence and 1.41
  under 10%.

## Decision

A fireteam member whose time played is under 90% of the run
(`ScoreParams::partial_presence`) is a **partial participant**. Their
clears get a reference bucket of their own per activity, beside the solo
and fireteam buckets (`activity_reference.is_partial`, `run_score.is_partial`),
and are scored by `ScoreParams::partial_performance`:

```text
kill_index = (kills / played) / (reference kills / reference played)   bounded 0.25–2.5
speed      = 1                                                        (the pace is in the rate)
survival   = haircut(deaths) / haircut(reference deaths × played / reference played)
```

- **Time played** is the player's own (summed over characters), floored at
  Searchlight's 120 seconds (`min_played_s`). The reference's time played is
  the median over the bucket.
- **The rate bounds** sit at about the 1st and 99th percentiles of partial
  participants' rate index (0.26 and 2.49), the rule used for kills.
- **Centring and borrowing** work as for the other buckets: `perf` is
  centred on the bucket's mean log score; an activity with fewer than 50
  partial clears borrows the activity-wide reference (8 activities, 59
  clears).
- **The solo premium's fireteam reference leaves partial participants
  out** (ADR 0008): the qualified fireteam bucket no longer holds them, and
  the unqualified fallback pool drops players under 90% presence. A soloist
  does the whole run's work, so the comparison is with players who saw the
  whole run.

Solo clears are never partial participants. Short solo clears in
checkpointed content (resumed runs) are unchanged: they are scored over
the run as before, and ADR 0008 keeps them out of the premium.

Schema version 7.

## Consequences

- Partial participants are no longer penalised for absence: every presence
  level from under 10% to 90% scores 0.95–1.14 against a typical
  partial participant, where whole-run scoring gave 0.17–0.80.
- The fireteam reference of checkpointed content rises slightly without
  latecomers, so its solo premiums fall slightly: Equilibrium ×2.8 → ×2.64,
  Master dungeons ×2.4–3.3 → ×2.2–3.2. Conquests and Ops have no
  latecomers and barely move.
- Rankings move little: per class, rank correlation with the previous
  all-activities ranking is +0.987–0.989; 68 of 80 (Hunter), 49 of 54
  (Titan) and 71 of 80 (Warlock) of the top 10% are unchanged; 604 of 649
  highlight picks are unchanged.
- Cross-validation keeps the priors (player 1, group 10, context 3). Player
  skill alone explains 41.6% of held-out performance, builds add 5.6
  points, 1.7 of them activity-specific. ADR 0008 recorded 40.9%, 6.6 and 1.9
  before this change and the checkpoint fix. Some of what builds seemed to
  explain was who joined runs late.
- A partial participant's run counts fully in the ranking model, like any
  other clear: their build was used for the time they played.
- Runs where a player switched characters can sum to more than the run's
  length; those are full participants.
