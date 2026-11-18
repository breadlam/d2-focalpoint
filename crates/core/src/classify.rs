//! Socket plug classification.
//!
//! Maps a plug's manifest `plugCategoryIdentifier` (plus its display name and
//! the slot group of the item it is socketed into) to a [`PlugKind`]. This is
//! the production Searchlight heuristic with fixes for gaps found in the
//! collected data (see `docs/adr/0002-build-identity.md`):
//!
//! * Prismatic Transcendence grenades (`*.prism_grenade`) and the
//!   Transcendence ability were unclassified.
//! * Weapon mods (`v400.weapon.mod_*`) were unclassified.
//! * Armor `intrinsics` (exotic perks, exotic class item "Spirit of ..." rolls,
//!   legendary archetype intrinsics) were labelled as weapon intrinsics.
//! * Armor stat-roll plugs (`armor_stats`), artifact perks and crafting plugs
//!   now have their own kinds.

use std::fmt;

use crate::slot::SlotGroup;

/// What a socketed plug is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PlugKind {
    // Subclass.
    Super,
    ClassAbility,
    Movement,
    Melee,
    Grenade,
    Aspect,
    Fragment,
    /// Prismatic's Transcendence ability (fixed per class).
    Transcendence,
    // Weapons.
    WeaponIntrinsic,
    WeaponPerk,
    OriginTrait,
    WeaponMod,
    // Armor.
    /// Armor intrinsic: exotic perk, exotic class item spirit, legendary intrinsic.
    ArmorPerk,
    ArmorArchetype,
    ArmorMod,
    ArmorStatMod,
    ArmorTuning,
    /// The random stat roll of a piece of armor (not a build choice).
    ArmorStatRoll,
    // Artifact.
    ArtifactPerk,
    // Noise: never part of a build's identity.
    Masterwork,
    Ornament,
    Shader,
    Cosmetic,
    Crafting,
    Empty,
    Other,
}

impl PlugKind {
    /// Every kind, for exhaustive tests and documentation.
    pub const ALL: [PlugKind; 26] = [
        PlugKind::Super,
        PlugKind::ClassAbility,
        PlugKind::Movement,
        PlugKind::Melee,
        PlugKind::Grenade,
        PlugKind::Aspect,
        PlugKind::Fragment,
        PlugKind::Transcendence,
        PlugKind::WeaponIntrinsic,
        PlugKind::WeaponPerk,
        PlugKind::OriginTrait,
        PlugKind::WeaponMod,
        PlugKind::ArmorPerk,
        PlugKind::ArmorArchetype,
        PlugKind::ArmorMod,
        PlugKind::ArmorStatMod,
        PlugKind::ArmorTuning,
        PlugKind::ArmorStatRoll,
        PlugKind::ArtifactPerk,
        PlugKind::Masterwork,
        PlugKind::Ornament,
        PlugKind::Shader,
        PlugKind::Cosmetic,
        PlugKind::Crafting,
        PlugKind::Empty,
        PlugKind::Other,
    ];

    /// Does this plug describe a gameplay choice that belongs in a build?
    #[must_use]
    pub fn is_build_relevant(self) -> bool {
        !matches!(
            self,
            PlugKind::ArmorStatRoll
                | PlugKind::Masterwork
                | PlugKind::Ornament
                | PlugKind::Shader
                | PlugKind::Cosmetic
                | PlugKind::Crafting
                | PlugKind::Empty
                | PlugKind::Other
        )
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            PlugKind::Super => "super",
            PlugKind::ClassAbility => "class_ability",
            PlugKind::Movement => "movement",
            PlugKind::Melee => "melee",
            PlugKind::Grenade => "grenade",
            PlugKind::Aspect => "aspect",
            PlugKind::Fragment => "fragment",
            PlugKind::Transcendence => "transcendence",
            PlugKind::WeaponIntrinsic => "weapon_intrinsic",
            PlugKind::WeaponPerk => "weapon_perk",
            PlugKind::OriginTrait => "origin_trait",
            PlugKind::WeaponMod => "weapon_mod",
            PlugKind::ArmorPerk => "armor_perk",
            PlugKind::ArmorArchetype => "armor_archetype",
            PlugKind::ArmorMod => "armor_mod",
            PlugKind::ArmorStatMod => "armor_stat_mod",
            PlugKind::ArmorTuning => "armor_tuning",
            PlugKind::ArmorStatRoll => "armor_stat_roll",
            PlugKind::ArtifactPerk => "artifact_perk",
            PlugKind::Masterwork => "masterwork",
            PlugKind::Ornament => "ornament",
            PlugKind::Shader => "shader",
            PlugKind::Cosmetic => "cosmetic",
            PlugKind::Crafting => "crafting",
            PlugKind::Empty => "empty",
            PlugKind::Other => "other",
        }
    }
}

