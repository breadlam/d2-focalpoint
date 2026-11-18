//! Parallel producers, one writer per table.
//!
//! Extraction is CPU-bound and runs on the rayon pool; DuckDB appenders are
//! single-threaded per connection. A [`Sink`] owns a writer thread with its own
//! connection and appender for one table, fed batches over a bounded channel
//! (backpressure keeps memory flat when parsing outruns writing).

use std::thread::JoinHandle;

use anyhow::{Context, Result, anyhow};
use crossbeam_channel::{Receiver, Sender, bounded};
use duckdb::{Appender, Connection};

/// A row that knows how to append itself to its table.
pub trait AppendRow: Send + 'static {
    fn append(&self, appender: &mut Appender<'_>) -> duckdb::Result<()>;
}

/// Writer handle for one table.
#[derive(Debug)]
pub struct Sink<R: AppendRow> {
    tx: Option<Sender<Vec<R>>>,
    handles: Vec<JoinHandle<Result<u64>>>,
    table: &'static str,
}

impl<R: AppendRow> Sink<R> {
    /// Spawn a single writer for `table` on a clone of `conn`.
    pub fn spawn(conn: &Connection, table: &'static str) -> Result<Self> {
        Self::spawn_parallel(conn, table, 1)
    }

    /// Spawn `writers` writer threads for `table`, each with its own
    /// connection and appender, all draining one channel. DuckDB accepts
    /// concurrent appends to a table from separate connections; use this for
    /// high-volume staging tables without uniqueness constraints.
    pub fn spawn_parallel(conn: &Connection, table: &'static str, writers: usize) -> Result<Self> {
        let (tx, rx) = bounded::<Vec<R>>(64 * writers.max(1));
        let handles = (0..writers.max(1))
            .map(|i| {
                let conn = conn
                    .try_clone()
                    .context("cloning DuckDB connection for writer")?;
                let rx = rx.clone();
                std::thread::Builder::new()
                    .name(format!("sink-{table}-{i}"))
                    .spawn(move || write_all(&conn, table, &rx))
                    .context("spawning writer thread")
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            tx: Some(tx),
            handles,
            table,
        })
    }

    /// A cheap, cloneable sender for producers.
    #[must_use]
    pub fn sender(&self) -> BatchSender<R> {
        BatchSender {
            tx: self.tx.clone().expect("sink not finished"),
            table: self.table,
        }
    }

    /// Close the channel and wait for the writers; returns rows written.
    pub fn finish(mut self) -> Result<u64> {
        self.tx.take();
        let mut total = 0;
        let mut first_err = None;
        for h in std::mem::take(&mut self.handles) {
            match h.join() {
                Ok(Ok(n)) => total += n,
                Ok(Err(e)) => {
                    first_err.get_or_insert(e);
                }
                Err(_) => {
                    first_err.get_or_insert(anyhow!("writer thread panicked"));
                }
            }
        }
        match first_err {
            Some(e) => Err(e).with_context(|| format!("writing {}", self.table)),
            None => Ok(total),
        }
    }
}

impl<R: AppendRow> Drop for Sink<R> {
    fn drop(&mut self) {
        // Unfinished sink (error path): close the channel and reap the threads.
        self.tx.take();
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

/// Writer threads for a high-volume table without uniqueness constraints.
///
/// Row-by-row appending costs more CPU than reading and parsing, so writers,
/// not producers, are the bottleneck: with one writer per 32 threads the
/// producers on a 256-core machine idled at ~5 % behind backpressure. One
/// writer per 8 threads (2..=64) keeps both sides busy.
#[must_use]
pub fn staging_writers() -> usize {
    (rayon::current_num_threads() / 8).clamp(2, 64)
}

/// Rows per writer between appender flushes; bounds transaction-local memory.
const FLUSH_EVERY: u64 = 2_000_000;

fn write_all<R: AppendRow>(conn: &Connection, table: &str, rx: &Receiver<Vec<R>>) -> Result<u64> {
    let mut appender = conn.appender(table)?;
    let (mut n, mut since_flush) = (0u64, 0u64);
    for batch in rx {
        for row in &batch {
            row.append(&mut appender)?;
        }
        n += batch.len() as u64;
        since_flush += batch.len() as u64;
        if since_flush >= FLUSH_EVERY {
            appender.flush()?;
            since_flush = 0;
        }
    }
    appender.flush()?;
    Ok(n)
}

/// Producer-side handle.
#[derive(Debug)]
pub struct BatchSender<R> {
    tx: Sender<Vec<R>>,
    table: &'static str,
}

impl<R> Clone for BatchSender<R> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            table: self.table,
        }
    }
}

