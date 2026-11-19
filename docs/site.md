# The build-ranking site

`site/` is a static page for GitHub Pages: **a book of exact recipes**. Every
listing is a build a player actually used in a qualified clear, published
exactly as equipped — subclass, abilities, aspects, fragments, weapons with
their frame, perks, origin trait and mod, armor pieces with archetype,
tuning, stat mods and mods, set bonuses, artifact perks and final stats.
Nothing is averaged or combined; the ranking tools only choose which builds
to publish:

1. **Build types** (class, subclass element, exotic armor and weapon, super,
   aspects) are ranked by **build effect** (ADR 0006): how much better
   players do with the type than on their own other runs.
2. Within a type, **recipe families** (the same kit: exotic perks,
   abilities, aspects, fragments) and within the best family, **loadouts**
   (the same kit and legendary weapons) are ranked the same way, pooling the
   evidence of near-identical builds.
3. The listing's **top recipe** is the best-ranked exact build of the best
   loadout that was used in at least 3 clears and beat its players' usual
   level (falling back to 2 clears, then to the best loadout).
4. Its **near-identical variants** are the loadout's other exact builds
   within 8 element changes (a perk roll, a mod, an artifact perk), each
   listed with exactly what differs and its own full recipe.

Recipes are drawn the way Searchlight drew a build (after the in-game
loadout screen): weapons and subclass, stats and set bonuses, armor and
artifact, as icon tiles in aligned columns whose names show on hover, focus
or tap; narrow screens stack the sections in the same reading order. The page
uses Searchlight's dark palette.

Pick "All activities", a target group (e.g. "Master Raid") or a single
activity to get the listings ranked there, and narrow them by class,
subclass, super, exotics, aspects or a component a listed recipe includes.
Filters and activity are kept in the URL, so a view can be shared.

A second view, **Find any build**, searches the **catalog**: every build
Searchlight collected (594,233), exactly as equipped, whether or not it was
used in a qualified clear. Its filters work on build types first (class,
subclass, super, exotics, aspects, activity); once they select at most
45,000 builds, the page downloads just the shards holding them and matches
everything the builds include (fragments, weapons, perks, mods, set bonuses,
artifact perks). Ranked builds come first, the rest by use.

## Files

| Path | What it is |
|---|---|
| `site/index.html`, `site/style.css`, `site/favicon.svg` | the page; no build step, no dependencies |
| `site/app.js` | an ES module: pure functions (recipe decoding and differences, filtering, facet counts, ranking, URL state) plus the DOM layer |
| `site/finder.js` | "Find any build": catalog parsing, filters and rendering (loaded only when the view is opened) |
| `site/data/focalpoint.json` | the ranked book, exported by `focalpoint site` and committed |
| `site/data/catalog/` | the catalog of every build: `index.txt` and shards `sNNN.txt`, exported by `focalpoint site` and committed |
| `site/.nojekyll` | serve files as they are |
| `tests/site/test.html` | browser tests for `app.js` |
| `tests/site/details.html` | a screenshot harness that opens the first listing's panels |
| `.github/workflows/pages.yml` | publishes `site/` to GitHub Pages |

## Updating the data

The ranking model needs every player's runs, so it cannot run in a browser.
`focalpoint site` precomputes the rankings for every **scope** (all
activities, each Searchlight target group, and each activity of a group with
more than one), then describes each build group from its captured runs:

```sh
focalpoint import --backups ../searchlight-backups   # if the database changed
focalpoint site                                      # writes site/data/focalpoint.json
git add site/data/focalpoint.json
```

It takes about 90 s: the book needs four model fits per scope (types,
families, loadouts, exact builds) and writes about 6 MB (1.6 MB compressed,
which is how GitHub Pages serves it); the catalog writes 219 text files,
157 MB (46 MB compressed — about what git stores). Recipes are stored once and serialized as positional
arrays, since they are most of the file. The output is deterministic, so an
unchanged database gives an unchanged file. A build type is listed in a
scope once at least 2 players have used it there for half a run
(`--min-players`); the page defaults to 3.

## Previewing and testing

Browsers will not load the data from a `file://` page, so serve the
directory:

```sh
python3 -m http.server -d site 8000          # then open http://localhost:8000
```

The logic tests run in a browser. Serve the repository root and open
`/tests/site/test.html`; it reports `PASSED: N checks` or lists the failures:

