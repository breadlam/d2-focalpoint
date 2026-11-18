# Analysis database schema (version 1)

The database is a single DuckDB file built by `searchlight import`. The DDL is
[`crates/db/src/schema.sql`](../crates/db/src/schema.sql); this document
explains the model and how to query it. For the source data and its quirks
see [data-sources.md](data-sources.md).

## Conventions

- **Ids:** Bungie 64-bit ids (membership, character, instance, item instance)
  are `BIGINT`. Manifest hashes are `UINTEGER`.
- **Time:** `started_at` is the PGCR `period` (activity start, UTC), as
  `TIMESTAMP`. Run-relative offsets are seconds (`*_s`).
- **Enumerations** are `VARCHAR` (values listed below).
- **Bungie-reported numbers are `INTEGER`** even when normally small: real
  reports contain sentinel or garbage values (e.g. `light_level` of 65535,
  99999, 9999966 in a few hundred rows). Filter them in analysis rather than
  expecting the importer to have guessed what they meant.
- **Keys:** dimension tables have primary keys. Large fact tables do not (see
  ADR 0001); their natural keys are listed here and enforced by the importer's
  validation step.

## Entity overview

```text
            activity ◄── run ──► run_player ──► run_player_weapon
                          │          │  (instance, membership, character)
                          │          │
                          │          └── has_build ─┐
                          ▼                         ▼
                       segment ──► build ──► build_component ──► item
     (instance, membership, idx)    │
                          │         └─ wide summary columns (aspects, exotics, …)
                          ▼
                       loadout ──► loadout_item          (exact, lossless)

 player ◄── character          activity_history (all modes, per player)
```

- A **run** is one activity instance (one PGCR).
- A **build timeline** is one player's segments in one run, keyed by
  `(instance_id, membership_id)`. Only monitored players were sampled, so
  only ~570k of ~60M player-runs have one (`run_player.has_build`).
- A **build** is a distinct gameplay-relevant configuration (ADR 0002); the
  same build used in many runs/segments is stored once.

## Tables

### Runs and performance

**`run`** — one row per activity instance. Natural key `instance_id`.

