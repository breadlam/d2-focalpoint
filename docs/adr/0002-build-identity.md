# ADR 0002: Build identity and plug classification

- **Status:** accepted
- **Date:** 2026-11-18

## Context

Production stored each build-timeline segment as a full `LoadoutState` JSON
document: every equipped item (including ghost, ship, emblem, emotes), the
specific item instance ids, every socketed plug in socket order (including
shaders, ornaments, kill trackers, masterworks), artifact state and
character stats. There was no notion of "the same build" across segments,
runs or players.

The CLI needs to count, compare, rank and cluster *builds*, so it needs an
identity that ignores everything that does not change how a character plays.

## Decision

A **build** is the set of gameplay-relevant components extracted from a
loadout (`focalpoint_core::build::extract_build`):

| Included | Excluded |
|---|---|
| subclass item; super, class ability, movement, melee, grenade, aspects, fragments, Transcendence | cosmetic slots: ghost, sparrow, ship, emblem, emotes, finisher, clan banner |
| weapons; their perks, intrinsics, origin traits, weapon mods | shaders, ornaments, kill trackers, mementos, crafting plugs |
| armor; armor perks (exotic perks, exotic class item spirits), archetypes, armor mods, stat mods, tuning mods | masterwork / enhancement / upgrade plugs, empty sockets |
| armor sets (piece counts) and active set bonuses | armor stat rolls (`armor_stats` plugs) |
| artifact and its active perks | item instance ids; character stat totals |

Each component is `(kind, slot, hash, parent_hash, quantity)`; e.g. a weapon
perk's parent is its weapon, so the same perk on two weapons is two
components, and two copies of an armor mod on one piece is quantity 2.
Components are sorted and fingerprinted with xxh3-64 under a domain tag and
a version byte; the top bit is dropped so the id is a positive `BIGINT`:
`build_id`.

The exact loadout is fingerprinted too (`loadout_id`, all items with instance
ids and plugs in socket order, plus artifact state; stats excluded) and stored
losslessly in `loadout` / `loadout_item`.

### Versioning

`BUILD_FINGERPRINT_VERSION` / `LOADOUT_FINGERPRINT_VERSION` are recorded in the
database's `meta` table. Any change to what goes into a fingerprint **must**
bump the version (a golden-value unit test guards the encoding). The importer
verifies there are no collisions with a second, independent fingerprint and
aborts if one is found.

### Plug classification fixes

Classification is by manifest `plugCategoryIdentifier` plus the host slot.
Production's heuristic was kept but corrected where the collected data showed
it was wrong:

- **Prismatic Transcendence** (`*.transcendence`, and the class's fixed
  Transcendence grenade `*.prism_grenade`) is its own kind; production left it
  as `other`. It is *not* the chosen grenade (a real Prismatic loadout has
  both Healing Grenade and Freezing Singularity socketed).
- **Weapon mods** (`v400.weapon.mod_*`) were `other`; now `weapon_mod`.
- **Armor intrinsics** (exotic perks, exotic class item "Spirit of …" rolls)
  were labelled weapon intrinsics; now `armor_perk`.
- **Armor stat rolls**, **artifact perks** and **crafting plugs** have explicit
  kinds instead of falling into `other`.
- Subclass rules only apply to subclass plugs, so no gear category can be
  mistaken for an ability.

## Consequences

- Facet queries are exact set-membership queries over `build_component`
  (or the list columns on `build`).
- A "build" is fine-grained (it includes weapon rolls and armor mods). Coarser
  notions — e.g. *archetype* = subclass + aspects + exotic armor + exotic
  weapon — are `GROUP BY`s over `build` columns, not stored identities.
- Changing classification rules changes `build_id`s; bump the version and
  re-import.
