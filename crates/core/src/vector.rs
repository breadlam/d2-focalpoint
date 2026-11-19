//! Element count vectors: a build as a sparse vector of element counts.
//!
//! Conceptually a build is a vector with one coordinate per possible game
//! element (every super, aspect, fragment, weapon, perk, armor piece, mod,
//! ...) holding how many of that element the build has. That vector is ~99 %
//! zeros, so an [`ElementVector`] stores only the non-zero coordinates,
//! sorted, keyed by item hash. A missing key means a count of zero. Item
//! hashes are Bungie's stable, open-ended identifiers, so new game content
//! needs no vocabulary, re-indexing or migration, and every operation on the
//! dense vector is computed exactly by merging two sorted sparse lists.
//!
//! Placement does not matter: an element counts the same wherever it is
//! socketed (fragments are unordered; a perk counts the same on any weapon).
//!
//! **Which elements count** (see `docs/adr/0004-element-count-vectors.md`):
//! every build component that is a player *choice*. Components fully
//! determined by another component are excluded so no choice counts twice:
//! weapon frame intrinsics (by the weapon), fixed exotic armor perks (by the
//! exotic; exotic class item rolls *are* counted), Transcendence (by the
//! subclass), and armor sets / set bonuses (by the armor pieces).
//!
//! **Distances** (all exact on the sparse form):
//!
//! * [`ElementVector::jaccard_distance`] — weighted Jaccard / Ruzicka distance
//!   `1 − Σ wᵢ·min(aᵢ, bᵢ) / Σ wᵢ·max(aᵢ, bᵢ)`, in `[0, 1]`; the default for
//!   clustering because it is comparable across builds of different sizes;
//! * [`ElementVector::l1_distance`] — `Σ wᵢ·|aᵢ − bᵢ|`, elements added plus
//!   removed (a swap counts 2);
//! * [`ElementVector::slot_edits`] — how many slots changed: per element kind,
//!   `max(|A∖B|, |B∖A|)` (a swap counts 1), for human-readable output.
//!
//! All three are metrics (symmetric, zero only for equal vectors, triangle
//! inequality); tests check them against a brute-force dense implementation.

use std::cmp::Ordering;

use crate::build::{Build, ComponentKind};
use crate::slot::Slot;

/// Version of the element-selection rules. Recorded in database metadata;
/// bump when [`ElementVector::from_build`] changes.
pub const ELEMENT_VECTOR_VERSION: u8 = 1;

/// One non-zero coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementCount {
    pub kind: ComponentKind,
    pub hash: u32,
    pub count: u16,
}

/// A build's sparse element count vector, sorted by `(kind, hash)`.
///
/// Every item hash belongs to exactly one kind (verified on the real data),
/// so ordering by kind first keeps each kind's elements contiguous for
/// per-kind work without changing the key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ElementVector {
    entries: Vec<ElementCount>,
}

/// Per-kind weights for the weighted distances (default: every kind 1.0).
#[derive(Debug, Clone, PartialEq)]
pub struct Weights {
    per_kind: Vec<(ComponentKind, f64)>,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            per_kind: ComponentKind::ALL.iter().map(|&k| (k, 1.0)).collect(),
        }
    }
}

impl Weights {
    /// Set the weight of one kind (must be finite and non-negative).
    #[must_use]
    pub fn with(mut self, kind: ComponentKind, weight: f64) -> Self {
        assert!(
            weight.is_finite() && weight >= 0.0,
            "weights must be finite and non-negative"
        );
        if let Some(w) = self.per_kind.iter_mut().find(|(k, _)| *k == kind) {
            w.1 = weight;
        }
        self
    }

    #[must_use]
    pub fn of(&self, kind: ComponentKind) -> f64 {
        self.per_kind
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(1.0, |&(_, w)| w)
    }
}

/// Is this component kind counted as an element (a player choice)?
/// `ArmorPerk` is decided per component in [`ElementVector::from_build`].
#[must_use]
pub fn is_element_kind(kind: ComponentKind) -> bool {
    !matches!(
        kind,
        ComponentKind::WeaponIntrinsic
            | ComponentKind::Transcendence
            | ComponentKind::ArmorSet
            | ComponentKind::SetBonus
    )
}

