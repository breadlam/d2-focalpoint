//! Armor stat investment: what a build's six stats say about player intent.
//!
//! Raw stat totals are a poor basis for comparing builds. Effects change at
//! the 100 and 200 thresholds, players land near rather than on them, and a
//! player held back by their armor collection or fragment choices may have
//! the same *intent* as one with better armor. This module turns a build's
//! stats into an **investment level** per stat (0–4):
//!
//! | level | meaning |
//! |---|---|
//! | 0 | not invested (≈ 0–34; "25 is nearly the same as 0") |
//! | 1 | partial investment toward 100 |
//! | 2 | the 100 tier ("90-ish would have been 100") |
//! | 3 | partial investment into the 101–200 band |
//! | 4 | the 200 tier |
//!
//! Levels start from tolerance bands, then are **budget-aware**: a stat is
//! *held back by budget* when the final stat line itself shows it could not
//! have been higher: stats that reached a threshold offer only their excess
//! above it, and the stats below 100 hold no more than the unavoidable
//! remainder (≈ 17 per stat, taken together). A held-back stat is promoted
//! to the threshold it was reaching for. The final investment is the
//! indicator — what the player's inventory allowed is unknowable — so
//! nothing is promoted on speculation; players with weaker armor still land
//! close to others because adjacent levels share partial credit. Fragment
//! points are fixed to their stat, so fragments shift both the stat and the
//! effective budget.
//!
//! See `docs/adr/0005-tiered-distance-and-stat-investment.md` for the data
//! behind the default parameters.

use crate::build::{Build, ComponentKind};
use crate::catalog::Catalog;

/// The six Armor 3.0 character stats, in storage order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ArmorStat {
    Weapons,
    Health,
    Class,
    Grenade,
    Super,
    Melee,
}

/// Stat values in [`ArmorStat::ALL`] order.
pub type StatLine = [i32; 6];

impl ArmorStat {
    pub const ALL: [ArmorStat; 6] = [
        ArmorStat::Weapons,
        ArmorStat::Health,
        ArmorStat::Class,
        ArmorStat::Grenade,
        ArmorStat::Super,
        ArmorStat::Melee,
    ];

    /// `DestinyStatDefinition` hash.
    #[must_use]
    pub fn hash(self) -> u32 {
        match self {
            ArmorStat::Weapons => 2_996_146_975,
            ArmorStat::Health => 392_767_087,
            ArmorStat::Class => 1_943_323_491,
            ArmorStat::Grenade => 1_735_777_505,
            ArmorStat::Super => 144_602_215,
            ArmorStat::Melee => 4_244_567_218,
        }
    }

    #[must_use]
    pub fn from_hash(hash: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.hash() == hash)
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ArmorStat::Weapons => "Weapons",
            ArmorStat::Health => "Health",
            ArmorStat::Class => "Class",
            ArmorStat::Grenade => "Grenade",
            ArmorStat::Super => "Super",
            ArmorStat::Melee => "Melee",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Stat changes granted by a build's fragments (fixed, not reallocatable).
#[must_use]
pub fn fragment_stats(build: &Build, catalog: &Catalog) -> StatLine {
    let mut line = [0; 6];
    for c in build
        .components
        .iter()
        .filter(|c| c.kind == ComponentKind::Fragment)
    {
        let Some(def) = catalog.item(c.hash) else {
            continue;
        };
        for &(stat_hash, value) in &def.stat_effects {
            if let Some(stat) = ArmorStat::from_hash(stat_hash) {
                line[stat.index()] += value * i32::from(c.quantity.max(1));
            }
        }
    }
    line
}

/// Tunable parameters of the investment model.
#[derive(Debug, Clone, PartialEq)]
pub struct InvestmentParams {
    /// Lower bounds of levels 1, 2, 3 and 4.
    pub band_cutoffs: [i32; 4],
    /// Armor points a de-prioritised stat keeps anyway (cannot be moved).
    pub floor: i32,
    /// A stat is budget-limited when its slack is at most this.
    pub slack_tolerance: i32,
    /// Values within this margin above 100 count as having reached 100.
    pub reached_margin: i32,
}

impl Default for InvestmentParams {
    /// Defaults measured on the Searchlight data (ADR 0005): the bottom three
    /// stats of fully optimised builds hold ≈ 50 points (≈ 17 each), and
    /// players who reach 100 commonly spill over to 101–115.
    fn default() -> Self {
        Self {
            band_cutoffs: [35, 75, 125, 175],
            floor: 17,
            slack_tolerance: 15,
            reached_margin: 15,
        }
    }
}

/// The investment reading of one build's stats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Investment {
    /// Level 0–4 per stat, in [`ArmorStat::ALL`] order.
    pub levels: [u8; 6],
    /// Stats promoted because the budget held them back.
    pub promoted: [bool; 6],
}

