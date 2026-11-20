//! Rankings of builds and players that separate the build from the player.
//!
//! Every query fits `perf = skill(player) + effect(build group)` with
//! [`focalpoint_core::rank::fit_effects`] over the scored runs (`run_score`)
//! that the [`RunFilter`] selects:
//!
//! * runs without a captured build inform only their player's skill;
//! * runs with captured builds are split across their builds by share of
//!   the run, and inform both.
//!
//! The run filter's activity, solo and date conditions choose the runs the
//! model learns from; its player condition and the build [`Facets`] only
//! choose what is listed, so skills are always net of *every* build their
//! players used. Effects are reported relative to the average captured
//! build (see [`Model::effect_offset`]).
//!
//! With [`RankOptions::context_prior`] the fit also has build × activity
//! group effects (`fit_effects_in_context`), and the reported effect is the
//! part that holds across activity groups: the **neutral-game** ranking.

use std::collections::{HashMap, HashSet};

use duckdb::params_from_iter;
use duckdb::types::Value;
use focalpoint_core::rank::{
    Baseline, Contexts, FitParams, Observation, fit_effects, fit_effects_in_context,
};

use crate::filter::{Facets, RunFilter};
use crate::search::GroupBy;
use crate::{Database, Result};

/// Ranking parameters.
#[derive(Debug, Clone)]
pub struct RankOptions {
    pub facets: Facets,
    pub runs: RunFilter,
    /// How builds are grouped (for players: the grouping their skill is
    /// netted against). `GroupBy::Player` is treated as `Core`.
    pub group_by: GroupBy,
    pub fit: FitParams,
    /// When set, also fit a build × activity group effect with this ridge
    /// prior (pseudo-runs), and report only the part of a build's effect
    /// that holds across activity groups: the neutral-game ranking.
    pub context_prior: Option<f64>,
    /// What solo clears are measured against.
    pub solo: SoloScoring,
}

/// What a solo clear's performance is measured against (ADR 0008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SoloScoring {
    /// Other solo clears of the activity (ADR 0006 as first written):
    /// soloing earns nothing by itself.
    AgainstSolo,
    /// The activity's fireteam clears: a solo clear of content made for a
    /// fireteam earns what it takes — every kill is the soloist's, and there
    /// is no one to revive them — as Searchlight's 2× solo multiplier did,
    /// measured instead of fixed. Same factors and bounds as `run_score`
    /// (`ScoreParams::default`), centred like the fireteam clears.
    AgainstFireteam,
    /// Other solo clears, plus the activity's solo premium
    /// ([`solo_credit_sql`]) for solo clears that started from the
    /// beginning. Soloists stay ranked among themselves; soloing content
    /// made for a fireteam earns what it typically takes, never less than
    /// nothing. The default.
    #[default]
    ActivityCredit,
}

/// The fireteam-referenced log score of a solo clear `s` against reference
/// `t` (the factors and bounds of `run_score`), and its kill index.
pub(crate) fn against_fireteam(s: &str, t: &str) -> (String, String) {
    let p = focalpoint_core::rank::ScoreParams::default();
    let kill_index = format!(
        "least(greatest({s}.kills / {t}.kills, {k0}), {k1})",
        k0 = p.kill_bounds.0,
        k1 = p.kill_bounds.1
    );
    let log_score = format!(
        "ln({kill_index}
            * least(greatest({t}.duration_s / greatest({s}.duration_s, 1), {s0}), {s1})
            * least(greatest((1 + {d} * {t}.deaths) / (1 + {d} * {s}.deaths), {v0}), {v1}))",
        s0 = p.speed_bounds.0,
        s1 = p.speed_bounds.1,
        d = p.death_rate,
        v0 = p.survival_bounds.0,
        v1 = p.survival_bounds.1,
    );
    (log_score, kill_index)
}