impl ElementVector {
    /// Build from `(kind, hash, count)` triples; duplicates are summed and
    /// zero counts dropped.
    #[must_use]
    pub fn from_counts(counts: impl IntoIterator<Item = (ComponentKind, u32, u16)>) -> Self {
        let mut entries: Vec<ElementCount> = counts
            .into_iter()
            .filter(|&(_, _, c)| c > 0)
            .map(|(kind, hash, count)| ElementCount { kind, hash, count })
            .collect();
        entries.sort_unstable_by_key(|e| (e.kind, e.hash));
        entries.dedup_by(|later, kept| {
            if (later.kind, later.hash) == (kept.kind, kept.hash) {
                kept.count = kept.count.saturating_add(later.count);
                true
            } else {
                false
            }
        });
        Self { entries }
    }

    /// Project a build onto its element counts (see module docs for the
    /// selection rules). Placement is ignored: the same perk on two weapons
    /// is one element with count 2.
    #[must_use]
    pub fn from_build(b: &Build) -> Self {
        let exotic_class_item = b
            .item_in(Slot::ClassItem)
            .filter(|&h| b.exotic_armor_hash == Some(h));
        Self::from_counts(
            b.components
                .iter()
                .filter(|c| match c.kind {
                    // Rolled exotic class item perks are choices; every other
                    // armor perk is fixed by its item.
                    ComponentKind::ArmorPerk => {
                        c.parent_hash.is_some() && c.parent_hash == exotic_class_item
                    }
                    k => is_element_kind(k),
                })
                .map(|c| (c.kind, c.hash, c.quantity.max(1))),
        )
    }

    #[must_use]
    pub fn entries(&self) -> &[ElementCount] {
        &self.entries
    }

    /// Total number of elements (the vector's L1 norm).
    #[must_use]
    pub fn total(&self) -> u32 {
        self.entries.iter().map(|e| u32::from(e.count)).sum()
    }

    /// The vector as a sorted multiset of hashes (each hash repeated `count`
    /// times) — the database encoding (`build.element_hashes`).
    #[must_use]
    pub fn multiset(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self
            .entries
            .iter()
            .flat_map(|e| std::iter::repeat_n(e.hash, usize::from(e.count)))
            .collect();
        v.sort_unstable();
        v
    }

    /// Visit every coordinate present in either vector as
    /// `(kind, count in self, count in other)`.
    fn merge(&self, other: &Self, mut visit: impl FnMut(ComponentKind, u16, u16)) {
        let (left, right) = (&self.entries, &other.entries);
        let (mut li, mut ri) = (0, 0);
        while li < left.len() || ri < right.len() {
            let order = match (left.get(li), right.get(ri)) {
                (Some(l), Some(r)) => (l.kind, l.hash).cmp(&(r.kind, r.hash)),
                (Some(_), None) => Ordering::Less,
                (None, _) => Ordering::Greater,
            };
            match order {
                Ordering::Less => {
                    visit(left[li].kind, left[li].count, 0);
                    li += 1;
                }
                Ordering::Greater => {
                    visit(right[ri].kind, 0, right[ri].count);
                    ri += 1;
                }
                Ordering::Equal => {
                    visit(left[li].kind, left[li].count, right[ri].count);
                    li += 1;
                    ri += 1;
                }
            }
        }
    }

    /// Weighted L1 distance `Σ wᵢ·|aᵢ − bᵢ|`.
    #[must_use]
    pub fn l1_distance(&self, other: &Self, weights: &Weights) -> f64 {
        let mut d = 0.0;
        self.merge(other, |k, a, b| {
            d += weights.of(k) * f64::from(a.abs_diff(b));
        });
        d
    }

    /// Weighted Jaccard (Ruzicka) distance `1 − Σ wᵢ·min / Σ wᵢ·max`, in
    /// `[0, 1]`; 0 for two empty vectors.
    #[must_use]
    pub fn jaccard_distance(&self, other: &Self, weights: &Weights) -> f64 {
        let (mut num, mut den) = (0.0, 0.0);
        self.merge(other, |k, a, b| {
            let w = weights.of(k);
            num += w * f64::from(a.min(b));
            den += w * f64::from(a.max(b));
        });
        if den == 0.0 { 0.0 } else { 1.0 - num / den }
    }

