//! Row structs for the staging and build tables, in column order.

use duckdb::types::Value;
use duckdb::{Appender, params};

use crate::sink::AppendRow;

/// `UINTEGER[]` value.
#[must_use]
pub fn u32_list(v: &[u32]) -> Value {
    Value::List(v.iter().map(|&x| Value::UInt(x)).collect())
}

/// `INTEGER[]` value.
#[must_use]
pub fn i32_list(v: &[i32]) -> Value {
    Value::List(v.iter().map(|&x| Value::Int(x)).collect())
}

/// Source of a staged run/entry row (see `sql/staging.sql`).
pub const SRC_ARCHIVE: u8 = 0;
pub const SRC_TRACKED: u8 = 1;

#[derive(Debug, Clone)]
pub struct StgRun {
    pub src: u8,
    pub instance_id: i64,
    pub period: String,
    pub activity_hash: u32,
    pub reference_hash: u32,
    pub mode: i32,
    pub modes: Option<Vec<i32>>,
    pub difficulty_tier: i32,
    pub duration_s: u32,
    pub player_count: u32,
    pub started_from_beginning: Option<bool>,
    pub is_private: Option<bool>,
    pub starting_phase_index: Option<i32>,
    pub skull_hashes: Vec<u32>,
    pub tracked_target: Option<String>,
}

impl AppendRow for StgRun {
    fn append(&self, a: &mut Appender<'_>) -> duckdb::Result<()> {
        a.append_row(params![
            self.src,
            self.instance_id,
            self.period,
            self.activity_hash,
            self.reference_hash,
            self.mode,
            self.modes.as_deref().map_or(Value::Null, i32_list),
            self.difficulty_tier,
            self.duration_s,
            self.player_count,
            self.started_from_beginning,
            self.is_private,
            self.starting_phase_index,
            u32_list(&self.skull_hashes),
            self.tracked_target,
        ])
    }
}

#[derive(Debug, Clone, Default)]
pub struct StgEntry {
    pub src: u8,
    pub instance_id: i64,
    pub membership_id: i64,
    pub character_id: i64,
    pub membership_type: i32,
    pub display_name: Option<String>,
    pub global_name: Option<String>,
    pub global_name_code: Option<u16>,
    pub class: String,
    pub light_level: i32,
    pub character_level: Option<i32>,
    pub emblem_hash: Option<u32>,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub precision_kills: u32,
    pub super_kills: u32,
    pub melee_kills: u32,
    pub grenade_kills: u32,
    pub ability_kills: u32,
    pub completed: bool,
    pub completion_reason: i32,
    pub time_played_s: u32,
    pub start_s: Option<u32>,
}

impl AppendRow for StgEntry {
    fn append(&self, a: &mut Appender<'_>) -> duckdb::Result<()> {
        a.append_row(params![
            self.src,
            self.instance_id,
            self.membership_id,
            self.character_id,
            self.membership_type,
            self.display_name,
            self.global_name,
            self.global_name_code,
            self.class,
            self.light_level,
            self.character_level,
            self.emblem_hash,
            self.kills,
            self.deaths,
            self.assists,
            self.precision_kills,
            self.super_kills,
            self.melee_kills,
            self.grenade_kills,
            self.ability_kills,
            self.completed,
            self.completion_reason,
            self.time_played_s,
            self.start_s,
        ])
    }
}

#[derive(Debug, Clone)]
pub struct StgWeapon {
    pub src: u8,
    pub instance_id: i64,
    pub membership_id: i64,
    pub character_id: i64,
    pub item_hash: u32,
    pub kills: u32,
    pub precision_kills: u32,
}

impl AppendRow for StgWeapon {
    fn append(&self, a: &mut Appender<'_>) -> duckdb::Result<()> {
        a.append_row(params![
            self.src,
            self.instance_id,
            self.membership_id,
            self.character_id,
            self.item_hash,
            self.kills,
            self.precision_kills,
        ])
    }
}

#[derive(Debug, Clone)]
pub struct StgHistory {
    /// Position of the source in oldest → newest order (live table newest).
    pub recency: i32,
    /// Insertion rowid in the live table: final tie-breaker within a source.
    pub row_key: i64,
    pub membership_id: i64,
    pub character_id: Option<i64>,
    pub instance_id: i64,
    pub period: Option<String>,
    pub activity_hash: Option<u32>,
    pub mode: Option<i32>,
    pub kills: Option<i64>,
    pub deaths: Option<i64>,
    pub assists: Option<i64>,
    pub duration_s: Option<i64>,
    pub completion_reason: Option<i64>,
    pub completed: Option<bool>,
    pub player_count: Option<i64>,
}

impl AppendRow for StgHistory {
    fn append(&self, a: &mut Appender<'_>) -> duckdb::Result<()> {
        a.append_row(params![
            self.recency,
            self.row_key,
            self.membership_id,
            self.character_id,
            self.instance_id,
            self.period,
            self.activity_hash,
            self.mode,
            self.kills,
            self.deaths,
            self.assists,
            self.duration_s,
            self.completion_reason,
            self.completed,
            self.player_count,
        ])
    }
}

