# Enemy durability

How much damage enemies take to kill, per enemy and per context. Escalation
loops turn on it ([escalation.md](escalation.md)). Enemies that survive long
enough sustain ignition chains and damage-counted ammo. Enemies that die
quickly feed kill-volume engines.

An enemy's **damage to kill** is its health at the activity's **health
tier** divided by the outgoing **damage multiplier** of the **Power delta**
it is fought at:

    damage to kill = health(enemy, rank, health tier) × durability
    durability     = 1 / damage multiplier(Power delta)

| Power delta | Damage multiplier | Durability |
|---|---|---|
| 0 | ×1.000 | ×1.00 |
| −10 | ×0.802 | ×1.25 |
| −20 | ×0.618 | ×1.62 |
| −30 | ×0.446 | ×2.24 |
| −50 | ×0.144 | ×6.97 |

`focalpoint mechanics import` reads these numbers
([mechanics.md](mechanics.md)):

| Table | Source | What |
|---|---|---|
| `enemy_health` | MossyMax's Foetracer measurements (Damage Scaling v1.9.2, Feb 2025: **before Edge of Fate**) | 55 enemy archetypes by faction, minor and elite, at health tiers 1–4 (1: patrol, story; 2: matchmade strikes; 3: Legend Nightfalls, dungeons; 4: Master raids, Master and Grandmaster Nightfalls) |
| `enemy_measured` | the same | 44 measured bosses and minibosses: estimated health (damage dealt) and baseline health |
| `power_delta` | MossyMax's `EoF Standard` curve, relative to a delta of 0 | the damage multiplier from −100 to +100. It matches the Data Compendium's ×0.446 at −30 and ×0.144 at −50 |
| `champion_stat` | the Data Compendium (current) | Champion health (2,200 Standard … 5,475 Master/Grandmaster), shields, regeneration and stun times, by difficulty |
| view `context_enemy_health` | the above with each context's `health_tier` and `durability` | every archetype's health and damage to kill in every context that has both |

## Each context's difficulty

[`data/difficulty.toml`](../data/difficulty.toml) gives each activity group a
Power delta, health tier and Champion column, with a **confidence** and its
sources:

- **documented**: a current source states it;
- **dated**: from before Edge of Fate, or superseded;
- **assumed**: inferred.

The import adds them as features of the group, its activities and their
encounters ([contexts.md](contexts.md)): `power_delta`,
`damage_multiplier`, `durability`, `health_tier`, `champion_difficulty`,
`difficulty_confidence` and `difficulty_evidence`.

| Group | Delta | Confidence |
|---|---|---|
| Ultimate Fireteam Op, Ultimate Pinnacle Op | −30 | documented |
| Ultimate Conquest, Grandmaster Conquest, Heliostat | −30 | assumed |
| Master Raid, Master Dungeon | −20 | dated |
| Equilibrium, Pantheon, The Desert Perpetual | — | unknown |

What is known:

- **Edge of Fate** (9.0.0.1 patch notes, July 2025) set the Portal's steps
  at Normal 0, Advanced −10, Expert −20, Master −30, Grandmaster −40 and
  Ultimate −50. It also made **red-bar enemies gain health more slowly**
  with delta than majors and bosses ("kill-triggered perks will be more
  useful in high-end content"). `durability` applies one curve to every
  rank, so it overstates minors' durability; the patch notes give no
  numbers.
- **The Data Compendium** (current) gives **Ultimate −30** and the
  Grandmaster Alert −50, so Ultimate was eased after launch.
- **The recorded clears** agree with −30, not −50 (below).
- **The manifest's difficulty ladders** give each step an activity level:
  Portal Normal 31, Advanced 33, Expert 35, Master 40, Grandmaster 45,
  Ultimate 50; Conquests' Grandmaster steps 44–55 and Ultimate 58–60. This
  is the `activity_level` context feature. Conquests sit at or above the
  Portal's Ultimate, so −30 is a floor for them.
- **Skulls** name what each context fields: its Champion types,
  Difficult or Deadly combatants, banes, and **Galvanized** I–III, which
  make enemies harder to stagger, not tougher. The Compendium says that with
  Galvanized II only Unstoppable weapons stagger elites. These are context
  features too.

## Measured on the recorded clears

`focalpoint analyze difficulty` compares each player's kill rate in an
activity at every step of its ladder with their own clears of the same
activity at Master. Comparing players with themselves removes the selection
of stronger players into harder steps:

| Step | Fireteam Ops | Pinnacle Ops |
|---|---|---|
| Normal | ×1.40 | ×1.25 |
| Advanced | ×1.26 | ×1.17 |
| Expert | ×1.17 | ×1.10 |
| Master | ×1.00 | ×1.00 |
| Grandmaster | ×0.97 | ×0.95 |
| Ultimate | ×0.89 | ×0.89 |

From Normal to Ultimate the same players kill ×0.64 (Fireteam Ops) and ×0.71
(Pinnacle Ops) as fast. Enemies needing ×2.24 the damage (−30) slows kills
by less than that: fights are not all shooting, and minors scale less. ×6.97
(−50) would not fit.

The kill rate is the measure the evaluator can trust. It is what durability
does to a build's throughput in that context.

## Gaps

- **Health after Edge of Fate.** The archetype tables predate it. Health
  tiers per Portal step and per Conquest are assumed (tier 4).
- **Conquests' deltas.** No source gives them. Each Conquest is played at a
  single step, so the same-player comparison cannot place them.
- **Feats.** Cutthroat Combat ("all combatants are tougher") has no
  published magnitude.
- **Rank-dependent scaling.** Minors' curve differs from majors' and
  bosses' by an unknown amount.
- **Enemy composition.** How many of each enemy each activity fields is
  not yet recorded. It can go in `data/contexts.toml` as
  `enemies.<type>.count`.
