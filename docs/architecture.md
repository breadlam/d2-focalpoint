# Architecture

`searchlight-cli` turns the frozen Searchlight backups into a DuckDB
analysis database and (going forward) answers questions about builds,
players and trends from it.

## Crates

| Crate | Kind | Responsibility |
|---|---|---|
| [`searchlight-core`](../crates/core) | library, no I/O | Source formats (`LoadoutState`, PGCR JSON), manifest model (`Catalog`), plug classification, build extraction and fingerprints. Everything here is deterministic and unit-tested, including against real captured fixtures. |
| [`searchlight-db`](../crates/db) | library | The DuckDB schema (`schema.sql`), schema versioning, create / read-only open, metadata helpers, default database location. Future query APIs for the CLI belong here. |
| [`searchlight-import`](../crates/import) | library | The ETL: source discovery, local staging, parallel extract into staging tables, SQL transform, validation, publish. |
| [`searchlight-cli`](../crates/cli) | binary `searchlight` | Command-line interface: `import`, `info`, `sql` today. |

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
- **Declarative merge rules.** All dedupe/precedence logic is SQL in one file,
  readable without the Rust around it.
- **Fail loudly on invariants, record data gaps.** Validation errors abort;
  known source gaps are counted in `import_stat`.

## Adding a CLI feature

1. Put pure logic (scoring, similarity, feature vectors) in `searchlight-core`
   with unit tests.
2. Put SQL-facing query functions in `searchlight-db` (they take a
   `&Database` and return typed rows), tested against a small in-memory
   database.
3. Add a subcommand module under `crates/cli/src/commands/` that parses
   arguments, calls the query layer, and renders with `output.rs`.

## Testing

- `cargo test --workspace` runs everything (unit tests, real-data fixture
  tests, DuckDB schema/macro tests, ETL component tests).
- `cargo clippy --workspace --all-targets` must be clean (pedantic lints are
  enabled workspace-wide; deliberate exceptions are listed in `Cargo.toml`).
- End-to-end: `searchlight import` on the real backups, then
  `searchlight info --detailed`; validation results are part of the report.
