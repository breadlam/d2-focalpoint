-- searchlight-cli analysis database, schema version 4.
--
-- Conventions
--   * Bungie 64-bit ids (membership, character, instance, item instance) are
--     BIGINT; every observed value is below 2^63.
--   * Manifest hashes are UINTEGER (they are u32 in the Bungie API).
--   * Times are TIMESTAMP in UTC. `started_at` is the PGCR `period`.
--   * Numeric values reported by Bungie are INTEGER even when small: real
--     reports contain sentinel/garbage values (e.g. light_level 65535).
--   * Enumerations are VARCHAR (DuckDB dictionary-compresses them); allowed
--     values are documented in docs/schema.md.
--   * Large fact tables carry no PRIMARY KEY (DuckDB's ART indexes are costly
--     at this scale); their natural keys are documented and enforced by the
--     importer's validation step instead.

-- ---------------------------------------------------------------------------
-- Metadata
-- ---------------------------------------------------------------------------

CREATE TABLE meta (
    key   VARCHAR PRIMARY KEY,
    value VARCHAR NOT NULL
);

-- Counters recorded by the importer (rows read/kept/skipped per stage, etc.).
CREATE TABLE import_stat (
    stage  VARCHAR NOT NULL,
    metric VARCHAR NOT NULL,
    value  BIGINT  NOT NULL,
    PRIMARY KEY (stage, metric)
);

-- ---------------------------------------------------------------------------
-- Manifest dimensions
-- ---------------------------------------------------------------------------

-- Every DestinyInventoryItemDefinition entry: gear, subclasses, and plugs.
CREATE TABLE item (
    item_hash      UINTEGER PRIMARY KEY,
    name           VARCHAR  NOT NULL,
    description    VARCHAR,
    item_type      INTEGER,            -- DestinyItemType
    item_sub_type  INTEGER,            -- DestinyItemSubType
    item_type_name VARCHAR,            -- "Hand Cannon", "Gauntlets", "Warlock Aspect", ...
    tier_type      UTINYINT,
    tier_name      VARCHAR,            -- "Exotic", "Legendary", ...
    is_exotic      BOOLEAN  NOT NULL,
    class          VARCHAR,            -- Titan | Hunter | Warlock; NULL = any class
    bucket_hash    UINTEGER,
    slot           VARCHAR,            -- see docs/schema.md (kinetic, helmet, subclass, ...)
    damage_type    VARCHAR,            -- weapons: Kinetic | Arc | Solar | Void | Stasis | Strand
    element        VARCHAR,            -- subclasses: Arc | Solar | Void | Stasis | Strand | Prismatic
    ammo_type      UTINYINT,           -- 1 primary, 2 special, 3 heavy
    breaker_type   UTINYINT,
    plug_category  VARCHAR,            -- plug.plugCategoryIdentifier
    plug_kind      VARCHAR,            -- classification when socketed (aspect, weapon_perk, ...)
    item_set_hash  UINTEGER,           -- armor set membership
    icon           VARCHAR             -- Bungie CDN path
);

CREATE TABLE item_set (
    set_hash UINTEGER PRIMARY KEY,
    name     VARCHAR NOT NULL
);

CREATE TABLE item_set_perk (
    set_hash        UINTEGER NOT NULL,
    required_pieces UTINYINT NOT NULL,
    perk_hash       UINTEGER NOT NULL,  -- sandbox_perk.perk_hash
    PRIMARY KEY (set_hash, required_pieces)
);

CREATE TABLE sandbox_perk (
    perk_hash   UINTEGER PRIMARY KEY,
    name        VARCHAR NOT NULL,
    description VARCHAR,
    icon        VARCHAR
);

CREATE TABLE stat (
    stat_hash   UINTEGER PRIMARY KEY,
    name        VARCHAR NOT NULL,
    description VARCHAR
);

CREATE TABLE activity_mode (
    mode          INTEGER PRIMARY KEY,  -- DestinyActivityModeType
    name          VARCHAR NOT NULL,
    friendly_name VARCHAR
);

