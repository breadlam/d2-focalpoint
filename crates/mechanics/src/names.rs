//! Game element names from the manifest, for the sources that name what
//! they document rather than give its hash.

use std::collections::HashMap;

use anyhow::Result;
use focalpoint_db::Database;

/// What a manifest name belongs to; a source's tab says which it lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    WeaponPerk,
    OriginTrait,
    /// Weapon frames and exotic weapon perks, and exotic armor perks.
    Intrinsic,
    WeaponMod,
    ArmorMod,
    ArtifactPerk,
    Fragment,
    Aspect,
    Super,
    Grenade,
    Melee,
    ClassAbility,
    Movement,
    Transcendence,
    ArmorArchetype,
    ArmorTuning,
    ExoticArmor,
    ExoticWeapon,
    SetBonus,
    ArmorSet,
}

impl Kind {
    fn from_plug_kind(s: &str) -> Option<Self> {
        Some(match s {
            "weapon_perk" => Kind::WeaponPerk,
            "origin_trait" => Kind::OriginTrait,
            "weapon_intrinsic" => Kind::Intrinsic,
            "weapon_mod" => Kind::WeaponMod,
            "armor_mod" => Kind::ArmorMod,
            "artifact_perk" => Kind::ArtifactPerk,
            "fragment" => Kind::Fragment,
            "aspect" => Kind::Aspect,
            "super" => Kind::Super,
            "grenade" => Kind::Grenade,
            "melee" => Kind::Melee,
            "class_ability" => Kind::ClassAbility,
            "movement" => Kind::Movement,
            "transcendence" => Kind::Transcendence,
            "armor_archetype" => Kind::ArmorArchetype,
            "armor_tuning" => Kind::ArmorTuning,
            _ => return None,
        })
    }
}

/// A manifest hash a name refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subject {
    pub hash: u32,
    /// `item`, `sandbox_perk` (set bonuses) or `item_set`.
    pub hash_kind: &'static str,
    pub kind: Kind,
}

/// Fragment names begin with their element's word: "Ember of Ashes" is
/// listed as "Ashes" on the Solar tab.
const FRAGMENT_WORDS: [(&str, &str); 6] = [
    ("Arc", "spark"),
    ("Solar", "ember"),
    ("Void", "echo"),
    ("Stasis", "whisper"),
    ("Strand", "thread"),
    ("Prismatic", "facet"),
];

/// Names, normalised, to what they refer to.
#[derive(Debug, Default)]
pub struct Names {
    by_name: HashMap<String, Vec<Subject>>,
    /// `(element, short fragment name)` to fragments.
    fragments: HashMap<(String, String), Vec<Subject>>,
}

impl Names {
    /// Every named plug of a kind a source documents, exotic armor and
    /// weapons, armor sets and their bonuses.
    pub fn load(db: &Database) -> Result<Self> {
        let mut names = Self::default();
        let mut stmt = db.conn().prepare(
            "SELECT item_hash, name, plug_kind, is_exotic AND item_type = 2, is_exotic AND item_type = 3
             FROM item WHERE name IS NOT NULL AND name <> ''",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let hash: u32 = r.get(0)?;
            let name: String = r.get(1)?;
            let kind = match r.get::<_, Option<String>>(2)? {
                Some(k) => Kind::from_plug_kind(&k),
                None if r.get::<_, Option<bool>>(3)?.unwrap_or(false) => Some(Kind::ExoticArmor),
                None if r.get::<_, Option<bool>>(4)?.unwrap_or(false) => Some(Kind::ExoticWeapon),
                None => None,
            };
            if let Some(kind) = kind {
                names.add(
                    &name,
                    Subject {
                        hash,
                        hash_kind: "item",
                        kind,
                    },
                );
            }
        }
        for (sql, hash_kind, kind) in [
            (
                "SELECT DISTINCT p.perk_hash, p.name FROM sandbox_perk p JOIN item_set_perk s USING (perk_hash)
                 WHERE p.name IS NOT NULL",
                "sandbox_perk",
                Kind::SetBonus,
            ),
            (
                "SELECT set_hash, name FROM item_set WHERE name IS NOT NULL",
                "item_set",
                Kind::ArmorSet,
            ),
        ] {
            let mut stmt = db.conn().prepare(sql)?;
            let mut rows = stmt.query([])?;
            while let Some(r) = rows.next()? {
                let name: String = r.get(1)?;
                names.add(&name, Subject { hash: r.get(0)?, hash_kind, kind });
            }
        }
        Ok(names)
    }

