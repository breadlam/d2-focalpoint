//! End-to-end import over a tiny synthetic backup tree shaped like the real
//! one (production-schema SQLite files, gzipped day shards, an unpulled
//! outbox containing a duplicate shard, and a manifest subset).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use searchlight_db::Database;
use searchlight_import::{ImportOptions, Progress};

const DATA: &str = "builds.rodeo/snapshot-2026-09-20/opt/searchlight/searchlight/data";

fn core_fixture(name: &str) -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../core/tests/fixtures")
        .join(name);
    fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(bytes).unwrap();
    e.finish().unwrap()
}

/// A bare archived report with one Warlock entry.
fn pgcr_json(instance_id: i64, activity: u32, membership_id: i64, kills: u32) -> String {
    format!(
        r#"{{"period":"2026-07-01T12:00:00Z","activityWasStartedFromBeginning":true,"activityDifficultyTier":11,
            "activityDetails":{{"referenceId":{activity},"directorActivityHash":{activity},"instanceId":"{instance_id}",
                                 "mode":82,"modes":[82,7],"isPrivate":false}},
            "entries":[{{"characterId":"{char}","player":{{"destinyUserInfo":{{"membershipType":3,
                "membershipId":"{membership_id}","displayName":"Tester","bungieGlobalDisplayName":"Tester",
                "bungieGlobalDisplayNameCode":42}},"characterClass":"Warlock","lightLevel":550,"emblemHash":9}},
              "values":{{"kills":{{"basic":{{"value":{kills}}}}},"deaths":{{"basic":{{"value":1}}}},
                "completed":{{"basic":{{"value":1}}}},"completionReason":{{"basic":{{"value":0}}}},
                "activityDurationSeconds":{{"basic":{{"value":600}}}},"timePlayedSeconds":{{"basic":{{"value":590}}}}}},
              "extended":{{"weapons":[{{"referenceId":1001,"values":{{"uniqueWeaponKills":{{"basic":{{"value":{kills}}}}}}}}}]}}}}]}}"#,
        char = membership_id + 1
    )
}

fn create_pgcr_table(c: &Connection, with_orig_rowid: bool) {
    let extra = if with_orig_rowid {
        "orig_rowid INT, "
    } else {
        ""
    };
    c.execute_batch(&format!(
        "CREATE TABLE pgcr_raw ({extra}instance_id INTEGER, period TEXT, director_activity_hash INTEGER, \
         fetched_at INTEGER, raw BLOB)"
    ))
    .unwrap();
}

fn write_shard(path: &Path, table_sql: &str, fill: impl FnOnce(&Connection)) {
    let tmp = path.with_extension("tmp");
    {
        let c = Connection::open(&tmp).unwrap();
        c.execute_batch(table_sql).unwrap();
        fill(&c);
    }
    fs::write(path, gzip(&fs::read(&tmp).unwrap())).unwrap();
    fs::remove_file(tmp).unwrap();
}

