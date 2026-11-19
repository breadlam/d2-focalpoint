# Importing the backups

`searchlight import` rebuilds the analysis database from the Searchlight
backups. It never modifies the backups.

```sh
searchlight import --backups /mnt/saspool/projects/searchlight-backups
```

| Option | Default | Notes |
|---|---|---|
| `--backups DIR` | — | root containing `builds.rodeo/` and `shards/` |
| `--db PATH` (global) | `$SEARCHLIGHT_DB`, else `~/.local/share/searchlight/searchlight.duckdb` | output database |
| `--work-dir DIR` | `~/.cache/searchlight/import` | **must be local disk**; needs ≈ 150 GB (staged sources ≈ 80 GB + the database under construction and DuckDB spill) |
| `--snapshot NAME` | latest `snapshot-*` | which server snapshot to use |
| `--force` | off | replace an existing database |
| `--threads N` | all cores | rayon and DuckDB worker threads |
| `--memory-limit X` | 80 % of RAM | DuckDB memory limit, e.g. `64GB`; DuckDB spills to the work dir beyond it |
| `--skip archive,history` | none | skip the heavy stages for quick development imports |

## What happens

1. **Stage.** The live SQLite databases are copied *with* their `-wal` files,
   and every shard is decompressed, into `<work-dir>/live` and
   `<work-dir>/shards`. A `.source` marker beside each copy records the
   source's size and mtime; unchanged sources are not re-staged on later
   runs, so the work dir doubles as a cache. Delete it to reclaim space.
2. **Extract** into a separate staging database
   (`<work-dir>/staging.duckdb`, attached as `stg`) so the published file
   never contains staging data or the free space it would leave behind.
   Producers run on all cores; each high-volume table gets several DuckDB
   appender threads (one per 8 cores):
   - *manifest* → `item`, `item_set*`, `sandbox_perk`, `stat`, `activity_mode`;
   - *tracked* → production's `run` / `run_entry` / `player` / slate / skulls;
   - *archive* → all raw PGCRs; a pre-pass reads only `(instance_id,
     fetched_at)` and picks the latest-fetched copy of each report, so only
     that copy is decompressed and parsed;
   - *history* → all `player_activity` rows (live + shards);
   - *builds* → each `build_segment` is parsed, projected to its build and
     fingerprinted; builds and exact loadouts are deduplicated, and a second
     fingerprint guards against id collisions.
3. **Transform.** [`transform.sql`](../crates/import/src/sql/transform.sql)
   merges sources (a raw PGCR wins over production's parsed copy of the same
   run), dedupes, derives difficulty gates and per-player aggregates, and
   writes the final tables in a scan-friendly order.
4. **Investment.** A Rust stage reads each build's modal stats and fragment
   share, applies the budget-aware stat investment model (ADR 0005), and
   [`finalize.sql`](../crates/import/src/sql/finalize.sql) assembles the
   final `build` table in a single insert.
5. **Scoring.** [`scoring.rs`](../crates/import/src/scoring.rs) measures
   every qualified clear against a typical clear of its activity and solo
   status and writes `activity_reference` and `run_score` (ADR 0006); about
   7 s.
6. **Validate.** Natural-key uniqueness and referential checks must pass
   (otherwise the import fails and the partial database is kept in the work
   dir for inspection). Warnings and cross-source parity counts are recorded
   in `import_stat`.
7. **Publish.** The staging database is detached and deleted, metadata written, and the file is
   moved into place (atomic rename, or copy + rename across filesystems).

The import prints a report; `searchlight info --detailed` shows the same
statistics later.

## Performance notes

Measured on the original 256-core / 1 TB machine with the work dir on tmpfs:

| | |
|---|---|
| full import, sources already staged | ≈ 2.5 min (141–158 s) |
| first run | add the time to pull ≈ 80 GB from the share |
| published database | ≈ 11 GB |
| peak work-dir usage | ≈ 150 GB (staged sources + staging DB + output) |

Extraction is bounded by DuckDB's row-by-row appender, not by reading or
parsing: heavy tables get one writer thread per 8 cores
(`sink::staging_writers`). The transform step is DuckDB SQL and uses all
cores. On smaller machines set `--memory-limit` and `--threads` and expect
proportionally longer runs.

The import is deterministic: two independent full imports of the same
backups produce identical tables (verified by comparing an order-independent
hash of every table).

## Re-importing after code changes

The import is deterministic for the same sources and code. Changing plug
classification or build identity rules changes `build_id`s: bump the
fingerprint version in `searchlight-core` (a unit test will remind you) and
re-import with `--force`.
