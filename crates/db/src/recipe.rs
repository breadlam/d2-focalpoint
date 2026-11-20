//! Exact recipes: one build's equipment, as players equipped it, with names
//! interned in a [`Dict`]. Shared by the ranked book (`site`) and the
//! catalog of every build (`catalog`).

use std::collections::HashMap;

use serde::{Serialize, Serializer};

use crate::{Database, Result};

pub(crate) const ARMOR_SLOTS: [&str; 5] = ["helmet", "gauntlets", "chest", "legs", "class_item"];
pub(crate) const WEAPON_SLOTS: [&str; 3] = ["kinetic", "energy", "power"];

/// One exact build, as equipped. Numbers are indices into `dict`.
#[derive(Debug, Default, Clone)]
pub struct Recipe {
    /// The build id, as a string (ids exceed JavaScript's safe integers).
    pub id: String,
    /// Final stats: Weapons, Health, Class, Grenade, Super, Melee.
    pub stats: Option<[i32; 6]>,
    pub kit: Kit,
    pub armor: Vec<ArmorPiece>,
    pub weapons: Vec<Weapon>,
    /// Armor sets with the bonuses active in this build.
    pub sets: Vec<SetEntry>,
    /// The artifact item ("Hunter's Journal") and its applied perks.
    pub artifact_item: Option<u32>,
    pub artifact: Vec<u32>,
    /// The artifact perks are the character's *unlocked* perks (older
    /// captures recorded no applied ones), not the applied perks.
    pub artifact_unlocked: bool,
}

/// An armor set: how many pieces are equipped and the bonuses they activate.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SetEntry {
    pub set: u32,
    pub pieces: u16,
    pub bonuses: Vec<u32>,
}

/// `[set, pieces, [bonus]]`
impl Serialize for SetEntry {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        (self.set, self.pieces, &self.bonuses).serialize(s)
    }
}

/// One dictionary entry: name, icon, the item's type ("Pulse Rifle",
/// "Stasis Fragment") and a weapon's damage type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictEntry {
    pub name: String,
    pub icon: Option<String>,
    pub kind: Option<String>,
    pub damage: Option<String>,
}

/// `[name, icon, type, damage type]`
impl Serialize for DictEntry {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        (&self.name, &self.icon, &self.kind, &self.damage).serialize(s)
    }
}

#[derive(Debug, Default, Clone)]
pub struct Kit {
    pub subclass: Option<u32>,
    pub super_: Option<u32>,
    pub class_ability: Option<u32>,
    pub movement: Option<u32>,
    pub melee: Option<u32>,
    pub grenade: Option<u32>,
    pub aspects: Vec<u32>,
    pub fragments: Vec<u32>,
    pub transcendence: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct ArmorPiece {
    pub slot: &'static str,
    pub item: u32,
    pub exotic: bool,
    pub archetype: Option<u32>,
    pub tuning: Option<u32>,
    pub stat_mods: Vec<u32>,
    /// One entry per copy (the same mod can be socketed more than once).
    pub mods: Vec<u32>,
    /// Exotic intrinsics and rolled class item perks.
    pub perks: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct Weapon {
    pub slot: &'static str,
    pub item: u32,
    pub exotic: bool,
    pub frame: Option<u32>,
    pub perks: Vec<u32>,
    pub origin: Option<u32>,
    pub mod_: Option<u32>,
}

// Recipes are the bulk of the export, so they serialize as positional
// arrays (documented in docs/site.md) rather than objects with keys.

fn slot_index(slots: &[&str], slot: &str) -> usize {
    slots.iter().position(|s| *s == slot).unwrap_or(usize::MAX)
}

/// `[id, stats, kit, armor, weapons, sets, artifact item, artifact perks,
/// artifact perks are unlocked rather than applied (0/1)]`
impl Serialize for Recipe {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        (
            &self.id,
            &self.stats,
            &self.kit,
            &self.armor,
            &self.weapons,
            &self.sets,
            self.artifact_item,
            &self.artifact,
            u8::from(self.artifact_unlocked),
        )
            .serialize(s)
    }
}

/// `[subclass, super, class ability, movement, melee, grenade, aspects,
/// fragments, transcendence]`
impl Serialize for Kit {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        (
            self.subclass,
            self.super_,
            self.class_ability,
            self.movement,
            self.melee,
            self.grenade,
            &self.aspects,
            &self.fragments,
            &self.transcendence,
        )
            .serialize(s)
    }
}

