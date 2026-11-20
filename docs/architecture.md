# Architecture

Focalpoint turns the frozen Searchlight backups into a DuckDB
analysis database and (going forward) answers questions about builds,
players and trends from it.

## Crates

| Crate | Kind | Responsibility |
|---|---|---|
| [`focalpoint-core`](../crates/core) | library, no I/O | Source formats (`LoadoutState`, PGCR JSON), manifest model (`Catalog`), plug classification, build extraction and fingerprints, stat investment (`stats`), element vectors and distances (`vector`), run scoring and the player/build model (`rank`), diverse well-ranked picks (`highlight`). Everything here is deterministic and unit-tested, including against real captured fixtures. |
| [`focalpoint-db`](../crates/db) | library | The DuckDB schema (`schema.sql`), schema versioning, create / read-only open, metadata helpers, default database location, and the query APIs behind the CLI: shared facets and run filters (`filter`), `search`, `similar`, `rank`, the site export: exact recipes (`recipe`), the ranked book (`site`), highlights (`highlight`) and the catalog of every build (`catalog`), and the analyses behind the design (`analysis`). |
| [`focalpoint-import`](../crates/import) | library | The ETL: source discovery, local staging, parallel extract into staging tables, SQL transform, stat investment, scoring, validation, publish. |
| [`focalpoint-mechanics`](../crates/mechanics) | library | The community-mechanics layer ([mechanics.md](mechanics.md)): Google Sheets tabs as grids (`sheet`), the Clarity and Compendium readers, manifest name matching (`names`), and writing the `mechanic_*` tables into an existing database. |
| [`focalpoint-cli`](../crates/cli) | binary `focalpoint` | Command-line interface: `analyze`, `import`, `info`, `mechanics`, `search`, `similar`, `rank`, `site`, `sql`. |

The static site in [`site/`](../site) reads only the JSON that `focalpoint site` exports ([site.md](site.md)).

Dependency direction: `cli → import → (db, core)`, `cli → db`. `core` depends
on nothing project-internal, so it can be reused (e.g. by a future web UI or
a WASM build).

## Import data flow

```text
backups (CIFS, read-only)
  │  stage.rs: copy *.sqlite + -wal, gunzip shards (rayon, cached by marker)
  ▼
work dir (local disk)
  │  rayon workers read SQLite in rowid ranges (rusqlite)
  │  ──► bounded channels ──► N writer threads per table (DuckDB Appender)
  ▼
staging DB (stg.*, separate file)        builds.rs writes final build tables directly
  │  transform.sql (merge, dedupe, derive, order)
  ▼
final tables ──► validate.rs ──► drop staging ──► atomic publish
```

Key properties:

- **Backups are never opened in place.** SQLite on CIFS is unsafe and opening
  a WAL database writes to it.
- **Backpressure.** Producers block on bounded channels when writers fall
  behind, keeping memory flat. Row-by-row appending is the costliest step,
  so heavy tables get one writer per 8 cores (a CPU trace showed 256
  producers idling behind 8 writers before this was tuned).
- **Deterministic.** Where sources disagree (re-fetched reports), a fixed
  rule picks the winner before any parallel work, so re-imports of the same
  backups produce the same database.
- **Declarative merge rules.** All dedupe/precedence logic is SQL in one file,
  readable without the Rust around it.
- **Fail loudly on invariants, record data gaps.** Validation errors abort;
  known source gaps are counted in `import_stat`.

## Adding a CLI feature

1. Put pure logic (scoring, similarity, feature vectors) in `focalpoint-core`
   with unit tests.
2. Put SQL-facing query functions in `focalpoint-db` (they take a
   `&Database` and return typed rows), tested against a small in-memory
   database.
3. Add a subcommand module under `crates/cli/src/commands/` that parses
   arguments, calls the query layer, and renders with `output.rs`.
4. An analysis that informs a design decision goes in
   `focalpoint_db::analysis` (returning plain data) with a subcommand of
   `focalpoint analyze`, and its findings in `docs/analysis.md`, so it can be
   rerun after a re-import rather than living in a one-off script.

## Testing

- `cargo test --workspace` runs everything (unit tests, real-data fixture
  tests, DuckDB schema/macro tests, ETL component tests).
- `cargo clippy --workspace --all-targets` must be clean (pedantic lints are
  enabled workspace-wide; deliberate exceptions are listed in `Cargo.toml`).
- End-to-end: `focalpoint import` on the real backups, then
  `focalpoint info --detailed`; validation results are part of the report.
