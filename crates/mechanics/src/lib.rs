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

use std::path::Path;

use anyhow::{Context, Result};
use duckdb::params;
use focalpoint_db::{Database, MECHANICS_SQL, MECHANICS_VERSION, meta_key};

pub mod aegis;
pub mod clarity;
pub mod compendium;
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

/// Read every source under `dir` and replace the `mechanic_*` tables.
pub fn import(db: &Database, dir: &Path, mut progress: impl FnMut(&str)) -> Result<Vec<Summary>> {
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

    progress("writing");
    write(db, &read)?;
    db.set_meta(meta_key::MECHANICS_VERSION, &MECHANICS_VERSION.to_string())?;
    db.set_meta(meta_key::MECHANICS_SOURCES, &dir.display().to_string())?;
    let now: String =
        db.conn()
            .query_row("SELECT strftime(now(), '%Y-%m-%dT%H:%M:%SZ')", [], |r| {
                r.get(0)
            })?;
    db.set_meta(meta_key::MECHANICS_IMPORTED_AT, &now)?;
    Ok(read.into_iter().map(Source::summary).collect())
}

fn count(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

fn write(db: &Database, read: &[Source]) -> Result<()> {
    let conn = db.conn();
    conn.execute_batch("BEGIN TRANSACTION")?;
    conn.execute_batch(MECHANICS_SQL)?;
    {
        let mut sources = conn.appender("mechanic_source")?;
        let mut entries = conn.appender("mechanic_entry")?;
        let mut subjects = conn.appender("mechanic_subject")?;
        let mut keywords = conn.appender("mechanic_keyword")?;
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
    conn.execute_batch("COMMIT")?;
    Ok(())
}
