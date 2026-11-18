//! Build extraction and identity.
//!
//! A *loadout* is everything a character had equipped (including cosmetics and
//! specific item instances). A *build* is the gameplay-relevant projection of
//! a loadout: subclass configuration, weapons and their perks, armor and its
//! mods/perks, armor set bonuses, and artifact perks. Two loadouts that differ
//! only in shaders, ornaments, masterworks, armor stat rolls, or which copy of
//! an item is equipped are the same build.
//!
//! Identity rules are documented in `docs/adr/0002-build-identity.md`.

use std::collections::BTreeMap;
use std::fmt;

use crate::catalog::Catalog;
use crate::class::{Element, GuardianClass};
use crate::classify::PlugKind;
use crate::fingerprint::Fingerprinter;
use crate::loadout::LoadoutState;
use crate::slot::{Slot, SlotGroup};

/// Version of the build identity rules. Bump whenever [`extract_build`] would
/// assign a different `build_id` to the same loadout.
pub const BUILD_FINGERPRINT_VERSION: u8 = 1;

/// Version of the exact-loadout fingerprint ([`loadout_id`]).
pub const LOADOUT_FINGERPRINT_VERSION: u8 = 1;

/// What role a component plays in a build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ComponentKind {
    Subclass,
    Super,
    ClassAbility,
    Movement,
    Melee,
    Grenade,
    Aspect,
    Fragment,
    Transcendence,
    Weapon,
    WeaponIntrinsic,
    WeaponPerk,
    OriginTrait,
    WeaponMod,
    Armor,
    ArmorPerk,
    ArmorArchetype,
    ArmorMod,
    ArmorStatMod,
    ArmorTuning,
    /// An armor set with `quantity` pieces equipped; `hash` is the set hash.
    ArmorSet,
    /// An active set bonus; `hash` is a sandbox perk, `parent_hash` the set.
    /// (The piece count that unlocks it is in the set definition.)
    SetBonus,
    Artifact,
    ArtifactPerk,
}

impl ComponentKind {
    pub const ALL: [ComponentKind; 24] = [
        ComponentKind::Subclass,
        ComponentKind::Super,
        ComponentKind::ClassAbility,
        ComponentKind::Movement,
        ComponentKind::Melee,
        ComponentKind::Grenade,
        ComponentKind::Aspect,
        ComponentKind::Fragment,
        ComponentKind::Transcendence,
        ComponentKind::Weapon,
        ComponentKind::WeaponIntrinsic,
        ComponentKind::WeaponPerk,
        ComponentKind::OriginTrait,
        ComponentKind::WeaponMod,
        ComponentKind::Armor,
        ComponentKind::ArmorPerk,
        ComponentKind::ArmorArchetype,
        ComponentKind::ArmorMod,
        ComponentKind::ArmorStatMod,
        ComponentKind::ArmorTuning,
        ComponentKind::ArmorSet,
        ComponentKind::SetBonus,
        ComponentKind::Artifact,
        ComponentKind::ArtifactPerk,
    ];

    /// Map a build-relevant plug kind to its component kind.
    #[must_use]
    pub fn from_plug(kind: PlugKind) -> Option<Self> {
        Some(match kind {
            PlugKind::Super => Self::Super,
            PlugKind::ClassAbility => Self::ClassAbility,
            PlugKind::Movement => Self::Movement,
            PlugKind::Melee => Self::Melee,
            PlugKind::Grenade => Self::Grenade,
            PlugKind::Aspect => Self::Aspect,
            PlugKind::Fragment => Self::Fragment,
            PlugKind::Transcendence => Self::Transcendence,
            PlugKind::WeaponIntrinsic => Self::WeaponIntrinsic,
            PlugKind::WeaponPerk => Self::WeaponPerk,
            PlugKind::OriginTrait => Self::OriginTrait,
            PlugKind::WeaponMod => Self::WeaponMod,
            PlugKind::ArmorPerk => Self::ArmorPerk,
            PlugKind::ArmorArchetype => Self::ArmorArchetype,
            PlugKind::ArmorMod => Self::ArmorMod,
            PlugKind::ArmorStatMod => Self::ArmorStatMod,
            PlugKind::ArmorTuning => Self::ArmorTuning,
            PlugKind::ArtifactPerk => Self::ArtifactPerk,
            _ => return None,
        })
    }

