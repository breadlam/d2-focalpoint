//! Highlights by subclass: what a quota per subclass (say three per classic
//! subclass and six Prismatic for the neutral game, one per subclass for an
//! activity) would pick, and how distinct, well ranked and well evidenced
//! those picks are.
//!
//! Within a subclass the picks are chosen as everywhere else
//! ([`crate::highlight::select`]): complete exemplars only, as well ranked
//! and as unlike each other as the subclass's candidates allow.

use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;

use focalpoint_core::vector::ElementVector;

use super::diversity::{Pair, Profile};
use crate::highlight::{NEUTRAL, candidates, playstyles, select};
use crate::site::SiteData;

/// The subclasses, the classic five first.
pub const SUBCLASSES: [&str; 6] = ["Arc", "Solar", "Void", "Stasis", "Strand", "Prismatic"];

/// One pick within a subclass.
#[derive(Debug, Clone, PartialEq)]
pub struct SubclassPick {
    pub type_id: u32,
    /// Rank among the subclass's candidates, and among the class's (1 = best).
    pub rank: usize,
    pub class_rank: usize,
    pub quality: f64,
    pub players: usize,
    pub runs: f64,
    /// Activity groups the build type was used in.
    pub groups: usize,
}

/// One subclass's picks and how they differ.
#[derive(Debug, Clone, PartialEq)]
pub struct SubclassPicks {
    pub subclass: &'static str,
    pub candidates: usize,
    pub picks: Vec<SubclassPick>,
    /// Smallest kit distance (without artifact perks) and playstyle distance
    /// between two picks (1 with fewer than two picks).
    pub min_kit: f64,
    pub min_distance: f64,
    /// Different supers and aspect pairs among the picks.
    pub supers: usize,
    pub aspect_pairs: usize,
    /// Pairs of picks sharing an exotic armor or weapon.
    pub shared_exotic_pairs: usize,
}

/// Activity groups each build type was used in.
#[must_use]
pub fn groups_used(data: &SiteData) -> HashMap<u32, usize> {
    let mut out: HashMap<u32, usize> = HashMap::new();
    for scope in data.scopes.iter().filter(|s| s.kind == "group") {
        for r in &scope.rows {
            *out.entry(r.0).or_default() += 1;
        }
    }
    out
}

/// Up to `quota(subclass)` picks per subclass of `class` in `set`.
#[must_use]
pub fn by_subclass<S: BuildHasher, G: BuildHasher>(
    data: &SiteData,
    vectors: &HashMap<i64, ElementVector, S>,
    groups: &HashMap<u32, usize, G>,
    set: &str,
    class: &str,
    quota: &dyn Fn(&str) -> usize,
    quality_weight: f64,
) -> Vec<SubclassPicks> {
    let all = candidates(data, set, class);
    let scope_id = if set == NEUTRAL { "all" } else { set };
    let Some(scope) = data.scopes.iter().find(|s| s.id == scope_id) else {
        return Vec::new();
    };
    SUBCLASSES
        .iter()
        .map(|&subclass| {
            let ranks: Vec<usize> = (0..all.len())
                .filter(|&i| {
                    data.types[all[i].type_id as usize].element.as_deref() == Some(subclass)
                })
                .collect();
            let cands: Vec<_> = ranks.iter().map(|&i| all[i].clone()).collect();
            let styles = playstyles(&cands, vectors);
            let chosen: Vec<usize> = select(&cands, &styles, quota(subclass), quality_weight)
                .iter()
                .map(|p| p.index)
                .collect();
            let profiles: Vec<Profile> = chosen
                .iter()
                .map(|&i| {
                    vectors
                        .get(&cands[i].build_id)
                        .map_or_else(|| Profile::of(&ElementVector::default()), Profile::of)
                })
                .collect();
            let (mut min_kit, mut min_distance, mut shared) = (1.0_f64, 1.0_f64, 0);
            for a in 0..chosen.len() {
                for b in a + 1..chosen.len() {
                    let p = Pair::of(&profiles[a], &profiles[b]);
                    min_kit = min_kit.min(p.kit);
                    shared += usize::from(p.shares_exotic);
                    min_distance =
                        min_distance.min(1.0 - styles[chosen[a]].similarity(&styles[chosen[b]]));
                }
            }
            let types: Vec<_> = chosen
                .iter()
                .map(|&i| &data.types[cands[i].type_id as usize])
                .collect();
            let supers: HashSet<_> = types.iter().map(|t| t.super_name.clone()).collect();
            let pairs: HashSet<_> = types
                .iter()
                .map(|t| {
                    let mut a = t.aspects.clone();
                    a.sort();
                    a
                })
                .collect();
            SubclassPicks {
                subclass,
                candidates: cands.len(),
                picks: chosen
                    .iter()
                    .map(|&i| {
                        let c = &cands[i];
                        let r = &scope.rows[c.row];
                        SubclassPick {
                            type_id: c.type_id,
                            rank: i + 1,
                            class_rank: ranks[i] + 1,
                            quality: c.quality,
                            players: r.4,
                            runs: r.5,
                            groups: groups.get(&c.type_id).copied().unwrap_or(0),
                        }
                    })
                    .collect(),
                min_kit,
                min_distance,
                supers: supers.len(),
                aspect_pairs: pairs.len(),
                shared_exotic_pairs: shared,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::{SiteOptions, build_vectors, site_data};

    #[test]
    fn picks_stay_within_their_subclass_and_quota() {
        let (_d, db) = crate::site::tests::db();
        let mut data = site_data(&db, &SiteOptions::default(), |_| {}).unwrap();
        data.incomplete.clear();
        let vectors = build_vectors(&db).unwrap();
        let groups = groups_used(&data);
        let picks = by_subclass(&data, &vectors, &groups, "all", "Warlock", &|_| 1, 2.0);
        assert_eq!(picks.len(), SUBCLASSES.len());
        // The fixture's two build types are both Solar.
        let solar = picks.iter().find(|p| p.subclass == "Solar").unwrap();
        assert_eq!((solar.candidates, solar.picks.len()), (2, 1));
        assert_eq!(solar.picks[0].rank, 1);
        assert!(
            picks
                .iter()
                .filter(|p| p.subclass != "Solar")
                .all(|p| p.picks.is_empty())
        );
    }
}