| Column | Notes |
|---|---|
| `activity_hash` | `directorActivityHash`; join `activity` |
| `difficulty_tier` | PGCR `activityDifficultyTier` (GM ≈ 9–11; legacy raids report −1) |
| `player_count` / `roster_size` | reported `playerCount` vs. entries in the report (they differ for ~460k runs) |
| `is_solo` | `roster_size = 1` (production's definition) |
| `skull_hashes` | selected modifiers; `has_boon`, `equipment_locked`, `challenge_count`, `feat_count` are derived via `skull`. `NULL` when the report lists no skulls |
| `qualified_target` | production's target name when the run met that target's gates (e.g. fresh start, feat floor); `NULL` otherwise |
| `source` | `archive` (parsed from the raw PGCR) or `tracked` (only production's parsed rows survive — mostly pre-Jun-24 runs) |
| `is_tracked` | the run is also in production's `run` table |

**`run_player`** — one row per character per run. Natural key
`(instance_id, membership_id, character_id)`. `started_at` and `activity_hash`
are denormalised from `run` for cheap filtering. `is_clear` = completed with
`completion_reason = 0` (Bungie also marks failed runs `completed`).
`character_level`, `emblem_hash`, `start_s` are `NULL` for `tracked` runs.

**`run_player_weapon`** — kills per weapon (`item_hash`) per character per run.

### Builds

**`segment`** — natural key `(instance_id, membership_id, segment_index)`.
Contiguous stretch of a run with an unchanged loadout: `start_s`,
`duration_s`, the `build_id`, the exact `loadout_id`, and character stats
(`power`, `stat_weapons`, `stat_health`, `stat_class`, `stat_grenade`,
`stat_super`, `stat_melee` — Armor 3.0 totals; `NULL` for early captures).

**`run_build`** — per player-run, per build used: number of segments, first
start, total `duration_s`, `share` of the timeline, and `is_primary` (the
longest-used build). Use this to attribute a run's performance to a build.

**`build`** — one row per distinct build. Summary columns for filtering:
`class`, `element` (`Arc|Solar|Void|Stasis|Strand|Prismatic`),
`subclass_hash`, `super_hash`, `class_ability_hash`, `movement_hash`,
`melee_hash`, `grenade_hash`, `aspect_hashes[]`, `fragment_hashes[]`,
`kinetic_hash`, `energy_hash`, `power_hash`, the five armor slot hashes,
`exotic_armor_hash`, `exotic_weapon_hash`, `artifact_hash`,
`artifact_perk_hashes[]`, `armor_mod_hashes[]` (with repeats), and
`item_hashes[]` (every inventory-item hash in the build, for facet filters).

**`build_component`** — the long form. Natural key
`(build_id, kind, slot, component_hash, parent_hash)`.

| `kind` | `component_hash` | `parent_hash` |
|---|---|---|
| `subclass`, `weapon`, `armor`, `artifact` | the equipped item | — |
| `super`, `class_ability`, `movement`, `melee`, `grenade`, `aspect`, `fragment`, `transcendence` | subclass plug | — |
| `weapon_perk`, `weapon_intrinsic`, `origin_trait`, `weapon_mod` | weapon plug | the weapon |
| `armor_perk`, `armor_archetype`, `armor_mod`, `armor_stat_mod`, `armor_tuning` | armor plug | the armor piece |
| `artifact_perk` | artifact perk item | the artifact |
| `armor_set` | `item_set.set_hash`; `quantity` = pieces equipped | — |
| `set_bonus` | `sandbox_perk.perk_hash` | the set |

`slot` is one of `subclass, kinetic, energy, power, helmet, gauntlets, chest,
legs, class_item, artifact` (`NULL` for `armor_set` / `set_bonus`).
`quantity` counts copies (e.g. two identical armor mods on one piece);
`position` is the socket index, for display order.

**`loadout`**, **`loadout_item`** — the exact captured equipment (all slots
including cosmetics, item instance ids, plugs in socket order, sandbox perks),
deduplicated by `loadout_id`.

### Players

**`player`** — every membership id seen anywhere. Identity (`display_name`,
`global_name`, `global_name_code`, `bungie_name` = `Name#0123`) is the most
recent one seen in a PGCR. `first_seen` / `last_seen` span runs and activity
history. `run_count`, `build_timeline_count`, `is_seeded` (operator watch
list), `is_random_sample` (unbiased baseline cohort).

**`character`** — `character_id → membership_id, class`.

**`activity_history`** — natural key `(membership_id, instance_id)`. The
all-modes activity history (crucible, strikes, raids, …) of discovered
players, from 2026-06-09. Useful for player profiles and behaviour; it has no
weapons or builds.

### Dimensions (from the Bungie manifest)

| Table | Key | Notes |
|---|---|---|
| `item` | `item_hash` | all inventory items incl. plugs: name, type, tier, `is_exotic`, `class`, `slot`, `damage_type` (weapons), `element` (subclasses), `ammo_type`, `plug_category`, `plug_kind` (context-free; `build_component.kind` is authoritative), `item_set_hash`, `icon` |
| `item_set`, `item_set_perk` | `set_hash` | armor sets and their 2/4-piece bonuses |
| `sandbox_perk` | `perk_hash` | perk names/descriptions (set bonuses) |
| `stat` | `stat_hash` | stat names |
| `activity` | `activity_hash` | name, mode, `target_group` (Searchlight's curated slate: "Ultimate Conquest", "Master Raid", …), `run_count` |
| `activity_mode` | `mode` | `DestinyActivityModeType` names |
| `skull` | `skull_hash` | modifiers with `kind` = `boon | feat | challenge | equipment_lock | other` |

### Metadata

- **`meta`** — `schema_version`, `build_fingerprint_version`,
  `loadout_fingerprint_version`, `tool_version`, `imported_at`,
  `source_root`, `source_snapshot`, `manifest_version`, `import_seconds`,
  `skipped_stages`.
- **`import_stat`** — per-stage counters and every validation check's value.

## Macros

`item_name(hash)`, `item_names(list)` (sorted names) and `activity_name(hash)`
resolve hashes in ad-hoc SQL.

## Example queries

```sql
-- Builds using Nothing Manacles with a specific aspect (facet intersection).
SELECT b.build_id, item_names(b.aspect_hashes) AS aspects, item_names(b.fragment_hashes) AS fragments
FROM build b
WHERE b.exotic_armor_hash = (SELECT item_hash FROM item WHERE name = 'Nothing Manacles' AND is_exotic)
  AND list_contains(b.aspect_hashes, (SELECT item_hash FROM item WHERE name = 'Chaos Accelerant' LIMIT 1));

-- The same filter in long form: builds containing ALL of a set of components.
SELECT build_id
FROM build_component
WHERE component_hash IN (300502917, 3291545503)
GROUP BY build_id
HAVING count(DISTINCT component_hash) = 2;

-- Most-used exotic armor per class among clears, weighted by time in use.
SELECT b.class, item_name(b.exotic_armor_hash) AS exotic, round(sum(rb.duration_s) / 3600) AS hours
FROM run_build rb
JOIN build b USING (build_id)
JOIN run_player p USING (instance_id, membership_id)
WHERE p.is_clear
GROUP BY ALL
QUALIFY row_number() OVER (PARTITION BY b.class ORDER BY hours DESC) <= 5
ORDER BY b.class, hours DESC;

-- Weekly usage trend of a weapon across all archived raid/dungeon PGCRs.
SELECT date_trunc('week', p.started_at) AS week, count(*) AS uses, sum(w.kills) AS kills
FROM run_player_weapon w
JOIN run_player p USING (instance_id, membership_id, character_id)
WHERE w.item_hash = (SELECT item_hash FROM item WHERE name = 'Gjallarhorn' LIMIT 1)
GROUP BY 1 ORDER BY 1;
```
