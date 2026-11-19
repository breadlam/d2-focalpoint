# Data sources: the Searchlight backups

Searchlight (builds.rodeo) discovered high-difficulty Destiny 2 completions by
enumerating post-game carnage reports (PGCRs), then live-sampled the loadouts
of standout players every ~10 s during their next runs to record *build
timelines*. The service ran from 2026-06-09 until its disk filled around
2026-09-17; the server was snapshotted on 2026-09-20 and taken down.

This document records what the backups contain, how they were produced, and
the quirks the importer has to handle. Paths are relative to the backups root
(the directory passed to `focalpoint import --backups`).

## Layout

```text
builds.rodeo/snapshot-2026-09-20/          full server snapshot
  opt/searchlight/searchlight/             production source tree (108 commits)
    data/searchlight.sqlite (+ -wal)       main DB, 34 GB
    data/pgcr_raw.sqlite (+ -wal)          raw PGCRs, last ~7 days only
    data/archive/outbox/*.sqlite.gz        655 day-shards never pulled off the server
    data/manifest/Destiny*.json            cached Bungie manifest tables
    data/{perf,web}.sqlite                 ops metrics, web analytics (not imported)
  home/, etc/                              host config, logs (not imported)
shards/*.sqlite.gz                         1,645 day-shards pulled nightly until 2026-08-17
snapshots/store_*.sql.gz                   logical dumps of the main DB (Jul 8 – Aug 10)
snapshots/web_*.sql.gz, *.json             web analytics dumps, model/report artifacts
```

> `../searchlight/` (outside the backups) is an **older** June checkout of the
> source. The authoritative source is the copy inside the server snapshot.

## Main database (`searchlight.sqlite`)

| Table | Rows | What it is | Imported as |
|---|---:|---|---|
| `run` | 4.96M | PGCR header for runs of curated target activities (and live-captured runs of monitored players, `target_name = '?'`) | `run` (fallback source) |
| `run_entry` | 12.9M | one row per player per run, weapons as JSON | `run_player`, `run_player_weapon` (fallback) |
| `build_segment` | 1.21M | build timeline segments; `loadout_state` JSON (~5 KB each) | `segment`, `build*`, `loadout*` |
| `player_activity` | 128.6M | all-modes activity history of discovered players (last ~45 days live) | `activity_history` |
| `player_history` | 6.07M | per-player target runs with a cached score | not imported (derivable from runs; 4,454 rows lack a PGCR but appear in activity history) |
| `player` | 1.31M | identities, first-seen time | identity fallback for `player` |
| `component` | 18k | production's manifest-derived plug classification | superseded by the manifest stage |
| `skull_catalog` | 1.2k | difficulty modifiers classified (boon/feat/challenge/…) | `skull` |
| `activity` | 42 | curated target slate per activity hash | `activity.target_group` |
| `seeded_player`, `random_player` | 6, 12k | operator watch list; unbiased baseline cohort | `player.is_seeded`, `player.is_random_sample` |
| `candidate` | 11k | monitor-pool bookkeeping | not imported |

Production's per-run scoring (`score = kills/time ÷ (1+deaths) × 2 if solo`) is
not stored; the CLI computes its own metrics from raw stats.

## Raw PGCR archive

Every fetched PGCR for a target activity or any raid/dungeon was stored
(gzip JSON, bare report without the API envelope) in `pgcr_raw.sqlite`. A
nightly job (`scripts/archive_export.sh`) moved rows older than 7 days into
per-day shard files and deleted them from the live table; the backup host
pulled the shards. Shards are plain SQLite (`CREATE TABLE AS SELECT`, an extra
`orig_rowid` column, no keys).

Totals: 19.05M stored rows in 2,192 shards → **17.14M distinct**, plus 590k
live rows (disjoint) → **17.73M distinct reports**, 2025-03-19 → 2026-09-17
(pre-June dates come from history mining).

## Quirks the importer handles

1. **Duplicated shard rows.** From late July the server disk sat at 95–100 %.
   When the export's delete step failed (`database or disk is full`,
   `database is locked`), the rows stayed live and were exported *again* the
   next night under a new run id; some reports were also fetched again weeks
   later. 1.9M of 19.6M stored rows (≈10 %) are extra copies of 1.64M
   instances. Copies are *not* always identical: in a 20k sample, 90 % were
   byte-identical and 10 % differed only in player identity (Bungie
   re-renders names, emblems and light level at fetch time); none differed
   in stats for the same roster. Across all 1.64M duplicated reports, 77 have
   rosters of different sizes, and in every one the most recently fetched
   copy has the largest roster. The importer therefore keeps the **most
   recently fetched** copy of each instance (ties broken by source order),
   chosen in a pre-pass so the result is deterministic.
   Export failures were otherwise fail-safe: rows were only deleted after a
   verified shard existed, so live + shards + outbox hold every row.
2. **Unpulled outbox.** The backup host stopped pulling after 2026-08-17; 655
   shards (Aug 18 → Sep 20) exist only in the server snapshot's outbox.
3. **WAL files.** The snapshot was taken live; committed data still sat in
   `-wal` files (254 MB for `pgcr_raw`). Sources are copied *with* their WAL
   to a local work directory and opened there. The backups are never opened
   in place (they live on a CIFS share where SQLite locking is unreliable, and
   opening a WAL database writes to it).
4. **Superseded timelines.** Production re-finalised some timelines, replacing
   them with fewer segments. 66 `build_segment` rows present in older dumps
   are absent from the live DB for this reason; the live version is
   authoritative. The live DB is otherwise a strict superset of every dump, so
   the dumps are not imported.
5. **Mixed encodings.** Older `pgcr_raw.raw` values are plain JSON text, newer
   ones gzip blobs; both are detected by magic bytes.
6. **Leftover temp shard.** `data/archive/tmp/` holds one shard from an
   interrupted export. Its rows were never deleted from the live table, so it
   is ignored.
7. **Cross-source parity.** For runs present both as a raw report and in
   production's parsed tables, ~11.9M player rows overlap and ~300 (0.003 %)
   disagree on some stat — production recorded those rows from a different
   fetch than the archived copy. The raw report wins.
8. **Checkpoint bots and passengers.** Some captured players never kill and
   never clear: 8,361 players and 5 % of build timelines, led by checkpoint
   bot accounts (e.g. one with 2,972 runs) that hold Master raid/dungeon
   checkpoints for others to join. Their rows are kept as reported; filter
   on `run_player.kills > 0` (or `is_clear`) when ranking builds or players.
9. **Partial last days.** Runs end 2026-09-17 although the snapshot is from
   09-20: the gatherer stalled when the disk filled.

## Manifest

`data/manifest/` holds the full `DestinyInventoryItemDefinition` (38,894
items, version `…65864`, fetched 2026-09-12) plus activity, mode, stat,
sandbox-perk, armor-set, skull and plug-set tables. Every item, plug, perk,
stat and artifact hash in the build data resolves against it (verified at
100 %), so no Bungie API access is needed.
