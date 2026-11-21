# Effects: what build parts need and do

The synergy evaluator needs to know, for each part of a build, what sets it
off, what it gives, and which game states it produces or depends on — so
that it can discount a kill-driven engine where kills are scarce
([contexts.md](contexts.md)) and credit parts that feed each other. These
are read from the community's mechanics texts ([mechanics.md](mechanics.md))
by `focalpoint_mechanics::effects` during `focalpoint mechanics import`, as
**tags** with the sentence each was read from.

## Reading

Texts are split into **conditions** and **effects**. A line ending in a
colon is the condition of the lines under it, up to a blank line ("On
Weapon Kill:", "While Amplified:"). In a sentence, the part before its main
verb (grants, triggers, spawns, applies, refills, increases, …) is its
condition and the rest its effect: "Weapon Kills | grant 10% Grenade
Ability Energy"; "Picking up an Orb of Power | grants Devour".

| Role | Read from | Tags |
|---|---|---|
| `trigger` | conditions, and phrases such as "from Grenade Kills" in effects | `kill` (`kill.weapon`, `kill.ability`, `kill.precision`, `kill.debuffed`, `kill.elite`, `kill.multi`), `hit` (`hit.precision`), `ability_use`, `pickup`, `surrounded` (enemies within a distance), `damage_taken`, `movement`, `handling`, `combat_time`, `passive` — a part with payoffs but no trigger is `passive` |
| `payoff` | effects | `damage.weapon`, `damage.ability`, `damage.direct` (explosions, projectiles), `energy.grenade`/`melee`/`class`/`super`/`ability`, `survival`, `ammo`, `stats`, `speed`, `spawn` |
| `produces` | a state in an effect after grants, inflicts, applies, creates, spawns, … | Radiant, Restoration, Cure, Scorch, Ignition, Firesprite, Weaken, Volatile, Devour, Invisibility, Overshield, Suppression, Void Breach, Amplified, Jolt, Blind, Ionic Trace, Bolt Charge, Slow, Freeze, Shatter, Frost Armor, Stasis Crystal, Stasis Shard, Sever, Suspend, Unravel, Woven Mail, Tangle, Threadling, Orb of Power, Armor Charge, Transcendence, Speed Booster; element debuffs (Arc/Solar/Void/Stasis/Strand Debuff) |
| `depends` | a state in a condition, or in an effect after while, against, on, when, … or before "enemies", "pickup", "is active" | the same states |

The game states are those the Compendium's subclass glossaries define,
with Orbs of Power, Armor Charge and the element debuffs. Patterns are
deliberately simple and every tag keeps its evidence; the reading is
checked by unit tests on real texts (Demolitionist, Echo of Starvation,
Repulsor Brace, Incandescent, Bait and Switch, Synthoceps) and can be
audited in the table.

## Tables

| Table | What |
|---|---|
| `mechanic_tag` | `(source, entry_id, role, tag, evidence)` for every documented entry |
| `mechanic_hash_tag` (view) | per manifest hash: `role`, `tag`, the `sources` that say so, and one evidence sentence (perks documented with their weapon count for the perk, not the weapon) |

Of 3,510 entries, about 990 have a kill trigger, 700 a hit trigger, 255
ability use, 175 a pickup; 1,180 work passively. Orbs of Power are produced
by 82 entries and depended on by 81; Scorch by 92 and 44.

## A check: kill reliance by activity

`focalpoint analyze triggers` compares what sets off the parts of the
builds played in each activity group — on the parts both sides record:
subclass choices, armor mods, exotic armor — with the boss damage
workbook's measured rotations. Where kills are scarce or hard, players
lean less on kill-triggered parts:

| Builds of | Kill | Hit | Pickup | Nearby enemies |
|---|---|---|---|---|
| Equilibrium, Ultimate Fireteam Op, Master Dungeon | 23.9–24.2% | 16.8–17.5% | 12.0–14.4% | 4.3–4.4% |
| Pantheon, Ultimate Pinnacle Op, Master Raid | 22.5–23.1% | 14.2–17.0% | 10.1–13.8% | 3.3–4.6% |
| Grandmaster Conquests | 21.4–21.8% | 16.8–16.9% | 14.1–14.5% | 4.8% |
| Ultimate Conquest | 18.8% | 16.4% | 14.9% | 4.9% |
| Boss damage rotations | **17.0%** | **20.3%** | **6.9%** | **0.8%** |

The boss rotations lean least on kills, pickups and nearby enemies and most
on hits. The differences between activity groups are modest — most parts
are general-purpose — but run along the gradient. (The Desert Perpetual's
recorded builds document about 13 such parts against 22 elsewhere: a
capture gap, not a play style.)
