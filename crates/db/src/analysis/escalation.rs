//! Parts whose worth depends on the content: the pieces of synergistic
//! escalation loops (`docs/escalation.md`).
//!
//! An escalation loop — an ignition that ignites its neighbours, a melee
//! that refunds itself through the shards it makes — sustains itself only
//! past a threshold: in the build (enough Scorch per ignition to ignite)
//! and in the context (enemies that survive long enough, close enough). A
//! part that closes such a loop lifts its players in content where the
//! loop runs and not elsewhere, so its lift *differs by context*.
//!
//! Each scored clear with a recorded build is measured against its
//! player's usual level (the mean `perf` of their other scored clears;
//! players with at least [`MIN_CLEARS`]). Contexts are coarse for now:
//! **hard** (Conquests: enemies hard to kill), **easy** (Ops, Equilibrium,
//! dungeons) and **boss** (raids, Pantheon, The Desert Perpetual).
//!
//! * [`lifts`]: per subclass and part (artifact perk, fragment, aspect,
//!   exotic armor and its perk, set bonus), the mean of clears with the
//!   part minus those of the same subclass without it, per context —
//!   ranked by hard minus easy;
//! * [`combinations`]: builds with an anchor part, by which of a few other
//!   parts they also run, per context.

use crate::{Database, Result};

/// Scored clears a player needs for a usual level.
pub const MIN_CLEARS: i64 = 5;

/// Activity groups by context.
pub const HARD: [&str; 3] = [
    "Ultimate Conquest",
    "Grandmaster Conquest",
    "Grandmaster Conquest: Heliostat",
];
pub const EASY: [&str; 4] = [
    "Ultimate Fireteam Op",
    "Equilibrium",
    "Ultimate Pinnacle Op",
    "Master Dungeon",
];

