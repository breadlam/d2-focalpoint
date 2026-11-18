//! Builds stage: production's `build_segment` timelines → `segment`, plus the
//! deduplicated `build` / `build_component` and `loadout` / `loadout_item`
//! tables.
//!
//! Parsing and build extraction run in parallel; a single collector thread
//! deduplicates builds and loadouts by id and verifies that no two different
//! builds ever share a fingerprint (a collision aborts the import).

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::Path;

use anyhow::{Context, Result, bail};
use crossbeam_channel::bounded;
use duckdb::Connection;
use rayon::prelude::*;
use searchlight_core::build::{Build, ComponentKind, extract_build, loadout_id};
use searchlight_core::fingerprint::Fingerprinter;
use searchlight_core::loadout::{LoadoutState, stat_hash};
use searchlight_core::{Catalog, Slot};

use crate::progress::Progress;
use crate::rows::{BuildComponentRow, BuildRow, LoadoutItemRow, LoadoutRow, SegmentRow};
use crate::sink::{Batcher, Sink, staging_writers};
use crate::sqlite;

const CHUNK_ROWS: i64 = 10_000;
const BATCH: usize = 4_096;

/// Counters from the builds stage.
#[derive(Debug, Default, Clone)]
pub struct BuildStats {
    pub segments: u64,
    pub bad_loadouts: u64,
    pub builds: u64,
    pub components: u64,
    pub loadouts: u64,
    pub loadout_items: u64,
    /// Distinct plug/item hashes that the manifest could not resolve.
    pub unresolved_hashes: u64,
}

/// One processed `build_segment` row.
struct Processed {
    segment: SegmentRow,
    build: Build,
    loadout: LoadoutState,
}

pub fn load(
    duck: &Connection,
    main_db: &Path,
    catalog: &Catalog,
    progress: &Progress,
) -> Result<BuildStats> {
    let src = sqlite::open(main_db)?;
    let total = sqlite::count(&src, "build_segment")?;
    let ranges = sqlite::rowid_ranges(&src, "build_segment", CHUNK_ROWS)?;
    drop(src);
    let bar = progress.bar("build timelines", total);

    let (tx, rx) = bounded::<Vec<Processed>>(256);
    let collector = {
        let duck = duck.try_clone()?;
        std::thread::Builder::new()
            .name("builds-collector".into())
            .spawn(move || collect(&duck, &rx))?
    };

    let produced = ranges
        .par_iter()
        .map(|&(lo, hi)| -> Result<u64> {
            let conn = sqlite::open(main_db)?;
            let mut stmt = conn.prepare(
                "SELECT instance_id, membership_id, segment_index, run_relative_start_s, duration_s, loadout_state \
                 FROM build_segment WHERE rowid BETWEEN ?1 AND ?2",
            )?;
            let mut rows = stmt.query([lo, hi])?;
            let mut batch = Vec::with_capacity(512);
            let (mut bad, mut n) = (0, 0);
            while let Some(r) = rows.next()? {
                n += 1;
                let json: String = r.get(5)?;
                let Ok(loadout) = LoadoutState::from_json(&json) else {
                    bad += 1;
                    continue;
                };
                let build = extract_build(&loadout, catalog);
                let stat_of = |h| loadout.stat(h);
                batch.push(Processed {
                    segment: SegmentRow {
                        instance_id: r.get(0)?,
                        membership_id: r.get(1)?,
                        segment_index: r.get(2)?,
                        start_s: r.get(3)?,
                        duration_s: r.get(4)?,
                        build_id: build.id,
                        loadout_id: loadout_id(&loadout),
                        stats: [
                            stat_of(stat_hash::POWER),
                            stat_of(stat_hash::WEAPONS),
                            stat_of(stat_hash::HEALTH),
                            stat_of(stat_hash::CLASS),
                            stat_of(stat_hash::GRENADE),
                            stat_of(stat_hash::SUPER),
                            stat_of(stat_hash::MELEE),
                        ],
                    },
                    build,
                    loadout,
                });
                if batch.len() >= 512 {
                    tx.send(std::mem::take(&mut batch)).context("builds collector stopped")?;
                }
            }
            if !batch.is_empty() {
                tx.send(batch).context("builds collector stopped")?;
            }
            bar.inc(n);
            Ok(bad)
        })
        .sum::<Result<u64>>();
    drop(tx);
    // Settle writers before producers: a writer's error is the root cause of
    // a producer's "writer stopped" error.
    let mut stats = collector
        .join()
        .map_err(|_| anyhow::anyhow!("builds collector panicked"))??;
    let bad = produced.context("reading build_segment")?;
    stats.bad_loadouts = bad;
    bar.finish();
    Ok(stats)
}

/// Secondary fingerprint of a build's components, used to detect `build_id`
/// collisions (different content, same id).
fn check_fingerprint(b: &Build) -> i64 {
    let mut fp = Fingerprinter::new(b"searchlight.build.check", 1);
    for c in &b.components {
        fp.u8(c.kind as u8)
            .u32(c.hash)
            .u32(c.parent_hash.unwrap_or(0))
            .u16(c.quantity);
        fp.u8(c.slot.map_or(0, |s| s as u8));
    }
    fp.finish()
}

