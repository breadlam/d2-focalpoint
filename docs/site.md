# The build-ranking site

`site/` is a static page for GitHub Pages, branded **Skinwalker** as
Searchlight's own dashboard was: the Rahool mask (Festival of the Lost)
beside the name in Jost Light, caps with wide tracking (Jost is an open
Futura clone; Futura is the Destiny logo face), and Searchlight's class
symbols beside class names. Only the page carries the brand; the tool and
its data remain Focalpoint. It has three views. **Ranked builds**
lists build types ranked by build effect, each with exact builds players
actually used in qualified clears, published exactly as equipped —
subclass, abilities, aspects, fragments, weapons with their frame, perks,
origin trait and mod, armor pieces with archetype, tuning, stat mods and
mods, set bonuses, artifact perks and final stats. Nothing is averaged or
combined; the ranking tools only choose which builds to show:

1. **Build types** (class, subclass element, exotic armor and weapon, super,
   aspects) are ranked by **build effect** (ADR 0006): how much better
   players do with the type than on their own other runs. Solo clears count,
   measured against other solo clears of the same activity plus that
   activity's solo premium (ADR 0008); players who joined late or left
   early are measured by kill rate over their time played against others
   who did (ADR 0009).
2. Within a type, **families** (the same kit: exotic perks, abilities,
   aspects, fragments) and within the best family, **loadouts** (the same
   kit and legendary weapons) are ranked the same way, pooling the evidence
   of near-identical builds.
3. The listing's **top build** is the best-ranked exact build of the best
   loadout that was used in at least 3 clears and beat its players' usual
   level (falling back to 2 clears, then to the best loadout).
4. Its **near-identical variants** are the loadout's other exact builds
   within 8 element changes (a perk roll, a mod, an artifact perk), each
   listed with exactly what differs and its own full build.

"Minimum players" counts the different players who ran a build type in the
chosen activity (each for at least half a run); their exact builds may
differ. It defaults to 3; 1 includes build types only one player used.

Listings lead with plain usage (players, runs, last used). The ranking
numbers — build effect, raw result, players' skill — sit under "Ranking
details": Searchlight's players were already among the best, so most builds
sit near zero and a very good build can read slightly negative, which
without that context sends the wrong message. The list is still ordered by
build effect ("Best ranked").

Every build has **Copy link to this build**: a finder URL with the build
type's names and the build's short id (`&build=<id>`), which opens exactly
that build, with a way to see the rest of its type. Links keep working
across exports as long as the build's identity rules (ADR 0002) do not
change.

Builds are drawn the way Searchlight drew them (after the in-game loadout
screen): weapons and subclass, stats and set bonuses, armor and artifact, as
icon tiles in aligned columns whose names show on hover, focus or tap;
narrow screens stack the sections in the same reading order. The page uses
Searchlight's dark palette.

Pick "All activities", a target group (e.g. "Master Raid") or a single
activity to get the listings ranked there, and narrow them by class,
subclass, super, exotics, aspects or a component a listed build includes.
Filters and activity are kept in the URL, so a view can be shared.

**Highlights** picks builds per class and subclass, as well ranked and as
unlike each other as the data allows (ADR 0007): for the **neutral game**
(workhorses, ranked by the part of their effect that holds across activity
groups; up to three per classic subclass and six Prismatic) and for every
activity group and activity (up to one per subclass). A subclass shows
fewer when its next build would rank below average or only swap an earlier
one's exotics. There are no
player or rank cut-offs: every ranked build type whose exemplar build is
complete (two aspects, four or more fragments, four abilities, armor mods)
is a candidate, and each pick says how
much of its playstyle (subclass, super, aspects, exotics, abilities,
fragments) is new compared with its subclass's picks above. Neutral picks also say
how many activity groups the build type was used in.

**Find any build** searches the **catalog**: every build
Searchlight collected (594,340), exactly as equipped, whether or not it was
used in a qualified clear. Its filters work on build types first (class,
subclass, super, exotics, aspects, activity); once they select at most
45,000 builds, the page downloads just the shards holding them and matches
everything the builds include (fragments, weapons, perks, mods, set bonuses,
artifact perks). Ranked builds come first, the rest by use.

## Files

