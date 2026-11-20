//! Does the all-activities ranking favour build types that did well across
//! many activities over ones that did very well in only a few?
//!
//! The all-activities scope is one fit over every run, each run counted
//! equally whatever its activity: a build type's effect is, roughly, its
//! runs' summed performance beyond their players' usual level divided by
//! (runs + the group prior). Breadth enters only through the number of runs.
//! This example measures how a build type's spread over activity groups
//! relates to its all-activities ranking, and how consistent the best-ranked
//! build types are across the groups' own rankings.
//!
//! ```text
//! cargo run --release -p focalpoint-db --example rank_breadth [DB]
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use focalpoint_db::Database;
use focalpoint_db::site::{SiteData, SiteOptions, site_data};

const MIN_PLAYERS: usize = 3;
/// Runs a build type needs in a group for its effect there to count.
const MIN_GROUP_RUNS: f64 = 5.0;

struct Listing {
    label: String,
    effect: f64,
    runs: f64,
    /// `(group, effect in the group's own ranking, runs there)`.
    groups: Vec<(String, f64, f64)>,
}

impl Listing {
    fn group_runs(&self) -> impl Iterator<Item = f64> + '_ {
        self.groups.iter().map(|g| g.2)
    }

    /// The effective number of activity groups: (Σ runs)² / Σ runs².
    fn breadth(&self) -> f64 {
        let sum: f64 = self.group_runs().sum();
        let squares: f64 = self.group_runs().map(|r| r * r).sum();
        if squares > 0.0 {
            sum * sum / squares
        } else {
            0.0
        }
    }

    /// The share of its runs in its busiest group, and that group.
    fn main_group(&self) -> (f64, &str) {
        let sum: f64 = self.group_runs().sum();
        self.groups
            .iter()
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .map_or((0.0, ""), |g| (g.2 / sum, g.0.as_str()))
    }

    /// Groups with at least `MIN_GROUP_RUNS` runs, and how many of those
    /// rank it above zero.
    fn consistency(&self) -> (usize, usize) {
        let enough: Vec<_> = self
            .groups
            .iter()
            .filter(|g| g.2 >= MIN_GROUP_RUNS)
            .collect();
        (enough.iter().filter(|g| g.1 > 0.0).count(), enough.len())
    }
}

fn pct(x: f64) -> String {
    format!("{:+.0}%", (x.exp() - 1.0) * 100.0)
}

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    xs.get(xs.len() / 2).copied().unwrap_or(f64::NAN)
}