    /// Number of slots that changed: per element kind, `max(|A∖B|, |B∖A|)`.
    #[must_use]
    pub fn slot_edits(&self, other: &Self) -> u32 {
        // Per kind: (elements only in self, elements only in other).
        let mut per_kind: Vec<(ComponentKind, u32, u32)> = Vec::new();
        self.merge(other, |k, a, b| {
            if per_kind.last().is_none_or(|&(last, _, _)| last != k) {
                per_kind.push((k, 0, 0));
            }
            let e = per_kind.last_mut().expect("just pushed");
            e.1 += u32::from(a.saturating_sub(b));
            e.2 += u32::from(b.saturating_sub(a));
        });
        per_kind.iter().map(|&(_, x, y)| x.max(y)).sum()
    }
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)] // math-style names in tests
mod tests {
    use super::*;

    const K: [ComponentKind; 3] = [
        ComponentKind::Aspect,
        ComponentKind::Fragment,
        ComponentKind::WeaponPerk,
    ];

    fn ev(items: &[(ComponentKind, u32, u16)]) -> ElementVector {
        ElementVector::from_counts(items.iter().copied())
    }

    #[test]
    fn duplicates_merge_and_order_is_irrelevant() {
        let a = ev(&[(K[1], 5, 1), (K[1], 3, 1), (K[1], 5, 1)]);
        let b = ev(&[(K[1], 3, 1), (K[1], 5, 2)]);
        assert_eq!(a, b);
        assert_eq!(a.total(), 3);
        assert_eq!(a.multiset(), vec![3, 5, 5]);
    }

    #[test]
    fn distances_on_small_examples() {
        let w = Weights::default();
        // One aspect swapped.
        let a = ev(&[(K[0], 1, 1), (K[0], 2, 1)]);
        let b = ev(&[(K[0], 2, 1), (K[0], 3, 1)]);
        assert_eq!(a.slot_edits(&b), 1);
        assert!((a.l1_distance(&b, &w) - 2.0).abs() < 1e-12);
        assert!((a.jaccard_distance(&b, &w) - 2.0 / 3.0).abs() < 1e-12);
        // An extra fragment and a swapped one: L1 3, slot edits 2.
        let c = ev(&[(K[1], 1, 1), (K[1], 2, 1)]);
        let d = ev(&[(K[1], 1, 1), (K[1], 3, 1), (K[1], 4, 1)]);
        assert_eq!(c.slot_edits(&d), 2);
        assert!((c.l1_distance(&d, &w) - 3.0).abs() < 1e-12);
        // Slot edits never align an aspect with a fragment.
        let e = ev(&[(K[0], 1, 1)]);
        let f = ev(&[(K[1], 2, 1)]);
        assert_eq!(e.slot_edits(&f), 2);
        // Same perk on two weapons is a count, compared as such.
        let g = ev(&[(K[2], 9, 2)]);
        let h = ev(&[(K[2], 9, 1)]);
        assert_eq!(g.slot_edits(&h), 1);
        assert!((g.jaccard_distance(&h, &w) - 0.5).abs() < 1e-12);
        // Empty vectors.
        assert!(
            ElementVector::default()
                .jaccard_distance(&ElementVector::default(), &w)
                .abs()
                < 1e-12
        );
    }

    #[test]
    fn weights_scale_kinds() {
        let a = ev(&[(K[0], 1, 1), (K[2], 7, 1)]);
        let b = ev(&[(K[0], 1, 1), (K[2], 8, 1)]);
        let half_perks = Weights::default().with(ComponentKind::WeaponPerk, 0.5);
        assert!((a.l1_distance(&b, &half_perks) - 1.0).abs() < 1e-12);
        // num = 1 (shared aspect), den = 1 + 0.5 + 0.5.
        assert!((a.jaccard_distance(&b, &half_perks) - 0.5).abs() < 1e-12);
    }

    // --- Brute-force dense reference: the "vector of all possible elements" ---

