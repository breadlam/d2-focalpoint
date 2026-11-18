//! An in-memory view of the parts of the Bungie manifest this tool needs.
//!
//! Parsing works on JSON bytes (the cached `Destiny*Definition.json` files are
//! hash-keyed objects); reading the files is the caller's job, keeping this
//! module I/O-free. Only the fields we use are deserialized, so the parser is
//! robust to manifest schema drift.

use std::collections::HashMap;

use serde::Deserialize;

use crate::class::{Element, GuardianClass};
use crate::classify::{PlugKind, classify_plug};
use crate::slot::{Slot, SlotGroup};

/// `tierType` value for Exotic items.
pub const TIER_EXOTIC: u8 = 6;

/// The subset of `DestinyInventoryItemDefinition` we keep per item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemDef {
    pub hash: u32,
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
    /// `DestinyItemType` (3 = weapon, 2 = armor, 16 = subclass, 19 = mod, ...).
    pub item_type: i32,
    /// `DestinyItemSubType` (weapon archetype, armor piece, ...).
    pub item_sub_type: i32,
    /// e.g. "Hand Cannon", "Gauntlets", "Warlock Aspect".
    pub item_type_name: String,
    pub tier_type: u8,
    pub tier_name: String,
    /// 0 = Titan, 1 = Hunter, 2 = Warlock, 3 = any.
    pub class_type: u8,
    pub bucket_hash: u32,
    /// `DamageType` enum (1 = Kinetic, 2 = Arc, 3 = Solar, 4 = Void, 6 = Stasis, 7 = Strand).
    pub default_damage_type: u8,
    /// 1 = primary, 2 = special, 3 = heavy (weapons only).
    pub ammo_type: u8,
    pub breaker_type: u8,
    /// `plug.plugCategoryIdentifier`, for items that can be socketed.
    pub plug_category: Option<String>,
    /// `inventory.stackUniqueLabel` — carries the element for subclasses.
    pub stack_unique_label: Option<String>,
    /// `talentGrid.hudDamageType` (subclasses; 1 = Prismatic there).
    pub hud_damage_type: u8,
    /// Armor set (`DestinyEquipableItemSetDefinition`) this item belongs to.
    pub item_set_hash: Option<u32>,
}

impl ItemDef {
    #[must_use]
    pub fn is_exotic(&self) -> bool {
        self.tier_type == TIER_EXOTIC
    }

    #[must_use]
    pub fn slot(&self) -> Slot {
        Slot::from_bucket(self.bucket_hash)
    }

    #[must_use]
    pub fn class(&self) -> GuardianClass {
        GuardianClass::from_class_type(self.class_type)
    }

    /// Element of a subclass item.
    #[must_use]
    pub fn subclass_element(&self) -> Option<Element> {
        if let Some(e) = self
            .stack_unique_label
            .as_deref()
            .and_then(Element::from_identifier)
        {
            return Some(e);
        }
        match self.hud_damage_type {
            1 => Some(Element::Prismatic),
            2 => Some(Element::Arc),
            3 => Some(Element::Solar),
            4 => Some(Element::Void),
            6 => Some(Element::Stasis),
            7 => Some(Element::Strand),
            _ => None,
        }
    }

    /// Classify this item as a plug socketed into an item in `host` slot group.
    #[must_use]
    pub fn plug_kind(&self, host: SlotGroup) -> PlugKind {
        classify_plug(
            self.plug_category.as_deref().unwrap_or(""),
            &self.name,
            host,
        )
    }
}

/// Name of a `DamageType` enum value.
#[must_use]
pub fn damage_type_name(damage_type: u8) -> Option<&'static str> {
    Some(match damage_type {
        1 => "Kinetic",
        2 => "Arc",
        3 => "Solar",
        4 => "Void",
        5 => "Raid",
        6 => "Stasis",
        7 => "Strand",
        _ => return None,
    })
}

