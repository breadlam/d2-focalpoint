# ADR 0008: A solo premium for soloing fireteam content

- **Status:** accepted
- **Date:** 2026-11-20
- **Amends:** ADR 0006 §2 (solo runs compared with solo runs)

## Context

Every activity Searchlight collected is fireteam content: three-player
(Conquests, Ops, dungeons) or six-player (raids, Pantheon). A build that
carries one player through a fireteam's work — roughly three times the
attention of enemies in a three-player mission — is a stellar build.
Searchlight's evaluator doubled a solo clear's score for that reason. ADR
0006 instead measured solo clears only against other solo clears of the
same activity, so soloing earned a build nothing.

What the data shows (`focalpoint analyze solo`, `docs/analysis.md`):

- Measured against the fireteam clears of the same activity, a solo clear
  makes 2.6–2.8× a fireteam member's kills at 0.64–0.95× the pace: it
  scores ×1.2–1.5 (Grandmaster Conquests), ×1.5–2.2 (Ultimate Ops), ×2.8
  (Equilibrium), ×3.0 (Ultimate Conquest) and ×2.4–3.4 (Master dungeons)
  better than against other solo clears.
- Searchlight's gates and capture shape the solo data: five Grandmaster
  Conquests qualified solo runs only (their fireteam runs are recorded but
  unqualified), and solo clears had a captured build 5–135× as often as
  fireteam clears (players were chosen to monitor by an evaluator that
  doubled solo scores).
- Some "clears" are not full clears though Bungie flags them fresh,
  completed and solo. Solo Root of Nightmares (Master) clears run a median
  54 minutes, fastest 14, a tenth under the 24-minute Normal solo world
  record, with 931 kills against a fireteam's 2,360; solo King's Fall
  (Master) runs a median 8 minutes. A fifth to two fifths of Master dungeon
  solo clears are faster than the fastest 1% of fireteam clears, some in a
  minute or two. Pantheon and The Desert Perpetual solo clears are
  five-minute boss encounters where kills cannot measure the work.

## Decision

Rankings measure a solo clear against other solo clears of its activity, as
before, **plus the activity's solo premium** (`SoloScoring::ActivityCredit`,
the default; `focalpoint rank builds --solo-scoring alone` ranks without
it):

```text
perf(solo clear) = perf against solo clears + ln(premium of its activity)
```

- **The premium** is the median, over the activity's solo clears that look
  like full clears, of how much better each scores against the fireteam
  clears than against other solo clears (`rank::solo_credit_sql`). It is
  never below ×1.
- **The fireteam clears** are the activity's qualified fireteam clears, or,
  where too few qualified (the solo-gated Grandmaster Conquests), its
  unqualified fireteam clears — cleared, started from the beginning, in the
  same period (`rank::team_reference_sql`). Players present for under 90%
  of the run are left out of both (ADR 0009).
- **A solo clear looks like a full clear** when it started from the
  beginning and, in content with checkpoints, took at least as long as the
  fastest 1% of the activity's fireteam clears. Conquests and Ops cannot be
  resumed or joined in progress (0.01% of their 4.9M recorded runs did not
  start from the beginning, `focalpoint analyze partial`), so every clear of
  theirs is a full clear however fast; dungeons and raids can (22–80%).
  Only full-looking clears set the premium and receive it.
- **Raids get no premium** (activity mode 4: Master raids, Pantheon, The
  Desert Perpetual): the data cannot tell full solo clears of six-player
  content from partial ones.

The premiums are computed once per database connection
(`Database::solo_credits`).

The ranking model's priors were re-chosen by cross-validation on the
credited data: player prior 1 (was 3), group prior 10, context prior 3. With
the premium, player skill alone explains 40.9% of held-out performance,
builds add 6.6 points and activity-specific build effects a further 1.9.

## Consequences

- The all-activities ranking keeps a rank correlation of +0.88–0.92 with
  the uncredited one, but about a third of each class's top 10% changes,
  and nearly all of the new top 10% have solo clears: the builds elite
  players take into fireteam content alone rise (Fortune's Favor +
  Wolfsbane Shadowshot, soloed by 139 players, from 29th to 1st of Hunter
  build types; Hallowfire Heart + Tractor Cannon Hammer of Sol from 13th to
  1st of Titan's).
- Effects grow in scale (soloing is worth ×1.2–3.4); they stay under
  "Ranking details" on the site.
- Because Searchlight captured solo play far more often, the premium
  touches a large share of build observations. That is what Searchlight
  captured; the premium stops treating those clears as ordinary.
- Solo raid clears, real or not, earn nothing extra. If real solo raid
  clears can be told apart later (say, by encounter timing), the raid rule
  can be revisited.
- Partial "clears" — resumed runs — still count as fast clears in the
  scoring itself (solo and fireteam alike, speed bounded at 2×); only the
  premium excludes them. Partial *participants*, fireteam members who
  joined late or left early, are scored by kill rate over time played
  (ADR 0009).
