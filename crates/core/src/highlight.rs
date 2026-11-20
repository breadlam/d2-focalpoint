//! Choosing a few builds that are both well ranked and unlike each other.
//!
//! [`select_diverse`] is the greedy MAP of a determinantal point process
//! (DPP) whose kernel is `L = diag(q) · S · diag(q)`: `q_i = exp(w · quality_i)`
//! and `S` a similarity with ones on the diagonal ([`Playstyle::similarity`]).
//! Each step adds the candidate with the largest
//!
//! ```text
//! 2 · w · quality_i + ln(novelty_i)
//! ```
//!
//! where `novelty_i` is the part of candidate `i` not explained by the picks
//! so far *together* (its squared distance, in the kernel's feature space,
//! from their span): 1 for a candidate unlike every pick, 0 for a duplicate.
//! There are no thresholds: when the candidates are all alike the picks are
//! the best of them, less spread; when they differ, a slightly lower-ranked
//! build that adds something new beats a near-copy of a pick. The greedy
//! order is kept, so the first `k` picks are the best set of `k` this
//! method finds for every `k`.
//!
//! The quality weight `w` is the exchange rate: halving a candidate's
//! novelty costs as much as lowering its quality by `ln 2 / (2w)`.

use crate::vector::{ElementKind, ElementVector};

/// How much each kind of element says about how a build plays; zero kinds
/// are not part of the playstyle. Seasonal artifact perks, legendary weapons
/// and their rolls, mods and stats are left out: they vary between nearly
/// identical builds.
#[must_use]
pub fn playstyle_weight(kind: ElementKind) -> f64 {
    use ElementKind as K;
    match kind {
        K::ExoticArmor => 4.0,
        K::ExoticWeapon | K::Subclass | K::Super => 3.0,
        K::Aspect => 2.0,
        K::ExoticArmorPerk => 1.5,
        K::Grenade | K::Melee | K::ClassAbility | K::Movement => 1.0,
        K::Fragment => 0.5,
        _ => 0.0,
    }
}

/// A build's playstyle: its defining elements, weighted by
/// [`playstyle_weight`], sorted by `(kind, hash)`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Playstyle {
    entries: Vec<(ElementKind, u32, f64)>,
}