/// Per activity, the fireteam clears a solo clear is compared with:
/// `(activity_hash, kills, duration_s, deaths, centre, from_qualified,
/// min_duration)` — `min_duration` being the 1st percentile of their
/// durations: no full clear is faster than the fastest fireteams.
///
/// The activity's own fireteam reference (`activity_reference`) when it has
/// enough qualified fireteam clears; otherwise one built the same way from
/// its unqualified fireteam clears — cleared, started from the beginning,
/// in the same period as the scored runs. Searchlight's qualification gates
/// can exclude fireteams entirely: five Grandmaster Conquests qualified solo
/// runs only (`solo_only`), though all are fireteam content with hundreds of
/// thousands of fireteam runs recorded.
#[must_use]
pub fn team_reference_sql() -> String {
    let p = focalpoint_core::rank::ScoreParams::default();
    let (log_score, _) = against_fireteam("pool", "med");
    format!(
        "WITH fast AS (
             SELECT activity_hash, quantile_cont(duration_s, 0.01) AS min_duration
             FROM run_score WHERE NOT is_solo GROUP BY 1),
         own AS (
             SELECT activity_hash, kills, duration_s, deaths, log_score_mean AS centre, min_duration
             FROM activity_reference JOIN fast USING (activity_hash)
             WHERE NOT is_solo AND is_scored AND NOT is_borrowed AND kills > 0
                   AND log_score_mean IS NOT NULL),
         pool AS (
             SELECT p.activity_hash, sum(p.kills) AS kills, sum(p.deaths) AS deaths,
                    any_value(r.duration_s) AS duration_s
             FROM run_player p JOIN run r USING (instance_id)
             WHERE r.roster_size > 1 AND r.duration_s > 0 AND r.started_from_beginning
                   AND r.started_at >= (SELECT min(started_at) FROM run_score)
                   AND p.activity_hash IN (
                       SELECT activity_hash FROM activity_reference
                       WHERE NOT is_solo AND is_scored AND is_borrowed)
             GROUP BY p.instance_id, p.membership_id, p.activity_hash
             HAVING bool_or(p.is_clear) AND sum(p.kills) > 0),
         med AS (
             SELECT activity_hash, median(kills) AS kills, median(duration_s) AS duration_s,
                    median(deaths) AS deaths, quantile_cont(duration_s, 0.01) AS min_duration
             FROM pool GROUP BY 1 HAVING count(*) >= {min_clears})
         SELECT activity_hash, kills, duration_s, deaths, centre, true AS from_qualified, min_duration
         FROM own
         UNION ALL
         SELECT med.activity_hash, any_value(med.kills), any_value(med.duration_s),
                any_value(med.deaths), avg({log_score}), false, any_value(med.min_duration)
         FROM pool JOIN med USING (activity_hash) GROUP BY med.activity_hash",
        min_clears = p.min_reference_clears,
    )
}

/// The Destiny activity mode of raids (`DestinyActivityModeType.Raid`).
/// Solo raid clears get no premium: the data cannot tell full solo clears
/// of six-player content from partial ones (solo Root of Nightmares Master
/// "clears" below the Normal solo world record, King's Fall Master in a
/// median 8 minutes), and kills do not measure boss encounters (Pantheon).
pub const RAID_MODE: i32 = 4;

/// Per activity, the log solo premium of [`SoloScoring::ActivityCredit`]:
/// `(activity_hash, log_credit, solo_clears, kill_index, min_duration)` —
/// the median, over the activity's solo clears that look like full clears,
/// of how much better each scores against the fireteam clears
/// ([`team_reference_sql`]) than against other solo clears; never negative.
/// A solo clear looks like a full clear when it started from the beginning
/// and took at least `min_duration` (the fastest 1% of fireteam clears):
/// a fifth to two fifths of Master dungeon solo "clears" are faster than
/// that, some in a minute or two. Raids ([`RAID_MODE`]) get none. Where
/// kills cannot measure a soloist's work the measured premium falls below
/// one and no credit is given.
#[must_use]
pub fn solo_credit_sql() -> String {
    let (log_score, kill_index) = against_fireteam("s", "t");
    format!(
        "SELECT s.activity_hash,
                greatest(0, median({log_score} - t.centre - s.perf)) AS log_credit,
                count(*) AS solo_clears, median({kill_index}) AS kill_index,
                any_value(t.min_duration) AS min_duration
         FROM run_score s
         JOIN run USING (instance_id)
         JOIN ({team}) t ON t.activity_hash = s.activity_hash
         WHERE s.is_solo AND run.started_from_beginning AND coalesce(run.mode, 0) <> {RAID_MODE}
               AND s.duration_s >= t.min_duration
         GROUP BY 1",
        team = team_reference_sql(),
    )
}

