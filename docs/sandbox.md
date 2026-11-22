# The sandbox: vocabulary and rule base

The build model ([build-model.md](build-model.md)) describes what build
parts do as **records over a closed vocabulary**. `data/sandbox/` holds two
kinds of file, both built into the binary:

- **`ontology.toml`**, the vocabulary:
  - value lists: elements, ranks, damage categories, damage tags, stats…;
  - the **events** that rules fire on, with each event's attributes;
  - the **states**, with the core verbs' parameters and their sources.
- **`rules/*.toml`**, the **rule base**: rules, modifiers, counters and
  gaps;
- **`variables.toml`**, the **variables**: values no source gives, each
  with a guess, a range and how to measure it ([measurements.md](measurements.md)).

```sh
focalpoint sandbox check                      # check the rule base; coverage, gaps and the drafting worklist
focalpoint sandbox check --dir data/sandbox   # while editing, without rebuilding
focalpoint sandbox analyze --part 'aspect:Howl of the Storm' --part 'melee:Shiver Strike' \
    --part 'fragment:Whisper of Hunger' --context 'Ultimate Conquest'   # a build's loops
```

`check` fails on any name the vocabulary does not define. With a database,
it also:

- resolves every part the records name against the manifest;
- reports, by kind, how much of the recorded builds' part use is modelled;
- lists the most-used parts that are not modelled yet.

## Records, not parts

The unit of knowledge is the record, not the part. Each record says what
**grants** it: one part, or several parts together. A part's behaviour is
whatever it grants, alone or in combination.

Much behaviour belongs to no single part:

- Dawn Chorus makes Daybreak's projectiles ignite;
- set bonuses need a number of pieces;
- with Ember of Ashes, Caliban's Hand applies its own amount of Scorch.

A description built around parts would have to file such behaviour under
one of the parts arbitrarily, or carry special cases for it. A part's
behaviour also comes from more than one place:

- the manifest (stats, energy costs);
- the records written here;
- later, activity modifiers, which grant records too.

Building one rule base from all of these lets each source stay separate.

Compiling a build is then a selection: keep the records its loadout
grants. Conditions on the loadout are settled there. The solvers see only
conditions on the moment, such as stacks held, health, or the target hit.

Path of Building's modifier database works the same way: every modifier
carries its source. Bungie's manifest is similar too: items carry sandbox
perks rather than behaviour.

## States

| `kind` | What | Examples |
|---|---|---|
| `resource` | the player's: ability and super energy, ammo, health, Armor Charge, Transcendence | `melee_energy` |
| `buff` | on the player or allies | `radiant`, `frost_armor`, `woven_mail` |
| `debuff` | on enemies | `scorch`, `jolt`, `volatile`, `slow`, `frozen`, `unravel` |
| `object` | in the world: picked up, destroyed or seeking | `orb_of_power`, `stasis_crystal`, `stasis_shard`, `firesprite`, `threadling` |
| `verb` | an instantaneous mechanic | `ignition`, `shatter`, `cure` |

A state with `max_stacks` and `at_max` fires that event when it fills:

- Scorch at x100 fires `ignition`;
- Slow at x100 fires `freeze`.

Parameters are addressed as `<state>.<param>` (`ignition.damage`, 676) or
`<ability>.<param>` (`melee.charges`).

## Grants and provenance

Every rule and modifier has these fields:

| Field | |
|---|---|
| `granted_by` | a part, `"kind:name"`; or a list of parts, all of which must be equipped: `["armor:Dawn Chorus", "super:Daybreak"]` |
| `sources` | at least one, `family:reference`; the families are in the vocabulary |
| `confidence` | `documented` (default), `dated` or `assumed`, as in `data/difficulty.toml` |
| `note` | optional |

A part names every manifest item of that kind and name, so reissues share
it. A part counts as **modelled** once it grants some record alone. A part
named only in combinations, like Daybreak above, is not modelled by them.

```toml
[[rule]]
granted_by = "fragment:Ember of Char"
sources = ["compendium:Solar/Fragments/Char", "clarity:362132291"]
on = { event = "ignition_hit", origin = false }
effect = { apply = "scorch", stacks = 40, target = "target" }
```

## Rules

`on` an event, `when` every condition holds, do each `effect`.

- **`on`**: an event name, or a table of the event and attribute values:
  `{ event = "kill", source = "weapon", element = "solar" }`.
- **`when`**: conditions on the moment, each setting one of:
  - `has` / `lacks` a buff;
  - `stacks` (with `min`), of a state or a counter;
  - `movement`;
  - `in_super`;
  - `health_below`;
  - `enemies_within` (with `min`);
  - `target_has`;
  - `target_rank` / `target_rank_not`.

  What is equipped is not a condition: it belongs in `granted_by`.
