//! Element count vectors and the tiered build distance.
//!
//! Conceptually a build is a vector with one coordinate per possible element
//! (every exotic, super, aspect, fragment, perk, mod, stat investment, ...)
//! holding how many of that element the build has. That vector is ~99 %
//! zeros, so an [`ElementVector`] stores only the non-zero coordinates,
//! sorted, keyed by `(element kind, hash)`; a missing key means zero. Hashes
//! are Bungie's stable identifiers, so new content needs no vocabulary or
//! migration, and every operation on the dense vector is computed exactly by
//! merging two sorted sparse lists.
//!
//! Placement does not matter: an element counts the same wherever it is
//! socketed (fragments are unordered; a perk counts the same on any weapon).
//!
//! **Tiers** (ADR 0005) rank how much an element defines a build:
//!
//! 1. exotic armor (and an exotic class item's rolled perks), exotic weapon;
//! 2. subclass, super, abilities, aspects, fragments, artifact perks;
//! 3. armor stat investment ([`crate::stats`]), set bonuses, armor mods;
//! 4. legendary weapons, weapon elements, frames, perks/traits, origin
//!    traits, weapon mods.
//!
//! Legendary armor pieces, archetypes, stat and tuning mods matter only
//! through the stats they produce, which stat investment captures; they, the
//! artifact item, Transcendence and armor-set membership are kept in the
//! vector but carry no weight.
//!
//! **Distance.** [`ElementVector::tiered_distance`] gives each tier a fixed
//! share of the total — `Σₜ shareₜ · Jₜ` with `Jₜ` the Jaccard distance
//! `1 − Σmin/Σmax` over that tier's counts — so tiers with many elements
//! (perks, artifact perks) cannot drown out tiers with few (exotics). It is a
//! metric on the weighted elements, in `[0, 1]`. [`ElementVector::l1_distance`]
//! and [`ElementVector::slot_edits`] describe differences in absolute terms.

use std::cmp::Ordering;

use crate::build::{Build, ComponentKind};
use crate::catalog::Catalog;
use crate::slot::Slot;
use crate::stats::{ArmorStat, Investment};

/// Version of the element rules (kinds, tiers, selection). Recorded in
/// database metadata; bump when any of them change.
pub const ELEMENT_VECTOR_VERSION: u8 = 3;

/// Importance tier of an element kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tier {
    Exotics,
    Kit,
    ArmorEffects,
    Weapons,
}

impl Tier {
    pub const ALL: [Tier; 4] = [Tier::Exotics, Tier::Kit, Tier::ArmorEffects, Tier::Weapons];

    /// 1-based tier number.
    #[must_use]
    pub fn number(self) -> u8 {
        self as u8 + 1
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Exotics => "exotics",
            Tier::Kit => "kit",
            Tier::ArmorEffects => "armor effects",
            Tier::Weapons => "weapons",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// What an element is. Codes are persisted (database keys): never reuse or
/// renumber them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ElementKind {
    ExoticArmor,
    ExoticArmorPerk,
    ExoticWeapon,
    Subclass,
    Super,
    Grenade,
    Melee,
    ClassAbility,
    Movement,
    Aspect,
    Fragment,
    ArtifactPerk,
    /// `hash` is a stat hash; `count` is the investment level (1–4).
    StatInvestment,
    /// `hash` is a sandbox perk.
    SetBonus,
    ArmorMod,
    Weapon,
    /// `hash` is a `DamageType` enum value; one per equipped weapon.
    WeaponElement,
    Frame,
    WeaponPerk,
    OriginTrait,
    WeaponMod,
    LegendaryArmor,
    ArmorArchetype,
    ArmorStatMod,
    ArmorTuning,
    Artifact,
    Transcendence,
    /// `hash` is an armor set; `count` is pieces equipped.
    ArmorSet,
}

impl ElementKind {
    pub const ALL: [ElementKind; 28] = [
        ElementKind::ExoticArmor,
        ElementKind::ExoticArmorPerk,
        ElementKind::ExoticWeapon,
        ElementKind::Subclass,
        ElementKind::Super,
        ElementKind::Grenade,
        ElementKind::Melee,
        ElementKind::ClassAbility,
        ElementKind::Movement,
        ElementKind::Aspect,
        ElementKind::Fragment,
        ElementKind::ArtifactPerk,
        ElementKind::StatInvestment,
        ElementKind::SetBonus,
        ElementKind::ArmorMod,
        ElementKind::Weapon,
        ElementKind::WeaponElement,
        ElementKind::Frame,
        ElementKind::WeaponPerk,
        ElementKind::OriginTrait,
        ElementKind::WeaponMod,
        ElementKind::LegendaryArmor,
        ElementKind::ArmorArchetype,
        ElementKind::ArmorStatMod,
        ElementKind::ArmorTuning,
        ElementKind::Artifact,
        ElementKind::Transcendence,
        ElementKind::ArmorSet,
    ];

