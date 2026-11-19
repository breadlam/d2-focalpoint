//! Slot vectors: a build as a fixed set of slots, for similarity analysis.
//!
//! A [`SlotVector`] describes a character build by what occupies each slot:
//! single-item slots (subclass, super, each ability, each weapon, each armor
//! piece) and unordered multi-item slot groups (aspects, fragments, weapon
//! mods, armor mods, exotic class item perks, artifact perks).
//!
//! The distance between two vectors counts differing slots:
//!
//! * a single slot contributes 1 when its items differ (including one side
//!   being empty);
//! * a slot group contributes its *bag distance*, `max(|A∖B|, |B∖A|)` over
//!   multisets — the Hamming distance between the groups under the best
//!   alignment of their items, so order never matters and swapping one aspect
//!   costs exactly 1.
//!
//! Both terms are metrics, so the sum is a metric (symmetric, zero only for
//! equal vectors, triangle inequality), which keeps clustering well-behaved.
//! See `docs/adr/0003-slot-vectors.md`.
//!
//! Deliberately excluded: weapon perk rolls, armor archetypes and stat rolls,
//! masterworks and cosmetics (they over-split builds), fixed exotic perks
//! (determined by the item, so they would double-count), and character stats
//! and power level.

use crate::build::{Build, ComponentKind};
use crate::fingerprint::Fingerprinter;
use crate::slot::Slot;

/// Version of the vector definition. Bump whenever [`SlotVector::from_build`]
/// or the fingerprint encoding changes.
pub const VECTOR_FINGERPRINT_VERSION: u8 = 1;

/// Slots that hold at most one item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SingleSlot {
    Subclass,
    Super,
    Grenade,
    Melee,
    ClassAbility,
    Movement,
    Kinetic,
    Energy,
    Power,
    Helmet,
    Gauntlets,
    Chest,
    Legs,
    ClassItem,
}

/// Slot groups that hold an unordered multiset of items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GroupSlot {
    Aspects,
    Fragments,
    WeaponMods,
    /// General, stat and tuning mods across all armor pieces.
    ArmorMods,
    /// The rolled perks of an exotic class item (e.g. "Spirit of ...").
    ClassItemPerks,
    ArtifactPerks,
}

impl SingleSlot {
    pub const ALL: [SingleSlot; 14] = [
        SingleSlot::Subclass,
        SingleSlot::Super,
        SingleSlot::Grenade,
        SingleSlot::Melee,
        SingleSlot::ClassAbility,
        SingleSlot::Movement,
        SingleSlot::Kinetic,
        SingleSlot::Energy,
        SingleSlot::Power,
        SingleSlot::Helmet,
        SingleSlot::Gauntlets,
        SingleSlot::Chest,
        SingleSlot::Legs,
        SingleSlot::ClassItem,
    ];

    /// Stable column/label name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SingleSlot::Subclass => "subclass",
            SingleSlot::Super => "super",
            SingleSlot::Grenade => "grenade",
            SingleSlot::Melee => "melee",
            SingleSlot::ClassAbility => "class_ability",
            SingleSlot::Movement => "movement",
            SingleSlot::Kinetic => "kinetic",
            SingleSlot::Energy => "energy",
            SingleSlot::Power => "power",
            SingleSlot::Helmet => "helmet",
            SingleSlot::Gauntlets => "gauntlets",
            SingleSlot::Chest => "chest",
            SingleSlot::Legs => "legs",
            SingleSlot::ClassItem => "class_item",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

impl GroupSlot {
    pub const ALL: [GroupSlot; 6] = [
        GroupSlot::Aspects,
        GroupSlot::Fragments,
        GroupSlot::WeaponMods,
        GroupSlot::ArmorMods,
        GroupSlot::ClassItemPerks,
        GroupSlot::ArtifactPerks,
    ];

    /// Stable column/label name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            GroupSlot::Aspects => "aspects",
            GroupSlot::Fragments => "fragments",
            GroupSlot::WeaponMods => "weapon_mods",
            GroupSlot::ArmorMods => "armor_mods",
            GroupSlot::ClassItemPerks => "class_item_perks",
            GroupSlot::ArtifactPerks => "artifact_perks",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// A build as slots. Groups are kept sorted (multisets; repeats allowed).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SlotVector {
    singles: [Option<u32>; SingleSlot::ALL.len()],
    groups: [Vec<u32>; GroupSlot::ALL.len()],
}

impl SlotVector {
    /// Build a vector from explicit slot contents (groups need not be sorted).
    #[must_use]
    pub fn new(
        singles: impl IntoIterator<Item = (SingleSlot, u32)>,
        groups: impl IntoIterator<Item = (GroupSlot, Vec<u32>)>,
    ) -> Self {
        let mut v = Self::default();
        for (slot, hash) in singles {
            v.singles[slot.index()] = Some(hash);
        }
        for (slot, mut items) in groups {
            items.sort_unstable();
            v.groups[slot.index()] = items;
        }
        v
    }

