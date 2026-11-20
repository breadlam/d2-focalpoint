//! How the highlight selection ([`crate::highlight`]) trades rank for
//! variety: why each pick was made, how the picks change with the quality
//! weight, and — the survey — what distances, ranks and evidence the picks
//! have in every set when the hard constraints are loosened.

use std::collections::HashMap;
use std::hash::BuildHasher;

use focalpoint_core::highlight::{Pick, Playstyle};
use focalpoint_core::vector::ElementVector;

use super::diversity::{CHOSEN, CRITERIA, Pair, Profile};
use crate::highlight::{Candidate, NEUTRAL, candidates, playstyles, select};
use crate::site::SiteData;

/// One pick, explained.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Explained {
    /// Index into the candidates, which are best-ranked first: the pick's
    /// rank is `candidate + 1`.
    pub candidate: usize,
    pub novelty: f64,
    /// The most similar earlier pick, by position in the picks, and its
    /// similarity.
    pub nearest: Option<(usize, f64)>,
}

/// Explain `picks` of `cands`.
#[must_use]
pub fn explain(styles: &[Playstyle], picks: &[Pick]) -> Vec<Explained> {
    picks
        .iter()
        .enumerate()
        .map(|(k, p)| Explained {
            candidate: p.index,
            novelty: p.novelty,
            nearest: picks[..k]
                .iter()
                .enumerate()
                .map(|(j, q)| (j, styles[p.index].similarity(&styles[q.index])))
                .max_by(|a, b| a.1.total_cmp(&b.1)),
        })
        .collect()
}

/// The picks at one quality weight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweepPoint {
    pub weight: f64,
    pub picks: usize,
    /// Mean quality of the picks.
    pub mean_quality: f64,
    /// Mean pairwise playstyle similarity of the picks, and the largest.
    pub mean_similarity: f64,
    pub max_similarity: f64,
    /// Ranks (1 = best) of the median and the worst pick.
    pub median_rank: usize,
    pub worst_rank: usize,
}

/// Select `count` picks at each weight. An infinite weight is the plain
/// ranking (the top `count`), for comparison.
#[must_use]
pub fn sweep(
    cands: &[Candidate],
    styles: &[Playstyle],
    count: usize,
    weights: &[f64],
) -> Vec<SweepPoint> {
    weights
        .iter()
        .map(|&weight| {
            let picks: Vec<usize> = if weight.is_infinite() {
                (0..count.min(cands.len())).collect()
            } else {
                select(cands, styles, count, weight)
                    .iter()
                    .map(|p| p.index)
                    .collect()
            };
            let n = picks.len();
            let mut sims = Vec::new();
            for (a, &i) in picks.iter().enumerate() {
                for &j in &picks[a + 1..] {
                    sims.push(styles[i].similarity(&styles[j]));
                }
            }
            let mut ranks: Vec<usize> = picks.iter().map(|&i| i + 1).collect();
            ranks.sort_unstable();
            SweepPoint {
                weight,
                picks: n,
                mean_quality: picks.iter().map(|&i| cands[i].quality).sum::<f64>()
                    / n.max(1) as f64,
                mean_similarity: sims.iter().sum::<f64>() / sims.len().max(1) as f64,
                max_similarity: sims.iter().copied().fold(0.0, f64::max),
                median_rank: ranks.get(n / 2).copied().unwrap_or(0),
                worst_rank: ranks.last().copied().unwrap_or(0),
            }
        })
        .collect()
}

/// How the soft selection's picks differ from each other and rank.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoftPicks {
    pub picks: usize,
    /// The smallest kit distance (without artifact perks) between two picks.
    pub min_kit: f64,
    /// Pairs of picks sharing an exotic armor or weapon.
    pub shared_exotic_pairs: usize,
    /// The smallest playstyle distance (1 − similarity) between two picks.
    pub min_distance: f64,
    /// Mean quality of the picks, and of the plain top `picks`.
    pub mean_quality: f64,
    pub top_quality: f64,
    /// The worst pick's rank among the candidates (1 = best).
    pub worst_rank: usize,
    /// Picks one player used, and picks used in at most one run.
    pub one_player: usize,
    pub one_run: usize,
}

