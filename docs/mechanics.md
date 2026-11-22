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
| `court/` | Court's damage buffs/debuffs stacking guide | damage buffs by stacking category (below) | name |
| `aegis/` | TheAegisRelic's boss damage workbook (xlsx), and the 205 DIM loadouts it links (`dim/<share id>.json`) | the ranked rotations and their loadouts | hash (loadouts) |
| `cooldowns/` | Ability cooldowns and energy factors | the energy economy (below) | name |
| `endgame/` | Endgame Analysis expert ratings | not yet | name |
| `scaling/` | MossyMax's damage scaling (pre-MoT) | enemy health, measured bosses, the Power delta curve ([durability.md](durability.md)) | name |

Google Sheets tabs are saved as their static HTML (`htmlview/sheet`), which
holds exactly what the sheet shows; the CSV export infers a type per column
and drops cells that do not fit it (198 of Game Mechanics' 734). The HTML
leaves out the columns a sheet hides, so a tab whose hidden columns are
needed is read from its CSV (the cooldown workbook's Supers tab). A frozen
column puts a handle cell in every row of the HTML; it is not a column of
the sheet.

**Clarity.** Clarity asks apps that use or display its data to arrange a
partnership (d2clarity.com/partnerships). This project does not: the terms
over-reach on data that is largely the community's, and its database is
public. Clarity's content is read from that repository for this personal
project, and every number derived from it keeps its provenance
(`clarity:<hash>`). Revisit before the public site shows anything that rests
on Clarity alone.

## Importing

```sh
focalpoint mechanics import                 # reads ~/.local/share/focalpoint/sources
focalpoint mechanics import --sources DIR
focalpoint mechanics import --contexts FILE --difficulty FILE --enemy-health FILE   # while editing the data/ files
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
| `mechanic_tag`, `mechanic_hash_tag` (view) | — | what each entry's text says its subject needs and does: triggers, payoffs, states produced and depended on, each with its evidence sentence ([effects.md](effects.md)) |
| `mechanic_keyword` | — | runs of text a source marks: Clarity's element and ammo classes (`solar`, `special`, `enhancedArrow`, …), the Compendium's text colours (`#rrggbb`) |
| `mechanic_rotation` | `(source, rotation_id)` | a measured boss-damage rotation: category (Universal, Specialized, Support, …), class, boss, `core_dps` (its repeating core), average DPS after 10–60 s and at its end, artifact mods, other factors, notes |
| `mechanic_rotation_loadout` | — | the DIM loadouts a rotation was run with: `role` `loadout 1`, `loadout 2`, or `note` (a variant a cell note links, `label` its line) |
| `mechanic_loadout` | `(source, loadout_id)` | a shared DIM loadout (`loadout_id` its dim.gg share id): name, class |
| `mechanic_loadout_item` | — | what it uses: `equipped` items, `socket` choices on them (`parent_hash`, `socket_index`: the subclass's super, abilities, aspects and fragments, exotic class item perks), armor `mod`s, `artifact` unlocks |

Game states — Radiant, Scorch, Volatile, Woven Mail, Jolt, Frost Armor and
the rest, defined in each subclass tab's glossary — are entries without a
subject.

## Measured rotations

TheAegisRelic's boss damage workbook measures, for Monument of Triumph,
what each damage rotation actually deals: base damage of every weapon and
ability, the multipliers of buffs, debuffs and surges, and on its ranking
sheet (`Ranking (S29)`) 109 tested rotations with their DPS over time —
each linked to the DIM loadouts it was run with (dim.gg shares; DIM's
share API answers without a key). The import reads the ranking sheet's
cached figures, its notes, and the saved loadouts: every rotation has its
loadout, all 534 hashes resolve in the manifest, and 104 of the 109 run
two set bonuses. These are expert-built, measured builds: a check on the
synergy evaluator (it should rate them well) and templates for build
synthesis. The workbook is read directly (`xlsx`), since its hyperlinks
and notes carry much of its meaning; its notes are context-sensitive and
kept with each rotation. Not yet read: the base values (Weapons,
Abilities), single-source and swap DPS (Sustained, Swap), the damage
curves behind the ranking (Graphing) and team compositions (Fireteam).

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

## Contexts

The import also builds the profiles of the contexts builds are evaluated
in — activity groups and activities from the recorded clears, boss
encounters from the boss damage workbook's `Bosses` sheet, and
hand-maintained detail from `data/contexts.toml`. See
[contexts.md](contexts.md).

## Enemy durability

And how much damage enemies take to kill: archetype health, measured
bosses, the Power delta curve and Champion stats, with each activity
group's Power delta and health tier from `data/difficulty.toml`. See
[durability.md](durability.md).

## Energy economy

The ability cooldown workbook (`cooldowns/`) gives the build model's energy
economy ([build-model.md](build-model.md)). Percentages are stored as
fractions.

| Table | What |
|---|---|
| `ability_cooldown` | each grenade, melee, class ability and super: `cooldown_s` at 0 stat, the flat-gain factor (the share of a chunk of energy it receives), class abilities' class-mod multiplier, supers' tier |
| `super_damage` | the Supers tab: each super, often with an exotic, at its best: damage per cast, cast time, DPS, tier, cooldown |
| `stat_curve` | what each stat gives every 10 points from 0 to 200 (Armor 3.0): `cooldown_scalar`, `energy_gained`, `damage_increase`, `health_per_orb`, … |
| `regen_effect` | parts that speed an ability's regeneration: the bonus over the 0-stat rate at 0 and 100 stat (Spark of Recharge: +160%, +360%) |
| `flat_gain` | parts that grant a chunk of energy, by ability (Impact Induction: 12% of a grenade) |
| `cooldown_subject` | the manifest items each part named in these tables is |

How the numbers combine, from the workbook's notes and its own figures:

- **Cooldown** at stat *s* is `cooldown_s / cooldown_scalar(s)`. The
  scalar is 1.00 at 0, 1.38 at 30, 2.56 at 70 and 2.75 at 100 for grenades
  and melees (2.42 for class abilities). Above 100 a stat adds damage,
  not cooldown.
- **Chunks of energy and regen effects** scale by
  `1 + energy_gained(s)`: ×2.25 at 100. So Spark of Recharge's +160% at 0
  is +360% at 100. 70 stat matches the old (pre-Edge of Fate) benefit.
- **Supers' base cooldowns** are 455 s (tier T4), 500 s (T3) and 556 s
  (T2). The Super stat scales the energy gained, not the base cooldown.

What to know about the sheet:

- The ability tabs' "T3 Cooldown" is the base before Edge of Fate. It is
  the T0 cooldown over 1.45 for every ability, where the Armor 3.0 curve
  gives 1.38 at 30 stat. It is kept as `pre_eof_base_s`.
- Above 100 the grenade cooldown scalar reads 2.17 (the melee one from 120
  up), while the cooldown reduction stays −63.6%, which is a scalar of 2.75.
  The 2.17s are inconsistent with the rest of the sheet. The table keeps
  what the sheet says; the model uses 2.75.
- The Supers tab lists super and exotic pairs. A super's cooldown is the
  most common among its rows; the import reports rows that disagree
  (Winter's Wrath is listed as both T2 and T3, both at 556 s).
- A few names match no manifest item: Ionic Traces (a pickup, not a part)
  and Rally and Towering Bastion (Barricades changed by the Bastion
  aspect). The sheet's spellings "Heartof Inmost Light", "Sunbracer's" and
  "Contraverse" are read as the manifest's names.

Every grenade, melee, class ability and super recorded builds use has a
cooldown, except Shatter Grenade, Slicewire Grenade and Phantom Surge, which
the workbook does not list. Uses covered: class abilities and supers 100%,
melees 97.8%, grenades 92.8%.

## Damage buffs

Court's stacking guide (`court/`) sorts every damage buff and debuff into
the category that decides how it stacks. Within global debuffs (Weaken,
Tether…), empowering buffs (Radiant, Well, Rift…) and weapon surges only
the highest applies. Amplification, exotic, weapon perk and world
modifiers multiply with everything. These are the sandbox vocabulary's
`damage_categories`.

| Table | What |
|---|---|
| `damage_buff` | one buff, or one way of getting it: `category`, the guide's `section`, `name` and `kind` ("Void Hunter Super"), `trigger`, `pve` (the largest PvE value), `duration_s`, notes, when it was introduced |
| `damage_buff_step` | PvE values by stack, where the guide gives one per stack (surges: 10%, 17%, 22%, 25%) |
| `damage_buff_subject` | the manifest items each name is |

The guide gives values for global debuffs (43 rows), empowering buffs (24),
surges (23) and the amplification table (27). For exotic intrinsics, weapon
perks and world modifiers it only lists names (158 rows, `trigger` NULL);
their values are in the Compendium. A few of the guide's names are read as
the manifest's: Deadfall and Moebius Tether as their Shadowshot supers,
Banner Shield as Sentinel Shield, "Arc Surge Mod" as Arc Weapon Surge. 278
of the 304 rows name a manifest item. The rest are not items: the Weapons
stat, world and activity modifiers, intrinsic effects, and raid mods. PvP
values are not read.

## Next

- MossyMax's combatant damage scalars by weapon archetype.
- The boss-damage workbook's base values and per-source DPS, and expert
  ratings (a yardstick for the evaluator).
