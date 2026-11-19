//! Tests against real captured data (see `tests/fixtures/README.md`).

use searchlight_core::build::{ComponentKind, extract_build};
use searchlight_core::catalog::{Catalog, parse_item_sets, parse_items};
use searchlight_core::pgcr::{parse_slice, parse_stored};
use searchlight_core::vector::{ElementVector, Weights};
use searchlight_core::{Element, GuardianClass, LoadoutState, Slot};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

fn catalog() -> Catalog {
    Catalog::new(
        parse_items(&fixture("manifest_items_subset.json")).unwrap(),
        parse_item_sets(&fixture("manifest_item_sets_subset.json")).unwrap(),
    )
}

fn names(cat: &Catalog, hashes: impl IntoIterator<Item = u32>) -> Vec<String> {
    hashes
        .into_iter()
        .map(|h| {
            cat.item(h)
                .map_or_else(|| h.to_string(), |d| d.name.clone())
        })
        .collect()
}

#[test]
fn prismatic_exotic_class_item_build() {
    let cat = catalog();
    let loadout = LoadoutState::from_json(
        std::str::from_utf8(&fixture("loadout_prismatic_exotic_class_item.json")).unwrap(),
    )
    .unwrap();
    let b = extract_build(&loadout, &cat);

    assert!(
        b.unresolved_hashes.is_empty(),
        "unresolved: {:?}",
        b.unresolved_hashes
    );
    assert_eq!(b.class, GuardianClass::Warlock);
    assert_eq!(b.element, Some(Element::Prismatic));

    let one = |k| names(&cat, b.first(k));
    assert_eq!(one(ComponentKind::Super), ["Winter's Wrath"]);
    assert_eq!(one(ComponentKind::Grenade), ["Healing Grenade"]);
    assert_eq!(one(ComponentKind::Melee), ["Arcane Needle"]);
    assert_eq!(one(ComponentKind::ClassAbility), ["Healing Rift"]);
    assert_eq!(one(ComponentKind::Movement), ["Strafe Glide"]);

    let mut aspects = names(&cat, b.hashes(ComponentKind::Aspect));
    aspects.sort();
    assert_eq!(aspects, ["Feed the Void", "Lightning Surge"]);
    assert_eq!(b.hashes(ComponentKind::Fragment).count(), 5);
    // The Transcendence grenade is a fixed ability, not a second grenade.
    assert_eq!(b.hashes(ComponentKind::Grenade).count(), 1);
    assert_eq!(names(&cat, b.hashes(ComponentKind::Transcendence)).len(), 2);

    // Exotic class item with its two rolled spirits.
    assert_eq!(names(&cat, b.exotic_armor_hash), ["Solipsism"]);
    let spirits: Vec<_> = b
        .components
        .iter()
        .filter(|c| c.kind == ComponentKind::ArmorPerk && c.slot == Some(Slot::ClassItem))
        .map(|c| cat.item(c.hash).unwrap().name.clone())
        .collect();
    assert!(
        spirits.contains(&"Spirit of Inmost Light".to_string()),
        "{spirits:?}"
    );
    assert!(
        spirits.contains(&"Spirit of Synthoceps".to_string()),
        "{spirits:?}"
    );

    assert_eq!(names(&cat, b.exotic_weapon_hash), ["Praxic Blade"]);

    // Two 2-piece armor sets are active.
    let sets: Vec<_> = b
        .components
        .iter()
        .filter(|c| c.kind == ComponentKind::ArmorSet)
        .map(|c| c.quantity)
        .collect();
    assert_eq!(sets, vec![2, 2]);
    assert_eq!(b.hashes(ComponentKind::SetBonus).count(), 2);

    // Duplicate armor mods are kept as a quantity.
    let fonts = b
        .components
        .iter()
        .find(|c| cat.item(c.hash).is_some_and(|d| d.name == "Weapons Font"))
        .unwrap();
    assert_eq!(fonts.quantity, 2);

    // Nothing cosmetic leaks in.
    let all = names(&cat, b.item_hashes());
    for cosmetic in [
        "Kill Tracker",
        "Upgrade Armor",
        "Renegade Leader",
        "Sagira's Shell",
        "Default Ornament",
    ] {
        assert!(
            !all.iter().any(|n| n == cosmetic),
            "{cosmetic} leaked into build: {all:?}"
        );
    }
}