| Path | What it is |
|---|---|
| `site/index.html`, `site/style.css` | the page; no build step, no dependencies |
| `site/brand/` | Searchlight's brand assets: the Rahool mask (`rahool-mask.png`, rendered at 192 px from Searchlight's SVG; also the favicon), `Jost-Light.ttf` with its licence (`OFL.txt`), and the class symbols (`class-icons/`, CC0) |
| `site/app.js` | an ES module: pure functions (build decoding and differences, filtering, facet counts, ranking, URL state) plus the DOM layer |
| `site/highlights.js` | "Highlights": URL state and rendering (loaded only when the view is opened) |
| `site/finder.js` | "Find any build": catalog parsing, filters and rendering (loaded only when the view is opened) |
| `site/data/book/` | the ranked builds and highlights: `index.json`, two files per scope and one `highlights.<set>.json` per highlight set, exported by `focalpoint site` and committed |
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
focalpoint site                                      # writes site/data/book/ and site/data/catalog/
git add -A site/data
```

It takes about 90 s. The ranked builds need four model fits per scope
(types, families, loadouts, exact builds); they are split into
`book/index.json` (names, types, and each scope's listings in brief, for
"ranked in other activities") and two files per scope: build types with at
least 3 players, loaded with the page, and those with 1–2 players, loaded
only when the minimum is lowered. Opening the page fetches about 0.9 MB
(compressed, which is how GitHub Pages serves it); all 88 files total
28 MB (about 7 MB compressed). The catalog writes 219 text files, 151 MB
(46 MB compressed — about what git stores). Exact builds are serialized as
positional arrays, since they are most of the data. The output is
deterministic, so an unchanged database gives unchanged files. A build type
is listed in a scope once at least `--min-players` (default 1) players have
used it there for half a run.

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

## Data format (`format: 6`)

`book/index.json`:

```text
{ format, imported_at, first_run, last_run,
  icon_base,                    // "https://www.bungie.net"
  dict:     [[name, icon, type, damage type]],  // what builds refer to by index
  stats:    [[name, icon]],     // the six armor stats in build order
  elements: { name: icon },     // damage-type glyphs ("Arc", "Solar", …)
  icons:    { name: path },     // icons for names in build types
  types:    [{ class, element, armor, weapon, super, aspects }],
  scopes:   [{ id, name, kind: all|group|activity, parent, clears, build_runs, players,
               parts:   [[least players, most players or null, file, listings]],
               summary: [[type, effect, players]] }],
  highlights: [[set id, name, file]] }   // "neutral" first, then each group and activity
```

`book/<scope>.json` and `book/<scope>.more.json` (scope ids with `/` as `--`):

```text
{ format, scope,
  rows:    [[type, effect, raw, players' skill, players, runs, last used,
             top pick, [variant pick], variants not listed,
             loadout clears, loadout exact builds]],
  recipes: [build] }            // this file's exact builds, positional, below

pick    = [index into this file's recipes, effect, runs, players, last used]
build   = [build id (string), stats [6] or null, kit, [armor piece], [weapon],
           [[set, pieces, [bonus]]], artifact item, [artifact perk],
           artifact perks are unlocked rather than applied (0/1; older captures)]
kit     = [subclass, super, class ability, movement, melee, grenade,
           [aspect], [fragment], [transcendence]]
armor   = [slot 0–4 (helmet … class item), item, exotic 0/1, archetype, tuning,
           [stat mod], [mod, one per copy], [perk]]
weapon  = [slot 0–2 (kinetic, energy, power), item, exotic 0/1, frame, [perk],
           origin trait, mod]
```

`book/highlights.<set>.json` (set ids with `/` as `--`):

```text
{ format, id, name,
  classes: [{ class, picks: [[type, effect, players, runs, last used, build,
                              activity groups the type was used in, novelty]] }],
  recipes: [build] }            // this file's exact builds; `build` indexes them
```

Picks are grouped by subclass (Arc, Solar, Void, Stasis, Strand,
Prismatic) and in pick order within each. `effect` is the
set's ranking: the neutral effect for `neutral`, else the scope's build
effect. `novelty` is the share of the pick's playstyle not covered by its
subclass's picks above (1 for the first).

Numbers inside builds are indices into `dict`; an entry's type ("Pulse
Rifle", "Stasis Fragment") feeds the tooltips and a weapon's damage type its
element glyph. Stats are stored Weapons, Health, Class, Grenade, Super, Melee
and shown in the game's order. Effects are on the log scale (`exp(x) − 1` is
the percentage). Bump `SITE_FORMAT_VERSION` (`crates/db/src/site.rs`) and
`FORMAT` (`site/app.js`) together when the layout changes; the page refuses
data of another format.

## Catalog format (`focalpoint-catalog 3`)

Plain text, numbers in base 36, `-` for none, lists comma-separated.
Players reuse a weapon roll, an armor piece or a kit across many builds, so
builds refer to **parts** stored once per shard. Build types are sorted by
name and packed into shards of about 3,000 builds (a type larger than that
spans several shards), and builds within a type by loadout.

`index.txt` (≈ 3.4 MB, 0.7 MB compressed) — loaded when the view opens:

```text
focalpoint-catalog 3
@meta        key<TAB>value                 imported_at, first_run, last_run, icon_base, builds, shards
@dict        name<TAB>icon<TAB>type<TAB>damage type   referenced by index from types and shards
@stats       name<TAB>icon                 the six armor stats in build order
@elements    name<TAB>icon                 damage-type glyphs
@activities  name<TAB>target group
@types       class element armor weapon super aspects builds runs players shards activities   (TAB-separated)
```

`sNNN.txt` (≈ 750 KB, 220 KB compressed) — loaded when a search needs it.
Each section's lines are numbered from 0 within the shard:

```text
focalpoint-shard 3
@kits      subclass super class-ability movement melee grenade aspects fragments transcendence
@weapons   slot(0 kinetic,1 energy,2 power) item exotic(0/1) frame origin mod perks
@armor     slot(0 helmet … 4 class item) item exotic archetype tuning stat-mods mods perks
@artifact  item perks unlocked(0/1: perks are the unlocked, not applied, ones)
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
exported (a test checks this for the ranked builds; the catalog's fields are listed
above).