fn collect(
    duck: &Connection,
    rx: &crossbeam_channel::Receiver<Vec<Processed>>,
) -> Result<BuildStats> {
    let segments = Sink::<SegmentRow>::spawn_parallel(duck, "segment", staging_writers())?;
    let builds = Sink::<BuildRow>::spawn(duck, "build")?;
    let components =
        Sink::<BuildComponentRow>::spawn_parallel(duck, "build_component", staging_writers())?;
    let loadouts = Sink::<LoadoutRow>::spawn(duck, "loadout")?;
    let items = Sink::<LoadoutItemRow>::spawn_parallel(duck, "loadout_item", staging_writers())?;
    let mut seg_b = Batcher::new(&segments.sender(), BATCH);
    let mut build_b = Batcher::new(&builds.sender(), BATCH);
    let mut comp_b = Batcher::new(&components.sender(), BATCH);
    let mut lo_b = Batcher::new(&loadouts.sender(), BATCH);
    let mut item_b = Batcher::new(&items.sender(), BATCH);

    let mut seen_builds: HashMap<i64, i64> = HashMap::new();
    let mut seen_loadouts: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let mut unresolved: std::collections::HashSet<u32> = std::collections::HashSet::new();

    let collected = (|| -> Result<()> {
        for batch in rx {
            for p in batch {
                match seen_builds.entry(p.build.id) {
                    Entry::Occupied(e) => {
                        if *e.get() != check_fingerprint(&p.build) {
                            bail!(
                                "build_id collision on {} — bump the build fingerprint version",
                                p.build.id
                            );
                        }
                    }
                    Entry::Vacant(e) => {
                        e.insert(check_fingerprint(&p.build));
                        unresolved.extend(&p.build.unresolved_hashes);
                        build_b.push(build_row(&p.build))?;
                        for c in &p.build.components {
                            comp_b.push(BuildComponentRow {
                                build_id: p.build.id,
                                kind: c.kind.as_str(),
                                slot: c.slot.map(Slot::as_str),
                                component_hash: c.hash,
                                parent_hash: c.parent_hash,
                                quantity: c.quantity,
                                position: c.position,
                            })?;
                        }
                    }
                }
                let lid = p.segment.loadout_id;
                if seen_loadouts.insert(lid) {
                    lo_b.push(LoadoutRow {
                        loadout_id: lid,
                        build_id: p.build.id,
                        artifact_def_hash: p.loadout.artifact.as_ref().map(|a| a.artifact_hash),
                        artifact_active_perks: p
                            .loadout
                            .artifact
                            .as_ref()
                            .map(|a| a.active_perks.clone()),
                    })?;
                    for i in &p.loadout.items {
                        item_b.push(LoadoutItemRow {
                            loadout_id: lid,
                            slot: i.slot().as_str(),
                            bucket_hash: i.bucket_hash,
                            item_hash: i.item_hash,
                            item_instance_id: i.item_instance_id as i64,
                            plug_hashes: i.plug_hashes.clone(),
                            perk_hashes: i.perk_hashes.clone(),
                        })?;
                    }
                }
                seg_b.push(p.segment)?;
            }
        }
        seg_b.flush()?;
        build_b.flush()?;
        comp_b.flush()?;
        lo_b.flush()?;
        item_b.flush()
    })();
    // Batchers hold channel senders; drop them so the writers can finish.
    drop((seg_b, build_b, comp_b, lo_b, item_b));
    // Settle writers before producers: a writer's error is the root cause of
    // a producer's "writer stopped" error.
    let finished = [
        segments.finish(),
        builds.finish(),
        components.finish(),
        loadouts.finish(),
        items.finish(),
    ];
    let [segments, builds, components, loadouts, loadout_items] = finished;
    let stats = BuildStats {
        segments: segments?,
        bad_loadouts: 0,
        builds: builds?,
        components: components?,
        loadouts: loadouts?,
        loadout_items: loadout_items?,
        unresolved_hashes: unresolved.len() as u64,
    };
    collected?;
    Ok(stats)
}

fn build_row(b: &Build) -> BuildRow {
    let mut aspects: Vec<u32> = b.hashes(ComponentKind::Aspect).collect();
    let mut fragments: Vec<u32> = b.hashes(ComponentKind::Fragment).collect();
    let mut artifact_perks: Vec<u32> = b.hashes(ComponentKind::ArtifactPerk).collect();
    let mut armor_mods: Vec<u32> = b.hashes(ComponentKind::ArmorMod).collect();
    for v in [
        &mut aspects,
        &mut fragments,
        &mut artifact_perks,
        &mut armor_mods,
    ] {
        v.sort_unstable();
    }
    BuildRow {
        build_id: b.id,
        class: b.class.as_str().to_string(),
        element: b.element.map(searchlight_core::Element::as_str),
        subclass_hash: b.item_in(Slot::Subclass),
        super_hash: b.first(ComponentKind::Super),
        class_ability_hash: b.first(ComponentKind::ClassAbility),
        movement_hash: b.first(ComponentKind::Movement),
        melee_hash: b.first(ComponentKind::Melee),
        grenade_hash: b.first(ComponentKind::Grenade),
        aspect_hashes: aspects,
        fragment_hashes: fragments,
        kinetic_hash: b.item_in(Slot::Kinetic),
        energy_hash: b.item_in(Slot::Energy),
        power_hash: b.item_in(Slot::Power),
        helmet_hash: b.item_in(Slot::Helmet),
        gauntlets_hash: b.item_in(Slot::Gauntlets),
        chest_hash: b.item_in(Slot::Chest),
        legs_hash: b.item_in(Slot::Legs),
        class_item_hash: b.item_in(Slot::ClassItem),
        exotic_armor_hash: b.exotic_armor_hash,
        exotic_weapon_hash: b.exotic_weapon_hash,
        artifact_hash: b.item_in(Slot::Artifact),
        artifact_perk_hashes: artifact_perks,
        armor_mod_hashes: armor_mods,
        item_hashes: b.item_hashes(),
        component_count: b.components.len() as u16,
        unresolved_count: b.unresolved_hashes.len() as u16,
    }
}
