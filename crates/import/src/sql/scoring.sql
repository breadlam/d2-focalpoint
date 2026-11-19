-- Scoring: publish the references and per-run scores computed by the Rust
-- scoring stage (crates/import/src/scoring.rs, ADR 0006).
--
-- `perf` is ln(score) centred on the mean of the reference population, so a
-- player's skill and a build's effect are comparable across activities. A
-- bucket that borrows the activity-wide reference is centred on the
-- activity-wide mean, keeping its runs' advantage (e.g. a rare solo raid).

INSERT INTO activity_reference
WITH logs AS (
    SELECT i.activity_hash, i.is_solo, ln(p.kill_index * p.speed * p.survival) AS l
    FROM stg.run_perf p JOIN stg.score_input i USING (instance_id, membership_id)
), bucket AS (
    SELECT activity_hash, is_solo, avg(l) AS m FROM logs GROUP BY ALL
), activity AS (
    SELECT activity_hash, avg(l) AS m FROM logs GROUP BY ALL
)
SELECT r.activity_hash, r.is_solo, r.clears, r.kills, r.duration_s, r.deaths,
       r.is_borrowed, r.is_scored,
       CASE WHEN r.is_borrowed THEN a.m ELSE b.m END AS log_score_mean
FROM stg.reference r
LEFT JOIN bucket b USING (activity_hash, is_solo)
LEFT JOIN activity a USING (activity_hash)
ORDER BY r.activity_hash, r.is_solo;

INSERT INTO run_score
SELECT i.instance_id, i.membership_id, i.activity_hash, i.started_at, i.is_solo, i.has_build,
       i.kills, i.deaths, i.duration_s,
       p.kill_index, p.speed, p.survival,
       p.kill_index * p.speed * p.survival                          AS score,
       ln(p.kill_index * p.speed * p.survival) - r.log_score_mean   AS perf
FROM stg.run_perf p
JOIN stg.score_input i USING (instance_id, membership_id)
JOIN activity_reference r USING (activity_hash, is_solo)
ORDER BY i.instance_id, i.membership_id;
