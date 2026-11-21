//! The community's documentation of Destiny 2 mechanics, read from raw
//! downloads into the Focalpoint database (`focalpoint mechanics import`,
//! `docs/mechanics.md`).
//!
//! The raw sources live outside the repository (by default
//! `~/.local/share/focalpoint/sources/`, one folder per source with a
//! `manifest.json`). Each source is read into [`Entry`]s — one documented
//! game element or game state, its `PvE` text, the manifest hashes it
//! documents, and the words the source marks as keywords — and written to
//! the `mechanic_*` tables, replacing them.
//!
//! * [`clarity`] — the Clarity database, keyed by hash;
//! * [`compendium`] — the Destiny Data Compendium's tabs, keyed by name,
//!   matched to the manifest by [`names`];
//! * [`aegis`] — `TheAegisRelic`'s boss damage workbook: measured damage
//!   rotations and the DIM loadouts they were run with.
//!
//! With them it builds the profiles of the [`context`]s builds are evaluated
//! in: activity groups, activities and boss encounters.

use std::path::Path;

use anyhow::{Context, Result};
use duckdb::params;
use focalpoint_db::{Database, MECHANICS_SQL, MECHANICS_VERSION, meta_key};

pub mod aegis;
pub mod clarity;
pub mod compendium;
pub mod context;
pub mod durability;
pub mod effects;
pub mod names;
pub mod sheet;
pub mod xlsx;

/// One documented game element or game state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The source's tab or type.
    pub section: String,
    pub name: String,
    /// `PvE` text, lines joined by `\n`.
    pub text: String,
    pub subjects: Vec<SubjectRef>,
    /// `(keyword, the source's marking)`.
    pub keywords: Vec<(String, String)>,
}

/// A manifest hash an entry documents, and how it was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubjectRef {
    pub hash: u32,
    pub hash_kind: &'static str,
    /// `hash` (given by the source), `item` (the weapon or armor a
    /// perk is on), `name`, or `alias` (a fragment's short name).
    pub rule: &'static str,
}

/// A source's provenance, from its `manifest.json`.
#[derive(Debug, Clone, Default)]
pub struct Provenance {
    pub title: String,
    pub url: Option<String>,
    pub fetched: Option<String>,
}

/// What an import read from one source.
#[derive(Debug, Clone)]
pub struct Summary {
    pub source: &'static str,
    pub provenance: Provenance,
    pub entries: usize,
    pub with_subject: usize,
    pub subjects: usize,
    pub keywords: usize,
    pub rotations: usize,
    pub loadouts: usize,
    /// Loadouts linked but not saved.
    pub missing_loadouts: usize,
}

/// One source as read.
#[derive(Debug, Default)]
struct Source {
    name: &'static str,
    provenance: Provenance,
    entries: Vec<Entry>,
    measured: aegis::Aegis,
}

impl Source {
    fn summary(self) -> Summary {
        let e = &self.entries;
        Summary {
            source: self.name,
            entries: e.len(),
            with_subject: e.iter().filter(|e| !e.subjects.is_empty()).count(),
            subjects: e.iter().map(|e| e.subjects.len()).sum(),
            keywords: e.iter().map(|e| e.keywords.len()).sum(),
            rotations: self.measured.rotations.len(),
            loadouts: self.measured.loadouts.len(),
            missing_loadouts: self.measured.missing.len(),
            provenance: self.provenance,
        }
    }
}

/// Read a source folder's `manifest.json`.
pub fn provenance(dir: &Path) -> Result<Provenance> {
    let path = dir.join("manifest.json");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let s = |k: &str| {
        v.get(k)
            .and_then(serde_json::Value::as_str)
            .map(String::from)
    };
    Ok(Provenance {
        title: s("title").unwrap_or_default(),
        url: s("url"),
        fetched: s("fetched"),
    })
}

