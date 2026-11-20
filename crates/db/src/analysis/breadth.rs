//! Does a build type's all-activities rank depend on how many activity
//! groups it was used in?
//!
//! The all-activities scope is one fit over every run, each run counted the
//! same whatever its activity, so breadth enters only through the number of
//! runs. The neutral-game ranking ([`crate::rank::RankOptions::context_prior`])
//! credits only what holds across activity groups. This study measures
//! both against each build type's spread over activity groups and its place
//! in every group's own ranking.

use std::collections::HashMap;

use super::{median, spearman};
use crate::site::SiteData;

/// Runs a build type needs in a group for its effect there to count toward
/// consistency.
pub const MIN_GROUP_RUNS: f64 = 5.0;

/// One all-activities listing.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    /// Index into [`SiteData::types`].
    pub type_id: u32,
    pub effect: f64,
    /// The activity-independent effect (NaN when not computed).
    pub neutral: f64,
    pub runs: f64,
    pub players: usize,
    /// `(group, effect in the group's own ranking, runs there)`.
    pub groups: Vec<(String, f64, f64)>,
}

impl Listing {
    fn group_runs(&self) -> impl Iterator<Item = f64> + '_ {
        self.groups.iter().map(|g| g.2)
    }

    /// The effective number of activity groups: (Σ runs)² / Σ runs².
    #[must_use]
    pub fn breadth(&self) -> f64 {
        let sum: f64 = self.group_runs().sum();
        let squares: f64 = self.group_runs().map(|r| r * r).sum();
        if squares > 0.0 {
            sum * sum / squares
        } else {
            0.0
        }
    }

    /// Its busiest group and the share of its runs there.
    #[must_use]
    pub fn main_group(&self) -> (&str, f64) {
        let sum: f64 = self.group_runs().sum();
        self.groups
            .iter()
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .map_or(("", 0.0), |g| (g.0.as_str(), g.2 / sum))
    }

    /// Groups with at least [`MIN_GROUP_RUNS`] runs, and how many of them
    /// rank it above zero: `(above zero, groups)`.
    #[must_use]
    pub fn consistency(&self) -> (usize, usize) {
        let enough: Vec<_> = self
            .groups
            .iter()
            .filter(|g| g.2 >= MIN_GROUP_RUNS)
            .collect();
        (enough.iter().filter(|g| g.1 > 0.0).count(), enough.len())
    }
}

/// The all-activities listings of `class` with at least `min_players`
/// players, best-ranked first.
#[must_use]
pub fn listings(data: &SiteData, class: &str, min_players: usize) -> Vec<Listing> {
    let mut by_type: HashMap<u32, Vec<(String, f64, f64)>> = HashMap::new();
    for scope in data.scopes.iter().filter(|s| s.kind == "group") {
        for r in &scope.rows {
            by_type
                .entry(r.0)
                .or_default()
                .push((scope.name.clone(), r.1, r.5));
        }
    }
    let Some(all) = data.scopes.iter().find(|s| s.id == "all") else {
        return Vec::new();
    };
    let mut out: Vec<Listing> = all
        .rows
        .iter()
        .filter(|r| r.4 >= min_players && data.types[r.0 as usize].class.as_deref() == Some(class))
        .map(|r| Listing {
            type_id: r.0,
            effect: r.1,
            neutral: data.neutral.get(&r.0).copied().unwrap_or(f64::NAN),
            runs: r.5,
            players: r.4,
            groups: by_type.remove(&r.0).unwrap_or_default(),
        })
        .collect();
    out.sort_by(|a, b| b.effect.total_cmp(&a.effect));
    out
}

/// Build types in one breadth band.
#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    pub label: &'static str,
    pub count: usize,
    pub median_runs: f64,
    pub median_effect: f64,
    /// How many are in the top 10% by the all-activities effect.
    pub in_top_decile: usize,
    /// … and by the neutral-game effect.
    pub in_neutral_top_decile: usize,
}

