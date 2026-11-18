//! Post-game carnage report (PGCR) parsing.
//!
//! The archive stores Bungie's `GetPostGameCarnageReport` response body (with
//! or without the `{ "Response": ... }` envelope), usually gzip-compressed.
//! Field semantics deliberately match the production Searchlight parser so
//! that runs parsed here agree with the rows production stored in `run` /
//! `run_entry` (the importer cross-checks this):
//!
//! * run duration = max `activityDurationSeconds` over entries;
//! * `player_count` = max `playerCount` over entries, else the roster size;
//! * stat values are floats in JSON; negative/NaN clamp to 0;
//! * weapon kills come from `uniqueWeaponKills` / `uniqueWeaponPrecisionKills`.

use std::io::Read;

use serde::Deserialize;
use thiserror::Error;

/// Errors from decoding or parsing an archived PGCR.
#[derive(Debug, Error)]
pub enum PgcrError {
    #[error("gzip decode failed: {0}")]
    Gzip(#[from] std::io::Error),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("response envelope has no Response body")]
    EmptyResponse,
    #[error("invalid {field} id {value:?}")]
    BadId { field: &'static str, value: String },
}

/// One parsed activity instance.
#[derive(Debug, Clone, PartialEq)]
pub struct Pgcr {
    pub instance_id: i64,
    /// ISO-8601 UTC start time (`period`).
    pub period: String,
    pub reference_id: u32,
    pub director_activity_hash: u32,
    pub mode: i32,
    pub modes: Vec<i32>,
    pub difficulty_tier: i32,
    pub is_private: bool,
    pub started_from_beginning: bool,
    pub starting_phase_index: Option<i32>,
    pub skull_hashes: Vec<u32>,
    pub duration_s: u32,
    pub player_count: u32,
    pub entries: Vec<PgcrEntry>,
}

/// One character's line in a PGCR.
#[derive(Debug, Clone, PartialEq)]
pub struct PgcrEntry {
    pub membership_id: i64,
    pub membership_type: i32,
    pub display_name: String,
    pub global_name: Option<String>,
    pub global_name_code: Option<u16>,
    pub is_public: bool,
    pub character_id: i64,
    pub class_name: String,
    pub light_level: i32,
    pub character_level: i32,
    pub emblem_hash: u32,
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
    pub start_s: u32,
    pub weapons: Vec<WeaponKills>,
}

/// Kills with one weapon (`extended.weapons[]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, Deserialize)]
pub struct WeaponKills {
    pub item_hash: u32,
    pub kills: u32,
    pub precision_kills: u32,
}

/// Decode a stored archive `raw` value — gzip (detected by its magic bytes) or
/// plain JSON, as production wrote both — into the JSON bytes. `buf` is reused
/// across calls to avoid reallocating for every report.
pub fn decode_raw<'a>(stored: &'a [u8], buf: &'a mut Vec<u8>) -> Result<&'a [u8], PgcrError> {
    if stored.starts_with(&[0x1f, 0x8b]) {
        buf.clear();
        flate2::read::MultiGzDecoder::new(stored).read_to_end(buf)?;
        Ok(buf.as_slice())
    } else {
        Ok(stored)
    }
}

/// Parse a stored archive value (gzip or plain) in one step.
pub fn parse_stored(stored: &[u8], buf: &mut Vec<u8>) -> Result<Pgcr, PgcrError> {
    parse_slice(decode_raw(stored, buf)?)
}

/// Parse a PGCR JSON document (bare report, or Bungie's response envelope).
///
/// The archive stores bare reports, so that is the fast path; the envelope is
/// only tried when the bare parse fails.
pub fn parse_slice(json: &[u8]) -> Result<Pgcr, PgcrError> {
    match serde_json::from_slice::<RawPgcr>(json) {
        Ok(raw) => map(raw),
        Err(bare_err) => match serde_json::from_slice::<RawEnvelope>(json) {
            Ok(env) => map(env.response.ok_or(PgcrError::EmptyResponse)?),
            Err(_) => Err(bare_err.into()),
        },
    }
}

fn parse_id(field: &'static str, s: &str) -> Result<i64, PgcrError> {
    s.parse::<i64>()
        .ok()
        .filter(|&v| v > 0)
        .ok_or_else(|| PgcrError::BadId {
            field,
            value: s.to_string(),
        })
}

fn map(raw: RawPgcr) -> Result<Pgcr, PgcrError> {
    let instance_id = parse_id("instanceId", &raw.activity_details.instance_id)?;
    let duration_s = raw
        .entries
        .iter()
        .map(|e| e.values.activity_duration_seconds.as_u32())
        .max()
        .unwrap_or(0);
    let player_count = raw
        .entries
        .iter()
        .map(|e| e.values.player_count.as_u32())
        .max()
        .unwrap_or(raw.entries.len() as u32);

    let mut entries = Vec::with_capacity(raw.entries.len());
    for e in raw.entries {
        let ui = e.player.destiny_user_info;
        let ext = e.extended.unwrap_or_default();
        let ev = &ext.values;
        entries.push(PgcrEntry {
            membership_id: parse_id("membershipId", &ui.membership_id)?,
            membership_type: ui.membership_type,
            display_name: ui.display_name,
            global_name: ui.global_name.filter(|s| !s.is_empty()),
            global_name_code: ui.global_name_code,
            is_public: ui.is_public,
            character_id: parse_id("characterId", &e.character_id)?,
            class_name: e.player.character_class,
            light_level: e.player.light_level,
            character_level: e.player.character_level,
            emblem_hash: e.player.emblem_hash,
            kills: e.values.kills.as_u32(),
            deaths: e.values.deaths.as_u32(),
            assists: e.values.assists.as_u32(),
            precision_kills: ev.precision_kills.as_u32(),
            super_kills: ev.super_kills.as_u32(),
            melee_kills: ev.melee_kills.as_u32(),
            grenade_kills: ev.grenade_kills.as_u32(),
            ability_kills: ev.ability_kills.as_u32(),
            completed: e.values.completed.as_bool(),
            completion_reason: e.values.completion_reason.as_u32() as i32,
            time_played_s: e.values.time_played_seconds.as_u32(),
            start_s: e.values.start_seconds.as_u32(),
            weapons: ext
                .weapons
                .into_iter()
                .map(|w| WeaponKills {
                    item_hash: w.reference_id,
                    kills: w.values.unique_weapon_kills.as_u32(),
                    precision_kills: w.values.unique_weapon_precision_kills.as_u32(),
                })
                .collect(),
        });
    }

    Ok(Pgcr {
        instance_id,
        period: raw.period,
        reference_id: raw.activity_details.reference_id,
        director_activity_hash: raw.activity_details.director_activity_hash,
        mode: raw.activity_details.mode,
        modes: raw.activity_details.modes,
        difficulty_tier: raw.activity_difficulty_tier,
        is_private: raw.activity_details.is_private,
        started_from_beginning: raw.activity_was_started_from_beginning,
        starting_phase_index: raw.starting_phase_index,
        skull_hashes: raw.selected_skull_hashes,
        duration_s,
        player_count,
        entries,
    })
}

// --- Raw serde shapes ------------------------------------------------------

/// Bungie's `{ "Response": ..., "ErrorCode": ... }` envelope.
#[derive(Deserialize)]
struct RawEnvelope {
    #[serde(rename = "Response")]
    response: Option<RawPgcr>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPgcr {
    #[serde(default)]
    period: String,
    #[serde(default)]
    starting_phase_index: Option<i32>,
    #[serde(default)]
    activity_was_started_from_beginning: bool,
    #[serde(default)]
    activity_difficulty_tier: i32,
    #[serde(default)]
    selected_skull_hashes: Vec<u32>,
    activity_details: RawActivityDetails,
    #[serde(default)]
    entries: Vec<RawEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawActivityDetails {
    #[serde(default)]
    reference_id: u32,
    #[serde(default)]
    director_activity_hash: u32,
    instance_id: String,
    #[serde(default)]
    mode: i32,
    #[serde(default)]
    modes: Vec<i32>,
    #[serde(default)]
    is_private: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEntry {
    player: RawPlayer,
    character_id: String,
    #[serde(default)]
    values: RawValues,
    #[serde(default)]
    extended: Option<RawExtended>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPlayer {
    destiny_user_info: RawUserInfo,
    #[serde(default)]
    character_class: String,
    #[serde(default)]
    light_level: i32,
    #[serde(default)]
    character_level: i32,
    #[serde(default)]
    emblem_hash: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawUserInfo {
    #[serde(default)]
    membership_type: i32,
    membership_id: String,
    #[serde(default)]
    display_name: String,
    #[serde(rename = "bungieGlobalDisplayName", default)]
    global_name: Option<String>,
    #[serde(rename = "bungieGlobalDisplayNameCode", default)]
    global_name_code: Option<u16>,
    #[serde(default)]
    is_public: bool,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawValues {
    kills: Stat,
    deaths: Stat,
    assists: Stat,
    completed: Stat,
    completion_reason: Stat,
    activity_duration_seconds: Stat,
    time_played_seconds: Stat,
    start_seconds: Stat,
    player_count: Stat,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawExtended {
    weapons: Vec<RawWeapon>,
    values: RawExtendedValues,
}

#[derive(Deserialize, Default)]
#[serde(default)]
#[allow(clippy::struct_field_names)] // mirrors Bungie's field names
struct RawExtendedValues {
    #[serde(rename = "precisionKills")]
    precision_kills: Stat,
    #[serde(rename = "weaponKillsSuper")]
    super_kills: Stat,
    #[serde(rename = "weaponKillsMelee")]
    melee_kills: Stat,
    #[serde(rename = "weaponKillsGrenade")]
    grenade_kills: Stat,
    #[serde(rename = "weaponKillsAbility")]
    ability_kills: Stat,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawWeapon {
    #[serde(default)]
    reference_id: u32,
    #[serde(default)]
    values: RawWeaponValues,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawWeaponValues {
    unique_weapon_kills: Stat,
    unique_weapon_precision_kills: Stat,
}

/// Bungie's `{ "basic": { "value": <float> } }` stat wrapper.
#[derive(Deserialize, Default)]
struct Stat {
    #[serde(default)]
    basic: Basic,
}

#[derive(Deserialize, Default)]
struct Basic {
    #[serde(default)]
    value: f64,
}

impl Stat {
    fn as_u32(&self) -> u32 {
        let v = self.basic.value;
        if v.is_finite() && v >= 0.0 {
            v as u32
        } else {
            0
        }
    }

    fn as_bool(&self) -> bool {
        self.basic.value != 0.0
    }
}
