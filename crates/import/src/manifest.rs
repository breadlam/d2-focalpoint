//! Manifest stage: cached `Destiny*Definition.json` files → dimension tables
//! and the in-memory [`Catalog`] the build stage resolves hashes against.

use std::path::Path;

use anyhow::{Context, Result};
use duckdb::{Connection, params};
use focalpoint_core::Catalog;
use focalpoint_core::catalog::{
    self, ActivityDef, ActivityModeDef, ItemDef, ItemSetDef, NamedDef, damage_type_name,
};
use focalpoint_core::slot::SlotGroup;

use crate::rows::i32_list;

/// What the manifest stage produced.
#[derive(Debug)]
pub struct ManifestOutput {
    pub catalog: Catalog,
    pub version: Option<String>,
    pub items: usize,
    pub activities: usize,
}

fn read(dir: &Path, name: &str) -> Result<Vec<u8>> {
    let p = dir.join(name);
    std::fs::read(&p).with_context(|| format!("reading manifest table {}", p.display()))
}

/// Load the manifest tables from `dir` and write the dimension tables.
pub fn load(conn: &Connection, dir: &Path) -> Result<ManifestOutput> {
    let items = catalog::parse_items(&read(dir, "DestinyInventoryItemDefinition.json")?)
        .context("parsing DestinyInventoryItemDefinition")?;
    let sets = catalog::parse_item_sets(&read(dir, "DestinyEquipableItemSetDefinition.json")?)
        .context("parsing DestinyEquipableItemSetDefinition")?;
    let perks = catalog::parse_named(&read(dir, "DestinySandboxPerkDefinition.json")?)
        .context("parsing DestinySandboxPerkDefinition")?;
    let stats = catalog::parse_named(&read(dir, "DestinyStatDefinition.json")?)
        .context("parsing DestinyStatDefinition")?;
    // Older manifest caches lack damage types; they only add element glyphs.
    let damage_types = match std::fs::read(dir.join("DestinyDamageTypeDefinition.json")) {
        Ok(json) => catalog::parse_named(&json).context("parsing DestinyDamageTypeDefinition")?,
        Err(_) => Vec::new(),
    };
    let activities = catalog::parse_activities(&read(dir, "DestinyActivityDefinition.json")?)
        .context("parsing DestinyActivityDefinition")?;
    let modes = catalog::parse_activity_modes(&read(dir, "DestinyActivityModeDefinition.json")?)
        .context("parsing DestinyActivityModeDefinition")?;
    // Older manifest caches lack difficulty ladders.
    let tiers =
        match std::fs::read(dir.join("DestinyActivityDifficultyTierCollectionDefinition.json")) {
            Ok(json) => catalog::parse_difficulty_tiers(&json)
                .context("parsing DestinyActivityDifficultyTierCollectionDefinition")?,
            Err(_) => Vec::new(),
        };
    let version = std::fs::read_to_string(dir.join("item_version.txt"))
        .or_else(|_| std::fs::read_to_string(dir.join("version.txt")))
        .ok()
        .map(|s| s.trim().to_string());

    write_items(conn, &items)?;
    write_sets(conn, &sets)?;
    write_named(conn, "sandbox_perk", &perks, true)?;
    write_named(conn, "stat", &stats, true)?;
    write_named(conn, "damage_type", &damage_types, true)?;
    write_modes(conn, &modes)?;
    write_activity_defs(conn, &activities)?;
    write_difficulty_tiers(conn, &tiers)?;

    let n_items = items.len();
    let n_acts = activities.len();
    Ok(ManifestOutput {
        catalog: Catalog::new(items, sets),
        version,
        items: n_items,
        activities: n_acts,
    })
}

fn write_items(conn: &Connection, items: &[ItemDef]) -> Result<()> {
    let mut a = conn.appender("item")?;
    for i in items {
        let class = match i.class_type {
            0..=2 => Some(i.class().as_str()),
            _ => None,
        };
        let slot = i.slot();
        let is_subclass = slot.group() == SlotGroup::Subclass;
        a.append_row(params![
            i.hash,
            i.name,
            (!i.description.is_empty()).then_some(i.description.as_str()),
            i.item_type,
            i.item_sub_type,
            (!i.item_type_name.is_empty()).then_some(i.item_type_name.as_str()),
            i.tier_type,
            (!i.tier_name.is_empty()).then_some(i.tier_name.as_str()),
            i.is_exotic(),
            class,
            (i.bucket_hash != 0).then_some(i.bucket_hash),
            (i.bucket_hash != 0).then_some(slot.as_str()),
            (!is_subclass)
                .then(|| damage_type_name(i.default_damage_type))
                .flatten(),
            is_subclass
                .then(|| i.subclass_element().map(focalpoint_core::Element::as_str))
                .flatten(),
            i.ammo_type,
            i.breaker_type,
            i.plug_category,
            i.plug_category
                .as_ref()
                .map(|_| i.plug_kind(SlotGroup::Other).as_str()),
            i.item_set_hash,
            i.icon,
        ])?;
    }
    a.flush()?;
    Ok(())
}

fn write_sets(conn: &Connection, sets: &[ItemSetDef]) -> Result<()> {
    let mut a = conn.appender("item_set")?;
    for s in sets {
        a.append_row(params![s.hash, s.name])?;
    }
    a.flush()?;
    drop(a);
    let mut a = conn.appender("item_set_perk")?;
    for s in sets {
        for &(required, perk) in &s.perks {
            a.append_row(params![s.hash, required, perk])?;
        }
    }
    a.flush()?;
    Ok(())
}

fn write_named(conn: &Connection, table: &str, defs: &[NamedDef], with_icon: bool) -> Result<()> {
    let mut a = conn.appender(table)?;
    for d in defs {
        let desc = (!d.description.is_empty()).then_some(d.description.as_str());
        if with_icon {
            a.append_row(params![d.hash, d.name, desc, d.icon])?;
        } else {
            a.append_row(params![d.hash, d.name, desc])?;
        }
    }
    a.flush()?;
    Ok(())
}

fn write_modes(conn: &Connection, modes: &[ActivityModeDef]) -> Result<()> {
    // Several mode definitions can share a modeType; keep the first by hash.
    let mut modes: Vec<&ActivityModeDef> = modes.iter().collect();
    modes.sort_by_key(|m| (m.mode_type, m.hash));
    modes.dedup_by_key(|m| m.mode_type);
    let mut a = conn.appender("activity_mode")?;
    for m in modes {
        a.append_row(params![
            m.mode_type,
            m.name,
            (!m.friendly_name.is_empty()).then_some(m.friendly_name.as_str())
        ])?;
    }
    a.flush()?;
    Ok(())
}

fn write_activity_defs(conn: &Connection, defs: &[ActivityDef]) -> Result<()> {
    let mut a = crate::sink::appender(conn, "stg.activity_def")?;
    for d in defs {
        a.append_row(params![
            d.hash,
            d.name,
            (!d.description.is_empty()).then_some(d.description.as_str()),
            d.mode,
            i32_list(&d.modes),
            d.activity_type_hash,
            d.is_playlist,
            d.light_level,
            d.difficulty_collection_hash,
        ])?;
    }
    a.flush()?;
    Ok(())
}

fn write_difficulty_tiers(conn: &Connection, tiers: &[catalog::DifficultyTier]) -> Result<()> {
    let mut a = conn.appender("difficulty_tier")?;
    for t in tiers {
        a.append_row(params![
            t.collection_hash,
            t.tier,
            t.name,
            t.activity_level,
            t.tier_rank,
            t.min_power,
            t.level_offset,
        ])?;
    }
    a.flush()?;
    Ok(())
}