- **`effect`**: one or a list. Each sets exactly one primary field.
- **Limits:**
  - `cooldown`: seconds between firings;
  - `chance`;
  - `limit`: times per activation, such as per super cast.

| Primary field | Effect | Qualifiers |
|---|---|---|
| `apply` | apply a buff or debuff | `stacks`, `duration`, `max_duration`, `target`, `radius`, `except` |
| `damage` | deal damage | `radius`, `falloff` (fraction at the edge), `element`, `tags`, `source` (what it counts as) |
| `energy` | flat ability energy, a fraction of a charge, scaled by the ability's flat-gain factor | `amount` |
| `regen` | multiply an ability's regeneration | `amount`, `duration`, `max_duration` |
| `spawn` | create objects | `count`, `size` or `sizes`, `tags`, `source` |
| `heal` | restore health | |
| `emit` | fire an event, for a verb such as an ignition | `attributes`; `damage`, `radius` override the verb's own |
| `charges` | grant ability charges | `count` |
| `consume` | remove a state, or reset a counter | |
| `progress` | advance a counter | `amount` |

**Amounts** (`damage`, `heal`, `amount`, `stacks`, `count`) are numbers or
tables:

- `{ by_tier = [...] }`: by combatant tier, from T1;
- `{ by_rank = { minor = ..., elite = ... } }`;
- `{ by_counter = [[value, amount], ...], counter = "..." }`: the highest
  row reached;
- `{ by_count = [...] }`: by copies of the granting part equipped;
- `{ var = "howl_of_the_storm.freeze_radius" }`: a variable's value.

Radii, durations, chances, cooldowns and modifiers' values are amounts
too, so any of them can be a variable.

## Modifiers

A `[[modifier]]` changes exactly one of the following, either passively or
`when` conditions hold:

- **a parameter**: `param = "scorch.applied_multiplier"`;
- **damage carrying a tag**: `damage = "ignition"`, optionally of one
  `element`, with Court's `category` deciding how it stacks;
- **a stat**: `stat = "handling"`.

It does so by exactly one of `add`, `mul` or `set`. Add `round = "up"`
where the source says so.

## Counters

A `[[counter]]` declares a named counter (`name`, `threshold`, `note`).
Rules can:

- `progress` it and `consume` it;
- read it in `stacks` conditions and `by_counter` amounts;
- fire on its `counter_full`.

Names are global, so give each a prefix for its part: `converter_shards`.
Every counter must have a rule that progresses it.

## Gaps

A `[[gap]]` records behaviour of a `part` that is known but not yet
modelled, in a `note`, with `sources` where they apply. A modelled part
with a gap is reported as partly modelled. `check` lists every gap, so the
missing behaviour stays visible.

## Writing records

- Take numbers from these sources, and cite each:
  - the Data Compendium;
  - Court's guides;
  - the cooldown workbook;
  - MossyMax's sheets;
  - Endgame Analysis;
  - Bungie's notes;
  - in-game measurement;
  - Clarity.
- Group records in files by subject (`solar.toml`, `exotic_armor.toml`).
  A file is only a container: a part's records can be split across files,
  and a combination's records go where they read best.
- Where sources disagree, use the better one and say so in the `note`. For
  example, Whisper of Hunger: the Compendium has +60%, Clarity has 17.5%
  and 75%.
- Where no source gives a number, add a variable to `variables.toml` and
  reference it: `{ var = "part_name.quantity" }`. The variable needs:
  - a best guess and a range;
  - the basis of the guess;
  - how to measure it.

  Mark the record `assumed`. Never put an invented number in as
  documented. Record a gap only for behaviour the vocabulary cannot
  express yet.
- Write what the game does, not what it adds up to. Dawn Chorus has no rule
  for the Compendium's "Fulmination doubles the melee energy". A
  Fulmination counts as an ignition, so each enemy it damages fires the
  ignition rule again, and the doubling follows.
- Don't re-implement a core verb. Apply it or modify its parameters.

## Analysing a build

`focalpoint sandbox analyze` compiles a build and follows what it does
in a scenario (`docs/build-model.md` §4–5). It needs the economy and
contexts from `focalpoint mechanics import`.

- **Compiling.** The loadout's granted records. Every passive
  modifier's parameter is resolved once: the largest `set`, then each
  `add`, then each `mul`. Ember of Ashes' ×1.5 Scorch is rounded up.
- **The scenario.** A pack of enemies spread over a disc, which gives
  the expected neighbours within any radius. Elites make up a share of
  the pack. Health is the context's health tier times its durability
  ([durability.md](durability.md)). Stats default to 100. Pack size,
  spread and elite share are priors until calibrated: `--pack`,
  `--spread`, `--elites`, `--stat melee=70`.
