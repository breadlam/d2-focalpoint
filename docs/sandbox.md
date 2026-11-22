# The sandbox: vocabulary and rule base

The build model ([build-model.md](build-model.md)) describes what build
parts do as **records over a closed vocabulary**. `data/sandbox/` holds two
kinds of file, both built into the binary:

- **`ontology.toml`**, the vocabulary:
  - value lists: elements, ranks, damage categories, damage tags, stats…;
  - the **events** that rules fire on, with each event's attributes;
  - the **states**, with the core verbs' parameters and their sources.
- **`rules/*.toml`**, the **rule base**: rules, modifiers, counters and
  gaps.

```sh
focalpoint sandbox check                      # check the rule base; coverage, gaps and the drafting worklist
focalpoint sandbox check --dir data/sandbox   # while editing, without rebuilding
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
- `{ by_count = [...] }`: by copies of the granting part equipped.

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
- Where no source gives a number, do one of the following:
  - mark the record `assumed` and say so;
  - leave the behaviour out and record a gap.

  Never put an invented number in as documented.
- Write what the game does, not what it adds up to. Dawn Chorus has no rule
  for the Compendium's "Fulmination doubles the melee energy". A
  Fulmination counts as an ignition, so each enemy it damages fires the
  ignition rule again, and the doubling follows.
- Don't re-implement a core verb. Apply it or modify its parameters.
