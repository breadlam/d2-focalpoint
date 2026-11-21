# Analysis database schema (version 8)

The database is a single DuckDB file built by `focalpoint import`. The DDL is
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
                          │         ├─ wide summary columns (aspects, exotics, …)
                          │         └─ tier1..4_elements (sparse element count vector, ADR 0005)
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
| `difficulty_tier` | PGCR `activityDifficultyTier`: the step of the activity's difficulty ladder (`difficulty_tier` table; legacy raids report −1) |
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

**`run_score`** — how well each qualified clear went (ADR 0006). One row per
player and run that met a Searchlight target's gates, was cleared, and had
kills, in a scored activity (entries of a player who switched characters are
summed). `kill_index`, `speed` and `survival` measure the run against a
typical clear of the same activity by the same kind of player — solo,
fireteam member, or partial participant (`is_partial`: a fireteam member
whose `played_s` is under 90% of the run's `duration_s`, measured by kill
rate over time played with `speed` 1; ADR 0009) — each bounded; `score` is
their product (1.0 = typical) and `perf` its centred logarithm, the response
of the ranking model. `has_build` marks runs with a captured build.

**`activity_reference`** — the typical clear per `(activity_hash, is_solo,
is_partial)`: median `kills`, `duration_s` (time played for partial
participants), `deaths` over its clears. `is_borrowed` buckets
(fewer than 50 clears) use the activity-wide reference; activities whose
median clear has fewer than 20 kills are `NOT is_scored` (Pantheon boss
encounters). `log_score_mean` is the centre of `perf`.

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

The build's **element count vector** (ADRs 0004–0005) is split by tier into
`tier1_elements[]` (exotics), `tier2_elements[]` (subclass kit, artifact
perks), `tier3_elements[]` (stat investment, set bonuses, armor mods) and
`tier4_elements[]` (weapons, elements, frames, perks, weapon mods). Each is a
sorted multiset of element keys, `(kind code << 32) | hash` (kinds and tiers
in `element_kind`; `element_name(key)` gives a readable name), with a key
repeated once per copy and placement ignored. Compare builds with
`build_distance(a, b)`.

Stats: `stat_weapons` … `stat_melee` are the build's (modal) Armor 3.0 stats;
`fragment_stats[]` the fragments' fixed share of each; `stat_levels[]` the
budget-aware investment level 0–4 per stat and `stats_promoted[]` the stats
read as held back by budget (list order: Weapons, Health, Class, Grenade,
Super, Melee). Usage: `timeline_count` (player-runs), `player_count`,
`seconds_used`.

