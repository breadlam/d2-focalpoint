//! The contexts a build is evaluated in, and their profiles
//! (`docs/contexts.md`).
//!
//! A build's worth depends on where it is played: kill-driven engines
//! thrive on easy, dense enemies and flounder where enemies are hard to kill
//! or absent; boss-damage builds live on hits. Contexts run from activity
//! groups through activities down to encounters, each a child of the one
//! above.
//!
//! A profile is open-ended — any number of named **features** (a number
//! and/or a text) and timed **events** (damage windows, add waves,
//! mechanics), each with its source — so detail can be added without
//! changing the schema:
//!
//! * `runs` — measured from the recorded clears: how many kills are to be
//!   had, how they are made, how often players die, how long runs take;
//! * `aegis` — boss encounters from the boss damage workbook's `Bosses`
//!   sheet: health, adds during damage, damage-phase length, setup;
//! * `curated` — hand-maintained detail in the repository
//!   (`data/contexts.toml`), added to or creating profiles by id.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{Context as _, Result};
use focalpoint_db::Database;
use serde::Deserialize;

use crate::aegis::Boss;

/// A context and its profile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Context {
    /// `group:<slug>`, `activity:<slug>` or `encounter:<activity slug>/<boss slug>`
    /// (the activity as the encounter's source names it).
    pub id: String,
    /// `group`, `activity` or `encounter`.
    pub kind: String,
    pub parent: Option<String>,
    pub name: String,
    /// The manifest activities it covers.
    pub activities: Vec<u32>,
    pub features: Vec<Feature>,
    pub events: Vec<Event>,
}

/// A named property of a context: a number, a text, or both.
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    pub name: String,
    pub value: Option<f64>,
    pub text: Option<String>,
    pub source: String,
}

/// Something that happens in a context at a time: a damage window, an add
/// wave, a mechanic.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub kind: String,
    pub start_s: Option<f64>,
    pub length_s: Option<f64>,
    pub detail: Option<String>,
    pub source: String,
}

impl Context {
    pub(crate) fn number(&mut self, name: &str, value: Option<f64>, source: &str) {
        if let Some(v) = value.filter(|v| v.is_finite()) {
            self.features.push(Feature {
                name: name.into(),
                value: Some(v),
                text: None,
                source: source.into(),
            });
        }
    }

    pub(crate) fn text(&mut self, name: &str, text: Option<&str>, source: &str) {
        if let Some(t) = text.map(str::trim).filter(|t| !t.is_empty()) {
            self.features.push(Feature {
                name: name.into(),
                value: None,
                text: Some(t.into()),
                source: source.into(),
            });
        }
    }

    /// A feature's value from any source, the first given.
    #[must_use]
    pub fn feature(&self, name: &str) -> Option<&Feature> {
        self.features.iter().find(|f| f.name == name)
    }
}

/// Lower case letters and digits, runs of anything else as one `-`.
#[must_use]
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// An activity's name without its difficulty, as the community names it:
/// "The Desert Perpetual (Epic): Standard" is "desert perpetual epic",
/// "Desert Perpetual - Epic" too.
#[must_use]
pub fn base_name(name: &str) -> String {
    let base = name.split(':').next().unwrap_or(name);
    let words: Vec<String> = base
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .filter(|w| w != "the")
        .collect();
    words.join(" ")
}

