//! Build facets and run filters shared by search and ranking.
//!
//! Each renders to a SQL predicate over fixed table aliases, appending its
//! bound parameters in placeholder order:
//!
//! * [`Facets`] over `b` (`build`);
//! * [`RunFilter`] over `r` (a table with `started_at` and `is_solo`, such
//!   as `run` or `run_score`), `a` (`activity`) and `pl` (`player`).
//!
//! Names match case-insensitively against *every* item carrying them,
//! because item names are not unique; an unknown name fails with close
//! suggestions.

use std::fmt::Write as _;

use duckdb::types::Value;

use crate::{Database, DbError, Result};

/// What a build must contain. Empty facets impose no constraint; every
/// listed name must be present (AND).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facets {
    pub class: Option<String>,
    pub element: Option<String>,
    pub exotic_armor: Option<String>,
    pub exotic_weapon: Option<String>,
    pub super_name: Option<String>,
    pub aspects: Vec<String>,
    pub fragments: Vec<String>,
    /// Weapons in any slot.
    pub weapons: Vec<String>,
    /// Weapon perks, traits, origin traits and frames.
    pub perks: Vec<String>,
    /// Armor mods (general, stat, tuning) and weapon mods.
    pub mods: Vec<String>,
    pub artifact_perks: Vec<String>,
    /// Armor sets with at least two pieces equipped.
    pub armor_sets: Vec<String>,
}

impl Facets {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// A predicate over `b`, resolving names against the item tables.
    pub fn sql(&self, db: &Database, params: &mut Vec<Value>) -> Result<String> {
        let mut sql = String::from("TRUE");
        if let Some(c) = &self.class {
            sql.push_str(" AND lower(b.class) = lower(?)");
            params.push(Value::Text(c.clone()));
        }
        if let Some(e) = &self.element {
            sql.push_str(" AND lower(b.element) = lower(?)");
            params.push(Value::Text(e.clone()));
        }
        for (name, column) in [
            (&self.exotic_armor, "exotic_armor_hash"),
            (&self.exotic_weapon, "exotic_weapon_hash"),
            (&self.super_name, "super_hash"),
        ] {
            if let Some(n) = name {
                let hashes = resolve(db, "item", "item_hash", n)?;
                let _ = write!(sql, " AND b.{column} IN ({})", hash_list(&hashes));
            }
        }
        let component_facets: [(&Vec<String>, &str); 7] = [
            (&self.aspects, "'aspect'"),
            (&self.fragments, "'fragment'"),
            (&self.weapons, "'weapon'"),
            (
                &self.perks,
                "'weapon_perk', 'origin_trait', 'weapon_intrinsic'",
            ),
            (
                &self.mods,
                "'armor_mod', 'armor_stat_mod', 'armor_tuning', 'weapon_mod'",
            ),
            (&self.artifact_perks, "'artifact_perk'"),
            (&self.armor_sets, "'armor_set'"),
        ];
        for (names, kinds) in component_facets {
            for n in names {
                let (hashes, extra) = if kinds == "'armor_set'" {
                    (
                        resolve(db, "item_set", "set_hash", n)?,
                        " AND quantity >= 2",
                    )
                } else {
                    (resolve(db, "item", "item_hash", n)?, "")
                };
                let _ = write!(
                    sql,
                    " AND b.build_id IN (SELECT build_id FROM build_component WHERE kind IN ({kinds}) \
                     AND component_hash IN ({}){extra})",
                    hash_list(&hashes)
                );
            }
        }
        Ok(sql)
    }
}

/// Which runs count.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunFilter {
    /// Substring of an activity name or Searchlight target group.
    pub activity: Option<String>,
    /// Exactly this Searchlight target group (e.g. "Master Raid").
    pub target_group: Option<String>,
    /// Exactly this activity name (names repeat across difficulty hashes).
    pub activity_name: Option<String>,
    /// Substring of a player's Bungie name or display name.
    pub player: Option<String>,
    pub solo: bool,
    /// Inclusive `YYYY-MM-DD` bounds on the run start date.
    pub since: Option<String>,
    pub until: Option<String>,
}

impl RunFilter {
    /// A predicate over `r`, `a` and `pl`.
    pub fn sql(&self, params: &mut Vec<Value>) -> String {
        let mut sql = String::from("TRUE");
        if self.solo {
            sql.push_str(" AND r.is_solo");
        }
        if let Some(a) = &self.activity {
            sql.push_str(
                " AND (a.name ILIKE '%' || ? || '%' OR a.target_group ILIKE '%' || ? || '%')",
            );
            params.push(Value::Text(a.clone()));
            params.push(Value::Text(a.clone()));
        }
        if let Some(g) = &self.target_group {
            sql.push_str(" AND a.target_group = ?");
            params.push(Value::Text(g.clone()));
        }
        if let Some(n) = &self.activity_name {
            sql.push_str(" AND a.name = ?");
            params.push(Value::Text(n.clone()));
        }
        if let Some(p) = &self.player {
            sql.push_str(
                " AND (pl.bungie_name ILIKE '%' || ? || '%' OR pl.display_name ILIKE '%' || ? || '%')",
            );
            params.push(Value::Text(p.clone()));
            params.push(Value::Text(p.clone()));
        }
        if let Some(d) = &self.since {
            sql.push_str(" AND r.started_at >= CAST(? AS DATE)");
            params.push(Value::Text(d.clone()));
        }
        if let Some(d) = &self.until {
            sql.push_str(" AND r.started_at < CAST(? AS DATE) + INTERVAL 1 DAY");
            params.push(Value::Text(d.clone()));
        }
        sql
    }
}

/// Resolve a name to every hash carrying it (case-insensitive), or fail with
/// close suggestions.
fn resolve(db: &Database, table: &str, hash_col: &str, name: &str) -> Result<Vec<u32>> {
    let mut stmt = db.conn().prepare(&format!(
        "SELECT {hash_col} FROM {table} WHERE lower(name) = lower(?)"
    ))?;
    let hashes: Vec<u32> = stmt
        .query_map([name], |r| r.get(0))?
        .collect::<duckdb::Result<_>>()?;
    if !hashes.is_empty() {
        return Ok(hashes);
    }
    let mut stmt = db.conn().prepare(&format!(
        "SELECT DISTINCT name FROM {table} WHERE name ILIKE '%' || ? || '%' ORDER BY length(name), name LIMIT 5"
    ))?;
    let suggestions: Vec<String> = stmt
        .query_map([name], |r| r.get(0))?
        .collect::<duckdb::Result<_>>()?;
    Err(DbError::UnknownName {
        name: name.to_string(),
        suggestions,
    })
}

fn hash_list(hashes: &[u32]) -> String {
    hashes
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