- **The energy economy** ([mechanics.md](mechanics.md)). A documented
  chunk of energy is the pre-Edge of Fate figure. It arrives multiplied
  by the ability's chunk scalar and by 0.40 to 0.90 for the stat
  (`scaling = "stat"` or `"fixed"` on an energy effect skips the
  scalar, or both).
- **Generations.** The analysis follows events, on average, from a
  *boundary* to the next ones. A boundary is an ability cast (energy
  comes back as casts), or an event a state set off by filling (an
  ignition by Scorch at x100). The expected boundaries each boundary
  reaches form a matrix. Its spectral radius, per loop, is the loop's
  **gain**: above 1 the loop feeds itself. `analyze` also finds the
  pack size, durability and stat at which each loop crosses 1.
- **Core verbs, written once.**
  - An ignition hits its target and every neighbour within its radius.
    An ignition and what it sets off in the same place (a Fulmination)
    form one burst. A neighbour ignites in turn when the burst's Scorch
    on it reaches x100 and it survives the burst's damage.
  - Stasis crystals freeze enemies within 2.6 m and are shattered.
    Frozen enemies are shattered; the overlapping freezes of a
    generation cover a share of the pack.
  - Every shatter leaves a Stasis Shard (a large one from an elite). A
    picked-up shard grants melee energy.
- **What it takes without data.** Every value no source gives is a
  variable ([measurements.md](measurements.md)): the player's policy
  (`policy.*`), the scenario's priors (`scenario.*`), the economy's
  readings (`economy.*`) and parts' unknown numbers.
  - `analyze` lists the variables a result reads.
  - `--set name=value` changes one for a run.
  - `--sensitivity` shows each loop's gain at each variable's low and
    high.
  - Conditions it reads as always or never true are listed under "Taken
    without data".

- **Kills in a generation.**
  - The damage an enemy actually takes spreads log-normally around the
    documented amount (`scenario.damage_spread`), so the share killed
    moves smoothly with damage instead of stepping at each health value.
  - Area damage counts by `scenario.ability_efficiency`, single-target
    hits by `scenario.direct_hit_efficiency`; both are fitted to the
    recorded kits ([analysis.md](analysis.md)).
  - An effect that hits one target many times (an Arc Soul's 33 rounds)
    spreads over at most the pack. Its damage adds up to kills, and what
    its hits apply reaches distinct enemies that many times fewer.
  - Jolt chains, Volatile explosions, Unravel threads and freezes are
    capped at the pack's enemies not yet affected in the generation.
- **Orbs and Armor Charge** are core records: every build has them.
  Orbs from abilities are large (`orb_of_power.super_energy`), those from
  armor mods small (1.25%). Each Orb grants a stack of Armor Charge,
  which Font mods turn into stat while it is up; stat modifiers feed the
  cooldowns and the energy scaling.

**Rates** put time in: the steady state per minute in the scenario.
- **Casts** come from passive regeneration: the economy's cooldowns at
  the build's stats (Font bonuses by Armor Charge's uptime), plus
  regeneration bonuses by their uptime. They also come from the energy
  the build returns. Abilities the cooldown workbook lacks take the
  Compendium's base cooldown and chunk scalar
  (`economy.unknown_chunk_scalar` where it gives none).
- **Kills:** the scenario supplies the context's recorded kills a minute.
  Abilities and verbs kill some of them:
  - outright, as the generations count;
  - by **chip damage**: damage a generation deals beyond what its kills
    used still wears enemies down, and the final blow falls by damage
    share. It counts as kills at damage over mean health, times
    `scenario.chip_kill_credit`.

  Weapons take the rest, and weapon kills fire the rules that need them.
- **Enemy availability:** an enemy meets at most
  `scenario.engagements_per_enemy` bursts before it dies. Casts beyond
  what the supply allows find no one fresh, and what they would do to
  enemies scales down.
- **Damage, healing and buff uptimes:** each buff's mean stacks come from
  its applications times its duration, up to its maximum.
- **Caps:** a self-feeding loop is capped at one cast per
  `policy.min_cast_interval`, and chained events and kills at the enemies
  supplied. `analyze` reports which caps were hit.

The acceptance cases ([escalation.md](escalation.md)) come out of the
records, with no rule written for them. Default pack, stats at 100, Howl of
the Storm's freeze radius 3 m (the tests pin the variables they read, with
stats at 70):

| Loop | Build | Gain | Crosses 1 |
|---|---|---|---|
| Ignition chain | Char, Ashes, Fulmination, Dawn Chorus; Ultimate Conquest | 2.04 | runs above durability 2.4 and in packs of more than 4.4 |
| | the same, Grandmaster Conquest | 1.65 | |
| | the same, durability 1 | 0.46 | neighbours die first |
| | without Fulmination | 0 | 60 Scorch a neighbour, short of 100 |
| Shard loop | Howl, Shiver Strike, Conduction | 1.00 | at 8 enemies a pack |
| | + Whisper of Hunger | 1.60 | at melee stat 38.6 |
| | + Crystalline Converter | 1.18 | at melee stat 64.8 |
| | + both | 1.89 | at melee stat 25.0 |