/// Profiles of every activity group and activity with scored clears,
/// measured from those clears: their pace and kill mix, and what their
/// difficulty and modifiers ("skulls") put in front of the players — the
/// difficulty step most clears are at and its activity level, and the
/// share of clears with each kind of Champion, Difficult or Deadly
/// combatants, Galvanized (enemies harder to stagger), Feats and banes.
pub fn from_runs(db: &Database, checkpoint_share: f64) -> Result<Vec<Context>> {
    let sql = format!(
        "WITH checkpoints AS (
             SELECT activity_hash,
                    avg((NOT coalesce(started_from_beginning, true))::INTEGER) >= {checkpoint_share} AS resumable
             FROM run GROUP BY 1),
         targets AS (
             SELECT DISTINCT instance_id FROM run_score JOIN activity a USING (activity_hash)
             WHERE a.target_group IS NOT NULL),
         skulls AS (
             SELECT r.instance_id,
                    bool_or(s.name LIKE '%[Shield-Piercing]%') AS barrier,
                    bool_or(s.name LIKE '%[Disruption]%') AS overload,
                    bool_or(s.name LIKE '%[Stagger]%') AS unstoppable,
                    bool_or(s.name LIKE 'Difficult Combatants%') AS difficult,
                    bool_or(s.name LIKE 'Deadly Combatants%') AS deadly,
                    max(CASE s.name WHEN 'Galvanized I' THEN 1 WHEN 'Galvanized II' THEN 2
                                    WHEN 'Galvanized III' THEN 3 END) AS galvanized,
                    bool_or(s.name = 'Feat: Cutthroat Combat') AS cutthroat,
                    bool_or(s.name = 'Feat: Battalions') AS battalions,
                    bool_or(s.description LIKE '%Banes,%') AS banes
             FROM (SELECT instance_id, unnest(skull_hashes) AS skull_hash
                   FROM run SEMI JOIN targets USING (instance_id)) r
             JOIN skull s USING (skull_hash)
             GROUP BY 1),
         kills AS (
             SELECT instance_id, membership_id, sum(precision_kills) AS precision, sum(super_kills) AS supers,
                    sum(ability_kills + grenade_kills + melee_kills + super_kills) AS abilities
             FROM run_player GROUP BY ALL),
         clears AS (
             SELECT a.target_group AS grp, a.name AS activity, r.activity_hash, r.is_solo,
                    NOT r.is_solo AND NOT r.is_partial AS is_full,
                    r.kills * 60.0 / greatest(r.played_s, 60) AS kpm,
                    r.deaths * 600.0 / greatest(r.played_s, 60) AS dpm,
                    k.precision / nullif(r.kills, 0) AS precision_share,
                    k.abilities / nullif(r.kills, 0) AS ability_share,
                    k.supers / nullif(r.kills, 0) AS super_share,
                    r.duration_s, ru.player_count, c.resumable::INTEGER AS resumable,
                    t.name AS tier, t.activity_level, ru.equipment_locked,
                    coalesce(s.barrier, false)::INTEGER AS barrier,
                    coalesce(s.overload, false)::INTEGER AS overload,
                    coalesce(s.unstoppable, false)::INTEGER AS unstoppable,
                    coalesce(s.difficult, false)::INTEGER AS difficult,
                    coalesce(s.deadly, false)::INTEGER AS deadly,
                    coalesce(s.galvanized, 0) AS galvanized,
                    coalesce(s.cutthroat, false)::INTEGER AS cutthroat,
                    coalesce(s.battalions, false)::INTEGER AS battalions,
                    coalesce(s.banes, false)::INTEGER AS banes
             FROM run_score r JOIN activity a USING (activity_hash)
             JOIN run ru USING (instance_id)
             JOIN kills k USING (instance_id, membership_id)
             LEFT JOIN checkpoints c USING (activity_hash)
             LEFT JOIN difficulty_tier t
                    ON t.collection_hash = a.difficulty_collection_hash AND t.tier = ru.difficulty_tier
             LEFT JOIN skulls s USING (instance_id)
             WHERE a.target_group IS NOT NULL)
         SELECT grp, activity, list(DISTINCT activity_hash), count(*),
                avg(is_solo::INTEGER),
                median(kpm) FILTER (WHERE is_full), median(precision_share) FILTER (WHERE is_full),
                median(ability_share) FILTER (WHERE is_full), median(super_share) FILTER (WHERE is_full),
                median(dpm) FILTER (WHERE is_full), median(duration_s) FILTER (WHERE is_full) / 60.0,
                median(player_count) FILTER (WHERE is_full), avg(resumable),
                mode(tier), median(activity_level), avg(equipment_locked::INTEGER),
                avg(barrier), avg(overload), avg(unstoppable), avg(difficult), avg(deadly),
                avg(galvanized), avg(cutthroat), avg(battalions), avg(banes)
         FROM clears
         GROUP BY GROUPING SETS ((grp), (grp, activity))
         ORDER BY grp, activity NULLS FIRST"
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let group: String = r.get(0)?;
        let activity: Option<String> = r.get(1)?;
        let hashes: duckdb::types::Value = r.get(2)?;
        let activities = match hashes {
            duckdb::types::Value::List(v) => v
                .into_iter()
                .filter_map(|x| match x {
                    duckdb::types::Value::UInt(h) => Some(h),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let mut c = match &activity {
            None => Context {
                id: format!("group:{}", slug(&group)),
                kind: "group".into(),
                name: group.clone(),
                ..Context::default()
            },
            Some(a) => Context {
                id: format!("activity:{}", slug(a)),
                kind: "activity".into(),
                parent: Some(format!("group:{}", slug(&group))),
                name: a.clone(),
                ..Context::default()
            },
        };
        c.activities = activities;
        let n = |i: usize| -> Result<Option<f64>> { Ok(r.get::<_, Option<f64>>(i)?) };
        c.number("clears", Some(r.get::<_, i64>(3)? as f64), "runs");
        c.number("solo_share", n(4)?, "runs");
        c.number("kills_per_min", n(5)?, "runs");
        c.number("precision_kill_share", n(6)?, "runs");
        c.number("ability_kill_share", n(7)?, "runs");
        c.number("super_kill_share", n(8)?, "runs");
        c.number("deaths_per_10min", n(9)?, "runs");
        c.number("duration_min", n(10)?, "runs");
        c.number("fireteam_size", n(11)?, "runs");
        c.number("resumable", n(12)?, "runs");
        c.text(
            "difficulty",
            r.get::<_, Option<String>>(13)?
                .as_deref()
                .filter(|t| !t.is_empty()),
            "runs",
        );
        c.number("activity_level", n(14)?, "runs");
        c.number("equipment_locked", n(15)?, "runs");
        for (i, name) in [
            "champions.barrier",
            "champions.overload",
            "champions.unstoppable",
            "combatants.difficult",
            "combatants.deadly",
            "galvanized",
            "feat.cutthroat_combat",
            "feat.battalions",
            "banes",
        ]
        .into_iter()
        .enumerate()
        {
            c.number(name, n(16 + i)?, "runs");
        }
        out.push(c);
    }
    Ok(out)
}

/// Encounter profiles from the boss damage workbook's bosses, each a child
/// of the activity of its name with the most scored clears; `activities`
/// are `(hash, name)` of every manifest activity, `scored` the activity
/// contexts measured from runs.
#[must_use]
pub fn from_bosses(
    bosses: &[Boss],
    activities: &[(u32, String)],
    scored: &[Context],
) -> Vec<Context> {
    let mut by_base: HashMap<String, Vec<u32>> = HashMap::new();
    for (hash, name) in activities {
        by_base.entry(base_name(name)).or_default().push(*hash);
    }
    let clears = |c: &Context| c.feature("clears").and_then(|f| f.value).unwrap_or(0.0);
    let mut out = Vec::new();
    for b in bosses {
        let base = base_name(&b.activity);
        let hashes = by_base.get(&base).cloned().unwrap_or_default();
        let parent = scored
            .iter()
            .filter(|c| c.kind == "activity" && base_name(&c.name) == base)
            .max_by(|a, b| clears(a).total_cmp(&clears(b)))
            .map(|c| c.id.clone());
        let mut c = Context {
            id: format!("encounter:{}/{}", slug(&b.activity), slug(&b.name)),
            kind: "encounter".into(),
            parent,
            name: format!("{}: {}", b.activity, b.name),
            activities: hashes,
            ..Context::default()
        };
        let s = "aegis";
        c.text("activity", Some(&b.activity), s);
        c.number("boss.health", b.health, s);
        c.number("boss.effective_health", b.effective_health, s);
        c.number("boss.dps_window_s", b.phase_s, s);
        c.text("boss.range", b.range.as_deref(), s);
        c.text("adds.during_dps", b.adds.as_deref(), s);
        c.text("adds.range", b.adds_range.as_deref(), s);
        c.text("setup", b.setup.as_deref(), s);
        c.text("mechanics", b.mechanics.as_deref(), s);
        c.text("boss.species", b.species.as_deref(), s);
        c.text("boss.model", b.model.as_deref(), s);
        c.text("notes", b.notes.as_deref(), s);
        c.number("dps.clearable", b.clearable_dps, s);
        c.number("dps.one_phase", b.one_phase_dps, s);
        c.text("dps.one_phase_description", b.one_phase.as_deref(), s);
        c.text("mods", b.mods.as_deref(), s);
        if let Some(len) = b.phase_s {
            c.events.push(Event {
                kind: "dps_window".into(),
                start_s: None,
                length_s: Some(len),
                detail: b.one_phase.clone(),
                source: s.into(),
            });
        }
        out.push(c);
    }
    out
}

/// `data/contexts.toml`: hand-maintained detail.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CuratedFile {
    #[serde(default)]
    context: Vec<CuratedContext>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CuratedContext {
    id: String,
    kind: Option<String>,
    parent: Option<String>,
    name: Option<String>,
    #[serde(default)]
    features: BTreeMap<String, toml::Value>,
    #[serde(default)]
    event: Vec<CuratedEvent>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CuratedEvent {
    kind: String,
    start_s: Option<f64>,
    length_s: Option<f64>,
    detail: Option<String>,
}

/// The repository's `data/contexts.toml`, built in.
pub const CURATED: &str = include_str!("../../../data/contexts.toml");

/// Read hand-maintained detail from a file.
pub fn curated(path: &Path) -> Result<Vec<Context>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_curated(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Read hand-maintained detail in the format of `data/contexts.toml`.
pub fn parse_curated(text: &str) -> Result<Vec<Context>> {
    let file: CuratedFile = toml::from_str(text)?;
    Ok(file
        .context
        .into_iter()
        .map(|c| {
            let mut ctx = Context {
                kind: c
                    .kind
                    .unwrap_or_else(|| c.id.split(':').next().unwrap_or("").to_string()),
                parent: c.parent,
                name: c.name.unwrap_or_default(),
                id: c.id,
                ..Context::default()
            };
            for (name, v) in c.features {
                match v {
                    toml::Value::Integer(i) => ctx.number(&name, Some(i as f64), "curated"),
                    toml::Value::Float(f) => ctx.number(&name, Some(f), "curated"),
                    toml::Value::Boolean(b) => {
                        ctx.number(&name, Some(f64::from(u8::from(b))), "curated");
                    }
                    toml::Value::String(s) => ctx.text(&name, Some(&s), "curated"),
                    other => ctx.text(&name, Some(&other.to_string()), "curated"),
                }
            }
            ctx.events = c
                .event
                .into_iter()
                .map(|e| Event {
                    kind: e.kind,
                    start_s: e.start_s,
                    length_s: e.length_s,
                    detail: e.detail,
                    source: "curated".into(),
                })
                .collect();
            ctx
        })
        .collect())
}

/// Merge profiles by id, in order: a later list adds features and events
/// to a context of an earlier one (a feature of the same name and source
/// replaces it) and fills its kind, parent, name and activities where
/// unset.
#[must_use]
pub fn merge(lists: Vec<Vec<Context>>) -> Vec<Context> {
    let mut out: Vec<Context> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for list in lists {
        for c in list {
            let Some(&i) = at.get(&c.id) else {
                at.insert(c.id.clone(), out.len());
                out.push(c);
                continue;
            };
            let e = &mut out[i];
            if e.kind.is_empty() {
                e.kind = c.kind;
            }
            if e.parent.is_none() {
                e.parent = c.parent;
            }
            if e.name.is_empty() {
                e.name = c.name;
            }
            for h in c.activities {
                if !e.activities.contains(&h) {
                    e.activities.push(h);
                }
            }
            for f in c.features {
                e.features
                    .retain(|g| !(g.name == f.name && g.source == f.source));
                e.features.push(f);
            }
            e.events.extend(c.events);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two Master Raid activities: Vault of Glass (two fireteam clears, one
    /// solo) and King's Fall (one fireteam clear, resumed from a
    /// checkpoint).
    fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(&dir.path().join("c.duckdb")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO activity (activity_hash, name, target_group, difficulty_collection_hash) VALUES
                    (1, 'Vault of Glass: Master', 'Master Raid', 7), (2, 'King''s Fall: Master', 'Master Raid', NULL);
                 INSERT INTO difficulty_tier VALUES (7, 0, 'Normal', 31, 10, NULL, 0), (7, 1, 'Master', 40, 40, 300, 0);
                 INSERT INTO skull (skull_hash, name, kind, description) VALUES
                    (100, 'Galvanized II', 'other', 'Combatants are harder to stun.'),
                    (101, 'Deadly Combatants [Shield-Piercing] [Stagger]', 'other', 'Extreme numbers of advanced and challenging combatants will appear.'),
                    (102, 'Rage and Meteors', 'other', 'Deadly Banes, Rage and Meteors, will appear on enemy combatants.');
                 INSERT INTO run (instance_id, started_at, activity_hash, started_from_beginning, player_count, source, is_tracked,
                                  difficulty_tier, skull_hashes)
                 VALUES (10, '2026-07-01', 1, true, 6, 'archive', false, 1, [100, 101]),
                        (11, '2026-07-02', 1, true, 6, 'archive', false, 1, [100, 101, 102]),
                        (12, '2026-07-03', 1, true, 1, 'archive', false, 0, []),
                        (20, '2026-07-04', 2, false, 6, 'archive', false, 3, NULL);
                 INSERT INTO run_player (instance_id, membership_id, character_id, started_at, activity_hash,
                     kills, deaths, assists, precision_kills, super_kills, melee_kills, grenade_kills, ability_kills,
                     completed, is_clear, time_played_s, has_build)
                 SELECT i, 1, 1, TIMESTAMP '2026-07-01', a, k, 0, 0, p, 0, 0, 0, 0, true, true, 600, true
                 FROM (VALUES (10, 1, 60, 6), (11, 1, 120, 12), (12, 1, 30, 3), (20, 2, 30, 15)) v(i, a, k, p);
                 INSERT INTO run_score (instance_id, membership_id, activity_hash, started_at, is_solo, is_partial,
                     has_build, kills, deaths, duration_s, played_s, kill_index, speed, survival, score, perf)
                 VALUES (10, 1, 1, '2026-07-01', false, false, true, 60, 1, 600, 600, 1, 1, 1, 1, 0),
                        (11, 1, 1, '2026-07-02', false, false, true, 120, 0, 600, 600, 1, 1, 1, 1, 0),
                        (12, 1, 1, '2026-07-03', true, false, true, 30, 0, 600, 600, 1, 1, 1, 1, 0),
                        (20, 1, 2, '2026-07-04', false, false, true, 30, 2, 600, 600, 1, 1, 1, 1, 0);",
            )
            .unwrap();
        (dir, db)
    }

    #[test]
    fn runs_profile_groups_and_activities() {
        let (_dir, db) = db();
        let c = from_runs(&db, 0.01).unwrap();
        let ids: Vec<&str> = c.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "group:master-raid",
                "activity:king-s-fall-master",
                "activity:vault-of-glass-master"
            ]
        );
        let value = |c: &Context, f: &str| c.feature(f).and_then(|f| f.value);
        let vog = &c[2];
        assert_eq!(vog.parent.as_deref(), Some("group:master-raid"));
        assert_eq!(vog.activities, [1]);
        assert_eq!(value(vog, "clears"), Some(3.0));
        assert!((value(vog, "solo_share").unwrap() - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(
            value(vog, "kills_per_min"),
            Some(9.0),
            "median of 6 and 12, fireteam clears only"
        );
        assert_eq!(value(vog, "precision_kill_share"), Some(0.1));
        assert_eq!(value(vog, "resumable"), Some(0.0));
        assert_eq!(value(&c[1], "resumable"), Some(1.0));
        assert_eq!(value(&c[0], "clears"), Some(4.0));
        let text = |c: &Context, f: &str| c.feature(f).and_then(|f| f.text.clone());
        assert_eq!(text(vog, "difficulty").as_deref(), Some("Master"));
        assert_eq!(value(vog, "activity_level"), Some(40.0));
        assert!((value(vog, "champions.barrier").unwrap() - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(value(vog, "champions.overload"), Some(0.0));
        assert!((value(vog, "galvanized").unwrap() - 4.0 / 3.0).abs() < 1e-9);
        assert!((value(vog, "banes").unwrap() - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(text(&c[1], "difficulty"), None, "King's Fall has no ladder");
    }

    #[test]
    fn activities_are_matched_by_base_name() {
        assert_eq!(
            base_name("The Desert Perpetual (Epic): Standard"),
            "desert perpetual epic"
        );
        assert_eq!(
            base_name("Desert Perpetual - Epic"),
            "desert perpetual epic"
        );
        assert_eq!(
            base_name("Vault of Glass: Master"),
            base_name("Vault of Glass")
        );
        assert_eq!(base_name("Crota's End: Master"), "crota's end");
        assert_eq!(slug("Crota's End: Master"), "crota-s-end-master");
    }

    #[test]
    fn bosses_become_encounters_under_their_most_cleared_activity() {
        let master = Context {
            id: "activity:vault-of-glass-master".into(),
            kind: "activity".into(),
            name: "Vault of Glass: Master".into(),
            features: vec![Feature {
                name: "clears".into(),
                value: Some(100.0),
                text: None,
                source: "runs".into(),
            }],
            ..Context::default()
        };
        let boss = Boss {
            activity: "Vault of Glass".into(),
            name: "Atheon".into(),
            health: Some(247_148.0),
            adds: Some("Many".into()),
            phase_s: Some(30.0),
            ..Boss::default()
        };
        let acts = [
            (1, "Vault of Glass: Normal".to_string()),
            (2, "Vault of Glass: Master".to_string()),
        ];
        let e = &from_bosses(&[boss], &acts, &[master])[0];
        assert_eq!(e.id, "encounter:vault-of-glass/atheon");
        assert_eq!(e.parent.as_deref(), Some("activity:vault-of-glass-master"));
        assert_eq!(e.activities, [1, 2]);
        assert_eq!(
            e.feature("boss.dps_window_s").and_then(|f| f.value),
            Some(30.0)
        );
        assert_eq!(
            e.feature("adds.during_dps").and_then(|f| f.text.as_deref()),
            Some("Many")
        );
        assert_eq!(e.events[0].length_s, Some(30.0));
    }

    #[test]
    fn the_built_in_curated_file_parses() {
        parse_curated(CURATED).unwrap();
    }

    #[test]
    fn curated_detail_adds_to_profiles_and_replaces_its_own_features() {
        let curated = parse_curated(
            r#"
            [[context]]
            id = "encounter:vault-of-glass/atheon"
            features = { "adds.waves" = 4, "mechanics.teleport" = true, "boss.dps_window_s" = 28.5, "note" = "oracles" }
            [[context.event]]
            kind = "add_wave"
            start_s = 0.0
            detail = "oracles and hobgoblins"

            [[context]]
            id = "encounter:new/boss"
            name = "New: Boss"
            "#,
        )
        .unwrap();
        let mut aegis = Context {
            id: "encounter:vault-of-glass/atheon".into(),
            kind: "encounter".into(),
            ..Context::default()
        };
        aegis.number("boss.dps_window_s", Some(30.0), "aegis");
        let merged = merge(vec![vec![aegis], curated]);
        assert_eq!(merged.len(), 2);
        let a = &merged[0];
        let windows: Vec<(&str, Option<f64>)> = a
            .features
            .iter()
            .filter(|f| f.name == "boss.dps_window_s")
            .map(|f| (f.source.as_str(), f.value))
            .collect();
        assert_eq!(
            windows,
            [("aegis", Some(30.0)), ("curated", Some(28.5))],
            "sources stand side by side"
        );
        assert_eq!(
            a.feature("mechanics.teleport").and_then(|f| f.value),
            Some(1.0)
        );
        assert_eq!(
            a.feature("note").and_then(|f| f.text.as_deref()),
            Some("oracles")
        );
        assert_eq!(a.events[0].kind, "add_wave");
        assert_eq!(
            (merged[1].kind.as_str(), merged[1].name.as_str()),
            ("encounter", "New: Boss")
        );
        assert!(
            parse_curated("[[context]]\nid = \"x\"\nunknown = 1").is_err(),
            "typos are errors"
        );
    }
}
