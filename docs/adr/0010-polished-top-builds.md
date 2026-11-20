# ADR 0010: Top builds use full set bonuses whenever the type has such a build

- **Status:** accepted
- **Date:** 2026-11-20
- **Amends:** the choice of a listing's top build (`crate::site`,
  `docs/site.md`), which highlights also use (ADR 0007)

## Context

A listing's top build is the best-ranked exact build of the best-ranked
loadout of its build type, preferring loadouts used in at least 3 clears
(then 2) that beat their players' usual level. Nothing in that choice looks
at armor set bonuses, and players notice: listing #6 of all activities
(Solar Titan, Hallowfire Heart, Burning Maul) was topped by a build of five
armor pieces from five different sets, no bonus active, stats spread thin.

Armor is always captured whole, so a build without set bonuses really ran
without them. A strong build uses a **4-piece set** (its 2- and 4-piece
bonuses) or **two 2-piece sets**: two active bonuses. With none or a single
2-piece bonus it is a **lesser** build.

What the data shows (`focalpoint analyze sets`, `docs/analysis.md`):

- 22.5% of recorded scored clears ran a lesser build (6.5% no bonus, 16.0%
  one).
- The same player in the same activity does **5% better (± 1.6%)** with
  full set bonuses than with a lesser build (729 player-activities with
  both). One bonus is worth no more than none. The polished clears came 3.1
  days later on average, but the gain does not grow with lateness
  (r = +0.08): it is not learning.
- Lesser builds are not the mark of stronger players. Within bands of
  recorded clears, players who always run lesser builds are as strong as
  those who never do (5–9 clears: +29% and +29%), or weaker (1 clear: −2%
  and +5%, about the build's own cost).
- Lesser top builds were not over-represented — 21.8% of listings with 3+
  players, against 26.3% of ranked exact builds — but 427 of those 469
  types had a polished build that could have been shown, and only 43 of
  those shared the top build's weapons.

## Decision

A build is **polished** with at least two active set bonuses
(`site::POLISHED_SET_BONUSES`). The top build is chosen as before, with
polished builds first: at each evidence floor in turn, the best family's
best loadout that has a polished build, and that loadout's best polished
build. Only a build type with no polished build in the scope gets a lesser
top build, chosen as before. Variants are unchanged: the loadout's other
exact builds near the top build.

Set bonuses are a rule of the choice, not a term of the ranking model:

- **Build types are not adjusted.** A type's effect already counts its
  lesser clears at their (lower) performance; adjusting for set usage
  would raise types whose players skip sets.
- **A prior on exact builds would not reach.** The top build descends
  family → loadout → exact build; penalising lesser exact builds at the
  last step changes the pick only when a polished build shares the
  loadout's weapons (43 of 469).

## Consequences

- Lesser top builds fall from 469 to 51 (2.4%) of the 2,148 listings with
  3+ players, and from 41 to 3 in their top 10%; the 51 are types with no
  polished build in the scope. Listings with 1–2 players change less
  (2,396 to 2,205): most have no polished build.
- Highlights pick listings' top builds, so they follow.
- Rankings and effects are unchanged; only which exact build is shown.
- The evidence behind top builds does not change: 85% rest on one clear
  or less, because most types spread their clears over many exact builds
  (#6: 116 clears over 535 builds, none used in two full clears). Among
  thin candidates the choice is still close to arbitrary; choosing the
  type's most representative polished build is a separate question.
