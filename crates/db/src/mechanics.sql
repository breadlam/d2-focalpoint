-- Community documentation of Destiny 2 mechanics, a layer written into an
-- existing database by `focalpoint mechanics import` from the raw sources
-- (docs/mechanics.md). Every import replaces it whole; its version is
-- meta.mechanics_version.

-- One row per source read.
CREATE OR REPLACE TABLE mechanic_source (
    source     VARCHAR PRIMARY KEY,  -- clarity | compendium | aegis
    title      VARCHAR NOT NULL,
    url        VARCHAR,
    fetched    DATE,
    entries    INTEGER NOT NULL,
    rotations  INTEGER NOT NULL,
    loadouts   INTEGER NOT NULL
);

-- One documented game element, or game state (Radiant, Scorch, ...), per
-- source.
CREATE OR REPLACE TABLE mechanic_entry (
    source    VARCHAR NOT NULL,
    entry_id  INTEGER NOT NULL,  -- per source, in source order
    section   VARCHAR NOT NULL,  -- the source's tab or type ("Armor Mods", "Weapon Trait Enhanced")
    name      VARCHAR NOT NULL,
    text      VARCHAR NOT NULL,  -- PvE text; lines separated by newlines
    PRIMARY KEY (source, entry_id)
);

-- What an entry documents, in the manifest. Game states have none.
CREATE OR REPLACE TABLE mechanic_subject (
    source     VARCHAR NOT NULL,
    entry_id   INTEGER NOT NULL,
    hash       UINTEGER NOT NULL,
    hash_kind  VARCHAR NOT NULL,  -- item | sandbox_perk (set bonuses) | item_set
    rule       VARCHAR NOT NULL,  -- hash (the source's own) | item (the weapon or armor a perk is on) | name | alias
    PRIMARY KEY (source, entry_id, hash, hash_kind)
);

-- Runs of text a source marks as a keyword: Clarity's element and ammo
-- classes (solar, special, ...), the Compendium's text colours (#rrggbb).
CREATE OR REPLACE TABLE mechanic_keyword (
    source    VARCHAR NOT NULL,
    entry_id  INTEGER NOT NULL,
    keyword   VARCHAR NOT NULL,
    mark      VARCHAR NOT NULL
);

-- Measured damage rotations: TheAegisRelic's boss damage workbook, one per
-- tested rotation of its ranking sheet.
CREATE OR REPLACE TABLE mechanic_rotation (
    source           VARCHAR NOT NULL,   -- aegis
    rotation_id      INTEGER NOT NULL,   -- in sheet order
    section          VARCHAR NOT NULL,   -- the sheet ("Ranking (S29)")
    category         VARCHAR NOT NULL,   -- Universal | Specialized | Support | ...
    class            VARCHAR,            -- Hunter | Titan | Warlock
    name             VARCHAR NOT NULL,
    boss             VARCHAR,
    core_dps         DOUBLE,             -- DPS of the rotation's repeating core
    dps_10s          DOUBLE,             -- average DPS after 10 seconds, ...
    dps_20s          DOUBLE,
    dps_30s          DOUBLE,
    dps_40s          DOUBLE,
    dps_50s          DOUBLE,
    dps_60s          DOUBLE,
    end_dps          DOUBLE,             -- average DPS when the measured run ends
    end_s            DOUBLE,
    end_damage       DOUBLE,
    continues        BOOLEAN,            -- whether the rotation goes on past that
    segment_start_s  DOUBLE,             -- the repeating core's span
    segment_end_s    DOUBLE,
    artifact_mods    VARCHAR,
    other_factors    VARCHAR,
    notes            VARCHAR,            -- the sheet's notes column and cell notes
    PRIMARY KEY (source, rotation_id)
);

-- The DIM loadouts (dim.gg shares) a rotation was run with.
CREATE OR REPLACE TABLE mechanic_rotation_loadout (
    source       VARCHAR NOT NULL,
    rotation_id  INTEGER NOT NULL,
    loadout_id   VARCHAR NOT NULL,   -- the share id
    role         VARCHAR NOT NULL,   -- loadout 1 | loadout 2 | note (a variant a note links)
    label        VARCHAR             -- a note's line
);

CREATE OR REPLACE TABLE mechanic_loadout (
    source      VARCHAR NOT NULL,
    loadout_id  VARCHAR NOT NULL,
    name        VARCHAR,
    class       VARCHAR,
    PRIMARY KEY (source, loadout_id)
);

-- What a loadout uses: equipped items, the choices in their sockets (the
-- subclass's super, abilities, aspects and fragments; exotic class item
-- perks), armor mods and artifact unlocks.
CREATE OR REPLACE TABLE mechanic_loadout_item (
    source        VARCHAR NOT NULL,
    loadout_id    VARCHAR NOT NULL,
    hash          UINTEGER NOT NULL,
    role          VARCHAR NOT NULL,   -- equipped | socket | mod | artifact
    parent_hash   UINTEGER,           -- for a socket choice: the item it is on
    socket_index  USMALLINT
);