impl Investment {
    #[must_use]
    pub fn level(&self, stat: ArmorStat) -> u8 {
        self.levels[stat.index()]
    }

    #[must_use]
    pub fn is_promoted(&self, stat: ArmorStat) -> bool {
        self.promoted[stat.index()]
    }
}

impl InvestmentParams {
    fn band(&self, value: i32) -> u8 {
        self.band_cutoffs.iter().filter(|&&c| value >= c).count() as u8
    }

    /// Points the stats other than `k` could have given to `k`, judged only
    /// from the final stat line (what was in the player's inventory is
    /// unknowable):
    ///
    /// * a stat that reached a threshold keeps it and offers only its excess
    ///   above 200 or 100;
    /// * stats below 100 offer their armor points beyond a reserve of
    ///   `floor` per stat, taken *together* — the unavoidable remainder is
    ///   spread unevenly ("the remaining 50 spread across three stats");
    /// * fragment points are fixed to their stat and never offered.
    fn slack(&self, k: usize, observed: &StatLine, armor: &StatLine) -> i32 {
        let mut excess = 0;
        let (mut below_armor, mut below_count) = (0, 0);
        for j in (0..6).filter(|&j| j != k) {
            let armor_j = armor[j].max(0);
            if observed[j] >= 100 {
                let threshold = if observed[j] >= 200 { 200 } else { 100 };
                excess += (observed[j] - threshold).min((armor_j - self.floor).max(0));
            } else {
                below_armor += armor_j;
                below_count += 1;
            }
        }
        excess + (below_armor - self.floor * below_count).max(0)
    }

