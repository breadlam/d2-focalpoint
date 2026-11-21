-- Staging tables: raw extracts that the transform step (transform.sql) merges,
-- dedupes, and reshapes into the final schema. They live in a separate
-- database file attached as `stg` (in the work dir), so the published
-- database never contains staging data or the space it occupied. `src`
-- identifies the origin of a row:
--   0 = archive (raw PGCR JSON parsed by focalpoint-core)
--   1 = tracked (production's already-parsed run / run_entry tables)

CREATE TABLE stg.run (
    src                    UTINYINT NOT NULL,
    instance_id            BIGINT   NOT NULL,
    period                 VARCHAR,
    activity_hash          UINTEGER,
    reference_hash         UINTEGER,
    mode                   INTEGER,
    modes                  INTEGER[],
    difficulty_tier        INTEGER,
    duration_s             INTEGER,
    player_count           INTEGER,
    started_from_beginning BOOLEAN,
    is_private             BOOLEAN,
    starting_phase_index   INTEGER,
    skull_hashes           UINTEGER[],
    tracked_target         VARCHAR   -- production target_name (tracked only)
);

CREATE TABLE stg.entry (
    src               UTINYINT NOT NULL,
    instance_id       BIGINT   NOT NULL,
    membership_id     BIGINT   NOT NULL,
    character_id      BIGINT   NOT NULL,
    membership_type   INTEGER,
    display_name      VARCHAR,
    global_name       VARCHAR,
    global_name_code  INTEGER,
    class             VARCHAR,
    light_level       INTEGER,
    character_level   INTEGER,
    emblem_hash       UINTEGER,
    kills             INTEGER,
    deaths            INTEGER,
    assists           INTEGER,
    precision_kills   INTEGER,
    super_kills       INTEGER,
    melee_kills       INTEGER,
    grenade_kills     INTEGER,
    ability_kills     INTEGER,
    completed         BOOLEAN,
    completion_reason INTEGER,
    time_played_s     INTEGER,
    start_s           INTEGER
);

CREATE TABLE stg.weapon (
    src             UTINYINT NOT NULL,
    instance_id     BIGINT   NOT NULL,
    membership_id   BIGINT   NOT NULL,
    character_id    BIGINT   NOT NULL,
    item_hash       UINTEGER NOT NULL,
    kills           INTEGER,
    precision_kills INTEGER
);

-- Activity history rows from the live table and archive shards. `recency`
-- orders sources oldest → newest (the live table is newest); `row_key` is the
-- row's insertion rowid in the live table.
CREATE TABLE stg.history (
    recency           INTEGER  NOT NULL,
    row_key           BIGINT   NOT NULL,
    membership_id     BIGINT   NOT NULL,
    character_id      BIGINT,
    instance_id       BIGINT   NOT NULL,
    period            VARCHAR,
    activity_hash     UINTEGER,
    mode              INTEGER,
    kills             INTEGER,
    deaths            INTEGER,
    assists           INTEGER,
    duration_s        INTEGER,
    completion_reason INTEGER,
    completed         BOOLEAN,
    player_count      INTEGER
);

-- Production's player table: identity fallback and first-seen time.
CREATE TABLE stg.player (
    membership_id    BIGINT NOT NULL,
    membership_type  INTEGER,
    display_name     VARCHAR,
    global_name      VARCHAR,
    global_name_code INTEGER,
    first_seen_unix  BIGINT
);

CREATE TABLE stg.player_flag (
    membership_id BIGINT  NOT NULL,
    flag          VARCHAR NOT NULL   -- seeded | random_sample
);

-- Production's curated target slate, by activity hash.
CREATE TABLE stg.target_group (
    activity_hash UINTEGER NOT NULL,
    target_group  VARCHAR  NOT NULL
);

-- DestinyActivityDefinition rows (named later for hashes seen in the data).
CREATE TABLE stg.activity_def (
    activity_hash      UINTEGER NOT NULL,
    name               VARCHAR,
    description        VARCHAR,
    mode               INTEGER,
    modes              INTEGER[],
    activity_type_hash UINTEGER,
    is_playlist        BOOLEAN,
    light_level        INTEGER,
    difficulty_collection_hash UINTEGER
);

-- Builds as extracted, before stats, investment and usage are joined in by
-- finalize.sql (same columns as main.build).
CREATE TABLE stg.build AS SELECT * FROM main.build WHERE false;