**`element_kind`** — `code`, `name`, `tier` (1–4, NULL = unweighted) of the
element kinds used in the tier lists.

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
| `stat` | `stat_hash` | stat names and icons |
| `damage_type` | `damage_type_hash` | element names and glyphs (`item.damage_type` holds the name) |
| `activity` | `activity_hash` | name, mode, `difficulty_collection_hash` (its difficulty ladder), `target_group` (Searchlight's curated slate: "Ultimate Conquest", "Master Raid", …), `run_count` |
| `difficulty_tier` | `(collection_hash, tier)` | the steps of each difficulty ladder: `name` (Normal … Grandmaster, Ultimate), `activity_level` (rises with difficulty: Portal Normal 31 … Ultimate 50; Conquests' Grandmaster 44–55, Ultimate 58–60), `tier_rank`, `min_power`, `level_offset`. A run's step: `JOIN difficulty_tier t ON (t.collection_hash, t.tier) = (activity.difficulty_collection_hash, run.difficulty_tier)` |
| `activity_mode` | `mode` | `DestinyActivityModeType` names |
| `skull` | `skull_hash` | modifiers with `kind` = `boon | feat | challenge | equipment_lock | other` |

### Community mechanics (a layer)

`mechanic_source`, `mechanic_entry`, `mechanic_subject` and
`mechanic_keyword` hold the community's documentation of game mechanics,
keyed to manifest hashes, and `mechanic_tag` (with the view
`mechanic_hash_tag`) what it says each part needs and does
([effects.md](effects.md)); `mechanic_rotation`, `mechanic_rotation_loadout`,
`mechanic_loadout` and `mechanic_loadout_item` measured boss-damage
rotations and the DIM loadouts behind them; `context`, `context_activity`,
`context_feature` and `context_event` the profiles of the contexts builds
are evaluated in ([contexts.md](contexts.md)); `enemy_health`,
`enemy_measured`, `power_delta` and `champion_stat` how much damage enemies
take to kill, with the view `context_enemy_health` per context; and
`difficulty` and `difficulty_modifier` the Power cap and combatant Power by
context, step and Feat, with the view `run_difficulty` per run
([durability.md](durability.md)). They are not written by the import: `focalpoint
mechanics import` adds and replaces them in an existing database
(`meta.mechanics_version`). See [mechanics.md](mechanics.md).

### Metadata

- **`meta`** — `schema_version`, `build_fingerprint_version`,
  `loadout_fingerprint_version`, `tool_version`, `imported_at`,
  `element_vector_version`, `investment_params`, `score_params`,
  `source_root`, `source_snapshot`, `manifest_version`, `import_seconds`,
  `skipped_stages`; with the mechanics layer, `mechanics_version`,
  `mechanics_imported_at`, `mechanics_sources`.
- **`import_stat`** — per-stage counters and every validation check's value.

## Macros

`item_name(hash)`, `item_names(list)` (sorted names) and `activity_name(hash)`
resolve hashes in ad-hoc SQL.

`build_distance(a, b)` takes two `build` rows (pass the table aliases) and
returns the tiered distance `0.4·J₁ + 0.3·J₂ + 0.2·J₃ + 0.1·J₄` (ADR 0005).
Its building blocks work on any two multisets (lists, a value repeated once
per copy): `jaccard_distance(a, b)` (`1 − Σmin/Σmax`), `l1_distance(a, b)`
(added plus removed), `bag_distance(a, b)` (slots changed) and
`multiset_common(a, b)` (`Σmin`). `element_name(key)` names an element key.
For nearest-neighbour searches use `focalpoint similar <build_id>`, which
computes the same distance in Rust in parallel.

## Example queries

For facet searches (class, exotics, aspects, perks, mods, activity, …) and
popularity counts, `focalpoint search` builds these queries for you; it
matches each name against every item carrying it.

> **Item names are not unique.** Reissued or versioned items share a name
> across several hashes (e.g. *Nothing Manacles* has more than one), so match
> names with `IN (SELECT item_hash FROM item WHERE name = ...)`, never `=`.

```sql
-- Builds using Nothing Manacles with a specific aspect (facet intersection).
SELECT b.build_id, item_names(b.aspect_hashes) AS aspects, item_names(b.fragment_hashes) AS fragments
FROM build b
WHERE b.exotic_armor_hash IN (SELECT item_hash FROM item WHERE name = 'Nothing Manacles')
  AND list_has_any(b.aspect_hashes, (SELECT list(item_hash) FROM item WHERE name = 'Chaos Accelerant'))
LIMIT 10;

-- The same filter in long form: builds containing ALL of a set of components
-- (here: any version of each named component).
SELECT c.build_id
FROM build_component c
JOIN item i ON i.item_hash = c.component_hash
WHERE i.name IN ('Nothing Manacles', 'Chaos Accelerant')
GROUP BY c.build_id
HAVING count(DISTINCT i.name) = 2;

-- Most-used exotic armor per class among clears, weighted by time in use.
SELECT b.class, item_name(b.exotic_armor_hash) AS exotic, round(sum(rb.duration_s) / 3600) AS hours
FROM run_build rb
JOIN build b USING (build_id)
JOIN run_player p USING (instance_id, membership_id)
WHERE p.is_clear
GROUP BY 1, 2
QUALIFY row_number() OVER (PARTITION BY b.class ORDER BY hours DESC) <= 5
ORDER BY b.class, hours DESC;

-- Weekly usage trend of a weapon across all archived raid/dungeon PGCRs.
SELECT date_trunc('week', p.started_at)::DATE AS week, count(*) AS uses, sum(w.kills) AS kills
FROM run_player_weapon w
JOIN run_player p USING (instance_id, membership_id, character_id)
WHERE w.item_hash IN (SELECT item_hash FROM item WHERE name = 'Gjallarhorn')
GROUP BY 1 ORDER BY 1;

-- A build's nearest neighbours by the tiered distance (or use `focalpoint similar`).
WITH target AS (SELECT * FROM build WHERE build_id = 8953961516047688000)
SELECT b.build_id, round(build_distance(target, b), 3) AS distance,
       item_name(b.exotic_armor_hash) AS exotic, b.timeline_count
FROM target, build b
WHERE b.class = target.class AND b.build_id <> target.build_id
ORDER BY distance
LIMIT 10;

-- A build's tier-3 elements (stat investment, set bonuses, armor mods) by name.
SELECT ek.name AS kind, element_name(k) AS element, count(*) AS copies
FROM (SELECT unnest(tier3_elements) AS k FROM build WHERE build_id = 8953961516047688000)
JOIN element_kind ek ON ek.code = k >> 32
GROUP BY ALL ORDER BY kind, element;
```
