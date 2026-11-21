# Contexts

A build's worth depends on where it is played. PvE builds split broadly
into **add clear** and **boss damage**, on a gradient: from easy,
enemy-dense activities, through difficult content where ordinary enemies do
not fall easily, to a single boss with no adds and damage windows of
varying length. Kill-driven engines (ability energy on kill, Orbs from
kills) thrive at one end and flounder where kills are scarce or hard;
boss-damage builds live on hits, with or without anything else to kill.
So builds are evaluated **per context**: per activity, and for raids and
dungeons per encounter. A correct evaluator rates TheAegisRelic's measured
boss rotations highly against bosses and markedly lower elsewhere.

The recorded clears show the gradient (scored fireteam clears, medians):

| Activity group | Kills/min | Precision share | Deaths/10 min |
|---|---|---|---|
| Ultimate Fireteam Op | 9.1 | 0.07 | 0.76 |
| Ultimate Pinnacle Op, Equilibrium | 5.3 | 0.07–0.11 | 0.97–1.19 |
| Master Dungeon | 4.9 | 0.10 | 1.52 |
| Ultimate Conquest, Master Raid, Heliostat | 4.1–4.3 | 0.04–0.11 | 0–1.97 |
| The Desert Perpetual | 3.3 | 0.09 | 1.16 |
| Pantheon 2.0 (boss rush) | 3.1 | 0.18 | 1.26 |

## Profiles

Contexts form a tree — activity group › activity › encounter — and each has
a **profile** built by `focalpoint mechanics import` (`docs/mechanics.md`).
A profile is open-ended, so detail can be added as it is learned without
changing the schema:

- **Features**: named properties, each a number and/or a text with its
  source. Names are dotted (`boss.dps_window_s`, `adds.during_dps`); the
  evaluator reads the ones it knows. The same feature from two sources
  stands side by side.
- **Events**: things that happen at a time — `dps_window`, `add_wave`,
  `mechanic`, … — with optional start, length and detail.
- **Activities**: the manifest activities a context covers, so runs and
  rankings join to it at any level.

| Source | Contexts | Features |
|---|---|---|
| `runs` — the recorded clears | every activity group and activity with scored clears | `clears`, `solo_share`, and over full fireteam clears: `kills_per_min`, `precision_kill_share`, `ability_kill_share`, `super_kill_share`, `deaths_per_10min`, `duration_min`, `fireteam_size`; `resumable` (share of its activities with checkpoints); `difficulty` (the step most clears are at) and `activity_level` (its level); shares of clears with `equipment_locked`, Champions (`champions.barrier`, `champions.overload`, `champions.unstoppable`), `combatants.difficult` and `combatants.deadly`, `feat.cutthroat_combat`, `feat.battalions` and `banes`; `galvanized` (mean level 0–3: enemies harder to stagger) |
| `curated` — [`data/difficulty.toml`](../data/difficulty.toml) | each group or activity it names, and their descendants, from the entry for the step most of its clears are at | `power_delta`, `damage_multiplier`, `durability` (damage to kill against a delta of 0), `power_cap`, `combatant_power`, `health_tier`, `champion_difficulty`, `difficulty_confidence`, `difficulty_evidence` ([durability.md](durability.md)) |
| `aegis` — the boss damage workbook's `Bosses` sheet | 61 boss encounters of raids, dungeons, Pantheon and Equilibrium, each a child of the scored activity of its name (22, of raids and dungeons Searchlight did not target, have none) | `boss.health`, `boss.effective_health`, `boss.dps_window_s` (also a `dps_window` event), `boss.range`, `adds.during_dps` (No / Few / Many), `adds.range`, `setup` (None … Long), `mechanics`, `boss.species`, `boss.model`, `notes`, `dps.clearable`, `dps.one_phase`, `mods` |
| `curated` — [`data/contexts.toml`](../data/contexts.toml) in this repository | adds to any context by id, or creates one | anything: e.g. `enemies.<type>.count`, `enemies.<type>.health`, `adds.waves`, `mechanics.<kind>` (teleport, verticality, …), with timed events for add waves and damage windows |

Ids: `group:<slug>`, `activity:<slug of the activity name>`,
`encounter:<activity slug>/<boss slug>`. The curated file is built into the
binary; `focalpoint mechanics import --contexts FILE` reads another while
editing it (`--difficulty FILE` likewise for `data/difficulty.toml`).

```sh
focalpoint mechanics context                  # every context, with its feature and event counts
focalpoint mechanics context caretaker        # one profile in full
```

## Tables

| Table | Key | What |
|---|---|---|
| `context` | `context_id` | kind (`group`, `activity`, `encounter`), parent, name |
| `context_activity` | `(context_id, activity_hash)` | the manifest activities it covers |
| `context_feature` | `(context_id, feature, source)` | `value` (number) and/or `text` |
| `context_event` | — | `seq`, `kind`, `start_s`, `length_s`, `detail`, `source` |

## Limits

- **Encounters have profiles but not outcomes.** Post-game reports cover a
  whole run, so recorded clears cannot be split by encounter. Searchlight's
  build timeline (sampled every 10–60 s) shows build swaps within runs and
  may let fresh-start clears be divided into encounters; until then,
  per-encounter checks rest on the boss damage workbook and expert ratings.
- **The run score is kill-weighted.** It scores kills, speed and deaths
  (ADR 0006); in boss-heavy content, boss damage shows only through speed.
