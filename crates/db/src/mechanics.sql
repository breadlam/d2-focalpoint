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

-- What each entry's text says its subject needs and does
-- (focalpoint_mechanics::effects; docs/effects.md): triggers, payoffs, and
-- game states it produces or depends on, with the sentence each was read from.
CREATE OR REPLACE TABLE mechanic_tag (
    source    VARCHAR NOT NULL,
    entry_id  INTEGER NOT NULL,
    role      VARCHAR NOT NULL,  -- trigger | payoff | produces | depends
    tag       VARCHAR NOT NULL,  -- dotted and open-ended: kill.precision, energy.grenade, Radiant, ...
    evidence  VARCHAR NOT NULL
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

-- Contexts a build is evaluated in: activity groups, activities, encounters,
-- each a child of the one above (docs/contexts.md). Profiles are
-- open-ended: any number of features and timed events, each with its source
-- (runs | aegis | curated).
CREATE OR REPLACE TABLE context (
    context_id  VARCHAR PRIMARY KEY,  -- group:<slug> | activity:<slug> | encounter:<activity slug>/<boss slug>
    kind        VARCHAR NOT NULL,     -- group | activity | encounter
    parent_id   VARCHAR,
    name        VARCHAR NOT NULL
);

-- The manifest activities a context covers.
CREATE OR REPLACE TABLE context_activity (
    context_id     VARCHAR NOT NULL,
    activity_hash  UINTEGER NOT NULL,
    PRIMARY KEY (context_id, activity_hash)
);

CREATE OR REPLACE TABLE context_feature (
    context_id  VARCHAR NOT NULL,
    feature     VARCHAR NOT NULL,  -- dotted and open-ended: kills_per_min, boss.dps_window_s, adds.during_dps, ...
    value       DOUBLE,
    text        VARCHAR,
    source      VARCHAR NOT NULL,
    PRIMARY KEY (context_id, feature, source)
);

CREATE OR REPLACE TABLE context_event (
    context_id  VARCHAR NOT NULL,
    seq         INTEGER NOT NULL,  -- in the order given
    kind        VARCHAR NOT NULL,  -- dps_window | add_wave | mechanic | ...
    start_s     DOUBLE,
    length_s    DOUBLE,
    detail      VARCHAR,
    source      VARCHAR NOT NULL
);

-- Each manifest hash's tags over every source documenting it (not those of a
-- perk documented with its weapon: a weapon's perks are in the build too).
CREATE OR REPLACE VIEW mechanic_hash_tag AS
SELECT s.hash, s.hash_kind, t.role, t.tag,
       list(DISTINCT t.source ORDER BY t.source) AS sources,
       min(t.evidence) AS evidence
FROM mechanic_subject s JOIN mechanic_tag t USING (source, entry_id)
WHERE s.rule <> 'item'
GROUP BY ALL;

-- Enemy durability (focalpoint_mechanics::durability; docs/durability.md).
-- Health is in the game's health units at an activity health tier, before
-- Power delta: damage to kill = health / the delta's damage multiplier.

-- MossyMax's Foetracer-measured health of each enemy archetype.
CREATE OR REPLACE TABLE enemy_health (
    source       VARCHAR NOT NULL,
    faction      VARCHAR NOT NULL,   -- Cabal | Fallen | Hive | Vex | Taken | Scorn
    enemy        VARCHAR NOT NULL,
    rank         VARCHAR NOT NULL,   -- minor | elite
    base         DOUBLE NOT NULL,    -- the source's base health class (50 ... 4400)
    health_tier  UTINYINT NOT NULL,  -- 1: patrol, story; 2: matchmade strikes; 3: Legend Nightfall, dungeons; 4: Master raids, Master/GM Nightfalls (before Edge of Fate)
    health       DOUBLE NOT NULL
);

-- Measured bosses and minibosses.
CREATE OR REPLACE TABLE enemy_measured (
    source            VARCHAR NOT NULL,
    kind              VARCHAR NOT NULL,  -- boss | miniboss
    enemy             VARCHAR NOT NULL,
    activity          VARCHAR,
    health_tier       UTINYINT,
    estimated_health  DOUBLE,            -- damage dealt to kill, as measured
    baseline_health   DOUBLE             -- in health units
);

-- Outgoing damage by Power delta, relative to a delta of 0 (Edge of Fate).
CREATE OR REPLACE TABLE power_delta (
    delta              INTEGER PRIMARY KEY,
    damage_multiplier  DOUBLE NOT NULL,
    source             VARCHAR NOT NULL
);

-- Champion health, shields and regeneration by activity difficulty.
CREATE OR REPLACE TABLE champion_stat (
    source      VARCHAR NOT NULL,
    champion    VARCHAR NOT NULL,   -- Barrier Champion | Overload Champion | Unstoppable Champion
    property    VARCHAR NOT NULL,   -- Champion Health, Shield Health, ...
    difficulty  VARCHAR NOT NULL,   -- Standard | Raid (Expert/Master) | Advanced | Expert | Master Grandmaster
    value       DOUBLE,
    text        VARCHAR NOT NULL
);

-- data/difficulty.toml's entries, resolved to contexts: the Power cap,
-- combatant Power and delta of the runs of the context's activities at the
-- steps listed (any step when empty).
CREATE OR REPLACE TABLE difficulty (
    context_id       VARCHAR NOT NULL,
    context          VARCHAR NOT NULL,
    depth            UTINYINT NOT NULL,   -- 0 group, 1 activity: the more specific entry wins
    steps            VARCHAR[] NOT NULL,  -- difficulty_tier.name values; empty for any
    power_cap        INTEGER,
    combatant_power  INTEGER,
    power_delta      INTEGER,
    health_tier      UTINYINT,
    confidence       VARCHAR NOT NULL,    -- documented | dated | assumed
    evidence         VARCHAR NOT NULL
);

-- What a skull adds to the Power cap and the combatants' Power in the runs
-- of the context's activities that have it (Feats: +20/+20 each,
-- Cutthroat Combat +20/+50).
CREATE OR REPLACE TABLE difficulty_modifier (
    context_id       VARCHAR NOT NULL,
    skull            VARCHAR NOT NULL,    -- skull.name
    power_cap        INTEGER NOT NULL,
    combatant_power  INTEGER NOT NULL,
    confidence       VARCHAR NOT NULL,
    evidence         VARCHAR NOT NULL
);