    /// The tier an element counts in, or `None` for unweighted kinds.
    #[must_use]
    pub fn tier(self) -> Option<Tier> {
        use ElementKind as K;
        Some(match self {
            K::ExoticArmor | K::ExoticArmorPerk | K::ExoticWeapon => Tier::Exotics,
            K::Subclass
            | K::Super
            | K::Grenade
            | K::Melee
            | K::ClassAbility
            | K::Movement
            | K::Aspect
            | K::Fragment
            | K::ArtifactPerk => Tier::Kit,
            K::StatInvestment | K::SetBonus | K::ArmorMod => Tier::ArmorEffects,
            K::Weapon
            | K::WeaponElement
            | K::Frame
            | K::WeaponPerk
            | K::OriginTrait
            | K::WeaponMod => Tier::Weapons,
            K::LegendaryArmor
            | K::ArmorArchetype
            | K::ArmorStatMod
            | K::ArmorTuning
            | K::Artifact
            | K::Transcendence
            | K::ArmorSet => return None,
        })
    }

    /// Stable persisted code (1-based position in [`ElementKind::ALL`]).
    #[must_use]
    pub fn code(self) -> u8 {
        self as u8 + 1
    }

    #[must_use]
    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.get(usize::from(code).checked_sub(1)?).copied()
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        use ElementKind as K;
        match self {
            K::ExoticArmor => "exotic_armor",
            K::ExoticArmorPerk => "exotic_armor_perk",
            K::ExoticWeapon => "exotic_weapon",
            K::Subclass => "subclass",
            K::Super => "super",
            K::Grenade => "grenade",
            K::Melee => "melee",
            K::ClassAbility => "class_ability",
            K::Movement => "movement",
            K::Aspect => "aspect",
            K::Fragment => "fragment",
            K::ArtifactPerk => "artifact_perk",
            K::StatInvestment => "stat_investment",
            K::SetBonus => "set_bonus",
            K::ArmorMod => "armor_mod",
            K::Weapon => "weapon",
            K::WeaponElement => "weapon_element",
            K::Frame => "frame",
            K::WeaponPerk => "weapon_perk",
            K::OriginTrait => "origin_trait",
            K::WeaponMod => "weapon_mod",
            K::LegendaryArmor => "legendary_armor",
            K::ArmorArchetype => "armor_archetype",
            K::ArmorStatMod => "armor_stat_mod",
            K::ArmorTuning => "armor_tuning",
            K::Artifact => "artifact",
            K::Transcendence => "transcendence",
            K::ArmorSet => "armor_set",
        }
    }
}

/// Persisted element key: `(kind code << 32) | hash`.
#[must_use]
pub fn element_key(kind: ElementKind, hash: u32) -> i64 {
    (i64::from(kind.code()) << 32) | i64::from(hash)
}

/// Inverse of [`element_key`].
#[must_use]
pub fn split_key(key: i64) -> Option<(ElementKind, u32)> {
    let kind = ElementKind::from_code(u8::try_from(key >> 32).ok()?)?;
    Some((kind, (key & 0xFFFF_FFFF) as u32))
}

/// One non-zero coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementCount {
    pub kind: ElementKind,
    pub hash: u32,
    pub count: u16,
}

/// A build's sparse element count vector, sorted by `(kind, hash)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ElementVector {
    entries: Vec<ElementCount>,
}

/// Share of the total distance given to each tier (sums to 1 by default).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TierWeights(pub [f64; 4]);

impl Default for TierWeights {
    /// 0.4 / 0.3 / 0.2 / 0.1 for tiers 1–4.
    fn default() -> Self {
        Self([0.4, 0.3, 0.2, 0.1])
    }
}

/// A distance with its per-tier parts (`total = Σ share · per_tier`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TieredDistance {
    pub total: f64,
    /// Jaccard distance within each tier, in [`Tier::ALL`] order.
    pub per_tier: [f64; 4],
}