/// A build-timeline segment (final `segment` table).
#[derive(Debug, Clone)]
pub struct SegmentRow {
    pub instance_id: i64,
    pub membership_id: i64,
    pub segment_index: i32,
    pub start_s: i64,
    pub duration_s: i64,
    pub build_id: i64,
    pub loadout_id: i64,
    pub stats: [Option<i32>; 7],
}

impl AppendRow for SegmentRow {
    fn append(&self, a: &mut Appender<'_>) -> duckdb::Result<()> {
        let s = &self.stats;
        a.append_row(params![
            self.instance_id,
            self.membership_id,
            self.segment_index,
            self.start_s,
            self.duration_s,
            self.build_id,
            self.loadout_id,
            s[0],
            s[1],
            s[2],
            s[3],
            s[4],
            s[5],
            s[6],
        ])
    }
}

/// Final `build` table row.
#[derive(Debug, Clone)]
pub struct BuildRow {
    pub build_id: i64,
    pub class: String,
    pub element: Option<&'static str>,
    pub subclass_hash: Option<u32>,
    pub super_hash: Option<u32>,
    pub class_ability_hash: Option<u32>,
    pub movement_hash: Option<u32>,
    pub melee_hash: Option<u32>,
    pub grenade_hash: Option<u32>,
    pub aspect_hashes: Vec<u32>,
    pub fragment_hashes: Vec<u32>,
    pub kinetic_hash: Option<u32>,
    pub energy_hash: Option<u32>,
    pub power_hash: Option<u32>,
    pub helmet_hash: Option<u32>,
    pub gauntlets_hash: Option<u32>,
    pub chest_hash: Option<u32>,
    pub legs_hash: Option<u32>,
    pub class_item_hash: Option<u32>,
    pub exotic_armor_hash: Option<u32>,
    pub exotic_weapon_hash: Option<u32>,
    pub artifact_hash: Option<u32>,
    pub artifact_perk_hashes: Vec<u32>,
    pub armor_mod_hashes: Vec<u32>,
    pub item_hashes: Vec<u32>,
    pub element_hashes: Vec<u32>,
    pub component_count: u16,
    pub unresolved_count: u16,
}

impl AppendRow for BuildRow {
    fn append(&self, a: &mut Appender<'_>) -> duckdb::Result<()> {
        a.append_row(params![
            self.build_id,
            self.class,
            self.element,
            self.subclass_hash,
            self.super_hash,
            self.class_ability_hash,
            self.movement_hash,
            self.melee_hash,
            self.grenade_hash,
            u32_list(&self.aspect_hashes),
            u32_list(&self.fragment_hashes),
            self.kinetic_hash,
            self.energy_hash,
            self.power_hash,
            self.helmet_hash,
            self.gauntlets_hash,
            self.chest_hash,
            self.legs_hash,
            self.class_item_hash,
            self.exotic_armor_hash,
            self.exotic_weapon_hash,
            self.artifact_hash,
            u32_list(&self.artifact_perk_hashes),
            u32_list(&self.armor_mod_hashes),
            u32_list(&self.item_hashes),
            u32_list(&self.element_hashes),
            self.element_hashes.len() as u16,
            self.component_count,
            self.unresolved_count,
            0_i32,
            0_i32,
            0_i64,
        ])
    }
}

#[derive(Debug, Clone)]
pub struct BuildComponentRow {
    pub build_id: i64,
    pub kind: &'static str,
    pub slot: Option<&'static str>,
    pub component_hash: u32,
    pub parent_hash: Option<u32>,
    pub quantity: u16,
    pub position: u16,
}

impl AppendRow for BuildComponentRow {
    fn append(&self, a: &mut Appender<'_>) -> duckdb::Result<()> {
        a.append_row(params![
            self.build_id,
            self.kind,
            self.slot,
            self.component_hash,
            self.parent_hash,
            self.quantity,
            self.position,
        ])
    }
}

#[derive(Debug, Clone)]
pub struct LoadoutRow {
    pub loadout_id: i64,
    pub build_id: i64,
    pub artifact_def_hash: Option<u32>,
    pub artifact_active_perks: Option<Vec<u32>>,
}

impl AppendRow for LoadoutRow {
    fn append(&self, a: &mut Appender<'_>) -> duckdb::Result<()> {
        a.append_row(params![
            self.loadout_id,
            self.build_id,
            self.artifact_def_hash,
            self.artifact_active_perks
                .as_deref()
                .map_or(Value::Null, u32_list),
        ])
    }
}

#[derive(Debug, Clone)]
pub struct LoadoutItemRow {
    pub loadout_id: i64,
    pub slot: &'static str,
    pub bucket_hash: u32,
    pub item_hash: u32,
    pub item_instance_id: i64,
    pub plug_hashes: Vec<u32>,
    pub perk_hashes: Vec<u32>,
}

impl AppendRow for LoadoutItemRow {
    fn append(&self, a: &mut Appender<'_>) -> duckdb::Result<()> {
        a.append_row(params![
            self.loadout_id,
            self.slot,
            self.bucket_hash,
            self.item_hash,
            self.item_instance_id,
            u32_list(&self.plug_hashes),
            u32_list(&self.perk_hashes),
        ])
    }
}