-- One row per director activity hash seen in the data (named from the
-- manifest where possible), annotated with Searchlight's curated target slate.
CREATE TABLE activity (
    activity_hash      UINTEGER PRIMARY KEY,
    name               VARCHAR,
    description        VARCHAR,
    mode               INTEGER,
    modes              INTEGER[],
    activity_type_hash UINTEGER,
    is_playlist        BOOLEAN,
    light_level        INTEGER,
    target_group       VARCHAR,   -- e.g. "Ultimate Conquest"; NULL if never targeted
    run_count          BIGINT NOT NULL DEFAULT 0
);

-- Difficulty modifiers ("skulls") with Searchlight's classification.
CREATE TABLE skull (
    skull_hash  UINTEGER PRIMARY KEY,
    name        VARCHAR NOT NULL,
    kind        VARCHAR NOT NULL,   -- boon | feat | challenge | equipment_lock | other
    slot        VARCHAR,
    connotation INTEGER,
    description VARCHAR
);

-- ---------------------------------------------------------------------------
-- Players
-- ---------------------------------------------------------------------------

CREATE TABLE player (
    membership_id         BIGINT PRIMARY KEY,
    membership_type       SMALLINT,
    display_name          VARCHAR,
    global_name           VARCHAR,
    global_name_code      USMALLINT,
    bungie_name           VARCHAR,     -- "Name#0123" when the global name is known
    first_seen            TIMESTAMP,
    last_seen             TIMESTAMP,
    run_count             BIGINT  NOT NULL DEFAULT 0,  -- rows in run_player
    build_timeline_count  BIGINT  NOT NULL DEFAULT 0,  -- runs with a captured build
    is_seeded             BOOLEAN NOT NULL DEFAULT false, -- operator-curated watch list
    is_random_sample      BOOLEAN NOT NULL DEFAULT false  -- unbiased baseline cohort
);

CREATE TABLE "character" (
    character_id  BIGINT PRIMARY KEY,
    membership_id BIGINT NOT NULL,
    class         VARCHAR
);

-- ---------------------------------------------------------------------------
-- Runs (activity instances) and per-player performance
-- ---------------------------------------------------------------------------

-- Natural key: instance_id.
CREATE TABLE run (
    instance_id            BIGINT    NOT NULL,
    started_at             TIMESTAMP NOT NULL,
    activity_hash          UINTEGER  NOT NULL,  -- directorActivityHash
    reference_hash         UINTEGER,            -- referenceId
    mode                   INTEGER,
    modes                  INTEGER[],
    difficulty_tier        INTEGER ,
    duration_s             INTEGER,
    player_count           INTEGER ,            -- PGCR playerCount (as reported)
    roster_size            INTEGER ,            -- entries in the report
    is_solo                BOOLEAN,             -- roster_size = 1
    started_from_beginning BOOLEAN,
    is_private             BOOLEAN,
    starting_phase_index   INTEGER ,
    skull_hashes           UINTEGER[],
    has_boon               BOOLEAN,
    equipment_locked       BOOLEAN,
    challenge_count        INTEGER ,
    feat_count             INTEGER ,
    qualified_target       VARCHAR,   -- production target_name when the run met that target's gates
    source                 VARCHAR   NOT NULL,  -- archive | tracked (see docs/schema.md)
    is_tracked             BOOLEAN   NOT NULL   -- also present in production's run table
);

