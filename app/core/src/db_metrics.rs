//! Database measurements, numbers only: statements run and pool-acquire waits (D-3292, D-3298).
//!
//! sqlx 0.9 reports each executed statement as an event on the target `sqlx::query` (statement
//! logging is on by default at DEBUG; `db::app_pool` sets it explicitly) and each pool acquire on
//! `sqlx::pool::acquire` with the field `acquired_after_secs` (turned on by
//! `acquire_time_level`). An acquire slower than `acquire_slow_threshold` emits only sqlx's
//! "exceeded slow threshold" event, so both acquire events count. [`MetricsLayer`] reads the
//! event's target and that one float, and nothing else: never the SQL text in `db.statement` or
//! `summary`, a bind value or the database URL (SEC-001, PRIV-016). The JSON log never sees a
//! `sqlx::query` event (`logging::subscriber` gives it its own per-layer filter, which drops the
//! target at every level, since a slow statement arrives at WARN with the full SQL text).
//!
//! The counters only grow. A reader (the test-control route `GET /__test/perf/db`, the route
//! benchmark, the war games) takes two [`DbSnapshot`]s and subtracts: the statement difference
//! with one request in flight is that request's queries, and the bucket differences give a
//! nearest-rank p95 of the acquire wait. The maximum is since the process started. sqlx emits no
//! event for an acquire that times out, so timeouts are not counted here; the benchmark's error
//! rate covers them. Nothing here changes how a request behaves.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::Duration;

use serde::Serialize;
use sqlx::PgPool;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// sqlx's target for one executed statement.
pub(crate) const STATEMENT_TARGET: &str = "sqlx::query";
/// sqlx's target for one pool acquire.
pub(crate) const ACQUIRE_TARGET: &str = "sqlx::pool::acquire";
/// The one field read from an acquire event.
const WAIT_FIELD: &str = "acquired_after_secs";
/// Upper bounds of the wait buckets in microseconds, log-scale from 0.1 ms to 5 s; one more
/// bucket holds every wait above the last bound (D-3298).
pub const WAIT_BUCKETS_US: [u64; 15] = [
    100, 250, 500, 1_000, 2_500, 5_000, 10_000, 25_000, 50_000, 100_000, 250_000, 500_000,
    1_000_000, 2_500_000, 5_000_000,
];
const BUCKETS: usize = WAIT_BUCKETS_US.len() + 1;

static GLOBAL: LazyLock<Arc<DbMetrics>> = LazyLock::new(|| Arc::new(DbMetrics::new()));

/// The process's own counters: the ones `logging::init` installs, `db::app_pool` registers its
/// pool with and `GET /__test/perf/db` reads (D-3292).
#[must_use]
pub fn global() -> Arc<DbMetrics> {
    Arc::clone(&GLOBAL)
}

/// Counters for statements and acquire waits, plus the pool whose size a snapshot reads.
#[derive(Debug, Default)]
pub struct DbMetrics {
    statements: AtomicU64,
    acquires: AtomicU64,
    wait_sum_us: AtomicU64,
    wait_max_us: AtomicU64,
    buckets: [AtomicU64; BUCKETS],
    pool: Mutex<Option<PgPool>>,
}

/// One reading of [`DbMetrics`]: numbers only (SEC-001, PRIV-016).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DbSnapshot {
    /// Statements executed since start.
    pub statements: u64,
    /// Pool acquires since start.
    pub acquires: u64,
    /// The sum of every acquire wait, in microseconds.
    pub wait_sum_us: u64,
    /// The longest acquire wait since start, in microseconds.
    pub wait_max_us: u64,
    /// Acquire counts per wait bucket, in the order of [`WAIT_BUCKETS_US`].
    pub buckets: Vec<WaitBucket>,
    /// The registered pool's size, idle count and maximum; `None` before a pool is registered.
    pub pool: Option<PoolSnapshot>,
    /// Always `"not exposed"`: sqlx emits no event for a timed-out acquire.
    pub acquire_timeouts: &'static str,
}

/// The acquires whose wait was at most `le_us` and above the previous bucket's bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WaitBucket {
    /// The bucket's upper bound in microseconds; `None` for the bucket above 5 s.
    pub le_us: Option<u64>,
    /// How many acquires landed in it.
    pub count: u64,
}