impl SoloScoring {
    /// The joins the performance expression needs, after `run_score r`.
    fn join(self, db: &Database) -> Result<String> {
        Ok(match self {
            Self::AgainstSolo => String::new(),
            Self::AgainstFireteam => format!(
                "LEFT JOIN ({}) team ON team.activity_hash = r.activity_hash",
                team_reference_sql()
            ),
            Self::ActivityCredit => {
                let credits = db.solo_credits()?;
                let table = if credits.is_empty() {
                    "SELECT NULL::UINTEGER AS activity_hash, NULL::DOUBLE AS log_credit,
                            NULL::DOUBLE AS min_duration WHERE false"
                        .to_string()
                } else {
                    let rows: Vec<String> = credits
                        .iter()
                        .map(|(h, c, d)| format!("({h}, {c}, {d})"))
                        .collect();
                    format!(
                        "SELECT * FROM (VALUES {}) v(activity_hash, log_credit, min_duration)",
                        rows.join(", ")
                    )
                };
                format!(
                    "LEFT JOIN ({table}) credit ON credit.activity_hash = r.activity_hash
                     LEFT JOIN run fresh ON fresh.instance_id = r.instance_id"
                )
            }
        })
    }

    /// A run's performance: `run_score.perf`, or its credited counterpart
    /// for solo clears.
    fn perf(self) -> String {
        match self {
            Self::AgainstSolo => "r.perf".into(),
            Self::AgainstFireteam => {
                let (log_score, _) = against_fireteam("r", "team");
                format!(
                    "CASE WHEN r.is_solo AND team.kills > 0
                          THEN {log_score} - team.centre
                          ELSE r.perf END"
                )
            }
            Self::ActivityCredit => "r.perf + CASE WHEN r.is_solo AND fresh.started_from_beginning
                                    AND r.duration_s >= credit.min_duration
                               THEN credit.log_credit ELSE 0 END"
                .into(),
        }
    }
}

impl Default for RankOptions {
    fn default() -> Self {
        Self {
            facets: Facets::default(),
            runs: RunFilter::default(),
            group_by: GroupBy::Core,
            fit: FitParams::default(),
            context_prior: None,
            solo: SoloScoring::default(),
        }
    }
}

/// The context prior chosen by five-fold cross-validation together with the
/// other priors (`focalpoint analyze priors --contexts`; ADR 0007).
pub const NEUTRAL_CONTEXT_PRIOR: f64 = 3.0;

/// What the fitted model saw.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// Scored runs selected.
    pub runs: usize,
    /// Of those, runs with a captured build.
    pub build_runs: usize,
    pub players: usize,
    pub groups: usize,
    pub iterations: usize,
    pub converged: bool,
    /// Run-weighted mean build effect, subtracted from reported effects so
    /// that 0 is the average captured build. Tracked players were chosen for
    /// standout runs, so their other runs flatter their skill a little and
    /// raw effects sit below zero on average.
    pub effect_offset: f64,
}

/// A build group's ranking. Log-scale values; `exp(x) - 1` is a percentage.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildRank {
    pub key: Vec<String>,
    /// The build's effect: how its players perform relative to their own
    /// usual level, relative to the average captured build.
    pub effect: f64,
    /// Mean performance with the build, without separating the player.
    pub raw: f64,
    /// Mean skill of the players using it (run-weighted).
    pub player_skill: f64,
    /// Players who used it for at least half a run in total.
    pub players: usize,
    /// Runs, counting a run split across builds by its share.
    pub runs: f64,
    pub builds: usize,
    pub last_used: String,
}