impl fmt::Display for PlugKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Weapon perk column categories (barrels, magazines, traits, ...).
const WEAPON_PERK_CATEGORIES: &[&str] = &[
    "barrels",
    "magazines",
    "frames",
    "scopes",
    "stocks",
    "grips",
    "batteries",
    "blades",
    "guards",
    "bowstrings",
    "arrows",
    "tubes",
    "hafts",
    "projectiles",
    "ammunition",
    "sights",
    "perks",
    "catalysts",
];

const COSMETIC_CATEGORIES: &[&str] = &[
    "tracker",
    "memento",
    "weapon_cosmetic",
    "mod_cosmetic",
    "glowing",
    "kill_vfx",
    "confetti",
    "emote",
];

/// Classify a plug from its `plugCategoryIdentifier`, display name, and the
/// slot group of the item it is socketed into.
#[must_use]
pub fn classify_plug(plug_category: &str, name: &str, host: SlotGroup) -> PlugKind {
    let c = plug_category.to_ascii_lowercase();

    // Armor stat rolls are nameless; check before the empty-name rule.
    if c == "armor_stats" {
        return PlugKind::ArmorStatRoll;
    }
    if name.is_empty()
        || name.starts_with("Empty ")
        || c.contains("empty_socket")
        || c.ends_with("mod_empty")
        || c.starts_with("armor_skins_empty")
    {
        return PlugKind::Empty;
    }
    if c.contains("shader") {
        return PlugKind::Shader;
    }
    if c.contains("skins") || c.contains("ornament") {
        return PlugKind::Ornament;
    }
    if c.starts_with("crafting.") || c == "intermediate_plug_that_works_in_every_category" {
        return PlugKind::Crafting;
    }
    if c == "artifact_perks" || host == SlotGroup::Artifact {
        return PlugKind::ArtifactPerk;
    }

    // Subclass plugs only ever socket into subclasses; `Other` means the host
    // is unknown (e.g. classifying a bare manifest item), so allow it too.
    if matches!(host, SlotGroup::Subclass | SlotGroup::Other)
        && let Some(kind) = classify_subclass_plug(&c)
    {
        return kind;
    }

    // Masterworks and enhancement-tier upgrades (weapons and armor).
    if c.contains("masterwork") || c.contains("enhancers") || c.contains("artifice.exotic") {
        return PlugKind::Masterwork;
    }

    // Intrinsics mean different things on weapons and armor.
    if c == "intrinsics" || c.contains(".intrinsics") {
        return if host == SlotGroup::Armor {
            PlugKind::ArmorPerk
        } else {
            PlugKind::WeaponIntrinsic
        };
    }

    // Weapons.
    if c.contains("origins") {
        return PlugKind::OriginTrait;
    }
    if c.contains("weapon.mod_") || c.contains("weapon_mod") {
        return if COSMETIC_CATEGORIES.iter().any(|p| c.contains(p)) {
            PlugKind::Cosmetic
        } else {
            PlugKind::WeaponMod
        };
    }
    if COSMETIC_CATEGORIES.iter().any(|p| c.contains(p)) {
        return PlugKind::Cosmetic;
    }
    if WEAPON_PERK_CATEGORIES.iter().any(|p| c.contains(p)) {
        return PlugKind::WeaponPerk;
    }

    // Armor (Armor 3.0 and legacy).
    if c.contains("archetypes") {
        return PlugKind::ArmorArchetype;
    }
    if c.contains("v2_general") {
        return PlugKind::ArmorStatMod;
    }
    if c.contains("armor_tiering") || c.contains("artifice") {
        return PlugKind::ArmorTuning;
    }
    if c.starts_with("enhancements") {
        return PlugKind::ArmorMod;
    }
    if host == SlotGroup::Armor && matches!(c.as_str(), "mods" | "build_perk" | "ammo_perk") {
        return PlugKind::ArmorMod;
    }

    PlugKind::Other
}

