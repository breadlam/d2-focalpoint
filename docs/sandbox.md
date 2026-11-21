# The sandbox: vocabulary and part cards

The build model ([build-model.md](build-model.md)) describes every build
part as **rules over a closed vocabulary**. Two kinds of file in
`data/sandbox/`, both built into the binary:

- **`ontology.toml`**, the vocabulary: value lists (elements, ranks, damage
  categories, damage tags, stats…), the **events** rules fire on with the
  attributes each has, and the **states** with the core verbs' parameters
  and their sources;
- **`parts/*.toml`**, the **cards**: one per part, saying what it does.

```sh
focalpoint sandbox check                      # check the cards; coverage and the drafting worklist
focalpoint sandbox check --dir data/sandbox   # while editing, without rebuilding
```

`check` fails on any name the vocabulary does not define. With a database,
it resolves every card, and every part a card names, against the manifest.
It also reports how much of the recorded builds' part use the cards cover,
by kind, and lists the most-used parts without a card.

## States

| `kind` | What | Examples |
|---|---|---|
| `resource` | the player's: ability and super energy, ammo, health, Armor Charge, Transcendence | `melee_energy` |
| `buff` | on the player or allies | `radiant`, `frost_armor`, `woven_mail` |
| `debuff` | on enemies | `scorch`, `jolt`, `volatile`, `slow`, `frozen`, `unravel` |
| `object` | in the world: picked up, destroyed or seeking | `orb_of_power`, `stasis_crystal`, `stasis_shard`, `firesprite`, `threadling` |
| `verb` | an instantaneous mechanic | `ignition`, `shatter`, `cure` |

A state with `max_stacks` and `at_max` fires that event when it fills:
Scorch at x100 fires `ignition`, Slow at x100 fires `freeze`. Parameters
are addressed `<state>.<param>` (`ignition.damage`, 676) or
`<ability>.<param>` (`melee.charges`).

## Cards

A card names its part and kind. It applies to every manifest item of that
name and kind, so reissues share it; `hashes = [...]` restricts it. Every
card cites `sources` (`family:reference`; families in the vocabulary) and
has a `confidence` (`documented`, default; `dated`; `assumed`), as
`data/difficulty.toml` does. Rules and modifiers may cite more sources and
carry their own confidence and `note`.

```toml
[[part]]
name = "Ember of Char"
kind = "fragment"
sources = ["compendium:Solar/Fragments/Char", "clarity:362132291"]

[[part.rule]]
on = { event = "ignition_hit", origin = false }
effect = { apply = "scorch", stacks = 40, target = "target" }
```

### Rules

`on` an event, `when` every condition holds, do each `effect`.

- **`on`**: an event name, or a table of the event and attribute values:
  `{ event = "kill", source = "weapon", element = "solar" }`.
- **`when`**: conditions, each setting one of:
  - `has` / `lacks` a buff;
  - `equipped` a part;
  - `stacks` (with `min`), of a state or one of the card's counters;
  - `movement`;
  - `in_super`;
  - `health_below`;
  - `enemies_within` (with `min`);
  - `target_has`;
  - `target_rank` / `target_rank_not`.
- **`effect`**: one or a list. Each sets exactly one primary field.
- **Limits:**
  - `cooldown`: seconds between firings;
  - `chance`;
  - `limit`: times per activation, such as per super cast.
- **`with`**: variants of a single-effect rule while another part is
  equipped, overriding effect fields: `with = [{ part = "Ember of Ashes",
  stacks = 60 }]`.

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
| `consume` | remove a state, or reset one of the card's counters | |
| `progress` | advance one of the card's counters | `amount` |

**Amounts** (`damage`, `heal`, `amount`, `stacks`, `count`) are numbers or
tables:

- `{ by_tier = [...] }`: by combatant tier from T1;
- `{ by_rank = { minor = ..., elite = ... } }`;
- `{ by_counter = [[value, amount], ...], counter = "..." }`: the highest
  row reached;
- `{ by_count = [...] }`: by copies equipped.

### Modifiers

`[[part.modify]]` changes, passively or `when` conditions hold, exactly
one of:

- **a parameter**: `param = "scorch.applied_multiplier"`;
- **damage carrying a tag**: `damage = "ignition"`, optionally of one
  `element`, with Court's `category` deciding how it stacks;
- **a stat**: `stat = "handling"`.

It does so by exactly one of `add`, `mul` or `set`; `round = "up"` where
the source says so.

### Counters

`[[part.counter]]` declares a counter of the part's own, with a
`threshold`. Rules `progress` it, `consume` it, read it in `stacks`
conditions and `by_counter` amounts, and fire on `counter_full`.

## Writing a card

- Take numbers from the Data Compendium, Court's guides, the cooldown
  workbook, MossyMax's sheets, Endgame Analysis, Bungie's notes, in-game
  measurement and Clarity. Cite each.
- Where sources disagree, use the better one and say so in the `note`
  (Whisper of Hunger: Compendium +60% against Clarity's 17.5% and 75%).
- Where no source gives a number, mark the rule `assumed` and say so, or
  leave it out and say so in the card's `note`. Never put an invented
  number in as documented.
- Write what the game does, not what it adds up to. Dawn Chorus has no
  rule for the Compendium's "Fulmination doubles the melee energy": a
  Fulmination counts as an ignition, so each enemy it damages fires the
  ignition rule again, and the doubling follows.
- Don't re-implement a core verb. Apply it or modify its parameters.