/// An armor set and its piece-count bonuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSetDef {
    pub hash: u32,
    pub name: String,
    pub items: Vec<u32>,
    /// `(required piece count, sandbox perk hash)`, ascending by count.
    pub perks: Vec<(u8, u32)>,
}

/// A generic `hash → name/description` definition (sandbox perks, stats, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedDef {
    pub hash: u32,
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
}

/// Activity definition subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityDef {
    pub hash: u32,
    pub name: String,
    pub description: String,
    pub activity_type_hash: u32,
    /// `directActivityModeType`.
    pub mode: Option<i32>,
    pub modes: Vec<i32>,
    pub is_playlist: bool,
    pub light_level: i32,
}

/// Activity mode definition subset, keyed by `modeType`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityModeDef {
    pub mode_type: i32,
    pub hash: u32,
    pub name: String,
    pub friendly_name: String,
}

/// Everything the build pipeline needs to resolve hashes.
#[derive(Debug, Default, Clone)]
pub struct Catalog {
    pub items: HashMap<u32, ItemDef>,
    pub item_sets: HashMap<u32, ItemSetDef>,
    /// Reverse index: armor item hash → set hash.
    pub set_of_item: HashMap<u32, u32>,
}

impl Catalog {
    /// Build a catalog from parsed item and item-set definitions.
    #[must_use]
    pub fn new(items: Vec<ItemDef>, item_sets: Vec<ItemSetDef>) -> Self {
        let mut set_of_item = HashMap::new();
        for set in &item_sets {
            for &item in &set.items {
                set_of_item.insert(item, set.hash);
            }
        }
        let mut items: HashMap<u32, ItemDef> = items.into_iter().map(|i| (i.hash, i)).collect();
        // Some items carry the set hash on the item itself instead; both
        // directions are honoured.
        for item in items.values_mut() {
            if item.item_set_hash.is_none() {
                item.item_set_hash = set_of_item.get(&item.hash).copied();
            } else if let Some(h) = item.item_set_hash {
                set_of_item.entry(item.hash).or_insert(h);
            }
        }
        Self {
            items,
            item_sets: item_sets.into_iter().map(|s| (s.hash, s)).collect(),
            set_of_item,
        }
    }

    #[must_use]
    pub fn item(&self, hash: u32) -> Option<&ItemDef> {
        self.items.get(&hash)
    }
}