/// One set and class of the survey.
#[derive(Debug, Clone, PartialEq)]
pub struct SurveyRow {
    pub set: String,
    pub class: String,
    pub candidates: usize,
    /// Candidates with at least the survey's minimum of players.
    pub established: usize,
    /// Picks (up to the count) under the hard rules — no shared exotic, kit
    /// distance ≥ 0.7, best-ranked first: from the top half of the
    /// established candidates (the earlier, strict study) and from every
    /// candidate (loosened), with the loosened set's worst rank.
    pub hard_strict: usize,
    pub hard_loose: usize,
    pub hard_loose_worst_rank: usize,
    /// Loosened hard-rule picks that one player used.
    pub hard_loose_one_player: usize,
    /// The plain top `count` by rank that one player used.
    pub top_one_player: usize,
    pub soft: SoftPicks,
}

/// Rank-first under `ok`, comparing profiles as needed; stops at `count`.
fn rank_first_lazy(
    profiles: &[Profile],
    pool: impl IntoIterator<Item = usize>,
    count: usize,
    ok: fn(&Pair) -> bool,
) -> Vec<usize> {
    let mut kept: Vec<usize> = Vec::new();
    for i in pool {
        if kept.len() == count {
            break;
        }
        if kept
            .iter()
            .all(|&k| ok(&Pair::of(&profiles[i], &profiles[k])))
        {
            kept.push(i);
        }
    }
    kept
}

/// Survey one set and class: `count` picks by the strict hard rules, the
/// loosened hard rules and the soft selection at `quality_weight`.
#[must_use]
pub fn survey<S: BuildHasher>(
    data: &SiteData,
    vectors: &HashMap<i64, ElementVector, S>,
    set: &str,
    class: &str,
    count: usize,
    quality_weight: f64,
    min_players: usize,
) -> SurveyRow {
    let cands = candidates(data, set, class);
    let scope_id = if set == NEUTRAL { "all" } else { set };
    let rows = &data
        .scopes
        .iter()
        .find(|s| s.id == scope_id)
        .expect("candidates come from a computed scope")
        .rows;
    let players = |i: usize| rows[cands[i].row].4;
    let runs = |i: usize| rows[cands[i].row].5;
    let profiles: Vec<Profile> = cands
        .iter()
        .map(|c| {
            vectors
                .get(&c.build_id)
                .map_or_else(|| Profile::of(&ElementVector::default()), Profile::of)
        })
        .collect();
    let established: Vec<usize> = (0..cands.len())
        .filter(|&i| players(i) >= min_players)
        .collect();
    let half = established.len().div_ceil(2);
    let ok = CRITERIA[CHOSEN].1;
    let strict = rank_first_lazy(&profiles, established[..half].iter().copied(), count, ok);
    let loose = rank_first_lazy(&profiles, 0..cands.len(), count, ok);

    let styles = playstyles(&cands, vectors);
    let picks: Vec<usize> = select(&cands, &styles, count, quality_weight)
        .iter()
        .map(|p| p.index)
        .collect();
    let (mut min_kit, mut min_distance, mut shared) = (1.0_f64, 1.0_f64, 0);
    for (a, &i) in picks.iter().enumerate() {
        for &j in &picks[a + 1..] {
            let p = Pair::of(&profiles[i], &profiles[j]);
            min_kit = min_kit.min(p.kit);
            shared += usize::from(p.shares_exotic);
            min_distance = min_distance.min(1.0 - styles[i].similarity(&styles[j]));
        }
    }
    let n = picks.len();
    let mean = |idx: &mut dyn Iterator<Item = usize>| {
        let (sum, k) = idx.fold((0.0, 0_u32), |(s, k), i| (s + cands[i].quality, k + 1));
        if k == 0 { f64::NAN } else { sum / f64::from(k) }
    };
    SurveyRow {
        set: set.to_string(),
        class: class.to_string(),
        candidates: cands.len(),
        established: established.len(),
        hard_strict: strict.len(),
        hard_loose: loose.len(),
        hard_loose_worst_rank: loose.iter().max().map_or(0, |&i| i + 1),
        hard_loose_one_player: loose.iter().filter(|&&i| players(i) <= 1).count(),
        top_one_player: (0..count.min(cands.len()))
            .filter(|&i| players(i) <= 1)
            .count(),
        soft: SoftPicks {
            picks: n,
            min_kit,
            shared_exotic_pairs: shared,
            min_distance,
            mean_quality: mean(&mut picks.iter().copied()),
            top_quality: mean(&mut (0..n)),
            worst_rank: picks.iter().max().map_or(0, |&i| i + 1),
            one_player: picks.iter().filter(|&&i| players(i) <= 1).count(),
            one_run: picks.iter().filter(|&&i| runs(i) <= 1.0).count(),
        },
    }
}
