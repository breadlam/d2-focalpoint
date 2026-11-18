//! Equipment slots, keyed by Destiny inventory bucket hash.
//!
//! Bucket hashes are stable game constants (not manifest-version specific), so
//! they are hard-coded here rather than read from `DestinyInventoryBucketDefinition`
//! (which the Searchlight manifest cache did not keep).

use std::fmt;

/// Where an equipped item lives on a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Slot {
    Subclass,
    Kinetic,
    Energy,
    Power,
    Helmet,
    Gauntlets,
    Chest,
    Legs,
    ClassItem,
    Artifact,
    Ghost,
    Sparrow,
    Ship,
    Emblem,
    Emote,
    Finisher,
    ClanBanner,
    /// A bucket this crate does not know about.
    Other,
}

/// Coarse grouping of [`Slot`]s, used to disambiguate plug categories that
/// mean different things on weapons and armor (e.g. `intrinsics`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlotGroup {
    Subclass,
    Weapon,
    Armor,
    Artifact,
    Cosmetic,
    Other,
}

impl Slot {
    /// Every known slot, in display order.
    pub const ALL: [Slot; 18] = [
        Slot::Subclass,
        Slot::Kinetic,
        Slot::Energy,
        Slot::Power,
        Slot::Helmet,
        Slot::Gauntlets,
        Slot::Chest,
        Slot::Legs,
        Slot::ClassItem,
        Slot::Artifact,
        Slot::Ghost,
        Slot::Sparrow,
        Slot::Ship,
        Slot::Emblem,
        Slot::Emote,
        Slot::Finisher,
        Slot::ClanBanner,
        Slot::Other,
    ];

    /// Map an inventory bucket hash to its slot.
    #[must_use]
    pub fn from_bucket(bucket_hash: u32) -> Self {
        match bucket_hash {
            3_284_755_031 => Slot::Subclass,
            1_498_876_634 => Slot::Kinetic,
            2_465_295_065 => Slot::Energy,
            953_998_645 => Slot::Power,
            3_448_274_439 => Slot::Helmet,
            3_551_918_588 => Slot::Gauntlets,
            14_239_492 => Slot::Chest,
            20_886_954 => Slot::Legs,
            1_585_787_867 => Slot::ClassItem,
            1_506_418_338 => Slot::Artifact,
            4_023_194_814 => Slot::Ghost,
            2_025_709_351 => Slot::Sparrow,
            284_967_655 => Slot::Ship,
            4_274_335_291 => Slot::Emblem,
            1_107_761_855 | 3_054_419_239 => Slot::Emote,
            3_683_254_069 => Slot::Finisher,
            4_292_445_962 => Slot::ClanBanner,
            _ => Slot::Other,
        }
    }

    /// The canonical bucket hash for this slot (`None` for [`Slot::Other`]).
    #[must_use]
    pub fn bucket_hash(self) -> Option<u32> {
        Some(match self {
            Slot::Subclass => 3_284_755_031,
            Slot::Kinetic => 1_498_876_634,
            Slot::Energy => 2_465_295_065,
            Slot::Power => 953_998_645,
            Slot::Helmet => 3_448_274_439,
            Slot::Gauntlets => 3_551_918_588,
            Slot::Chest => 14_239_492,
            Slot::Legs => 20_886_954,
            Slot::ClassItem => 1_585_787_867,
            Slot::Artifact => 1_506_418_338,
            Slot::Ghost => 4_023_194_814,
            Slot::Sparrow => 2_025_709_351,
            Slot::Ship => 284_967_655,
            Slot::Emblem => 4_274_335_291,
            Slot::Emote => 1_107_761_855,
            Slot::Finisher => 3_683_254_069,
            Slot::ClanBanner => 4_292_445_962,
            Slot::Other => return None,
        })
    }

    #[must_use]
    pub fn group(self) -> SlotGroup {
        match self {
            Slot::Subclass => SlotGroup::Subclass,
            Slot::Kinetic | Slot::Energy | Slot::Power => SlotGroup::Weapon,
            Slot::Helmet | Slot::Gauntlets | Slot::Chest | Slot::Legs | Slot::ClassItem => {
                SlotGroup::Armor
            }
            Slot::Artifact => SlotGroup::Artifact,
            Slot::Ghost
            | Slot::Sparrow
            | Slot::Ship
            | Slot::Emblem
            | Slot::Emote
            | Slot::Finisher
            | Slot::ClanBanner => SlotGroup::Cosmetic,
            Slot::Other => SlotGroup::Other,
        }
    }

    /// Stable storage name (used verbatim in the database).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Slot::Subclass => "subclass",
            Slot::Kinetic => "kinetic",
            Slot::Energy => "energy",
            Slot::Power => "power",
            Slot::Helmet => "helmet",
            Slot::Gauntlets => "gauntlets",
            Slot::Chest => "chest",
            Slot::Legs => "legs",
            Slot::ClassItem => "class_item",
            Slot::Artifact => "artifact",
            Slot::Ghost => "ghost",
            Slot::Sparrow => "sparrow",
            Slot::Ship => "ship",
            Slot::Emblem => "emblem",
            Slot::Emote => "emote",
            Slot::Finisher => "finisher",
            Slot::ClanBanner => "clan_banner",
            Slot::Other => "other",
        }
    }

    /// A small stable code used inside build fingerprints. Never reorder:
    /// changing these values changes every `build_id`.
    #[must_use]
    pub(crate) fn code(self) -> u8 {
        match self {
            Slot::Subclass => 1,
            Slot::Kinetic => 2,
            Slot::Energy => 3,
            Slot::Power => 4,
            Slot::Helmet => 5,
            Slot::Gauntlets => 6,
            Slot::Chest => 7,
            Slot::Legs => 8,
            Slot::ClassItem => 9,
            Slot::Artifact => 10,
            Slot::Ghost => 11,
            Slot::Sparrow => 12,
            Slot::Ship => 13,
            Slot::Emblem => 14,
            Slot::Emote => 15,
            Slot::Finisher => 16,
            Slot::ClanBanner => 17,
            Slot::Other => 255,
        }
    }
}

impl SlotGroup {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SlotGroup::Subclass => "subclass",
            SlotGroup::Weapon => "weapon",
            SlotGroup::Armor => "armor",
            SlotGroup::Artifact => "artifact",
            SlotGroup::Cosmetic => "cosmetic",
            SlotGroup::Other => "other",
        }
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_roundtrip_for_every_known_slot() {
        for slot in Slot::ALL {
            match slot.bucket_hash() {
                Some(b) => assert_eq!(Slot::from_bucket(b), slot, "{slot}"),
                None => assert_eq!(slot, Slot::Other),
            }
        }
    }

    #[test]
    fn fingerprint_codes_are_unique() {
        let mut codes: Vec<u8> = Slot::ALL.iter().map(|s| s.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), Slot::ALL.len());
    }

    #[test]
    fn groups() {
        assert_eq!(Slot::Power.group(), SlotGroup::Weapon);
        assert_eq!(Slot::ClassItem.group(), SlotGroup::Armor);
        assert_eq!(Slot::Ship.group(), SlotGroup::Cosmetic);
        assert_eq!(Slot::from_bucket(42).group(), SlotGroup::Other);
    }
}
