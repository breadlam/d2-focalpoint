//! Highlights: per class, a few exact builds that are as well ranked and as
//! unlike each other as the data allows ([`focalpoint_core::highlight`]),
//! for the neutral game and for every activity scope (ADR 0007).
//!
//! Candidates are a scope's listings: every ranked build type, however few
//! players it had, represented by its exemplar build. Their quality is the
//! build type's effect in the scope — for the neutral game, the part of its
//! all-activities effect that holds across activity groups
//! ([`crate::rank::RankOptions::context_prior`]). Nothing is filtered out:
//! a build type seen once competes on its (shrunk) effect and on what it
//! adds to the picks.

use std::collections::HashMap;
use std::hash::BuildHasher;

use focalpoint_core::highlight::{Pick, Playstyle, select_diverse};
use focalpoint_core::vector::ElementVector;
use serde::Serialize;

use crate::site::SiteData;

/// Highlights kept per class and scope; the site shows the first 3, 5 or 10.
pub const HIGHLIGHT_COUNT: usize = 10;

/// The quality weight of [`select_diverse`]. At 2, a candidate half like an
/// earlier pick (similarity 0.45, novelty 0.8) must be about 6 points of
/// effect better than an entirely new one to be preferred; at 5 the same
/// near-repeat costs only 2 points and exotic swaps of a pick get in. Chosen
/// with `focalpoint analyze highlights --sweep` (ADR 0007): against the plain
/// top 10 it costs about a point of mean effect and cuts the mean pairwise
/// similarity of the picks from 0.15–0.25 to 0.03–0.05.
pub const QUALITY_WEIGHT: f64 = 2.0;

/// The id of the neutral-game highlight set.
pub const NEUTRAL: &str = "neutral";

/// One candidate: a build type's exemplar in a scope.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Index into [`SiteData::types`].
    pub type_id: u32,
    /// Index into the scope's rows (the all-activities scope for the
    /// neutral game).
    pub row: usize,
    /// Index into [`SiteData::recipes`].
    pub recipe: u32,
    pub build_id: i64,
    pub quality: f64,
}

/// A class's highlights in one set.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ClassHighlights {
    pub class: String,
    pub picks: Vec<Highlight>,
}

/// `[type, effect, players, runs, last used, recipe, activity groups the
/// build type was used in, novelty]`. The effect is the set's quality (for
/// the neutral game, the activity-independent effect).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Highlight(
    pub u32,
    pub f64,
    pub usize,
    pub f64,
    pub String,
    pub u32,
    pub usize,
    pub f64,
);

/// One set of highlights: the neutral game or a scope.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HighlightSet {
    pub id: String,
    pub name: String,
    pub classes: Vec<ClassHighlights>,
}

/// The scope a highlight set draws its listings from.
fn scope_of(data: &SiteData, set: &str) -> Option<usize> {
    let id = if set == NEUTRAL { "all" } else { set };
    data.scopes.iter().position(|s| s.id == id)
}

/// The classes with listings in a set, sorted.
#[must_use]
pub fn classes(data: &SiteData, set: &str) -> Vec<String> {
    let Some(scope) = scope_of(data, set) else {
        return Vec::new();
    };
    let mut out: Vec<String> = data.scopes[scope]
        .rows
        .iter()
        .filter_map(|r| data.types[r.0 as usize].class.clone())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// A set's candidates of one class, best first.
#[must_use]
pub fn candidates(data: &SiteData, set: &str, class: &str) -> Vec<Candidate> {
    let Some(scope) = scope_of(data, set) else {
        return Vec::new();
    };
    let mut out: Vec<Candidate> = data.scopes[scope]
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| data.types[r.0 as usize].class.as_deref() == Some(class))
        .filter_map(|(i, r)| {
            let quality = if set == NEUTRAL {
                *data.neutral.get(&r.0)?
            } else {
                r.1
            };
            let recipe = r.7.0;
            Some(Candidate {
                type_id: r.0,
                row: i,
                recipe,
                build_id: data.recipes[recipe as usize].id.parse().ok()?,
                quality,
            })
        })
        .collect();
    out.sort_by(|a, b| b.quality.total_cmp(&a.quality).then(a.row.cmp(&b.row)));
    out
}

/// The playstyle of every candidate's exemplar build.
#[must_use]
pub fn playstyles<S: BuildHasher>(
    cands: &[Candidate],
    vectors: &HashMap<i64, ElementVector, S>,
) -> Vec<Playstyle> {
    cands
        .iter()
        .map(|c| {
            vectors
                .get(&c.build_id)
                .map(Playstyle::of)
                .unwrap_or_default()
        })
        .collect()
}

/// Pick up to `count` candidates (indices into `cands`), in pick order.
#[must_use]
pub fn select(
    cands: &[Candidate],
    styles: &[Playstyle],
    count: usize,
    quality_weight: f64,
) -> Vec<Pick> {
    let quality: Vec<f64> = cands.iter().map(|c| c.quality).collect();
    select_diverse(
        &quality,
        |i, j| {
            if i == j {
                1.0
            } else {
                styles[i].similarity(&styles[j])
            }
        },
        count,
        quality_weight,
    )
}

/// Every highlight set: the neutral game first (when the all-activities
/// scope was computed), then each activity group and activity.
#[must_use]
pub fn highlight_sets<S: BuildHasher>(
    data: &SiteData,
    vectors: &HashMap<i64, ElementVector, S>,
    count: usize,
    quality_weight: f64,
) -> Vec<HighlightSet> {
    // Activity groups each build type was used in.
    let mut groups_of: HashMap<u32, usize> = HashMap::new();
    for scope in data.scopes.iter().filter(|s| s.kind == "group") {
        for r in &scope.rows {
            *groups_of.entry(r.0).or_default() += 1;
        }
    }
    let mut sets: Vec<(String, String)> = Vec::new();
    if !data.neutral.is_empty() && scope_of(data, NEUTRAL).is_some() {
        sets.push((NEUTRAL.to_string(), "Neutral game".to_string()));
    }
    // The neutral game stands in for the all-activities scope.
    sets.extend(
        data.scopes
            .iter()
            .filter(|s| s.kind != "all")
            .map(|s| (s.id.clone(), s.name.clone())),
    );
    sets.into_iter()
        .map(|(id, name)| {
            let scope = scope_of(data, &id).expect("sets come from scopes");
            let classes = classes(data, &id)
                .into_iter()
                .map(|class| {
                    let cands = candidates(data, &id, &class);
                    let styles = playstyles(&cands, vectors);
                    let picks = select(&cands, &styles, count, quality_weight)
                        .into_iter()
                        .map(|p| {
                            let c = &cands[p.index];
                            let r = &data.scopes[scope].rows[c.row];
                            Highlight(
                                c.type_id,
                                round(c.quality, 3),
                                r.4,
                                r.5,
                                r.6.clone(),
                                c.recipe,
                                groups_of.get(&c.type_id).copied().unwrap_or(0),
                                round(p.novelty, 2),
                            )
                        })
                        .collect();
                    ClassHighlights { class, picks }
                })
                .collect();
            HighlightSet { id, name, classes }
        })
        .collect()
}

fn round(x: f64, digits: i32) -> f64 {
    let scale = 10_f64.powi(digits);
    (x * scale).round() / scale
}