    fn lcg(seed: &mut u64) -> u32 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*seed >> 33) as u32
    }

    /// Vocabulary size of the random test vectors (3 elements per kind).
    const V: usize = 9;

    /// Random vector over a tiny vocabulary so vectors overlap often (and the
    /// brute-force reference stays cheap). Element `i` has kind `K[i / 3]`.
    fn random(seed: &mut u64) -> ElementVector {
        let mut counts = Vec::new();
        for i in 0..V as u32 {
            let c = lcg(seed) % 3; // 0, 1 or 2
            if c > 0 && lcg(seed).is_multiple_of(2) {
                counts.push((K[(i / 3) as usize], i, c as u16));
            }
        }
        ElementVector::from_counts(counts)
    }

    fn dense(v: &ElementVector) -> [f64; V] {
        let mut d = [0.0; V];
        for e in v.entries() {
            d[e.hash as usize] = f64::from(e.count);
        }
        d
    }

    fn dense_l1(a: &[f64; V], b: &[f64; V], w: &[f64; V]) -> f64 {
        (0..V).map(|i| w[i] * (a[i] - b[i]).abs()).sum()
    }

    fn dense_jaccard(a: &[f64; V], b: &[f64; V], w: &[f64; V]) -> f64 {
        let num: f64 = (0..V).map(|i| w[i] * a[i].min(b[i])).sum();
        let den: f64 = (0..V).map(|i| w[i] * a[i].max(b[i])).sum();
        if den == 0.0 { 0.0 } else { 1.0 - num / den }
    }

    /// Slot edits by brute force: for each kind, the minimum number of slot
    /// changes (positional Hamming distance under the best alignment), found
    /// by trying every arrangement of the longer side against the shorter
    /// side padded with empty slots.
    fn dense_slot_edits(a: &ElementVector, b: &ElementVector) -> u32 {
        let items = |v: &ElementVector, k: ComponentKind| -> Vec<u32> {
            v.entries()
                .iter()
                .filter(|e| e.kind == k)
                .flat_map(|e| std::iter::repeat_n(e.hash, usize::from(e.count)))
                .collect()
        };
        K.iter()
            .map(|&k| {
                let (mut x, mut y) = (items(a, k), items(b, k));
                if x.len() < y.len() {
                    std::mem::swap(&mut x, &mut y);
                }
                if x.is_empty() {
                    return 0;
                }
                y.resize(x.len(), u32::MAX);
                let mut best = u32::MAX;
                permute(&mut x, 0, &mut |p| {
                    best = best.min(p.iter().zip(&y).filter(|(i, j)| i != j).count() as u32);
                });
                best
            })
            .sum()
    }

    fn permute(v: &mut Vec<u32>, k: usize, f: &mut impl FnMut(&[u32])) {
        if k == v.len() {
            f(v);
            return;
        }
        for i in k..v.len() {
            v.swap(k, i);
            permute(v, k + 1, f);
            v.swap(k, i);
        }
    }

    #[test]
    fn sparse_distances_equal_the_dense_count_vector() {
        let mut seed = 7;
        let weights = Weights::default().with(K[2], 0.25).with(K[0], 2.0);
        let w: [f64; V] = std::array::from_fn(|i| weights.of(K[i / 3]));
        let vs: Vec<ElementVector> = (0..40).map(|_| random(&mut seed)).collect();
        for a in &vs {
            for b in &vs {
                let (da, db) = (dense(a), dense(b));
                assert!((a.l1_distance(b, &weights) - dense_l1(&da, &db, &w)).abs() < 1e-9);
                assert!(
                    (a.jaccard_distance(b, &weights) - dense_jaccard(&da, &db, &w)).abs() < 1e-9
                );
                assert_eq!(a.slot_edits(b), dense_slot_edits(a, b), "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn distances_are_metrics() {
        let mut seed = 11;
        let w = Weights::default().with(K[1], 0.5);
        let vs: Vec<ElementVector> = (0..40).map(|_| random(&mut seed)).collect();
        for a in &vs {
            for b in &vs {
                let (j, l, s) = (
                    a.jaccard_distance(b, &w),
                    a.l1_distance(b, &w),
                    a.slot_edits(b),
                );
                assert!((j - b.jaccard_distance(a, &w)).abs() < 1e-12);
                assert!((l - b.l1_distance(a, &w)).abs() < 1e-12);
                assert_eq!(s, b.slot_edits(a));
                assert_eq!(s == 0, a == b);
                for c in &vs {
                    assert!(a.jaccard_distance(c, &w) <= j + b.jaccard_distance(c, &w) + 1e-12);
                    assert!(a.l1_distance(c, &w) <= l + b.l1_distance(c, &w) + 1e-12);
                    assert!(a.slot_edits(c) <= s + b.slot_edits(c));
                }
            }
        }
    }
}
