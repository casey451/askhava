//! The Stripe-or-webhook-errors alert (T518; OPS-004, OPS-023, D-220, D-3877, D-3882, D-4280,
//! D-4880, D-5670, D-5876).
//!
//! OPS-004 alerts Casey when "Stripe or webhook errors last over 30 minutes". Two parts:
//!
//! - [`layer`], on the merged `/webhooks` routers (`lib.rs` `webhook_routes`), writes one `ops`
//!   audit row [`WEBHOOK_ERROR_ACTION`] (the route's code name as subject kind, a fresh UUID as
//!   subject, no body, no sender) for each 5xx answer, and one [`WEBHOOK_OK_ACTION`] row for a
//!   route's first 2xx in each 5-minute window through `ops::api::audit_once` (subject ID from the
//!   route and the window, so a second 2xx writes nothing). It keeps no memory, so this holds
//!   across restarts and processes. Each row is spawned after the answer is decided, in its own
//!   transaction on a pool connection; a failed write is logged and never changes the answer. A
//!   request whose future is dropped or unwinds before it answers (the outer time limit or panic
//!   layer) counts as a 5xx through a drop guard (D-5876). A refused signature (403) is a security
//!   event, not an error. A 5xx while the database is down cannot be recorded; T624's health check
//!   covers that.
//! - [`CHECK_KIND`], a worker task once per 5-minute slot (`health_watch`'s slots and chain),
//!   reads the Stripe calls `payments` recorded (`payments::api::stripe_calls`) and the webhook
//!   rows (`ops::api::audit_subjects`) since the last run, up to [`SETTLE`] before `now` so a row
//!   still committing is read next time. Error runs are tracked per source: Stripe, and each
//!   webhook route. A Stripe run is broken only by a recorded successful Stripe call, a route's run
//!   only by its own `server.webhook_ok` row. A run whose error comes [`RUN_LIMIT`] or more after
//!   its first error raises one High `ops.vendor_errors` alert through `ops::api::raise_kind_once`
//!   (none while one is open). The run starts, the scanned end and the last error travel in the
//!   next run's payload (D-4280), never in memory or a table; the start-up seed reads the last 2
//!   hours. Each run clears an open alert (`ops::api::acknowledge_cleared`, OPS-023) when no error
//!   was seen for `ops::api::VENDOR_QUIET` (2 hours). In dev and ci nothing is raised or cleared.

use std::collections::BTreeMap;
use std::sync::Arc;

use askhava_core::api::{Clock, Environment, HandlerError, NewTask, Registry, TaskError, enqueue};
use askhava_ops::api::{
    Actor, AuditEntry, Change, Severity, VENDOR_ALERT, VENDOR_QUIET, acknowledge_cleared, audit,
    audit_once, audit_subjects, raise_kind_once, vendor_quiet,
};
use askhava_payments::api::{STRIPE_WEBHOOK_PATH, stripe_calls};
use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use jiff::{SignedDuration, Timestamp};
use serde_json::{Map, Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::health_watch::{SLOT_SECS, next_slot, slot_of};

/// The `core.task` kind of the 5-minute Stripe-or-webhook-errors check.
pub const CHECK_KIND: &str = "ops.vendor_errors_check";
/// The audit action of a webhook answered with a 5xx (or never answered).
pub const WEBHOOK_ERROR_ACTION: &str = "server.webhook_error";
/// The audit action of a route's first 2xx answer in a 5-minute window.
pub const WEBHOOK_OK_ACTION: &str = "server.webhook_ok";
/// The source name of `payments`' Stripe calls in a run's payload.
pub const STRIPE_SOURCE: &str = "payments.stripe";
/// OPS-004: an error run raises the alert once an error comes this long after its first.
pub const RUN_LIMIT: SignedDuration = SignedDuration::from_mins(30);
/// How far behind `now` a run stops reading, so rows still committing are read by the next run.
pub const SETTLE: SignedDuration = SignedDuration::from_mins(1);
/// Each `/webhooks` path and its route code name; the index is part of the window's subject ID.
const ROUTES: [(&str, &str); 7] = [
    (STRIPE_WEBHOOK_PATH, "webhooks.stripe"),
    (crate::inbound_text::INBOUND_PATH, "webhooks.twilio_sms"),
    (crate::inbound_text::STATUS_PATH, "webhooks.twilio_status"),
    (crate::inbound_voice::VOICE_PATH, "webhooks.twilio_voice"),
    (
        crate::voice_status::STATUS_PATH,
        "webhooks.twilio_voice_status",
    ),
    (crate::voice_status::KEY_PATH, "webhooks.twilio_safety_key"),
    (
        crate::call_status::STATUS_PATH,
        "webhooks.twilio_dialed_status",
    ),
];
/// The code name of a `/webhooks` path not in [`ROUTES`].
const OTHER_ROUTE: &str = "webhooks.other";
/// The handler's failure when the rows, the queue or the alert table cannot be reached.
const DB_FAILED: HandlerError =
    HandlerError("the vendor errors check could not reach the database");
/// The handler's failure when a run's payload is not the one this module writes.
const BAD_PAYLOAD: HandlerError =
    HandlerError("the vendor errors check run has a malformed payload");

/// One webhook route: its place in [`ROUTES`] and its code name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Route {
    index: u64,
    name: &'static str,
}