/// Build the synthetic backup tree; returns its root.
fn build_backups(root: &Path) {
    let data = root.join(DATA);
    fs::create_dir_all(data.join("manifest")).unwrap();
    fs::create_dir_all(data.join("archive/outbox")).unwrap();
    fs::create_dir_all(root.join("shards")).unwrap();

    // Manifest: the real loadout's item subset plus a weapon and an activity.
    let mut items: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&core_fixture("manifest_items_subset.json")).unwrap();
    items.insert(
        "1001".into(),
        serde_json::json!({"displayProperties": {"name": "Test Rifle"}, "itemType": 3,
            "inventory": {"bucketTypeHash": 1_498_876_634_u32, "tierType": 5}}),
    );
    let m = data.join("manifest");
    fs::write(
        m.join("DestinyInventoryItemDefinition.json"),
        serde_json::to_vec(&items).unwrap(),
    )
    .unwrap();
    fs::write(
        m.join("DestinyEquipableItemSetDefinition.json"),
        core_fixture("manifest_item_sets_subset.json"),
    )
    .unwrap();
    fs::write(
        m.join("DestinySandboxPerkDefinition.json"),
        br#"{"7":{"displayProperties":{"name":"Perk"}}}"#,
    )
    .unwrap();
    fs::write(
        m.join("DestinyStatDefinition.json"),
        br#"{"1935470627":{"displayProperties":{"name":"Power"}}}"#,
    )
    .unwrap();
    fs::write(
        m.join("DestinyActivityDefinition.json"),
        br#"{"2727361621":{"displayProperties":{"name":"Equilibrium: Standard"},"directActivityModeType":82,"activityModeTypes":[82,7]}}"#,
    )
    .unwrap();
    fs::write(
        m.join("DestinyActivityModeDefinition.json"),
        br#"{"1":{"displayProperties":{"name":"Dungeon"},"modeType":82}}"#,
    )
    .unwrap();
    fs::write(m.join("item_version.txt"), "test-manifest\n").unwrap();

    // Main DB (production column names; only what the importer reads).
    let main = Connection::open(data.join("searchlight.sqlite")).unwrap();
    main.execute_batch(
        "CREATE TABLE run (instance_id INTEGER PRIMARY KEY, reference_id INTEGER, director_activity_hash INTEGER,
            target_name TEXT, activity_difficulty_tier INTEGER, mode INTEGER, period TEXT, duration_seconds INTEGER,
            player_count INTEGER, is_solo INTEGER, started_from_beginning INTEGER, selected_skull_hashes TEXT);
         CREATE TABLE run_entry (instance_id INTEGER, membership_id INTEGER, membership_type INTEGER,
            character_id INTEGER, display_name TEXT, global_name TEXT, global_name_code INTEGER,
            character_class TEXT, light_level INTEGER, kills INTEGER, deaths INTEGER, assists INTEGER,
            completion_reason INTEGER, precision_kills INTEGER, super_kills INTEGER, melee_kills INTEGER,
            grenade_kills INTEGER, ability_kills INTEGER, completed INTEGER, time_played_seconds INTEGER,
            weapons_used TEXT, PRIMARY KEY (instance_id, membership_id));
         CREATE TABLE build_segment (instance_id INTEGER, membership_id INTEGER, segment_index INTEGER,
            run_relative_start_s INTEGER, duration_s INTEGER, loadout_state TEXT,
            PRIMARY KEY (instance_id, membership_id, segment_index));
         CREATE TABLE player (membership_id INTEGER PRIMARY KEY, membership_type INTEGER, character_id INTEGER,
            display_name TEXT, global_name TEXT, global_name_code INTEGER, first_seen INTEGER);
         CREATE TABLE seeded_player (membership_id INTEGER PRIMARY KEY);
         CREATE TABLE random_player (membership_id INTEGER PRIMARY KEY);
         CREATE TABLE skull_catalog (skull_hash INTEGER PRIMARY KEY, name TEXT, connotation INTEGER, kind TEXT,
            slot TEXT, icon TEXT, description TEXT);
         CREATE TABLE activity (director_activity_hash INTEGER PRIMARY KEY, target_name TEXT);
         CREATE TABLE player_activity (membership_id INTEGER, character_id INTEGER, instance_id INTEGER,
            period TEXT, director_activity_hash INTEGER, mode INTEGER, kills INTEGER, deaths INTEGER,
            assists INTEGER, duration_seconds INTEGER, completion_reason INTEGER, completed INTEGER,
            player_count INTEGER, PRIMARY KEY (membership_id, instance_id));
         INSERT INTO skull_catalog VALUES (77, 'Feat A', 0, 'feat', NULL, NULL, NULL);
         INSERT INTO activity VALUES (2727361621, 'Equilibrium');
         INSERT INTO player VALUES (100, 3, 101, 'Tester', 'Tester', 42, 1780000000);
         INSERT INTO seeded_player VALUES (100);",
    )
    .unwrap();
    // Run 1 is tracked AND archived (archive wins); run 2 is tracked only.
    for (iid, kills) in [(1_i64, 50), (2, 70)] {
        main.execute(
            "INSERT INTO run VALUES (?1, 2727361621, 2727361621, 'Equilibrium', 11, 82, '2026-07-01T12:00:00Z',
                                    600, 1, 1, 1, '[77]')",
            params![iid],
        )
        .unwrap();
        main.execute(
            "INSERT INTO run_entry VALUES (?1, 100, 3, 101, 'Tester', 'Tester', 42, 'Warlock', 550, ?2, 1, 0, 0,
                                          0, 0, 0, 0, 0, 1, 590, '[{\"item_hash\":1001,\"kills\":5,\"precision_kills\":1}]')",
            params![iid, kills],
        )
        .unwrap();
    }
    // A two-segment build timeline on run 2 (the second segment drops a mod).
    let loadout =
        String::from_utf8(core_fixture("loadout_prismatic_exotic_class_item.json")).unwrap();
    let mut changed: serde_json::Value = serde_json::from_str(&loadout).unwrap();
    changed["items"][0]["plug_hashes"]
        .as_array_mut()
        .unwrap()
        .pop();
    for (idx, (start, dur, json)) in [(0, 300, loadout.clone()), (300, 300, changed.to_string())]
        .into_iter()
        .enumerate()
    {
        main.execute(
            "INSERT INTO build_segment VALUES (2, 100, ?1, ?2, ?3, ?4)",
            params![idx as i64, start, dur, json],
        )
        .unwrap();
    }
    main.execute(
        "INSERT INTO player_activity VALUES (100, 101, 1, '2026-07-01T12:00:00Z', 2727361621, 82, 50, 1, 0, 600, 0, 1, 1)",
        [],
    )
    .unwrap();
    drop(main);

    // Live pgcr_raw: empty but present.
    let live = Connection::open(data.join("pgcr_raw.sqlite")).unwrap();
    create_pgcr_table(&live, false);
    drop(live);

    // A pulled shard with runs 1 and 3, and the same shard re-exported into
    // the outbox under a different run id (the duplicate-export quirk).
    let shard_sql = "CREATE TABLE pgcr_raw (orig_rowid INT, instance_id INT, period TEXT, \
                     director_activity_hash INT, fetched_at INT, raw BLOB)";
    let fill =
        |c: &Connection| {
            for (iid, kills) in [(1_i64, 50_u32), (3, 90)] {
                c.execute(
                "INSERT INTO pgcr_raw VALUES (?1, ?1, '2026-07-01T12:00:00Z', 2727361621, 0, ?2)",
                params![iid, gzip(pgcr_json(iid, 2_727_361_621, 100, kills).as_bytes())],
            )
            .unwrap();
            }
            // One corrupt row must be counted, not fatal.
            c.execute(
                "INSERT INTO pgcr_raw VALUES (4, 4, '2026-07-01T12:00:00Z', 1, 0, X'1f8b0000')",
                [],
            )
            .unwrap();
        };
    write_shard(
        &root.join("shards/pgcr_raw_2026-07-01_1.sqlite.gz"),
        shard_sql,
        fill,
    );
    write_shard(
        &data.join("archive/outbox/pgcr_raw_2026-07-01_2.sqlite.gz"),
        shard_sql,
        fill,
    );

    // An activity-history shard duplicating the live row plus one new row.
    write_shard(
        &root.join("shards/player_activity_2026-07-01_1.sqlite.gz"),
        "CREATE TABLE player_activity (orig_rowid INT, membership_id INT, character_id INT, instance_id INT, \
         period TEXT, director_activity_hash INT, mode INT, kills INT, deaths INT, assists INT, \
         duration_seconds INT, completion_reason INT, completed INT, player_count INT)",
        |c| {
            for iid in [1, 5] {
                c.execute(
                    "INSERT INTO player_activity VALUES (?1, 100, 101, ?1, '2026-07-01T12:00:00Z', 2727361621, 82, \
                     1, 0, 0, 60, 0, 1, 1)",
                    params![iid],
                )
                .unwrap();
            }
        },
    );
}

