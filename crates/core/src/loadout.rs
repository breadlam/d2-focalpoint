//! The loadout format Searchlight stored in `build_segment.loadout_state`.
//!
//! This mirrors the production `LoadoutState` serde shape exactly (see
//! `docs/data-sources.md`), so every stored timeline deserializes. Fields added
//! over the service's life (`artifact`, `stats`) are optional.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::slot::Slot;

/// Well-known stat hashes found in `LoadoutState::stats`.
pub mod stat_hash {
    pub const POWER: u32 = 1_935_470_627;
    pub const WEAPONS: u32 = 2_996_146_975;
    pub const HEALTH: u32 = 392_767_087;
    pub const CLASS: u32 = 1_943_323_491;
    pub const GRENADE: u32 = 1_735_777_505;
    pub const SUPER: u32 = 144_602_215;
    pub const MELEE: u32 = 4_244_567_218;
}

/// A character's full equipped loadout at one point in a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadoutState {
    pub items: Vec<EquippedItem>,
    #[serde(default)]
    pub artifact: Option<ArtifactState>,
    /// Character stat totals (`statHash` → value), including Power.
    #[serde(default)]
    pub stats: BTreeMap<u32, i32>,
}

/// One equipped item with its socketed plugs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquippedItem {
    pub item_hash: u32,
    pub item_instance_id: u64,
    pub bucket_hash: u32,
    /// Socket plug hashes in socket order (subclass abilities/aspects/fragments,
    /// weapon perks, armor mods, cosmetics, ...).
    #[serde(default)]
    pub plug_hashes: Vec<u32>,
    /// `DestinySandboxPerkDefinition` hashes from the perks component.
    #[serde(default)]
    pub perk_hashes: Vec<u32>,
}

/// The equipped seasonal artifact and its active perks (from progressions).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactState {
    pub artifact_hash: u32,
    #[serde(default)]
    pub active_perks: Vec<u32>,
}

impl LoadoutState {
    /// Parse a stored `loadout_state` JSON document.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// The item in a given slot, if equipped.
    #[must_use]
    pub fn item_in(&self, slot: Slot) -> Option<&EquippedItem> {
        self.items.iter().find(|i| i.slot() == slot)
    }

    #[must_use]
    pub fn stat(&self, stat_hash: u32) -> Option<i32> {
        self.stats.get(&stat_hash).copied()
    }
}

impl EquippedItem {
    #[must_use]
    pub fn slot(&self) -> Slot {
        Slot::from_bucket(self.bucket_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_legacy_shape_without_artifact_or_stats() {
        let json = r#"{"items":[{"item_hash":1,"item_instance_id":2,"bucket_hash":3284755031,
                       "plug_hashes":[5,6],"perk_hashes":[]}]}"#;
        let l = LoadoutState::from_json(json).unwrap();
        assert!(l.artifact.is_none());
        assert!(l.stats.is_empty());
        assert_eq!(l.item_in(Slot::Subclass).unwrap().plug_hashes, vec![5, 6]);
    }

    #[test]
    fn parses_string_keyed_stats() {
        let json = r#"{"items":[],"artifact":{"artifact_hash":9,"active_perks":[]},
                       "stats":{"1935470627":549,"392767087":3}}"#;
        let l = LoadoutState::from_json(json).unwrap();
        assert_eq!(l.stat(stat_hash::POWER), Some(549));
        assert_eq!(l.stat(stat_hash::HEALTH), Some(3));
        assert_eq!(l.artifact.unwrap().artifact_hash, 9);
    }
}