-- Each run's Power cap, combatant Power, delta and durability: the most
-- specific difficulty entry for its activity and step, plus its skulls'
-- modifiers. Runs with no entry for their step are absent.
CREATE OR REPLACE VIEW run_difficulty AS
WITH covered AS (
    SELECT DISTINCT ca.activity_hash
    FROM context_activity ca JOIN difficulty d USING (context_id)
), runs AS (
    SELECT r.instance_id, r.activity_hash, r.skull_hashes, t.name AS step
    FROM run r
    JOIN covered USING (activity_hash)
    JOIN activity a USING (activity_hash)
    LEFT JOIN difficulty_tier t
           ON t.collection_hash = a.difficulty_collection_hash AND t.tier = r.difficulty_tier
), base AS (
    SELECT u.instance_id, u.activity_hash, u.skull_hashes, d.context_id,
           d.power_cap, d.combatant_power, d.power_delta, d.confidence
    FROM runs u
    JOIN context_activity ca ON ca.activity_hash = u.activity_hash
    JOIN difficulty d ON d.context_id = ca.context_id
    WHERE d.power_delta IS NOT NULL
      AND (len(d.steps) = 0 OR list_contains(d.steps, u.step))
    QUALIFY row_number() OVER (PARTITION BY u.instance_id ORDER BY d.depth DESC) = 1
), mods AS (
    SELECT ca.activity_hash, m.skull, m.power_cap, m.combatant_power, list(s.skull_hash) AS skulls
    FROM difficulty_modifier m
    JOIN context_activity ca USING (context_id)
    JOIN skull s ON s.name = m.skull
    GROUP BY ALL
), adjust AS (
    SELECT b.instance_id, sum(m.power_cap) AS cap, sum(m.combatant_power) AS combatants
    FROM base b
    JOIN mods m ON m.activity_hash = b.activity_hash AND list_has_any(b.skull_hashes, m.skulls)
    GROUP BY 1
), runs_delta AS (
    SELECT b.instance_id, b.context_id, b.confidence,
           b.power_cap + coalesce(j.cap, 0) AS power_cap,
           b.combatant_power + coalesce(j.combatants, 0) AS combatant_power,
           (b.power_delta + coalesce(j.cap, 0) - coalesce(j.combatants, 0))::INTEGER AS power_delta
    FROM base b LEFT JOIN adjust j USING (instance_id)
)
SELECT r.instance_id, r.context_id, r.power_cap, r.combatant_power, r.power_delta,
       p.damage_multiplier, 1 / p.damage_multiplier AS durability, r.confidence
FROM runs_delta r
LEFT JOIN power_delta p ON p.delta = greatest(-100, least(100, r.power_delta));

-- Each enemy archetype's damage to kill in each context with a health tier
-- and a durability (data/difficulty.toml).
CREATE OR REPLACE VIEW context_enemy_health AS
SELECT c.context_id, c.kind, c.name, e.faction, e.enemy, e.rank, e.health,
       e.health * d.value AS effective_health
FROM context c
JOIN context_feature t ON t.context_id = c.context_id AND t.feature = 'health_tier'
JOIN context_feature d ON d.context_id = c.context_id AND d.feature = 'durability'
JOIN enemy_health e ON e.health_tier = t.value;
