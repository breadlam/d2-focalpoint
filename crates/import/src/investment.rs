//! Investment stage (after transform): read each staged build's modal stats
//! and fragment share, apply the budget-aware investment model
//! ([`searchlight_core::stats`]), and stage the levels and the tier-3
//! elements with stat investment for `finalize.sql`. Also writes the
//! `element_kind` dimension.

use anyhow::Result;
use duckdb::types::Value;
use duckdb::{Connection, params};
use rayon::prelude::*;
use searchlight_core::stats::{ArmorStat, InvestmentParams, StatLine};
use searchlight_core::vector::{ElementKind, ElementVector, Tier};

use crate::progress::Progress;
use crate::rows::{i64_list, u32_list};
use crate::sink::appender;

/// Counters from the investment stage.
#[derive(Debug, Default, Clone)]
pub struct InvestmentStats {
    pub builds: u64,
    pub builds_with_promotion: u64,
}

/// Write the `element_kind` dimension from [`ElementKind::ALL`].
pub fn write_element_kinds(conn: &Connection) -> Result<()> {
    let mut a = conn.appender("element_kind")?;
    for k in ElementKind::ALL {
        a.append_row(params![k.code(), k.as_str(), k.tier().map(Tier::number)])?;
    }
    a.flush()?;
    Ok(())
}

/// One build's computed investment, ready to store.
struct Output {
    build_id: i64,
    levels: Vec<u32>,
    promoted: Vec<String>,
    tier3: Vec<i64>,
}

struct Input {
    build_id: i64,
    observed: StatLine,
    fragments: StatLine,
    tier3: Vec<i64>,
}

fn ints(v: Value) -> Vec<i64> {
    match v {
        Value::List(items) => items
            .into_iter()
            .filter_map(|x| match x {
                Value::TinyInt(n) => Some(i64::from(n)),
                Value::SmallInt(n) => Some(i64::from(n)),
                Value::Int(n) => Some(i64::from(n)),
                Value::BigInt(n) => Some(n),
                Value::UTinyInt(n) => Some(i64::from(n)),
                Value::USmallInt(n) => Some(i64::from(n)),
                Value::UInt(n) => Some(i64::from(n)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn line(v: &[i64]) -> StatLine {
    std::array::from_fn(|i| v.get(i).copied().unwrap_or(0) as i32)
}

/// Compute and store stat investment for every build with stats.
pub fn apply(
    conn: &Connection,
    params: &InvestmentParams,
    progress: &Progress,
) -> Result<InvestmentStats> {
    let spinner = progress.spinner("investment");
    spinner.set_message("reading build stats");
    let inputs: Vec<Input> = {
        let mut stmt = conn.prepare(
            "SELECT b.build_id, s.s, b.fragment_stats, b.tier3_elements
             FROM stg.build b JOIN stg.build_stats s USING (build_id)",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Input {
                build_id: r.get(0)?,
                observed: line(&ints(r.get(1)?)),
                fragments: line(&ints(r.get(2)?)),
                tier3: ints(r.get(3)?),
            })
        })?;
        rows.collect::<duckdb::Result<_>>()?
    };

    spinner.set_message("applying the budget-aware model");
    let outputs: Vec<Output> = inputs
        .par_iter()
        .map(|i| {
            let inv = params.investment(i.observed, i.fragments);
            let tier3 = ElementVector::from_keys(i.tier3.iter().copied())
                .with_investment(&inv)
                .tier_keys(Tier::ArmorEffects);
            let promoted: Vec<String> = ArmorStat::ALL
                .into_iter()
                .filter(|&s| inv.is_promoted(s))
                .map(|s| s.as_str().to_string())
                .collect();
            Output {
                build_id: i.build_id,
                levels: inv.levels.iter().map(|&l| u32::from(l)).collect(),
                promoted,
                tier3,
            }
        })
        .collect();

    spinner.set_message("writing");
    conn.execute_batch(
        "CREATE TABLE stg.investment (build_id BIGINT, stat_levels UTINYINT[], stats_promoted VARCHAR[], tier3 BIGINT[])",
    )?;
    {
        let mut a = appender(conn, "stg.investment")?;
        for o in &outputs {
            let promoted = Value::List(o.promoted.iter().map(|s| Value::Text(s.clone())).collect());
            a.append_row(params![
                o.build_id,
                u32_list(&o.levels),
                promoted,
                i64_list(&o.tier3)
            ])?;
        }
        a.flush()?;
    }
    spinner.finish_with_message("investment done");
    Ok(InvestmentStats {
        builds: outputs.len() as u64,
        builds_with_promotion: outputs.iter().filter(|o| !o.promoted.is_empty()).count() as u64,
    })
}
