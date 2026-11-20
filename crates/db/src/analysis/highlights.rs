//! How the highlight selection ([`crate::highlight`]) trades rank for
//! variety: why each pick was made, and how the picks change with the
//! quality weight.

use focalpoint_core::highlight::{Pick, Playstyle};

use crate::highlight::{Candidate, select};

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
