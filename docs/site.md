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

Pick "All activities", a target group (e.g. "Master Raid") or a single
activity to get the listings ranked there, and narrow them by class,
subclass, super, exotics, aspects or a component a listed recipe includes.
Filters and activity are kept in the URL, so a view can be shared.

## Files

| Path | What it is |
|---|---|
| `site/index.html`, `site/style.css`, `site/favicon.svg` | the page; no build step, no dependencies |
| `site/app.js` | an ES module: pure functions (recipe decoding and differences, filtering, facet counts, ranking, URL state) plus the DOM layer |
| `site/data/focalpoint.json` | the data, exported by `focalpoint site` and committed |
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

It takes about 45 s (four model fits per scope: types, families, loadouts,
exact builds) and writes about 6 MB (1.6 MB compressed, which is how GitHub
Pages serves it). Recipes are stored once and serialized as positional
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

## Data format (`format: 2`)

```text
{ format, imported_at, first_run, last_run,
  icon_base,                    // "https://www.bungie.net"
  dict:    [[name, icon path or null]],   // what recipes refer to by index
  icons:   { name: path },                // icons for names in build types
  types:   [{ class, element, armor, weapon, super, aspects }],
  recipes: [recipe],                      // positional, below
  scopes:  [{ id, name, kind: all|group|activity, parent, clears, build_runs, players,
              rows: [[type, effect, raw, players' skill, players, runs, last used,
                      top pick, [variant pick], variants not listed,
                      loadout clears, loadout exact builds]] }] }

pick    = [recipe index, effect, runs, players, last used]
recipe  = [build id (string), stats [6] or null, kit, [armor piece], [weapon],
           [[set, pieces]], [set bonus], [artifact perk]]
kit     = [subclass, super, class ability, movement, melee, grenade,
           [aspect], [fragment], [transcendence]]
armor   = [slot 0–4 (helmet … class item), item, exotic 0/1, archetype, tuning,
           [stat mod], [mod, one per copy], [perk]]
weapon  = [slot 0–2 (kinetic, energy, power), item, exotic 0/1, frame, [perk],
           origin trait, mod]
```

Numbers inside recipes are indices into `dict`. Effects are on the log scale
(`exp(x) − 1` is the percentage). Bump `SITE_FORMAT_VERSION`
(`crates/db/src/site.rs`) and `FORMAT` (`site/app.js`) together when the
layout changes; the page refuses data of another format.

## What the data contains

Exact builds (equipment only) with per-scope counts of runs and players and
the fitted effects. No player names, Bungie names or membership ids are
exported (a test checks this).