/// The study for one class.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub class: String,
    pub listings: Vec<Listing>,
    /// Rank correlations: effect ~ breadth, effect ~ runs, breadth ~ runs,
    /// neutral effect ~ breadth.
    pub effect_breadth: f64,
    pub effect_runs: f64,
    pub breadth_runs: f64,
    pub neutral_breadth: f64,
    pub bands: Vec<Band>,
    /// Shares with ≥ 80% of their runs in one group: of the top 10% (by
    /// effect), of the rest, and of the top 10% by neutral effect.
    pub concentrated_top: f64,
    pub concentrated_rest: f64,
    pub concentrated_neutral_top: f64,
    /// Over the top 10%: groups (with enough runs) ranking them above zero,
    /// and groups with enough runs.
    pub consistency: (usize, usize),
}

/// Run the study for `class`.
#[must_use]
pub fn summarize(data: &SiteData, class: &str, min_players: usize) -> Summary {
    let ls = listings(data, class, min_players);
    let n = ls.len();
    let decile = n.div_ceil(10);
    let effect: Vec<f64> = ls.iter().map(|l| l.effect).collect();
    let neutral: Vec<f64> = ls.iter().map(|l| l.neutral).collect();
    let breadth: Vec<f64> = ls.iter().map(Listing::breadth).collect();
    let runs: Vec<f64> = ls.iter().map(|l| l.runs).collect();
    // Listings in the top 10% by neutral effect.
    let mut by_neutral: Vec<usize> = (0..n).collect();
    by_neutral.sort_by(|&a, &b| neutral[b].total_cmp(&neutral[a]));
    let mut neutral_top = vec![false; n];
    for &i in by_neutral.iter().take(decile) {
        neutral_top[i] = true;
    }
    let bands = [
        ("one group (< 1.5)", 0.0, 1.5),
        ("about two (1.5–2.5)", 1.5, 2.5),
        ("three or more (≥ 2.5)", 2.5, f64::INFINITY),
    ]
    .into_iter()
    .map(|(label, lo, hi)| {
        let members: Vec<usize> = (0..n).filter(|&i| (lo..hi).contains(&breadth[i])).collect();
        Band {
            label,
            count: members.len(),
            median_runs: median(members.iter().map(|&i| runs[i]).collect()),
            median_effect: median(members.iter().map(|&i| effect[i]).collect()),
            in_top_decile: members.iter().filter(|&&i| i < decile).count(),
            in_neutral_top_decile: members.iter().filter(|&&i| neutral_top[i]).count(),
        }
    })
    .collect();
    let share = |idx: &mut dyn Iterator<Item = usize>| {
        let (mut hit, mut all) = (0, 0);
        for i in idx {
            all += 1;
            if ls[i].main_group().1 >= 0.8 {
                hit += 1;
            }
        }
        if all == 0 {
            f64::NAN
        } else {
            f64::from(hit) / f64::from(all)
        }
    };
    let consistency = ls[..decile.min(n)]
        .iter()
        .map(Listing::consistency)
        .fold((0, 0), |(a, b), (p, w)| (a + p, b + w));
    Summary {
        class: class.to_string(),
        effect_breadth: spearman(&effect, &breadth),
        effect_runs: spearman(&effect, &runs),
        breadth_runs: spearman(&breadth, &runs),
        neutral_breadth: spearman(&neutral, &breadth),
        bands,
        concentrated_top: share(&mut (0..decile)),
        concentrated_rest: share(&mut (decile..n)),
        concentrated_neutral_top: share(&mut (0..n).filter(|&i| neutral_top[i])),
        consistency,
        listings: ls,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(groups: &[(&str, f64, f64)]) -> Listing {
        Listing {
            type_id: 0,
            effect: 0.0,
            neutral: 0.0,
            runs: groups.iter().map(|g| g.2).sum(),
            players: 1,
            groups: groups
                .iter()
                .map(|&(n, e, r)| (n.to_string(), e, r))
                .collect(),
        }
    }

    #[test]
    fn breadth_counts_groups_by_their_share_of_runs() {
        let even = listing(&[("a", 0.1, 10.0), ("b", 0.1, 10.0)]);
        assert!((even.breadth() - 2.0).abs() < 1e-12);
        let skewed = listing(&[("a", 0.1, 18.0), ("b", -0.1, 2.0)]);
        assert!(skewed.breadth() < 1.3);
        assert_eq!(skewed.main_group(), ("a", 0.9));
        // Only "a" has enough runs to count.
        assert_eq!(skewed.consistency(), (1, 1));
    }
}
