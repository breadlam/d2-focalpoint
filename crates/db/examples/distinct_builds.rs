//! Which well-ranked builds stand furthest apart? An analysis toward
//! "highlight builds": per class, for all activities and for each activity
//! group, how many well-ranked builds are mutually distinct under several
//! notions of "distinct", and what that diversity costs in ranking.
//!
//! Candidates are the builds the site shows: the top build of each ranked
//! build type used by at least `MIN_PLAYERS` players (default 3), ordered by
//! the build type's effect. Two selections are reported for each criterion:
//!
//! - *rank-first*: walk the candidates best-ranked first, keeping each one
//!   that is distinct from every build kept so far (quality first);
//! - *most*: a greedy maximum independent set — repeatedly keep the build
//!   that rules out the fewest others — estimating how many mutually
//!   distinct builds exist at all.
//!
//! ```text
//! MIN_PLAYERS=3 cargo run --release -p focalpoint-db --example distinct_builds [DB]
//! ```

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use focalpoint_core::vector::{ElementVector, TierWeights};
use focalpoint_db::Database;
use focalpoint_db::site::{SiteData, SiteOptions, site_data};

/// `ElementKind` codes (1-based positions in `ElementKind::ALL`).
const EXOTIC_ARMOR: i64 = 1;
const EXOTIC_WEAPON: i64 = 3;
const ARTIFACT_PERK: i64 = 12;

struct Candidate {
    build_id: i64,
    label: String,
    effect: f64,
    players: usize,
}

/// What the analysis needs of one build.
#[derive(Clone)]
struct Profile {
    /// Every tiered element, artifact perks included.
    full: ElementVector,
    /// The same without artifact perks, which follow the season.
    lasting: ElementVector,
    /// Exotic armor and weapon keys.
    exotics: Vec<i64>,
}

/// How two candidates differ.
#[derive(Clone, Copy, Default)]
struct Pair {
    /// Weighted tiered distance, as `focalpoint similar` uses.
    total: f64,
    /// The same without artifact perks.
    total_lasting: f64,
    /// Kit tier distance (subclass, super, abilities, aspects, fragments)
    /// without artifact perks.
    kit: f64,
    shares_exotic: bool,
}

/// How a per-activity highlight relates to the all-activities collection.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Novelty {
    NewPlaystyle,
    OtherExotics,
    Repeat,
}

