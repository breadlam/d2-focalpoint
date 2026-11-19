# searchlight-cli

Query and analyse the Destiny 2 builds collected by **Searchlight**
(formerly builds.rodeo), a service that discovered top-tier players in
high-difficulty content and recorded what they had equipped, second by
second, while they played.

The service is offline; its backups remain. This tool rebuilds them into a
single, analysis-ready [DuckDB](https://duckdb.org) database and queries it.

## Quick start

```sh
# Build (first build compiles the bundled DuckDB engine: a few minutes)
cargo build --release

# Build the analysis database from the backups (local scratch space required)
./target/release/searchlight import --backups /mnt/saspool/projects/searchlight-backups

# What is in it?
./target/release/searchlight info

# Ask anything in SQL
./target/release/searchlight sql "
  SELECT class, item_name(exotic_armor_hash) AS exotic, count(*) AS builds
  FROM build GROUP BY ALL ORDER BY builds DESC LIMIT 10"
```

The database lives at `$SEARCHLIGHT_DB`, else
`~/.local/share/searchlight/searchlight.duckdb`; override per command with
`--db PATH`.

## What the database holds

- **Builds** — ~1.2M build-timeline segments across ~570k player-runs, each
  resolved into a deduplicated, gameplay-relevant *build* (subclass, aspects,
  fragments, abilities, weapons with perks, armor with mods and set bonuses,
  artifact perks) with a stable `build_id`, plus the exact lossless loadout.
- **Element count vectors** — each build as a sparse vector of element
  counts (abilities, aspects, fragments, weapons and perks, armor and mods,
  artifact perks), with exact weighted-Jaccard, L1 and slot-edit distances
  for similarity, popularity and clustering ([ADR 0004](docs/adr/0004-element-count-vectors.md)).
- **Runs** — ~17.7M raw post-game carnage reports (target activities and all
  raids/dungeons) merged with production's parsed runs: per-character kills,
  deaths, kill sources, clears, and per-weapon kills.
- **Players** — identities, characters, and the all-modes activity history of
  every discovered player.
- **Manifest dimensions** — item, perk, set, stat, activity and modifier
  names, so every hash is human-readable.

See [docs/schema.md](docs/schema.md) for the model and example queries.

## Commands

| Command | Purpose |
|---|---|
| `searchlight import --backups DIR` | rebuild the database ([docs/importing.md](docs/importing.md)) |
| `searchlight info [--detailed]` | provenance, headline counts, coverage, import statistics |
| `searchlight sql "QUERY"` | ad-hoc read-only SQL (`--format tsv`, `--max-rows N`) |

Build search, reports, rankings, clustering and charts are the next
milestones; see [docs/architecture.md](docs/architecture.md#adding-a-cli-feature)
for where new features go.

## Documentation

- [docs/data-sources.md](docs/data-sources.md) — the backups, how production
  produced them, and the quirks handled on import
- [docs/schema.md](docs/schema.md) — tables, columns, keys, example queries
- [docs/importing.md](docs/importing.md) — running and tuning the import
- [docs/architecture.md](docs/architecture.md) — crates, data flow, testing
- [docs/adr/](docs/adr) — decision records (DuckDB; build identity; element count vectors)

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets   # pedantic, must be clean
cargo fmt --all
```

Rust toolchain is pinned in `rust-toolchain.toml`. If the checkout lives on a
network share, point cargo's target directory at local disk with a
git-ignored `.cargo/config.toml`:

```toml
[build]
target-dir = "/home/<you>/.cache/searchlight-cli/target"
```

## License

[MIT](LICENSE)
