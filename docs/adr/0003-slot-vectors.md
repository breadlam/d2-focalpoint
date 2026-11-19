# ADR 0003: Slot vectors for popularity, trends and similarity

- **Status:** accepted
- **Date:** 2026-11-18

## Context

A *build* (ADR 0002) is fine-grained: it includes weapon perk rolls, armor
archetypes and every mod, so 82 % of the 594k distinct builds were used in
exactly one run and no build is shared by more than six players. That
identity is right for "did this player reuse this exact setup?", but too fine
for popularity, trends and clustering.

The analysis model is to treat a character build as a **vector of slots** —
super, aspects, fragments, armor pieces, armor mods, weapons, weapon mods,
artifact perks — and compare builds with standard methods such as Hamming
distance.

## Decision

`searchlight_core::vector::SlotVector` projects a build onto:

| Single-item slots (14) | Multi-item groups (6) |
|---|---|
| subclass, super, grenade, melee, class ability, movement | aspects, fragments |
| kinetic, energy, power weapon | weapon mods |
| helmet, gauntlets, chest, legs, class item | armor mods (general, stat and tuning mods) |
| | exotic class item perks |
| | artifact perks |

**Distance** = number of differing slots:

- a single slot contributes 1 when its items differ (or one side is empty);
- a group contributes its **bag distance** `max(|A∖B|, |B∖A|)` over multisets.

The bag distance is Hamming distance under the best alignment of a group's
items. Plain positional Hamming would be wrong for unordered groups: with
aspects `{X, Y}` vs `{Y, Z}`, any fixed ordering (even sorted: `[X, Y]` vs
`[Y, Z]`) reports 2 differences for a single swapped aspect. Both terms are
metrics (bag distance is a known metric on multisets), so the total is a
metric: symmetric, zero only for identical vectors, and it satisfies the
triangle inequality — a property test in `vector.rs` checks all three.

Each distinct vector is fingerprinted (`vector_id`, versioned like build ids)
and stored once in `build_vector` with usage counts; `build.vector_id` links
the fine-grained builds to it. SQL macros `bag_distance` and `slot_distance`
expose the same distance for ad-hoc queries; bulk work (clustering) belongs
in Rust using `SlotVector::distance`.

### Deliberately not slots

- **Weapon perk rolls, armor archetypes and stat rolls, masterworks,
  cosmetics** — they split otherwise-identical builds.
- **Fixed exotic perks** — determined by the exotic item, so they would
  count one change twice. Exotic class items are the exception: their two
  perks are rolled, so they form their own group.
- **Armor set bonuses** — derived from the armor pieces.
- **Transcendence** — fixed per Prismatic subclass.
- **The artifact item itself** — its active perks carry the information.
- **Character stats and power level** — power is set relatively in the
  content Searchlight tracked and is not part of a build.

## Consequences

- **Exact vectors rarely repeat, by nature.** On the real data, 594,233
  builds map to 582,508 vectors. Even coarse projections stay large
  (subclass configuration + exotic armor + three weapons: 471k distinct;
  subclass configuration alone: 123k), because players' exact combinations
  seldom recur. Aggregate by *distance* (neighbourhoods, clusters) or by a
  projection onto chosen slots, not by exact `vector_id`.
- **Mods dominate the slot count.** An average vector occupies 59.5 slots,
  of which ~24 are armor mods and ~13 artifact perks. Among well-used vectors
  (≥ 3 player-runs, sample of 40) the nearest neighbour is median 2.5 slots
  away, and those differences are mostly armor mods (3.9 on average) ahead of
  artifact perks (1.2), weapons (1.0) and subclass choices (0.5). Analyses
  that care about "the same build" in a player's sense will likely want
  per-group weights or group normalisation on top of the unweighted metric.
- Every slot weighs 1. Weighted variants (e.g. exotic armor counting more
  than one fragment) can be layered on later without changing storage.
- Changing the slot set or encoding changes `vector_id`s: bump
  `VECTOR_FINGERPRINT_VERSION` and re-import.
