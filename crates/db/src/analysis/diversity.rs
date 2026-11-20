//! How many well-ranked build types are mutually distinct under hard
//! thresholds — the study that led to the threshold-free highlights
//! ([`crate::highlight`], ADR 0007).
//!
//! Candidates are each scope's listings of one class with at least a given
//! number of players, best-ranked first. For each criterion of "distinct"
//! two selections are counted: *rank-first* (walk the candidates best-ranked
//! first, keeping each one distinct from all kept so far) and *most* (a
//! greedy maximum independent set: repeatedly keep the candidate that rules
//! out the fewest others), within the whole list, its top half and its top
//! quarter. Its findings: the weighted tiered distance calls almost any two
//! builds distinct (an exotic swap alone is 0.4, and weapons and mods
//! rarely match); artifact perks, 7 of about 20 kit elements, make the same
//! subclass setup look half different; and the counts collapse for small
//! scopes.

use std::collections::HashMap;
use std::hash::BuildHasher;

use focalpoint_core::vector::{ElementKind, ElementVector, Tier, TierWeights};

use crate::highlight::candidates;
use crate::site::SiteData;

/// What the study needs of one build.
#[derive(Debug, Clone)]
pub struct Profile {
    /// Every tiered element.
    pub full: ElementVector,
    /// The same without artifact perks, which follow the season.
    pub lasting: ElementVector,
    /// Exotic armor and weapon.
    pub exotics: Vec<(ElementKind, u32)>,
}

impl Profile {
    #[must_use]
    pub fn of(v: &ElementVector) -> Self {
        let lasting = ElementVector::from_counts(
            v.entries()
                .iter()
                .filter(|e| e.kind != ElementKind::ArtifactPerk)
                .map(|e| (e.kind, e.hash, e.count)),
        );
        let exotics = v
            .entries()
            .iter()
            .filter(|e| matches!(e.kind, ElementKind::ExoticArmor | ElementKind::ExoticWeapon))
            .map(|e| (e.kind, e.hash))
            .collect();
        Self {
            full: v.clone(),
            lasting,
            exotics,
        }
    }
}

/// How two builds differ.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pair {
    /// The weighted tiered distance (`focalpoint similar`).
    pub total: f64,
    /// The same without artifact perks.
    pub total_lasting: f64,
    /// The kit tier's distance without artifact perks.
    pub kit: f64,
    pub shares_exotic: bool,
}

impl Pair {
    #[must_use]
    pub fn of(a: &Profile, b: &Profile) -> Self {
        let weights = TierWeights::default();
        let lasting = a.lasting.tiered_distance(&b.lasting, &weights);
        let kit = Tier::ALL
            .iter()
            .position(|&t| t == Tier::Kit)
            .map_or(1.0, |i| lasting.per_tier[i]);
        Self {
            total: a.full.tiered_distance(&b.full, &weights).total,
            total_lasting: lasting.total,
            kit,
            shares_exotic: a.exotics.iter().any(|x| b.exotics.contains(x)),
        }
    }
}

/// A named rule for "these two are distinct".
pub type Criterion = (&'static str, fn(&Pair) -> bool);

pub const CRITERIA: [Criterion; 6] = [
    ("weighted distance ≥ 0.9", |p| p.total >= 0.9),
    ("  … without artifact perks", |p| p.total_lasting >= 0.9),
    ("no shared exotic, kit ≥ 0.5", |p| {
        !p.shares_exotic && p.kit >= 0.5
    }),
    ("no shared exotic, kit ≥ 0.7", |p| {
        !p.shares_exotic && p.kit >= 0.7
    }),
    ("no shared exotic, kit ≥ 0.85", |p| {
        !p.shares_exotic && p.kit >= 0.85
    }),
    ("no shared exotic, kit ≥ 0.95", |p| {
        !p.shares_exotic && p.kit >= 0.95
    }),
];

/// The criterion the per-scope tally uses.
pub const CHOSEN: usize = 3;

/// Every pair of profiles.
#[must_use]
pub fn pairs(profiles: &[Profile]) -> Vec<Vec<Pair>> {
    let n = profiles.len();
    let mut out = vec![vec![Pair::default(); n]; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let p = Pair::of(&profiles[i], &profiles[j]);
            out[i][j] = p;
            out[j][i] = p;
        }
    }
    out
}

/// Quality first: best-ranked first, keep each distinct from all kept.
#[must_use]
pub fn rank_first(m: &[Vec<Pair>], pool: &[usize], ok: fn(&Pair) -> bool) -> Vec<usize> {
    let mut kept: Vec<usize> = Vec::new();
    for &i in pool {
        if kept.iter().all(|&k| ok(&m[i][k])) {
            kept.push(i);
        }
    }
    kept
}

