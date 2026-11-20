//! `focalpoint analyze` — the analyses behind Focalpoint's design decisions
//! (`docs/analysis.md`), rerunnable on new data.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Result, bail};
use focalpoint_db::Database;
use focalpoint_db::analysis::{breadth, diversity, highlights as hl, priors};
use focalpoint_db::highlight::{self, NEUTRAL, QUALITY_WEIGHT, candidates, playstyles, select};
use focalpoint_db::rank::{NEUTRAL_CONTEXT_PRIOR, RankOptions, load_model};
use focalpoint_db::search::GroupBy;
use focalpoint_db::site::{SiteData, SiteOptions, build_vectors, site_data};

use crate::output::{Format, render, thousands};

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub what: What,

    /// Output format for tables
    #[arg(long, value_enum, default_value_t = Format::Table, global = true)]
    pub format: Format,
}

#[derive(Debug, clap::Subcommand)]
pub enum What {
    /// Choose the ranking model's priors by five-fold cross-validation
    #[command(after_help = "Examples:\n  \
focalpoint analyze priors\n  \
focalpoint analyze priors --contexts            # the neutral game's context prior\n  \
focalpoint analyze priors --grouping exotic-armor")]
    Priors(PriorArgs),
    /// Does a build type's rank depend on how many activity groups it was used in?
    #[command(
        after_help = "Compares the all-activities ranking with the neutral-game ranking \
(activity-independent effect) against each build type's spread over activity groups."
    )]
    Breadth(BreadthArgs),
    /// How many well-ranked build types are distinct under hard thresholds
    #[command(
        after_help = "The study that led to threshold-free highlights: counts of mutually \
distinct build types per class under several criteria, and how per-activity sets relate to the \
all-activities set."
    )]
    Diversity(DiversityArgs),
    /// Explain the highlight picks, or sweep the quality weight
    #[command(after_help = "Examples:\n  \
