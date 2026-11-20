//! Analyses behind Focalpoint's design decisions, kept so they can be rerun
//! on new data and extended (`focalpoint analyze …`; `docs/analysis.md`).
//!
//! * [`priors`] — the ranking model's ridge priors, by cross-validation
//!   (ADR 0006; the neutral game's context prior, ADR 0007);
//! * [`completeness`] — which captured builds are missing significant parts
//!   (aspects, fragments, mods), and how much evidence they carry;
//! * [`breadth`] — whether a build type's all-activities rank depends on how
//!   many activity groups it was used in, and how the neutral-game ranking
//!   changes that;
//! * [`diversity`] — how many well-ranked build types are mutually distinct
//!   under hard thresholds (the study that led to [`crate::highlight`]);
//! * [`highlights`] — how the highlight selection trades rank for variety.
//!
//! Each returns plain data; the CLI renders it.

pub mod breadth;
pub mod completeness;
pub mod diversity;
pub mod highlights;
pub mod priors;

/// Average ranks, ties sharing their mean rank.
#[must_use]
pub fn ranks(xs: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..xs.len()).collect();
    idx.sort_by(|&a, &b| xs[a].total_cmp(&xs[b]));
    let mut out = vec![0.0; xs.len()];
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && xs[idx[j + 1]].total_cmp(&xs[idx[i]]).is_eq() {
            j += 1;
        }
        let rank = (i + j) as f64 / 2.0;
        for &k in &idx[i..=j] {
            out[k] = rank;
        }
        i = j + 1;
    }
    out
}

/// Spearman's rank correlation; NaN when either side is constant.
#[must_use]
pub fn spearman(a: &[f64], b: &[f64]) -> f64 {
    let (ra, rb) = (ranks(a), ranks(b));
    let n = ra.len() as f64;
    let (ma, mb) = (ra.iter().sum::<f64>() / n, rb.iter().sum::<f64>() / n);
    let cov: f64 = ra.iter().zip(&rb).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let va: f64 = ra.iter().map(|x| (x - ma).powi(2)).sum();
    let vb: f64 = rb.iter().map(|y| (y - mb).powi(2)).sum();
    cov / (va * vb).sqrt()
}

/// The middle value (the upper one of an even count); NaN when empty.
#[must_use]
pub fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    xs.get(xs.len() / 2).copied().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_correlation() {
        assert!((spearman(&[1.0, 2.0, 3.0], &[10.0, 20.0, 35.0]) - 1.0).abs() < 1e-12);
        assert!((spearman(&[1.0, 2.0, 3.0], &[3.0, 2.0, 1.0]) + 1.0).abs() < 1e-12);
        assert_eq!(ranks(&[5.0, 1.0, 5.0]), [1.5, 0.0, 1.5]);
        assert!((median(vec![3.0, 1.0, 2.0]) - 2.0).abs() < 1e-12);
    }
}