/// What an import wrote.
#[derive(Debug, Clone)]
pub struct Imported {
    pub sources: Vec<Summary>,
    pub contexts: usize,
    pub encounters: usize,
    pub features: usize,
    pub events: usize,
    /// Encounters whose activity has no scored clears: no parent.
    pub unplaced: Vec<String>,
    pub durability: DurabilitySummary,
}

/// Hand-maintained files to read instead of the built-in ones.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Instead of `data/contexts.toml`.
    pub contexts: Option<std::path::PathBuf>,
    /// Instead of `data/difficulty.toml`.
    pub difficulty: Option<std::path::PathBuf>,
    /// Instead of `data/enemy_health.toml`.
    pub enemy_health: Option<std::path::PathBuf>,
}

/// What the import read about enemy durability.
#[derive(Debug, Clone, Default)]
pub struct DurabilitySummary {
    pub enemies: usize,
    pub health_rows: usize,
    pub measured: usize,
    pub curve_points: usize,
    pub champion_stats: usize,
    /// Contexts given a Power delta and health tier.
    pub contexts: usize,
    /// `data/difficulty.toml` entries and modifier rows (context × skull)
    /// written for the runs.
    pub difficulties: usize,
    pub modifiers: usize,
    /// In-game health measurements (`data/enemy_health.toml`).
    pub observed: usize,
    /// `data/difficulty.toml` entries naming no context (activities
    /// Searchlight did not target).
    pub unmatched: Vec<String>,
}

