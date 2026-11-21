//! What sets off the parts of the builds players run in each activity
//! group, against the boss damage workbook's measured rotations: a check
//! that the trigger tags read from the mechanics texts (the mechanics
//! layer's `mechanic_hash_tag`) say something about where builds are
//! played. Where kills are scarce or hard, players should lean less on
//! kill-triggered parts.
//!
//! Only parts both sides record are compared — subclass choices (super,
//! abilities, aspects, fragments), armor mods and exotic armor — since the
//! workbook's shared loadouts carry no weapon perk rolls. Each build counts
//! by its clears (a clear split across builds by its share).

use crate::{Database, Result};

/// Triggers reported, as prefixes of the trigger tags.
pub const TRIGGERS: [&str; 6] = [
    "kill",
    "hit",
    "ability_use",
    "pickup",
    "surrounded",
    "passive",
];

/// A group's builds (or the workbook's rotations): how many, how many
/// documented parts each, and the share of those parts set off by each of
/// [`TRIGGERS`].
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerMix {
    pub group: String,
    /// Clears (or rotations) counted.
    pub weight: f64,
    pub parts: f64,
    pub shares: [f64; 6],
}

fn trigger_columns() -> (String, String) {
    let flags = TRIGGERS
        .iter()
        .enumerate()
        .map(|(i, t)| format!("bool_or(tag = '{t}' OR tag LIKE '{t}.%') AS t{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let means = (0..TRIGGERS.len())
        .map(|i| format!("avg(t.t{i}::DOUBLE) AS t{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    (flags, means)
}

/// The trigger mix of each activity group's recorded builds, then of the
/// workbook's rotations (their first loadout each).
pub fn by_group(db: &Database) -> Result<Vec<TriggerMix>> {
    let (flags, means) = trigger_columns();
    let weighted = (0..TRIGGERS.len())
        .map(|i| format!("sum(share * t{i}) / sum(share)"))
        .collect::<Vec<_>>()
        .join(", ");
    let plain = (0..TRIGGERS.len())
        .map(|i| format!("avg(t{i})"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "WITH trig AS (SELECT hash, {flags} FROM mechanic_hash_tag WHERE role = 'trigger' GROUP BY 1),
         exotic_armor AS (SELECT item_hash FROM item WHERE is_exotic AND item_type = 2),
         comp AS (
             SELECT DISTINCT bc.build_id, bc.component_hash AS hash FROM build_component bc
             WHERE bc.kind IN ('fragment', 'aspect', 'grenade', 'melee', 'class_ability', 'super', 'armor_mod')
                OR (bc.kind = 'armor' AND bc.component_hash IN (SELECT item_hash FROM exotic_armor))),
         per_build AS (SELECT c.build_id, count(*) AS n, {means} FROM comp c JOIN trig t USING (hash) GROUP BY 1),
         clears AS (
             SELECT a.target_group AS grp, rb.share, pb.*
             FROM run_score r JOIN run_build rb USING (instance_id, membership_id)
             JOIN activity a ON a.activity_hash = r.activity_hash
             JOIN per_build pb ON pb.build_id = rb.build_id
             WHERE r.has_build),
         rotation_parts AS (
             SELECT DISTINCT li.loadout_id, li.hash
             FROM mechanic_loadout_item li JOIN mechanic_rotation_loadout rl USING (source, loadout_id)
             JOIN item i ON i.item_hash = li.hash
             WHERE rl.role = 'loadout 1'
               AND (i.plug_kind IN ('fragment', 'aspect', 'grenade', 'melee', 'class_ability', 'super', 'armor_mod')
                    OR li.hash IN (SELECT item_hash FROM exotic_armor))),
         per_rotation AS (SELECT c.loadout_id, count(*) AS n, {means} FROM rotation_parts c JOIN trig t USING (hash) GROUP BY 1)
         SELECT * FROM (
             SELECT grp, sum(share), sum(share * n) / sum(share), {weighted} FROM clears GROUP BY grp
             ORDER BY 4 DESC)
         UNION ALL
         SELECT 'Boss damage workbook rotations', count(*)::DOUBLE, avg(n), {plain} FROM per_rotation"
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        let mut shares = [0.0; 6];
        for (i, s) in shares.iter_mut().enumerate() {
            *s = r.get::<_, Option<f64>>(3 + i)?.unwrap_or(f64::NAN);
        }
        Ok(TriggerMix {
            group: r.get(0)?,
            weight: r.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
            parts: r.get::<_, Option<f64>>(2)?.unwrap_or(f64::NAN),
            shares,
        })
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One Ultimate Conquest clear with a build of two documented fragments,
    /// one kill-triggered and one hit-triggered, and an undocumented one.
    #[test]
    fn shares_count_documented_parts_by_trigger() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(&dir.path().join("t.duckdb")).unwrap();
        db.conn().execute_batch(crate::MECHANICS_SQL).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO activity (activity_hash, name, target_group) VALUES (100, 'UC', 'Ultimate Conquest');
                 INSERT INTO item (item_hash, name, is_exotic, plug_kind) VALUES (5, 'Kill Fragment', false, 'fragment'),
                     (6, 'Hit Fragment', false, 'fragment'), (7, 'Quiet Fragment', false, 'fragment');
                 INSERT INTO build_component (build_id, kind, slot, component_hash, parent_hash, quantity, position)
                 VALUES (1, 'fragment', 'subclass', 5, NULL, 1, 9), (1, 'fragment', 'subclass', 6, NULL, 1, 10),
                        (1, 'fragment', 'subclass', 7, NULL, 1, 11);
                 INSERT INTO run_score (instance_id, membership_id, activity_hash, started_at, is_solo, is_partial,
                     has_build, kills, deaths, duration_s, played_s, kill_index, speed, survival, score, perf)
                 VALUES (1, 1, 100, '2026-07-01', false, false, true, 10, 0, 600, 600, 1, 1, 1, 1, 0);
                 INSERT INTO run_build VALUES (1, 1, 1, 1, 0, 600, 1.0, true);
                 INSERT INTO mechanic_subject VALUES ('clarity', 0, 5, 'item', 'hash'), ('clarity', 1, 6, 'item', 'hash');
                 INSERT INTO mechanic_tag VALUES ('clarity', 0, 'trigger', 'kill.ability', 'Grenade kills grant ...'),
                        ('clarity', 1, 'trigger', 'hit', 'Dealing damage grants ...');",
            )
            .unwrap();
        let mix = by_group(&db).unwrap();
        assert_eq!(mix[0].group, "Ultimate Conquest");
        assert_eq!(
            (mix[0].weight, mix[0].parts),
            (1.0, 2.0),
            "the undocumented part is not counted"
        );
        assert_eq!(&mix[0].shares[..2], &[0.5, 0.5]);
        assert!(mix[1].weight.abs() < f64::EPSILON, "no rotations imported");
    }
}