// --- Manifest JSON parsing -------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawDisplay {
    name: String,
    description: String,
    icon: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawInventory {
    bucket_type_hash: u32,
    tier_type: u8,
    tier_type_name: Option<String>,
    stack_unique_label: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawEquipping {
    ammo_type: u8,
    equipable_item_set_hash: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawPlug {
    plug_category_identifier: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawTalentGrid {
    hud_damage_type: u8,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawItem {
    #[serde(default)]
    display_properties: RawDisplay,
    #[serde(default)]
    item_type: i32,
    #[serde(default)]
    item_sub_type: i32,
    #[serde(default)]
    item_type_display_name: Option<String>,
    #[serde(default)]
    class_type: u8,
    #[serde(default)]
    default_damage_type: u8,
    #[serde(default)]
    breaker_type: u8,
    #[serde(default)]
    inventory: RawInventory,
    #[serde(default)]
    equipping_block: Option<RawEquipping>,
    #[serde(default)]
    plug: Option<RawPlug>,
    #[serde(default)]
    talent_grid: Option<RawTalentGrid>,
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.is_empty())
}

/// Parse a hash-keyed definition table, skipping keys that are not u32s.
fn parse_table<T: for<'de> Deserialize<'de>>(
    json: &[u8],
) -> Result<Vec<(u32, T)>, serde_json::Error> {
    let raw: HashMap<String, T> = serde_json::from_slice(json)?;
    Ok(raw
        .into_iter()
        .filter_map(|(k, v)| k.parse::<u32>().ok().map(|h| (h, v)))
        .collect())
}

/// Parse `DestinyInventoryItemDefinition.json`.
pub fn parse_items(json: &[u8]) -> Result<Vec<ItemDef>, serde_json::Error> {
    Ok(parse_table::<RawItem>(json)?
        .into_iter()
        .map(|(hash, r)| {
            let equipping = r.equipping_block.unwrap_or_default();
            ItemDef {
                hash,
                name: r.display_properties.name,
                description: r.display_properties.description,
                icon: non_empty(r.display_properties.icon),
                item_type: r.item_type,
                item_sub_type: r.item_sub_type,
                item_type_name: r.item_type_display_name.unwrap_or_default(),
                tier_type: r.inventory.tier_type,
                tier_name: r.inventory.tier_type_name.unwrap_or_default(),
                class_type: r.class_type,
                bucket_hash: r.inventory.bucket_type_hash,
                default_damage_type: r.default_damage_type,
                ammo_type: equipping.ammo_type,
                breaker_type: r.breaker_type,
                plug_category: non_empty(r.plug.and_then(|p| p.plug_category_identifier)),
                stack_unique_label: non_empty(r.inventory.stack_unique_label),
                hud_damage_type: r.talent_grid.map_or(0, |t| t.hud_damage_type),
                item_set_hash: equipping.equipable_item_set_hash.filter(|&h| h != 0),
            }
        })
        .collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSetPerk {
    required_set_count: u8,
    sandbox_perk_hash: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawItemSet {
    #[serde(default)]
    display_properties: RawDisplay,
    #[serde(default)]
    set_items: Vec<u32>,
    #[serde(default)]
    set_perks: Vec<RawSetPerk>,
}

/// Parse `DestinyEquipableItemSetDefinition.json`.
pub fn parse_item_sets(json: &[u8]) -> Result<Vec<ItemSetDef>, serde_json::Error> {
    Ok(parse_table::<RawItemSet>(json)?
        .into_iter()
        .map(|(hash, r)| {
            let mut perks: Vec<(u8, u32)> = r
                .set_perks
                .into_iter()
                .map(|p| (p.required_set_count, p.sandbox_perk_hash))
                .collect();
            perks.sort_unstable();
            ItemSetDef {
                hash,
                name: r.display_properties.name,
                items: r.set_items,
                perks,
            }
        })
        .collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawNamed {
    #[serde(default)]
    display_properties: RawDisplay,
}

/// Parse any definition table down to `hash → name/description/icon`
/// (`DestinySandboxPerkDefinition`, `DestinyStatDefinition`, ...).
pub fn parse_named(json: &[u8]) -> Result<Vec<NamedDef>, serde_json::Error> {
    Ok(parse_table::<RawNamed>(json)?
        .into_iter()
        .map(|(hash, r)| NamedDef {
            hash,
            name: r.display_properties.name,
            description: r.display_properties.description,
            icon: non_empty(r.display_properties.icon),
        })
        .collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawActivity {
    #[serde(default)]
    display_properties: RawDisplay,
    #[serde(default)]
    activity_type_hash: u32,
    #[serde(default)]
    direct_activity_mode_type: Option<i32>,
    #[serde(default)]
    activity_mode_types: Vec<i32>,
    #[serde(default)]
    is_playlist: bool,
    #[serde(default)]
    activity_light_level: i32,
}

/// Parse `DestinyActivityDefinition.json`.
pub fn parse_activities(json: &[u8]) -> Result<Vec<ActivityDef>, serde_json::Error> {
    Ok(parse_table::<RawActivity>(json)?
        .into_iter()
        .map(|(hash, r)| ActivityDef {
            hash,
            name: r.display_properties.name,
            description: r.display_properties.description,
            activity_type_hash: r.activity_type_hash,
            mode: r.direct_activity_mode_type,
            modes: r.activity_mode_types,
            is_playlist: r.is_playlist,
            light_level: r.activity_light_level,
        })
        .collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMode {
    #[serde(default)]
    display_properties: RawDisplay,
    mode_type: i32,
    #[serde(default)]
    friendly_name: String,
}

/// Parse `DestinyActivityModeDefinition.json` (keyed on `modeType` by callers).
pub fn parse_activity_modes(json: &[u8]) -> Result<Vec<ActivityModeDef>, serde_json::Error> {
    Ok(parse_table::<RawMode>(json)?
        .into_iter()
        .map(|(hash, r)| ActivityModeDef {
            mode_type: r.mode_type,
            hash,
            name: r.display_properties.name,
            friendly_name: r.friendly_name,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITEMS: &str = r#"{
      "300502917": {"displayProperties":{"name":"Nothing Manacles","description":"","icon":"/a.jpg"},
        "itemTypeDisplayName":"Gauntlets","itemType":2,"itemSubType":27,"classType":2,
        "defaultDamageType":0,"breakerType":0,
        "inventory":{"bucketTypeHash":3551918588,"tierType":6,"tierTypeName":"Exotic"},
        "equippingBlock":{"ammoType":0}},
      "3941205951": {"displayProperties":{"name":"Dawnblade"},"itemType":16,"classType":2,
        "inventory":{"bucketTypeHash":3284755031,"tierType":2,"stackUniqueLabel":"v610.builds.solar.warlock.build0"},
        "talentGrid":{"hudDamageType":3}},
      "12345": {"displayProperties":{"name":"Touch of Flame"},"plug":{"plugCategoryIdentifier":"warlock.solar.aspects"}},
      "not-a-hash": {"displayProperties":{"name":"ignored"}}
    }"#;

    #[test]
    fn parses_items_and_skips_non_numeric_keys() {
        let items = parse_items(ITEMS.as_bytes()).unwrap();
        assert_eq!(items.len(), 3);
        let cat = Catalog::new(items, vec![]);
        let manacles = cat.item(300_502_917).unwrap();
        assert!(manacles.is_exotic());
        assert_eq!(manacles.slot(), Slot::Gauntlets);
        assert_eq!(manacles.class(), GuardianClass::Warlock);
        assert_eq!(manacles.icon.as_deref(), Some("/a.jpg"));

        let dawnblade = cat.item(3_941_205_951).unwrap();
        assert_eq!(dawnblade.subclass_element(), Some(Element::Solar));

        let aspect = cat.item(12345).unwrap();
        assert_eq!(aspect.plug_kind(SlotGroup::Subclass), PlugKind::Aspect);
    }

    #[test]
    fn item_sets_index_both_directions() {
        let sets = parse_item_sets(
            br#"{"77":{"displayProperties":{"name":"AION Renewal"},"setItems":[1,2],
                 "setPerks":[{"requiredSetCount":4,"sandboxPerkHash":9},{"requiredSetCount":2,"sandboxPerkHash":8}]}}"#,
        )
        .unwrap();
        assert_eq!(sets[0].perks, vec![(2, 8), (4, 9)]);
        let items = parse_items(br#"{"1":{"displayProperties":{"name":"Helm"}}}"#).unwrap();
        let cat = Catalog::new(items, sets);
        assert_eq!(cat.item(1).unwrap().item_set_hash, Some(77));
        assert_eq!(cat.set_of_item.get(&2), Some(&77));
    }

    #[test]
    fn parses_activities_and_modes() {
        let acts = parse_activities(
            br#"{"2727361621":{"displayProperties":{"name":"Equilibrium: Standard"},
                 "directActivityModeType":82,"activityModeTypes":[82,7],"activityTypeHash":608898761}}"#,
        )
        .unwrap();
        assert_eq!(acts[0].mode, Some(82));
        assert_eq!(acts[0].modes, vec![82, 7]);
        let modes = parse_activity_modes(
            br#"{"608898761":{"displayProperties":{"name":"Dungeon"},"modeType":82,"friendlyName":"dungeon"}}"#,
        )
        .unwrap();
        assert_eq!(modes[0].mode_type, 82);
    }
}