impl Route {
    /// The route of a request path.
    fn of(path: &str) -> Self {
        let found = ROUTES.iter().zip(0_u64..).find(|((p, _), _)| *p == path);
        found.map_or(
            Self {
                index: u64::MAX,
                name: OTHER_ROUTE,
            },
            |((_, name), index)| Self { index, name },
        )
    }
}

/// OPS-004, D-3877, D-4880: `router` with the outcome layer: a 5xx (or no answer) writes one
/// [`WEBHOOK_ERROR_ACTION`] row, a route's first 2xx in a 5-minute window of `clock` one
/// [`WEBHOOK_OK_ACTION`] row; the answer itself is never changed.
pub fn layer(router: Router, pool: PgPool, clock: Arc<dyn Clock>) -> Router {
    router.layer(axum::middleware::from_fn_with_state(
        (pool, clock),
        record_outcome,
    ))
}

/// Writes the error row when dropped while still armed: the request never answered (D-5876).
struct Unanswered {
    pool: Option<PgPool>,
    route: Route,
}

impl Drop for Unanswered {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.take() {
            spawn_write(pool, self.route, None);
        }
    }
}

/// The layer's body: runs the request, then spawns the row its status calls for.
async fn record_outcome(
    State((pool, clock)): State<(PgPool, Arc<dyn Clock>)>,
    request: Request,
    next: Next,
) -> Response {
    let route = Route::of(request.uri().path());
    let mut guard = Unanswered {
        pool: Some(pool),
        route,
    };
    let response = next.run(request).await;
    if let Some(pool) = guard.pool.take() {
        let status = response.status();
        if status.is_server_error() {
            spawn_write(pool, route, None);
        } else if status.is_success() {
            spawn_write(pool, route, Some(slot_of(clock.now())));
        }
    }
    response
}

/// Spawns the error row (`slot` `None`) or the window's ok row; a failure is logged, by route only.
fn spawn_write(pool: PgPool, route: Route, slot: Option<i64>) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        tracing::warn!(
            route = route.name,
            "a webhook outcome had no runtime to record it"
        );
        return;
    };
    runtime.spawn(async move {
        if write_outcome(&pool, route, slot).await.is_none() {
            tracing::warn!(
                route = route.name,
                "a webhook outcome could not be recorded"
            );
        }
    });
}

/// Writes one outcome row on a fresh pool connection, in its own transaction; `None` on failure.
async fn write_outcome(pool: &PgPool, route: Route, slot: Option<i64>) -> Option<()> {
    let mut conn = pool.acquire().await.ok()?;
    let Some(slot) = slot else {
        let entry = entry(WEBHOOK_ERROR_ACTION, route.name, Uuid::new_v4());
        let mut tx = conn.begin().await.ok()?;
        audit(&mut tx, &entry).await.ok()?;
        return tx.commit().await.ok();
    };
    let subject = Uuid::from_u64_pair(route.index, u64::from_ne_bytes(slot.to_ne_bytes()));
    let entry = entry(WEBHOOK_OK_ACTION, route.name, subject);
    audit_once(&mut conn, &entry).await.ok().map(|_| ())
}

