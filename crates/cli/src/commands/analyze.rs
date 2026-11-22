//! `focalpoint analyze` — the analyses behind Focalpoint's design decisions
//! (`docs/analysis.md`), rerunnable on new data.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Result, bail};
use focalpoint_core::vector::ElementVector;
use focalpoint_db::Database;
use focalpoint_db::analysis::{
    attempts, breadth, completeness, difficulty, diversity, escalation, highlights as hl, partial,
    priors, sets, solo, subclasses, triggers,
};
use focalpoint_db::highlight::{self, NEUTRAL, QUALITY_WEIGHT, candidates, playstyles, select};
use focalpoint_db::rank::{NEUTRAL_CONTEXT_PRIOR, RankOptions, SoloScoring, load_model};
use focalpoint_db::search::GroupBy;
use focalpoint_db::site::{
    POLISHED_SET_BONUSES, SiteData, SiteOptions, build_vectors, polished_builds, site_data,
};

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
    /// Which captured builds are missing significant parts, and what that does to highlights
    #[command(
        after_help = "Counts builds by aspects and fragments, checks fragment counts against \
each aspect pair's usual count, compares the evidence behind complete and incomplete builds, and \
surveys the highlights with incomplete builds left out."
    )]
    Completeness(CompletenessArgs),
    /// What a quota of highlights per subclass would pick
    #[command(
        after_help = "Neutral game: --neutral-per picks per classic subclass and --prismatic \
Prismatic picks per class; activities: --activity-per picks per subclass. Reports the picks' \
ranks, evidence and distinctness, and compares them with picking without a quota."
    )]
    Subclasses(SubclassArgs),
    /// Soloing content made for a fireteam: volume, what it takes, and crediting it in the rankings
    #[command(
        after_help = "Compares solo clears with the fireteam clears of the same activities, and \
the all-activities build ranking with solo clears measured against solo clears (now) and against \
fireteam clears (credited)."
    )]
    Solo(SoloArgs),
    /// Partial clears and partial participation: which content allows them, and how runs are scored
    #[command(
        after_help = "Checkpoints and late joins per activity group, kill rates of fireteam, \
full-length solo and very fast solo clears, and the run score of players present for all or part of a \
fireteam clear — now, and as a kill rate over time played (Searchlight's later evaluator)."
    )]
    Partial,
    /// Armor set bonuses: what full bonuses are worth, who skips them, and top builds without them
    #[command(
        after_help = "Recorded scored clears by their main build's set bonuses; the gain from full \
bonuses (a 4-piece set or two 2-piece sets) for the same player in the same activity; players' level \
by how often they skip full bonuses; and the all-activities listings whose top build lacks them \
(ADR 0010)."
    )]
    Sets,
    /// Failed attempts in Conquests and Ops: do build types differ in how often they clear at all?
    #[command(
        after_help = "Uncleared runs per activity group (failures where runs cannot be resumed, \
sessions where they can); players' sequences of tries in Conquests and Ops; first tries' clear rates \
by build type against players of the same level, with their split-half reliability; and how those \
compare with each group's build-type ranking, which scores clears only."
    )]
    Attempts,
    /// What sets off the parts of the builds played in each activity group (needs `mechanics import`)
    #[command(
        after_help = "Shares of the builds' documented parts (subclass choices, armor mods, exotic armor) \
triggered by kills, hits, ability use, pickups, nearby enemies, or nothing (passive), by activity group and \
for the boss damage workbook's measured rotations. Where kills are scarce or hard, builds should lean less on \
kill-triggered parts."
    )]
    Triggers,
    /// How much each difficulty step slows the same players' kills in the same activities
    #[command(
        after_help = "Each player's kill rate and deaths at every step of an activity's difficulty \
ladder, against their own clears of the same activity at the base step; kill rates are the geometric \
mean of the ratios. Comparing players with themselves removes the selection of stronger players into \
harder steps. --skull NAME compares clears with and without a modifier instead, at the same step. \
See docs/durability.md."
    )]
    Difficulty(DifficultyArgs),
    /// Parts whose lift depends on the content: pieces of synergistic escalation loops
    #[command(after_help = "Examples:\n  \
focalpoint analyze escalation                     # parts by hard-minus-easy lift\n  \
focalpoint analyze escalation --anchor \"Dawn Chorus\" --parts \"Ember of Char,Ember of Ashes,Solar Fulmination\"\n  \
focalpoint analyze escalation --anchor \"Glacial Quake\" --parts \"Howl of the Storm,Tectonic Harvest,Whisper of Hunger,Crystalline Converter\"\n\n\
Clears are measured against their player's usual level. Contexts: hard (Conquests), easy (Ops, Equilibrium, \
dungeons), boss (raids, Pantheon, The Desert Perpetual). See docs/escalation.md.")]
    Escalation(EscalationArgs),
    /// Explain the highlight picks, or sweep the quality weight
    #[command(after_help = "Examples:\n  \
focalpoint analyze highlights                          # neutral game, every class\n  \
focalpoint analyze highlights --set pantheon-2-0 --class Titan\n  \
focalpoint analyze highlights --sweep --count 5\n  \
focalpoint analyze highlights --sets                   # list the set ids")]
    Highlights(HighlightArgs),
    /// The build model against the recorded clears: each kit's metrics against its measured effect
    #[command(
        after_help = "Each recorded kit (super, abilities, aspects, fragments, Transcendence, exotic \
armor) with enough clears in a context is measured against its players' usual level, as `analyze \
escalation` does, and evaluated by the build model in that context's scenario (hard: Grandmaster Conquest; \
easy: Ultimate Fireteam Op). Reports each metric's rank correlation with the effect, and a ridge \
regression of the effect on the metrics scored on held-out kits. Needs `focalpoint mechanics import`. \
See docs/build-model.md and docs/sandbox.md."
    )]
    Model(ModelArgs),
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, clap::Args)]
pub struct ModelArgs {
    /// Clears (by build share) a kit needs in a context
    #[arg(long, default_value_t = 100.0)]
    pub min_clears: f64,

    /// Players a kit needs in a context
    #[arg(long, default_value_t = 15)]
    pub min_players: i64,

    /// Instead: each modelled part's predicted marginal value against its measured lift
    #[arg(long)]
    pub parts: bool,

    /// Calibrate each scenario's ability efficiency to the recorded share of kills by abilities first
    #[arg(long)]
    pub fit: bool,

    /// Kits each part is judged on (--parts)
    #[arg(long, default_value_t = 20)]
    pub max_kits: usize,

    /// Instead: list each context's kits with their accessories and metrics
    #[arg(long)]
    pub kits: bool,

    /// Instead: the simulator's share of kills by abilities against each kit's recorded share
    #[arg(long)]
    pub sim: bool,

    /// Simulated runs a kit (--sim)
    #[arg(long, default_value_t = 8)]
    pub sim_runs: usize,

    /// Minutes a simulated run lasts (--sim)
    #[arg(long, default_value_t = 5.0)]
    pub sim_minutes: f64,

    /// Set a variable (`name=value`), as `sandbox analyze --set`
    #[arg(long, value_name = "NAME=VALUE")]
    pub set: Vec<String>,
}