/// `[slot (0 helmet … 4 class item), item, exotic (0/1), archetype, tuning,
/// stat mods, mods, perks]`
impl Serialize for ArmorPiece {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        (
            slot_index(&ARMOR_SLOTS, self.slot),
            self.item,
            u8::from(self.exotic),
            self.archetype,
            self.tuning,
            &self.stat_mods,
            &self.mods,
            &self.perks,
        )
            .serialize(s)
    }
}

/// `[slot (0 kinetic, 1 energy, 2 power), item, exotic (0/1), frame, perks,
/// origin trait, mod]`
impl Serialize for Weapon {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        (
            slot_index(&WEAPON_SLOTS, self.slot),
            self.item,
            u8::from(self.exotic),
            self.frame,
            &self.perks,
            self.origin,
            self.mod_,
        )
            .serialize(s)
    }
}

/// Interns names for recipes: `(namespace, hash)` → index into `dict`.
#[derive(Default)]
pub(crate) struct Dict {
    index: HashMap<(u8, u32), u32>,
    keys: Vec<(u8, u32)>,
}

pub(crate) const NS_ITEM: u8 = 0;
pub(crate) const NS_PERK: u8 = 1;
pub(crate) const NS_SET: u8 = 2;

impl Dict {
    pub(crate) fn id(&mut self, ns: u8, hash: u32) -> u32 {
        let next = self.keys.len() as u32;
        *self.index.entry((ns, hash)).or_insert_with(|| {
            self.keys.push((ns, hash));
            next
        })
    }

    /// Resolve every entry's name, icon, type and damage type.
    pub(crate) fn resolve(&self, db: &Database) -> Result<Vec<DictEntry>> {
        let mut names: HashMap<(u8, u32), DictEntry> = HashMap::new();
        for (ns, sql) in [
            (
                NS_ITEM,
                "SELECT item_hash, name, icon, nullif(item_type_name, ''),
                        CASE WHEN item_type = 3 THEN damage_type END
                 FROM item",
            ),
            (
                NS_PERK,
                "SELECT perk_hash, name, icon, 'Set bonus', NULL FROM sandbox_perk",
            ),
            (
                NS_SET,
                "SELECT set_hash, name, NULL, 'Armor set', NULL FROM item_set",
            ),
        ] {
            let mut stmt = db.conn().prepare(sql)?;
            let mut rows = stmt.query([])?;
            while let Some(r) = rows.next()? {
                let hash: u32 = r.get(0)?;
                if self.index.contains_key(&(ns, hash)) {
                    names.insert(
                        (ns, hash),
                        DictEntry {
                            name: r.get(1)?,
                            icon: r.get(2)?,
                            kind: r.get(3)?,
                            damage: r.get(4)?,
                        },
                    );
                }
            }
        }
        Ok(self
            .keys
            .iter()
            .map(|k| {
                names.remove(k).unwrap_or_else(|| DictEntry {
                    name: format!("Unknown ({})", k.1),
                    icon: None,
                    kind: None,
                    damage: None,
                })
            })
            .collect())
    }
}