    /// Is `hash` an inventory item hash (vs. a set or sandbox-perk hash)?
    #[must_use]
    pub fn is_item_hash(self) -> bool {
        !matches!(self, Self::ArmorSet | Self::SetBonus)
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Subclass => "subclass",
            Self::Super => "super",
            Self::ClassAbility => "class_ability",
            Self::Movement => "movement",
            Self::Melee => "melee",
            Self::Grenade => "grenade",
            Self::Aspect => "aspect",
            Self::Fragment => "fragment",
            Self::Transcendence => "transcendence",
            Self::Weapon => "weapon",
            Self::WeaponIntrinsic => "weapon_intrinsic",
            Self::WeaponPerk => "weapon_perk",
            Self::OriginTrait => "origin_trait",
            Self::WeaponMod => "weapon_mod",
            Self::Armor => "armor",
            Self::ArmorPerk => "armor_perk",
            Self::ArmorArchetype => "armor_archetype",
            Self::ArmorMod => "armor_mod",
            Self::ArmorStatMod => "armor_stat_mod",
            Self::ArmorTuning => "armor_tuning",
            Self::ArmorSet => "armor_set",
            Self::SetBonus => "set_bonus",
            Self::Artifact => "artifact",
            Self::ArtifactPerk => "artifact_perk",
        }
    }

    /// Stable fingerprint code. Never reorder or reuse values.
    fn code(self) -> u8 {
        match self {
            Self::Subclass => 1,
            Self::Super => 2,
            Self::ClassAbility => 3,
            Self::Movement => 4,
            Self::Melee => 5,
            Self::Grenade => 6,
            Self::Aspect => 7,
            Self::Fragment => 8,
            Self::Transcendence => 9,
            Self::Weapon => 10,
            Self::WeaponIntrinsic => 11,
            Self::WeaponPerk => 12,
            Self::OriginTrait => 13,
            Self::WeaponMod => 14,
            Self::Armor => 15,
            Self::ArmorPerk => 16,
            Self::ArmorArchetype => 17,
            Self::ArmorMod => 18,
            Self::ArmorStatMod => 19,
            Self::ArmorTuning => 20,
            Self::ArmorSet => 21,
            Self::SetBonus => 22,
            Self::Artifact => 23,
            Self::ArtifactPerk => 24,
        }
    }
}

impl fmt::Display for ComponentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One element of a build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Component {
    pub kind: ComponentKind,
    /// The slot the component lives in (`None` for build-wide components such
    /// as armor sets and set bonuses).
    pub slot: Option<Slot>,
    pub hash: u32,
    /// The item a plug is socketed into, or the set a set bonus belongs to.
    pub parent_hash: Option<u32>,
    /// Multiplicity: copies of the same plug on one item (e.g. two identical
    /// armor mods), or pieces equipped for an [`ComponentKind::ArmorSet`].
    pub quantity: u16,
    /// Socket index of the first occurrence (display order). Not identity.
    pub position: u16,
}

impl Component {
    fn identity_key(&self) -> (u8, u8, u32, u32, u16) {
        (
            self.kind.code(),
            self.slot.map_or(0, Slot::code),
            self.hash,
            self.parent_hash.unwrap_or(0),
            self.quantity,
        )
    }
}

/// A build extracted from a loadout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    /// Content fingerprint of [`Build::components`] (see module docs).
    pub id: i64,
    pub class: GuardianClass,
    pub element: Option<Element>,
    pub exotic_armor_hash: Option<u32>,
    pub exotic_weapon_hash: Option<u32>,
    /// Canonically ordered components (identity order).
    pub components: Vec<Component>,
    /// Plug hashes that were not in the catalog and so could not be classified.
    pub unresolved_hashes: Vec<u32>,
}