#[derive(Debug, clap::Args)]
pub struct DifficultyArgs {
    /// The step players are compared with
    #[arg(long, default_value = "Master")]
    pub base: String,
    /// Player-activity pairs a step (or group, with --skull) needs to be listed
    #[arg(long, default_value_t = 100)]
    pub min_pairs: i64,
    /// Compare clears with this skull (e.g. "Feat: Cutthroat Combat") and without it instead
    #[arg(long)]
    pub skull: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct EscalationArgs {
    /// Compare builds with this part (by name: an exotic, aspect, super, ...) by which of --parts they run
    #[arg(long)]
    pub anchor: Option<String>,
    /// Other parts, comma-separated, for --anchor
    #[arg(long, value_delimiter = ',')]
    pub parts: Vec<String>,
    /// Parts listed at each end of the scan
    #[arg(long, default_value_t = 25)]
    pub top: usize,
    /// Clears (and players) a part needs in both the hard and easy contexts
    #[arg(long, default_value_t = 100.0)]
    pub min_clears: f64,
    #[arg(long, default_value_t = 40)]
    pub min_players: i64,
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
pub struct SoloArgs {
    /// Build types need this many players in the ranking comparison
    #[arg(long, default_value_t = 3)]
    pub min_players: usize,
    /// Credited build types listed per class
    #[arg(long, default_value_t = 15)]
    pub top: usize,
}

#[derive(Debug, clap::Args)]
pub struct SubclassArgs {
    /// Neutral-game picks per classic subclass
    #[arg(long, default_value_t = 3)]
    pub neutral_per: usize,
    /// Neutral-game Prismatic picks
    #[arg(long, default_value_t = 6)]
    pub prismatic: usize,
    /// Picks per subclass for each activity
    #[arg(long, default_value_t = 1)]
    pub activity_per: usize,
    #[arg(long, default_value_t = QUALITY_WEIGHT)]
    pub quality_weight: f64,
}

#[derive(Debug, clap::Args)]
pub struct CompletenessArgs {
    /// Highlights per set and class in the survey
    #[arg(long, default_value_t = 5)]
    pub count: usize,
    #[arg(long, default_value_t = QUALITY_WEIGHT)]
    pub quality_weight: f64,
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
    /// Survey every set and class: picks under the strict and loosened hard
    /// rules and the soft selection, with their distances, ranks and evidence
    #[arg(long)]
    pub survey: bool,
    /// The survey's strict rules count only build types with this many players
    #[arg(long, default_value_t = 3)]
    pub min_players: usize,
}

pub fn run(args: &Args, db: &Path) -> Result<()> {
    let db = Database::open_read_only(db)?;
    match &args.what {
        What::Priors(a) => run_priors(&db, a, args.format),
        What::Breadth(a) => run_breadth(&db, a, args.format),
        What::Diversity(a) => run_diversity(&db, a, args.format),
        What::Completeness(a) => run_completeness(&db, a, args.format),
        What::Subclasses(a) => run_subclasses(&db, a, args.format),
        What::Solo(a) => run_solo(&db, a, args.format),
        What::Partial => run_partial(&db, args.format),
        What::Sets => run_sets(&db, args.format),
        What::Attempts => run_attempts(&db, args.format),
        What::Triggers => run_triggers(&db, args.format),
        What::Difficulty(a) => run_difficulty(&db, a, args.format),
        What::Escalation(a) => run_escalation(&db, a, args.format),
        What::Highlights(a) => run_highlights(&db, a, args.format),
        What::Model(a) => run_model(&db, a, args.format),
    }
}

/// `analyze model --kits`: each context's kits, with the accessories they
/// are evaluated with and the model's metrics.
#[allow(clippy::many_single_char_names)]
fn list_kits(
    db: &Database,
    o: &focalpoint_sandbox::ontology::Ontology,
    base: &focalpoint_sandbox::rules::RuleBase,
    vars: &focalpoint_sandbox::variables::Variables,
    a: &ModelArgs,
    format: Format,
) -> Result<()> {
    use focalpoint_sandbox::{economy::Economy, scenario::Scenario, validate};
    let e = Economy::load(db, vars)?;
    let all = validate::kits(
        db,
        a.min_clears,
        a.min_players,
        &validate::accessories(base),
    )?;
    for (context, name) in validate::SCENARIOS {
        let s = Scenario::from_context(db, name, vars)?;
        println!("## {context} content, in {name}");
        let header = [
            "kit",
            "accessories",
            "clears",
            "effect",
            "ability kill share",
            "recorded",
            "loop gain",
            "casts a minute",
            "damage resist",
        ]
        .map(String::from);
        let mut rows = Vec::new();
        for k in all.iter().filter(|k| k.context == context) {
            let m = validate::metrics(o, base, vars, &e, &s, &k.build())?;
            let short = |parts: &[String]| {
                parts
                    .iter()
                    .map(|p| p.split_once(':').map_or(p.as_str(), |(_, n)| n))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            rows.push(vec![
                short(&k.parts),
                short(&k.accessories),
                format!("{:.0}", k.clears),
                pct(k.effect.exp_m1()),
                format!("{:.0}%", 100.0 * m[0].exp_m1().min(1.0)),
                k.ability_kill_share
                    .map_or_else(|| "—".to_string(), |v| format!("{:.0}%", 100.0 * v)),
                format!("{:.2}", m[4]),
                format!("{:.1}", m[5]),
                format!("{:.2}", m[3]),
            ]);
        }
        println!("{}", render(format, &header, &rows));
    }
    Ok(())
}

fn run_model(db: &Database, a: &ModelArgs, format: Format) -> Result<()> {
    use focalpoint_sandbox::validate;
    let ontology = focalpoint_sandbox::ontology::Ontology::builtin()?;
    let base = focalpoint_sandbox::builtin_rules()?;
    let mut vars = focalpoint_sandbox::variables::Variables::builtin()?;
    for (k, v) in super::sandbox::pairs(&a.set, "set")? {
        vars.set(&k, v)?;
    }
    if a.kits {
        return list_kits(db, &ontology, &base, &vars, a, format);
    }
    if a.sim {
        let config = focalpoint_sandbox::sim::Config {
            seconds: a.sim_minutes * 60.0,
            runs: a.sim_runs,
            seed: 1,
        };
        for r in validate::validate_sim(
            db,
            &ontology,
            &base,
            &vars,
            a.min_clears,
            a.min_players,
            a.fit,
            config,
        )? {
            println!(
                "## {} content, in {}: {} kits",
                r.context, r.scenario, r.kits
            );
            if let Some(x) = r.fitted_efficiency {
                println!("Calibrated: scenario.ability_efficiency = {x:.3}");
            }
            println!(
                "Share of kills by abilities: simulated {:.0}% (mean over the kits), recorded {:.0}%",
                100.0 * r.mean_simulated,
                100.0 * r.mean_recorded
            );
            println!(
                "Each kit's share against recorded: simulated Pearson {:.2}, Spearman {:.2}; the analysis at the same variables {:.2}, {:.2}",
                r.pearson, r.spearman, r.analysis_pearson, r.analysis_spearman
            );
        }
        return Ok(());
    }
    if a.parts {
        let checks = validate::part_checks(db, &ontology, &base, &vars, 150.0, 20, a.max_kits)?;
        println!(
            "## {} modelled parts with a measured lift (`analyze escalation`): predicted change against measured lift",
            checks.len()
        );
        let header = [
            "metric",
            "hard: rank correlation",
            "hard minus easy: rank correlation",
        ]
        .map(String::from);
        let (sp, ss, sn) = validate::share_correlation(&checks);
        println!(
            "Predicted change in the share of kills by abilities against the recorded difference with and without the part (hard content, {sn} parts): Pearson {sp:.2}, Spearman {ss:.2}"
        );
        let rows: Vec<Vec<String>> = validate::part_correlations(&checks)
            .into_iter()
            .map(|(m, h, d)| vec![m.to_string(), format!("{h:+.2}"), format!("{d:+.2}")])
            .collect();
        println!("{}", render(format, &header, &rows));
        println!("## Parts by measured hard-content lift");
        let header = [
            "subclass",
            "part",
            "kits",
            "lift hard",
            "lift easy",
            "Δ ability kill share hard: recorded",
            "model",
            "Δ loop gain hard",
            "Δ damage resist hard",
        ]
        .map(String::from);
        let mut sorted = checks.clone();
        sorted.sort_by(|a, b| b.measured_hard.total_cmp(&a.measured_hard));
        let rows: Vec<Vec<String>> = sorted
            .iter()
            .map(|c| {
                vec![
                    c.subclass.clone(),
                    c.part.clone(),
                    c.kits.to_string(),
                    pct(c.measured_hard.exp_m1()),
                    pct(c.measured_easy.exp_m1()),
                    c.measured_share_hard
                        .map_or_else(|| "—".to_string(), |v| format!("{:+.1} pts", 100.0 * v)),
                    format!("{:+.1} pts", 100.0 * c.predicted_share_hard),
                    format!("{:+.2}", c.hard[4]),
                    format!("{:+.3}", c.hard[3]),
                ]
            })
            .collect();
        println!("{}", render(format, &header, &rows));
        return Ok(());
    }
    let reports = validate::validate(
        db,
        &ontology,
        &base,
        &vars,
        a.min_clears,
        a.min_players,
        a.fit,
    )?;
    for r in &reports {
        println!(
            "## {} content, in {}: {} kits ({:.0}% of their parts modelled)",
            r.context,
            r.scenario,
            r.kits,
            100.0 * r.modelled_share
        );
        println!(
            "Held-out prediction of the kits' effects: Pearson {:.2}, Spearman {:.2}",
            r.held_out_pearson, r.held_out_spearman
        );
        if let Some(x) = r.fitted_efficiency {
            println!("Calibrated: scenario.ability_efficiency = {x:.3}");
        }
        println!(
            "Share of kills by abilities: model {:.0}% (mean over the kits), recorded {} (the kits), {} (the context's median player)",
            100.0 * r.model_ability_kill_share,
            r.kits_recorded_kill_share
                .map_or_else(|| "—".to_string(), |v| format!("{:.0}%", 100.0 * v)),
            r.recorded_ability_kill_share
                .map_or_else(|| "—".to_string(), |v| format!("{:.0}%", 100.0 * v))
        );
        println!(
            "Each kit's share, model against recorded: Pearson {:.2}, Spearman {:.2}",
            r.kill_share_pearson, r.kill_share_spearman
        );
        let header = [
            "metric",
            "rank correlation with effect",
            "weight (standardised)",
        ]
        .map(String::from);
        let rows: Vec<Vec<String>> = r
            .correlations
            .iter()
            .zip(&r.weights)
            .map(|((m, c), (_, w))| vec![(*m).to_string(), format!("{c:+.2}"), format!("{w:+.4}")])
            .collect();
        println!("{}", render(format, &header, &rows));
    }
    Ok(())
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
    if a.survey {
        return run_survey(db, a, format);
    }
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

fn quantile(mut xs: Vec<f64>, q: f64) -> f64 {
    xs.sort_by(f64::total_cmp);
    if xs.is_empty() {
        return f64::NAN;
    }
    xs[((xs.len() - 1) as f64 * q).round() as usize]
}

/// The survey of every set and class, among candidates `keep` accepts.
fn survey_rows(
    data: &SiteData,
    vectors: &HashMap<i64, ElementVector>,
    a: &HighlightArgs,
    keep: &dyn Fn(i64) -> bool,
) -> Vec<hl::SurveyRow> {
    let mut sets = vec![NEUTRAL.to_string()];
    sets.extend(
        data.scopes
            .iter()
            .filter(|s| s.kind != "all")
            .map(|s| s.id.clone()),
    );
    let mut rows = Vec::new();
    for set in &sets {
        for class in highlight::classes(data, set) {
            rows.push(hl::survey(
                data,
                vectors,
                set,
                &class,
                a.count,
                a.quality_weight,
                a.min_players,
                keep,
            ));
        }
    }
    rows
}

fn run_survey(db: &Database, a: &HighlightArgs, format: Format) -> Result<()> {
    let data = site(db, None, a.context_prior)?;
    let vectors = build_vectors(db)?;
    let rows = survey_rows(&data, &vectors, a, &|id| !data.incomplete.contains(&id));
    let k = a.count;
    println!(
        "{k} picks per set and class. Hard rules: no shared exotic, kit distance ≥ 0.7, best-ranked first — \
strict: top half of build types with ≥ {} players; loose: every build type. Soft: quality weight {}.\n",
        a.min_players, a.quality_weight
    );
    let header = [
        "set",
        "class",
        "build types",
        format!("≥ {} players", a.min_players).as_str(),
        "strict",
        "loose (worst rank)",
        "soft: min kit",
        "shared exotics",
        "min distance",
        "effect (top)",
        "worst rank",
        "1 player",
    ]
    .map(String::from);
    let table: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            let s = &r.soft;
            vec![
                r.set.clone(),
                r.class.clone(),
                r.candidates.to_string(),
                r.established.to_string(),
                r.hard_strict.to_string(),
                format!("{} ({})", r.hard_loose, r.hard_loose_worst_rank),
                format!("{:.2}", s.min_kit),
                s.shared_exotic_pairs.to_string(),
                format!("{:.2}", s.min_distance),
                format!("{} ({})", pct(s.mean_quality), pct(s.top_quality)),
                s.worst_rank.to_string(),
                format!("{}/{}", s.one_player, s.picks),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &table));
    survey_summary(&rows, k);
    Ok(())
}

/// The survey's summary lines.
fn survey_summary(rows: &[hl::SurveyRow], k: usize) {
    let total = rows.len();
    let full = |f: &dyn Fn(&hl::SurveyRow) -> usize| rows.iter().filter(|r| f(r) >= k).count();
    let filled: Vec<&hl::SurveyRow> = rows.iter().filter(|r| r.soft.picks >= k).collect();
    let meets = filled
        .iter()
        .filter(|r| r.soft.min_kit >= 0.7 && r.soft.shared_exotic_pairs == 0)
        .count();
    let picks: usize = rows.iter().map(|r| r.soft.picks).sum();
    let one_player: usize = rows.iter().map(|r| r.soft.one_player).sum();
    let one_run: usize = rows.iter().map(|r| r.soft.one_run).sum();
    let kits: Vec<f64> = filled.iter().map(|r| r.soft.min_kit).collect();
    let dists: Vec<f64> = filled.iter().map(|r| r.soft.min_distance).collect();
    let cost: Vec<f64> = filled
        .iter()
        .map(|r| 100.0 * (r.soft.top_quality.exp() - r.soft.mean_quality.exp()))
        .collect();
    println!(
        "Sets × classes: {total}. Filled to {k} picks: strict hard rules {}, loosened hard rules {}, soft {}.",
        full(&|r| r.hard_strict),
        full(&|r| r.hard_loose),
        filled.len()
    );
    println!(
        "Soft picks meeting the hard rule anyway (no shared exotic, kit ≥ 0.7 between every pair): {meets} of {} sets.",
        filled.len()
    );
    println!(
        "Smallest kit distance between two soft picks, per set: p10 {:.2} · median {:.2} · p90 {:.2}; smallest playstyle distance: p10 {:.2} · median {:.2}.",
        quantile(kits.clone(), 0.1),
        quantile(kits.clone(), 0.5),
        quantile(kits, 0.9),
        quantile(dists.clone(), 0.1),
        quantile(dists, 0.5)
    );
    println!(
        "Mean effect given up against the plain top {k}: median {:.1} points, p90 {:.1}.",
        quantile(cost.clone(), 0.5),
        quantile(cost, 0.9)
    );
    println!(
        "Soft picks one player used: {one_player} of {picks} ({:.0}%); used in a single run: {one_run} ({:.0}%).",
        100.0 * one_player as f64 / picks.max(1) as f64,
        100.0 * one_run as f64 / picks.max(1) as f64
    );
    let share = |f: &dyn Fn(&hl::SurveyRow) -> (usize, usize)| {
        let (a, b) = rows
            .iter()
            .map(f)
            .fold((0, 0), |(x, y), (p, q)| (x + p, y + q));
        100.0 * a as f64 / b.max(1) as f64
    };
    println!(
        "One-player picks elsewhere: plain top {k} {:.0}%, loosened hard rules {:.0}%.",
        share(&|r| (r.top_one_player, r.candidates.min(k))),
        share(&|r| (r.hard_loose_one_player, r.hard_loose)),
    );
    let worst = |f: &dyn Fn(&hl::SurveyRow) -> usize| {
        quantile(
            rows.iter()
                .filter(|r| r.candidates >= 50)
                .map(|r| f(r) as f64)
                .collect(),
            0.5,
        )
    };
    println!(
        "Median worst rank of a pick, sets with ≥ 50 build types: loosened hard rules {:.0}, soft {:.0}.",
        worst(&|r| r.hard_loose_worst_rank),
        worst(&|r| r.soft.worst_rank),
    );
}

fn run_completeness(db: &Database, a: &CompletenessArgs, format: Format) -> Result<()> {
    let parts = completeness::parts(db)?;
    println!(
        "## Builds by aspects and fragments ({} builds)",
        thousands(parts.len() as i64)
    );
    let header = [
        "aspects",
        "fragments",
        "builds",
        "share",
        "no armor mods",
        "no artifact perks",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = completeness::distribution(&parts)
        .into_iter()
        .map(|(asp, frag, n, no_mods, no_art)| {
            vec![
                asp.to_string(),
                if frag >= 6 {
                    "6+".into()
                } else {
                    frag.to_string()
                },
                thousands(n as i64),
                format!("{:.2}%", 100.0 * n as f64 / parts.len() as f64),
                format!("{no_mods} ({:.0}%)", 100.0 * no_mods as f64 / n as f64),
                format!("{no_art} ({:.0}%)", 100.0 * no_art as f64 / n as f64),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));

    println!("\n## Two-aspect builds against their aspect pair's usual fragment count");
    let gaps = completeness::fragment_gaps(db)?;
    if let Some(&(_, _, _, least)) = gaps.first() {
        println!("The smallest usual fragment count of any aspect pair: {least}.");
    }
    let header = ["fragments", "at least the pair's usual", "fewer"].map(String::from);
    let rows: Vec<Vec<String>> = gaps
        .iter()
        .map(|&(f, ok, short, _)| {
            vec![f.to_string(), thousands(ok as i64), thousands(short as i64)]
        })
        .collect();
    println!("{}", render(format, &header, &rows));

    println!(
        "\n## Evidence (complete: two aspects, ≥ {} fragments, four abilities, armor mods)",
        completeness::MIN_FRAGMENTS
    );
    let (complete, incomplete) = completeness::evidence(db, &parts)?;
    let header = [
        "builds",
        "count",
        "used in scored runs",
        "one player",
        "one run",
        "runs per build",
        "most players",
    ]
    .map(String::from);
    let row = |name: &str, e: &completeness::Evidence| {
        vec![
            name.to_string(),
            thousands(e.builds as i64),
            thousands(e.used as i64),
            format!("{:.0}%", 100.0 * e.one_player as f64 / e.used.max(1) as f64),
            format!("{:.0}%", 100.0 * e.one_run as f64 / e.used.max(1) as f64),
            format!("{:.2}", e.runs as f64 / e.used.max(1) as f64),
            e.most_players.to_string(),
        ]
    };
    println!(
        "{}",
        render(
            format,
            &header,
            &[row("complete", &complete), row("incomplete", &incomplete)]
        )
    );

    let data = site(db, None, NEUTRAL_CONTEXT_PRIOR)?;
    let vectors = build_vectors(db)?;
    // The ranked builds view: listings whose top build is incomplete.
    let is_complete = |id: i64| parts.get(&id).is_some_and(completeness::Parts::complete);
    let exemplar =
        |r: &focalpoint_db::site::Row| data.recipes[r.7.0 as usize].id.parse::<i64>().ok();
    let (mut listings, mut incomplete_tops, mut players) = (0_usize, 0_usize, Vec::new());
    for r in data.scopes.iter().flat_map(|s| &s.rows) {
        listings += 1;
        if exemplar(r).is_some_and(|id| !is_complete(id)) {
            incomplete_tops += 1;
            players.push(r.4);
        }
    }
    let types_without_aspects = data.types.iter().filter(|t| t.aspects.is_empty()).count();
    println!(
        "\n## Ranked builds\nListings whose top build is incomplete: {incomplete_tops} of {} ({:.1}%), across every scope; \
their build types had {} player(s) at most. Build types with no aspects at all: {types_without_aspects} of {}.",
        thousands(listings as i64),
        100.0 * incomplete_tops as f64 / listings.max(1) as f64,
        players.iter().max().copied().unwrap_or(0),
        thousands(data.types.len() as i64)
    );
    let args = HighlightArgs {
        set: NEUTRAL.into(),
        class: None,
        count: a.count,
        quality_weight: a.quality_weight,
        context_prior: NEUTRAL_CONTEXT_PRIOR,
        sweep: false,
        weights: Vec::new(),
        sets: false,
        survey: true,
        min_players: 3,
    };
    let before = survey_rows(&data, &vectors, &args, &|_| true);
    let incomplete_picks: usize = before
        .iter()
        .map(|r| r.soft_builds.iter().filter(|&&id| !is_complete(id)).count())
        .sum();
    let sets_hit = before
        .iter()
        .filter(|r| r.soft_builds.iter().any(|&id| !is_complete(id)))
        .count();
    let picks: usize = before.iter().map(|r| r.soft.picks).sum();
    println!(
        "\n## Highlights ({} per set and class)\nPicks that would be incomplete if every build were a candidate: {incomplete_picks} of {picks}, in {sets_hit} of {} sets × classes.",
        a.count,
        before.len()
    );
    // Each incomplete pick with its build type's evidence and what it lacks.
    let mut listing: HashMap<(&str, i64), (u32, usize, f64)> = HashMap::new();
    for scope in &data.scopes {
        let set = if scope.id == "all" {
            NEUTRAL
        } else {
            scope.id.as_str()
        };
        for r in &scope.rows {
            if let Ok(id) = data.recipes[r.7.0 as usize].id.parse::<i64>() {
                listing.insert((set, id), (r.0, r.4, r.5));
            }
        }
    }
    let header = [
        "set",
        "class",
        "players",
        "runs",
        "aspects",
        "fragments",
        "armor mods",
        "build type",
    ]
    .map(String::from);
    let mut rows = Vec::new();
    for r in &before {
        for &id in r.soft_builds.iter().filter(|&&id| !is_complete(id)) {
            let p = parts.get(&id).copied().unwrap_or_default();
            let (t, players, runs) = listing
                .get(&(r.set.as_str(), id))
                .copied()
                .unwrap_or((0, 0, 0.0));
            rows.push(vec![
                r.set.clone(),
                r.class.clone(),
                players.to_string(),
                format!("{runs:.0}"),
                p.aspects.to_string(),
                p.fragments.to_string(),
                p.armor_mods.to_string(),
                label(&data, t),
            ]);
        }
    }
    println!("{}", render(format, &header, &rows));

    println!("\nWith incomplete builds left out:");
    let after = survey_rows(&data, &vectors, &args, &is_complete);
    survey_summary(&after, a.count);
    println!("\nFor comparison, with every build:");
    survey_summary(&before, a.count);
    Ok(())
}

fn run_subclasses(db: &Database, a: &SubclassArgs, format: Format) -> Result<()> {
    let data = site(db, None, NEUTRAL_CONTEXT_PRIOR)?;
    let vectors = build_vectors(db)?;
    let groups = subclasses::groups_used(&data);
    let n_groups = data.scopes.iter().filter(|s| s.kind == "group").count();
    let neutral_quota = |s: &str| {
        if s == "Prismatic" {
            a.prismatic
        } else {
            a.neutral_per
        }
    };
    let element = |t: u32| data.types[t as usize].element.clone().unwrap_or_default();

    println!(
        "# Neutral game: {} per classic subclass, {} Prismatic (complete builds; quality weight {})\n",
        a.neutral_per, a.prismatic, a.quality_weight
    );
    let (mut total, mut positive, mut two_players, mut two_groups) =
        (0_usize, 0_usize, 0_usize, 0_usize);
    for class in highlight::classes(&data, NEUTRAL) {
        let picks = subclasses::by_subclass(
            &data,
            &vectors,
            &groups,
            NEUTRAL,
            &class,
            &neutral_quota,
            a.quality_weight,
        );
        let quota_total: usize = picks.iter().map(|p| p.picks.len()).sum();
        println!("## {class}: {quota_total} picks");
        let header = [
            "subclass",
            "#",
            "rank in subclass",
            "class rank",
            "neutral effect",
            "players",
            "runs",
            "activity groups",
            "build type",
        ]
        .map(String::from);
        let mut rows = Vec::new();
        for sp in &picks {
            for (k, p) in sp.picks.iter().enumerate() {
                total += 1;
                positive += usize::from(p.quality > 0.0);
                two_players += usize::from(p.players >= 2);
                two_groups += usize::from(p.groups >= 2);
                rows.push(vec![
                    sp.subclass.to_string(),
                    (k + 1).to_string(),
                    format!("{} of {}", p.rank, sp.candidates),
                    p.class_rank.to_string(),
                    pct(p.quality),
                    p.players.to_string(),
                    format!("{:.0}", p.runs),
                    format!("{} of {n_groups}", p.groups),
                    label(&data, p.type_id),
                ]);
            }
        }
        println!("{}", render(format, &header, &rows));
        let header = [
            "subclass",
            "candidates",
            "picks",
            "min kit",
            "min distance",
            "supers",
            "aspect pairs",
            "shared exotics",
        ]
        .map(String::from);
        let rows: Vec<Vec<String>> = picks
            .iter()
            .map(|sp| {
                vec![
                    sp.subclass.to_string(),
                    sp.candidates.to_string(),
                    sp.picks.len().to_string(),
                    format!("{:.2}", sp.min_kit),
                    format!("{:.2}", sp.min_distance),
                    sp.supers.to_string(),
                    sp.aspect_pairs.to_string(),
                    sp.shared_exotic_pairs.to_string(),
                ]
            })
            .collect();
        println!("{}", render(format, &header, &rows));
        // How the plain ranking and the unquoted selection spread over subclasses.
        let cands = candidates(&data, NEUTRAL, &class);
        let spread = |idx: &mut dyn Iterator<Item = usize>| {
            let mut counts: HashMap<String, usize> = HashMap::new();
            for i in idx {
                *counts.entry(element(cands[i].type_id)).or_default() += 1;
            }
            subclasses::SUBCLASSES
                .iter()
                .map(|s| format!("{s} {}", counts.get(*s).copied().unwrap_or(0)))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        let styles = playstyles(&cands, &vectors);
        let soft: Vec<usize> = select(&cands, &styles, quota_total, a.quality_weight)
            .iter()
            .map(|p| p.index)
            .collect();
        println!(
            "Without a quota, by subclass — plain top {quota_total}: {}; soft {quota_total}: {}.\n",
            spread(&mut (0..quota_total.min(cands.len()))),
            spread(&mut soft.iter().copied())
        );
    }
    println!(
        "Neutral picks: {total}; neutral effect above zero {positive}; ≥ 2 players {two_players}; used in ≥ 2 activity groups {two_groups}.\n"
    );

    println!("# Activities: {} per subclass\n", a.activity_per);
    let header = [
        "set",
        "class",
        "subclasses",
        "picks (effect)",
        "mean (soft top)",
        "below zero",
        "1 player",
    ]
    .map(String::from);
    let mut rows = Vec::new();
    let (mut sets, mut full, mut picks_total, mut below, mut one_player) =
        (0_usize, 0_usize, 0_usize, 0_usize, 0_usize);
    let (mut soft_below, mut soft_one, mut soft_total) = (0_usize, 0_usize, 0_usize);
    let mut available: HashMap<usize, usize> = HashMap::new();
    let mut costs = Vec::new();
    for scope in data.scopes.iter().filter(|s| s.kind != "all") {
        for class in highlight::classes(&data, &scope.id) {
            let picks = subclasses::by_subclass(
                &data,
                &vectors,
                &groups,
                &scope.id,
                &class,
                &|_| a.activity_per,
                a.quality_weight,
            );
            let with: Vec<&subclasses::SubclassPicks> =
                picks.iter().filter(|p| !p.picks.is_empty()).collect();
            let chosen: Vec<&subclasses::SubclassPick> =
                with.iter().flat_map(|p| &p.picks).collect();
            if chosen.is_empty() {
                continue;
            }
            sets += 1;
            *available.entry(with.len()).or_default() += 1;
            full += usize::from(with.len() == subclasses::SUBCLASSES.len());
            picks_total += chosen.len();
            let b = chosen.iter().filter(|p| p.quality < 0.0).count();
            let o = chosen.iter().filter(|p| p.players <= 1).count();
            below += b;
            one_player += o;
            let mean = chosen.iter().map(|p| p.quality).sum::<f64>() / chosen.len() as f64;
            // The same number of picks without the quota.
            let cands = candidates(&data, &scope.id, &class);
            let styles = playstyles(&cands, &vectors);
            let soft: Vec<usize> = select(&cands, &styles, chosen.len(), a.quality_weight)
                .iter()
                .map(|p| p.index)
                .collect();
            let soft_mean =
                soft.iter().map(|&i| cands[i].quality).sum::<f64>() / soft.len().max(1) as f64;
            soft_total += soft.len();
            soft_below += soft.iter().filter(|&&i| cands[i].quality < 0.0).count();
            let scope_rows = &scope.rows;
            soft_one += soft
                .iter()
                .filter(|&&i| scope_rows[cands[i].row].4 <= 1)
                .count();
            costs.push(100.0 * (soft_mean.exp() - mean.exp()));
            rows.push(vec![
                scope.name.clone(),
                class,
                format!("{}/6", with.len()),
                with.iter()
                    .map(|p| format!("{} {}", p.subclass, pct(p.picks[0].quality)))
                    .collect::<Vec<_>>()
                    .join(" · "),
                format!("{} ({})", pct(mean), pct(soft_mean)),
                b.to_string(),
                format!("{o}/{}", chosen.len()),
            ]);
        }
    }
    println!("{}", render(format, &header, &rows));
    let mut avail: Vec<_> = available.into_iter().collect();
    avail.sort_unstable();
    println!(
        "Sets × classes: {sets}; with every subclass: {full}; subclasses available: {}.",
        avail
            .iter()
            .map(|(k, n)| format!("{k}: {n}"))
            .collect::<Vec<_>>()
            .join(" · ")
    );
    println!(
        "Picks below zero: {below} of {picks_total} ({:.0}%) — without the quota {soft_below} of {soft_total} ({:.0}%). \
One-player picks: {:.0}% — without the quota {:.0}%.",
        100.0 * below as f64 / picks_total.max(1) as f64,
        100.0 * soft_below as f64 / soft_total.max(1) as f64,
        100.0 * one_player as f64 / picks_total.max(1) as f64,
        100.0 * soft_one as f64 / soft_total.max(1) as f64
    );
    println!(
        "Mean effect given up against the same number of picks without the quota: median {:.1} points, p90 {:.1}.",
        quantile(costs.clone(), 0.5),
        quantile(costs, 0.9)
    );
    Ok(())
}

/// "Exotic armor + exotic weapon · Element Super · aspects" from a core key.
fn key_label(key: &[String]) -> String {
    let none = |s: &str, what: &str| {
        if s == "—" {
            format!("no exotic {what}")
        } else {
            s.to_string()
        }
    };
    format!(
        "{} + {} · {} {} · {}",
        none(&key[2], "armor"),
        none(&key[3], "weapon"),
        key[1],
        key[4],
        key[5]
    )
}

fn run_solo(db: &Database, a: &SoloArgs, format: Format) -> Result<()> {
    let share = |x: f64| format!("{:.1}%", 100.0 * x);
    println!("## How Searchlight qualified and captured runs");
    let header = [
        "activity group",
        "runs",
        "qualified",
        "solo: qualified",
        "solo: unqualified",
        "qualified solo, fresh",
        "roster p99",
        "build captured: solo clears",
        "fireteam clears",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = solo::qualification(db)?
        .into_iter()
        .map(|q| {
            vec![
                q.group,
                thousands(q.runs as i64),
                thousands(q.qualified as i64),
                share(q.solo_qualified),
                share(q.solo_unqualified),
                share(q.fresh_solo),
                q.roster_p99.to_string(),
                share(q.build_solo),
                share(q.build_fireteam),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    println!(
        "Runs are attempts; a group whose qualified runs are all solo while its unqualified runs are not was gated \
to solo runs. Build capture is per scored clear."
    );

    println!("\n## Scored clears by activity group");
    let header = [
        "activity group",
        "clears",
        "solo clears",
        "solo share",
        "solo players",
        "solo with a build",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = solo::volume(db)?
        .into_iter()
        .map(|v| {
            vec![
                v.group,
                thousands(v.clears as i64),
                thousands(v.solo_clears as i64),
                format!(
                    "{:.1}%",
                    100.0 * v.solo_clears as f64 / v.clears.max(1) as f64
                ),
                thousands(v.solo_players as i64),
                thousands(v.solo_build_runs as i64),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));

    println!(
        "\n## Solo clears that started from the beginning, against the fireteam clears of the same activity (medians)"
    );
    let header = [
        "activity group",
        "fresh solo clears",
        "all solo clears",
        "minutes",
        "kill index",
        "speed",
        "survival",
        "premium (quartiles)",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = solo::premium(db)?
        .into_iter()
        .map(|p| {
            vec![
                p.group,
                thousands(p.solo_clears as i64),
                thousands(p.all_solo_clears as i64),
                format!("{:.0}", p.duration / 60.0),
                format!("{:.2}", p.kill_index),
                format!("{:.2}", p.speed),
                format!("{:.2}", p.survival),
                format!("×{:.2} (×{:.2}–×{:.2})", p.credit, p.credit_q1, p.credit_q3),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    println!(
        "Premium: how much better a solo clear scores against the fireteam clears than against other solo clears \
(Searchlight used a flat ×2). The fireteam clears are the qualified ones, or unqualified ones where too few qualified."
    );

    println!(
        "\n## Do solo clears look like full clears? (activities with ≥ 20 solo clears; minutes)"
    );
    let header = [
        "activity group",
        "activity",
        "mode",
        "solo clears",
        "solo median",
        "solo p10",
        "fastest",
        "fireteam median",
        "fireteam p1",
        "solo faster than fireteam p1",
        "kills: solo",
        "fireteam (all players)",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = solo::clear_checks(db, 20)?
        .into_iter()
        .map(|c| {
            vec![
                c.group,
                c.activity,
                c.mode.map_or_else(
                    || "—".into(),
                    |m| {
                        if m == i64::from(focalpoint_db::rank::RAID_MODE) {
                            "raid".into()
                        } else {
                            m.to_string()
                        }
                    },
                ),
                thousands(c.solo_clears as i64),
                format!("{:.0}", c.solo_median),
                format!("{:.0}", c.solo_p10),
                format!("{:.0}", c.solo_fastest),
                format!("{:.0}", c.fireteam_median),
                format!("{:.0}", c.fireteam_p1),
                format!("{:.0}%", 100.0 * c.solo_faster_than_p1),
                format!("{:.0}", c.solo_kills),
                format!("{:.0}", c.team_kills),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    println!(
        "Raids get no premium: their solo clears are often faster than the fastest fireteams (Root of Nightmares Master \
below the 24-minute Normal solo world record) with a fraction of a fireteam's kills."
    );

    println!("\n## Per-activity solo premium as credited (never below ×1; none for raids)");
    let header = [
        "activity group",
        "activity",
        "fresh solo clears",
        "fireteam reference",
        "kill index",
        "credited",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = solo::credits(db)?
        .into_iter()
        .filter(|c| c.solo_clears >= 20)
        .map(|c| {
            vec![
                c.group,
                c.activity,
                thousands(c.solo_clears as i64),
                if c.from_qualified {
                    "qualified"
                } else {
                    "unqualified"
                }
                .to_string(),
                format!("{:.2}", c.kill_index),
                format!("×{:.2}", c.credited),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    println!("(Activities with at least 20 fresh solo clears.)\n");

    eprintln!("ranking build types three ways…");
    let per_run = solo::shifts(db, SoloScoring::AgainstFireteam)?;
    for c in solo::by_class(&per_run, a.min_players) {
        println!(
            "Every solo clear against the fireteam reference — {}: rank correlation {:+.2}, top 10% in common {} of {}.",
            c.class, c.correlation, c.top_overlap.0, c.top_overlap.1
        );
    }
    let shifts = solo::shifts(db, SoloScoring::ActivityCredit)?;
    let with_solo = shifts.iter().filter(|s| s.solo_runs > 0.0).count();
    println!(
        "\n## The all-activities ranking with the per-activity solo premium\n{} build types; {with_solo} have a solo clear.",
        thousands(shifts.len() as i64)
    );
    for c in solo::by_class(&shifts, a.min_players) {
        println!(
            "\n### {}: {} build types with ≥ {} players\nRank correlation with today's ranking {:+.2}; top 10% in common {} of {}; \
with a solo clear: {:.0}% of the credited top 10%, {:.0}% of the rest.",
            c.class,
            c.types.len(),
            a.min_players,
            c.correlation,
            c.top_overlap.0,
            c.top_overlap.1,
            100.0 * c.top_with_solo,
            100.0 * c.rest_with_solo
        );
        let header = [
            "rank",
            "before",
            "credited",
            "today",
            "players",
            "solo runs",
            "solo players",
            "build type",
        ]
        .map(String::from);
        let rows: Vec<Vec<String>> = c
            .types
            .iter()
            .zip(&c.rank_before)
            .take(a.top)
            .enumerate()
            .map(|(i, (t, before))| {
                vec![
                    (i + 1).to_string(),
                    before.to_string(),
                    pct(t.credited),
                    pct(t.effect),
                    t.players.to_string(),
                    format!("{:.0}", t.solo_runs),
                    t.solo_players.to_string(),
                    key_label(&t.key),
                ]
            })
            .collect();
        println!("{}", render(format, &header, &rows));
    }
    Ok(())
}

fn run_partial(db: &Database, format: Format) -> Result<()> {
    let share = |x: f64| format!("{:.2}%", 100.0 * x);
    println!("## Checkpoints and late joins (every recorded run)");
    let header = [
        "activity group",
        "activities",
        "runs",
        "not from the beginning",
        "joined late",
        "with checkpoints",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = partial::resumption(db)?
        .into_iter()
        .map(|r| {
            vec![
                r.group,
                r.activities.to_string(),
                thousands(r.runs as i64),
                share(r.not_fresh),
                share(r.late_join),
                format!("{} of {}", r.with_checkpoints, r.activities),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    println!(
        "An activity has checkpoints when at least {}% of its runs did not start from the beginning; a clear of one \
without (Conquests, Ops) is always a full clear.",
        100.0 * focalpoint_db::rank::CHECKPOINT_SHARE
    );

    println!("\n## Kill rates of scored clears (medians)");
    let header = [
        "activity group",
        "clears",
        "kind",
        "kills per minute played",
        "per life",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = partial::pace(db)?
        .into_iter()
        .map(|p| {
            vec![
                p.group,
                thousands(p.clears as i64),
                p.kind.to_string(),
                format!("{:.1}", p.kills_per_minute),
                format!("{:.1}", p.per_life),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));

    println!("\n## Fireteam clears by the share of the run the player was present for");
    let header = [
        "present for",
        "scored as",
        "clears",
        "score",
        "whole-run score",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = partial::presence(db)?
        .into_iter()
        .map(|p| {
            vec![
                if p.partial {
                    format!("{:.0}–{:.0}%", 100.0 * p.share, 100.0 * p.share + 10.0)
                } else {
                    format!("{:.0}% or more", 100.0 * p.share)
                },
                if p.partial {
                    "partial participant"
                } else {
                    "fireteam member"
                }
                .to_string(),
                thousands(p.clears as i64),
                format!("{:.2}", p.score),
                format!("{:.2}", p.whole_run),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    println!(
        "Scores are geometric means against the player's bucket: 1 is typical. Partial participants are scored by \
kill rate over time played against other partial participants (ADR 0009); the whole-run score measures their kills \
and the run's pace against the fireteam reference."
    );
    Ok(())
}

fn run_sets(db: &Database, format: Format) -> Result<()> {
    let share = |x: f64| format!("{:.1}%", 100.0 * x);
    println!("## Recorded scored clears by their main build's set bonuses");
    let header = ["set bonuses", "clears", "share", "mean score"].map(String::from);
    let rows: Vec<Vec<String>> = sets::prevalence(db)?
        .into_iter()
        .map(|p| {
            vec![
                if p.bonuses >= POLISHED_SET_BONUSES {
                    format!("{} or more (polished)", p.bonuses)
                } else {
                    format!("{} (lesser)", p.bonuses)
                },
                thousands(p.clears as i64),
                share(p.share),
                pct(p.mean_perf),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    println!("Mean scores include the players' skill: compare players with themselves below.");

    println!("\n## Full set bonuses for the same player in the same activity");
    let g = sets::gain(db)?;
    let header = [
        "player-activities",
        "gain",
        "± s.e.",
        "median",
        "polished clears later by",
        "gain vs lateness",
    ]
    .map(String::from);
    let rows = vec![vec![
        thousands(g.pairs as i64),
        pct(g.mean),
        format!("{:.3}", g.se),
        pct(g.median),
        format!("{:.1} days", g.later_days),
        format!("r = {:+.2}", g.lateness_correlation),
    ]];
    println!("{}", render(format, &header, &rows));
    println!(
        "Per player and activity with both kinds of clears, polished minus lesser mean score. Learning over time \
would make the gain grow with lateness."
    );

    println!("\n## Players' level by how often their recorded clears ran lesser builds");
    let header = ["recorded clears", "lesser", "players", "level", "± s.e."].map(String::from);
    let rows: Vec<Vec<String>> = sets::players(db)?
        .into_iter()
        .map(|p| {
            vec![
                match p.recorded_from {
                    1 => "1".to_string(),
                    2 => "2–4".to_string(),
                    5 => "5–9".to_string(),
                    n => format!("{n}+"),
                },
                p.habit.label().to_string(),
                thousands(p.players as i64),
                pct(p.mean_perf),
                format!("{:.3}", p.se),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    println!(
        "Level: mean score of all the player's scored clears, recorded or not. Compare within a band: players \
with more recorded clears are stronger whatever their habit."
    );

    let data = site(db, Some(vec!["all".into()]), NEUTRAL_CONTEXT_PRIOR)?;
    let polished = polished_builds(db)?;
    println!(
        "\n## All-activities listings whose top build is lesser (the type has no polished build)"
    );
    let header = ["listings", "count", "lesser top builds", "in the top 10%"].map(String::from);
    let rows: Vec<Vec<String>> = sets::top_builds(&data, &polished)
        .into_iter()
        .map(|t| {
            vec![
                if t.core {
                    format!("{}+ players", focalpoint_db::site::CORE_PLAYERS)
                } else {
                    format!("1–{} players", focalpoint_db::site::CORE_PLAYERS - 1)
                },
                thousands(t.listings as i64),
                format!(
                    "{} ({})",
                    thousands(t.lesser as i64),
                    share(t.lesser as f64 / t.listings.max(1) as f64)
                ),
                thousands(t.lesser_in_top_tenth as i64),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    Ok(())
}

fn run_attempts(db: &Database, format: Format) -> Result<()> {
    let share = |x: f64| format!("{:.1}%", 100.0 * x);
    let opt_share = |x: Option<f64>| x.map_or("—".to_string(), share);
    println!("## Recorded runs by activity group (players with a recorded build)");
    let header = [
        "activity group",
        "checkpoints",
        "runs",
        "cleared",
        "uncleared: median min",
        "uncleared under 2 min",
        "cleared: median min",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = attempts::by_group(db)?
        .into_iter()
        .map(|g| {
            vec![
                g.group,
                if g.resumable { "yes" } else { "no" }.to_string(),
                thousands(g.runs as i64),
                share(g.cleared),
                format!("{:.1}", g.uncleared_minutes),
                share(g.uncleared_short),
                g.cleared_minutes.map_or("—".into(), |m| format!("{m:.1}")),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    println!(
        "Where runs resume from checkpoints, an uncleared run is mostly a session of a longer clear; where they cannot \
(Conquests, Ops), it is a failed attempt. Runs under {} s are bail-outs, not tries.",
        attempts::MIN_ATTEMPT_S
    );

    println!("\n## Tries per player and activity, where runs cannot be resumed");
    let header = [
        "activity group",
        "player-activities",
        "tries each",
        "ever cleared",
        "ended on the clear",
        "1st try clears",
        "2nd",
        "3rd",
        "4th+",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = attempts::sequences(db)?
        .into_iter()
        .map(|s| {
            let [a, b, c, d] = s.by_try;
            vec![
                s.group,
                thousands(s.sequences as i64),
                format!("{:.1}", s.mean_tries),
                share(s.ever_cleared),
                share(s.end_on_clear),
                opt_share(a),
                opt_share(b),
                opt_share(c),
                opt_share(d),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    println!(
        "Clear rates by try count tries up to the player's first clear. Players stop when they clear, so comparing a \
player's tries credits whatever build they switched to last; first tries are free of that."
    );

    let (firsts, types) = attempts::first_attempts(db)?;
    println!("\n## First tries: clear rate by build type against players of the same level");
    let header = [
        "activity group",
        "first tries",
        "cleared",
        "with a level",
        "top / bottom level decile",
        "build types",
        "split-half r",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = firsts
        .into_iter()
        .map(|f| {
            vec![
                f.group,
                thousands(f.attempts as i64),
                share(f.clear_rate),
                share(f.with_level),
                format!(
                    "{} / {}",
                    opt_share(f.top_decile),
                    opt_share(f.bottom_decile)
                ),
                f.reliable_types.to_string(),
                if f.split_half.is_nan() {
                    "—".into()
                } else {
                    format!("{:+.2}", f.split_half)
                },
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    println!(
        "A player's level: deciles of the mean score of their scored clears in other activity groups (at least {}; \
those with fewer form a level of their own). A type's excess is its clear rate above that expected of its players' \
levels in the same activities. Split-half r: the correlation of types' excess between two halves of the players, over \
types with {}+ first tries in each — near 0 would mean no build signal.",
        attempts::LEVEL_CLEARS,
        attempts::HALF_ATTEMPTS
    );

    for a in attempts::agreement(db, &types)? {
        println!(
            "\n## {}: build-type ranking (clears only) beside first-try clear rates",
            a.group
        );
        println!(
            "Rank correlation of effect and first-try excess over {} build types ({}+ first tries, {}+ ranked \
players): {}",
            a.types.len(),
            attempts::AGREEMENT_ATTEMPTS,
            attempts::AGREEMENT_PLAYERS,
            if a.spearman.is_nan() {
                "—".into()
            } else {
                format!("{:+.2}", a.spearman)
            }
        );
        let header = [
            "#",
            "build type",
            "effect",
            "players",
            "first tries",
            "cleared",
            "expected",
            "excess",
        ]
        .map(String::from);
        let rows: Vec<Vec<String>> = a
            .types
            .iter()
            .take(12)
            .enumerate()
            .map(|(i, (r, t))| {
                vec![
                    (i + 1).to_string(),
                    type_label(&r.key),
                    pct(r.effect),
                    thousands(r.players as i64),
                    thousands(t.attempts as i64),
                    share(t.clear_rate),
                    share(t.clear_rate - t.excess),
                    format!("{:+.1} pts", 100.0 * t.excess),
                ]
            })
            .collect();
        println!("{}", render(format, &header, &rows));
    }
    Ok(())
}

/// A build type from the ranking's key: "Class Element: armor + weapon · super · aspects".
fn type_label(key: &[String]) -> String {
    let [class, element, armor, weapon, sup, aspects] = key else {
        return key.join(" · ");
    };
    format!("{class} {element}: {armor} + {weapon} · {sup} · {aspects}")
}

fn run_triggers(db: &Database, format: Format) -> Result<()> {
    println!(
        "## What sets off the builds' documented parts (subclass choices, armor mods, exotic armor)"
    );
    let mut header = vec![
        "builds of".to_string(),
        "clears".to_string(),
        "parts".to_string(),
    ];
    header.extend(triggers::TRIGGERS.iter().map(|t| (*t).to_string()));
    let rows: Vec<Vec<String>> = triggers::by_group(db)?
        .into_iter()
        .map(|m| {
            let mut row = vec![
                m.group,
                thousands(m.weight.round() as i64),
                format!("{:.1}", m.parts),
            ];
            row.extend(m.shares.iter().map(|s| format!("{:.1}%", 100.0 * s)));
            row
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    println!(
        "A part counts for every trigger it has; parts are the build's components the mechanics sources document. \
The workbook's rotations count once each (their first loadout)."
    );
    Ok(())
}

fn run_difficulty(db: &Database, a: &DifficultyArgs, format: Format) -> Result<()> {
    if let Some(skull) = &a.skull {
        println!(
            "## Clears with {skull} against the same players' clears without it, same activity and step"
        );
        let header = [
            "group",
            "pairs",
            "players",
            "kill rate",
            "deaths / 10 min",
            "clear length",
        ]
        .map(String::from);
        let rows: Vec<Vec<String>> = difficulty::with_skull(db, skull, a.min_pairs)?
            .into_iter()
            .map(|e| {
                vec![
                    e.group,
                    thousands(e.pairs),
                    thousands(e.players),
                    format!("×{:.3}", e.kill_rate),
                    format!("{:+.2}", e.deaths_10m),
                    format!("×{:.3}", e.duration),
                ]
            })
            .collect();
        println!("{}", render(format, &header, &rows));
        return Ok(());
    }
    println!(
        "## Kill rate and deaths at each difficulty step, against the same players' {} clears of the same activity",
        a.base
    );
    let header = [
        "group",
        "step",
        "activity level",
        "pairs",
        "players",
        "kill rate",
        "deaths / 10 min",
    ]
    .map(String::from);
    let rows: Vec<Vec<String>> = difficulty::steps(db, &a.base, a.min_pairs)?
        .into_iter()
        .map(|s| {
            vec![
                s.group,
                s.step,
                format!("{:.0}", s.activity_level),
                thousands(s.pairs),
                thousands(s.players),
                format!("×{:.3}", s.kill_rate),
                format!("{:+.2}", s.deaths_10m),
            ]
        })
        .collect();
    println!("{}", render(format, &header, &rows));
    println!(
        "Pairs: a player and an activity with clears at both the step and the base. Kill rate falls by \
less than enemy health rises: fights are not all shooting, and red-bar enemies gain health more slowly with \
Power delta than majors and bosses."
    );
    Ok(())
}

fn run_escalation(db: &Database, a: &EscalationArgs, format: Format) -> Result<()> {
    if let Some(anchor) = &a.anchor {
        println!("## Builds with {anchor}, by which of the other parts they run");
        let mut header: Vec<String> = a.parts.clone();
        header.extend(["context", "clears", "players", "vs own level"].map(String::from));
        let rows: Vec<Vec<String>> = escalation::combinations(db, anchor, &a.parts)?
            .into_iter()
            .map(|c| {
                let mut row: Vec<String> = c
                    .runs
                    .iter()
                    .map(|r| if *r { "✓" } else { "·" }.to_string())
                    .collect();
                row.extend([
                    c.context,
                    thousands(c.clears.round() as i64),
                    thousands(c.players),
                    pct(c.vs_level),
                ]);
                row
            })
            .collect();
        println!("{}", render(format, &header, &rows));
        return Ok(());
    }
    let lifts = escalation::lifts(db, a.min_clears, a.min_players)?;
    let header = [
        "subclass",
        "kind",
        "part",
        "hard clears",
        "easy clears",
        "hard lift",
        "easy lift",
        "hard − easy",
        "boss lift",
    ]
    .map(String::from);
    let row = |l: &escalation::PartLift| {
        vec![
            l.subclass.clone(),
            l.kind.clone(),
            l.part.clone(),
            thousands(l.hard_clears.round() as i64),
            thousands(l.easy_clears.round() as i64),
            pct(l.hard),
            pct(l.easy),
            format!("{:+.1} pts", 100.0 * (l.hard - l.easy)),
            l.boss.map_or("—".into(), pct),
        ]
    };
    println!(
        "## Parts that lift their players more in hard content (Conquests) than in easy content"
    );
    let rows: Vec<Vec<String>> = lifts.iter().take(a.top).map(row).collect();
    println!("{}", render(format, &header, &rows));
    println!("\n## Parts that lift their players more in easy content than in hard content");
    let rows: Vec<Vec<String>> = lifts.iter().rev().take(a.top).map(row).collect();
    println!("{}", render(format, &header, &rows));
    println!(
        "Lift: clears with the part against clears of the same subclass without it, each against its player's usual \
level, per context; parts with {}+ clears and {}+ players in both contexts.",
        a.min_clears, a.min_players
    );
    Ok(())
}