-- One row per character per run. Natural key: (instance_id, membership_id, character_id).
CREATE TABLE run_player (
    instance_id       BIGINT    NOT NULL,
    membership_id     BIGINT    NOT NULL,
    character_id      BIGINT    NOT NULL,
    started_at        TIMESTAMP NOT NULL,  -- denormalized from run
    activity_hash     UINTEGER  NOT NULL,  -- denormalized from run
    class             VARCHAR,
    light_level       INTEGER ,
    character_level   INTEGER ,
    emblem_hash       UINTEGER,
    kills             INTEGER NOT NULL,
    deaths            INTEGER NOT NULL,
    assists           INTEGER NOT NULL,
    precision_kills   INTEGER NOT NULL,
    super_kills       INTEGER NOT NULL,
    melee_kills       INTEGER NOT NULL,
    grenade_kills     INTEGER NOT NULL,
    ability_kills     INTEGER NOT NULL,
    completed         BOOLEAN NOT NULL,
    completion_reason INTEGER ,
    is_clear          BOOLEAN NOT NULL,   -- completed AND completion_reason = 0
    time_played_s     INTEGER,
    start_s           INTEGER,
    has_build         BOOLEAN NOT NULL DEFAULT false
);

-- Kills per weapon per character per run.
CREATE TABLE run_player_weapon (
    instance_id     BIGINT   NOT NULL,
    membership_id   BIGINT   NOT NULL,
    character_id    BIGINT   NOT NULL,
    item_hash       UINTEGER NOT NULL,
    kills           INTEGER  NOT NULL,
    precision_kills INTEGER  NOT NULL
);

-- ---------------------------------------------------------------------------
-- Builds
-- ---------------------------------------------------------------------------

-- A distinct gameplay-relevant build (see docs/adr/0002-build-identity.md).
-- The wide columns summarise build_component for convenient filtering.
CREATE TABLE build (
    build_id             BIGINT PRIMARY KEY,
    class                VARCHAR,
    element              VARCHAR,
    subclass_hash        UINTEGER,
    super_hash           UINTEGER,
    class_ability_hash   UINTEGER,
    movement_hash        UINTEGER,
    melee_hash           UINTEGER,
    grenade_hash         UINTEGER,
    aspect_hashes        UINTEGER[],
    fragment_hashes      UINTEGER[],
    kinetic_hash         UINTEGER,
    energy_hash          UINTEGER,
    power_hash           UINTEGER,
    helmet_hash          UINTEGER,
    gauntlets_hash       UINTEGER,
    chest_hash           UINTEGER,
    legs_hash            UINTEGER,
    class_item_hash      UINTEGER,
    exotic_armor_hash    UINTEGER,
    exotic_weapon_hash   UINTEGER,
    artifact_hash        UINTEGER,
    artifact_perk_hashes UINTEGER[],
    armor_mod_hashes     UINTEGER[],   -- general armor mods, with repeats
    item_hashes          UINTEGER[],   -- every inventory-item hash in the build (facet filters)
    -- Armor stats (modal across the build's segments; Armor 3.0 order
    -- Weapons, Health, Class, Grenade, Super, Melee in the list columns).
    stat_weapons         INTEGER,
    stat_health          INTEGER,
    stat_class           INTEGER,
    stat_grenade         INTEGER,
    stat_super           INTEGER,
    stat_melee           INTEGER,
    fragment_stats       INTEGER[],     -- the fragments' fixed share of each stat
    stat_levels          UTINYINT[],    -- budget-aware investment level 0-4 per stat (ADR 0005)
    stats_promoted       VARCHAR[],     -- stats read as held back by budget
    -- The build's sparse element count vector (ADR 0004/0005), split by tier:
    -- element keys ((kind code << 32) | hash, see element_kind), a key
    -- repeated once per copy. Compare builds with build_distance(a, b).
    tier1_elements       BIGINT[] NOT NULL,   -- exotics
    tier2_elements       BIGINT[] NOT NULL,   -- subclass kit, artifact perks
    tier3_elements       BIGINT[] NOT NULL,   -- stat investment, set bonuses, armor mods
    tier4_elements       BIGINT[] NOT NULL,   -- weapons, elements, frames, perks, weapon mods
    component_count      SMALLINT NOT NULL,
    unresolved_count     SMALLINT NOT NULL,   -- plug hashes missing from the manifest
    timeline_count       INTEGER  NOT NULL DEFAULT 0,   -- player-runs that used the build
    player_count         INTEGER  NOT NULL DEFAULT 0,
    seconds_used         BIGINT   NOT NULL DEFAULT 0    -- total time equipped across runs
);

