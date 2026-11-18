//! Reading the (staged copies of the) production SQLite databases.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};

/// Open a staged SQLite copy. Read-write so SQLite can recover its WAL (the
/// copy is ours; the backup itself is never opened).
pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
    .with_context(|| format!("opening {}", path.display()))?;
    conn.busy_timeout(Duration::from_secs(60))?;
    Ok(conn)
}

/// Does `table` exist in this database?
pub fn has_table(conn: &Connection, table: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |r| r.get::<_, i64>(0),
    )? > 0)
}

/// Split a table's rowid space into inclusive ranges of roughly
/// `target_rows` rows each, assuming rowids are spread evenly between their
/// min and max (true for insertion-ordered tables; good enough for sparse
/// ones such as `pgcr_raw`, whose rowid is the instance id).
pub fn rowid_ranges(conn: &Connection, table: &str, target_rows: i64) -> Result<Vec<(i64, i64)>> {
    let (count, lo, hi): (i64, Option<i64>, Option<i64>) = conn.query_row(
        &format!("SELECT count(*), min(rowid), max(rowid) FROM \"{table}\""),
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let (Some(lo), Some(hi)) = (lo, hi) else {
        return Ok(Vec::new());
    };
    Ok(split_range(lo, hi, count, target_rows))
}

fn split_range(lo: i64, hi: i64, count: i64, target_rows: i64) -> Vec<(i64, i64)> {
    let span = (i128::from(hi) - i128::from(lo) + 1).max(1);
    let chunks = (i128::from(count) / i128::from(target_rows.max(1))).max(1);
    let width = (span / chunks).max(1);
    let mut out = Vec::new();
    let mut start = i128::from(lo);
    while start <= i128::from(hi) {
        let end = (start + width - 1).min(i128::from(hi));
        out.push((start as i64, end as i64));
        start = end + 1;
    }
    out
}

/// Row count of a table.
pub fn count(conn: &Connection, table: &str) -> Result<u64> {
    Ok(
        conn.query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |r| {
            r.get::<_, i64>(0)
        })? as u64,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_cover_exactly_once() {
        for (lo, hi, count, target) in [
            (1, 100, 100, 7),
            (5, 5, 1, 10),
            (10, 1_000_000, 50, 10),
            (-3, 3, 7, 1),
        ] {
            let r = split_range(lo, hi, count, target);
            assert_eq!(r.first().unwrap().0, lo);
            assert_eq!(r.last().unwrap().1, hi);
            for w in r.windows(2) {
                assert_eq!(w[0].1 + 1, w[1].0, "contiguous");
            }
        }
        assert_eq!(split_range(1, 100, 100, 10).len(), 10);
    }

    #[test]
    fn rowid_ranges_of_a_table() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE t (x);
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1000)
             INSERT INTO t SELECT i FROM n;",
        )
        .unwrap();
        let ranges = rowid_ranges(&c, "t", 100).unwrap();
        let total: i64 = ranges
            .iter()
            .map(|(a, b)| {
                c.query_row(
                    "SELECT count(*) FROM t WHERE rowid BETWEEN ?1 AND ?2",
                    [a, b],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap()
            })
            .sum();
        assert_eq!(total, 1000);
        assert!(has_table(&c, "t").unwrap());
        assert!(!has_table(&c, "nope").unwrap());
        assert!(
            rowid_ranges(&Connection::open_in_memory().unwrap(), "sqlite_master", 10)
                .unwrap()
                .is_empty()
        );
    }
}