    fn add(&mut self, name: &str, subject: Subject) {
        let key = normalize(name);
        if key.is_empty() {
            return;
        }
        if subject.kind == Kind::Fragment {
            for (element, word) in FRAGMENT_WORDS {
                if let Some(short) = key.strip_prefix(word).and_then(|s| s.strip_prefix(" of ")) {
                    push(
                        self.fragments
                            .entry((element.to_string(), short.to_string()))
                            .or_default(),
                        subject,
                    );
                }
            }
        }
        push(self.by_name.entry(key).or_default(), subject);
    }

    #[cfg(test)]
    pub(crate) fn insert_for_test(&mut self, name: &str, hash: u32, kind: Kind) {
        let hash_kind = match kind {
            Kind::SetBonus => "sandbox_perk",
            Kind::ArmorSet => "item_set",
            _ => "item",
        };
        self.add(
            name,
            Subject {
                hash,
                hash_kind,
                kind,
            },
        );
    }

    /// What `name` refers to among `kinds`.
    #[must_use]
    pub fn lookup(&self, name: &str, kinds: &[Kind]) -> Vec<Subject> {
        self.by_name
            .get(&normalize(name))
            .map(|v| {
                v.iter()
                    .copied()
                    .filter(|s| kinds.contains(&s.kind))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Armor mods listed by their family: "Ammo Finder" stands for every
    /// "… Ammo Finder" of `kinds`.
    #[must_use]
    pub fn family(&self, name: &str, kinds: &[Kind]) -> Vec<Subject> {
        let suffix = format!(" {}", normalize(name));
        let mut out = Vec::new();
        for (key, subjects) in &self.by_name {
            if key.ends_with(&suffix) {
                for s in subjects.iter().filter(|s| kinds.contains(&s.kind)) {
                    push(&mut out, *s);
                }
            }
        }
        out.sort_by_key(|s| s.hash);
        out
    }

    /// The fragments of `element` listed by their short name.
    #[must_use]
    pub fn fragment(&self, element: &str, short: &str) -> Vec<Subject> {
        self.fragments
            .get(&(element.to_string(), normalize(short)))
            .cloned()
            .unwrap_or_default()
    }
}

fn push(list: &mut Vec<Subject>, s: Subject) {
    if !list.contains(&s) {
        list.push(s);
    }
}

/// Lower case, typographic apostrophes as `'`, `&` as `and`, the word
/// "the" dropped ("Spirit of the Inmost Light" is the manifest's "Spirit of
/// Inmost Light"), whitespace collapsed, a trailing colon or asterisk
/// dropped.
#[must_use]
pub fn normalize(s: &str) -> String {
    let s = s
        .replace(['\u{2019}', '\u{2018}'], "'")
        .replace('&', " and ")
        .to_lowercase();
    let s = s
        .split_whitespace()
        .filter(|w| *w != "the")
        .collect::<Vec<_>>()
        .join(" ");
    s.trim_end_matches([':', '*']).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_despite_articles_ampersands_and_apostrophes() {
        assert_eq!(
            normalize("Spirit of the Inmost Light"),
            normalize("Spirit of Inmost Light")
        );
        assert_eq!(
            normalize("Silence & Squall"),
            normalize("Silence and Squall")
        );
        assert_eq!(normalize("Assassin\u{2019}s Cowl:"), "assassin's cowl");
    }

    #[test]
    fn mod_families_stand_for_each_element_version() {
        let mut n = Names::default();
        n.insert_for_test("Arc Ammo Finder", 1, Kind::ArmorMod);
        n.insert_for_test("Solar Ammo Finder", 2, Kind::ArmorMod);
        n.insert_for_test("Ammo Finder Ornament", 3, Kind::ArmorMod);
        let hashes: Vec<u32> = n
            .family("Ammo Finder", &[Kind::ArmorMod])
            .iter()
            .map(|s| s.hash)
            .collect();
        assert_eq!(hashes, [1, 2]);
    }
}