-- Element kinds of the build vectors: key = (code << 32) | hash.
CREATE TABLE element_kind (
    code UTINYINT PRIMARY KEY,
    name VARCHAR  NOT NULL,
    tier UTINYINT            -- 1-4; NULL = kept in vectors but unweighted
);

-- Long form of a build, one row per component. Natural key:
-- (build_id, kind, slot, component_hash, parent_hash).
CREATE TABLE build_component (
    build_id       BIGINT   NOT NULL,
    kind           VARCHAR  NOT NULL,   -- see docs/schema.md
    slot           VARCHAR,
    component_hash UINTEGER NOT NULL,   -- item hash; set hash for armor_set; sandbox perk for set_bonus
    parent_hash    UINTEGER,            -- item a plug is socketed into; set of a set_bonus
    quantity       SMALLINT NOT NULL,   -- copies (pieces equipped for armor_set)
    position       SMALLINT NOT NULL    -- socket index of the first copy (display order)
);

-- An exact captured loadout (lossless: every item instance and plug).
CREATE TABLE loadout (
    loadout_id            BIGINT PRIMARY KEY,
    build_id              BIGINT NOT NULL,
    artifact_def_hash     UINTEGER,       -- DestinyArtifactDefinition hash (progressions)
    artifact_active_perks UINTEGER[]
);

-- Natural key: (loadout_id, bucket_hash, item_hash, item_instance_id).
CREATE TABLE loadout_item (
    loadout_id       BIGINT   NOT NULL,
    slot             VARCHAR  NOT NULL,
    bucket_hash      UINTEGER NOT NULL,
    item_hash        UINTEGER NOT NULL,
    item_instance_id BIGINT   NOT NULL,
    plug_hashes      UINTEGER[] NOT NULL,  -- socket order
    perk_hashes      UINTEGER[] NOT NULL
);

-- The build timeline: one row per contiguous stretch of a run during which a
-- player's loadout was unchanged. Natural key: (instance_id, membership_id, segment_index).
CREATE TABLE segment (
    instance_id   BIGINT   NOT NULL,
    membership_id BIGINT   NOT NULL,
    segment_index SMALLINT NOT NULL,
    start_s       INTEGER  NOT NULL,   -- seconds from run start
    duration_s    INTEGER  NOT NULL,
    build_id      BIGINT   NOT NULL,
    loadout_id    BIGINT   NOT NULL,
    power         INTEGER ,
    stat_weapons  INTEGER ,
    stat_health   INTEGER ,
    stat_class    INTEGER ,
    stat_grenade  INTEGER ,
    stat_super    INTEGER ,
    stat_melee    INTEGER 
);

-- Builds used per player-run, aggregated over the timeline. Natural key:
-- (instance_id, membership_id, build_id).
CREATE TABLE run_build (
    instance_id   BIGINT   NOT NULL,
    membership_id BIGINT   NOT NULL,
    build_id      BIGINT   NOT NULL,
    segments      SMALLINT NOT NULL,
    first_start_s INTEGER  NOT NULL,
    duration_s    INTEGER  NOT NULL,
    share         DOUBLE,              -- duration_s / timeline duration (NULL if 0)
    is_primary    BOOLEAN  NOT NULL    -- longest-used build of the timeline
);

-- ---------------------------------------------------------------------------
-- Activity history (all modes; the behaviour-model corpus)
-- ---------------------------------------------------------------------------

