//! Which captured builds are missing significant parts, and how much
//! evidence they carry.
//!
//! Every captured build has a subclass, super, abilities, three weapons and
//! five armor pieces; what goes missing is the rest of the kit (aspects,
//! fragments), armor mods and artifact perks — a loadout read mid-change,
//! or sockets the API did not return. Such a build looks unlike everything
//! (the parts it lacks are parts it does not share), so it can win a
//! highlight on novelty it does not have.

use std::collections::HashMap;
use std::hash::BuildHasher;

use crate::{Database, Result};

/// A complete kit has two aspects and at least this many fragments: no pair
/// of aspects is usually run with fewer (`fragment_gaps`).
pub const MIN_FRAGMENTS: u32 = 4;

/// Counts of a build's parts that can go missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Parts {
    pub aspects: u32,
    pub fragments: u32,
    /// Grenade, melee, class ability, movement present.
    pub abilities: u32,
    pub armor_mods: u32,
    pub artifact_perks: u32,
}

impl Parts {
    /// Two aspects, at least [`MIN_FRAGMENTS`] fragments and all four abilities.
    #[must_use]
    pub fn kit_complete(&self) -> bool {
        self.aspects >= 2 && self.fragments >= MIN_FRAGMENTS && self.abilities >= 4
    }

    /// A complete kit and armor mods.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.kit_complete() && self.armor_mods > 0
    }
}

/// The parts of every build.
pub fn parts(db: &Database) -> Result<HashMap<i64, Parts>> {
    let mut stmt = db.conn().prepare(
        "SELECT build_id,
                count(*) FILTER (WHERE kind = 'aspect'),
                count(*) FILTER (WHERE kind = 'fragment'),
                count(*) FILTER (WHERE kind IN ('grenade', 'melee', 'class_ability', 'movement')),
                count(*) FILTER (WHERE kind = 'armor_mod'),
                count(*) FILTER (WHERE kind = 'artifact_perk')
         FROM build_component GROUP BY build_id",
    )?;
    let mut out = HashMap::new();
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let n = |i: usize| -> Result<u32> {
            Ok(u32::try_from(r.get::<_, i64>(i)?).unwrap_or(u32::MAX))
        };
        out.insert(
            r.get(0)?,
            Parts {
                aspects: n(1)?,
                fragments: n(2)?,
                abilities: n(3)?,
                armor_mods: n(4)?,
                artifact_perks: n(5)?,
            },
        );
    }
    Ok(out)
}

/// Builds by aspects and fragments (6 standing for 6 or more), with how
/// many of them have no armor mods and no artifact perks:
/// `(aspects, fragments, builds, without mods, without artifact perks)`.
#[must_use]
pub fn distribution<S: BuildHasher>(
    parts: &HashMap<i64, Parts, S>,
) -> Vec<(u32, u32, usize, usize, usize)> {
    let mut cells: HashMap<(u32, u32), (usize, usize, usize)> = HashMap::new();
    for p in parts.values() {
        let c = cells.entry((p.aspects, p.fragments.min(6))).or_default();
        c.0 += 1;
        c.1 += usize::from(p.armor_mods == 0);
        c.2 += usize::from(p.artifact_perks == 0);
    }
    let mut out: Vec<_> = cells
        .into_iter()
        .map(|((a, f), (n, m, x))| (a, f, n, m, x))
        .collect();
    out.sort_unstable();
    out
}