#[test]
fn parses_enveloped_production_fixture() {
    let p = parse_slice(&fixture("pgcr_enveloped_gm_conquest.json")).unwrap();
    assert_eq!(p.instance_id, 16_883_212_311);
    assert_eq!(p.director_activity_hash, 2_384_839_795);
    assert_eq!(p.difficulty_tier, 9);
    assert!(p.started_from_beginning);
    assert!(!p.skull_hashes.is_empty());
    assert_eq!(p.entries.len(), 3);
    assert!(
        p.entries
            .iter()
            .all(|e| e.membership_id > 0 && e.character_id > 0)
    );
    assert!(p.entries.iter().any(|e| !e.weapons.is_empty()));
    assert!(p.duration_s > 0);
}

#[test]
fn parses_gzipped_bare_archive_row() {
    let mut buf = Vec::new();
    let p = parse_stored(&fixture("pgcr_archive_bare.json.gz"), &mut buf).unwrap();
    assert_eq!(p.instance_id, 17_177_535_268);
    assert_eq!(p.director_activity_hash, 2_727_361_621);
    assert!(p.period.starts_with("2026-09"));
    assert!(!p.entries.is_empty());
}

#[test]
fn malformed_pgcr_is_an_error_not_a_panic() {
    let mut buf = Vec::new();
    assert!(parse_stored(b"{not json", &mut buf).is_err());
    assert!(parse_stored(&[0x1f, 0x8b, 0, 1, 2], &mut buf).is_err());
    assert!(parse_slice(br#"{"Response":null,"ErrorCode":5}"#).is_err());
}

#[test]
fn element_vector_of_the_prismatic_build() {
    let cat = catalog();
    let mut loadout = LoadoutState::from_json(
        std::str::from_utf8(&fixture("loadout_prismatic_exotic_class_item.json")).unwrap(),
    )
    .unwrap();
    let build = extract_build(&loadout, &cat);
    let v = ElementVector::from_build(&build);
    let named = |kind: ComponentKind| -> Vec<String> {
        let mut n: Vec<String> = v
            .entries()
            .iter()
            .filter(|e| e.kind == kind)
            .flat_map(|e| {
                std::iter::repeat_n(cat.item(e.hash).unwrap().name.clone(), usize::from(e.count))
            })
            .collect();
        n.sort();
        n
    };

    // Choices are counted, placement is not: Backup Mag on two weapons is one
    // element with count 2.
    assert_eq!(
        named(ComponentKind::WeaponMod),
        ["Backup Mag", "Backup Mag"]
    );
    assert!(named(ComponentKind::WeaponPerk).contains(&"Vorpal Weapon".to_string()));
    assert_eq!(named(ComponentKind::Aspect).len(), 2);
    assert_eq!(named(ComponentKind::Fragment).len(), 5);
    assert_eq!(
        named(ComponentKind::ArmorPerk),
        ["Spirit of Inmost Light", "Spirit of Synthoceps"]
    );
    assert_eq!(
        named(ComponentKind::ArmorMod)
            .iter()
            .filter(|m| *m == "Weapons Font")
            .count(),
        2
    );
    // Elements determined by another element are not counted.
    for kind in [
        ComponentKind::WeaponIntrinsic,
        ComponentKind::Transcendence,
        ComponentKind::ArmorSet,
        ComponentKind::SetBonus,
    ] {
        assert!(named(kind).is_empty(), "{kind} should not be an element");
    }
    // The multiset encoding has one entry per counted element.
    assert_eq!(v.multiset().len() as u32, v.total());

    // Dropping one fragment: one slot edit, L1 1, Jaccard 1/total.
    let subclass = loadout
        .items
        .iter_mut()
        .find(|i| i.slot() == Slot::Subclass)
        .unwrap();
    let facet = subclass
        .plug_hashes
        .iter()
        .position(|&p| cat.item(p).is_some_and(|d| d.name == "Facet of Courage"))
        .unwrap();
    subclass.plug_hashes.remove(facet);
    let fewer = ElementVector::from_build(&extract_build(&loadout, &cat));
    let w = Weights::default();
    assert_eq!(v.slot_edits(&fewer), 1);
    assert!((v.l1_distance(&fewer, &w) - 1.0).abs() < 1e-12);
    assert!((v.jaccard_distance(&fewer, &w) - 1.0 / f64::from(v.total())).abs() < 1e-12);
}