type Criterion = (&'static str, fn(&Pair) -> bool);

const CRITERIA: [Criterion; 6] = [
    ("weighted distance ≥ 0.9", |p| p.total >= 0.9),
    ("  … without artifact perks", |p| p.total_lasting >= 0.9),
    ("no shared exotic, kit ≥ 0.5", |p| {
        !p.shares_exotic && p.kit >= 0.5
    }),
    ("no shared exotic, kit ≥ 0.7", |p| {
        !p.shares_exotic && p.kit >= 0.7
    }),
    ("no shared exotic, kit ≥ 0.85", |p| {
        !p.shares_exotic && p.kit >= 0.85
    }),
    ("no shared exotic, kit ≥ 0.95", |p| {
        !p.shares_exotic && p.kit >= 0.95
    }),
];
/// The criterion listed in full and used per activity.
const CHOSEN: usize = 3;

fn profiles(db: &Database, cands: &[Candidate]) -> anyhow::Result<Vec<Profile>> {
    let list: Vec<String> = cands.iter().map(|c| c.build_id.to_string()).collect();
    let mut stmt = db.conn().prepare(&format!(
        "SELECT build_id, list_concat(tier1_elements, tier2_elements, tier3_elements, tier4_elements)
         FROM build WHERE build_id IN ({})",
        list.join(", ")
    ))?;
    let mut keys_of = HashMap::new();
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let keys: Vec<i64> = match r.get::<_, duckdb::types::Value>(1)? {
            duckdb::types::Value::List(v) => v
                .into_iter()
                .filter_map(|x| match x {
                    duckdb::types::Value::BigInt(k) => Some(k),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        keys_of.insert(id, keys);
    }
    Ok(cands
        .iter()
        .map(|c| {
            let keys = &keys_of[&c.build_id];
            Profile {
                full: ElementVector::from_keys(keys.iter().copied()),
                lasting: ElementVector::from_keys(
                    keys.iter().copied().filter(|k| k >> 32 != ARTIFACT_PERK),
                ),
                exotics: keys
                    .iter()
                    .copied()
                    .filter(|k| matches!(k >> 32, EXOTIC_ARMOR | EXOTIC_WEAPON))
                    .collect(),
            }
        })
        .collect())
}

fn pair(a: &Profile, b: &Profile) -> Pair {
    let weights = TierWeights::default();
    let lasting = a.lasting.tiered_distance(&b.lasting, &weights);
    Pair {
        total: a.full.tiered_distance(&b.full, &weights).total,
        total_lasting: lasting.total,
        kit: lasting.per_tier[1],
        shares_exotic: a.exotics.iter().any(|k| b.exotics.contains(k)),
    }
}

fn pairs(profiles: &[Profile]) -> Vec<Vec<Pair>> {
    let n = profiles.len();
    let mut out = vec![vec![Pair::default(); n]; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let p = pair(&profiles[i], &profiles[j]);
            out[i][j] = p;
            out[j][i] = p;
        }
    }
    out
}

/// Quality first: best-ranked first, keep each build distinct from all kept.
fn rank_first(m: &[Vec<Pair>], pool: &[usize], ok: fn(&Pair) -> bool) -> Vec<usize> {
    let mut kept: Vec<usize> = Vec::new();
    for &i in pool {
        if kept.iter().all(|&k| ok(&m[i][k])) {
            kept.push(i);
        }
    }
    kept
}

/// Size first: repeatedly keep the build that rules out the fewest others
/// (ties to the better-ranked) — a greedy maximum independent set.
fn most_distinct(m: &[Vec<Pair>], pool: &[usize], ok: fn(&Pair) -> bool) -> Vec<usize> {
    let mut left = pool.to_vec();
    let mut kept = Vec::new();
    while !left.is_empty() {
        let pick = *left
            .iter()
            .min_by_key(|&&i| (left.iter().filter(|&&j| j != i && !ok(&m[i][j])).count(), i))
            .unwrap();
        kept.push(pick);
        left.retain(|&j| j != pick && ok(&m[pick][j]));
    }
    kept.sort_unstable();
    kept
}

fn pct(x: f64) -> String {
    format!("{:+.0}%", (x.exp() - 1.0) * 100.0)
}

fn mean_effect(cands: &[Candidate], idx: impl IntoIterator<Item = usize>) -> f64 {
    let (sum, n) = idx
        .into_iter()
        .fold((0.0, 0), |(s, n), i| (s + cands[i].effect, n + 1));
    sum / f64::from(n.max(1))
}

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    xs.get(xs.len() / 2).copied().unwrap_or(f64::NAN)
}

fn candidates(data: &SiteData, scope: usize, class: &str, min_players: usize) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = data.scopes[scope]
        .rows
        .iter()
        .filter(|r| r.4 >= min_players && data.types[r.0 as usize].class.as_deref() == Some(class))
        .map(|r| {
            let t = &data.types[r.0 as usize];
            Candidate {
                build_id: data.recipes[r.7.0 as usize].id.parse().unwrap(),
                label: format!(
                    "{} + {} · {} {} · {}",
                    t.armor.as_deref().unwrap_or("no exotic armor"),
                    t.weapon.as_deref().unwrap_or("no exotic weapon"),
                    t.element.as_deref().unwrap_or(""),
                    t.super_name.as_deref().unwrap_or(""),
                    t.aspects.join(", ")
                ),
                effect: r.1,
                players: r.4,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.effect
            .total_cmp(&a.effect)
            .then(a.build_id.cmp(&b.build_id))
    });
    out
}

fn top(n: usize, share: f64) -> Vec<usize> {
    (0..((n as f64 * share).ceil() as usize).min(n)).collect()
}

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).map_or_else(
        || {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".local/share/focalpoint/focalpoint.duckdb")
        },
        PathBuf::from,
    );
    let min_players: usize = std::env::var("MIN_PLAYERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let db = Database::open_read_only(&path)?;
    eprintln!("ranking every scope (as the site does)…");
    let data = site_data(&db, &SiteOptions::default(), |_| {})?;
    let classes = ["Hunter", "Titan", "Warlock"];
    let chosen = CRITERIA[CHOSEN];

    println!("Candidates: the top build of each ranked build type with ≥ {min_players} players.\n");
    println!("# All activities\n");
    // Each class's all-activities collection, by build type.
    let mut global: HashMap<&str, Vec<Profile>> = HashMap::new();
    for class in classes {
        let cands = candidates(&data, 0, class, min_players);
        let profs = profiles(&db, &cands)?;
        let m = pairs(&profs);
        let n = cands.len();
        let all_pairs = || (0..n).flat_map(|i| ((i + 1)..n).map(move |j| (i, j)));
        let count = all_pairs().count().max(1) as f64;
        println!("## {class}: {n} build types");
        println!(
            "Pairs: median weighted distance {:.2}; median kit distance {:.2}; {:.1}% share an exotic; {:.0}% are ≥ 0.9 apart but share an exotic or have kit < 0.5.",
            median(all_pairs().map(|(i, j)| m[i][j].total).collect()),
            median(all_pairs().map(|(i, j)| m[i][j].kit).collect()),
            100.0 * all_pairs().filter(|&(i, j)| m[i][j].shares_exotic).count() as f64 / count,
            100.0
                * all_pairs()
                    .filter(|&(i, j)| {
                        let p = m[i][j];
                        p.total >= 0.9 && (p.shares_exotic || p.kit < 0.5)
                    })
                    .count() as f64
                / count,
        );
        let pools = [
            ("all", top(n, 1.0)),
            ("top half", top(n, 0.5)),
            ("top quarter", top(n, 0.25)),
        ];
        println!(
            "\n| distinct when | {} |\n|---|{}",
            pools
                .iter()
                .map(|(name, p)| format!("{name} ({}): rank-first / most", p.len()))
                .collect::<Vec<_>>()
                .join(" | "),
            "---|".repeat(pools.len())
        );
        for (name, ok) in CRITERIA {
            let cells: Vec<String> = pools
                .iter()
                .map(|(_, p)| {
                    format!(
                        "{} / {}",
                        rank_first(&m, p, ok).len(),
                        most_distinct(&m, p, ok).len()
                    )
                })
                .collect();
            println!("| {name} | {} |", cells.join(" | "));
        }

        let set = rank_first(&m, &top(n, 0.5), chosen.1);
        global.insert(class, set.iter().map(|&i| profs[i].clone()).collect());
        println!(
            "\nRank-first set, top half, {}: {} builds, mean effect {} (the top {} alone: {}).",
            chosen.0,
            set.len(),
            pct(mean_effect(&cands, set.iter().copied())),
            set.len(),
            pct(mean_effect(&cands, 0..set.len())),
        );
        for &i in &set {
            let nearest = set
                .iter()
                .filter(|&&j| j != i)
                .map(|&j| m[i][j].kit)
                .fold(1.0_f64, f64::min);
            println!(
                "  rank {:<4} {:>5}  {:>3} players  nearest kit {:.2}  {}",
                i + 1,
                pct(cands[i].effect),
                cands[i].players,
                nearest,
                cands[i].label
            );
        }
        println!();
    }

    // ---- Per scope: the collection each activity would get ---------------
    let show: Vec<String> = std::env::var("SHOW")
        .unwrap_or_else(|_| "grandmaster-conquest,master-dungeon,master-raid".into())
        .split(',')
        .map(String::from)
        .collect();
    println!(
        "# Per activity ({}, rank-first from the top half of each scope's build types)\n",
        chosen.0
    );
    println!(
        "Cells: highlights (build types with ≥ {min_players} players) · new playstyles: highlights whose kit is ≥ 0.7 from every all-activities highlight of the class.\n"
    );
    println!("| scope | Hunter | Titan | Warlock |\n|---|---|---|---|");
    let mut listings = Vec::new();
    let mut tally = [0usize; 3];
    let mut sizes: Vec<usize> = Vec::new();
    for (s, scope) in data
        .scopes
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind != "all")
    {
        let mut cells = Vec::new();
        for class in classes {
            let cands = candidates(&data, s, class, min_players);
            if cands.is_empty() {
                cells.push("—".to_string());
                sizes.push(0);
                continue;
            }
            let profs = profiles(&db, &cands)?;
            let m = pairs(&profs);
            let set = rank_first(&m, &top(cands.len(), 0.5), chosen.1);
            // How each highlight relates to the class's all-activities
            // collection: a new playstyle (kit ≥ 0.7 from all of them), the
            // same playstyle with other exotics, or a near repeat.
            let relation = |i: usize| {
                let close: Vec<Pair> = global[class]
                    .iter()
                    .map(|g| pair(&profs[i], g))
                    .filter(|p| p.kit < 0.7)
                    .collect();
                if close.is_empty() {
                    Novelty::NewPlaystyle
                } else if close.iter().any(|p| p.shares_exotic) {
                    Novelty::Repeat
                } else {
                    Novelty::OtherExotics
                }
            };
            for &i in &set {
                tally[relation(i) as usize] += 1;
            }
            let new = set
                .iter()
                .filter(|&&i| relation(i) == Novelty::NewPlaystyle)
                .count();
            cells.push(format!("{} ({}) · {new} new", set.len(), cands.len()));
            sizes.push(set.len());
            if show.contains(&scope.id) {
                let mut text = format!("## {} · {class}: {} highlights\n", scope.name, set.len());
                for &i in &set {
                    let _ = writeln!(
                        text,
                        "  rank {:<4} {:>5}  {:>3} players  {}{}",
                        i + 1,
                        pct(cands[i].effect),
                        cands[i].players,
                        cands[i].label,
                        match relation(i) {
                            Novelty::NewPlaystyle => "  [new playstyle]",
                            Novelty::OtherExotics => "",
                            Novelty::Repeat => "  [near repeat of an all-activities highlight]",
                        }
                    );
                }
                listings.push(text);
            }
        }
        let name = if scope.kind == "group" {
            format!("**{}** `{}`", scope.name, scope.id)
        } else {
            format!("↳ {}", scope.name)
        };
        println!("| {name} | {} |", cells.join(" | "));
    }
    let at_least = |k: usize| sizes.iter().filter(|&&n| n >= k).count();
    println!(
        "\nScope × class collections: {} in all; {} with ≥ 4 highlights, {} with ≥ 6, {} with ≥ 8.\n",
        sizes.len(),
        at_least(4),
        at_least(6),
        at_least(8)
    );
    println!(
        "Per-activity highlights: {} new playstyles, {} the same playstyle as an all-activities highlight with other exotics, {} near repeats.\n",
        tally[0], tally[1], tally[2]
    );
    for l in listings {
        println!("{l}");
    }
    Ok(())
}