    /// Project a build onto its slot vector.
    #[must_use]
    pub fn from_build(b: &Build) -> Self {
        let class_item = b.item_in(Slot::ClassItem);
        let exotic_class_item = class_item.filter(|&h| b.exotic_armor_hash == Some(h));
        let class_item_perks = exotic_class_item
            .map(|_| {
                b.components
                    .iter()
                    .filter(|c| {
                        c.kind == ComponentKind::ArmorPerk && c.slot == Some(Slot::ClassItem)
                    })
                    .map(|c| c.hash)
                    .collect()
            })
            .unwrap_or_default();
        let armor_mods = [
            ComponentKind::ArmorMod,
            ComponentKind::ArmorStatMod,
            ComponentKind::ArmorTuning,
        ]
        .into_iter()
        .flat_map(|k| b.hashes(k))
        .collect();

        let singles = [
            (SingleSlot::Subclass, b.item_in(Slot::Subclass)),
            (SingleSlot::Super, b.first(ComponentKind::Super)),
            (SingleSlot::Grenade, b.first(ComponentKind::Grenade)),
            (SingleSlot::Melee, b.first(ComponentKind::Melee)),
            (
                SingleSlot::ClassAbility,
                b.first(ComponentKind::ClassAbility),
            ),
            (SingleSlot::Movement, b.first(ComponentKind::Movement)),
            (SingleSlot::Kinetic, b.item_in(Slot::Kinetic)),
            (SingleSlot::Energy, b.item_in(Slot::Energy)),
            (SingleSlot::Power, b.item_in(Slot::Power)),
            (SingleSlot::Helmet, b.item_in(Slot::Helmet)),
            (SingleSlot::Gauntlets, b.item_in(Slot::Gauntlets)),
            (SingleSlot::Chest, b.item_in(Slot::Chest)),
            (SingleSlot::Legs, b.item_in(Slot::Legs)),
            (SingleSlot::ClassItem, class_item),
        ];
        Self::new(
            singles.into_iter().filter_map(|(s, h)| h.map(|h| (s, h))),
            [
                (
                    GroupSlot::Aspects,
                    b.hashes(ComponentKind::Aspect).collect(),
                ),
                (
                    GroupSlot::Fragments,
                    b.hashes(ComponentKind::Fragment).collect(),
                ),
                (
                    GroupSlot::WeaponMods,
                    b.hashes(ComponentKind::WeaponMod).collect(),
                ),
                (GroupSlot::ArmorMods, armor_mods),
                (GroupSlot::ClassItemPerks, class_item_perks),
                (
                    GroupSlot::ArtifactPerks,
                    b.hashes(ComponentKind::ArtifactPerk).collect(),
                ),
            ],
        )
    }

    #[must_use]
    pub fn single(&self, slot: SingleSlot) -> Option<u32> {
        self.singles[slot.index()]
    }

    /// The (sorted) items in a slot group.
    #[must_use]
    pub fn group(&self, slot: GroupSlot) -> &[u32] {
        &self.groups[slot.index()]
    }

    /// Number of occupied slots (each group item counts as one slot).
    #[must_use]
    pub fn filled_slots(&self) -> usize {
        self.singles.iter().filter(|s| s.is_some()).count()
            + self.groups.iter().map(Vec::len).sum::<usize>()
    }

    /// Number of differing slots (see module docs).
    #[must_use]
    pub fn distance(&self, other: &Self) -> u32 {
        let singles = self
            .singles
            .iter()
            .zip(&other.singles)
            .filter(|(a, b)| a != b)
            .count() as u32;
        let groups: u32 = self
            .groups
            .iter()
            .zip(&other.groups)
            .map(|(a, b)| bag_distance(a, b))
            .sum();
        singles + groups
    }