/// A player's ranking.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerRank {
    pub membership_id: i64,
    pub name: String,
    /// Performance relative to a typical clear, net of the builds used.
    pub skill: f64,
    pub runs: i64,
    pub solo_runs: i64,
    /// Runs with a captured build (matching the facets, if any).
    pub build_runs: i64,
    pub best_score: f64,
    pub last_run: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildSort {
    #[default]
    Effect,
    Raw,
    Players,
    Runs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerSort {
    #[default]
    Skill,
    Runs,
    Best,
}

#[derive(Debug)]
struct PlayerStats {
    membership_id: i64,
    runs: i64,
    solo_runs: i64,
    best_score: f64,
    last_run: String,
}

/// The model's inputs for one selection of runs, as passed to
/// [`fit_effects`]; public so the priors can be tuned
/// ([`crate::analysis::priors`]).
#[derive(Debug)]
pub struct ModelData {
    /// One per player in the selection.
    pub baselines: Vec<Baseline>,
    /// Runs with captured builds, one per (run, build), weighted by share.
    pub observations: Vec<Observation>,
    /// The run (`instance_id`) of each observation.
    pub observation_runs: Vec<i64>,
    /// The activity group (Searchlight target group) of each observation's
    /// run, indexing [`ModelData::context_names`].
    pub observation_contexts: Vec<u32>,
    pub context_names: Vec<String>,
    pub groups: usize,
    players: Vec<PlayerStats>,
    obs_build: Vec<i64>,
    obs_date: Vec<String>,
    /// Per group: key and whether its builds match the facets.
    group_keys: Vec<(bool, Vec<String>)>,
    total_runs: usize,
    build_runs: usize,
}

struct Fitted {
    data: ModelData,
    skill: Vec<f64>,
    effect: Vec<f64>,
    model: Model,
}

/// Load the model's inputs for the runs `opts` selects.
pub fn load_model(db: &Database, opts: &RankOptions) -> Result<ModelData> {
    let runs = RunFilter {
        player: None,
        ..opts.runs.clone()
    };
    let mut run_params = Vec::new();
    let run_where = runs.sql(&mut run_params);
    let solo_join = opts.solo.join(db)?;
    let perf = opts.solo.perf();
    let from = format!(
        "FROM run_score r JOIN activity a ON a.activity_hash = r.activity_hash {solo_join}"
    );

    // Players: their stats over the selection, and the baseline from runs
    // without a captured build.
    let mut players = Vec::new();
    let mut baselines = Vec::new();
    let mut total_runs = 0_usize;
    {
        let sql = format!(
            "SELECT r.membership_id, count(*), count(*) FILTER (WHERE r.is_solo),
                    max(r.score), strftime(max(r.started_at), '%Y-%m-%d'),
                    count(*) FILTER (WHERE NOT r.has_build),
                    coalesce(sum({perf}) FILTER (WHERE NOT r.has_build), 0)
             {from} WHERE {run_where}
             GROUP BY 1 ORDER BY 1"
        );
        let mut stmt = db.conn().prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(run_params.iter()))?;
        while let Some(r) = rows.next()? {
            let runs: i64 = r.get(1)?;
            total_runs += usize::try_from(runs).unwrap_or(0);
            players.push(PlayerStats {
                membership_id: r.get(0)?,
                runs,
                solo_runs: r.get(2)?,
                best_score: r.get(3)?,
                last_run: r.get(4)?,
            });
            let n: i64 = r.get(5)?;
            baselines.push(Baseline {
                weight: n as f64,
                sum: r.get(6)?,
            });
        }
    }
    let player_index: HashMap<i64, u32> = players
        .iter()
        .enumerate()
        .map(|(i, p)| (p.membership_id, i as u32))
        .collect();

    // Group keys for every build seen in the selection.
    let key = match opts.group_by {
        GroupBy::Player => GroupBy::Core.key(),
        g => g.key(),
    };
    let mut key_params = Vec::new();
    let facet_where = opts.facets.sql(db, &mut key_params)?;
    key_params.extend(run_params.iter().cloned());
    let key_select: Vec<String> = key.iter().map(|(_, e)| (*e).to_string()).collect();
    let mut group_of_build: HashMap<i64, u32> = HashMap::new();
    let mut group_index: HashMap<(bool, Vec<String>), u32> = HashMap::new();
    let mut group_keys = Vec::new();
    {
        let sql = format!(
            "SELECT b.build_id, ({facet_where}) AS matched, {keys}
             FROM build b
             WHERE b.build_id IN (SELECT rb.build_id FROM run_build rb
                                  JOIN run_score r USING (instance_id, membership_id)
                                  JOIN activity a ON a.activity_hash = r.activity_hash
                                  WHERE {run_where})
             ORDER BY b.build_id",
            keys = key_select.join(", "),
        );
        let mut stmt = db.conn().prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(key_params.iter()))?;
        while let Some(r) = rows.next()? {
            let build_id: i64 = r.get(0)?;
            let matched: bool = r.get(1)?;
            let mut k = Vec::with_capacity(key.len());
            for i in 0..key.len() {
                k.push(
                    r.get::<_, Option<String>>(i + 2)?
                        .unwrap_or_else(|| "—".into()),
                );
            }
            let id = (matched, k);
            let next = group_keys.len() as u32;
            let g = *group_index.entry(id.clone()).or_insert_with(|| {
                group_keys.push(id);
                next
            });
            group_of_build.insert(build_id, g);
        }
    }

    // Observations: runs with captured builds, split by share.
    let mut observations = Vec::new();
    let mut obs_build = Vec::new();
    let mut obs_date = Vec::new();
    let mut observation_runs = Vec::new();
    let mut observation_contexts = Vec::new();
    let mut context_index: HashMap<String, u32> = HashMap::new();
    let mut context_names = Vec::new();
    let mut build_run_keys: HashSet<(i64, i64)> = HashSet::new();
    {
        let sql = format!(
            "SELECT r.instance_id, r.membership_id, rb.build_id, {perf},
                    coalesce(rb.share / nullif(sum(rb.share) OVER w, 0), 1.0 / count(*) OVER w),
                    strftime(r.started_at, '%Y-%m-%d'), coalesce(a.target_group, '')
             FROM run_score r JOIN run_build rb USING (instance_id, membership_id)
             JOIN activity a ON a.activity_hash = r.activity_hash {solo_join}
             WHERE r.has_build AND {run_where}
             WINDOW w AS (PARTITION BY rb.instance_id, rb.membership_id)
             ORDER BY r.instance_id, r.membership_id, rb.build_id"
        );
        let mut stmt = db.conn().prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(run_params.iter()))?;
        while let Some(r) = rows.next()? {
            let instance: i64 = r.get(0)?;
            let member: i64 = r.get(1)?;
            let build_id: i64 = r.get(2)?;
            let (Some(&player), Some(&group)) =
                (player_index.get(&member), group_of_build.get(&build_id))
            else {
                continue;
            };
            build_run_keys.insert((instance, member));
            observations.push(Observation {
                player,
                group,
                y: r.get(3)?,
                weight: r.get(4)?,
            });
            obs_build.push(build_id);
            obs_date.push(r.get(5)?);
            observation_runs.push(instance);
            let context: String = r.get(6)?;
            let next = context_names.len() as u32;
            observation_contexts.push(*context_index.entry(context.clone()).or_insert_with(|| {
                context_names.push(context);
                next
            }));
        }
    }
    Ok(ModelData {
        baselines,
        groups: group_keys.len(),
        observations,
        observation_runs,
        observation_contexts,
        context_names,
        players,
        obs_build,
        obs_date,
        group_keys,
        total_runs,
        build_runs: build_run_keys.len(),
    })
}

