//! The production worker's `payments` jobs (T433; FEE-019, FEE-020, FEE-021, FEE-026, FEE-027,
//! FEE-029; D-2286, D-3134, D-3468).
//!
//! [`register`] adds the seven `payments` kinds that need Stripe ([`KINDS`]) to the worker
//! registry, each through its existing `run_*` function in `payments`, over the one
//! `StripePayments` client `worker_registry` builds from the Stripe webhook's settings (D-3118):
//! the charge check, the payment timer, the refund retry and check, the late-success refund, the
//! dispute evidence submit and the nightly reconcile. They are registered on every path; with no
//! Stripe settings (dev and CI only; test and live refuse to start without them) a run does nothing
//! and ends done. [`schedule_reconcile`] seeds the nightly reconcile at start under the dedupe key
//! the run itself queues its next night with, so two starts queue one run.

use std::sync::Arc;

use askhava_adapters::api::StripePayments;
use askhava_core::api::{Claimed, Clock, HandlerError, NewTask, Registry, TaskError, enqueue};
use askhava_payments::api::{
    CHECK_KIND, LATE_KIND, RECONCILE_KIND, REFUND_CHECK_KIND, RETRY_KIND, SUBMIT_KIND, TIMER_KIND,
    run_charge_check, run_dispute_submit, run_late_refund, run_pay_timer, run_reconcile,
    run_refund_check, run_refund_retry,
};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::json;
use sqlx::{PgConnection, PgPool};

/// The `payments` task kinds this module registers (D-2286).
pub const KINDS: [&str; 7] = [
    CHECK_KIND,
    TIMER_KIND,
    RETRY_KIND,
    REFUND_CHECK_KIND,
    LATE_KIND,
    SUBMIT_KIND,
    RECONCILE_KIND,
];

/// The UTC hour the nightly reconcile runs: 10:00 UTC is 3:00 a.m. in Arizona (FEE-029).
pub const RECONCILE_HOUR_UTC: i64 = 10;

const DAY_SECONDS: i64 = 86_400;

/// What every registered run shares: the Stripe client, `ID_SECRET` (idempotency keys) and the
/// worker clock.
struct Parts {
    stripe: StripePayments,
    secret: SecretString,
    clock: Arc<dyn Clock>,
}

/// Registers every kind of [`KINDS`] (FEE-019, FEE-020, FEE-021, FEE-026, FEE-027, FEE-029): with
/// `stripe`, each through its `payments` `run_*` function at the clock's now; with none (dev and
/// CI, D-3134), a run that does nothing and ends done.
pub fn register(
    registry: &mut Registry,
    stripe: Option<StripePayments>,
    secret: SecretString,
    clock: Arc<dyn Clock>,
) {
    let Some(stripe) = stripe else {
        for kind in KINDS {
            registry.register(kind, |_pool, _task| async { Ok(()) });
        }
        return;
    };
    let parts = Arc::new(Parts {
        stripe,
        secret,
        clock,
    });
    for kind in KINDS {
        let parts = Arc::clone(&parts);
        registry.register(kind, move |pool, task| run(Arc::clone(&parts), pool, task));
    }
}

/// Runs one claimed `payments` task through its `run_*` function (JOB-006: each is safe to run
/// twice).
async fn run(parts: Arc<Parts>, pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let (stripe, secret) = (&parts.stripe, &parts.secret);
    let (payload, now) = (&task.payload, parts.clock.now());
    match task.kind.as_str() {
        CHECK_KIND => run_charge_check(&pool, stripe, payload, now).await,
        TIMER_KIND => run_pay_timer(&pool, stripe, payload, now).await,
        RETRY_KIND => run_refund_retry(&pool, stripe, secret, payload, now).await,
        REFUND_CHECK_KIND => run_refund_check(&pool, stripe, payload, now).await,
        LATE_KIND => run_late_refund(&pool, stripe, secret, payload, now).await,
        SUBMIT_KIND => run_dispute_submit(&pool, stripe, secret, payload, now).await,
        RECONCILE_KIND => run_reconcile(&pool, stripe, secret, payload, now).await,
        _ => Err(HandlerError("not a payments job")),
    }
}

/// The first nightly reconcile after `now`: the next [`RECONCILE_HOUR_UTC`], under
/// `payments.reconcile.<day>`, the key `run_reconcile` queues its next night with, so a restart,
/// or a start after a run already queued that night, queues nothing more (FEE-029, JOB-006).
///
/// # Errors
/// [`TaskError`] when the insert fails.
pub async fn schedule_reconcile(conn: &mut PgConnection, now: Timestamp) -> Result<(), TaskError> {
    let today = now.as_second().div_euclid(DAY_SECONDS) * DAY_SECONDS;
    let mut at = today + RECONCILE_HOUR_UTC * 3_600;
    if at <= now.as_second() {
        at += DAY_SECONDS;
    }
    let run_at = Timestamp::from_second(at).unwrap_or(now);
    let mut task = NewTask::new(RECONCILE_KIND, json!({}), run_at);
    let day = at.div_euclid(DAY_SECONDS);
    task.dedupe_key = Some(format!("{RECONCILE_KIND}.{day}"));
    enqueue(conn, &task).await.map(|_| ())
}