impl<R: AppendRow> BatchSender<R> {
    pub fn send(&self, batch: Vec<R>) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }
        self.tx
            .send(batch)
            .map_err(|_| anyhow!("writer for {} stopped (see its error)", self.table))
    }
}

/// Accumulates rows and sends them in fixed-size batches.
#[derive(Debug)]
pub struct Batcher<R: AppendRow> {
    sender: BatchSender<R>,
    buf: Vec<R>,
    cap: usize,
}

impl<R: AppendRow> Batcher<R> {
    #[must_use]
    pub fn new(sender: &BatchSender<R>, cap: usize) -> Self {
        Self {
            sender: sender.clone(),
            buf: Vec::with_capacity(cap),
            cap,
        }
    }

    pub fn push(&mut self, row: R) -> Result<()> {
        self.buf.push(row);
        if self.buf.len() >= self.cap {
            self.flush()?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        let batch = std::mem::replace(&mut self.buf, Vec::with_capacity(self.cap));
        self.sender.send(batch)
    }
}

#[cfg(test)]
mod tests {
    use duckdb::params;
    use rayon::prelude::*;

    use super::*;

    struct Pair(i64, String);
    impl AppendRow for Pair {
        fn append(&self, a: &mut Appender<'_>) -> duckdb::Result<()> {
            a.append_row(params![self.0, self.1])
        }
    }

    #[test]
    fn parallel_producers_single_writer() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("s.duckdb")).unwrap();
        conn.execute_batch("CREATE TABLE t (a BIGINT, b VARCHAR)")
            .unwrap();
        let sink = Sink::<Pair>::spawn(&conn, "t").unwrap();
        let sender = sink.sender();
        (0..8i64).into_par_iter().for_each(|chunk| {
            let mut b = Batcher::new(&sender, 7);
            for i in 0..100 {
                b.push(Pair(chunk * 100 + i, format!("r{i}"))).unwrap();
            }
            b.flush().unwrap();
        });
        drop(sender);
        assert_eq!(sink.finish().unwrap(), 800);
        let (n, distinct): (i64, i64) = conn
            .query_row("SELECT count(*), count(DISTINCT a) FROM t", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((n, distinct), (800, 800));
    }

    #[test]
    fn parallel_writers_write_every_row_once() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("p.duckdb")).unwrap();
        conn.execute_batch("CREATE TABLE t (a BIGINT, b VARCHAR)")
            .unwrap();
        let sink = Sink::<Pair>::spawn_parallel(&conn, "t", 4).unwrap();
        let sender = sink.sender();
        (0..16i64).into_par_iter().for_each(|chunk| {
            let mut b = Batcher::new(&sender, 100);
            for i in 0..1000 {
                b.push(Pair(chunk * 1000 + i, String::new())).unwrap();
            }
            b.flush().unwrap();
        });
        drop(sender);
        assert_eq!(sink.finish().unwrap(), 16_000);
        let (n, distinct): (i64, i64) = conn
            .query_row("SELECT count(*), count(DISTINCT a) FROM t", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((n, distinct), (16_000, 16_000));
    }

    #[test]
    fn writer_errors_surface_on_finish() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (a BIGINT NOT NULL, b VARCHAR)")
            .unwrap();
        let sink = Sink::<Pair>::spawn(&conn, "missing_table").unwrap();
        let _ = sink.sender().send(vec![Pair(1, "x".into())]);
        assert!(sink.finish().is_err());
    }
}