/// Builds with two aspects, by fragment count (up to [`MIN_FRAGMENTS`]):
/// how many have at least the usual (modal) number of fragments for their
/// aspect pair, how many fewer, and the smallest usual count of any pair:
/// `(fragments, at least usual, fewer, smallest usual)`.
pub fn fragment_gaps(db: &Database) -> Result<Vec<(u32, usize, usize, u32)>> {
    let mut stmt = db.conn().prepare(&format!(
        "WITH p AS (
             SELECT build_id,
                    list_sort(list(component_hash) FILTER (WHERE kind = 'aspect')) AS pair,
                    count(*) FILTER (WHERE kind = 'fragment') AS fragments
             FROM build_component GROUP BY build_id),
         m AS (SELECT pair, mode(fragments) AS usual FROM p WHERE len(pair) = 2 GROUP BY pair)
         SELECT p.fragments,
                count(*) FILTER (WHERE p.fragments >= m.usual),
                count(*) FILTER (WHERE p.fragments < m.usual),
                (SELECT min(usual) FROM m)
         FROM p JOIN m USING (pair)
         WHERE p.fragments <= {MIN_FRAGMENTS}
         GROUP BY 1 ORDER BY 1"
    ))?;
    let rows = stmt.query_map([], |r| {
        let n = |i: usize| r.get::<_, i64>(i).map(|v| usize::try_from(v).unwrap_or(0));
        Ok((
            u32::try_from(r.get::<_, i64>(0)?).unwrap_or(0),
            n(1)?,
            n(2)?,
            u32::try_from(r.get::<_, i64>(3)?).unwrap_or(0),
        ))
    })?;
    Ok(rows.collect::<duckdb::Result<_>>()?)
}

/// How much evidence a group of builds carries.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Evidence {
    pub builds: usize,
    /// Of those, used in a scored run.
    pub used: usize,
    /// Of the used: by one player, in one run.
    pub one_player: usize,
    pub one_run: usize,
    pub most_players: usize,
    pub runs: usize,
}

/// Evidence of complete and incomplete builds: `(complete, incomplete)`.
pub fn evidence<S: BuildHasher>(
    db: &Database,
    parts: &HashMap<i64, Parts, S>,
) -> Result<(Evidence, Evidence)> {
    let mut stmt = db.conn().prepare(
        "SELECT rb.build_id, count(DISTINCT (r.instance_id, r.membership_id)), count(DISTINCT r.membership_id)
         FROM run_build rb JOIN run_score r USING (instance_id, membership_id)
         GROUP BY 1",
    )?;
    let mut usage: HashMap<i64, (usize, usize)> = HashMap::new();
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let n = |i: usize| r.get::<_, i64>(i).map(|v| usize::try_from(v).unwrap_or(0));
        usage.insert(r.get(0)?, (n(1)?, n(2)?));
    }
    let (mut complete, mut incomplete) = (Evidence::default(), Evidence::default());
    for (id, p) in parts {
        let e = if p.complete() {
            &mut complete
        } else {
            &mut incomplete
        };
        e.builds += 1;
        if let Some(&(runs, players)) = usage.get(id) {
            e.used += 1;
            e.runs += runs;
            e.one_player += usize::from(players <= 1);
            e.one_run += usize::from(runs <= 1);
            e.most_players = e.most_players.max(players);
        }
    }
    Ok((complete, incomplete))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_complete_build_has_its_whole_kit_and_mods() {
        let full = Parts {
            aspects: 2,
            fragments: 4,
            abilities: 4,
            armor_mods: 12,
            artifact_perks: 7,
        };
        assert!(full.complete());
        assert!(
            !Parts {
                aspects: 0,
                fragments: 0,
                ..full
            }
            .kit_complete()
        );
        assert!(
            !Parts {
                fragments: 3,
                ..full
            }
            .kit_complete()
        );
        assert!(
            !Parts {
                armor_mods: 0,
                ..full
            }
            .complete()
        );
        assert!(
            Parts {
                artifact_perks: 0,
                ..full
            }
            .complete(),
            "artifact perks are seasonal"
        );
    }

    #[test]
    fn parts_are_counted_per_build() {
        let (_dir, db) = crate::site::tests::db();
        let parts = parts(&db).unwrap();
        // The fixture's build 10 has one aspect and one fragment.
        let p = parts[&10];
        assert_eq!((p.aspects, p.fragments, p.armor_mods), (1, 1, 1));
        assert!(!p.kit_complete());
        assert_eq!(
            distribution(&parts).iter().map(|c| c.2).sum::<usize>(),
            parts.len()
        );
    }
}