/// Average ranks (ties share their mean rank).
fn ranks(xs: &[f64]) -> Vec<f64> {
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

fn spearman(a: &[f64], b: &[f64]) -> f64 {
    let (ra, rb) = (ranks(a), ranks(b));
    let n = ra.len() as f64;
    let (ma, mb) = (ra.iter().sum::<f64>() / n, rb.iter().sum::<f64>() / n);
    let cov: f64 = ra.iter().zip(&rb).map(|(x, y)| (x - ma) * (y - mb)).sum();
    let va: f64 = ra.iter().map(|x| (x - ma).powi(2)).sum();
    let vb: f64 = rb.iter().map(|y| (y - mb).powi(2)).sum();
    cov / (va * vb).sqrt()
}

/// The all-activities listings of `class`, best-ranked first, with each
/// build type's place in every activity group's own ranking.
fn listings(data: &SiteData, class: &str) -> Vec<Listing> {
    let mut by_group: HashMap<u32, Vec<(String, f64, f64)>> = HashMap::new();
    for scope in data.scopes.iter().filter(|s| s.kind == "group") {
        for r in &scope.rows {
            by_group
                .entry(r.0)
                .or_default()
                .push((scope.name.clone(), r.1, r.5));
        }
    }
    let mut out: Vec<Listing> = data.scopes[0]
        .rows
        .iter()
        .filter(|r| r.4 >= MIN_PLAYERS && data.types[r.0 as usize].class.as_deref() == Some(class))
        .map(|r| {
            let t = &data.types[r.0 as usize];
            Listing {
                label: format!(
                    "{} + {} · {} {}",
                    t.armor.as_deref().unwrap_or("no exotic armor"),
                    t.weapon.as_deref().unwrap_or("no exotic weapon"),
                    t.element.as_deref().unwrap_or(""),
                    t.super_name.as_deref().unwrap_or(""),
                ),
                effect: r.1,
                runs: r.5,
                groups: by_group.remove(&r.0).unwrap_or_default(),
            }
        })
        .collect();
    out.sort_by(|a, b| b.effect.total_cmp(&a.effect));
    out
}

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).map_or_else(
        || {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".local/share/focalpoint/focalpoint.duckdb")
        },
        PathBuf::from,
    );
    let db = Database::open_read_only(&path)?;
    eprintln!("ranking every scope (as the site does)…");
    let data = site_data(&db, &SiteOptions::default(), |_| {})?;
    println!(
        "All-activities listings with ≥ {MIN_PLAYERS} players; breadth = effective number of activity groups.\n"
    );
    for class in ["Hunter", "Titan", "Warlock"] {
        let ls = listings(&data, class);
        let n = ls.len();
        let decile = n.div_ceil(10);
        let effect: Vec<f64> = ls.iter().map(|l| l.effect).collect();
        let breadth: Vec<f64> = ls.iter().map(Listing::breadth).collect();
        let runs: Vec<f64> = ls.iter().map(|l| l.runs).collect();
        println!("## {class}: {n} build types");
        println!(
            "Rank correlation: effect~breadth {:+.2} · effect~runs {:+.2} · breadth~runs {:+.2}",
            spearman(&effect, &breadth),
            spearman(&effect, &runs),
            spearman(&breadth, &runs)
        );
        println!(
            "\n| breadth | build types | median runs | median effect | in the top 10% |\n|---|---|---|---|---|"
        );
        for (name, lo, hi) in [
            ("one group (< 1.5)", 0.0, 1.5),
            ("about two (1.5–2.5)", 1.5, 2.5),
            ("three or more (≥ 2.5)", 2.5, f64::INFINITY),
        ] {
            let members: Vec<usize> = (0..n).filter(|&i| (lo..hi).contains(&breadth[i])).collect();
            let top = members.iter().filter(|&&i| i < decile).count();
            println!(
                "| {name} | {} | {:.0} | {} | {} ({:.0}% of the top 10%) |",
                members.len(),
                median(members.iter().map(|&i| runs[i]).collect()),
                pct(median(members.iter().map(|&i| effect[i]).collect())),
                top,
                100.0 * top as f64 / decile as f64
            );
        }
        let concentrated = |range: std::ops::Range<usize>| {
            let k = range.len().max(1);
            100.0 * range.filter(|&i| ls[i].main_group().0 >= 0.8).count() as f64 / k as f64
        };
        println!(
            "\n≥ 80% of runs in one group: {:.0}% of the top 10%, {:.0}% of the rest.",
            concentrated(0..decile),
            concentrated(decile..n)
        );
        let (pos, with): (usize, usize) = ls[..decile.min(n)]
            .iter()
            .map(Listing::consistency)
            .fold((0, 0), |(a, b), (p, w)| (a + p, b + w));
        println!(
            "Top 10% in the groups' own rankings (groups with ≥ {MIN_GROUP_RUNS} runs): above zero in {pos} of {with}."
        );
        println!("\nTop 12:");
        for (i, l) in ls.iter().take(12).enumerate() {
            let (share, group) = l.main_group();
            let (pos, with) = l.consistency();
            println!(
                "  {:>2}. {:>5}  {:>4.0} runs  breadth {:.1}  {:>3.0}% {:<34} above zero in {pos}/{with} groups  {}",
                i + 1,
                pct(l.effect),
                l.runs,
                l.breadth(),
                100.0 * share,
                group,
                l.label
            );
        }
        println!();
    }
    Ok(())
}
