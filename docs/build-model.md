# The build model (proposal)

**Status: accepted (2026-11-21); stage 1 in progress.** A design for modelling Destiny 2 builds as a whole,
so the synergy evaluator and build synthesis share one model. It replaces
the plan to model mechanics loop by loop ([escalation.md](escalation.md)):
the Scorch/Ignition chain, the Stasis shard loop and the rest fall out of
it instead of being modelled one at a time.

## What it has to do

- **Evaluate any build in any context.** For recorded builds and builds no
  one has played, per activity and per encounter
  ([contexts.md](contexts.md)), on the add-clear ↔ boss gradient: Aegis's boss
  rotations should score well against bosses and lower elsewhere.
- **Find escalation.** Detect loops that feed themselves, the thresholds,
  in the build and in the context, past which they run, and the margin a
  build has ([escalation.md](escalation.md)).
- **Drive synthesis.** Score thousands to millions of candidates quickly,
  say what each part contributes, and explain a build in mechanical terms.
- **Stay honest.** Every number has a source and a confidence. Unknown
  numbers are ranges, and results carry their uncertainty. The model also
  says which unknown matters most, so the next in-game measurement can be
  chosen deliberately.

## The core idea

Everything a build does is a set of **rules** of the form *on an event, if
a condition holds, do an effect*. Effects change **state** (resources,
buffs, debuffs, objects, enemies' health) and emit further **events**. A
build is the union of its parts' rules, with their numbers set by its stats.
A **context** is an environment that supplies enemies (how many, how
tough, how close together, when), time windows and a fireteam. Solving the
build's rules against a context gives rates, uptimes, damage, kills and
survival, and those give the score.

This is a stochastic reaction network, the formalism of chemical kinetics
and epidemics. In it, escalation has a precise meaning: a loop runs away
when its **gain** is above 1. That is the branching factor of a chain
reaction, or the energy a loop returns per unit spent. The **threshold** is
where the gain crosses 1.

- **Ignition chain.** An ignition reaches *k* living neighbours. Each
  ignites in turn with probability *p* (its Scorch reaches 100 and it
  survives the blast). The chain's reproduction number is *R = E[k] × p*.
  With Ember of Char and Ember of Ashes, *p* is small (60 Scorch per
  ignition). With Solar Fulmination it is near 1 (120). Whether *E[k] ≥ 1*
  depends on the context: pack density within 8 m, and whether neighbours
  survive 676 × (1 + 100% Dawn Chorus) damage at that context's
  durability. In Ultimate Conquest (×6.97), elites survive and propagate
  the chain; in Fireteam Ops they die first. That is the measured
  hard-vs-easy lift of Fulmination, derived rather than fitted.
- **Shard loop.** A Howl of the Storm melee makes 8 crystals. Eight small
  shards return 8 × 17.5% = 140% of a melee charge with Whisper of Hunger
  (80% without), so the loop gain is 1.4 against 0.8. Crystalline Converter adds
  crystals per shard, a second loop on top.
- **Kill-volume engines** have gains that rise with kill rate, so they
  fall as durability rises. That is the other end of the escalation scan.

## Architecture

```
 manifest ─┐                              recorded clears ─┐
 sheets  ──┼─▶ knowledge base ─▶ compiler ─▶ build rules ──┼─▶ evaluator ─▶ score per context
 texts   ──┘   (ontology, core     ▲          + scenario   │   (fluid + simulation,      │
               verbs, part cards)  │                       │    calibrated)              ▼
                                   └──── context profiles ─┘                        synthesis
```

### 1. Ontology: a closed vocabulary

The model only works if every part is described in the same terms. Five
registries, each small and closed:

| Registry | Contents (first cut, from the effects tagger and the sources) |
|---|---|
| **Events** | kill (by weapon, ability, super, melee, grenade, precision, of a debuffed or elite target, multikill), hit, precision hit, ability cast, super cast, damage taken, pickup (Orb, shard, Firesprite, Ionic Trace, ammo), finisher, rally, reload, sprint/slide/airborne, buff gained or lost, threshold reached |
| **States** | resources (grenade, melee, class, super energy; ammo; health and shields; Armor Charge; Transcendence; counters), buffs (Radiant, Restoration, Devour, Woven Mail, Frost Armor, Amplified, Bolt Charge, Invisibility, Speed Booster, Overshield, surges…), debuffs (Scorch, Jolt, Volatile, Weaken, Slow, Freeze, Sever, Suspend, Unravel, Blind, Suppression…), objects (Orb of Power, Stasis Crystal and Shard, Firesprite, Ionic Trace, Void Breach, Tangle, Threadling, Well…) |
| **Effects** | deal damage (amount, element, category, radius or chain, target filter), apply a state (stacks, duration), grant energy (flat %, regen multiplier for a time), spawn an object, modify (a stat, damage category, damage resistance, healing), consume, refund |
| **Conditions** | has a state or *n* stacks, health below or above, enemies within *r*, target rank, element of the source, equipped with a part (the "with Ember of Ashes" clauses) |
| **Limits** | duration, refresh or extend rules, maximum stacks, decay, internal cooldown, every *n*-th trigger, chance |

Damage modifiers carry **Court's categories** (global debuff, empowering
buff, weapon surge, amplification, exotic intrinsic, weapon perk, world).
Within the first three only the highest applies; across categories they
multiply. Targets carry **combatant ranks**: minor, elite, miniboss,
Champion, boss and vehicle, with MossyMax's per-archetype damage scalars.