impl Build {
    /// Hashes of all components of `kind`, in canonical order.
    pub fn hashes(&self, kind: ComponentKind) -> impl Iterator<Item = u32> + '_ {
        self.components
            .iter()
            .filter(move |c| c.kind == kind)
            .flat_map(|c| std::iter::repeat_n(c.hash, usize::from(c.quantity.max(1))))
    }

    /// The single component of `kind` (first, if several).
    #[must_use]
    pub fn first(&self, kind: ComponentKind) -> Option<u32> {
        self.components
            .iter()
            .find(|c| c.kind == kind)
            .map(|c| c.hash)
    }

    /// The item equipped in a slot.
    #[must_use]
    pub fn item_in(&self, slot: Slot) -> Option<u32> {
        self.components
            .iter()
            .find(|c| {
                c.slot == Some(slot)
                    && matches!(
                        c.kind,
                        ComponentKind::Subclass
                            | ComponentKind::Weapon
                            | ComponentKind::Armor
                            | ComponentKind::Artifact
                    )
            })
            .map(|c| c.hash)
    }

    /// Distinct inventory-item hashes of every component (for set-membership
    /// facet filters). Excludes armor-set and set-bonus hashes, which live in
    /// different hash namespaces.
    #[must_use]
    pub fn item_hashes(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self
            .components
            .iter()
            .filter(|c| c.kind.is_item_hash())
            .map(|c| c.hash)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// Accumulates components, merging duplicates into a quantity.
/// `(kind, slot, hash, parent)` → `(quantity, first socket position)`.
type ComponentKey = (ComponentKind, Option<Slot>, u32, Option<u32>);

#[derive(Default)]
struct ComponentSet(BTreeMap<ComponentKey, (u16, u16)>);

impl ComponentSet {
    fn add(
        &mut self,
        kind: ComponentKind,
        slot: Option<Slot>,
        hash: u32,
        parent: Option<u32>,
        position: u16,
    ) {
        let e = self
            .0
            .entry((kind, slot, hash, parent))
            .or_insert((0, position));
        e.0 = e.0.saturating_add(1);
    }

    fn add_with_quantity(
        &mut self,
        kind: ComponentKind,
        slot: Option<Slot>,
        hash: u32,
        parent: Option<u32>,
        quantity: u16,
    ) {
        self.0.insert((kind, slot, hash, parent), (quantity, 0));
    }

    fn into_canonical(self) -> Vec<Component> {
        let mut v: Vec<Component> = self
            .0
            .into_iter()
            .map(
                |((kind, slot, hash, parent_hash), (quantity, position))| Component {
                    kind,
                    slot,
                    hash,
                    parent_hash,
                    quantity,
                    position,
                },
            )
            .collect();
        v.sort_by_key(Component::identity_key);
        v
    }
}

/// Project a loadout to its build.
#[must_use]
pub fn extract_build(loadout: &LoadoutState, catalog: &Catalog) -> Build {
    let mut set = ComponentSet::default();
    let mut unresolved = Vec::new();
    let mut class = GuardianClass::Unknown;
    let mut armor_class = GuardianClass::Unknown;
    let mut element = None;
    let mut exotic_armor_hash = None;
    let mut exotic_weapon_hash = None;
    let mut set_pieces: BTreeMap<u32, u16> = BTreeMap::new();

    for item in &loadout.items {
        let slot = item.slot();
        let group = slot.group();
        let item_kind = match group {
            SlotGroup::Subclass => ComponentKind::Subclass,
            SlotGroup::Weapon => ComponentKind::Weapon,
            SlotGroup::Armor => ComponentKind::Armor,
            SlotGroup::Artifact => ComponentKind::Artifact,
            SlotGroup::Cosmetic | SlotGroup::Other => continue,
        };
        set.add(item_kind, Some(slot), item.item_hash, None, 0);

        if let Some(def) = catalog.item(item.item_hash) {
            match group {
                SlotGroup::Subclass => {
                    class = def.class();
                    element = def.subclass_element();
                }
                SlotGroup::Weapon if def.is_exotic() => exotic_weapon_hash = Some(item.item_hash),
                SlotGroup::Armor => {
                    if def.is_exotic() {
                        exotic_armor_hash = Some(item.item_hash);
                    }
                    if armor_class == GuardianClass::Unknown {
                        armor_class = def.class();
                    }
                    if let Some(s) = def.item_set_hash {
                        *set_pieces.entry(s).or_default() += 1;
                    }
                }
                _ => {}
            }
        } else {
            unresolved.push(item.item_hash);
        }

        // Plugs hang off their item, except subclass plugs, which are the
        // subclass itself (parent would be redundant with the slot).
        let parent = match group {
            SlotGroup::Subclass => None,
            _ => Some(item.item_hash),
        };
        for (pos, &plug) in item.plug_hashes.iter().enumerate() {
            let Some(def) = catalog.item(plug) else {
                unresolved.push(plug);
                continue;
            };
            if let Some(kind) = ComponentKind::from_plug(def.plug_kind(group)) {
                set.add(kind, Some(slot), plug, parent, pos as u16);
            }
        }
    }

    // Artifact perks reported via progressions (older captures) complement the
    // artifact item's socketed perks; merge without double counting.
    if let Some(artifact) = &loadout.artifact {
        let artifact_item = loadout.item_in(Slot::Artifact).map(|i| i.item_hash);
        for &perk in &artifact.active_perks {
            let key = (
                ComponentKind::ArtifactPerk,
                Some(Slot::Artifact),
                perk,
                artifact_item,
            );
            if !set.0.contains_key(&key) {
                set.add(
                    ComponentKind::ArtifactPerk,
                    Some(Slot::Artifact),
                    perk,
                    artifact_item,
                    u16::MAX,
                );
            }
        }
    }

    for (&set_hash, &pieces) in &set_pieces {
        set.add_with_quantity(ComponentKind::ArmorSet, None, set_hash, None, pieces);
        if let Some(def) = catalog.item_sets.get(&set_hash) {
            for &(required, perk) in &def.perks {
                if pieces >= u16::from(required) {
                    set.add(ComponentKind::SetBonus, None, perk, Some(set_hash), 0);
                }
            }
        }
    }

    if class == GuardianClass::Unknown {
        class = armor_class;
    }
    let components = set.into_canonical();
    unresolved.sort_unstable();
    unresolved.dedup();
    Build {
        id: build_fingerprint(&components),
        class,
        element,
        exotic_armor_hash,
        exotic_weapon_hash,
        components,
        unresolved_hashes: unresolved,
    }
}

/// Fingerprint canonically ordered components.
#[must_use]
pub fn build_fingerprint(components: &[Component]) -> i64 {
    let mut fp = Fingerprinter::new(b"searchlight.build", BUILD_FINGERPRINT_VERSION);
    for c in components {
        let (kind, slot, hash, parent, qty) = c.identity_key();
        fp.u8(kind).u8(slot).u32(hash).u32(parent).u16(qty);
    }
    fp.finish()
}

/// Fingerprint the exact loadout: every item (including cosmetics and the
/// specific item instance) with its plugs in socket order, plus the artifact.
/// Character stats are deliberately excluded; they are recorded per segment.
#[must_use]
pub fn loadout_id(loadout: &LoadoutState) -> i64 {
    let mut items: Vec<_> = loadout.items.iter().collect();
    items.sort_by_key(|i| (i.bucket_hash, i.item_hash, i.item_instance_id));
    let mut fp = Fingerprinter::new(b"searchlight.loadout", LOADOUT_FINGERPRINT_VERSION);
    fp.u32(items.len() as u32);
    for i in items {
        fp.u32(i.bucket_hash)
            .u32(i.item_hash)
            .u64(i.item_instance_id);
        fp.u32(i.plug_hashes.len() as u32);
        for &p in &i.plug_hashes {
            fp.u32(p);
        }
        let mut perks = i.perk_hashes.clone();
        perks.sort_unstable();
        fp.u32(perks.len() as u32);
        for p in perks {
            fp.u32(p);
        }
    }
    match &loadout.artifact {
        Some(a) => {
            let mut perks = a.active_perks.clone();
            perks.sort_unstable();
            fp.u8(1).u32(a.artifact_hash).u32(perks.len() as u32);
            for p in perks {
                fp.u32(p);
            }
        }
        None => {
            fp.u8(0);
        }
    }
    fp.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{ItemDef, ItemSetDef};
    use crate::loadout::{ArtifactState, EquippedItem};

    fn def(
        hash: u32,
        name: &str,
        bucket: u32,
        tier: u8,
        class_type: u8,
        cat: Option<&str>,
    ) -> ItemDef {
        ItemDef {
            hash,
            name: name.into(),
            description: String::new(),
            icon: None,
            item_type: 0,
            item_sub_type: 0,
            item_type_name: String::new(),
            tier_type: tier,
            tier_name: String::new(),
            class_type,
            bucket_hash: bucket,
            default_damage_type: 0,
            ammo_type: 0,
            breaker_type: 0,
            plug_category: cat.map(Into::into),
            stack_unique_label: (bucket == SUBCLASS)
                .then(|| "v610.builds.solar.warlock.build0".into()),
            hud_damage_type: 0,
            item_set_hash: None,
        }
    }

    const SUBCLASS: u32 = 3_284_755_031;
    const KINETIC: u32 = 1_498_876_634;
    const GAUNTLETS: u32 = 3_551_918_588;
    const HELMET: u32 = 3_448_274_439;
    const SHIP: u32 = 284_967_655;
    const ARTIFACT: u32 = 1_506_418_338;

    fn catalog() -> Catalog {
        let items = vec![
            def(100, "Dawnblade", SUBCLASS, 2, 2, None),
            def(
                101,
                "Well of Radiance",
                0,
                0,
                3,
                Some("warlock.solar.supers"),
            ),
            def(
                102,
                "Touch of Flame",
                0,
                0,
                3,
                Some("warlock.solar.aspects"),
            ),
            def(103, "Heat Rises", 0, 0, 3, Some("warlock.solar.aspects")),
            def(
                104,
                "Ember of Torches",
                0,
                0,
                3,
                Some("shared.solar.fragments"),
            ),
            def(
                105,
                "Healing Grenade",
                0,
                0,
                3,
                Some("shared.solar.grenades"),
            ),
            def(200, "Ace of Spades", KINETIC, 6, 3, None),
            def(201, "Memento Mori", 0, 0, 3, Some("frames")),
            def(202, "Default Shader", 0, 0, 3, Some("shader")),
            def(
                203,
                "Range MW",
                0,
                0,
                3,
                Some("v400.plugs.weapons.masterworks.stat.range"),
            ),
            def(300, "Sunbracers", GAUNTLETS, 6, 2, None),
            def(301, "Firepower", 0, 0, 3, Some("enhancements.v2_arms")),
            def(302, "Stat Roll", 0, 0, 3, Some("armor_stats")),
            def(400, "Set Helm", HELMET, 5, 2, None),
            def(500, "Ship", SHIP, 5, 3, None),
            def(600, "Artifact", ARTIFACT, 5, 3, None),
            def(
                601,
                "Anti-Barrier Auto Rifle",
                0,
                0,
                3,
                Some("artifact_perks"),
            ),
        ];
        let sets = vec![ItemSetDef {
            hash: 9000,
            name: "Set".into(),
            items: vec![400],
            perks: vec![(1, 77), (2, 78)],
        }];
        Catalog::new(items, sets)
    }

    fn item(hash: u32, inst: u64, bucket: u32, plugs: &[u32]) -> EquippedItem {
        EquippedItem {
            item_hash: hash,
            item_instance_id: inst,
            bucket_hash: bucket,
            plug_hashes: plugs.to_vec(),
            perk_hashes: vec![],
        }
    }

    fn loadout() -> LoadoutState {
        LoadoutState {
            items: vec![
                item(100, 1, SUBCLASS, &[101, 105, 102, 103, 104, 104]),
                item(200, 2, KINETIC, &[201, 202, 203]),
                item(300, 3, GAUNTLETS, &[301, 301, 302]),
                item(400, 4, HELMET, &[]),
                item(500, 5, SHIP, &[]),
                item(600, 6, ARTIFACT, &[601]),
            ],
            artifact: Some(ArtifactState {
                artifact_hash: 1,
                active_perks: vec![601],
            }),
            stats: BTreeMap::new(),
        }
    }

    #[test]
    fn extracts_subclass_gear_and_summary() {
        let b = extract_build(&loadout(), &catalog());
        assert_eq!(b.class, GuardianClass::Warlock);
        assert_eq!(b.element, Some(Element::Solar));
        assert_eq!(b.first(ComponentKind::Super), Some(101));
        assert_eq!(b.first(ComponentKind::Grenade), Some(105));
        assert_eq!(
            b.hashes(ComponentKind::Aspect).collect::<Vec<_>>(),
            vec![102, 103]
        );
        // Fragments keep multiplicity.
        assert_eq!(
            b.hashes(ComponentKind::Fragment).collect::<Vec<_>>(),
            vec![104, 104]
        );
        assert_eq!(b.exotic_weapon_hash, Some(200));
        assert_eq!(b.exotic_armor_hash, Some(300));
        assert_eq!(b.item_in(Slot::Kinetic), Some(200));
        assert!(b.unresolved_hashes.is_empty());
    }

    #[test]
    fn drops_noise_and_cosmetic_slots() {
        let b = extract_build(&loadout(), &catalog());
        let hashes = b.item_hashes();
        for noise in [202, 203, 302, 500] {
            assert!(
                !hashes.contains(&noise),
                "{noise} should not be in the build"
            );
        }
        // Weapon perk is attached to its weapon.
        let perk = b.components.iter().find(|c| c.hash == 201).unwrap();
        assert_eq!(perk.kind, ComponentKind::WeaponPerk);
        assert_eq!(perk.parent_hash, Some(200));
        // Duplicate armor mods merge into a quantity.
        let m = b.components.iter().find(|c| c.hash == 301).unwrap();
        assert_eq!(m.quantity, 2);
        // Artifact perk from sockets and progressions is counted once.
        assert_eq!(b.hashes(ComponentKind::ArtifactPerk).count(), 1);
    }

    #[test]
    fn armor_sets_and_bonuses() {
        let b = extract_build(&loadout(), &catalog());
        let set = b
            .components
            .iter()
            .find(|c| c.kind == ComponentKind::ArmorSet)
            .unwrap();
        assert_eq!((set.hash, set.quantity), (9000, 1));
        // One piece unlocks only the 1-piece bonus.
        let bonuses: Vec<_> = b.hashes(ComponentKind::SetBonus).collect();
        assert_eq!(bonuses, vec![77]);
    }

    #[test]
    fn identity_ignores_cosmetics_instances_order_and_stats() {
        let cat = catalog();
        let base = extract_build(&loadout(), &cat).id;

        let mut l = loadout();
        l.items.reverse();
        l.items[0].item_instance_id = 999; // a different copy of the same item
        l.stats.insert(1, 2);
        // Reordered aspects and a different shader.
        l.items
            .iter_mut()
            .find(|i| i.item_hash == 100)
            .unwrap()
            .plug_hashes = vec![103, 102, 101, 104, 105, 104];
        assert_eq!(extract_build(&l, &cat).id, base);
        assert_ne!(loadout_id(&l), loadout_id(&loadout()));
    }

    #[test]
    fn identity_changes_with_gameplay_choices() {
        let cat = catalog();
        let base = extract_build(&loadout(), &cat).id;
        let mut l = loadout();
        // Drop one fragment copy.
        l.items[0].plug_hashes.pop();
        assert_ne!(extract_build(&l, &cat).id, base);
        // Drop a weapon perk.
        let mut l = loadout();
        l.items[1].plug_hashes.retain(|&p| p != 201);
        assert_ne!(extract_build(&l, &cat).id, base);
    }

    #[test]
    fn unknown_hashes_are_reported_not_fatal() {
        let mut l = loadout();
        l.items[0].plug_hashes.push(424_242);
        let b = extract_build(&l, &catalog());
        assert_eq!(b.unresolved_hashes, vec![424_242]);
    }

    #[test]
    fn loadout_id_is_order_independent_across_items() {
        let mut l = loadout();
        let a = loadout_id(&l);
        l.items.reverse();
        assert_eq!(loadout_id(&l), a);
    }
}