fn slot_name(slot: &str, slots: &[&'static str]) -> Option<&'static str> {
    slots.iter().copied().find(|s| *s == slot)
}

/// Load the exact recipes of `ids`, in that order.
pub(crate) fn recipes(db: &Database, ids: &[i64], dict: &mut Dict) -> Result<Vec<Recipe>> {
    let mut by_id: HashMap<i64, Recipe> = ids
        .iter()
        .map(|&id| {
            (
                id,
                Recipe {
                    id: id.to_string(),
                    ..Recipe::default()
                },
            )
        })
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let list: Vec<String> = ids.iter().map(i64::to_string).collect();
    let list = list.join(", ");

    let mut stmt = db.conn().prepare(&format!(
        "SELECT build_id, stat_weapons, stat_health, stat_class, stat_grenade, stat_super, stat_melee
         FROM build WHERE build_id IN ({list})"
    ))?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let stats: [Option<i32>; 6] = std::array::from_fn(|i| r.get(i + 1).ok().flatten());
        if let (Some(recipe), true) = (by_id.get_mut(&id), stats.iter().all(Option::is_some)) {
            recipe.stats = Some(stats.map(|s| s.unwrap_or(0)));
        }
    }

    // Components in socket order. Children (mods, perks) follow their
    // parent's slot; parents come first within a slot.
    let mut stmt = db
        .conn()
        .prepare(&components_sql(&format!("bc.build_id IN ({list})")))?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let row = ComponentRow::read(r)?;
        if let Some(recipe) = by_id.get_mut(&row.build_id) {
            recipe.add(dict, &row);
        }
    }
    Ok(ids
        .iter()
        .filter_map(|id| by_id.remove(id))
        .map(|mut r| {
            r.finish();
            r
        })
        .collect())
}

/// One `build_component` row, as [`components_sql`] selects it.
pub(crate) struct ComponentRow {
    pub build_id: i64,
    pub kind: String,
    pub slot: String,
    pub hash: u32,
    pub quantity: i32,
    pub exotic: bool,
    pub parent: Option<u32>,
    pub position: i32,
}

impl ComponentRow {
    pub(crate) fn read(r: &duckdb::Row<'_>) -> duckdb::Result<Self> {
        Ok(Self {
            build_id: r.get(0)?,
            kind: r.get(1)?,
            slot: r.get(2)?,
            hash: r.get(3)?,
            quantity: r.get(4)?,
            exotic: r.get(5)?,
            parent: r.get(6)?,
            position: r.get(7)?,
        })
    }
}