/// Size first: keep the candidate that rules out the fewest others (ties to
/// the better-ranked), repeatedly.
#[must_use]
pub fn most_distinct(m: &[Vec<Pair>], pool: &[usize], ok: fn(&Pair) -> bool) -> Vec<usize> {
    let mut left = pool.to_vec();
    let mut kept = Vec::new();
    while !left.is_empty() {
        let pick = *left
            .iter()
            .min_by_key(|&&i| (left.iter().filter(|&&j| j != i && !ok(&m[i][j])).count(), i))
            .expect("not empty");
        kept.push(pick);
        left.retain(|&j| j != pick && ok(&m[pick][j]));
    }
    kept.sort_unstable();
    kept
}

/// The first `share` of `n` candidates.
#[must_use]
pub fn top(n: usize, share: f64) -> Vec<usize> {
    (0..((n as f64 * share).ceil() as usize).min(n)).collect()
}

/// One class's candidates with their profiles and pairs.
#[derive(Debug, Clone)]
pub struct Field {
    /// `(type, effect, players)`, best first.
    pub candidates: Vec<(u32, f64, usize)>,
    pub profiles: Vec<Profile>,
    pub pairs: Vec<Vec<Pair>>,
}

/// The candidates of `class` in scope `scope` with at least `min_players`.
#[must_use]
pub fn field<S: BuildHasher>(
    data: &SiteData,
    vectors: &HashMap<i64, ElementVector, S>,
    scope: &str,
    class: &str,
    min_players: usize,
) -> Field {
    let Some(s) = data.scopes.iter().find(|s| s.id == scope) else {
        return Field {
            candidates: Vec::new(),
            profiles: Vec::new(),
            pairs: Vec::new(),
        };
    };
    let cands: Vec<_> = candidates(data, scope, class)
        .into_iter()
        .filter(|c| s.rows[c.row].4 >= min_players)
        .collect();
    let profiles: Vec<Profile> = cands
        .iter()
        .map(|c| {
            vectors
                .get(&c.build_id)
                .map_or_else(|| Profile::of(&ElementVector::default()), Profile::of)
        })
        .collect();
    let pairs = pairs(&profiles);
    Field {
        candidates: cands
            .iter()
            .map(|c| (c.type_id, c.quality, s.rows[c.row].4))
            .collect(),
        profiles,
        pairs,
    }
}

/// Per criterion and pool (all, top half, top quarter): the rank-first and
/// most-distinct counts.
#[must_use]
pub fn counts(f: &Field) -> Vec<(&'static str, [(usize, usize); 3])> {
    let n = f.candidates.len();
    let pools = [top(n, 1.0), top(n, 0.5), top(n, 0.25)];
    CRITERIA
        .iter()
        .map(|&(name, ok)| {
            let cells = [0, 1, 2].map(|p| {
                (
                    rank_first(&f.pairs, &pools[p], ok).len(),
                    most_distinct(&f.pairs, &pools[p], ok).len(),
                )
            });
            (name, cells)
        })
        .collect()
}

/// How a scope's highlight (by [`CHOSEN`]) relates to the class's
/// all-activities set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Novelty {
    /// Kit ≥ 0.7 from every all-activities pick.
    NewPlaystyle,
    /// Close in kit to one, without sharing its exotics.
    OtherExotics,
    /// Close in kit to one and sharing an exotic with it.
    Repeat,
}

/// Classify each of `picks` (indices into `f`) against `reference` profiles.
#[must_use]
pub fn novelty(f: &Field, picks: &[usize], reference: &[Profile]) -> Vec<Novelty> {
    picks
        .iter()
        .map(|&i| {
            let close: Vec<Pair> = reference
                .iter()
                .map(|r| Pair::of(&f.profiles[i], r))
                .filter(|p| p.kit < 0.7)
                .collect();
            if close.is_empty() {
                Novelty::NewPlaystyle
            } else if close.iter().any(|p| p.shares_exotic) {
                Novelty::Repeat
            } else {
                Novelty::OtherExotics
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(total: f64, kit: f64, shares_exotic: bool) -> Pair {
        Pair {
            total,
            total_lasting: total,
            kit,
            shares_exotic,
        }
    }

    #[test]
    fn selections_respect_the_criterion() {
        // 0 and 1 conflict; 2 is distinct from both.
        let ok = pair(1.0, 1.0, false);
        let clash = pair(0.5, 0.2, true);
        let m = vec![vec![ok, clash, ok], vec![clash, ok, ok], vec![ok, ok, ok]];
        let crit = CRITERIA[CHOSEN].1;
        assert_eq!(rank_first(&m, &[0, 1, 2], crit), [0, 2]);
        assert_eq!(most_distinct(&m, &[0, 1, 2], crit).len(), 2);
        assert_eq!(top(10, 0.25), [0, 1, 2]);
    }
}
