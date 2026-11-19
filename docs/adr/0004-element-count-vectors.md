# ADR 0004: Builds as sparse element count vectors

- **Status:** accepted
- **Date:** 2026-11-18
- **Supersedes:** [ADR 0003](0003-slot-vectors.md)

## Context

ADR 0003 described a build as fixed slots plus unordered slot groups. Two
problems surfaced:

1. **Order.** The truest form of "a build as a vector" is one coordinate per
   possible game element holding how many of it the build has. Slot groups
   approximated that, and their per-group distance differed from the count
   vector's distance whenever group sizes differed.
2. **Weapon perks were missing.** Traits such as Chain Reaction or Hatchling
   are part of what a build is, but ADR 0003 left perk rolls out because they
   "over-split" builds — a concern for exact-match identity, not for
   distance-based analysis.

The full count vector is also very sparse (7,944 elements seen across all
builds, ~82 non-zero per build, ≈1 % dense) and a fixed vocabulary would be
awkward when the game adds items.

## Decision

A build is a **sparse element count vector** (`searchlight_core::vector::
ElementVector`): only non-zero coordinates are stored, as sorted
`(kind, item hash, count)` entries. A missing hash means a count of zero.

- **Keys are item hashes.** Bungie's hashes are a stable, open-ended
  coordinate system: new content needs no vocabulary or migration.
- **Placement does not matter.** An element counts the same wherever it is
  socketed: fragments are unordered, and a perk or mod on any weapon is the
  same element (two copies = count 2). Every hash belongs to exactly one
  kind (verified: 0 of 7,944 hashes appear under two kinds), so the kind is
  carried only to allow per-kind weights and slot-edit counting.
- **Which elements count:** every build component that is a player choice —
  subclass, super, abilities, aspects, fragments; weapons, weapon perks,
  origin traits, weapon mods; armor pieces, archetypes, armor / stat / tuning
  mods, rolled exotic class item perks; the artifact and its perks.
  Excluded are components *fully determined by another one*, so no choice
  counts twice: weapon frame intrinsics (by the weapon), fixed exotic armor
  perks (by the exotic), Transcendence (by the subclass), armor sets and set
  bonuses (by the armor pieces). Cosmetics, masterworks and stat rolls were
  never build components (ADR 0002). Power level is not part of a build.
- **Distances,** all computed exactly by merging two sorted lists and all
  equal to the same quantities on the dense vector:

  | Distance | Definition | One swapped aspect |
  |---|---|---|
  | weighted Jaccard (default) | `1 − Σ wᵢ·min(aᵢ, bᵢ) / Σ wᵢ·max(aᵢ, bᵢ)` | 2/(n+1) for n-element builds |
  | L1 | `Σ wᵢ·|aᵢ − bᵢ|` | 2 |
  | slot edits | per kind, `max(|A∖B|, |B∖A|)` | 1 |

  **Weighted Jaccard is the default** for similarity and clustering: it is
  bounded in `[0, 1]`, so builds with different numbers of filled elements
  (older captures lack artifact data; some builds run fewer mods) stay
  comparable, it is a metric, and it is the standard choice for sparse count
  data. Weights default to 1 per element and can be set per kind (e.g. to
  stop armor mods dominating). Slot edits are for human-readable output
  ("these builds differ in 3 slots"); L1 is the raw count difference.
- **Storage:** the vector is a function of the build, so it is stored on
  `build` as `element_hashes` (a sorted multiset; repetition = count) rather
  than in a separate table. It is almost one-to-one: on the real data
  594,233 builds have 594,173 distinct vectors. The 60 builds that share a
  vector differ only in *where* a stat or tuning mod is socketed (placement
  is deliberately ignored) or in a weapon frame's enhancement tier (the
  frame *type* is determined by the weapon; its tier is a roll property,
  like a masterwork). `build_component` is its long form. The SQL macros
  `jaccard_distance`, `l1_distance`, `bag_distance` and `multiset_common`
  compute the unweighted distances for ad-hoc queries; bulk and weighted
  work uses the Rust implementation.

## Consequences

- **Checkpoint bots inflate raw popularity.** The three most-used builds
  belong to single accounts that sit in Master raids/dungeons holding
  checkpoints (1,100–1,700 runs each, zero kills, zero clears); players who
  never kill and never clear account for 5 % of build timelines. Popularity
  and "good build" analyses should exclude zero-kill player-runs (see
  data-sources.md).
- Schema v3 drops ADR 0003's `build_vector` table, `build.vector_id` and
  `slot_distance`; `build` gains `element_hashes`, `element_count` and usage
  counts (`timeline_count`, `player_count`, `seconds_used`).
- Tests check the sparse distances against a brute-force dense count-vector
  implementation (including slot edits by exhaustive alignment) and check the
  metric axioms.
- Exact all-pairs comparison within a class and element is feasible on a
  large machine (≈1.5·10¹⁰ sparse merges); approximate neighbour search is
  not needed yet.
- Changing the selection rules changes `element_hashes`: bump
  `ELEMENT_VECTOR_VERSION` (recorded in `meta`) and re-import.