focalpoint analyze highlights                          # neutral game, every class\n  \
focalpoint analyze highlights --set pantheon-2-0 --class Titan\n  \
focalpoint analyze highlights --sweep --count 5\n  \
focalpoint analyze highlights --sets                   # list the set ids")]
    Highlights(HighlightArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Grouping {
    /// Class, subclass element, exotics, super and aspects
    Core,
    /// Exact builds
    Build,
    ExoticArmor,
}

#[derive(Debug, clap::Args)]
pub struct PriorArgs {
    #[arg(long, value_enum, default_value_t = Grouping::Core)]
    pub grouping: Grouping,
    /// Also fit build × activity group effects, over a grid of their prior
    #[arg(long)]
    pub contexts: bool,
    #[arg(long, value_delimiter = ',', default_value = "1,3,10,30,100")]
    pub player_priors: Vec<f64>,
    #[arg(long, value_delimiter = ',', default_value = "1,3,10,30,100")]
    pub group_priors: Vec<f64>,
    #[arg(long, value_delimiter = ',', default_value = "1,3,10,30,100")]
    pub context_priors: Vec<f64>,
}

#[derive(Debug, clap::Args)]
pub struct BreadthArgs {
    /// Build types need this many players
    #[arg(long, default_value_t = 3)]
    pub min_players: usize,
    /// Listings shown per class
    #[arg(long, default_value_t = 12)]
    pub top: usize,
}

#[derive(Debug, clap::Args)]
pub struct DiversityArgs {
    /// Build types need this many players
    #[arg(long, default_value_t = 3)]
    pub min_players: usize,
}

#[derive(Debug, clap::Args)]
pub struct HighlightArgs {
    /// `neutral` or a scope id (see --sets)
    #[arg(long, default_value = NEUTRAL)]
    pub set: String,
    /// One class (default: every class)
    #[arg(long)]
    pub class: Option<String>,
    #[arg(long, default_value_t = highlight::HIGHLIGHT_COUNT)]
    pub count: usize,
    #[arg(long, default_value_t = QUALITY_WEIGHT)]
    pub quality_weight: f64,
    /// The neutral game's context prior
    #[arg(long, default_value_t = NEUTRAL_CONTEXT_PRIOR)]
    pub context_prior: f64,
    /// Compare quality weights instead of listing picks
    #[arg(long)]
    pub sweep: bool,
    #[arg(long, value_delimiter = ',', default_value = "0.5,1,2,3,5,8,12,20,inf")]
    pub weights: Vec<f64>,
    /// List the highlight sets and exit
    #[arg(long)]
    pub sets: bool,
}

pub fn run(args: &Args, db: &Path) -> Result<()> {
    let db = Database::open_read_only(db)?;
    match &args.what {
        What::Priors(a) => run_priors(&db, a, args.format),
        What::Breadth(a) => run_breadth(&db, a, args.format),
        What::Diversity(a) => run_diversity(&db, a, args.format),
        What::Highlights(a) => run_highlights(&db, a, args.format),
    }
}

fn pct(x: f64) -> String {
    if x.is_nan() {
        return "—".into();
    }
    format!("{:+.0}%", (x.exp() - 1.0) * 100.0)
}

fn prior(x: f64) -> String {
    if x >= priors::NO_BUILDS || x.is_infinite() {
        "∞".into()
    } else {
        format!("{x}")
    }
}

/// "Exotic armor + exotic weapon · Element Super · aspects".
fn label(data: &SiteData, type_id: u32) -> String {
    let t = &data.types[type_id as usize];
    format!(
        "{} + {} · {} {} · {}",
        t.armor.as_deref().unwrap_or("no exotic armor"),
        t.weapon.as_deref().unwrap_or("no exotic weapon"),
        t.element.as_deref().unwrap_or(""),
        t.super_name.as_deref().unwrap_or(""),
        t.aspects.join(", ")
    )
}

fn site(db: &Database, scopes: Option<Vec<String>>, context_prior: f64) -> Result<SiteData> {
    eprintln!("ranking scopes…");
    let opts = SiteOptions {
        scopes,
        context_prior,
        ..SiteOptions::default()
    };
    Ok(site_data(db, &opts, |s| eprintln!("  {s}"))?)
}

fn run_priors(db: &Database, a: &PriorArgs, format: Format) -> Result<()> {
    let group_by = match a.grouping {
        Grouping::Core => GroupBy::Core,
        Grouping::Build => GroupBy::Build,
        Grouping::ExoticArmor => GroupBy::ExoticArmor,
    };
    let data = load_model(
        db,
        &RankOptions {
            group_by,
            ..RankOptions::default()
        },
    )?;
    let mut groups = a.group_priors.clone();
    groups.push(priors::NO_BUILDS);
    let mut contexts = vec![None];
    if a.contexts {
        contexts.extend(a.context_priors.iter().copied().map(Some));
    }
    let grid = priors::grid(&a.player_priors, &groups, &contexts);
    eprintln!(
        "cross-validating {} prior combinations over {} observations…",
        grid.len(),
        thousands(data.observations.len() as i64)
    );
    let report = priors::cross_validate(&data, &grid);
    println!(
        "{} observations, {} build groups, {} activity groups; error of predicting 0: {:.4}",
        thousands(report.observations as i64),
        thousands(report.groups as i64),
        data.context_names.len(),
        report.null_error
    );
    let header = [
        "player prior",
        "group prior",
        "context prior",
        "cv error",
        "explained",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = report
        .scores
        .iter()
        .map(|s| {
            vec![
                prior(s.priors.player),
                prior(s.priors.group),
                s.priors.context.map_or("—".into(), prior),
                format!("{:.5}", s.error),
                format!("{:.2}%", 100.0 * report.explained(s.error)),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    let describe = |s: &priors::Score| {
        format!(
            "player {}, group {}, context {} → explains {:.2}%",
            prior(s.priors.player),
            prior(s.priors.group),
            s.priors.context.map_or("—".into(), prior),
            100.0 * report.explained(s.error)
        )
    };
    if let (Some(best), Some(skill)) = (report.best(), report.best_without_builds()) {
        println!("best: {}", describe(best));
        println!(
            "player skill alone: {}; builds add {:.2} points",
            describe(skill),
            100.0 * (skill.error - best.error) / report.null_error
        );
        if a.contexts
            && let Some(plain) = report.best_without_contexts()
        {
            println!(
                "best without activity-specific effects: {}; they add {:.3} points",
                describe(plain),
                100.0 * (plain.error - best.error) / report.null_error
            );
        }
    }
    Ok(())
}

fn run_breadth(db: &Database, a: &BreadthArgs, format: Format) -> Result<()> {
    let data = site(db, None, NEUTRAL_CONTEXT_PRIOR)?;
    println!(
        "All-activities listings with ≥ {} players; breadth = effective number of activity groups; \
neutral = activity-independent effect.\n",
        a.min_players
    );
    for class in highlight::classes(&data, "all") {
        let s = breadth::summarize(&data, &class, a.min_players);
        println!("## {class}: {} build types", s.listings.len());
        println!(
            "Rank correlation: effect~breadth {:+.2} · effect~runs {:+.2} · breadth~runs {:+.2} · \
neutral~breadth {:+.2}",
            s.effect_breadth, s.effect_runs, s.breadth_runs, s.neutral_breadth
        );
        let header = [
            "breadth",
            "build types",
            "median runs",
            "median effect",
            "in top 10%",
            "in neutral top 10%",
        ]
        .map(String::from);
        let rows: Vec<Vec<String>> = s
            .bands
            .iter()
            .map(|b| {
                vec![
                    b.label.to_string(),
                    b.count.to_string(),
                    format!("{:.0}", b.median_runs),
                    pct(b.median_effect),
                    b.in_top_decile.to_string(),
                    b.in_neutral_top_decile.to_string(),
                ]
            })
            .collect();
        println!("{}", render(format, &header, &rows));
        println!(
            "≥ 80% of runs in one activity group: {:.0}% of the top 10%, {:.0}% of the rest, {:.0}% of \
the neutral top 10%.",
            100.0 * s.concentrated_top,
            100.0 * s.concentrated_rest,
            100.0 * s.concentrated_neutral_top
        );
        println!(
            "Top 10% in the groups' own rankings (groups with ≥ {} runs): above zero in {} of {}.",
            breadth::MIN_GROUP_RUNS,
            s.consistency.0,
            s.consistency.1
        );
        let header = [
            "#",
            "effect",
            "neutral",
            "runs",
            "breadth",
            "main group",
            "share",
            "above 0",
            "build type",
        ]
        .map(String::from);
        let rows: Vec<Vec<String>> = s
            .listings
            .iter()
            .take(a.top)
            .enumerate()
            .map(|(i, l)| {
                let (group, share) = l.main_group();
                let (pos, with) = l.consistency();
                vec![
                    (i + 1).to_string(),
                    pct(l.effect),
                    pct(l.neutral),
                    format!("{:.0}", l.runs),
                    format!("{:.1}", l.breadth()),
                    group.to_string(),
                    format!("{:.0}%", 100.0 * share),
                    format!("{pos}/{with}"),
                    label(&data, l.type_id),
                ]
            })
            .collect();
        println!("{}\n", render(format, &header, &rows));
    }
    Ok(())
}

fn run_diversity(db: &Database, a: &DiversityArgs, format: Format) -> Result<()> {
    let data = site(db, None, NEUTRAL_CONTEXT_PRIOR)?;
    let vectors = build_vectors(db)?;
    let (chosen_name, chosen) = diversity::CRITERIA[diversity::CHOSEN];
    println!(
        "Candidates: each build type's exemplar, ≥ {} players. Cells: rank-first / most distinct.\n",
        a.min_players
    );
    let mut reference: HashMap<String, Vec<diversity::Profile>> = HashMap::new();
    for class in highlight::classes(&data, "all") {
        let f = diversity::field(&data, &vectors, "all", &class, a.min_players);
        let n = f.candidates.len();
        println!("## {class}: {n} build types, all activities");
        let header = vec![
            "distinct when".to_string(),
            format!("all ({n})"),
            format!("top half ({})", n.div_ceil(2)),
            format!("top quarter ({})", n.div_ceil(4)),
        ];
        let rows: Vec<Vec<String>> = diversity::counts(&f)
            .into_iter()
            .map(|(name, cells)| {
                let mut r = vec![name.to_string()];
                r.extend(cells.iter().map(|(rf, most)| format!("{rf} / {most}")));
                r
            })
            .collect();
        println!("{}", render(format, &header, &rows));
        let set = diversity::rank_first(&f.pairs, &diversity::top(n, 0.5), chosen);
        println!("Rank-first, top half, {chosen_name}:");
        for &i in &set {
            let (t, effect, players) = f.candidates[i];
            println!(
                "  rank {:<4} {:>5}  {players:>3} players  {}",
                i + 1,
                pct(effect),
                label(&data, t)
            );
        }
        println!();
        reference.insert(class, set.iter().map(|&i| f.profiles[i].clone()).collect());
    }

    println!("# Per scope ({chosen_name}, rank-first from the top half)\n");
    let header = [
        "scope",
        "class",
        "build types",
        "picks",
        "new playstyles",
        "other exotics",
        "repeats",
    ]
    .map(String::from);
    let mut rows = Vec::new();
    for scope in data.scopes.iter().filter(|s| s.kind != "all") {
        for class in highlight::classes(&data, &scope.id) {
            let f = diversity::field(&data, &vectors, &scope.id, &class, a.min_players);
            if f.candidates.is_empty() {
                continue;
            }
            let set =
                diversity::rank_first(&f.pairs, &diversity::top(f.candidates.len(), 0.5), chosen);
            let kinds =
                diversity::novelty(&f, &set, reference.get(&class).map_or(&[], Vec::as_slice));
            let count =
                |k: diversity::Novelty| kinds.iter().filter(|&&x| x == k).count().to_string();
            rows.push(vec![
                scope.name.clone(),
                class,
                f.candidates.len().to_string(),
                set.len().to_string(),
                count(diversity::Novelty::NewPlaystyle),
                count(diversity::Novelty::OtherExotics),
                count(diversity::Novelty::Repeat),
            ]);
        }
    }
    println!("{}", render(format, &header, &rows));
    Ok(())
}

fn run_highlights(db: &Database, a: &HighlightArgs, format: Format) -> Result<()> {
    let scopes = if a.sets {
        None
    } else if a.set == NEUTRAL {
        Some(vec!["all".to_string()])
    } else {
        Some(vec![a.set.clone()])
    };
    let data = site(db, scopes, a.context_prior)?;
    if a.sets {
        let header = ["set", "name"].map(String::from);
        let mut rows = vec![vec![NEUTRAL.to_string(), "Neutral game".to_string()]];
        rows.extend(
            data.scopes
                .iter()
                .map(|s| vec![s.id.clone(), s.name.clone()]),
        );
        println!("{}", render(format, &header, &rows));
        return Ok(());
    }
    if data.scopes.is_empty() {
        bail!("no scope {:?} (see --sets)", a.set);
    }
    let vectors = build_vectors(db)?;
    let classes: Vec<String> = match &a.class {
        Some(c) => vec![c.clone()],
        None => highlight::classes(&data, &a.set),
    };
    for class in classes {
        let cands = candidates(&data, &a.set, &class);
        let styles = playstyles(&cands, &vectors);
        println!("## {} · {class}: {} candidates", a.set, cands.len());
        if a.sweep {
            let header = [
                "quality weight",
                "picks",
                "mean effect",
                "mean similarity",
                "max similarity",
                "median rank",
                "worst rank",
            ]
            .map(String::from);
            let rows: Vec<Vec<String>> = hl::sweep(&cands, &styles, a.count, &a.weights)
                .into_iter()
                .map(|p| {
                    vec![
                        if p.weight.is_infinite() {
                            "∞ (ranking)".into()
                        } else {
                            format!("{}", p.weight)
                        },
                        p.picks.to_string(),
                        pct(p.mean_quality),
                        format!("{:.2}", p.mean_similarity),
                        format!("{:.2}", p.max_similarity),
                        p.median_rank.to_string(),
                        p.worst_rank.to_string(),
                    ]
                })
                .collect();
            println!("{}\n", render(format, &header, &rows));
            continue;
        }
        let picks = select(&cands, &styles, a.count, a.quality_weight);
        let scope = data
            .scopes
            .iter()
            .find(|s| {
                s.id == if a.set == NEUTRAL {
                    "all"
                } else {
                    a.set.as_str()
                }
            })
            .expect("computed");
        let header = [
            "#",
            "rank",
            "effect",
            "players",
            "runs",
            "novelty",
            "closest pick",
            "build type",
        ]
        .map(String::from);
        let rows: Vec<Vec<String>> = hl::explain(&styles, &picks)
            .into_iter()
            .enumerate()
            .map(|(k, e)| {
                let c = &cands[e.candidate];
                let row = &scope.rows[c.row];
                vec![
                    (k + 1).to_string(),
                    (e.candidate + 1).to_string(),
                    pct(c.quality),
                    row.4.to_string(),
                    format!("{:.0}", row.5),
                    format!("{:.2}", e.novelty),
                    e.nearest
                        .map_or("—".into(), |(j, s)| format!("#{} ({s:.2})", j + 1)),
                    label(&data, c.type_id),
                ]
            })
            .collect();
        println!("{}\n", render(format, &header, &rows));
    }
    Ok(())
}