impl Recipe {
    /// Add one `build_component` row. Parents (armor pieces, weapons) must
    /// come before their mods and perks — see [`components_sql`].
    pub(crate) fn add(&mut self, dict: &mut Dict, row: &ComponentRow) {
        let ComponentRow {
            kind,
            slot,
            hash,
            quantity,
            exotic,
            parent,
            position,
            ..
        } = row;
        let (kind, slot, hash, quantity, exotic, parent) = (
            kind.as_str(),
            slot.as_str(),
            *hash,
            *quantity,
            *exotic,
            *parent,
        );
        let recipe = self;
        let k = &mut recipe.kit;
        match kind {
            "subclass" => k.subclass = Some(dict.id(NS_ITEM, hash)),
            "super" => k.super_ = Some(dict.id(NS_ITEM, hash)),
            "class_ability" => k.class_ability = Some(dict.id(NS_ITEM, hash)),
            "movement" => k.movement = Some(dict.id(NS_ITEM, hash)),
            "melee" => k.melee = Some(dict.id(NS_ITEM, hash)),
            "grenade" => k.grenade = Some(dict.id(NS_ITEM, hash)),
            "aspect" => k.aspects.push(dict.id(NS_ITEM, hash)),
            "fragment" => k.fragments.push(dict.id(NS_ITEM, hash)),
            "transcendence" => k.transcendence.push(dict.id(NS_ITEM, hash)),
            "artifact_perk" => {
                let d = dict.id(NS_ITEM, hash);
                if !recipe.artifact.contains(&d) {
                    recipe.artifact.push(d);
                }
                // Perks from the progression list carry no socket position.
                if i32::from(focalpoint_core::Component::UNSOCKETED) == *position {
                    recipe.artifact_unlocked = true;
                }
            }
            "artifact" => recipe.artifact_item = Some(dict.id(NS_ITEM, hash)),
            "armor_set" => {
                let set = dict.id(NS_SET, hash);
                recipe.set_entry(set).pieces = u16::try_from(quantity).unwrap_or(0);
            }
            "set_bonus" => {
                // A bonus's parent is its set.
                let bonus = dict.id(NS_PERK, hash);
                let set = parent.map_or(u32::MAX, |p| dict.id(NS_SET, p));
                recipe.set_entry(set).bonuses.push(bonus);
            }
            "armor" => {
                if let Some(slot) = slot_name(slot, &ARMOR_SLOTS) {
                    recipe.armor.push(ArmorPiece {
                        slot,
                        item: dict.id(NS_ITEM, hash),
                        exotic,
                        archetype: None,
                        tuning: None,
                        stat_mods: Vec::new(),
                        mods: Vec::new(),
                        perks: Vec::new(),
                    });
                }
            }
            "weapon" => {
                if let Some(slot) = slot_name(slot, &WEAPON_SLOTS) {
                    recipe.weapons.push(Weapon {
                        slot,
                        item: dict.id(NS_ITEM, hash),
                        exotic,
                        frame: None,
                        perks: Vec::new(),
                        origin: None,
                        mod_: None,
                    });
                }
            }
            "armor_mod" | "armor_stat_mod" | "armor_tuning" | "armor_archetype" | "armor_perk" => {
                let d = dict.id(NS_ITEM, hash);
                if let Some(piece) = recipe.armor.iter_mut().find(|p| p.slot == slot) {
                    match kind {
                        "armor_mod" => {
                            for _ in 0..quantity.max(1) {
                                piece.mods.push(d);
                            }
                        }
                        "armor_stat_mod" => piece.stat_mods.push(d),
                        "armor_tuning" => piece.tuning = Some(d),
                        "armor_archetype" => piece.archetype = Some(d),
                        _ => piece.perks.push(d),
                    }
                }
            }
            "weapon_intrinsic" | "weapon_perk" | "origin_trait" | "weapon_mod" => {
                let d = dict.id(NS_ITEM, hash);
                if let Some(w) = recipe.weapons.iter_mut().find(|w| w.slot == slot) {
                    match kind {
                        "weapon_intrinsic" => w.frame = Some(d),
                        "origin_trait" => w.origin = Some(d),
                        "weapon_mod" => w.mod_ = Some(d),
                        _ => {
                            if !w.perks.contains(&d) {
                                w.perks.push(d);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// The entry of an armor set, created on first mention.
    fn set_entry(&mut self, set: u32) -> &mut SetEntry {
        let i = if let Some(i) = self.sets.iter().position(|e| e.set == set) {
            i
        } else {
            self.sets.push(SetEntry {
                set,
                pieces: 0,
                bonuses: Vec::new(),
            });
            self.sets.len() - 1
        };
        &mut self.sets[i]
    }

    /// Put armor and weapons in slot order.
    pub(crate) fn finish(&mut self) {
        let order =
            |slots: &[&str], s: &str| slots.iter().position(|x| *x == s).unwrap_or(usize::MAX);
        self.armor.sort_by_key(|p| order(&ARMOR_SLOTS, p.slot));
        self.weapons.sort_by_key(|w| order(&WEAPON_SLOTS, w.slot));
    }
}

/// Components of the builds matching `condition` (over `bc`), in the order
/// [`Recipe::add`] needs: by build, parents first, then socket order.
pub(crate) fn components_sql(condition: &str) -> String {
    format!(
        "SELECT bc.build_id, bc.kind, coalesce(bc.slot, ''), bc.component_hash, bc.quantity,
                coalesce(i.is_exotic, false), bc.parent_hash, bc.position
         FROM build_component bc LEFT JOIN item i ON i.item_hash = bc.component_hash
         WHERE {condition}
         ORDER BY bc.build_id, bc.kind IN ('armor', 'weapon') DESC, bc.position, bc.kind, bc.component_hash"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipes_serialize_positionally() {
        let w = Weapon {
            slot: "energy",
            item: 5,
            exotic: false,
            frame: Some(6),
            perks: vec![7, 8],
            origin: None,
            mod_: Some(9),
        };
        assert_eq!(serde_json::to_string(&w).unwrap(), "[1,5,0,6,[7,8],null,9]");
        let a = ArmorPiece {
            slot: "class_item",
            item: 1,
            exotic: true,
            archetype: Some(2),
            tuning: None,
            stat_mods: vec![3],
            mods: vec![4, 4],
            perks: vec![],
        };
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            "[4,1,1,2,null,[3],[4,4],[]]"
        );
        let r = Recipe {
            id: "10".into(),
            ..Recipe::default()
        };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"["10",null,[null,null,null,null,null,null,[],[],[]],[],[],[],null,[],0]"#
        );
    }
}
