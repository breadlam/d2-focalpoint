//! The Clarity database (`descriptions/clarity.json` of
//! github.com/Database-Clarity/Live-Clarity-Database): community-written
//! descriptions keyed by the perk's hash.
//!
//! Each entry's English description is a list of blocks, one line each;
//! a block holds inline runs of text. Blocks and runs can be marked `pvp`
//! or `pve`: `PvP` text is left out. Runs marked with an element or ammo
//! class (`solar`, `special`, ...) are the keywords Clarity highlights.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::{Entry, SubjectRef};

/// Run classes kept as keyword marks.
const MARKS: [&str; 13] = [
    "arc",
    "solar",
    "void",
    "stasis",
    "strand",
    "kinetic",
    "primary",
    "special",
    "heavy",
    "barrier",
    "overload",
    "unstoppable",
    "enhancedArrow",
];

/// Read `clarity.json`.
pub fn read(path: &Path) -> Result<Vec<Entry>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let root: Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let map = root.as_object().context("clarity.json is not an object")?;
    let mut entries: Vec<(u64, Entry)> = map.values().filter_map(entry).collect();
    entries.sort_by_key(|(hash, _)| *hash);
    Ok(entries.into_iter().map(|(_, e)| e).collect())
}

fn classes(v: &Value) -> Vec<&str> {
    v.get("classNames")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn hash_of(v: &Value, key: &str) -> Option<u32> {
    v.get(key)
        .and_then(Value::as_u64)
        .and_then(|h| u32::try_from(h).ok())
}

fn entry(v: &Value) -> Option<(u64, Entry)> {
    let hash = hash_of(v, "hash")?;
    let name = v.get("name")?.as_str()?.to_string();
    let section = v
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let blocks = v.pointer("/descriptions/en").and_then(Value::as_array)?;
    let mut lines = Vec::new();
    let mut keywords = Vec::new();
    for block in blocks {
        if classes(block).contains(&"pvp") {
            continue;
        }
        let mut line = String::new();
        for run in block
            .get("linesContent")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let cls = classes(run);
            if cls.contains(&"pvp") {
                continue;
            }
            let Some(t) = run.get("text").and_then(Value::as_str) else {
                continue;
            };
            line.push_str(t);
            if let Some(mark) = cls.iter().find(|c| MARKS.contains(c)) {
                let k = t.trim();
                if !k.is_empty() {
                    keywords.push((k.to_string(), (*mark).to_string()));
                }
            }
        }
        lines.push(line.trim_end().to_string());
    }
    // Spacer blocks are blank lines; keep single blank lines only.
    let mut text = lines.join("\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        return None;
    }
    let mut subjects = vec![SubjectRef {
        hash,
        hash_kind: "item",
        rule: "hash",
    }];
    if let Some(item) = hash_of(v, "itemHash") {
        subjects.push(SubjectRef {
            hash: item,
            hash_kind: "item",
            rule: "item",
        });
    }
    Some((
        u64::from(hash),
        Entry {
            section,
            name,
            text,
            subjects,
            keywords,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pvp_text_is_left_out_and_marked_runs_are_keywords() {
        let v: Value = serde_json::from_str(
            r#"{"hash": 7, "name": "Surge", "type": "Armor Mod General", "itemHash": 9,
                "descriptions": {"en": [
                  {"linesContent": [{"text": "Grants "}, {"text": "Radiant", "classNames": ["solar"]}, {"text": " on kill."}]},
                  {"classNames": ["pve"], "linesContent": [{"text": "PVE: 20% damage"}]},
                  {"classNames": ["pvp"], "linesContent": [{"text": "PVP: 5% damage"}]},
                  {"classNames": ["spacer"]},
                  {"classNames": ["spacer"]},
                  {"linesContent": [{"text": "Enhanced: longer", "classNames": ["enhancedArrow"]}, {"text": " (PVP only)", "classNames": ["pvp"]}]}
                ]}}"#,
        )
        .unwrap();
        let (_, e) = entry(&v).unwrap();
        assert_eq!(
            e.text,
            "Grants Radiant on kill.\nPVE: 20% damage\n\nEnhanced: longer"
        );
        assert_eq!(
            e.keywords,
            [
                ("Radiant".to_string(), "solar".to_string()),
                ("Enhanced: longer".to_string(), "enhancedArrow".to_string())
            ]
        );
        assert_eq!(
            e.subjects
                .iter()
                .map(|s| (s.hash, s.rule))
                .collect::<Vec<_>>(),
            [(7, "hash"), (9, "item")]
        );
    }
}