/// The pool's connections at the time of the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PoolSnapshot {
    /// Open connections, idle or in use.
    pub size: u32,
    /// Open connections not in use.
    pub idle: u64,
    /// The pool's `max_connections`.
    pub max: u32,
}

impl DbMetrics {
    /// Empty counters with no pool, for a test's own subscriber.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps a handle to `pool` so a snapshot can read its size; a later pool replaces it.
    pub fn register_pool(&self, pool: &PgPool) {
        *self.pool.lock().unwrap_or_else(PoisonError::into_inner) = Some(pool.clone());
    }

    /// The layer that feeds these counters; give it [`logging::metrics_filter`](crate::logging::metrics_filter).
    #[must_use]
    pub fn layer(self: &Arc<Self>) -> MetricsLayer {
        MetricsLayer {
            metrics: Arc::clone(self),
        }
    }

    /// Reads every counter and the registered pool (D-3292).
    #[must_use]
    pub fn snapshot(&self) -> DbSnapshot {
        let bounds = WAIT_BUCKETS_US.iter().copied().map(Some).chain([None]);
        DbSnapshot {
            statements: self.statements.load(Ordering::Relaxed),
            acquires: self.acquires.load(Ordering::Relaxed),
            wait_sum_us: self.wait_sum_us.load(Ordering::Relaxed),
            wait_max_us: self.wait_max_us.load(Ordering::Relaxed),
            buckets: bounds
                .zip(&self.buckets)
                .map(|(le_us, count)| WaitBucket {
                    le_us,
                    count: count.load(Ordering::Relaxed),
                })
                .collect(),
            pool: self.pool_snapshot(),
            acquire_timeouts: "not exposed",
        }
    }

    fn pool_snapshot(&self) -> Option<PoolSnapshot> {
        let pool = self.pool.lock().unwrap_or_else(PoisonError::into_inner);
        pool.as_ref().map(|pool| PoolSnapshot {
            size: pool.size(),
            idle: u64::try_from(pool.num_idle()).unwrap_or(u64::MAX),
            max: pool.options().get_max_connections(),
        })
    }

    fn record_statement(&self) {
        self.statements.fetch_add(1, Ordering::Relaxed);
    }

    fn record_acquire(&self, wait_secs: f64) {
        let wait_us = micros(wait_secs);
        self.acquires.fetch_add(1, Ordering::Relaxed);
        self.wait_sum_us.fetch_add(wait_us, Ordering::Relaxed);
        self.wait_max_us.fetch_max(wait_us, Ordering::Relaxed);
        self.buckets[bucket_of(wait_us)].fetch_add(1, Ordering::Relaxed);
    }
}

/// Whole microseconds from sqlx's float seconds; a negative or non-finite value counts as 0.
fn micros(secs: f64) -> u64 {
    Duration::try_from_secs_f64(secs).map_or(0, |wait| {
        u64::try_from(wait.as_micros()).unwrap_or(u64::MAX)
    })
}

/// The index of the first bucket whose bound holds `wait_us`, else the overflow bucket.
fn bucket_of(wait_us: u64) -> usize {
    WAIT_BUCKETS_US
        .iter()
        .position(|bound| wait_us <= *bound)
        .unwrap_or(WAIT_BUCKETS_US.len())
}

/// The tracing layer that counts sqlx's statement and acquire events (D-3292, D-3298).
/// It stores and logs nothing but numbers (SEC-001, PRIV-016).
#[derive(Debug, Clone)]
pub struct MetricsLayer {
    metrics: Arc<DbMetrics>,
}

impl<S: Subscriber> Layer<S> for MetricsLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        match event.metadata().target() {
            STATEMENT_TARGET => self.metrics.record_statement(),
            ACQUIRE_TARGET => {
                let mut wait = WaitVisitor(None);
                event.record(&mut wait);
                if let Some(secs) = wait.0 {
                    self.metrics.record_acquire(secs);
                }
            }
            _ => {}
        }
    }
}

/// Reads `acquired_after_secs` and ignores every other field, unread (PRIV-016).
struct WaitVisitor(Option<f64>);

impl Visit for WaitVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        if field.name() == WAIT_FIELD {
            self.0 = Some(value);
        }
    }

    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
}