fn count(db: &Database, sql: &str) -> i64 {
    db.conn().query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn imports_a_synthetic_backup_tree() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("backups");
    build_backups(&root);
    let out: PathBuf = dir.path().join("out/searchlight.duckdb");
    let opts = ImportOptions {
        backups_root: root,
        snapshot: None,
        out: out.clone(),
        work_dir: dir.path().join("work"),
        force: false,
        threads: Some(4),
        memory_limit: Some("1GB".into()),
        skip: vec![],
    };
    let report = searchlight_import::run(&opts, &Progress::hidden()).unwrap();

    // Archive: 2 shards × 3 rows; the outbox copy is all duplicates, the
    // corrupt row fails to parse in both copies.
    let a = report.archive.as_ref().unwrap();
    assert_eq!((a.rows_read, a.parse_errors), (6, 2));
    assert_eq!(a.runs, 2);

    let db = Database::open_read_only(&out).unwrap();
    // Runs 1 (both sources → archive), 2 (tracked only), 3 (archive only).
    assert_eq!(count(&db, "SELECT count(*) FROM run"), 3);
    assert_eq!(
        count(&db, "SELECT count(*) FROM run WHERE source = 'archive'"),
        2
    );
    assert_eq!(count(&db, "SELECT count(*) FROM run WHERE is_tracked"), 2);
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM run WHERE qualified_target = 'Equilibrium'"
        ),
        2
    );
    assert_eq!(
        count(&db, "SELECT feat_count FROM run WHERE instance_id = 2"),
        1
    );
    assert_eq!(count(&db, "SELECT count(*) FROM run_player"), 3);
    // Archive and tracked agree on run 1.
    assert_eq!(
        count(&db, "SELECT kills FROM run_player WHERE instance_id = 1"),
        50
    );
    assert_eq!(
        count(
            &db,
            "SELECT value FROM import_stat WHERE metric = 'tracked_archive_mismatch'"
        ),
        0
    );
    assert_eq!(count(&db, "SELECT count(*) FROM run_player_weapon"), 3);

    // Builds: two segments, two distinct builds, run 2 flagged.
    assert_eq!(count(&db, "SELECT count(*) FROM segment"), 2);
    assert_eq!(count(&db, "SELECT count(*) FROM build"), 2);
    assert_eq!(
        count(&db, "SELECT count(*) FROM run_build WHERE is_primary"),
        1
    );
    assert_eq!(
        count(&db, "SELECT count(*) FROM run_player WHERE has_build"),
        1
    );
    let exotic: String = db
        .conn()
        .query_row(
            "SELECT DISTINCT item_name(exotic_armor_hash) FROM build",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(exotic, "Solipsism");

    // History: live row + shard duplicate + new row → 2 rows.
    assert_eq!(count(&db, "SELECT count(*) FROM activity_history"), 2);

    // Player identity and flags.
    let name: String = db
        .conn()
        .query_row(
            "SELECT bungie_name FROM player WHERE membership_id = 100",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(name, "Tester#0042");
    assert_eq!(count(&db, "SELECT count(*) FROM player WHERE is_seeded"), 1);
    assert_eq!(
        db.meta("manifest_version").unwrap().as_deref(),
        Some("test-manifest")
    );

    // No staging tables leak into the published database.
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM information_schema.tables WHERE table_name LIKE 'stg_%'"
        ),
        0
    );

    // Re-running without --force refuses to clobber the output.
    assert!(searchlight_import::run(&opts, &Progress::hidden()).is_err());
}