/// Read every source under `dir`, build the context profiles (with
/// hand-maintained detail from `data/contexts.toml` and
/// `data/difficulty.toml`, or the files in `options`), and replace the
/// layer's tables.
pub fn import(
    db: &Database,
    dir: &Path,
    options: &Options,
    mut progress: impl FnMut(&str),
) -> Result<Imported> {
    let names = names::Names::load(db)?;
    let mut read = Vec::new();

    progress("clarity");
    let clarity_dir = dir.join("clarity");
    read.push(Source {
        name: "clarity",
        provenance: provenance(&clarity_dir)?,
        entries: clarity::read(&clarity_dir.join("clarity.json"))?,
        ..Source::default()
    });

    progress("compendium");
    let compendium_dir = dir.join("compendium");
    read.push(Source {
        name: "compendium",
        provenance: provenance(&compendium_dir)?,
        entries: compendium::read(&compendium_dir, &names)?,
        ..Source::default()
    });

    progress("aegis");
    let aegis_dir = dir.join("aegis");
    read.push(Source {
        name: "aegis",
        provenance: provenance(&aegis_dir)?,
        measured: aegis::read(&aegis_dir)?,
        ..Source::default()
    });

    progress("contexts");
    let runs = context::from_runs(db, focalpoint_db::rank::CHECKPOINT_SHARE)?;
    let mut stmt = db
        .conn()
        .prepare("SELECT activity_hash, name FROM activity WHERE name IS NOT NULL")?;
    let activities: Vec<(u32, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<duckdb::Result<_>>()?;
    let bosses: Vec<aegis::Boss> = read
        .iter()
        .flat_map(|s| s.measured.bosses.iter().cloned())
        .collect();
    let encounters = context::from_bosses(&bosses, &activities, &runs);
    let curated = match &options.contexts {
        Some(path) => context::curated(path)?,
        None => context::parse_curated(context::CURATED)?,
    };
    let mut contexts = context::merge(vec![runs, encounters, curated]);

    progress("durability");
    let tables = durability::read(&dir.join("scaling"), &compendium_dir)?;
    let difficulties = match &options.difficulty {
        Some(path) => durability::difficulties(
            &std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?,
        )
        .with_context(|| format!("parsing {}", path.display()))?,
        None => durability::difficulties(durability::DIFFICULTY)?,
    };
    let applied = durability::apply(&mut contexts, &difficulties, &tables.curve);
    let observed = match &options.enemy_health {
        Some(path) => durability::observations(
            &std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?,
        )
        .with_context(|| format!("parsing {}", path.display()))?,
        None => durability::observations(durability::ENEMY_HEALTH)?,
    };

    progress("writing");
    write(db, &read, &contexts)?;
    write_durability(db, &tables, &applied, &observed)?;
    db.set_meta(meta_key::MECHANICS_VERSION, &MECHANICS_VERSION.to_string())?;
    db.set_meta(meta_key::MECHANICS_SOURCES, &dir.display().to_string())?;
    let now: String =
        db.conn()
            .query_row("SELECT strftime(now(), '%Y-%m-%dT%H:%M:%SZ')", [], |r| {
                r.get(0)
            })?;
    db.set_meta(meta_key::MECHANICS_IMPORTED_AT, &now)?;
    Ok(Imported {
        sources: read.into_iter().map(Source::summary).collect(),
        contexts: contexts.len(),
        encounters: contexts.iter().filter(|c| c.kind == "encounter").count(),
        features: contexts.iter().map(|c| c.features.len()).sum(),
        events: contexts.iter().map(|c| c.events.len()).sum(),
        unplaced: contexts
            .iter()
            .filter(|c| c.kind == "encounter" && c.parent.is_none())
            .map(|c| c.name.clone())
            .collect(),
        durability: DurabilitySummary {
            enemies: {
                let mut e: Vec<(&str, &str)> = tables
                    .health
                    .iter()
                    .map(|h| (h.faction.as_str(), h.enemy.as_str()))
                    .collect();
                e.sort_unstable();
                e.dedup();
                e.len()
            },
            health_rows: tables.health.len(),
            measured: tables.measured.len(),
            curve_points: tables.curve.len(),
            champion_stats: tables.champions.len(),
            contexts: applied.contexts,
            difficulties: applied.rows.len(),
            modifiers: applied.modifiers.len(),
            observed: observed.len(),
            unmatched: applied.unmatched,
        },
    })
}

fn count(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

fn write(db: &Database, read: &[Source], contexts: &[context::Context]) -> Result<()> {
    let conn = db.conn();
    conn.execute_batch("BEGIN TRANSACTION")?;
    conn.execute_batch(MECHANICS_SQL)?;
    {
        let mut sources = conn.appender("mechanic_source")?;
        let mut entries = conn.appender("mechanic_entry")?;
        let mut subjects = conn.appender("mechanic_subject")?;
        let mut keywords = conn.appender("mechanic_keyword")?;
        let mut tags = conn.appender("mechanic_tag")?;
        let mut rotations = conn.appender("mechanic_rotation")?;
        let mut rotation_loadouts = conn.appender("mechanic_rotation_loadout")?;
        let mut loadouts = conn.appender("mechanic_loadout")?;
        let mut loadout_items = conn.appender("mechanic_loadout_item")?;
        for src in read {
            let source = src.name;
            let prov = &src.provenance;
            let m = &src.measured;
            sources.append_row(params![
                source,
                prov.title,
                prov.url,
                prov.fetched,
                count(src.entries.len()),
                count(m.rotations.len()),
                count(m.loadouts.len())
            ])?;
            for (i, e) in src.entries.iter().enumerate() {
                let id = count(i);
                entries.append_row(params![source, id, e.section, e.name, e.text])?;
                let mut seen = Vec::new();
                for s in &e.subjects {
                    if seen.contains(&(s.hash, s.hash_kind)) {
                        continue;
                    }
                    seen.push((s.hash, s.hash_kind));
                    subjects.append_row(params![source, id, s.hash, s.hash_kind, s.rule])?;
                }
                for (k, mark) in &e.keywords {
                    keywords.append_row(params![source, id, k, mark])?;
                }
                for t in effects::read(&e.text) {
                    tags.append_row(params![source, id, t.role.as_str(), t.tag, t.evidence])?;
                }
            }
            for (i, r) in m.rotations.iter().enumerate() {
                let id = count(i);
                let [d10, d20, d30, d40, d50, d60] = r.dps_by_10s;
                rotations.append_row(params![
                    source,
                    id,
                    r.section,
                    r.category,
                    r.class,
                    r.name,
                    r.boss,
                    r.core_dps,
                    d10,
                    d20,
                    d30,
                    d40,
                    d50,
                    d60,
                    r.end_dps,
                    r.end_s,
                    r.end_damage,
                    r.continues,
                    r.segment_start_s,
                    r.segment_end_s,
                    r.artifact_mods,
                    r.other_factors,
                    r.notes
                ])?;
                for (loadout, role, label) in &r.loadouts {
                    rotation_loadouts.append_row(params![source, id, loadout, role, label])?;
                }
            }
            for l in &m.loadouts {
                loadouts.append_row(params![source, l.id, l.name, l.class])?;
                for i in &l.items {
                    loadout_items
                        .append_row(params![source, l.id, i.hash, i.role, i.parent, i.socket])?;
                }
            }
        }
    }
    {
        let mut ctx = conn.appender("context")?;
        let mut acts = conn.appender("context_activity")?;
        let mut features = conn.appender("context_feature")?;
        let mut events = conn.appender("context_event")?;
        for c in contexts {
            ctx.append_row(params![c.id, c.kind, c.parent, c.name])?;
            let mut seen = Vec::new();
            for h in &c.activities {
                if !seen.contains(h) {
                    seen.push(*h);
                    acts.append_row(params![c.id, h])?;
                }
            }
            for f in &c.features {
                features.append_row(params![c.id, f.name, f.value, f.text, f.source])?;
            }
            for (i, e) in c.events.iter().enumerate() {
                events.append_row(params![
                    c.id,
                    count(i),
                    e.kind,
                    e.start_s,
                    e.length_s,
                    e.detail,
                    e.source
                ])?;
            }
        }
    }
    conn.execute_batch("COMMIT")?;
    Ok(())
}

fn write_durability(
    db: &Database,
    t: &durability::Tables,
    applied: &durability::Applied,
    observed: &[durability::Observation],
) -> Result<()> {
    let conn = db.conn();
    conn.execute_batch("BEGIN TRANSACTION")?;
    {
        let mut health = conn.appender("enemy_health")?;
        for h in &t.health {
            health.append_row(params![
                "mossymax", h.faction, h.enemy, h.rank, h.base, h.tier, h.health
            ])?;
        }
        let mut measured = conn.appender("enemy_measured")?;
        for m in &t.measured {
            measured.append_row(params![
                "mossymax",
                m.kind,
                m.enemy,
                m.activity,
                m.tier,
                m.estimated,
                m.baseline
            ])?;
        }
        let mut curve = conn.appender("power_delta")?;
        for (d, m) in &t.curve {
            curve.append_row(params![d, m, "mossymax (EoF Standard)"])?;
        }
        let mut difficulty = conn.appender("difficulty")?;
        for r in &applied.rows {
            let steps = duckdb::types::Value::List(
                r.steps
                    .iter()
                    .map(|s| duckdb::types::Value::Text(s.clone()))
                    .collect(),
            );
            difficulty.append_row(params![
                r.context_id,
                r.context,
                r.depth,
                steps,
                r.power_cap,
                r.combatant_power,
                r.power_delta,
                r.health_tier,
                r.confidence,
                r.evidence
            ])?;
        }
        let mut modifiers = conn.appender("difficulty_modifier")?;
        for m in &applied.modifiers {
            modifiers.append_row(params![
                m.context_id,
                m.skull,
                m.power_cap,
                m.combatant_power,
                m.confidence,
                m.evidence
            ])?;
        }
        let mut observations = conn.appender("enemy_observed")?;
        for o in observed {
            observations.append_row(params![
                o.enemy,
                o.faction,
                o.archetype,
                o.rank,
                o.bar,
                o.place,
                o.step,
                o.power_delta,
                o.health,
                o.approximate,
                o.source,
                o.note
            ])?;
        }
        let mut champions = conn.appender("champion_stat")?;
        for c in &t.champions {
            champions.append_row(params![
                "compendium",
                c.champion,
                c.property,
                c.difficulty,
                c.value,
                c.text
            ])?;
        }
    }
    conn.execute_batch("COMMIT")?;
    Ok(())
}
