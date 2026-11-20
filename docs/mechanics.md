# Community mechanics

Better build evaluation needs to know what a build's parts *do* and how they
interact, which the game's own descriptions often leave vague or get
wrong. The community documents it: Clarity's hand-written descriptions
(which DIM shows), the Destiny Data Compendium's tested numbers, and a few
specialist spreadsheets. Focalpoint reads them into the database as a
layer of `mechanic_*` tables, keyed to manifest hashes, for the synergy
evaluator now and for build synthesis later.

Everything here describes Destiny 2 as of Monument of Triumph (update
9.7.0, 9 June 2026; hotfixes to 9.7.0.3, 7 July 2026): the game's last
update, and the period Searchlight recorded.

## Sources

The raw downloads are **not in the repository**. They live in
`~/.local/share/focalpoint/sources/`, one folder per source with a
`manifest.json` (URL, tabs, fetch date); the folder's `README.md` records
each source's provenance, currency and terms.

| Folder | Source | Read by the import | Keyed by |
|---|---|---|---|
| `clarity/` | Clarity Live Database (`descriptions/clarity.json`) | yes | hash |
| `compendium/` | Destiny Data Compendium, 22 tabs | yes (15 current tabs) | name |
| `court/` | Court's damage buffs/debuffs stacking guide | not yet | name |
| `aegis/` | TheAegisRelic's boss damage spreadsheet (xlsx) | not yet | name |
| `cooldowns/` | Ability cooldowns and energy factors | not yet | name |
| `endgame/` | Endgame Analysis expert ratings | not yet | name |
| `scaling/` | MossyMax's damage scaling (pre-MoT) | not yet | name |

Google Sheets tabs are saved as their static HTML (`htmlview/sheet`), which
holds exactly what the sheet shows; the CSV export infers a type per column
and drops cells that do not fit it (198 of Game Mechanics' 734).

**Terms.** Clarity asks apps that use or display its data to arrange a
partnership (free for hobbyist developers, via its Discord;
d2clarity.com/partnerships). The tables are built locally and not
published; arrange one before the public site shows anything derived from
Clarity.

## Importing

```sh
focalpoint mechanics import                 # reads ~/.local/share/focalpoint/sources
focalpoint mechanics import --sources DIR
```

The import writes into an existing database (it does not need a re-import
of the backups) and replaces the layer whole in under a second; its version
is `meta.mechanics_version`. It prints what each source gave and how much
of the recorded builds' component uses each documents.

## Tables

| Table | Key | What |
|---|---|---|
| `mechanic_source` | `source` | each source read: title, URL, fetch date, entries |
| `mechanic_entry` | `(source, entry_id)` | one documented game element or game state: `section` (the source's tab or type, with the tab's heading, e.g. `Solar / Glossary`), `name`, `text` (PvE only) |
| `mechanic_subject` | `(source, entry_id, hash, hash_kind)` | the manifest hashes an entry documents: `hash_kind` `item`, `sandbox_perk` (set bonuses) or `item_set`; `rule` how it was found — `hash` (Clarity's own), `item` (the weapon or armor a Clarity perk is on), `name`, `alias` (a fragment's short name), `family` (an armor mod family) |
| `mechanic_keyword` | — | runs of text a source marks: Clarity's element and ammo classes (`solar`, `special`, `enhancedArrow`, …), the Compendium's text colours (`#rrggbb`) |

Game states — Radiant, Scorch, Volatile, Woven Mail, Jolt, Frost Armor and
the rest, defined in each subclass tab's glossary — are entries without a
subject.

## How entries are found

**Clarity** gives an entry per hash. Its blocks are lines of text runs;
blocks and runs marked `pvp` are left out.

**The Compendium** is laid out for reading, each tab its own way, so its
entries are found by their names (`focalpoint_mechanics::compendium`). The
tab is parsed into a grid with merged cells and coloured runs
(`sheet::Grid`; long tabs come as several tables). A cell is a name when
its first line — or the part after a `|`, as in "2 Piece | Force
Absorption" — is a manifest name of a kind the tab lists (weapon perks,
set bonuses, aspects, …), ignoring case, a standalone "the" and `&`/"and";
fragments are listed by their short name on their element's tab ("Ashes"),
armor mods by family ("Ammo Finder" for every "… Ammo Finder"). A short
cell with a long text beside it is a name with no subject. An entry's text
is the rest of its name cell, then the cells beside it in the tab's reading
direction (leftwards on Artifact Perks) over the rows it spans, up to the
next name; a name cell whose own further lines are long carries its own
text (set bonuses). To see a tab as the importer does:

```sh
cargo run -p focalpoint-mechanics --example grid -- ~/.local/share/focalpoint/sources/compendium/Solar.html 0 40
```

## Coverage

Share of the recorded builds' component uses documented (2026-11-20):

| Component | Clarity | Compendium | either |
|---|---|---|---|
| armor mods | 100% | 58% | 100% |
| fragments, aspects, class abilities | 68–100% | 100% | 100% |
| set bonuses | — | 100% | 100% |
| grenades, melees, supers | — | 100% | 100% |
| exotic armor perks | 82% | 99% | 99% |
| artifact perks | — | 98% | 98% |
| origin traits | 29% | 99% | 99% |
| weapon mods | 15% | 95% | 96% |
| weapon frames | 34% | 61% | 90% |
| weapon perks | 53% | 41% | 55% |

Weapon perks fall short only on barrels, magazines and stocks — plain stat
perks nobody documents; armor archetypes, tuning and jumps are not
documented by these sources.

## Next

- **States a part produces and depends on** (`mechanic_link`): "grants
  Radiant", "inflicts Weaken", "creates Orbs" against "while Radiant",
  "against Weakened targets", "on Orb pickup" — the links a synergy
  evaluator credits, flags or penalises.
- **Damage-modifier categories** from Court's stacking guide: global
  debuffs, empowering buffs and weapon surges do not stack within their
  category.
- The boss-damage measurements, ability cooldowns and expert ratings, the
  last as a yardstick for the evaluator.