    /// Read investment from observed stats and the fragments' share of them.
    #[must_use]
    pub fn investment(&self, observed: StatLine, fragments: StatLine) -> Investment {
        let armor: StatLine = std::array::from_fn(|k| observed[k] - fragments[k]);
        let mut out = Investment::default();
        for k in 0..6 {
            let value = observed[k];
            let band = self.band(value);
            // Points other stats hold beyond what they keep could have been
            // moved into this stat.
            let slack = self.slack(k, &observed, &armor);
            let invested = value >= self.band_cutoffs[0];
            let held_back = invested && slack <= self.slack_tolerance;
            let level = if !held_back {
                band
            } else if value < 100 {
                band.max(2)
            } else if value < 100 + self.reached_margin {
                band // reached 100; whether more was wanted is unknowable
            } else {
                4
            };
            out.promoted[k] = level > band;
            out.levels[k] = level;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a stat line from `(stat, value)`; unspecified stats are 0.
    fn line(values: &[(ArmorStat, i32)]) -> StatLine {
        let mut l = [0; 6];
        for &(s, v) in values {
            l[s.index()] = v;
        }
        l
    }

    use ArmorStat::{Class, Grenade, Health, Melee, Super, Weapons};

    fn levels(observed: StatLine, fragments: StatLine) -> Investment {
        InvestmentParams::default().investment(observed, fragments)
    }

    #[test]
    fn bands_follow_the_thresholds() {
        let p = InvestmentParams::default();
        assert_eq!(
            [0, 25, 34, 35, 74, 75, 90, 124, 125, 174, 175, 200, 230].map(|v| p.band(v)),
            [0, 0, 0, 1, 1, 2, 2, 2, 3, 3, 4, 4, 4]
        );
    }

    #[test]
    fn maxed_third_stat_reads_as_wanting_200() {
        // The example: Super 200 and Weapons 100 leave at most 150 for
        // Grenade, with ~50 spread over the rest and none above 25.
        let inv = levels(
            line(&[
                (Super, 200),
                (Weapons, 100),
                (Grenade, 150),
                (Health, 20),
                (Melee, 18),
                (Class, 12),
            ]),
            [0; 6],
        );
        assert_eq!(inv.level(Super), 4);
        assert_eq!(inv.level(Weapons), 2);
        assert_eq!(inv.level(Grenade), 4, "held back by budget");
        assert!(inv.is_promoted(Grenade));
        assert_eq!(
            (inv.level(Health), inv.level(Melee), inv.level(Class)),
            (0, 0, 0)
        );
    }

    #[test]
    fn remainder_is_a_shared_reserve() {
        // "The remaining 50 spread across Health, Melee and Class, none above
        // 25": uneven, but no more than the reserve in total.
        let inv = levels(
            line(&[
                (Super, 200),
                (Weapons, 100),
                (Grenade, 150),
                (Health, 25),
                (Melee, 25),
                (Class, 0),
            ]),
            [0; 6],
        );
        assert_eq!(inv.level(Grenade), 4);
        assert!(inv.is_promoted(Grenade));
    }

    #[test]
    fn weaker_armor_same_intent_same_levels() {
        let strong = levels(
            line(&[
                (Super, 200),
                (Weapons, 100),
                (Grenade, 150),
                (Health, 20),
                (Melee, 18),
                (Class, 12),
            ]),
            [0; 6],
        );
        let weak = levels(
            line(&[
                (Super, 200),
                (Weapons, 100),
                (Grenade, 128),
                (Health, 17),
                (Melee, 15),
                (Class, 10),
            ]),
            [0; 6],
        );
        assert_eq!(strong.levels, weak.levels);
    }

    #[test]
    fn deliberate_partial_investment_is_not_promoted() {
        // Grenade at 130 while 60 Health could have gone into it: a choice.
        let inv = levels(
            line(&[
                (Super, 200),
                (Weapons, 100),
                (Grenade, 130),
                (Health, 60),
                (Melee, 10),
                (Class, 10),
            ]),
            [0; 6],
        );
        assert_eq!(inv.level(Grenade), 3);
        assert!(!inv.is_promoted(Grenade));
        // Nor is Health held back: 30 of Grenade's points above 100 could
        // have moved into it, and the player chose Grenade.
        assert_eq!(inv.level(Health), 1);
        assert!(!inv.is_promoted(Health));
        // Reached thresholds are kept, not counted as slack: Weapons at 100
        // is a target, so Grenade with only floor-level stats below it is
        // held back even though Weapons ranks below it.
        let inv = levels(
            line(&[
                (Super, 200),
                (Weapons, 100),
                (Grenade, 130),
                (Health, 18),
                (Melee, 10),
                (Class, 10),
            ]),
            [0; 6],
        );
        assert!(inv.is_promoted(Grenade));
    }

    #[test]
    fn near_misses_and_noise() {
        // 90 with no slack would have been 100; 25 is noise.
        let inv = levels(
            line(&[
                (Super, 200),
                (Melee, 160),
                (Grenade, 90),
                (Class, 25),
                (Health, 15),
                (Weapons, 10),
            ]),
            [0; 6],
        );
        assert_eq!(inv.level(Grenade), 2);
        assert_eq!(inv.level(Class), 0);
        // Reaching 100 (with a little spill-over) is not read as wanting 200.
        let inv = levels(
            line(&[
                (Super, 200),
                (Melee, 150),
                (Grenade, 108),
                (Class, 20),
                (Health, 15),
                (Weapons, 10),
            ]),
            [0; 6],
        );
        assert_eq!(inv.level(Grenade), 2);
        assert!(!inv.is_promoted(Grenade));
    }

    #[test]
    fn fragments_shift_the_budget() {
        // Health 60 from armor is 29 points beyond the shared reserve that
        // could have gone into Grenade; if a fragment provides 30 of it, the
        // armor remainder is exactly the reserve and Grenade was held back.
        let observed = line(&[
            (Super, 200),
            (Weapons, 100),
            (Grenade, 135),
            (Health, 60),
            (Melee, 15),
            (Class, 5),
        ]);
        let without = levels(observed, [0; 6]);
        assert_eq!(
            without.level(Grenade),
            3,
            "Health's armor points are movable"
        );
        let with_fragment = levels(observed, line(&[(Health, 30)]));
        assert_eq!(
            with_fragment.level(Grenade),
            4,
            "30 of Health comes from a fragment, not armor"
        );
        // A fragment penalty lowers a stat without making it movable slack.
        let penalised = levels(
            line(&[
                (Super, 200),
                (Weapons, 100),
                (Grenade, 140),
                (Health, -5),
                (Melee, 20),
                (Class, 15),
            ]),
            line(&[(Health, -10)]),
        );
        assert_eq!(penalised.level(Grenade), 4);
    }

    #[test]
    fn hashes_round_trip() {
        for s in ArmorStat::ALL {
            assert_eq!(ArmorStat::from_hash(s.hash()), Some(s));
        }
    }
}
