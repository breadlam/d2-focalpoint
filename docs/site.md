# The build-ranking site

`site/` is a static page for GitHub Pages that ranks builds by their
**build effect** (ADR 0006): how much better players do with a build than on
their own other runs. One page covers two uses:

1. **Search builds by constraints** (class, subclass, super, exotic armor,
   exotic weapon, aspects, or a fragment/weapon/mod the build is usually run
   with), ranked by build effect across all activities.
2. **Recommendations for an activity**: pick a target group (e.g. "Master
   Raid") or a single activity and get the builds ranked there, with the same
   constraints.

Each build expands to show how it is usually run (abilities, stat focus,
fragments, class item perks, weapons, armor mods, set bonuses, artifact
perks) and how it ranks in other activities. The filters and activity are
kept in the URL, so a view can be shared as a link.

## Files

| Path | What it is |
|---|---|
| `site/index.html`, `site/style.css`, `site/favicon.svg` | the page; no build step, no dependencies |
| `site/app.js` | an ES module: pure functions (filtering, facet counts, ranking, URL state) plus the DOM layer |
| `site/data/searchlight.json` | the data, exported by `searchlight site` and committed |
| `site/.nojekyll` | serve files as they are |
| `tests/site/test.html` | browser tests for `app.js` |
| `tests/site/details.html` | a screenshot harness that opens the first build's details |
| `.github/workflows/pages.yml` | publishes `site/` to GitHub Pages |

## Updating the data

The ranking model needs every player's runs, so it cannot run in a browser.
`searchlight site` precomputes the rankings for every **scope** (all
activities, each Searchlight target group, and each activity of a group with
more than one), then describes each build group from its captured runs:

```sh
searchlight import --backups ../searchlight-backups   # if the database changed
searchlight site                                      # writes site/data/searchlight.json
git add site/data/searchlight.json
```

It takes about 15 s and writes about 3.5 MB (0.9 MB compressed, which is how
GitHub Pages serves it). The output is deterministic, so an unchanged
database gives an unchanged file. A build is listed in a scope once at least
2 players have used it there for half a run (`--min-players`); the page
defaults to 3.

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

## Data format (`format: 1`)

```text
{ format, imported_at, first_run, last_run,
  icon_base,                    // "https://www.bungie.net"
  icons:  { name: path },       // item and perk icons, hotlinked from Bungie's CDN
  names:  [component name],     // referenced by index from details
  groups: [{ class, element, armor, weapon, super, aspects,
             details: { runs, stats: [6 mean levels 0-4],
                        components: { kind: [[name index, percent of runs]] } } }],
  scopes: [{ id, name, kind: all|group|activity, parent, clears, build_runs, players,
             rows: [[group, effect, raw, players' skill, players, runs, last used]] }] }
```

Effects are on the log scale (`exp(x) − 1` is the percentage). Bump
`SITE_FORMAT_VERSION` (`crates/db/src/site.rs`) and `FORMAT`
(`site/app.js`) together when the layout changes; the page refuses data of
another format.

## What the data contains

Aggregates only: per build group and scope, the number of players and runs
and the fitted effects. No player names, Bungie names or membership ids are
exported (a test checks this).
