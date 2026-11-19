//! The held release (OPS-001, OPS-024, FEE-017, FEE-021, PICK-022, SEC-030; T408, D-133, D-3144,
//! D-3263, D-4020, D-4419). In safe mode `PickReleaser::settle_and_release` calls [`hold`], which
//! queues one [`HELD_RELEASE_KIND`] task per pick. [`run_held_release`] re-enqueues itself
//! [`HOLD_EVERY`] after its own due time while safe mode lasts (a rerun's same key is refused);
//! once it ends, `payments::api::held_release` clears the pick again (the only way, SEC-030) and
//! the releaser's settle (FEE-017) and release (the row is the once) run.

use askhava_adapters::api::PaymentsAdapter;
use askhava_core::api::{HandlerError, NewTask, Registry, enqueue};
use askhava_ops::api::safe_mode_on;
use askhava_payments::api::{ReleaseError, StripeWebhook, held_release};
use askhava_picks::api::{Released, is_released};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::release_sink::{PickReleaser, StripeHooks};

/// The held-release task: payload `{"pick", "run_at"}` (OPS-024, D-3144).
pub const HELD_RELEASE_KIND: &str = "payments.held_release";
/// How long a held release waits before it reads safe mode again (OPS-024).
pub const HOLD_EVERY: SignedDuration = SignedDuration::from_mins(15);

const BAD_PAYLOAD: &str = "bad payments.held_release payload";
const NO_STRIPE: &str = "no Stripe settings for a held release";
const HELD_FAILED: &str = "the held release failed";

/// Holds pick `pick` while safe mode is on (OPS-001, OPS-024, PICK-022): `Already` for a pick
/// already released, else its one held-release task is queued (deduplicated per pick) due
/// [`HOLD_EVERY`] after `at`, and the answer is `MovedOn` (nothing released now).
///
/// # Errors
/// [`ReleaseError::Database`] when the release read or the enqueue fails.
pub(crate) async fn hold(
    conn: &mut PgConnection,
    pick: Uuid,
    at: Timestamp,
) -> Result<Released, ReleaseError> {
    if is_released(conn, pick)
        .await
        .map_err(|_| ReleaseError::Database)?
    {
        return Ok(Released::Already);
    }
    let key = format!("{HELD_RELEASE_KIND}.{pick}");
    queue(conn, pick, next_due(at), key).await?;
    Ok(Released::MovedOn)
}

/// The held-release task's run (OPS-001, OPS-024, FEE-017, FEE-021, PICK-022, SEC-030, D-3263):
/// see the module docs. `hooks` gives the Stripe client, the account and the releaser.
///
/// # Errors
/// A bad payload, or a database or Stripe failure (the task is tried again).
pub async fn run_held_release<S: PaymentsAdapter>(
    pool: &PgPool,
    hooks: &StripeWebhook<S, PickReleaser>,
    payload: &Value,
) -> Result<(), HandlerError> {
    let (pick, run_at) = parsed(payload).ok_or(HandlerError(BAD_PAYLOAD))?;
    let mut conn = pool
        .acquire()
        .await
        .map_err(|_| HandlerError(HELD_FAILED))?;
    if safe_mode_on(&mut conn)
        .await
        .map_err(|_| HandlerError(HELD_FAILED))?
    {
        let next = next_due(run_at);
        let key = format!("{HELD_RELEASE_KIND}.{pick}.{}", next.as_microsecond());
        return queue(&mut conn, pick, next, key)
            .await
            .map_err(|_| HandlerError(HELD_FAILED));
    }
    drop(conn);
    let at = hooks.sink.now();
    match held_release(pool, &hooks.stripe, &hooks.account, pick, at).await {
        Ok(cleared) => hooks
            .sink
            .release_now(cleared, at)
            .await
            .map(|_| ())
            .map_err(|_| HandlerError(HELD_FAILED)),
        Err(ReleaseError::Database | ReleaseError::Stripe) => Err(HandlerError(HELD_FAILED)),
        Err(_) => Ok(()),
    }
}

/// Registers the [`HELD_RELEASE_KIND`] handler, [`run_held_release`] over `hooks`, on every path
/// (OPS-024, JOB-006); with no Stripe settings (dev and CI, where no webhook route holds a pick)
/// a run is a handler error.
pub fn register_held_release(registry: &mut Registry, hooks: Option<StripeHooks>) {
    registry.register(HELD_RELEASE_KIND, move |pool, task| {
        let hooks = hooks.clone();
        async move {
            let hooks = hooks.ok_or(HandlerError(NO_STRIPE))?;
            run_held_release(&pool, &hooks, &task.payload).await
        }
    });
}

/// The due time [`HOLD_EVERY`] after `at`.
fn next_due(at: Timestamp) -> Timestamp {
    at.checked_add(HOLD_EVERY).unwrap_or(at)
}

/// The pick and due time of a held-release payload.
fn parsed(payload: &Value) -> Option<(Uuid, Timestamp)> {
    let pick = Uuid::parse_str(payload["pick"].as_str()?).ok()?;
    let run_at = payload["run_at"].as_str()?.parse().ok()?;
    Some((pick, run_at))
}

/// Queues one held-release run for `pick` due `run_at` under `key`; a used key queues nothing.
async fn queue(
    conn: &mut PgConnection,
    pick: Uuid,
    run_at: Timestamp,
    key: String,
) -> Result<(), ReleaseError> {
    let payload = json!({"pick": pick, "run_at": run_at.to_string()});
    let mut task = NewTask::new(HELD_RELEASE_KIND, payload, run_at);
    task.dedupe_key = Some(key);
    enqueue(conn, &task)
        .await
        .map(|_| ())
        .map_err(|_| ReleaseError::Database)
}
