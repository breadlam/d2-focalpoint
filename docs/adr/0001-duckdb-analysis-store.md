# ADR 0001: DuckDB as the analysis store

- **Status:** accepted
- **Date:** 2026-11-18

## Context

Searchlight (builds.rodeo) ran on SQLite in WAL mode, which suited its
workload: one writer ingesting live captures, many small point reads from a
web API. The service is gone; what remains is a static set of backups (a
34 GB SQLite snapshot, ~2,300 gzipped day-shards of raw reports, and older
dumps).

Focalpoint has a different workload. It is read-only and analytical:
filtering builds by arbitrary combinations of facets, aggregating usage and
performance over time, ranking players, computing similarity between builds,
and feeding charts. Typical queries scan or aggregate millions of rows
(≈18M runs, ≈60M player-run rows, ≈200M activity-history rows).

## Decision

Build a single **DuckDB** database file from the backups, with a fresh,
analysis-oriented star schema (`docs/schema.md`). Reading the SQLite sources
uses `rusqlite`; writing and querying uses the `duckdb` crate with the
bundled engine (no system library or runtime extension downloads).

## Rationale

- **Columnar, vectorised execution.** Group-bys and scans over tens of
  millions of rows run in sub-second to seconds, which is what reports,
  trends and clustering need. SQLite would need many hand-tuned indexes and
  still be orders of magnitude slower on aggregates.
- **Native `LIST` types** make facet filters direct:
  `list_has_all(aspect_hashes, [...])`, `list_contains(item_hashes, h)`.
- **Compression.** Dictionary/RLE compression keeps hash-heavy tables small.
- **Single file, embedded, read-only friendly.** The CLI opens the database
  read-only, so it can be shared or copied like the old SQLite file.
- **Rich SQL** (window functions, `QUALIFY`, `DISTINCT ON`, `arg_max`,
  `COLUMNS(*)`) keeps transform and report logic declarative.

## Consequences

- The import is a from-scratch rebuild, not incremental. This is fine: the
  sources are frozen backups, and a rebuild takes minutes on a large machine.
- First compile of the bundled engine takes a few minutes.
- Large fact tables carry no `PRIMARY KEY` (DuckDB's ART indexes are costly
  at this scale); natural keys are documented and the importer's validation
  step enforces their uniqueness.
- DuckDB allows one writer process; the CLI only ever reads after import.
- The production design doc's guidance ("DuckDB … fine later as an
  attach-and-query analytical engine") anticipated exactly this role.
