//! `GET /healthz`, the uptime monitor's probe (OPS-007, TD-OPS-02, D-3144, D-4044).
//!
//! [`check`] judges the database (`liveness`'s schema-less `SELECT 1`) and the worker (from
//! `core::api::queue_health`: the worker is down only when a task has waited, due with no lease,
//! for longer than `ASKHAVA_WORKER_DOWN_SECS`, 600 by default), so an idle worker and a database
//! no worker has served yet answer 200 (`app/server/tests/door_matrix.rs`). The route answers 200
//! or 503 with a `no-store` JSON body naming the two states and the age in seconds of the newest
//! lease or finish; it shows no secret, contact detail or setting value. [`web_health`] gives
//! T476's health page the process's start time and the same state as one plain value.
//! `check` holds no HTTP type, so T518's worker task can call it in process.

use std::io;
use std::sync::OnceLock;
use std::time::Duration;

use askhava_core::api::{QueueHealth, queue_health};
use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use jiff::{SignedDuration, Timestamp};
use sqlx::PgPool;

/// The setting for how long due work may wait before the worker counts as down (D-3144).
pub const WORKER_DOWN_KEY: &str = "ASKHAVA_WORKER_DOWN_SECS";
const DEFAULT_WORKER_DOWN_SECS: i64 = 600;
/// The longest the probe waits on the queue read, so the monitor gets an answer.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

static LIMITS: OnceLock<HealthLimits> = OnceLock::new();
static STARTED: OnceLock<Timestamp> = OnceLock::new();

/// The probe's thresholds (OPS-007, D-3144).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthLimits {
    worker_down: SignedDuration,
}

impl Default for HealthLimits {
    fn default() -> Self {
        Self {
            worker_down: SignedDuration::from_secs(DEFAULT_WORKER_DOWN_SECS),
        }
    }
}

impl HealthLimits {
    /// Reads `ASKHAVA_WORKER_DOWN_SECS` (60 to 86,400; unset is 600) from `pairs` (D-3144).
    ///
    /// # Errors
    /// A bad value, naming the key, never the value.
    pub fn from_pairs<I: IntoIterator<Item = (String, String)>>(pairs: I) -> io::Result<Self> {
        let Some((_, raw)) = pairs.into_iter().find(|(k, _)| k == WORKER_DOWN_KEY) else {
            return Ok(Self::default());
        };
        match raw.trim().parse::<i64>() {
            Ok(secs @ 60..=86_400) => Ok(Self {
                worker_down: SignedDuration::from_secs(secs),
            }),
            _ => Err(io::Error::other(format!("{WORKER_DOWN_KEY} is invalid"))),
        }
    }
}

/// Sets the process's probe thresholds once, at start (OPS-007); later calls change nothing.
pub fn configure(limits: HealthLimits) {
    let _first_wins = LIMITS.set(limits);
}

/// What the probe found (OPS-007, TD-OPS-02). Holds no value from a setting or a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthState {
    /// The database answered `SELECT 1`.
    pub database_up: bool,
    /// The worker is down: due work has waited longer than the limit. `false` when unknown.
    pub worker_down: bool,
    /// The age of the newest lease or finish; `None` before any worker served this database.
    pub activity_age: Option<SignedDuration>,
}

impl HealthState {
    /// OPS-007: healthy when the database is up and the worker is not down.
    #[must_use]
    pub fn healthy(&self) -> bool {
        self.database_up && !self.worker_down
    }
}

/// The web process's health for T476's page: its start time and [`HealthState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebHealth {
    /// When this process built its router.
    pub started: Timestamp,
    /// What the `/healthz` probe reports now.
    pub state: HealthState,
}

/// OPS-007, D-4044: the database and worker check at `now`, with no HTTP type.
pub(crate) async fn check(pool: &PgPool, now: Timestamp, limits: &HealthLimits) -> HealthState {
    let down = HealthState {
        database_up: false,
        worker_down: false,
        activity_age: None,
    };
    if !crate::liveness::round_trip(pool).await {
        return down;
    }
    match tokio::time::timeout(PROBE_TIMEOUT, queue_health(pool, now)).await {
        Ok(Ok(queue)) => judge(queue, now, limits),
        _ => down,
    }
}

/// D-3144: the worker is down only when the oldest waiting task fell due more than the limit ago.
pub(crate) fn judge(queue: QueueHealth, now: Timestamp, limits: &HealthLimits) -> HealthState {
    let age = |at: Timestamp| now.duration_since(at);
    HealthState {
        database_up: true,
        worker_down: queue
            .oldest_waiting
            .is_some_and(|due| age(due) > limits.worker_down),
        activity_age: queue.newest_activity.map(age),
    }
}

/// T476: this process's start time and the probe's state now (OPS-007, D-3753).
pub async fn web_health(pool: &PgPool) -> WebHealth {
    let now = Timestamp::now();
    WebHealth {
        started: *STARTED.get_or_init(|| now),
        state: check(pool, now, &LIMITS.get().copied().unwrap_or_default()).await,
    }
}

/// The `/healthz` route over `pool`; marks the process's start time (OPS-007).
pub fn routes(pool: PgPool) -> Router {
    STARTED.get_or_init(Timestamp::now);
    Router::new()
        .route("/healthz", get(healthz))
        .with_state(pool)
}

async fn healthz(State(pool): State<PgPool>) -> Response {
    let health = web_health(&pool).await.state;
    let status = if health.healthy() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, [(header::CACHE_CONTROL, "no-store")], body(&health)).into_response()
}

/// The probe's JSON: `ok` or `down`, the database and worker states and the activity age.
pub(crate) fn body(health: &HealthState) -> axum::Json<serde_json::Value> {
    let worker = match (health.database_up, health.worker_down) {
        (false, _) => "unknown",
        (true, true) => "down",
        (true, false) => "up",
    };
    axum::Json(serde_json::json!({
        "status": if health.healthy() { "ok" } else { "down" },
        "database": if health.database_up { "up" } else { "down" },
        "worker": worker,
        "last_activity_secs": health.activity_age.map(|a| a.as_secs().max(0)),
    }))
}