```sh
python3 -m http.server 8001                  # then open http://localhost:8001/tests/site/test.html
# or headless:
chrome --headless --dump-dom http://localhost:8001/tests/site/test.html | grep verdict
```

## Publishing

The workflow deploys `site/` whenever a push to `master` changes it. One-time
setup in the GitHub repository: **Settings → Pages → Build and deployment →
Source: GitHub Actions**.

## Data format (`format: 3`)

```text
{ format, imported_at, first_run, last_run,
  icon_base,                    // "https://www.bungie.net"
  dict:     [[name, icon, type, damage type]],  // what recipes refer to by index
  stats:    [[name, icon]],     // the six armor stats in recipe order
  elements: { name: icon },     // damage-type glyphs ("Arc", "Solar", …)
  icons:    { name: path },     // icons for names in build types
  types:    [{ class, element, armor, weapon, super, aspects }],
  recipes:  [recipe],           // positional, below
  scopes:   [{ id, name, kind: all|group|activity, parent, clears, build_runs, players,
               rows: [[type, effect, raw, players' skill, players, runs, last used,
                       top pick, [variant pick], variants not listed,
                       loadout clears, loadout exact builds]] }] }

pick    = [recipe index, effect, runs, players, last used]
recipe  = [build id (string), stats [6] or null, kit, [armor piece], [weapon],
           [[set, pieces, [bonus]]], artifact item, [artifact perk]]
kit     = [subclass, super, class ability, movement, melee, grenade,
           [aspect], [fragment], [transcendence]]
armor   = [slot 0–4 (helmet … class item), item, exotic 0/1, archetype, tuning,
           [stat mod], [mod, one per copy], [perk]]
weapon  = [slot 0–2 (kinetic, energy, power), item, exotic 0/1, frame, [perk],
           origin trait, mod]
```

Numbers inside recipes are indices into `dict`; an entry's type ("Pulse
Rifle", "Stasis Fragment") feeds the tooltips and a weapon's damage type its
element glyph. Stats are stored Weapons, Health, Class, Grenade, Super, Melee
and shown in the game's order. Effects are on the log scale (`exp(x) − 1` is
the percentage). Bump `SITE_FORMAT_VERSION` (`crates/db/src/site.rs`) and
`FORMAT` (`site/app.js`) together when the layout changes; the page refuses
data of another format.

## Catalog format (`focalpoint-catalog 2`)

Plain text, numbers in base 36, `-` for none, lists comma-separated.
Players reuse a weapon roll, an armor piece or a kit across many builds, so
builds refer to **parts** stored once per shard. Build types are sorted by
name and packed into shards of about 3,000 builds (a type larger than that
spans several shards), and builds within a type by loadout.

`index.txt` (≈ 3.4 MB, 0.7 MB compressed) — loaded when the view opens:

```text
focalpoint-catalog 2
@meta        key<TAB>value                 imported_at, first_run, last_run, icon_base, builds, shards
@dict        name<TAB>icon<TAB>type<TAB>damage type   referenced by index from types and shards
@stats       name<TAB>icon                 the six armor stats in recipe order
@elements    name<TAB>icon                 damage-type glyphs
@activities  name<TAB>target group
@types       class element armor weapon super aspects builds runs players shards activities   (TAB-separated)
```

`sNNN.txt` (≈ 750 KB, 220 KB compressed) — loaded when a search needs it.
Each section's lines are numbered from 0 within the shard:

```text
focalpoint-shard 2
@kits      subclass super class-ability movement melee grenade aspects fragments transcendence
@weapons   slot(0 kinetic,1 energy,2 power) item exotic(0/1) frame origin mod perks
@armor     slot(0 helmet … 4 class item) item exotic archetype tuning stat-mods mods perks
@artifact  item perks
@sets      set:pieces:bonus.bonus,…      (each set with the bonuses it activates)
@builds    id type kit weapons armor artifact sets stats runs players clears qualified last-day effect activities
```

In `@builds`, `id` is the build id's last 8 base-36 digits; `type` indexes
`@types` in the index; `kit`, `weapons`, `armor`, `artifact` and `sets` index
the shard's own sections; `stats` are the six final stats; `qualified` counts
qualified clears; `last-day` is days since 1970; `effect` is the exact
build's effect in thousandths (`-` if not ranked). Names everywhere are
indices into the index's `@dict`.

## What the data contains

Exact builds (equipment only) with counts of runs, players and clears and
the fitted effects. No player names, Bungie names or membership ids are
exported (a test checks this for the book; the catalog's fields are listed
above).
