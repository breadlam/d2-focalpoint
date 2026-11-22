# Synergistic escalation

Some builds are more than the sum of their parts: their parts form a
**loop** that feeds itself — an ignition that ignites its neighbours, a
melee that refunds itself through the shards it makes, a sword that earns
its own ammo. A loop runs only past a **threshold**: in the build (enough
Scorch per ignition to ignite; enough energy per shard to pay for the next
melee) and in the context (enemies that survive long enough, close enough).
Past it, the build's effectiveness escalates; short of it, the parts are
ordinary. The synergy evaluator has to find these loops and their
thresholds — in recorded builds now, and among candidate builds when
synthesizing them ([contexts.md](contexts.md), [effects.md](effects.md)).

This page collects the loops found so far: their mechanics, from the
community's sources ([mechanics.md](mechanics.md)), and the evidence in the
recorded clears, from `focalpoint analyze escalation`.

## Finding them in the data

A part that closes a loop lifts its players where the loop runs and not
elsewhere, so its lift **differs by context**. `focalpoint analyze
escalation` measures each scored clear against its player's usual level
(the mean of their other scored clears) and, per subclass and part
(artifact perks, fragments, aspects, exotics and their perks, set
bonuses), compares clears with the part against clears of the same
subclass without it — in **hard** content (Conquests, where enemies are
hard to kill), **easy** content (Ops, Equilibrium, dungeons) and **boss**
content (raids, Pantheon, The Desert Perpetual) — ranking parts by hard
minus easy lift. `--anchor PART --parts A,B,C` then splits the builds with
one part by which of the others they run.

This finds candidates, not proof: parts that travel together share their
lift, and the contexts are coarse.

## Loops that thrive in hard content

### Solar ignition chains

*Warlock (Dawn Chorus); Titan (Stronghold, Praxic Blade).*

- **Scorch** ignites at x100 stacks; an **Ignition** deals 676 damage over
  8 m, removes the Scorch and blocks new Scorch for 1.6 s.
- **Ember of Char**: an ignition puts x40 Scorch (x60 with **Ember of
  Ashes**) on every *other* enemy it damages.
- **Solar Fulmination** (artifact): each ignition adds a 171-damage hit over
  12 m that counts as an ignition and, with Char, another x40+20 Scorch.
- With Char, Ashes and Fulmination every neighbour of an ignition takes
  60 + 60 = **120 Scorch ≥ 100**: it ignites in turn — a chain. With Char
  and Ashes alone it is 60, short of the threshold.
- **Dawn Chorus**: ignitions deal 100% more damage, and every enemy an
  ignition damages grants 40% melee energy (doubled with Fulmination) — so
  the chain pays for the melee that starts the next one.

The chain needs neighbours within 8–12 m that **survive** the ignitions: in
easy content they die first. Dawn Chorus builds against their players'
usual level:

| Runs | Hard | Easy |
|---|---|---|
| Char + Ashes + Fulmination | **+13%** (1,088 clears) | +4% (517) |
| Char + Ashes, no Fulmination | −13% (100) | +9% (314) |
| none of them | −19% (22) | +12% (87) |

Across Solar Warlocks: Solar Fulmination +26% in hard content, +1% in
easy, −10% against bosses; Ember of Char and Ember of Ashes about +21 points
hard over easy. The same chain on Solar Titans: Ember of Ashes +23.8
points, Ember of Char +21.7, Solar Fulmination +18.3.

### Stasis shard loop

*Titan, Glacial Quake.*

- **Howl of the Storm**: a sliding Shiver Strike freezes enemies and creates
  8 Stasis Crystals (2 small, 4 medium, 2 large).
- Shattering crystals and frozen or slowed enemies spawns **Stasis Shards**;
  **Whisper of Conduction** pulls them in from 15 m.
- A shard grants 10% melee energy (small) or 50% (large); **Whisper of
  Hunger** raises that to **17.5% and 75%**: eight small shards return
  **140%** of a melee charge — a loop gain above 1 (80% without Hunger).
  The model's own figures, with Shiver Strike's chunk scalar, the stat
  and the shards of frozen enemies, are in [sandbox.md](sandbox.md).
- **Crystalline Converter** (artifact): every 8 | 12 | 15 shard pickups,
  the next Stasis melee spawns 1 | 2 | 3 more crystals — shards make
  crystals that make shards.
- Survival rides along: **Tectonic Harvest** turns shards into health and
  Frost Armor (x7 and 50% damage resistance with **Whisper of Rime**);
  **Curative Orbs** heals 40 HP per Orb or elemental pickup (shards are
  pickups). Super: **Hands-On** (super energy on powered melee kills, up to
  8% from the toughest), Glacial Quake's Orbs on kills, and three melee
  charges when it ends — which restart the loop.
- **Synthoceps** (+165% melee damage) wants three enemies within 15 m.

Glacial Quake Titans with Tectonic Harvest, against their players' usual
level, in hard content:

