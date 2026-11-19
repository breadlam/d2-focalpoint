-- Finalize: assemble the published `build` table from staging in one insert
-- (DuckDB rewrites list columns on UPDATE, which was slow at this scale).
-- Runs after the Rust investment stage.

-- step: build
INSERT INTO build
SELECT b.* REPLACE (
           s.s[1]                                 AS stat_weapons,
           s.s[2]                                 AS stat_health,
           s.s[3]                                 AS stat_class,
           s.s[4]                                 AS stat_grenade,
           s.s[5]                                 AS stat_super,
           s.s[6]                                 AS stat_melee,
           i.stat_levels                          AS stat_levels,
           i.stats_promoted                       AS stats_promoted,
           coalesce(i.tier3, b.tier3_elements)    AS tier3_elements,
           coalesce(u.timelines, 0)               AS timeline_count,
           coalesce(u.players, 0)                 AS player_count,
           coalesce(u.seconds, 0)                 AS seconds_used
       )
FROM stg.build b
LEFT JOIN stg.build_stats s USING (build_id)
LEFT JOIN stg.investment i USING (build_id)
LEFT JOIN stg.build_usage u USING (build_id)
ORDER BY b.build_id;