/// Subclass plug categories. Stasis uses legacy identifiers: aspects are
/// `totems`, fragments are `trinkets`; Prismatic grenades are `prism_grenade`.
fn classify_subclass_plug(c: &str) -> Option<PlugKind> {
    Some(if c.contains("aspects") || c.contains("totems") {
        PlugKind::Aspect
    } else if c.contains("fragments") || c.contains("trinkets") {
        PlugKind::Fragment
    } else if c.contains("supers") {
        PlugKind::Super
    } else if c.contains("transcendence") || c.contains("prism_grenade") {
        // `prism_grenade` is the class's fixed Transcendence grenade
        // (e.g. Freezing Singularity), socketed alongside the chosen grenade.
        PlugKind::Transcendence
    } else if c.contains("class_abilities") {
        PlugKind::ClassAbility
    } else if c.contains("grenade") {
        PlugKind::Grenade
    } else if c.contains("movement") {
        PlugKind::Movement
    } else if c.contains("melee") {
        PlugKind::Melee
    } else {
        return None;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUB: SlotGroup = SlotGroup::Subclass;
    const WPN: SlotGroup = SlotGroup::Weapon;
    const ARM: SlotGroup = SlotGroup::Armor;

    #[test]
    fn subclass_plugs() {
        assert_eq!(
            classify_plug("warlock.solar.aspects", "Touch of Flame", SUB),
            PlugKind::Aspect
        );
        assert_eq!(
            classify_plug("shared.solar.fragments", "Ember of Torches", SUB),
            PlugKind::Fragment
        );
        assert_eq!(
            classify_plug("warlock.solar.supers", "Well of Radiance", SUB),
            PlugKind::Super
        );
        assert_eq!(
            classify_plug("warlock.solar.class_abilities", "Phoenix Dive", SUB),
            PlugKind::ClassAbility
        );
        assert_eq!(
            classify_plug("shared.solar.grenades", "Healing Grenade", SUB),
            PlugKind::Grenade
        );
        assert_eq!(
            classify_plug("warlock.solar.melee", "Incinerator Snap", SUB),
            PlugKind::Melee
        );
        assert_eq!(
            classify_plug("warlock.solar.movement", "Strafe Glide", SUB),
            PlugKind::Movement
        );
        assert_eq!(
            classify_plug("warlock.stasis.totems", "Iceflare Bolts", SUB),
            PlugKind::Aspect
        );
        assert_eq!(
            classify_plug("shared.stasis.trinkets", "Whisper of Rime", SUB),
            PlugKind::Fragment
        );
    }

    #[test]
    fn prismatic_specifics() {
        // The Transcendence grenade is fixed per class, not the grenade choice.
        assert_eq!(
            classify_plug("warlock.prism.prism_grenade", "Freezing Singularity", SUB),
            PlugKind::Transcendence
        );
        assert_eq!(
            classify_plug("shared.prism.grenades", "Healing Grenade", SUB),
            PlugKind::Grenade
        );
        assert_eq!(
            classify_plug("warlock.prism.transcendence", "Transcendence", SUB),
            PlugKind::Transcendence
        );
        assert_eq!(
            classify_plug("shared.prism.fragments", "Facet of Dawn", SUB),
            PlugKind::Fragment
        );
    }

    #[test]
    fn weapon_plugs() {
        assert_eq!(
            classify_plug("frames", "Rampage", WPN),
            PlugKind::WeaponPerk
        );
        assert_eq!(
            classify_plug("magazines_gl", "Spike Grenades", WPN),
            PlugKind::WeaponPerk
        );
        assert_eq!(
            classify_plug("barrels", "Arrowhead Brake", WPN),
            PlugKind::WeaponPerk
        );
        assert_eq!(
            classify_plug("catalysts", "Disease Vector", WPN),
            PlugKind::WeaponPerk
        );
        assert_eq!(
            classify_plug("origins", "Veist Stinger", WPN),
            PlugKind::OriginTrait
        );
        assert_eq!(
            classify_plug("intrinsics", "Adaptive Frame", WPN),
            PlugKind::WeaponIntrinsic
        );
        assert_eq!(
            classify_plug("v400.weapon.mod_guns", "Backup Mag", WPN),
            PlugKind::WeaponMod
        );
        assert_eq!(
            classify_plug("v400.weapon.mod_empty", "Empty Mod Socket", WPN),
            PlugKind::Empty
        );
        assert_eq!(
            classify_plug("v900weapon.mod_confetti", "Confetti", WPN),
            PlugKind::Cosmetic
        );
        assert_eq!(
            classify_plug("v400.plugs.weapons.masterworks.stat.range", "Range MW", WPN),
            PlugKind::Masterwork
        );
        assert_eq!(
            classify_plug(
                "v400.plugs.weapons.masterworks.trackers",
                "Kill Tracker",
                WPN
            ),
            PlugKind::Masterwork
        );
        assert_eq!(
            classify_plug("weapon_tiering.plugs.mods.enhancers", "Enhance", WPN),
            PlugKind::Masterwork
        );
        assert_eq!(
            classify_plug("mementos", "Gambit Memento", WPN),
            PlugKind::Cosmetic
        );
        assert_eq!(
            classify_plug("weapon_tiering_kill_vfx", "Kill Effect", WPN),
            PlugKind::Cosmetic
        );
        assert_eq!(
            classify_plug("crafting.plugs.weapons.mods.memories", "Memory", WPN),
            PlugKind::Crafting
        );
        assert_eq!(
            classify_plug("exotic_all_skins", "Ornament", WPN),
            PlugKind::Ornament
        );
    }

    #[test]
    fn armor_plugs() {
        assert_eq!(
            classify_plug("armor_stats", "", ARM),
            PlugKind::ArmorStatRoll
        );
        assert_eq!(
            classify_plug("intrinsics", "Spirit of Inmost Light", ARM),
            PlugKind::ArmorPerk
        );
        assert_eq!(
            classify_plug("armor_archetypes", "Grenadier", ARM),
            PlugKind::ArmorArchetype
        );
        assert_eq!(
            classify_plug("enhancements.v2_general", "Grenade Mod", ARM),
            PlugKind::ArmorStatMod
        );
        assert_eq!(
            classify_plug(
                "core.gear_systems.armor_tiering.plugs.tuning.mods",
                "+Class / -Health",
                ARM
            ),
            PlugKind::ArmorTuning
        );
        assert_eq!(
            classify_plug("enhancements.artifice", "Grenade Forged", ARM),
            PlugKind::ArmorTuning
        );
        assert_eq!(
            classify_plug(
                "enhancements.artifice.exotic",
                "Upgrade to Artifice Armor",
                ARM
            ),
            PlugKind::Masterwork
        );
        assert_eq!(
            classify_plug("enhancements.v2_arms", "Heavy Handed", ARM),
            PlugKind::ArmorMod
        );
        assert_eq!(
            classify_plug("enhancements.raid_v720", "Raid Mod", ARM),
            PlugKind::ArmorMod
        );
        assert_eq!(
            classify_plug("v460.plugs.armor.masterworks", "Upgrade Armor", ARM),
            PlugKind::Masterwork
        );
        assert_eq!(
            classify_plug("armor_skins_warlock_head", "Some Ornament", ARM),
            PlugKind::Ornament
        );
        assert_eq!(
            classify_plug("armor_skins_empty", "Default Ornament", ARM),
            PlugKind::Empty
        );
        assert_eq!(classify_plug("shader", "Superblack", ARM), PlugKind::Shader);
        assert_eq!(
            classify_plug("enhancements.v2_general", "Empty Mod Socket", ARM),
            PlugKind::Empty
        );
    }

    #[test]
    fn subclass_rules_do_not_leak_into_gear() {
        // A hypothetical weapon category mentioning grenades is not an ability.
        assert_ne!(
            classify_plug("grenade_launcher_frames", "Frame", WPN),
            PlugKind::Grenade
        );
        // Unknown host still classifies subclass plugs (manifest dimension).
        assert_eq!(
            classify_plug("titan.arc.supers", "Thundercrash", SlotGroup::Other),
            PlugKind::Super
        );
    }

    #[test]
    fn artifact_plugs_classify_by_host() {
        assert_eq!(
            classify_plug(
                "artifact_perks",
                "Anti-Barrier Auto Rifle",
                SlotGroup::Artifact
            ),
            PlugKind::ArtifactPerk
        );
        assert_eq!(
            classify_plug("something_new", "Perk", SlotGroup::Artifact),
            PlugKind::ArtifactPerk
        );
    }

    #[test]
    fn relevance_partitions_kinds() {
        let relevant: Vec<_> = PlugKind::ALL
            .iter()
            .filter(|k| k.is_build_relevant())
            .collect();
        assert!(relevant.contains(&&PlugKind::Aspect));
        assert!(relevant.contains(&&PlugKind::ArmorPerk));
        assert!(!PlugKind::ArmorStatRoll.is_build_relevant());
        assert!(!PlugKind::Other.is_build_relevant());
        // Storage names are unique.
        let mut names: Vec<_> = PlugKind::ALL.iter().map(|k| k.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), PlugKind::ALL.len());
    }
}