-- Natural key: (membership_id, instance_id).
CREATE TABLE activity_history (
    membership_id     BIGINT    NOT NULL,
    character_id      BIGINT    NOT NULL,
    instance_id       BIGINT    NOT NULL,
    started_at        TIMESTAMP NOT NULL,
    activity_hash     UINTEGER  NOT NULL,
    mode              INTEGER,
    kills             INTEGER,
    deaths            INTEGER,
    assists           INTEGER,
    duration_s        INTEGER,
    completion_reason INTEGER ,
    completed         BOOLEAN,
    player_count      INTEGER 
);

-- ---------------------------------------------------------------------------
-- Convenience macros for ad-hoc SQL
-- ---------------------------------------------------------------------------

-- item_name(4282591831) -> 'Prismatic Hunter'
CREATE MACRO item_name(h) AS (SELECT name FROM item WHERE item_hash = h);

-- item_names(aspect_hashes) -> ['Feed the Void', 'Lightning Surge'] (sorted)
CREATE MACRO item_names(hs) AS (
    SELECT list(i.name ORDER BY i.name)
    FROM (SELECT unnest(hs) AS h) u
    JOIN item i ON i.item_hash = u.h
);

-- activity_name(2727361621) -> 'Equilibrium: Standard'
CREATE MACRO activity_name(h) AS (SELECT name FROM activity WHERE activity_hash = h);

-- Distances between element multisets (sorted lists, a value repeated once
-- per copy). They equal the same distances on the dense count vector.

-- Σ min(a_i, b_i): the size of the multiset intersection.
CREATE MACRO multiset_common(a, b) AS (
    coalesce(list_sum(list_transform(list_distinct(coalesce(a, [])),
        lambda x: least(len(list_filter(coalesce(a, []), lambda y: y = x)),
                        len(list_filter(coalesce(b, []), lambda y: y = x))))), 0)
);

-- Jaccard (Ruzicka) distance 1 - Σmin/Σmax, in [0, 1].
CREATE MACRO jaccard_distance(a, b) AS (
    CASE WHEN len(coalesce(a, [])) + len(coalesce(b, [])) = 0 THEN 0.0
         ELSE 1.0 - multiset_common(a, b)
                    / (len(coalesce(a, [])) + len(coalesce(b, [])) - multiset_common(a, b))
    END
);

-- L1 distance Σ |a_i - b_i|: elements added plus removed.
CREATE MACRO l1_distance(a, b) AS (
    len(coalesce(a, [])) + len(coalesce(b, [])) - 2 * multiset_common(a, b)
);

-- Bag distance max(|A\B|, |B\A|): slots changed among interchangeable slots.
CREATE MACRO bag_distance(a, b) AS (
    greatest(len(coalesce(a, [])), len(coalesce(b, []))) - multiset_common(a, b)
);

-- The default build distance (ADR 0005): each tier's Jaccard distance
-- weighted by its share (0.4 / 0.3 / 0.2 / 0.1). Pass two build rows:
-- SELECT build_distance(a, b) FROM build a, build b WHERE ...
CREATE MACRO build_distance(a, b) AS (
    0.4 * jaccard_distance(a.tier1_elements, b.tier1_elements)
    + 0.3 * jaccard_distance(a.tier2_elements, b.tier2_elements)
    + 0.2 * jaccard_distance(a.tier3_elements, b.tier3_elements)
    + 0.1 * jaccard_distance(a.tier4_elements, b.tier4_elements)
);

-- Human-readable name of an element key.
CREATE MACRO element_name(key) AS (
    CASE (SELECT name FROM element_kind WHERE code = key >> 32)
        WHEN 'stat_investment' THEN (SELECT name FROM stat WHERE stat_hash = (key & 4294967295)::UINTEGER)
        WHEN 'set_bonus' THEN (SELECT name FROM sandbox_perk WHERE perk_hash = (key & 4294967295)::UINTEGER)
        WHEN 'weapon_element' THEN ['Kinetic', 'Arc', 'Solar', 'Void', 'Raid', 'Stasis', 'Strand'][(key & 4294967295)::INTEGER]
        ELSE (SELECT name FROM item WHERE item_hash = (key & 4294967295)::UINTEGER)
    END
);
