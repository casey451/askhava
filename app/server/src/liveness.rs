//! The liveness exits of `web` and `worker` (OPS-049, TD-OPS-24, D-492, D-3881).
//!
//! The host restarts a service only when its process exits, so each process watches itself: every
//! 10 seconds `web` checks a database round trip and `worker` that round trip and a new pass of its
//! claim loop (`core::api::ClaimTick`). When the check has failed without a break for
//! `ASKHAVA_LIVENESS_LIMIT_SECS` (180 by default), the watch ends with an error and the binary
//! exits non-zero, so the On Failure policy restarts it. One good check resets the clock
//! ([`FailClock`]). The error and its one log line name no value.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use askhava_core::api::ClaimTick;
use sqlx::PgPool;
use tokio::time::Instant;

/// The setting for how long a liveness check may fail before the process exits (OPS-049).
pub const LIMIT_KEY: &str = "ASKHAVA_LIVENESS_LIMIT_SECS";
const DEFAULT_LIMIT_SECS: u64 = 180;
/// How often the watch checks.
const EVERY: Duration = Duration::from_secs(10);
/// The longest one database round trip may take before it counts as failed.
const ROUND_TRIP_TIMEOUT: Duration = Duration::from_secs(5);

/// How long the check may fail in a row before the process exits (OPS-049).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LivenessLimit(Duration);

impl Default for LivenessLimit {
    fn default() -> Self {
        Self(Duration::from_secs(DEFAULT_LIMIT_SECS))
    }
}

impl LivenessLimit {
    /// Reads `ASKHAVA_LIVENESS_LIMIT_SECS` (150 to 3,600; unset is 180) from `pairs` (OPS-049).
    /// The floor outlasts one 120-second handler run plus a check, when no tick moves (D-4224).
    ///
    /// # Errors
    /// A bad value, naming the key, never the value.
    pub fn from_pairs<I: IntoIterator<Item = (String, String)>>(pairs: I) -> io::Result<Self> {
        let Some((_, raw)) = pairs.into_iter().find(|(k, _)| k == LIMIT_KEY) else {
            return Ok(Self::default());
        };
        match raw.trim().parse::<u64>() {
            Ok(secs @ 150..=3_600) => Ok(Self(Duration::from_secs(secs))),
            _ => Err(io::Error::other(format!("{LIMIT_KEY} is invalid"))),
        }
    }
}

/// OPS-049, TD-OPS-24: when the current run of failed checks began; expired once it has lasted
/// the limit. A passing check clears it. Times are Tokio's, so a test can run the clock.
#[derive(Debug)]
pub struct FailClock {
    limit: LivenessLimit,
    failing_since: Option<Instant>,
}

impl FailClock {
    /// A clock with no failure yet.
    #[must_use]
    pub fn new(limit: LivenessLimit) -> Self {
        Self {
            limit,
            failing_since: None,
        }
    }

    /// Records one check at `now`; `true` when the checks have failed for the whole limit.
    pub fn observe(&mut self, ok: bool, now: Instant) -> bool {
        if ok {
            self.failing_since = None;
            return false;
        }
        let since = *self.failing_since.get_or_insert(now);
        now.saturating_duration_since(since) >= self.limit.0
    }
}

/// OPS-049: `web`'s watch; returns only when the database round trip has failed for the limit.
pub async fn watch_web(pool: PgPool, limit: LivenessLimit) -> io::Error {
    watch(&pool, limit, || true).await
}

/// OPS-049: `worker`'s watch; returns only when the round trip has failed, or `tick` has not
/// moved between two checks, for the limit.
pub async fn watch_worker(pool: PgPool, tick: Arc<ClaimTick>, limit: LivenessLimit) -> io::Error {
    let mut last = tick.count();
    let moved = move || {
        let now = tick.count();
        let moved = now != last;
        last = now;
        moved
    };
    watch(&pool, limit, moved).await
}

async fn watch(
    pool: &PgPool,
    limit: LivenessLimit,
    mut progressed: impl FnMut() -> bool,
) -> io::Error {
    let mut clock = FailClock::new(limit);
    let mut every = tokio::time::interval(EVERY);
    loop {
        every.tick().await;
        let ok = progressed() & round_trip(pool).await;
        if clock.observe(ok, Instant::now()) {
            tracing::error!("the liveness check failed for the whole limit; exiting");
            return io::Error::other("the liveness check failed for the whole limit");
        }
    }
}

/// One database round trip under [`ROUND_TRIP_TIMEOUT`] (OPS-049).
pub(crate) async fn round_trip(pool: &PgPool) -> bool {
    let trip = sqlx::query_scalar!(r#"SELECT 1 AS "one!""#).fetch_one(pool);
    matches!(
        tokio::time::timeout(ROUND_TRIP_TIMEOUT, trip).await,
        Ok(Ok(_))
    )
}
