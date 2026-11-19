# ADR 0005: Tiered build distance and armor stat investment

- **Status:** accepted; the budget rule (§3) is provisional — see "Open problem"
- **Date:** 2026-11-18
- **Refines:** [ADR 0004](0004-element-count-vectors.md)

## Context

ADR 0004 compared builds with an unweighted Jaccard distance over every
element, so whatever a build has most of (armor mods, weapon perks, artifact
perks) dominated. The intended weighting is by tier of importance:

1. exotic armor and exotic weapon;
2. super, abilities, aspects, fragments, artifact mods;
3. armor stats, set bonuses, armor mods;
4. other weapons, weapon elements, frames, perks/traits.

Armor stats need interpretation: effects change at 100 and 200, "90-ish
would have been 100", "25 is nearly 0", and a player held back by their armor
collection — or by fragment choices, which add or remove stat points — may
have the same effective build as a player with better armor.

## Decision

### 1. Element kinds carry a tier

| Tier | Element kinds |
|---|---|
| 1 | exotic armor, rolled exotic class item perks, exotic weapon |
| 2 | subclass, super, grenade, melee, class ability, movement, aspects, fragments, artifact perks |
| 3 | **stat investment** (§3), set bonuses, armor mods |
| 4 | legendary weapons, weapon element (damage type), frames, perks/traits, origin traits, weapon mods |
| — | legendary armor pieces, archetypes, stat mods, tuning mods, the artifact item, Transcendence, armor-set membership |

Compared with ADR 0004, set bonuses and frames are now counted (their
*effect* matters, and they give partial credit across different items), and
legendary armor pieces, archetypes, stat and tuning mods are unweighted —
their only effect is on stats, which stat investment captures. Unweighted
kinds stay in the vector so they can be weighted later.

### 2. Tiers get fixed shares of the distance

`distance = 0.4·J₁ + 0.3·J₂ + 0.2·J₃ + 0.1·J₄`, where `Jₜ` is the Jaccard
distance within tier `t`. Per-element weights were rejected: tiers hold very
different numbers of elements (≈2 exotics vs ≈25–30 kit or weapon elements),
so even heavily weighted exotics would be drowned out. With shares, swapping
the exotic armor alone costs 0.4 × J₁, changing the whole ability kit ≈ 0.3,
and entirely different weapons 0.1. The result is a metric in `[0, 1]`.

### 3. Stats become budget-aware investment levels (0–4)

Each of the six stats gets a level from tolerance bands — `< 35` → 0,
`35–74` → 1, `75–124` → 2 (the 100 tier), `125–174` → 3, `≥ 175` → 4 (the
200 tier) — added to tier 3 as a count, so levels 4 vs 3 get partial credit.

A stat that was **held back by budget** is promoted to the threshold it was
reaching for (to 100 if below it; to 200 if past ~115). A stat is held back
when the other stats hold no points that could have been moved into it. A
stat's movable points are its excess above the threshold it reached (100 or
200), or else its armor points above an unavoidable floor; fragment points
are fixed to their stat and never movable, so fragments shift both the stat
and the effective budget.

Parameters come from the data: totals are ≈ 490 (median 488); when the top
stat is ≈ 200 and the second ≥ 100 the third never exceeds 150; in the most
optimised builds the bottom three stats hold ≈ 50 points (floor ≈ 17 each);
and many players reaching 100 spill over to 101–115 (margin 15). Fragment
stat effects come from the manifest's `investmentStats` (e.g. Echo of
Expulsion +10 Super, Facet of Grace −10 Health).

Stats are a property of the build (99.2 % of builds show one stat line
across all their segments); the modal line is used.

## Open problem: what counts as movable

On the full data the rule above flags only 7,667 of 594k builds (1.3 %) as
budget-limited, which contradicts the observation that maxing an effect and
being held back is typical. Attributing each loadout's stat points to their
source (via `investmentStats`) shows why:

| Source | Points per loadout | Redirectable by the player? |
|---|---:|---|
| armor stat rolls | ≈ 352 | only by equipping different pieces |
| masterwork bonuses (conditional) | ≈ 108 | no |
| stat mods (+10 each) | ≈ 45 | yes |
| armor intrinsics | ≈ 16 | no |
| tuning (mostly conditional) | ≈ 6 | yes |
| fragments | ≈ 2 net | yes (but chosen for their effects) |

The rule treats every point above a 17-point floor as movable, but most
points sit where the player's armor pieces put them. A better definition of
movable points — based on stat mods, tuning and the choice of pieces — is
pending; the levels and promotion flags will change when it lands
(`InvestmentParams` and `ELEMENT_VECTOR_VERSION` are recorded in `meta`).

## Consequences

- Schema v4: `build` gains the modal stats, `fragment_stats`, `stat_levels`,
  `stats_promoted`, and the per-tier element lists `tier1_elements` …
  `tier4_elements` (keys `(kind code << 32) | hash`, documented in the new
  `element_kind` table); `element_hashes` is gone. SQL macros
  `build_distance(a, b)` and `element_name(key)` expose the model in SQL.
- Builds are staged and the final `build` table is assembled in one insert
  (`finalize.sql`) after the Rust investment stage: DuckDB rewrites list
  columns on `UPDATE`, which had made that stage take ~5 minutes.