impl ModelData {
    /// Each observation's cell — its build group in its activity group — as
    /// [`fit_effects_in_context`] wants them, and the number of cells.
    #[must_use]
    pub fn cells(&self) -> (Vec<u32>, usize) {
        let mut index: HashMap<(u32, u32), u32> = HashMap::new();
        let cell_of = self
            .observations
            .iter()
            .zip(&self.observation_contexts)
            .map(|(o, &c)| {
                let next = index.len() as u32;
                *index.entry((o.group, c)).or_insert(next)
            })
            .collect();
        (cell_of, index.len())
    }
}

fn fit(db: &Database, opts: &RankOptions) -> Result<Fitted> {
    let data = load_model(db, opts)?;
    let effects = match opts.context_prior {
        None => fit_effects(&data.baselines, data.groups, &data.observations, &opts.fit),
        Some(prior) => {
            let (cell_of, cells) = data.cells();
            let contexts = Contexts {
                cell_of: &cell_of,
                cells,
                prior,
            };
            fit_effects_in_context(
                &data.baselines,
                data.groups,
                &data.observations,
                &contexts,
                &opts.fit,
            )
        }
    };
    let (sum, weight) = data.observations.iter().fold((0.0, 0.0), |(s, w), o| {
        (
            s + o.weight * effects.effect[o.group as usize],
            w + o.weight,
        )
    });
    let effect_offset = if weight > 0.0 { sum / weight } else { 0.0 };
    let model = Model {
        runs: data.total_runs,
        build_runs: data.build_runs,
        players: data.players.len(),
        groups: data.groups,
        iterations: effects.iterations,
        converged: effects.converged,
        effect_offset,
    };
    Ok(Fitted {
        data,
        skill: effects.skill,
        effect: effects.effect,
        model,
    })
}

