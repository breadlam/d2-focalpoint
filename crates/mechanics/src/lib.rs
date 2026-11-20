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
//!   matched to the manifest by [`names`].

use std::path::Path;

use anyhow::{Context, Result};
use duckdb::params;
use focalpoint_db::{Database, MECHANICS_SQL, MECHANICS_VERSION, meta_key};

pub mod clarity;
pub mod compendium;
pub mod names;
pub mod sheet;

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
    let mut read: Vec<(&'static str, Provenance, Vec<Entry>)> = Vec::new();

    progress("clarity");
    let clarity_dir = dir.join("clarity");
    read.push((
        "clarity",
        provenance(&clarity_dir)?,
        clarity::read(&clarity_dir.join("clarity.json"))?,
    ));

    progress("compendium");
    let compendium_dir = dir.join("compendium");
    read.push((
        "compendium",
        provenance(&compendium_dir)?,
        compendium::read(&compendium_dir, &names)?,
    ));

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

    Ok(read
        .into_iter()
        .map(|(source, provenance, entries)| Summary {
            source,
            provenance,
            entries: entries.len(),
            with_subject: entries.iter().filter(|e| !e.subjects.is_empty()).count(),
            subjects: entries.iter().map(|e| e.subjects.len()).sum(),
            keywords: entries.iter().map(|e| e.keywords.len()).sum(),
        })
        .collect())
}

fn write(db: &Database, read: &[(&'static str, Provenance, Vec<Entry>)]) -> Result<()> {
    let conn = db.conn();
    conn.execute_batch("BEGIN TRANSACTION")?;
    conn.execute_batch(MECHANICS_SQL)?;
    {
        let mut sources = conn.appender("mechanic_source")?;
        let mut entries = conn.appender("mechanic_entry")?;
        let mut subjects = conn.appender("mechanic_subject")?;
        let mut keywords = conn.appender("mechanic_keyword")?;
        for (source, prov, list) in read {
            sources.append_row(params![
                source,
                prov.title,
                prov.url,
                prov.fetched,
                i32::try_from(list.len()).unwrap_or(i32::MAX)
            ])?;
            for (i, e) in list.iter().enumerate() {
                let id = i32::try_from(i).unwrap_or(i32::MAX);
                entries.append_row(params![source, id, e.section, e.name, e.text])?;
                let mut seen = Vec::new();
                for s in &e.subjects {
                    if seen.contains(&(s.hash, s.hash_kind)) {
                        continue;
                    }
                    seen.push((s.hash, s.hash_kind));
                    subjects.append_row(params![source, id, s.hash, s.hash_kind, s.rule])?;
                }
                for (k, m) in &e.keywords {
                    keywords.append_row(params![source, id, k, m])?;
                }
            }
        }
    }
    conn.execute_batch("COMMIT")?;
    Ok(())
}