### 2. Core verbs: written once, in code

About 35 subclass keywords have mechanics that many parts refer to:

- Scorch → Ignition at 100 stacks; Jolt chains; Volatile explosions.
- Weaken; Slow → Freeze → Shatter; crystal growth.
- Sever; Suspend; Unravel; Threadlings.
- Radiant; Restoration; Devour; Woven Mail; Frost Armor; Amplified;
  Bolt Charge.
- Orb generation and pickup; Armor Charge; Transcendence.

Each is implemented once in the engine, with its numbers from the
Compendium's glossaries and Clarity, and tested against them. Parts never
re-implement a verb; they refer to it ("apply 40 Scorch"). This is the
boundary that keeps the per-part work declarative. The same goes for the
systems every build uses:

- **Energy economy:** base cooldowns, flat-gain factors, regen effects, and
  the Armor 3.0 stat curves, from the cooldown workbook.
- **Damage pipeline:** weapon damage per archetype and frame, combatant
  scalars, buff categories, and the Power delta.
- **Survival:** health and shields by stat, damage resistance, healing.

### 3. Part cards: declarative, sourced, tested

The format and the checks are in [sandbox.md](sandbox.md).

Each part — aspect, fragment, ability, super, exotic and its perks, set
bonus, artifact perk, armor mod, weapon perk, origin trait — gets a
**card** in `data/sandbox/`: its rules and passive modifiers in the
ontology's terms. Every number cites its source and confidence, as in
`data/difficulty.toml`. Structure comes from the manifest, not the card:
slot, fragment capacity, energy cost, stat investments, class, element and
set membership. For example:

```toml
[[part]]
hash = 362132291                      # Ember of Char
sources = ["clarity:362132291", "compendium:Solar/Fragments"]

[[part.rule]]
on = "ignition"
target = { enemies_hit = true, except = "origin" }
effect = { apply = "scorch", stacks = 40 }
with = [{ part = "Ember of Ashes", stacks = 60 }]
confidence = "documented"
```

**Weapons are parametric, not per weapon.** Damage comes from archetype,
frame and combatant scalars (MossyMax), handling and reload from manifest
stats, and the 942 perks are cards (230 cover 90% of use). Exotic weapons
get cards of their own.

**Encoding is the main cost, so the design plans for it.**

- **Deterministic import** wherever the source is structured: manifest
  structure; the cooldown workbook's cooldowns, flat-gain factors and
  regen multipliers; Court's buff values and durations; MossyMax's weapon
  tables; the Compendium's mechanics tables.
