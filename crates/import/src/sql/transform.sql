-- Transform: staging tables -> final schema.
--
-- Each `-- step: <name>` block runs as one unit (reported as progress). Steps
-- run in file order; later steps may depend on earlier ones. Source
-- precedence for runs: a raw archived PGCR (src 0) wins over production's
-- pre-parsed tracked rows (src 1) for the same instance, since it is the
-- unmodified Bungie report; tracked-only facts (qualified target) are kept.

-- step: choose run sources
CREATE TEMP TABLE run_choice AS
SELECT instance_id,
       min(src)                                        AS src,
       bool_or(src = 1)                                AS is_tracked,
       max(tracked_target) FILTER (WHERE src = 1)      AS tracked_target
FROM stg.run
GROUP BY instance_id;

CREATE TEMP TABLE roster AS
SELECT e.instance_id, count(DISTINCT (e.membership_id, e.character_id)) AS n
FROM stg.entry e
JOIN run_choice c ON c.instance_id = e.instance_id AND c.src = e.src
GROUP BY e.instance_id;

-- Skull-derived difficulty gates (production's derive_run_difficulty).
CREATE TEMP TABLE run_skulls AS
SELECT u.instance_id,
       coalesce(bool_or(s.kind = 'boon'), false)                         AS has_boon,
       coalesce(bool_or(s.kind = 'equipment_lock'), false)               AS equipment_locked,
       count(DISTINCT s.skull_hash) FILTER (WHERE s.kind = 'challenge')  AS challenge_count,
       count(DISTINCT s.skull_hash) FILTER (WHERE s.kind = 'feat')       AS feat_count
FROM (
    SELECT r.instance_id, unnest(r.skull_hashes) AS skull_hash
    FROM stg.run r
    JOIN run_choice c ON c.instance_id = r.instance_id AND c.src = r.src
) u
LEFT JOIN skull s ON s.skull_hash = u.skull_hash
GROUP BY u.instance_id;

-- step: run
INSERT INTO run
SELECT r.instance_id,
       TRY_CAST(r.period AS TIMESTAMP)                  AS started_at,
       r.activity_hash,
       r.reference_hash,
       r.mode,
       r.modes,
       r.difficulty_tier,
       r.duration_s,
       r.player_count,
       coalesce(ro.n, 0)                                AS roster_size,
       coalesce(ro.n, 0) = 1                            AS is_solo,
       r.started_from_beginning,
       r.is_private,
       r.starting_phase_index,
       coalesce(r.skull_hashes, []::UINTEGER[])         AS skull_hashes,
       sk.has_boon,
       sk.equipment_locked,
       sk.challenge_count,
       sk.feat_count,
       CASE WHEN c.tracked_target <> '?' THEN c.tracked_target END AS qualified_target,
       CASE r.src WHEN 0 THEN 'archive' ELSE 'tracked' END         AS source,
       c.is_tracked
FROM stg.run r
JOIN run_choice c ON c.instance_id = r.instance_id AND c.src = r.src
LEFT JOIN roster ro ON ro.instance_id = r.instance_id
LEFT JOIN run_skulls sk ON sk.instance_id = r.instance_id
WHERE TRY_CAST(r.period AS TIMESTAMP) IS NOT NULL
ORDER BY started_at, r.instance_id;

-- step: run_player
CREATE TEMP TABLE timeline_key AS
SELECT DISTINCT instance_id, membership_id FROM segment;

INSERT INTO run_player
SELECT DISTINCT ON (e.instance_id, e.membership_id, e.character_id)
       e.instance_id,
       e.membership_id,
       e.character_id,
       r.started_at,
       r.activity_hash,
       CASE WHEN e.class IN ('Titan', 'Hunter', 'Warlock') THEN e.class ELSE 'Unknown' END,
       e.light_level,
       e.character_level,
       e.emblem_hash,
       coalesce(e.kills, 0),
       coalesce(e.deaths, 0),
       coalesce(e.assists, 0),
       coalesce(e.precision_kills, 0),
       coalesce(e.super_kills, 0),
       coalesce(e.melee_kills, 0),
       coalesce(e.grenade_kills, 0),
       coalesce(e.ability_kills, 0),
       coalesce(e.completed, false),
       e.completion_reason,
       coalesce(e.completed, false) AND e.completion_reason = 0 AS is_clear,
       e.time_played_s,
       e.start_s,
       t.instance_id IS NOT NULL AS has_build
FROM stg.entry e
JOIN run_choice c ON c.instance_id = e.instance_id AND c.src = e.src
JOIN run r ON r.instance_id = e.instance_id
LEFT JOIN timeline_key t ON t.instance_id = e.instance_id AND t.membership_id = e.membership_id
ORDER BY e.instance_id, e.membership_id, e.character_id;

-- step: run_player_weapon
INSERT INTO run_player_weapon
SELECT w.instance_id, w.membership_id, w.character_id, w.item_hash,
       sum(coalesce(w.kills, 0))::INTEGER, sum(coalesce(w.precision_kills, 0))::INTEGER
FROM stg.weapon w
JOIN run_choice c ON c.instance_id = w.instance_id AND c.src = w.src
WHERE EXISTS (SELECT 1 FROM run r WHERE r.instance_id = w.instance_id)
GROUP BY ALL
ORDER BY 1, 2, 3, 4;

-- step: activity_history
INSERT INTO activity_history
SELECT DISTINCT ON (membership_id, instance_id)
       membership_id,
       coalesce(character_id, 0),
       instance_id,
       TRY_CAST(period AS TIMESTAMP),
       coalesce(activity_hash, 0),
       mode, kills, deaths, assists, duration_s, completion_reason, completed, player_count
FROM stg.history
WHERE TRY_CAST(period AS TIMESTAMP) IS NOT NULL
ORDER BY membership_id, instance_id, src DESC;

-- step: run_build
INSERT INTO run_build
WITH per_build AS (
    SELECT instance_id, membership_id, build_id,
           count(*)          AS segments,
           min(start_s)      AS first_start_s,
           sum(duration_s)   AS duration_s
    FROM segment
    GROUP BY ALL
), per_timeline AS (
    SELECT instance_id, membership_id, sum(duration_s) AS total
    FROM segment
    GROUP BY ALL
)
SELECT b.instance_id, b.membership_id, b.build_id, b.segments, b.first_start_s, b.duration_s,
       CASE WHEN t.total > 0 THEN b.duration_s / t.total END AS share,
       row_number() OVER (PARTITION BY b.instance_id, b.membership_id
                          ORDER BY b.duration_s DESC, b.first_start_s, b.build_id) = 1 AS is_primary
FROM per_build b
JOIN per_timeline t USING (instance_id, membership_id)
ORDER BY b.instance_id, b.membership_id, b.build_id;

-- step: activity
INSERT INTO activity
WITH counts AS (
    SELECT activity_hash, count(*) AS run_count FROM run GROUP BY 1
), hashes AS (
    SELECT activity_hash FROM stg.activity_def
    UNION SELECT activity_hash FROM counts
    UNION SELECT DISTINCT activity_hash FROM activity_history
    UNION SELECT activity_hash FROM stg.target_group
), groups AS (
    SELECT activity_hash, min(target_group) AS target_group FROM stg.target_group GROUP BY 1
)
SELECT h.activity_hash, d.name, d.description, d.mode, d.modes, d.activity_type_hash,
       d.is_playlist, d.light_level, g.target_group, coalesce(c.run_count, 0)
FROM hashes h
LEFT JOIN stg.activity_def d USING (activity_hash)
LEFT JOIN groups g USING (activity_hash)
LEFT JOIN counts c USING (activity_hash)
ORDER BY h.activity_hash;

-- step: player
INSERT INTO player
WITH ids AS (
    SELECT membership_id FROM run_player
    UNION SELECT membership_id FROM segment
    UNION SELECT membership_id FROM activity_history
    UNION SELECT membership_id FROM stg.player
    UNION SELECT membership_id FROM stg.player_flag
), latest AS (
    -- Most recent identity seen in any PGCR.
    SELECT DISTINCT ON (e.membership_id)
           e.membership_id, e.membership_type, e.display_name, e.global_name, e.global_name_code
    FROM stg.entry e
    JOIN run r ON r.instance_id = e.instance_id
    ORDER BY e.membership_id, r.started_at DESC, e.src
), seen AS (
    SELECT membership_id, min(t) AS first_seen, max(t) AS last_seen
    FROM (
        SELECT membership_id, started_at AS t FROM run_player
        UNION ALL SELECT membership_id, started_at FROM activity_history
    )
    GROUP BY 1
), runs AS (
    SELECT membership_id, count(*) AS run_count FROM run_player GROUP BY 1
), timelines AS (
    SELECT membership_id, count(*) AS n FROM timeline_key GROUP BY 1
), flags AS (
    SELECT membership_id,
           bool_or(flag = 'seeded')        AS is_seeded,
           bool_or(flag = 'random_sample') AS is_random_sample
    FROM stg.player_flag GROUP BY 1
), prod AS (
    SELECT DISTINCT ON (membership_id) * FROM stg.player ORDER BY membership_id
)
SELECT i.membership_id,
       coalesce(l.membership_type, p.membership_type),
       coalesce(l.display_name, p.display_name),
       coalesce(l.global_name, p.global_name),
       coalesce(l.global_name_code, p.global_name_code),
       CASE WHEN coalesce(l.global_name, p.global_name) IS NOT NULL
                 AND coalesce(l.global_name_code, p.global_name_code) IS NOT NULL
            THEN coalesce(l.global_name, p.global_name) || '#'
                 || lpad(coalesce(l.global_name_code, p.global_name_code)::VARCHAR, 4, '0')
       END,
       least(s.first_seen, make_timestamp(p.first_seen_unix * 1000000)),
       s.last_seen,
       coalesce(r.run_count, 0),
       coalesce(t.n, 0),
       coalesce(f.is_seeded, false),
       coalesce(f.is_random_sample, false)
FROM ids i
LEFT JOIN latest l USING (membership_id)
LEFT JOIN prod p USING (membership_id)
LEFT JOIN seen s USING (membership_id)
LEFT JOIN runs r USING (membership_id)
LEFT JOIN timelines t USING (membership_id)
LEFT JOIN flags f USING (membership_id)
ORDER BY i.membership_id;

-- step: character
INSERT INTO "character"
WITH from_runs AS (
    SELECT character_id,
           arg_max(membership_id, started_at) AS membership_id,
           mode(class) FILTER (WHERE class <> 'Unknown') AS class
    FROM run_player
    GROUP BY character_id
), from_history AS (
    SELECT character_id, arg_max(membership_id, started_at) AS membership_id
    FROM activity_history
    WHERE character_id <> 0
    GROUP BY character_id
)
SELECT coalesce(r.character_id, h.character_id),
       coalesce(r.membership_id, h.membership_id),
       r.class
FROM from_runs r
FULL OUTER JOIN from_history h ON h.character_id = r.character_id
ORDER BY 1;