/// A system audit entry of `action` about `(kind, id)`.
fn entry(action: &'static str, kind: &'static str, id: Uuid) -> AuditEntry {
    AuditEntry {
        actor: Actor::System,
        action,
        subject_kind: kind,
        subject_id: id,
        change: Change::Action,
    }
}

/// What one 5-minute run hands the next (D-4280): its slot, the end of the window it read, the
/// last error seen and each open error run's first error, by source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VendorRun {
    /// The 5-minute slot this run is due in, the one its dedupe key names.
    pub slot: i64,
    /// Where the previous run stopped reading; `None` on a start-up seed.
    pub since: Option<Timestamp>,
    /// The latest error any run saw.
    pub last_error: Option<Timestamp>,
    /// The first error of each source's open run (no success since).
    pub runs: BTreeMap<String, Timestamp>,
}

/// A time as microseconds in a payload.
fn micros(at: Timestamp) -> Value {
    Value::from(at.as_microsecond())
}

/// A payload time: `null` is `Some(None)`, a whole microsecond count `Some(Some(at))`.
fn time_of(value: &Value) -> Option<Option<Timestamp>> {
    if value.is_null() {
        return Some(None);
    }
    Timestamp::from_microsecond(value.as_i64()?).ok().map(Some)
}

impl VendorRun {
    /// The run's payload: `{"slot", "since", "last_error", "runs": {source: micros}}`.
    #[must_use]
    pub fn to_payload(&self) -> Value {
        let runs: Map<String, Value> = self
            .runs
            .iter()
            .map(|(source, at)| (source.clone(), micros(*at)))
            .collect();
        json!({
            "slot": self.slot,
            "since": self.since.map_or(Value::Null, micros),
            "last_error": self.last_error.map_or(Value::Null, micros),
            "runs": runs,
        })
    }

    /// Parses [`Self::to_payload`]'s form; anything else is `None`.
    #[must_use]
    pub fn from_payload(payload: &Value) -> Option<Self> {
        let mut runs = BTreeMap::new();
        for (source, at) in payload.get("runs")?.as_object()? {
            runs.insert(source.clone(), time_of(at)??);
        }
        Some(Self {
            slot: payload.get("slot")?.as_i64()?,
            since: time_of(payload.get("since")?)?,
            last_error: time_of(payload.get("last_error")?)?,
            runs,
        })
    }

    /// OPS-004, D-3877: folds `events` (time, source, failed), oldest first, into the runs; true
    /// when an error came [`RUN_LIMIT`] or more after its run's first error.
    pub fn fold(&mut self, events: &[(Timestamp, String, bool)]) -> bool {
        let mut over = false;
        for (at, source, failed) in events {
            if !*failed {
                self.runs.remove(source);
                continue;
            }
            self.last_error = Some(self.last_error.map_or(*at, |last| last.max(*at)));
            let start = *self.runs.entry(source.clone()).or_insert(*at);
            over |= at.duration_since(start) >= RUN_LIMIT;
        }
        over
    }
}

/// OPS-004, D-3877: registers [`CHECK_KIND`]; `environment` decides whether it raises and clears
/// (test and live only) and `clock` gives each run's `now`.
pub fn register(registry: &mut Registry, environment: Environment, clock: Arc<dyn Clock>) {
    registry.register(CHECK_KIND, move |pool, task| {
        let now = clock.now();
        async move { run_payload(&pool, environment, &task.payload, now).await }
    });
}

/// JOB-006, D-4280: queues the run for the slot holding `now`, due at its start, with no carried
/// state; a second call in the same slot queues nothing.
///
/// # Errors
/// [`TaskError::Db`] when the queue cannot be reached.
pub async fn schedule(db: &mut PgConnection, now: Timestamp) -> Result<(), TaskError> {
    let run = VendorRun {
        slot: slot_of(now),
        ..VendorRun::default()
    };
    queue(db, &run).await
}

