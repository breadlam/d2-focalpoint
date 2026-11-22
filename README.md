# Focalpoint

Find the Destiny 2 builds that make players better. Focalpoint analyses the
builds collected by **Searchlight** (formerly builds.rodeo), a service that
discovered top-tier players in high-difficulty content and recorded what they
had equipped, second by second, while they played.

The service is offline; its backups remain. Focalpoint rebuilds them into a
single, analysis-ready [DuckDB](https://duckdb.org) database, then:

- searches builds by any combination of facets and reports their popularity;
- ranks builds and players, separating the value of a build from the skill
  of the players who use it;
- finds similar builds by a tiered build distance;
- publishes a static [build-ranking site](docs/site.md) for GitHub Pages.

The command-line tool is `focalpoint`.

## Quick start

```sh
# Build (first build compiles the bundled DuckDB engine: a few minutes)
cargo build --release

# Build the analysis database from the backups (local scratch space required)
./target/release/focalpoint import --backups ../searchlight-backups

# What is in it?
./target/release/focalpoint info

# Which builds make their players better in Ultimate Conquest? Who is best?
./target/release/focalpoint rank builds --activity "ultimate conquest" --min-players 10
./target/release/focalpoint rank players --activity "ultimate conquest"

# Which exotics do Hunters wear in Master raid clears?
./target/release/focalpoint search --class Hunter --activity "master raid" --clears --group-by exotic-armor

# Prismatic Warlock builds with two given aspects, then builds like one of them
./target/release/focalpoint search --class Warlock --aspect "Feed the Void" --aspect "Hellion"
./target/release/focalpoint similar <BUILD_ID> --other-players

# Ask anything in SQL
./target/release/focalpoint sql "
  SELECT class, item_name(exotic_armor_hash) AS exotic, count(*) AS builds
  FROM build GROUP BY ALL ORDER BY builds DESC LIMIT 10"
```

The database lives at `$FOCALPOINT_DB`, else
`~/.local/share/focalpoint/focalpoint.duckdb`; override per command with
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
| `focalpoint analyze priors\|breadth\|diversity\|highlights\|sets\|attempts\|triggers\|escalation\|…` | rerun the analyses behind the design: the model's priors by cross-validation, breadth and the neutral-game ranking, distinctness under thresholds, the highlight picks and their quality weight, what armor set bonuses are worth, failed attempts in Conquests and Ops, what triggers the parts of builds played in each activity, parts whose worth depends on the content ([docs/analysis.md](docs/analysis.md)) |
| `focalpoint import --backups DIR` | rebuild the database ([docs/importing.md](docs/importing.md)) |
| `focalpoint info [--detailed]` | provenance, headline counts, coverage, import statistics |
| `focalpoint mechanics import [--sources DIR]` | read the community's documentation of game mechanics (Clarity, the Destiny Data Compendium) and measured boss-damage rotations with their loadouts into the database, keyed to manifest hashes; enemy health and Power delta scaling; the energy economy (ability cooldowns, the Armor 3.0 stat curves, regen effects and flat gains); and build the profiles of the contexts builds are evaluated in ([docs/mechanics.md](docs/mechanics.md), [docs/contexts.md](docs/contexts.md), [docs/durability.md](docs/durability.md)) |
| `focalpoint sandbox check [--dir DIR]` | check the build model's rule base against its vocabulary, resolve the parts it names against the manifest, and report its coverage of the parts recorded builds use, with its gaps and the most-used parts not yet modelled ([docs/sandbox.md](docs/sandbox.md)) |
| `focalpoint mechanics context [QUERY]` | show context profiles: activity groups, activities and boss encounters with their features and timed events |
| `focalpoint rank builds [FACETS] [RUN FILTERS]` | builds ranked by how their players perform with them relative to those players' own usual level, so strong players don't make a build look good (`--group-by core\|build\|exotic-armor\|…`, `--min-players N`, `--sort effect\|raw\|players\|runs`); see [ADR 0006](docs/adr/0006-ranking.md) |
| `focalpoint rank players [FACETS] [RUN FILTERS]` | players ranked by skill net of the builds they used (`--min-runs N`, `--sort skill\|runs\|best`); facets list players who used matching builds |
| `focalpoint site [--out site/data]` | export the rankings for the static site ([docs/site.md](docs/site.md)) |
| `focalpoint search [FACETS] [RUN FILTERS]` | builds matching any combination of class, element, exotic armor/weapon, super, aspects, fragments, weapons, perks, mods, artifact perks, armor sets; run filters `--activity`, `--player`, `--solo`, `--clears`, `--since/--until`; `--group-by exotic-armor` (etc.) for popularity reports |
| `focalpoint similar BUILD_ID` | a build's nearest neighbours by tiered distance, with per-tier breakdown (`--other-players`, `--any-class`, `--include-passengers`, `--limit N`) |
| `focalpoint sql "QUERY"` | ad-hoc read-only SQL (`--format tsv`, `--max-rows N`) |

## Build-ranking site

[`site/`](site), branded **Skinwalker** as Searchlight's dashboard was, is a
static page for GitHub Pages with three views: **Ranked Builds** (build types ranked by build effect, each with its best-ranked
exact build and near-identical variants, per activity), **Highlights** (a
few builds per class as well ranked and as unlike each other as the data
allows, for the neutral game and each activity), and **Raw Search**, over
every collected build exactly as equipped.
Refresh its data with `focalpoint site`; see [docs/site.md](docs/site.md)
for the formats, previewing, tests and publishing.

Trend reports, clustering and charts are the next milestones; see [docs/architecture.md](docs/architecture.md#adding-a-cli-feature)
for where new features go.

## Documentation

- [docs/data-sources.md](docs/data-sources.md) — the backups, how production
  produced them, and the quirks handled on import
- [docs/schema.md](docs/schema.md) — tables, columns, keys, example queries
- [docs/importing.md](docs/importing.md) — running and tuning the import
- [docs/architecture.md](docs/architecture.md) — crates, data flow, testing
- [docs/site.md](docs/site.md) — the static build-ranking site: data export, preview, tests, publishing
- [docs/analysis.md](docs/analysis.md) — the analyses behind the design (`focalpoint analyze`) and their findings
- [docs/mechanics.md](docs/mechanics.md) — the community's documentation of game mechanics: sources, the `mechanic_*` tables, coverage
- [docs/contexts.md](docs/contexts.md) — the contexts builds are evaluated in (activity groups, activities, encounters) and their open-ended profiles
- [docs/effects.md](docs/effects.md) — what build parts need and do: triggers, payoffs and game states read from the mechanics texts
- [docs/sandbox.md](docs/sandbox.md) — the build model's vocabulary and rule base: format, checking, coverage
- [docs/build-model.md](docs/build-model.md) — one model of build mechanics (ontology, core verbs, rule base, scenarios, fluid analysis and simulation, calibration) for both the synergy evaluator and build synthesis
- [docs/durability.md](docs/durability.md) — how much damage enemies take to kill, per enemy and context: health tables, the Power delta curve, each activity group's delta, and the difficulty steps measured on the same players
- [docs/escalation.md](docs/escalation.md) — synergistic escalation: loops that feed themselves past a threshold (Solar ignition chains, the Stasis shard loop, the sword engine, …), their mechanics and the evidence
- [docs/adr/](docs/adr) — decision records (DuckDB; build identity; element count vectors; tiered distance and stat investment; ranking; highlights and the neutral game; the solo premium; partial participants; polished top builds)

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets   # pedantic, must be clean
cargo fmt --all
```

Rust toolchain is pinned in `rust-toolchain.toml`. Builds are large (the
bundled DuckDB is compiled per profile, and debug output grows past 100 GB
over time); put cargo's target directory where there is room, for example
`.tmp/` beside the checkout (git-ignored), with a git-ignored
`.cargo/config.toml`:

```toml
[build]
target-dir = "/path/to/checkout/.tmp/target"
```

## License

[MIT](LICENSE)
