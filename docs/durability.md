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
| −40 | ×0.288 | ×3.47 |
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
| `enemy_observed`, view `enemy_observed_tier` | [`data/enemy_health.toml`](../data/enemy_health.toml): in-game measurements | health measured in-game, compared with MossyMax's tiers and the Compendium's Champion health |
| `difficulty`, `difficulty_modifier` | [`data/difficulty.toml`](../data/difficulty.toml) | Power cap, combatant Power and delta by context and step; what each Feat adds |
| view `run_difficulty` | the above with each run's step and skulls | every run's Power cap, combatant Power, delta and durability |

## Each context's difficulty

[`data/difficulty.toml`](../data/difficulty.toml) gives activity groups and
activities a Power delta (or the Power cap and combatant Power it is the
difference of) at the difficulty steps it lists, health tier and Champion
column, with a **confidence** and its sources:

- **documented**: a current source states it;
- **dated**: from before Edge of Fate, or superseded;
- **assumed**: inferred.

The import adds them as features of the context and its descendants
([contexts.md](contexts.md)): `power_delta`, `damage_multiplier`,
`durability`, `power_cap`, `combatant_power`, `health_tier`,
`champion_difficulty`, `difficulty_confidence` and `difficulty_evidence`.
An activity's entry overrides its group's, and a context takes only the
entry for the step most of its clears are at.

| Group | Delta | Durability | Confidence |
|---|---|---|---|
| Ultimate Conquest | −50 | ×6.97 | documented |
| Grandmaster Conquest, Heliostat | −40 | ×3.47 | documented |
| Ultimate Fireteam Op, Ultimate Pinnacle Op | −30 | ×2.24 | documented |
| Master Raid, Master Dungeon | −30 | ×2.24 | documented |
| The Desert Perpetual, Equilibrium, Pantheon | 0; −30 with Cutthroat Combat | ×1.00; ×2.24 | documented |

All are in-game values. Health tiers and Champion columns are assumed.

**Runs.** The view `run_difficulty` gives each run the delta of its own step
and skulls. Portal steps: Normal 0, Advanced −10, Expert −15, Master −20,
Grandmaster −25, Ultimate −30. In The Desert Perpetual, Equilibrium and
Pantheon (350/350 without Feats):

- each Feat raises the Power cap and the combatants' Power by 20, so the
  delta stays 0;
- Cutthroat Combat adds +20/+50 (Equilibrium +30/+60), making the delta −30;
- Pantheon's Adventure step is 400/300, players 100 over.

Each Conquest is played at its own step of one ladder. In-game Power cap
and combatant Power:

| Step | Conquest | Cap / combatants | Delta |
|---|---|---|---|
| 0–1 | Expert: Sunless Cell, Moon | 200/220, 250/270 | −20 |
| 2–3 | Master: Conductor's Keep, Derealize | 300/330, 350/380 | −30 |
| 4–9 | Grandmaster: Defiant EDZ, Arms Dealer, Disgraced, Heist Mars, Scarlet Keep, Heliostat | 400/440, 420/460, 460/500, 470/510, 490/530, 510/550 | −40 |
| 10–11 | Ultimate: Lightblade, Operation Seraph's Shield | 530/580, 550/600 | −50 |

The ladder's activity level is the combatants' Power / 10 at every step.
Searchlight targeted only the Grandmaster and Ultimate ones.

What is known:

- **Edge of Fate** (9.0.0.1 patch notes, July 2025) launched the Portal's
  steps at Normal 0 … Ultimate −50, since eased to Ultimate −30. It also
  made **red-bar enemies gain health more slowly** with delta than majors
  and bosses ("kill-triggered perks will be more useful in high-end
  content"). `durability` applies one curve to every rank, so it overstates
  minors' durability; the patch notes give no numbers.
- **The Data Compendium** (current) gives Ultimate −30 and the Grandmaster
  Alert −50.
- **The recorded clears** agree (below).
- **The manifest's difficulty ladders** give each step an activity level:
  Portal Normal 31, Advanced 33, Expert 35, Master 40, Grandmaster 45,
  Ultimate 50. For Conquests it is the combatants' Power / 10. This is the
  `activity_level` context feature.
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

`--skull NAME` compares clears with a modifier against the same players'
clears without it, in the same activity at the same step. Cutthroat
Combat (−30):

| Group | Kill rate | Clear length |
|---|---|---|
| The Desert Perpetual | ×0.66 | ×1.01 |
| Pantheon | ×0.54 | — |
| Equilibrium | ×0.96 | ×1.86 |

Equilibrium's waves cap the kill rate, so its clears take longer instead.

The kill rate is the measure the evaluator can trust. It is what durability
does to a build's throughput in that context.

## Measured in-game

The Shooting Range at Ultimate (−30). Health is the damage to kill in the
game's damage numbers:

| Enemy | Health | Closest pre–Edge of Fate value | Ratio |
|---|---|---|---|
| Psion | ~125 | minor, tier 2: 123 | 1.02 |
| Acolyte | ~215 | minor, tier 2: 202 | 1.06 |
| Honored Legionary | ~525 | elite, tier 2: 550 | 0.95 |
| Honored Phalanx | ~550 | elite, tier 2: 550 | 1.00 |
| Revenant Knight | ~1,470 | elite, tier 2: 1,538 | 0.96 |
| Esteemed Centurion | ~1,650 | elite, tier 2: 911 | 1.81 |
| Blistered Wizard (orange bar) | ~2,250 | elite, tier 2: 911 | 2.47 |
| Unstoppable Ogre (Champion) | ~3,400 | Compendium, Raid (Expert/Master): 3,300 | 1.03 |

Psions, Acolytes, Legionaries, Phalanxes and Knights match MossyMax's
**health tier 2** (matchmade strikes) within 6%. Centurions and Wizards, one
health class before Edge of Fate, measure 1.8× and 2.5× above it. The
Wizard's orange bar is the tables' elite rank, so the class was rebalanced
(Edge of Fate: majors gain health faster than red bars) unless the tables'
elites were a different rank. Only the Wizard's bar is confirmed; the other
ranks are the closest match. Damage numbers already carry the Power delta, so if health
pools are fixed, the range's Ultimate pools are tier 2's. If Edge of Fate
instead grows pools with combatant Power, the same enemies at Normal will
show smaller totals, and minors will shrink less than elites. Measuring them
there will tell which.

## Gaps

- **Health after Edge of Fate.** The archetype tables predate it, and the
  health tiers of current content are assumed (the range at Ultimate
  matches tier 2, above). Whether health pools grow with combatant Power
  is unknown until the same enemies are measured at a second delta.
- **Rank-dependent scaling.** Minors' curve differs from majors' and
  bosses' by an unknown amount.
- **Enemy composition.** How many of each enemy each activity fields is
  not yet recorded. It can go in `data/contexts.toml` as
  `enemies.<type>.count`.