impl Playstyle {
    #[must_use]
    pub fn of(v: &ElementVector) -> Self {
        Self {
            entries: v
                .entries()
                .iter()
                .filter_map(|e| {
                    let w = playstyle_weight(e.kind) * f64::from(e.count);
                    (w > 0.0).then_some((e.kind, e.hash, w))
                })
                .collect(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Weighted Jaccard (Ružička) similarity: the weight both builds share
    /// over the weight either has. 1 for the same playstyle, 0 for nothing
    /// in common (or no playstyle elements). It is a positive semi-definite
    /// kernel (the min-max kernel), as [`select_diverse`] needs.
    #[must_use]
    pub fn similarity(&self, other: &Self) -> f64 {
        let (mine, theirs) = (&self.entries, &other.entries);
        let (mut i, mut j) = (0, 0);
        let (mut shared, mut either) = (0.0, 0.0);
        while i < mine.len() || j < theirs.len() {
            let order = match (mine.get(i), theirs.get(j)) {
                (Some(x), Some(y)) => (x.0, x.1).cmp(&(y.0, y.1)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, _) => std::cmp::Ordering::Greater,
            };
            match order {
                std::cmp::Ordering::Less => {
                    either += mine[i].2;
                    i += 1;
                }
                std::cmp::Ordering::Greater => {
                    either += theirs[j].2;
                    j += 1;
                }
                std::cmp::Ordering::Equal => {
                    shared += mine[i].2.min(theirs[j].2);
                    either += mine[i].2.max(theirs[j].2);
                    i += 1;
                    j += 1;
                }
            }
        }
        if either > 0.0 { shared / either } else { 0.0 }
    }
}

/// One pick of [`select_diverse`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pick {
    /// Index into the candidates.
    pub index: usize,
    /// The part of the candidate the earlier picks did not already cover,
    /// when it was picked: 1 for the first pick.
    pub novelty: f64,
}

/// Below this novelty a candidate duplicates the picks and is never chosen.
const MIN_NOVELTY: f64 = 1e-6;

/// Pick up to `k` candidates that are well ranked and unlike each other (see
/// the module docs). `quality[i]` is candidate `i`'s ranking on a log scale
/// (an effect); `similarity(i, j)` must be a positive semi-definite kernel
/// with `similarity(i, i) == 1`. Ties go to the lower index, so candidates
/// sorted best-first break ties by rank.
#[must_use]
pub fn select_diverse(
    quality: &[f64],
    similarity: impl Fn(usize, usize) -> f64,
    k: usize,
    quality_weight: f64,
) -> Vec<Pick> {
    let n = quality.len();
    // Incremental Cholesky of the similarity kernel restricted to the picks
    // (Chen, Zhang & Zhou 2018): `basis[i]` holds candidate i's coordinates
    // along the picks' orthogonalised directions, `novelty[i]` what is left.
    let mut basis: Vec<Vec<f64>> = vec![Vec::with_capacity(k); n];
    let mut novelty = vec![1.0_f64; n];
    let mut taken = vec![false; n];
    let mut picks = Vec::with_capacity(k.min(n));
    let gain = |i: usize, novelty: &[f64]| 2.0 * quality_weight * quality[i] + novelty[i].ln();
    while picks.len() < k {
        let Some(j) = (0..n)
            .filter(|&i| !taken[i] && novelty[i] > MIN_NOVELTY)
            .max_by(|&a, &b| {
                gain(a, &novelty)
                    .total_cmp(&gain(b, &novelty))
                    .then(b.cmp(&a))
            })
        else {
            break;
        };
        taken[j] = true;
        picks.push(Pick {
            index: j,
            novelty: novelty[j],
        });
        let norm = novelty[j].sqrt();
        let pivot = std::mem::take(&mut basis[j]);
        for i in (0..n).filter(|&i| !taken[i]) {
            let along: f64 = pivot.iter().zip(&basis[i]).map(|(a, b)| a * b).sum();
            let e = (similarity(j, i) - along) / norm;
            basis[i].push(e);
            novelty[i] = (novelty[i] - e * e).max(0.0);
        }
        basis[j] = pivot;
    }
    picks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::ElementKind as K;

    fn style(elements: &[(ElementKind, u32)]) -> Playstyle {
        Playstyle::of(&ElementVector::from_counts(
            elements.iter().map(|&(k, h)| (k, h, 1)),
        ))
    }

    #[test]
    fn similarity_weighs_what_defines_a_playstyle() {
        let a = style(&[
            (K::ExoticArmor, 1),
            (K::Subclass, 10),
            (K::Super, 11),
            (K::Aspect, 12),
            (K::ArtifactPerk, 99),
        ]);
        assert!((a.similarity(&a) - 1.0).abs() < 1e-12);
        // Artifact perks are not part of the playstyle.
        let b = style(&[
            (K::ExoticArmor, 1),
            (K::Subclass, 10),
            (K::Super, 11),
            (K::Aspect, 12),
        ]);
        assert!((a.similarity(&b) - 1.0).abs() < 1e-12);
        // Another exotic: 3 + 3 + 2 shared of 8 + 4 + 4.
        let c = style(&[
            (K::ExoticArmor, 2),
            (K::Subclass, 10),
            (K::Super, 11),
            (K::Aspect, 12),
        ]);
        assert!((a.similarity(&c) - 8.0 / 16.0).abs() < 1e-12);
        let d = style(&[(K::ExoticArmor, 3), (K::Subclass, 20)]);
        assert!(a.similarity(&d).abs() < 1e-12);
        assert!(Playstyle::default().similarity(&a).abs() < 1e-12);
    }

    /// Candidates 0–1 are near-copies, 2 is unlike both, 3 duplicates 0.
    fn sim(i: usize, j: usize) -> f64 {
        let group = |x: usize| x == 2;
        if i == j || (i.min(j) == 0 && i.max(j) == 3) {
            1.0
        } else if group(i) == group(j) {
            0.8
        } else {
            0.0
        }
    }

    #[test]
    fn the_best_comes_first_then_what_is_new() {
        let quality = [0.20, 0.18, 0.12, 0.20];
        let picks = select_diverse(&quality, sim, 3, 5.0);
        let order: Vec<usize> = picks.iter().map(|p| p.index).collect();
        // 1 is better ranked than 2 but mostly a copy of 0.
        assert_eq!(order, [0, 2, 1]);
        assert!((picks[0].novelty - 1.0).abs() < 1e-12);
        assert!((picks[2].novelty - (1.0 - 0.8 * 0.8)).abs() < 1e-9);
    }

    #[test]
    fn duplicates_are_never_picked() {
        let picks = select_diverse(&[0.2, 0.18, 0.12, 0.2], sim, 10, 5.0);
        assert_eq!(picks.len(), 3, "{picks:?}");
        assert!(picks.iter().all(|p| p.index != 3));
    }

    #[test]
    fn the_quality_weight_trades_rank_for_variety() {
        let quality = [0.20, 0.18, 0.12];
        let ranked = select_diverse(&quality, sim, 2, 100.0);
        assert_eq!(
            ranked[1].index, 1,
            "a heavy quality weight follows the ranking"
        );
        let varied = select_diverse(&quality, sim, 2, 0.5);
        assert_eq!(varied[1].index, 2);
    }

    #[test]
    fn without_similarity_the_ranking_is_kept() {
        let quality = [0.1, 0.3, -0.2, 0.25];
        let picks = select_diverse(&quality, |i, j| f64::from(u8::from(i == j)), 4, 1.0);
        let order: Vec<usize> = picks.iter().map(|p| p.index).collect();
        assert_eq!(order, [1, 3, 0, 2]);
    }

    #[test]
    fn fewer_picks_are_a_prefix_of_more() {
        let quality = [0.20, 0.18, 0.12, 0.05];
        let three = select_diverse(&quality, sim, 3, 3.0);
        let two = select_diverse(&quality, sim, 2, 3.0);
        assert_eq!(two, three[..2]);
    }
}