The chain's durability threshold sits between Master content (×2.24) and
Grandmaster Conquests (×3.47), as the recorded clears suggest.

In the shard loop, Shiver Strike's chunk scalar (0.7) and the stat (0.9 at
100) cut each shard's energy below a naive count's. The shards from frozen
enemies, large ones from elites, make up the difference. Without Hunger
or Converter the loop sits right at 1 in packs of eight. How far it gets
past that depends on the variables, above all
`economy.documented_chunk_scale` ([measurements.md](measurements.md)).

Not modelled yet:

- Weapons: their damage, and the perks that act on abilities (Voltshot,
  Incandescent, Demolitionist and the like).
- A loadout's copies of an armor mod: one copy's amounts are taken.
- Kills of slowed (not frozen) enemies leaving Stasis Shards.
- Shield breaks and Champion stuns as events.
- Exotic class items' spirit perks.
- Super durations: a super is one cast, its effects at once.

## Simulating a build

`focalpoint sandbox simulate` runs the same records as a seeded Monte
Carlo (`docs/build-model.md` §5). Where the analysis takes expected
values, the simulator draws them:

- **Enemies** stand at sampled points of each pack's disc, with health
  drawn from the scenario's archetypes. Packs arrive at the context's
  rate of kills.
- **Weapons** wear the enemies down at a steady rate: `sim.weapon_headroom`
  times the damage that kills what the scenario supplies. Ability damage
  that kills nothing outright adds up with theirs on the same health
  bars, and the final blow decides whose kill it is. The rates' chip kills
  and enemy availability come out of this instead of being assumed.
- **The player** regenerates energy by the economy and casts each ability
  when charged, at a live enemy, once per `policy.min_cast_interval` at
  most.
- **Rules** fire on concrete events. Chances, conditions, a weapon kill's
  precision and element and the player's movement are drawn. Cooldowns
  and per-activation limits are kept per rule. An effect on a target
  that has died does nothing, except repeated hits, which move on.
- **Core verbs** act on positions: ignitions hit everyone in their
  radius, crystals freeze the enemies around them, Jolt chains to the
  nearest enemies. Scorch ticks and decays after its linger.

It reports casts, kills by final blow, damage, healing and buff uptimes
beside the analysis's rates. `--offspring ignition` measures how many
ignitions each one sets off in a fresh pack, against the analysis's loop
gain:

| Dawn Chorus ignition chain (Char, Ashes, Fulmination) | Simulated | Analysis |
|---|---|---|
| Ultimate Conquest | 2.98 | 2.98 |
| Grandmaster Conquest | 2.88 | 2.98 |
| Ultimate Fireteam Op | 2.71 | 2.96 |
| Ultimate Conquest, `scenario.ability_efficiency` 1 | 1.73 | 2.12 |

Where damage is high the simulator branches less: neighbours igniting at
once hit each other and die before they go off, which the averages miss.

For the Stasis shard loop (Synthoceps Behemoth with Crystalline Converter
and Hail the Storm, Grandmaster Conquest) the simulator sustains 8.6 melee
casts a minute where the analysis reaches 20. Abilities kill 41% where
the analysis says 31%; the recorded clears say about 47%.

## Synthesis

`focalpoint sandbox synthesize --class C --element E [--context hard|easy]`
searches for the best subclass kits, first version (`docs/build-model.md`
§8).

- **Options** are what players run: each slot's modelled parts from the
  subclass's recorded builds (most used first), and each aspect pair's
  fragment slots from the builds that run it. Modelled artifact perks
  may be added. Parts the model knows nothing about are left out: they
  would score as nothing, which a search mistakes for safe.
- **Score** is the calibrated layer of `analyze model` for the context:
  the effect against a player's usual level, predicted from the model's
  metrics by a ridge regression fitted on the recorded kits. Its weights
  are non-negative, so more kill capacity, loop gain or survival never
  scores worse; a metric that would get a negative weight is dropped.
- **Search** is a beam over one-part changes from the subclass's most-used
  kit: another super, grenade, melee or class ability, aspect or fragment,
  or an artifact perk added.

On 2026-11-22, in hard content:

- **Solar Warlock:** Char, Ashes, Eruption and Solar Fulmination, with
  Hellion and Touch of Flame: the ignition chain.
- **Stasis Titan:** Howl of the Storm and Tectonic Harvest with Whispers
  of Hunger and Conduction and Crystalline Converter: the shard loop.

The data supports both: they are the combinations with the largest lifts
in hard content ([escalation.md](escalation.md)). With weapons, armor mods
and most exotic armor not modelled yet, the search is over the kit alone.