fn quoted(groups: &[&str]) -> String {
    groups
        .iter()
        .map(|g| format!("'{}'", g.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The CTE `c (build_id, share, membership_id, sub, ctx, resid)`: scored
/// clears with a recorded build against their player's usual level.
#[must_use]
pub fn clears_cte() -> String {
    format!(
        "lvl AS (SELECT membership_id, avg(perf) AS level, count(*) AS n FROM run_score GROUP BY 1),
         c AS (
             SELECT rb.build_id, rb.share, r.instance_id, r.membership_id, b.class || ' ' || b.element AS sub,
                    CASE WHEN a.target_group IN ({hard}) THEN 'hard'
                         WHEN a.target_group IN ({easy}) THEN 'easy' ELSE 'boss' END AS ctx,
                    r.perf - (l.level * l.n - r.perf) / (l.n - 1) AS resid
             FROM run_score r JOIN run_build rb USING (instance_id, membership_id)
             JOIN activity a ON a.activity_hash = r.activity_hash
             JOIN lvl l ON l.membership_id = r.membership_id
             JOIN build b ON b.build_id = rb.build_id
             WHERE r.has_build AND l.n >= {MIN_CLEARS})",
        hard = quoted(&HARD),
        easy = quoted(&EASY)
    )
}

/// The CTE `parts (build_id, kind, part)`: the parts compared.
const PARTS_CTE: &str = "parts AS (
    SELECT DISTINCT bc.build_id, bc.kind, coalesce(i.name, sp.name) AS part
    FROM build_component bc
    LEFT JOIN item i ON i.item_hash = bc.component_hash AND bc.kind <> 'set_bonus'
    LEFT JOIN sandbox_perk sp ON sp.perk_hash = bc.component_hash AND bc.kind = 'set_bonus'
    WHERE bc.kind IN ('artifact_perk', 'fragment', 'aspect', 'set_bonus', 'armor_perk')
       OR (bc.kind = 'armor' AND i.is_exotic) OR (bc.kind = 'weapon' AND i.is_exotic))";

/// A part's lift in each context: clears with it against clears of the
/// same subclass without it (log scale; `exp(x) - 1` is the percentage).
#[derive(Debug, Clone, PartialEq)]
pub struct PartLift {
    pub subclass: String,
    pub kind: String,
    pub part: String,
    pub hard_clears: f64,
    pub easy_clears: f64,
    pub hard: f64,
    pub easy: f64,
    pub boss: Option<f64>,
}

/// Parts with at least `min_clears` clears and `min_players` players in
/// both the hard and easy contexts, by hard minus easy lift, highest first.
pub fn lifts(db: &Database, min_clears: f64, min_players: i64) -> Result<Vec<PartLift>> {
    let sql = format!(
        "WITH {clears}, {PARTS_CTE},
         sub_ctx AS (SELECT sub, ctx, sum(share) AS w, sum(share * resid) AS s FROM c GROUP BY ALL),
         with_part AS (
             SELECT c.sub, p.kind, p.part, c.ctx, sum(c.share) AS w, sum(c.share * c.resid) AS s,
                    count(DISTINCT c.membership_id) AS players
             FROM c JOIN parts p USING (build_id) GROUP BY ALL),
         lift AS (
             SELECT wp.*, wp.s / wp.w - (sc.s - wp.s) / (sc.w - wp.w) AS lift
             FROM with_part wp JOIN sub_ctx sc USING (sub, ctx) WHERE sc.w - wp.w >= 50),
         piv AS (
             SELECT sub, kind, part,
                    max(lift) FILTER (WHERE ctx = 'hard') AS hard, max(lift) FILTER (WHERE ctx = 'easy') AS easy,
                    max(lift) FILTER (WHERE ctx = 'boss') AS boss,
                    max(w) FILTER (WHERE ctx = 'hard') AS w_hard, max(w) FILTER (WHERE ctx = 'easy') AS w_easy,
                    max(players) FILTER (WHERE ctx = 'hard') AS p_hard, max(players) FILTER (WHERE ctx = 'easy') AS p_easy
             FROM lift GROUP BY ALL)
         SELECT sub, kind, part, w_hard, w_easy, hard, easy, boss FROM piv
         WHERE w_hard >= ? AND w_easy >= ? AND p_hard >= ? AND p_easy >= ?
         ORDER BY hard - easy DESC, sub, part",
        clears = clears_cte()
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map(
        duckdb::params![min_clears, min_clears, min_players, min_players],
        |r| {
            Ok(PartLift {
                subclass: r.get(0)?,
                kind: r.get(1)?,
                part: r.get(2)?,
                hard_clears: r.get(3)?,
                easy_clears: r.get(4)?,
                hard: r.get(5)?,
                easy: r.get(6)?,
                boss: r.get(7)?,
            })
        },
    )?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

/// Builds with an anchor part that run a given set of the other parts, in
/// one context: how many clears and players, and how they do against
/// their players' usual level (log scale).
#[derive(Debug, Clone, PartialEq)]
pub struct Combination {
    /// For each of the other parts, whether the builds run it.
    pub runs: Vec<bool>,
    pub context: String,
    pub clears: f64,
    pub players: i64,
    pub vs_level: f64,
}

/// Builds with a part named `anchor` (any kind, a super included), by
/// which of `parts` they also run, per context.
pub fn combinations(db: &Database, anchor: &str, parts: &[String]) -> Result<Vec<Combination>> {
    let flags = parts
        .iter()
        .enumerate()
        .map(|(i, _)| format!("bool_or(i.name = ?) AS p{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let cols = (0..parts.len())
        .map(|i| format!("b.p{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "WITH {clears},
         anchored AS (
             SELECT DISTINCT bc.build_id FROM build_component bc JOIN item i ON i.item_hash = bc.component_hash
             WHERE i.name = ?),
         b AS (
             SELECT bc.build_id, {flags}
             FROM build_component bc LEFT JOIN item i ON i.item_hash = bc.component_hash
             WHERE bc.build_id IN (SELECT build_id FROM anchored) GROUP BY 1)
         SELECT {cols}, c.ctx, sum(c.share), count(DISTINCT c.membership_id), sum(c.share * c.resid) / sum(c.share)
         FROM c JOIN b USING (build_id)
         GROUP BY ALL ORDER BY c.ctx, {cols}",
        clears = clears_cte(),
        flags = if flags.is_empty() { "true AS p".to_string() } else { flags },
        cols = if cols.is_empty() { "b.p".to_string() } else { cols },
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let mut params: Vec<&dyn duckdb::ToSql> = vec![&anchor];
    params.extend(parts.iter().map(|p| p as &dyn duckdb::ToSql));
    let n = parts.len().max(1);
    let rows = stmt.query_map(params.as_slice(), |r| {
        let mut runs = Vec::with_capacity(parts.len());
        for i in 0..parts.len() {
            runs.push(r.get::<_, Option<bool>>(i)?.unwrap_or(false));
        }
        Ok(Combination {
            runs,
            context: r.get(n)?,
            clears: r.get(n + 1)?,
            players: r.get(n + 2)?,
            vs_level: r.get(n + 3)?,
        })
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Player 1 clears Ultimate Conquest five times: twice with the anchor
    /// exotic and the loop part (+0.4), three times with the anchor alone
    /// (−0.2).
    #[test]
    fn combinations_split_anchored_builds_by_the_parts_they_run() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(&dir.path().join("e.duckdb")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO activity (activity_hash, name, target_group) VALUES (100, 'UC', 'Ultimate Conquest');
                 INSERT INTO item (item_hash, name, is_exotic) VALUES (1, 'Anchor Boots', true), (2, 'Loop Part', false);
                 INSERT INTO build (build_id, class, element, aspect_hashes, fragment_hashes, artifact_perk_hashes,
                     armor_mod_hashes, item_hashes, component_count, unresolved_count,
                     tier1_elements, tier2_elements, tier3_elements, tier4_elements)
                 VALUES (10, 'Warlock', 'Solar', [], [], [], [], [], 0, 0, [], [], [], []),
                        (11, 'Warlock', 'Solar', [], [], [], [], [], 0, 0, [], [], [], []);
                 INSERT INTO build_component (build_id, kind, slot, component_hash, parent_hash, quantity, position)
                 VALUES (10, 'armor', 'legs', 1, NULL, 1, 0), (10, 'artifact_perk', 'artifact', 2, NULL, 1, 0),
                        (11, 'armor', 'legs', 1, NULL, 1, 0);
                 INSERT INTO run_score (instance_id, membership_id, activity_hash, started_at, is_solo, is_partial,
                     has_build, kills, deaths, duration_s, played_s, kill_index, speed, survival, score, perf)
                 SELECT i, 1, 100, TIMESTAMP '2026-07-01' + INTERVAL (i) DAY, false, false, true, 10, 0, 600, 600, 1, 1, 1, 1, p
                 FROM (VALUES (1, 0.4), (2, 0.4), (3, -0.2), (4, -0.2), (5, -0.2)) v(i, p);
                 INSERT INTO run_build VALUES (1, 1, 10, 1, 0, 600, 1.0, true), (2, 1, 10, 1, 0, 600, 1.0, true),
                     (3, 1, 11, 1, 0, 600, 1.0, true), (4, 1, 11, 1, 0, 600, 1.0, true), (5, 1, 11, 1, 0, 600, 1.0, true);",
            )
            .unwrap();
        let c = combinations(&db, "Anchor Boots", &["Loop Part".to_string()]).unwrap();
        let got: Vec<(bool, &str, i64)> = c
            .iter()
            .map(|c| (c.runs[0], c.context.as_str(), c.clears.round() as i64))
            .collect();
        assert_eq!(got, [(false, "hard", 3), (true, "hard", 2)]);
        // Each clear against the mean of the player's other four: +0.4 against
        // −0.05, −0.2 against +0.1.
        assert!((c[1].vs_level - 0.45).abs() < 1e-9, "{c:?}");
        assert!((c[0].vs_level + 0.30).abs() < 1e-9, "{c:?}");
    }
}