| Runs | Hard |
|---|---|
| Hunger + Converter | **+8%** (5,593 clears) |
| Converter, no Hunger | +2% (921) |
| Hunger, no Converter | −6% (461) |
| neither | −11% (196) |

In easy content every variant does well (+7% to +15%); against bosses the
loop build does poorly (−15%). Hail the Storm (crystal shatters release
homing shards that Slow) lifts Stasis Titans +18 points hard over easy.

### Sword engine

*Titan (Strand, Solar), Praxic Blade with Stronghold.*

- **Stronghold**: each blocked hit grants x2 Perfect Guard (to x100); sword
  damage with Perfect Guard grants Restoration x2 for up to 24 seconds.
- **Praxic Blade** (kinetic exotic sword): its combo applies the
  subclass's effect (Scorch, Weaken, Slow, Sever, Bolt Charge); guarding
  while taking damage fires Blaster Bolts back.
- **Sword Storm Combo** (artifact): three light attacks then a heavy grant
  5 seconds of automatic damage pulses around the user, **refreshed by
  sword kills**.
- **Combination Argent Blade** (artifact): a timed second swing grants +15%
  sword damage and +100 charge rate.
- **Kinetic Synthesis** (artifact): kinetic weapon damage dealt fills a
  counter — 3,770 damage, counted *before* difficulty scaling — that spawns
  an ammo brick refilling weapons. Tougher enemies absorb more damage
  before dying, so the sword earns more of its own ammo exactly where
  content is hard.
- **Balestra** (set bonus): sword, glaive and melee hits inflict Exhaustion:
  −25% combatant damage output.

Hard over easy lift: Combination Argent Blade +26.3 points, Sword Storm
Combo +23.9, Kinetic Synthesis +23.8 (Strand Titans); Praxic Blade +25.6,
Stronghold +25.3, Balestra +22.0 (Solar Titans).

### Orb and elemental-buff engine

*Warlock (Prismatic), Atheon's Memory set; Hunter (Prismatic), Facet of
Purpose.*

- **Collective Power** (4-piece): every 5 damage instances while any
  elemental buff is active spawns an Orb of Power (at most one per 4–5 s).
- **Facet of Purpose**: Orbs grant the super's elemental buff (Restoration,
  an Overshield, Frost Armor, Woven Mail, Bolt Charge) — which keeps
  Collective Power running.
- **Radiolaria Breach** (2-piece): an Orb arms an explosion and damaging
  pools for when the user reaches critical health.

Hard over easy lift: Collective Power +21.9 points, Radiolaria Breach +20.7
(Prismatic Warlocks); Facet of Purpose +21.2 (Prismatic Hunters, also +9%
against bosses).

### Precision and Armor Charge engine

*Hunter (Solar).* **On Your Mark**: precision kills and hits and class
ability use grant stacks (to allies within 15 m too) of weapon stat and
reload, up to x10; **Repurposed Charge** (set bonus): gaining, losing or
spending Armor Charge restores health and grants stacks of damage
resistance. Hard over easy lift: On Your Mark +22.1 points, Repurposed
Charge +16.8.

## Parts that flounder in hard content

The other end of the scan: parts that lift their players in easy content
and fall behind in hard content — kill-volume engines among them, which
need enemies that die quickly and often. Hard minus easy lift:

| Subclass | Part | Hard | Easy |
|---|---|---|---|
| Warlock Solar | Fever and Chill | −26% | −0% |
| Titan Solar | Ember of Torches | −20% | +5% |
| Hunter Prismatic | Fortune's Favor (Kismet) | −16% | +5% |
| Warlock Void | Echo of Starvation | −12% | +10% |
| Hunter Prismatic | Horde Shuttle | −17% | +3% |
| Titan Arc | Photonic Flare | −14% | +6% |
| Warlock Void | Echo of Harvest | −8% | +13% |
| Hunter Prismatic | To Shreds, Unraveling Orbs | −16%, −17% | +2%, +1% |

Their mechanics are yet to be read in the same detail.

## What the evaluator needs

- **Thresholds in the build**: the piece that pushes a loop's gain past 1
  — Solar Fulmination (Scorch per ignition 60 → 120), Whisper of Hunger
  with Crystalline Converter (energy per melee 80% → 140%, plus crystal
  regeneration). These are visible in the data as the step between "almost
  the loop" and "the loop".
- **Thresholds in the context**: enemy **durability**
  ([durability.md](durability.md): enemies need ×3.5 the damage in
  Grandmaster Conquests and ×7 in Ultimate Conquest, against ×2.2 in Ops at
  Ultimate) cuts both ways — it
  sustains loops that need survivors (ignition chains, damage-counted ammo)
  and starves kill-volume engines; enemy **density and spacing** (neighbours
  within 8–15 m) decide whether a chain propagates.
- **One model, not kernels** ([build-model.md](build-model.md)): the loops
  below fall out of a single model of build mechanics. Kernels, each a
  quantitative model built from the sources' numbers:
  Scorch and Ignition; Stasis crystals, shards and Frost Armor; sword ammo,
  guard and Restoration; the Orb economy and elemental buffs; Armor Charge.