- **Assisted drafting** for prose (Clarity, the Compendium's part texts,
  Endgame Analysis's trigger and effect columns). Cards are drafted in batches
  in working sessions and committed. Nothing is generated at run time.
- **Checks on every card:**
  - the schema;
  - the closed vocabulary: an unknown term fails;
  - the numbers in the card must reproduce the numbers in its source text;
  - Clarity and the Compendium must agree where both give a number.
- **Coverage-driven order.** Cards are written in order of recorded use,
  weighted by how much the model's rankings depend on them. A build
  containing parts without cards is still evaluated: the unmodelled parts
  are reported, and the data-driven layer below covers them.

| Parts | Distinct | 90% of uses | 99% of uses |
|---|---|---|---|
| Subclass: supers, grenades, melees, class abilities, jumps, aspects, fragments, Transcendence | 411 | 217 | 353 |
| Artifact perks | 148 | 85 | 131 |
| Set bonuses | 112 | 37 | 81 |
| Exotic and armor perks | 201 | 67 | 138 |
| Armor mods | 270 | 56 | 100 |
| Weapon perks, origin traits, weapon mods | 1,199 | 328 | 726 |

### 4. Scenarios: contexts as environments

A context profile becomes a scenario:

- **Enemies:** composition by archetype and rank, with tier-2 health ×
  the run's durability ([durability.md](durability.md)), and Champions
  by type.
- **Arrival:** waves or a stream, with time between engagements.
- **Space:** a pack model (packs of *n* enemies spread over *r* metres).
  It gives the expected number of neighbours within a radius, which every
  area effect and chain depends on.
- **Boss phases and damage windows.**
- **Modifiers:** Galvanized, banes, Cutthroat Combat, locked equipment.
- **Fireteam:** a default team that supplies typical support.

Positions are not in the data. Pack parameters start from curated priors
and are calibrated (below).

### 5. Solving: two methods on the same rules

- **Fluid (mean-field) analysis.** Event rates at steady state from a
  fixed point:
  - resource balance (energy in = regen + triggered gains; out = casts);
  - buff uptimes from trigger rates, durations and refresh rules;
  - damage and kills by source and rank.

  From the linearised rates come every loop's gain (the spectral radius
  of the trigger graph's Jacobian, and branching numbers for chains) and
  each threshold, solved for the build parameter or context parameter
  where the gain crosses 1. It takes milliseconds or less, so it is the
  synthesis inner loop.
- **Discrete-event simulation.** Seeded Monte Carlo of encounters, run in
  parallel across cores. It covers what averages miss:
  - cooldown alignment and burst inside a 30-second damage window;
  - stack thresholds;
  - internal cooldowns;
  - chains on sampled enemy positions.

  It verifies finalists and calibrates the fluid method where they
  disagree.

**Player behaviour** is a simple explicit policy. Abilities are cast when
ready on the best available targets, with per-archetype rules where a
build's loop needs them ("melee into a pack"). An execution-efficiency
factor per context is calibrated from data. Boss phases use Aegis's
rotations as templates.

### 6. Outputs and scoring

Per build and context, the evaluator reports metrics:

- **Add clear:** kills per minute by rank, and time to clear a wave.
- **Boss:** damage over each damage window.
- **Champions:** how they are handled (stun sources, damage to kill).
- **Survival:** effective health, healing per second, damage-resistance
  uptime.
- **Economy:** ability and super uptime, ammo sustainability.
- **Mechanics:** active loops with gains, thresholds and margins.

A **calibrated layer** maps metrics to performance per context. It is a
statistical model fitted on the recorded clears: what predicts a clear in
a Conquest is not what predicts one in a raid, and the data says how much
each metric matters where. Hand-set weights would only encode our
guesses. Pairwise **synergy** is the non-additive part of the score,
*v(A+B) − v(A) − v(B) + v(∅)*. Contributions are attributed to parts and
rules for explanations.

### 7. Calibration and validation

The 3 million recorded clears are what make the model trustworthy:

- **Fit context parameters** so the model, run on the recorded builds,
  reproduces each context's kill rate and kill mix: ability, super,
  melee, grenade and precision shares (method of simulated moments).
- **Acceptance cases:**
  - the Dawn Chorus threshold (Fulmination's step, hard against easy);
  - the Glacial Quake shard-loop gain;
  - the sword engine;
  - the Orb engine;
  - `analyze difficulty`'s kill-rate ratios.
- **Rank agreement** with Aegis's measured boss DPS, per boss.
- **Prediction of within-player lifts** from `analyze escalation`.
- **Held-out novelty:** predict builds whose part combinations were
  held out of calibration. Synthesis proposes builds no one has played,
  so this is the test that matters.
- **Endgame Analysis's tiers** as a weak sanity check.

### 8. Synthesis

- **Constraints from the manifest:** class and subclass, slots, aspect
  fragment capacity, mod energy, one exotic, set pieces, artifact unlocks.
  They are generated, not hand-written.
- **Generation.**
  - **Seeded by loops:** find cycles in the rule graph of *all* parts that
    can close within the constraints, rank them by attainable gain, and
    back-chain from each loop's payoff to the parts that feed it.
  - **Kit search** with the fluid evaluator: beam search or Monte Carlo
    tree search over super, abilities, aspects, fragments, exotic, set
    bonuses and artifact perks.
  - **Fill by optimisation:** stats as an integer program (as DIM's
    optimiser does), mods as a knapsack, and weapons by the states the kit
    produces and consumes.
- **Selection.**
  - **Verify** the top candidates by simulation.
  - **Calibrated scores** with uncertainty per context.
  - **A Pareto front** across add clear, boss damage and survival.
- **Explanation.** "Ignition chain, R = 1.6 in Ultimate Conquest, 0.7 in
  Fireteam Ops; runs while two or more elites within 8 m survive an
  ignition."
- **Learning, later.** A surrogate, a graph network over the build's rule
  graph, trained on evaluator outputs and recorded outcomes. It speeds up
  search and captures what the mechanics miss. A learned construction
  policy comes after that, if search proves too slow. Either way the
  evaluator is the reward, and the recorded data anchors it.

## Why not the alternatives

- **Per-part code, as SimulationCraft does.** Accurate, but thousands of
  hand-written handlers. It cannot be analysed for loops without running
  it, and does not survive this many parts.
- **Analytic modifiers only, as Path of Building does.** Fast and
  declarative, but uptimes and conditions are set by hand. Escalation
  *is* dynamics, so it needs rates and simulation.
- **Pure machine learning on the recorded clears.** It cannot extrapolate
  to unplayed combinations or explain a threshold. It is the calibration
  layer and a later surrogate, not the model.

## Order of work

The system is built breadth-first, so every element and resource system
exists from the start. Depth then follows coverage.

1. **Foundation.**
   - the ontology;
   - the card schema and compiler;
   - the scenario model;
   - the fluid solver and loop analysis, and the simulator;
   - the core verbs of all five elements and Prismatic;
   - the energy economy, damage pipeline and survival model, imported
     from the cooldown workbook, Court's guide and MossyMax's sheets;
   - manifest-derived constraints.
2. **First cards:** every subclass kit, the most-used exotics, set bonuses,
   artifact perks and mods, and weapons by archetype with the top perks.
   That is enough to evaluate the kits of most recorded builds. The
   acceptance cases pass here: the Scorch/Ignition chain falls out of the
   Solar verbs, three cards and the scenario.
3. **Calibration and validation harness** (`focalpoint analyze model`):
   fitted context parameters, acceptance cases, Aegis rank agreement,
   held-out lifts.
4. **Synthesis v1** on the fluid evaluator, verified by simulation, with
   explanations. Cards extended to 90% coverage.
5. **99% coverage, the surrogate,** and the site.

## Decisions

- **Card format: TOML** with a strict schema, like the other `data/` files.
  A compact language can replace it later; the compiler is the only reader.
- **Assisted drafting** of cards from source prose, each card checked
  against its source.
- **Weapons from the first stage**, parametric by archetype and frame, with
  perks as cards.
- **Solo first.** The model and synthesis target the best builds for one
  player in each context. Team modelling comes later: it matters most for
  boss damage, while solo builds do well everywhere else.
- **Clarity without the partnership.** The partnership's terms over-reach
  on data that is largely the community's. Clarity's content is used as
  any other source: read from its public repository for this personal
  project, or through services that partner with it. Every number keeps its
  provenance, so what rests on Clarity alone is visible should the project
  go public.

## Data still to gather

- MossyMax's tabs that saved empty: Scorch damage, ability and misc damage,
  weapon damage and DPS, perk damage.
- The Compendium's ability and verb numbers, read as tables. Today they are
  read as text.
- Enemy composition and pack sizes per activity, curated in
  `data/contexts.toml`.
- The rank-dependent damage curve, measurable in the Shooting Range.
