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

# Which builds make their players better in Ultimate Conquest? Who is best?
./target/release/searchlight rank builds --activity "ultimate conquest" --min-players 10
./target/release/searchlight rank players --activity "ultimate conquest"

# Which exotics do Hunters wear in Master raid clears?
./target/release/searchlight search --class Hunter --activity "master raid" --clears --group-by exotic-armor

# Prismatic Warlock builds with two given aspects, then builds like one of them
./target/release/searchlight search --class Warlock --aspect "Feed the Void" --aspect "Hellion"
./target/release/searchlight similar <BUILD_ID> --other-players

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
  counts with a tiered distance (exotics › kit › armor effects › weapons) and
  budget-aware armor stat investment, for similarity, popularity and
  clustering ([ADR 0004](docs/adr/0004-element-count-vectors.md),
  [ADR 0005](docs/adr/0005-tiered-distance-and-stat-investment.md)).
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
| `searchlight rank builds [FACETS] [RUN FILTERS]` | builds ranked by how their players perform with them relative to those players' own usual level, so strong players don't make a build look good (`--group-by core\|build\|exotic-armor\|…`, `--min-players N`, `--sort effect\|raw\|players\|runs`); see [ADR 0006](docs/adr/0006-ranking.md) |
| `searchlight rank players [FACETS] [RUN FILTERS]` | players ranked by skill net of the builds they used (`--min-runs N`, `--sort skill\|runs\|best`); facets list players who used matching builds |
| `searchlight search [FACETS] [RUN FILTERS]` | builds matching any combination of class, element, exotic armor/weapon, super, aspects, fragments, weapons, perks, mods, artifact perks, armor sets; run filters `--activity`, `--player`, `--solo`, `--clears`, `--since/--until`; `--group-by exotic-armor` (etc.) for popularity reports |
| `searchlight similar BUILD_ID` | a build's nearest neighbours by tiered distance, with per-tier breakdown (`--other-players`, `--any-class`, `--include-passengers`, `--limit N`) |
| `searchlight sql "QUERY"` | ad-hoc read-only SQL (`--format tsv`, `--max-rows N`) |

Trend reports, clustering and charts are the next milestones; see [docs/architecture.md](docs/architecture.md#adding-a-cli-feature)
for where new features go.

## Documentation

- [docs/data-sources.md](docs/data-sources.md) — the backups, how production
  produced them, and the quirks handled on import
- [docs/schema.md](docs/schema.md) — tables, columns, keys, example queries
- [docs/importing.md](docs/importing.md) — running and tuning the import
- [docs/architecture.md](docs/architecture.md) — crates, data flow, testing
- [docs/adr/](docs/adr) — decision records (DuckDB; build identity; element count vectors; tiered distance and stat investment)

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