#[derive(Default)]
/// Per-group accumulator for [`rank_builds`].
struct Acc {
    /// Use per player, in runs (shares summed).
    players: HashMap<u32, f64>,
    builds: HashSet<i64>,
    weight: f64,
    perf: f64,
    skill: f64,
    last: String,
}

/// Players who used a group for at least half a run in total; briefer use
/// (a swap for one encounter) still informs the fit but is not counted.
fn regular_players(a: &Acc) -> usize {
    a.players.values().filter(|&&w| w >= 0.5).count()
}

/// Rank build groups by their effect on their players' performance.
pub fn rank_builds(
    db: &Database,
    opts: &RankOptions,
    min_players: usize,
    sort: BuildSort,
) -> Result<(Model, Vec<&'static str>, Vec<BuildRank>)> {
    let f = fit(db, opts)?;
    let key_columns: Vec<&'static str> = match opts.group_by {
        GroupBy::Player => GroupBy::Core.key(),
        g => g.key(),
    }
    .iter()
    .map(|(label, _)| *label)
    .collect();

    let mut acc: Vec<Acc> = (0..f.data.group_keys.len())
        .map(|_| Acc::default())
        .collect();
    for (i, o) in f.data.observations.iter().enumerate() {
        let a = &mut acc[o.group as usize];
        *a.players.entry(o.player).or_default() += o.weight;
        a.builds.insert(f.data.obs_build[i]);
        a.weight += o.weight;
        a.perf += o.weight * o.y;
        a.skill += o.weight * f.skill[o.player as usize];
        if f.data.obs_date[i] > a.last {
            a.last.clone_from(&f.data.obs_date[i]);
        }
    }
    let mut rows: Vec<BuildRank> = acc
        .into_iter()
        .enumerate()
        .filter(|(g, a)| f.data.group_keys[*g].0 && a.weight > 0.0)
        .map(|(g, a)| (g, regular_players(&a), a))
        .filter(|(_, players, _)| *players >= min_players)
        .map(|(g, players, a)| BuildRank {
            key: f.data.group_keys[g].1.clone(),
            effect: f.effect[g] - f.model.effect_offset,
            raw: a.perf / a.weight,
            player_skill: a.skill / a.weight,
            players,
            runs: a.weight,
            builds: a.builds.len(),
            last_used: a.last,
        })
        .collect();
    let by = |r: &BuildRank| match sort {
        BuildSort::Effect => r.effect,
        BuildSort::Raw => r.raw,
        BuildSort::Players => r.players as f64,
        BuildSort::Runs => r.runs,
    };
    rows.sort_by(|a, b| {
        by(b)
            .total_cmp(&by(a))
            .then(b.runs.total_cmp(&a.runs))
            .then_with(|| a.key.cmp(&b.key))
    });
    Ok((f.model, key_columns, rows))
}