    /// Stable content fingerprint (`vector_id`).
    #[must_use]
    pub fn id(&self) -> i64 {
        let mut fp = Fingerprinter::new(b"searchlight.vector", VECTOR_FINGERPRINT_VERSION);
        for s in &self.singles {
            fp.u32(s.unwrap_or(0));
        }
        for g in &self.groups {
            fp.u32(g.len() as u32);
            for &h in g {
                fp.u32(h);
            }
        }
        fp.finish()
    }
}

/// Bag (multiset) distance `max(|A∖B|, |B∖A|)` between two **sorted**
/// slices: the number of slots that differ under the best alignment.
#[must_use]
pub fn bag_distance(a: &[u32], b: &[u32]) -> u32 {
    debug_assert!(a.is_sorted() && b.is_sorted());
    // Size of the multiset intersection via a sorted merge.
    let (mut i, mut j, mut common) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                common += 1;
                i += 1;
                j += 1;
            }
        }
    }
    (a.len().max(b.len()) - common) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(singles: &[(SingleSlot, u32)], groups: &[(GroupSlot, &[u32])]) -> SlotVector {
        SlotVector::new(
            singles.iter().copied(),
            groups.iter().map(|&(g, items)| (g, items.to_vec())),
        )
    }

    #[test]
    fn bag_distance_examples() {
        assert_eq!(bag_distance(&[], &[]), 0);
        assert_eq!(bag_distance(&[1, 2], &[1, 2]), 0);
        // One aspect swapped: 1, even though sorted positions both differ.
        assert_eq!(bag_distance(&[1, 2], &[2, 3]), 1);
        assert_eq!(bag_distance(&[1, 2, 3, 4], &[1, 2, 3, 4, 5]), 1); // one more fragment
        assert_eq!(bag_distance(&[1, 1, 2], &[1, 2, 2]), 1); // multiset: one copy changed
        assert_eq!(bag_distance(&[1, 2], &[3, 4, 5]), 3);
    }

    #[test]
    fn group_order_does_not_matter() {
        let a = v(&[], &[(GroupSlot::Aspects, &[7, 3])]);
        let b = v(&[], &[(GroupSlot::Aspects, &[3, 7])]);
        assert_eq!(a, b);
        assert_eq!(a.distance(&b), 0);
        assert_eq!(a.id(), b.id());
    }

    #[test]
    fn counts_differing_slots() {
        let base = v(
            &[
                (SingleSlot::Super, 1),
                (SingleSlot::Kinetic, 2),
                (SingleSlot::Helmet, 3),
            ],
            &[
                (GroupSlot::Aspects, &[10, 11]),
                (GroupSlot::Fragments, &[20, 21, 22]),
            ],
        );
        let other = v(
            &[(SingleSlot::Super, 1), (SingleSlot::Kinetic, 9)], // weapon swapped, helmet missing
            &[
                (GroupSlot::Aspects, &[11, 12]),
                (GroupSlot::Fragments, &[20, 21, 22, 23]),
            ],
        );
        // kinetic + helmet + one aspect + one extra fragment.
        assert_eq!(base.distance(&other), 4);
        assert_eq!(base.filled_slots(), 8);
        assert_ne!(base.id(), other.id());
    }

    /// A tiny deterministic generator so the property test needs no crate.
    fn lcg(seed: &mut u64) -> u32 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*seed >> 33) as u32
    }

    fn random_vector(seed: &mut u64) -> SlotVector {
        // Small alphabets so random vectors collide and share items often.
        let mut singles = Vec::new();
        for s in SingleSlot::ALL {
            if !lcg(seed).is_multiple_of(4) {
                singles.push((s, lcg(seed) % 3));
            }
        }
        let mut groups = Vec::new();
        for g in GroupSlot::ALL {
            let n = lcg(seed) % 5;
            let mut items = Vec::new();
            for _ in 0..n {
                items.push(lcg(seed) % 4);
            }
            groups.push((g, items));
        }
        SlotVector::new(singles, groups)
    }

    #[test]
    fn distance_is_a_metric() {
        let mut seed = 42;
        let vs: Vec<SlotVector> = (0..60).map(|_| random_vector(&mut seed)).collect();
        for a in &vs {
            assert_eq!(a.distance(a), 0);
            for b in &vs {
                let d = a.distance(b);
                assert_eq!(d, b.distance(a), "symmetry");
                assert_eq!(d == 0, a == b, "identity of indiscernibles");
                for c in &vs {
                    assert!(a.distance(c) <= d + b.distance(c), "triangle inequality");
                }
            }
        }
    }

    #[test]
    fn known_vector_id_is_stable() {
        // Guards the encoding: if this changes, bump VECTOR_FINGERPRINT_VERSION.
        let a = v(&[(SingleSlot::Super, 1)], &[(GroupSlot::Aspects, &[2, 3])]);
        assert_eq!(a.id(), 3_884_520_671_978_028_491);
    }
}
