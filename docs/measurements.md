# Measurements wanted

The build model ([build-model.md](build-model.md), [sandbox.md](sandbox.md))
needs some numbers no source gives. Each one is a **variable** in
[`data/sandbox/variables.toml`](../data/sandbox/variables.toml). A variable
has:

- a best guess, which the model runs on;
- a plausible range;
- the basis of the guess;
- how to measure it in-game.

```sh
focalpoint sandbox unknowns              # every variable: guess, range, status, what reads it
focalpoint sandbox unknowns --measure    # with the basis and how to measure each
focalpoint sandbox analyze ... --sensitivity                          # a result at each variable's low and high
focalpoint sandbox analyze ... --set howl_of_the_storm.freeze_radius=4   # a result with a value of your own
```

The catalog grows as more parts are modelled; `unknowns` is the current
list. When a value is measured, change its entry in `variables.toml`:

- set `guess` to the measured value;
- narrow `low` and `high` to the measurement's precision;
- set `status = "measured"`;
- put the date and the method in `basis`.

## What matters most

`--sensitivity` shows how much a result moves across each variable's range.
For the shard loop (Howl of the Storm, Shiver Strike, Whisper of Hunger,
Ultimate Conquest) the gain moves the most with:

| Variable | Gain across its range |
|---|---|
| `economy.documented_chunk_scale` | 1.01 – 3.15 |
| `scenario.pack_size` | 0.88 – 1.78 |
| `policy.shard_pickup` | 0.84 – 1.68 |
| `scenario.pack_radius` | 1.60 – 0.92 |
| `scenario.elite_share` | 1.06 – 1.59 |

So the order of work is:

1. The economy's readings: two short timed tests.
2. The scenario priors: from recordings, or calibrated on the recorded
   clears (stage 3 of the build model).
3. The radii and parts' numbers, a Shooting Range session.

## Sessions

### Economy (any activity with enemies; a few minutes each)

These settle how energy works under Armor 3.0. Every energy number in the
model depends on them.

- **Chunk energy and stats** (`economy.documented_chunk_scale`).
  1. Equip Shiver Strike and empty the melee bar.
  2. Pick up one small Stasis Shard and note the bar's gain.
  3. Do it at 0 and at 100 Melee. The model expects 2.8% and 6.3%; 7%
     and 15.75% would mean the documented figures are already the 0-stat
     ones.
- **Regeneration bonuses** (`economy.regen_relative_to_current`). Time a
  grenade's cooldown at 0 and at 100 Grenade, each with and without
  Spark of Recharge. Compare the speed-up at each stat.

### Shooting Range

These need enemies at known distances. The range's markers give them.

- `howl_of_the_storm.freeze_radius`: slide-uppercut at a line of
  enemies; the farthest one the uppercut itself freezes.
- `shatter.radius`: freeze two enemies a known distance apart, shatter
  one, and see whether the other is damaged.
- `unravel.radius`: the farthest enemy an Unravel's threads reach.
- `stasis_shard.pickup_radius`: the distance at which a shard is
  collected.
- `abilities.boss_scalar`: the same ability's damage on a boss and on a
  minor.
- `dawn_chorus.scorch_tick_energy`: melee energy per Scorch tick.
- `tectonic_harvest.progress_per_kill` and
  `tectonic_harvest.damage_taken_energy`.

- Parts' unstated numbers: `electrified_snare.radius`,
  `lightning_surge.bolt_damage`, `winters_guile.melee_bonus_x5` (punch
  at x0 and x5 Warlord's Sigil), `getaway_artist.arc_soul_rounds` and
  `.arc_soul_bursts` (count an Arc Soul's fire over its 20 s).

### From recordings

The player's habits and the shape of packs (`policy.*`, `scenario.*`):

- shards picked up out of shards made;
- frozen enemies shattered;
- typical pack sizes, spreads and elite counts per activity;
- streaks: Siphon Orbs per weapon kill (`policy.rapid_kills`),
  elemental pickups or Dielectric Orbs per kill (`policy.kill_streaks`);
- incoming damage by element and from area attacks
  (`scenario.incoming_share.*`), which the resistance mods cut;
- Icicles a Rime-coat Raiment turret fires
  (`rime_coat_raiment.icicles`), enemies a Deadfall tethers
  (`deadfall.tethered`), time spent blocking with Stronghold
  (`policy.sword_blocking`).

### Fitted to the recorded kits

`analyze model --fit` fits some values to the recorded clears
([analysis.md](analysis.md)); their entries in `variables.toml` say
how, with the scan behind each:

- `scenario.ability_efficiency`: matched to the kits' mean share of kills
  by abilities, per context (0.079 hard, 0.048 easy).
- `scenario.direct_hit_efficiency` (0.25) and
  `scenario.engagements_per_enemy` (20): the values that best order the
  kits' recorded shares.

A fitted value is not a measurement. It absorbs what the model gets
wrong elsewhere, above all loops' cast rates. Measuring the guesses it
leans on lets the fit mean what it says:

- `scenario.damage_spread`: the share of one enemy type an ability of
  known damage kills in one hit;
- `scenario.chip_kill_credit`: kills credited to abilities in a build
  whose abilities rarely kill outright, against their damage;
- `policy.min_cast_interval`: casts a minute of a build that never runs
  out of an ability's energy.