/// Rank players by skill, net of the builds they used.
pub fn rank_players(
    db: &Database,
    opts: &RankOptions,
    min_runs: i64,
    sort: PlayerSort,
) -> Result<(Model, Vec<PlayerRank>)> {
    let f = fit(db, opts)?;
    // Runs with builds matching the facets, per player.
    let mut matched_runs: HashMap<u32, f64> = HashMap::new();
    for o in &f.data.observations {
        if f.data.group_keys[o.group as usize].0 {
            *matched_runs.entry(o.player).or_default() += o.weight;
        }
    }
    let named: Option<HashSet<i64>> = match &opts.runs.player {
        None => None,
        Some(p) => {
            let mut stmt = db.conn().prepare(
                "SELECT membership_id FROM player pl
                 WHERE pl.bungie_name ILIKE '%' || ? || '%' OR pl.display_name ILIKE '%' || ? || '%'",
            )?;
            let ids = stmt
                .query_map([p, p], |r| r.get(0))?
                .collect::<duckdb::Result<HashSet<i64>>>()?;
            Some(ids)
        }
    };
    let needs_build = !opts.facets.is_empty();
    let mut rows: Vec<PlayerRank> = f
        .data
        .players
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            p.runs >= min_runs
                && (!needs_build || matched_runs.contains_key(&(*i as u32)))
                && named.as_ref().is_none_or(|n| n.contains(&p.membership_id))
        })
        .map(|(i, p)| PlayerRank {
            membership_id: p.membership_id,
            name: String::new(),
            skill: f.skill[i],
            runs: p.runs,
            solo_runs: p.solo_runs,
            build_runs: matched_runs
                .get(&(i as u32))
                .map_or(0, |w| w.round() as i64),
            best_score: p.best_score,
            last_run: p.last_run.clone(),
        })
        .collect();
    let by = |r: &PlayerRank| match sort {
        PlayerSort::Skill => r.skill,
        PlayerSort::Runs => r.runs as f64,
        PlayerSort::Best => r.best_score,
    };
    rows.sort_by(|a, b| {
        by(b)
            .total_cmp(&by(a))
            .then(b.runs.cmp(&a.runs))
            .then(a.membership_id.cmp(&b.membership_id))
    });
    Ok((f.model, rows))
}