impl ElementVector {
    /// Build from `(kind, hash, count)` triples; duplicates are summed and
    /// zero counts dropped.
    #[must_use]
    pub fn from_counts(counts: impl IntoIterator<Item = (ElementKind, u32, u16)>) -> Self {
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

    /// Rebuild from persisted keys (each key repeated once per count).
    #[must_use]
    pub fn from_keys(keys: impl IntoIterator<Item = i64>) -> Self {
        Self::from_counts(
            keys.into_iter()
                .filter_map(split_key)
                .map(|(k, h)| (k, h, 1)),
        )
    }

    /// Project a build onto its elements. Stat investment is added separately
    /// with [`ElementVector::with_investment`] because it needs the build's
    /// observed stats.
    #[must_use]
    pub fn from_build(b: &Build, catalog: &Catalog) -> Self {
        use ElementKind as E;
        let exotic_class_item = b
            .item_in(Slot::ClassItem)
            .filter(|&h| b.exotic_armor_hash == Some(h));
        let mut counts = Vec::with_capacity(b.components.len() + 3);
        for c in &b.components {
            let q = c.quantity.max(1);
            let kind = match c.kind {
                ComponentKind::Subclass => E::Subclass,
                ComponentKind::Super => E::Super,
                ComponentKind::Grenade => E::Grenade,
                ComponentKind::Melee => E::Melee,
                ComponentKind::ClassAbility => E::ClassAbility,
                ComponentKind::Movement => E::Movement,
                ComponentKind::Aspect => E::Aspect,
                ComponentKind::Fragment => E::Fragment,
                ComponentKind::Transcendence => E::Transcendence,
                ComponentKind::ArtifactPerk => E::ArtifactPerk,
                ComponentKind::Artifact => E::Artifact,
                ComponentKind::Weapon => {
                    if let Some(dt) = catalog
                        .item(c.hash)
                        .map(|d| d.default_damage_type)
                        .filter(|&d| d != 0)
                    {
                        counts.push((E::WeaponElement, u32::from(dt), q));
                    }
                    if b.exotic_weapon_hash == Some(c.hash) {
                        E::ExoticWeapon
                    } else {
                        E::Weapon
                    }
                }
                ComponentKind::WeaponIntrinsic => E::Frame,
                ComponentKind::WeaponPerk => E::WeaponPerk,
                ComponentKind::OriginTrait => E::OriginTrait,
                ComponentKind::WeaponMod => E::WeaponMod,
                ComponentKind::Armor => {
                    if b.exotic_armor_hash == Some(c.hash) {
                        E::ExoticArmor
                    } else {
                        E::LegendaryArmor
                    }
                }
                // Only an exotic class item's perks are rolled choices; every
                // other armor perk is fixed by its item.
                ComponentKind::ArmorPerk
                    if c.parent_hash.is_some() && c.parent_hash == exotic_class_item =>
                {
                    E::ExoticArmorPerk
                }
                ComponentKind::ArmorPerk => continue,
                ComponentKind::ArmorArchetype => E::ArmorArchetype,
                ComponentKind::ArmorMod => E::ArmorMod,
                ComponentKind::ArmorStatMod => E::ArmorStatMod,
                ComponentKind::ArmorTuning => E::ArmorTuning,
                ComponentKind::ArmorSet => E::ArmorSet,
                ComponentKind::SetBonus => E::SetBonus,
            };
            counts.push((kind, c.hash, q));
        }
        Self::from_counts(counts)
    }

    /// Add stat investment elements (one per invested stat, count = level),
    /// replacing any already present.
    #[must_use]
    pub fn with_investment(self, investment: &Investment) -> Self {
        let stats = ArmorStat::ALL.into_iter().map(|s| {
            (
                ElementKind::StatInvestment,
                s.hash(),
                u16::from(investment.level(s)),
            )
        });
        Self::from_counts(
            self.entries
                .into_iter()
                .filter(|e| e.kind != ElementKind::StatInvestment)
                .map(|e| (e.kind, e.hash, e.count))
                .chain(stats),
        )
    }

    #[must_use]
    pub fn entries(&self) -> &[ElementCount] {
        &self.entries
    }

    /// The elements of one tier as persisted keys, each repeated `count`
    /// times, sorted (the database encoding).
    #[must_use]
    pub fn tier_keys(&self, tier: Tier) -> Vec<i64> {
        let mut keys: Vec<i64> = self
            .entries
            .iter()
            .filter(|e| e.kind.tier() == Some(tier))
            .flat_map(|e| std::iter::repeat_n(element_key(e.kind, e.hash), usize::from(e.count)))
            .collect();
        keys.sort_unstable();
        keys
    }

    /// Visit every coordinate present in either vector as
    /// `(kind, count in self, count in other)`.
    fn merge(&self, other: &Self, mut visit: impl FnMut(ElementKind, u16, u16)) {
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

    /// The default build distance: `Σₜ shareₜ · Jₜ` (see module docs).
    #[must_use]
    pub fn tiered_distance(&self, other: &Self, weights: &TierWeights) -> TieredDistance {
        let mut min_sum = [0u32; 4];
        let mut max_sum = [0u32; 4];
        self.merge(other, |kind, a, b| {
            if let Some(t) = kind.tier() {
                min_sum[t.index()] += u32::from(a.min(b));
                max_sum[t.index()] += u32::from(a.max(b));
            }
        });
        let per_tier: [f64; 4] = std::array::from_fn(|t| {
            if max_sum[t] == 0 {
                0.0
            } else {
                1.0 - f64::from(min_sum[t]) / f64::from(max_sum[t])
            }
        });
        let total = per_tier.iter().zip(weights.0).map(|(d, w)| d * w).sum();
        TieredDistance { total, per_tier }
    }

    /// `Σ |aᵢ − bᵢ|` over weighted (tiered) elements: elements added plus
    /// removed.
    #[must_use]
    pub fn l1_distance(&self, other: &Self) -> u32 {
        let mut d = 0;
        self.merge(other, |kind, a, b| {
            if kind.tier().is_some() {
                d += u32::from(a.abs_diff(b));
            }
        });
        d
    }

    /// How many slots changed among weighted elements: per kind,
    /// `max(|A∖B|, |B∖A|)` (a swap counts 1).
    #[must_use]
    pub fn slot_edits(&self, other: &Self) -> u32 {
        let mut per_kind: Vec<(ElementKind, u32, u32)> = Vec::new();
        self.merge(other, |kind, a, b| {
            if kind.tier().is_none() {
                return;
            }
            if per_kind.last().is_none_or(|&(last, _, _)| last != kind) {
                per_kind.push((kind, 0, 0));
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

    const K: [ElementKind; 4] = [
        ElementKind::ExoticArmor,
        ElementKind::Fragment,
        ElementKind::ArmorMod,
        ElementKind::WeaponPerk,
    ];

    fn ev(items: &[(ElementKind, u32, u16)]) -> ElementVector {
        ElementVector::from_counts(items.iter().copied())
    }

    #[test]
    fn kinds_codes_and_tiers() {
        for k in ElementKind::ALL {
            assert_eq!(ElementKind::from_code(k.code()), Some(k));
            assert_eq!(
                split_key(element_key(k, 4_000_000_000)),
                Some((k, 4_000_000_000))
            );
        }
        assert_eq!(ElementKind::from_code(0), None);
        assert_eq!(ElementKind::ExoticWeapon.tier(), Some(Tier::Exotics));
        assert_eq!(ElementKind::ArtifactPerk.tier(), Some(Tier::Kit));
        assert_eq!(ElementKind::SetBonus.tier(), Some(Tier::ArmorEffects));
        assert_eq!(ElementKind::Frame.tier(), Some(Tier::Weapons));
        assert_eq!(ElementKind::ArmorArchetype.tier(), None);
    }

    #[test]
    fn duplicates_merge_and_keys_round_trip() {
        let a = ev(&[(K[1], 5, 1), (K[1], 3, 1), (K[1], 5, 1)]);
        assert_eq!(a, ev(&[(K[1], 3, 1), (K[1], 5, 2)]));
        let keys = a.tier_keys(Tier::Kit);
        assert_eq!(keys.len(), 3);
        assert_eq!(ElementVector::from_keys(keys), a);
    }

    #[test]
    fn tier_shares_on_examples() {
        let w = TierWeights::default();
        let base = ev(&[
            (K[0], 1, 1),
            (K[1], 10, 1),
            (K[1], 11, 1),
            (K[2], 20, 1),
            (K[3], 30, 1),
        ]);
        // Swap the exotic armor only: tier 1 goes from {1} to {2}: J1 = 1.
        let swapped = ev(&[
            (K[0], 2, 1),
            (K[1], 10, 1),
            (K[1], 11, 1),
            (K[2], 20, 1),
            (K[3], 30, 1),
        ]);
        let d = base.tiered_distance(&swapped, &w);
        assert!((d.per_tier[0] - 1.0).abs() < 1e-12);
        assert!((d.total - 0.4).abs() < 1e-12);
        // A different weapon perk only moves tier 4 (share 0.1).
        let perk = ev(&[
            (K[0], 1, 1),
            (K[1], 10, 1),
            (K[1], 11, 1),
            (K[2], 20, 1),
            (K[3], 31, 1),
        ]);
        assert!((base.tiered_distance(&perk, &w).total - 0.1).abs() < 1e-12);
        // Unweighted kinds never move the distance.
        let archetype = ev(&[
            (K[0], 1, 1),
            (K[1], 10, 1),
            (K[1], 11, 1),
            (K[2], 20, 1),
            (K[3], 30, 1),
            (ElementKind::ArmorArchetype, 99, 1),
        ]);
        assert!(base.tiered_distance(&archetype, &w).total.abs() < 1e-12);
        assert_eq!(base.slot_edits(&archetype), 0);
        assert_eq!(base.l1_distance(&swapped), 2);
        assert_eq!(base.slot_edits(&swapped), 1);
    }

    #[test]
    fn investment_levels_are_counts() {
        let mut inv = Investment::default();
        inv.levels[4] = 4; // Super
        let a = ElementVector::default().with_investment(&inv);
        inv.levels[4] = 3;
        let b = ElementVector::default().with_investment(&inv);
        // Tier 3 only: min 3 / max 4.
        let d = a.tiered_distance(&b, &TierWeights::default());
        assert!((d.per_tier[2] - 0.25).abs() < 1e-12);
        // Re-applying replaces rather than adds.
        assert_eq!(b.clone().with_investment(&inv), b);
    }

    // --- Brute-force dense reference: the "vector of all possible elements" ---

    fn lcg(seed: &mut u64) -> u32 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*seed >> 33) as u32
    }

    /// Vocabulary: 3 elements of each of the 4 kinds in `K` (one per tier).
    const V: usize = 12;

    fn random(seed: &mut u64) -> ElementVector {
        let mut counts = Vec::new();
        for i in 0..V as u32 {
            let c = lcg(seed) % 3;
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

    fn dense_tiered(a: &[f64; V], b: &[f64; V], w: &TierWeights) -> f64 {
        (0..4)
            .map(|t| {
                let idx = t * 3..t * 3 + 3;
                let num: f64 = idx.clone().map(|i| a[i].min(b[i])).sum();
                let den: f64 = idx.map(|i| a[i].max(b[i])).sum();
                w.0[t] * if den == 0.0 { 0.0 } else { 1.0 - num / den }
            })
            .sum()
    }

    fn dense_l1(a: &[f64; V], b: &[f64; V]) -> f64 {
        (0..V).map(|i| (a[i] - b[i]).abs()).sum()
    }

    /// Slot edits by exhaustive alignment within each kind.
    fn dense_slot_edits(a: &ElementVector, b: &ElementVector) -> u32 {
        let items = |v: &ElementVector, k: ElementKind| -> Vec<u32> {
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
        let w = TierWeights([0.5, 0.2, 0.2, 0.1]);
        let vs: Vec<ElementVector> = (0..40).map(|_| random(&mut seed)).collect();
        for a in &vs {
            for b in &vs {
                let (da, db) = (dense(a), dense(b));
                assert!((a.tiered_distance(b, &w).total - dense_tiered(&da, &db, &w)).abs() < 1e-9);
                assert!((f64::from(a.l1_distance(b)) - dense_l1(&da, &db)).abs() < 1e-9);
                assert_eq!(a.slot_edits(b), dense_slot_edits(a, b), "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn distances_are_metrics() {
        let mut seed = 11;
        let w = TierWeights::default();
        let vs: Vec<ElementVector> = (0..40).map(|_| random(&mut seed)).collect();
        for a in &vs {
            for b in &vs {
                let (t, l, s) = (
                    a.tiered_distance(b, &w).total,
                    a.l1_distance(b),
                    a.slot_edits(b),
                );
                assert!((t - b.tiered_distance(a, &w).total).abs() < 1e-12);
                assert_eq!((l, s), (b.l1_distance(a), b.slot_edits(a)));
                assert_eq!(s == 0, a == b);
                assert!((0.0..=1.0).contains(&t));
                for c in &vs {
                    assert!(
                        a.tiered_distance(c, &w).total
                            <= t + b.tiered_distance(c, &w).total + 1e-12
                    );
                    assert!(a.l1_distance(c) <= l + b.l1_distance(c));
                    assert!(a.slot_edits(c) <= s + b.slot_edits(c));
                }
            }
        }
    }
}