/// Queues `run`, due at its slot's start, under `<kind>.<slot>`.
async fn queue(db: &mut PgConnection, run: &VendorRun) -> Result<(), TaskError> {
    let start = run.slot.saturating_mul(SLOT_SECS);
    let due = Timestamp::from_second(start).map_err(|_| TaskError::Db)?;
    let mut task = NewTask::new(CHECK_KIND, run.to_payload(), due);
    task.dedupe_key = Some(format!("{CHECK_KIND}.{}", run.slot));
    enqueue(db, &task).await.map(|_| ())
}

/// The handler body: parses `payload` once, then [`run_check`].
///
/// # Errors
/// A fixed sentence for a malformed payload or an unreachable database.
pub async fn run_payload(
    pool: &PgPool,
    environment: Environment,
    payload: &Value,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let run = VendorRun::from_payload(payload).ok_or(BAD_PAYLOAD)?;
    run_check(pool, environment, run, now).await
}

/// OPS-004, OPS-023, D-220, D-3877: reads the window since the last run, folds it into the error
/// runs, then in one transaction queues the next slot's run with the new state and, in test and
/// live, raises one High [`VENDOR_ALERT`] for a run over 30 minutes or clears an open one after
/// 2 quiet hours.
///
/// # Errors
/// A fixed sentence when the rows, the queue or the alert table cannot be reached.
pub async fn run_check(
    pool: &PgPool,
    environment: Environment,
    mut run: VendorRun,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let to = now - SETTLE;
    let from = run.since.unwrap_or(to - VENDOR_QUIET).min(to);
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    let events = window_events(&mut conn, from, to).await?;
    let over = run.fold(&events);
    let quiet = run.last_error.is_none_or(|last| vendor_quiet(last, now));
    let next = VendorRun {
        slot: next_slot(run.slot, now),
        since: Some(to.max(from)),
        ..run
    };
    let mut tx = conn.begin().await.map_err(|_| DB_FAILED)?;
    queue(&mut tx, &next).await.map_err(|_| DB_FAILED)?;
    if !matches!(environment, Environment::Dev | Environment::Ci) {
        alert(&mut tx, over, quiet, now).await?;
    }
    tx.commit().await.map_err(|_| DB_FAILED)
}

/// Every Stripe call and webhook outcome in `[from, to)` as (time, source, failed), oldest first,
/// a failure before a success at the same time.
async fn window_events(
    db: &mut PgConnection,
    from: Timestamp,
    to: Timestamp,
) -> Result<Vec<(Timestamp, String, bool)>, HandlerError> {
    let calls = stripe_calls(db, from, to).await.map_err(|_| DB_FAILED)?;
    let mut events: Vec<_> = calls
        .into_iter()
        .map(|c| (c.at, STRIPE_SOURCE.to_owned(), c.failed))
        .collect();
    for (action, failed) in [(WEBHOOK_ERROR_ACTION, true), (WEBHOOK_OK_ACTION, false)] {
        let rows = audit_subjects(db, action, from, to)
            .await
            .map_err(|_| DB_FAILED)?;
        events.extend(rows.into_iter().map(|r| (r.at, r.subject_kind, failed)));
    }
    events.sort_by_key(|(at, _, failed)| (*at, !*failed));
    Ok(events)
}

/// Raises the alert for a run over the limit, else clears an open one after the quiet time.
async fn alert(
    db: &mut PgConnection,
    over: bool,
    quiet: bool,
    now: Timestamp,
) -> Result<(), HandlerError> {
    if over {
        let raised = raise_kind_once(db, VENDOR_ALERT, Severity::High)
            .await
            .map_err(|_| DB_FAILED)?;
        if raised.is_some() {
            tracing::error!("Stripe or webhook errors lasted over 30 minutes");
        }
    } else if quiet {
        acknowledge_cleared(db, VENDOR_ALERT, now)
            .await
            .map_err(|_| DB_FAILED)?;
    }
    Ok(())
}