/// Fill in display names for the given rows.
pub fn name_players(db: &Database, rows: &mut [PlayerRank]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let ids: Vec<Value> = rows
        .iter()
        .map(|r| Value::BigInt(r.membership_id))
        .collect();
    let placeholders = vec!["?"; ids.len()].join(", ");
    let mut stmt = db.conn().prepare(&format!(
        "SELECT membership_id, coalesce(bungie_name, display_name, membership_id::VARCHAR)
         FROM player WHERE membership_id IN ({placeholders})"
    ))?;
    let names: HashMap<i64, String> = stmt
        .query_map(params_from_iter(ids.iter()), |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<duckdb::Result<_>>()?;
    for r in rows {
        r.name = names
            .get(&r.membership_id)
            .cloned()
            .unwrap_or_else(|| r.membership_id.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Players 1-2 are strong (+0.5 on every run), 3-4 weak (-0.5). Each has
    /// ten runs without a captured build and two with one: the strong
    /// players wear Alpha Boots (build 10), the weak ones Beta Boots (build
    /// 20). Neither build changes how anyone performs.
    fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::create(&dir.path().join("r.duckdb")).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO item (item_hash, name, is_exotic) VALUES (1, 'Alpha Boots', true), (2, 'Beta Boots', true);
                 INSERT INTO activity (activity_hash, name, target_group) VALUES (100, 'Ultimate Conquest: Test', 'Ultimate Conquest');
                 INSERT INTO player (membership_id, bungie_name)
                     VALUES (1, 'Strong#0001'), (2, 'Strong#0002'), (3, 'Weak#0003'), (4, 'Weak#0004');
                 INSERT INTO build (build_id, class, exotic_armor_hash, aspect_hashes, fragment_hashes,
                     artifact_perk_hashes, armor_mod_hashes, item_hashes, component_count, unresolved_count,
                     tier1_elements, tier2_elements, tier3_elements, tier4_elements)
                 VALUES (10, 'Warlock', 1, [], [], [], [], [], 0, 0, [], [], [], []),
                        (20, 'Warlock', 2, [], [], [], [], [], 0, 0, [], [], [], []);
                 INSERT INTO run_score
                 SELECT 1000 * m + i, m, 100, TIMESTAMP '2026-07-01' + INTERVAL (i) DAY, i = 11, i >= 10,
                        100, 0, 600, 1, 1, 1, 1, CASE WHEN m <= 2 THEN 0.5 ELSE -0.5 END
                 FROM range(1, 5) p(m), range(0, 12) r(i);
                 INSERT INTO run_build
                 SELECT 1000 * m + i, m, CASE WHEN m <= 2 THEN 10 ELSE 20 END, 1, 0, 600, 1.0, true
                 FROM range(1, 5) p(m), range(10, 12) r(i);",
            )
            .unwrap();
        (dir, db)
    }

    fn exact() -> RankOptions {
        RankOptions {
            group_by: GroupBy::ExoticArmor,
            fit: FitParams {
                player_prior: 1e-9,
                group_prior: 1e-9,
                ..FitParams::default()
            },
            ..RankOptions::default()
        }
    }

    #[test]
    fn builds_are_judged_against_their_players() {
        let (_d, db) = db();
        let (model, columns, rows) = rank_builds(&db, &exact(), 1, BuildSort::Effect).unwrap();
        assert_eq!(columns, ["class", "exotic armor"]);
        assert_eq!((model.runs, model.build_runs, model.players), (48, 8, 4));
        assert!(model.converged);
        assert_eq!(rows.len(), 2);
        for r in &rows {
            assert!(r.effect.abs() < 1e-6, "{r:?}");
            assert_eq!((r.players, r.builds), (2, 1));
            assert!((r.runs - 4.0).abs() < 1e-9);
        }
        let alpha = rows.iter().find(|r| r.key[1] == "Alpha Boots").unwrap();
        assert!((alpha.raw - 0.5).abs() < 1e-9, "raw results flatter Alpha");
        assert!((alpha.player_skill - 0.5).abs() < 1e-6);
        assert_eq!(alpha.last_used, "2026-07-12");
    }

    #[test]
    fn players_rank_by_skill_and_filter() {
        let (_d, db) = db();
        let opts = exact();
        let (_, mut rows) = rank_players(&db, &opts, 1, PlayerSort::Skill).unwrap();
        name_players(&db, &mut rows).unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            ["Strong#0001", "Strong#0002", "Weak#0003", "Weak#0004"]
        );
        assert!((rows[0].skill - 0.5).abs() < 1e-6);
        assert_eq!(
            (rows[0].runs, rows[0].solo_runs, rows[0].build_runs),
            (12, 1, 2)
        );

        let beta = RankOptions {
            facets: Facets {
                exotic_armor: Some("beta boots".into()),
                ..Facets::default()
            },
            ..exact()
        };
        let (_, rows) = rank_players(&db, &beta, 1, PlayerSort::Skill).unwrap();
        let ids: Vec<i64> = rows.iter().map(|r| r.membership_id).collect();
        assert_eq!(ids, [3, 4], "only players who used a matching build");

        let one_player = RankOptions {
            runs: RunFilter {
                player: Some("strong#0002".into()),
                ..RunFilter::default()
            },
            ..exact()
        };
        let (_, rows) = rank_players(&db, &one_player, 1, PlayerSort::Skill).unwrap();
        assert_eq!(rows.len(), 1);
        let (_, rows) = rank_players(&db, &exact(), 13, PlayerSort::Skill).unwrap();
        assert!(rows.is_empty(), "min_runs");
    }

    #[test]
    fn an_empty_selection_is_not_an_error() {
        let (_d, db) = db();
        let opts = RankOptions {
            runs: RunFilter {
                activity: Some("no such activity".into()),
                ..RunFilter::default()
            },
            ..exact()
        };
        let (model, _, rows) = rank_builds(&db, &opts, 1, BuildSort::Effect).unwrap();
        assert_eq!((model.runs, rows.len()), (0, 0));
        let (_, rows) = rank_players(&db, &opts, 1, PlayerSort::Skill).unwrap();
        assert!(rows.is_empty());
    }
}
