//! The day-31 message body delete at Twilio (PRIV-027, TXT-020; D-1803, D-1841).
//!
//! Twilio keeps every text's body until it is redacted through its API. The daily [`SCAN_KIND`]
//! task finds every inbound (`messaging.inbox`) and outbound (`messaging.outbox`) message whose
//! provider SID is [`BODY_DELETE_AFTER`] (31 days) or more old and queues one [`DELETE_KIND`] task
//! per SID under the dedupe key `messaging.body_delete:<sid>`. That `core.task` row is the SID's
//! record: a later scan queues nothing for it again, so each body is deleted once, and a provider
//! error fails the task, which the queue retries. Nothing is queued before day 31. The local
//! 30-day cut of bodies stays `retention::cut_bodies` (T093, TXT-020); no row here is changed.

use std::sync::Arc;

use askhava_adapters::api::{AdapterError, IdempotencyKey, SmsBodyDelete};
use askhava_core::api::{Claimed, Clock, HandlerError, NewTask, Registry, TaskError, enqueue};
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::{PgConnection, PgPool};

use crate::models::OutboxError;

/// How old a message's provider SID must be before its body is deleted at Twilio (PRIV-027).
pub const BODY_DELETE_AFTER: SignedDuration = SignedDuration::from_hours(31 * 24);

/// The daily `core.task` kind that queues the day-31 body deletes (PRIV-027).
pub const SCAN_KIND: &str = "messaging.body_delete_scan";

/// The `core.task` kind that deletes one message's body at Twilio, payload `{"sid"}` (PRIV-027).
pub const DELETE_KIND: &str = "messaging.body_delete";

const DAY: SignedDuration = SignedDuration::from_hours(24);
const DELETE_ATTEMPTS: i32 = 10;
const DB_FAILED: HandlerError = HandlerError("message body delete database call failed");
const VENDOR_FAILED: HandlerError = HandlerError("message body delete at the provider failed");
const BAD_SID: HandlerError = HandlerError("body delete task payload has no message SID");
const ALERT_FAILED: HandlerError =
    HandlerError("message body delete failed and its alert was not stored");
/// The High alert a body delete's last failed attempt raises (PRIV-027, D-2162).
pub const DELETE_FAILED_ALERT: &str = "messaging.body_delete_failed";

/// PRIV-027, TXT-020: queues one [`DELETE_KIND`] task for every inbound and outbound SID at least
/// [`BODY_DELETE_AFTER`] old at `now` that has none yet; returns how many it newly queued. Safe to
/// run twice: each SID's dedupe key makes a second queue a no-op.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn queue_body_deletes(db: &mut PgConnection, now: Timestamp) -> Result<u64, OutboxError> {
    let cutoff = now - BODY_DELETE_AFTER;
    let sids = sqlx::query_scalar!(
        r#"SELECT message_sid AS "sid!" FROM messaging.inbox WHERE received_at <= $1
           UNION SELECT provider_sid AS "sid!" FROM messaging.outbox
           WHERE provider_sid IS NOT NULL AND COALESCE(sent_at, created_at) <= $1"#,
        cutoff.to_sqlx() as _
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| OutboxError::Database)?;
    let mut queued = 0;
    for sid in sids {
        let mut task = NewTask::new(DELETE_KIND, json!({ "sid": sid }), now);
        task.max_attempts = DELETE_ATTEMPTS;
        task.dedupe_key = Some(format!("{DELETE_KIND}:{sid}"));
        if enqueue(db, &task)
            .await
            .map_err(|_| OutboxError::Database)?
            .is_some()
        {
            queued += 1;
        }
    }
    Ok(queued)
}

/// PRIV-027, SEC-018: the [`DELETE_KIND`] handler. Redacts the body of the task's SID at the
/// provider under a key derived from `secret` and the SID; a message the provider no longer has
/// (404, code 20404) holds no body, so it counts as done. Any other error fails the task, which
/// the queue retries.
///
/// # Errors
///
/// A fixed [`HandlerError`] for a bad payload or a provider failure.
pub async fn run_body_delete<S: SmsBodyDelete + Sync>(
    sms: &S,
    secret: &SecretString,
    task: Claimed,
) -> Result<(), HandlerError> {
    let sid = task.payload["sid"].as_str().ok_or(BAD_SID)?;
    let key = IdempotencyKey::derive(secret, "sms.body_delete", &[sid]);
    match sms.delete_message_body(&key, sid).await {
        Ok(())
        | Err(AdapterError::Vendor {
            status: 404,
            code: Some(20404),
        }) => Ok(()),
        Err(_) => Err(VENDOR_FAILED),
    }
}

/// Queues the next daily [`SCAN_KIND`] task, one per day (PRIV-027).
///
/// # Errors
///
/// The [`TaskError`] of the insert.
pub async fn schedule_body_scan(db: &mut PgConnection, now: Timestamp) -> Result<(), TaskError> {
    let run_at = now + DAY;
    let mut task = NewTask::new(SCAN_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!("{SCAN_KIND}:{}", run_at.as_second() / 86_400));
    enqueue(db, &task).await.map(|_| ())
}

/// PRIV-027, D-1841: registers [`SCAN_KIND`] (at `clock`'s now, then the next day's scan) and
/// [`DELETE_KIND`] over the texting client `sms`, deriving each delete's key from `secret`. A
/// delete's last failed attempt raises a High alert (D-2162).
pub fn register_body_deletes<S>(
    registry: &mut Registry,
    sms: Arc<S>,
    secret: SecretString,
    clock: Arc<dyn Clock>,
) where
    S: SmsBodyDelete + Send + Sync + 'static,
{
    registry.register(DELETE_KIND, move |pool, task| {
        let (sms, secret) = (Arc::clone(&sms), secret.clone());
        async move {
            let outcome = run_body_delete(&*sms, &secret, task.clone()).await;
            alert_if_last(&pool, &task, outcome).await
        }
    });
    registry.register(SCAN_KIND, move |pool, _task| {
        let now = clock.now();
        async move {
            let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
            queue_body_deletes(&mut conn, now)
                .await
                .map_err(|_| DB_FAILED)?;
            schedule_body_scan(&mut conn, now)
                .await
                .map_err(|_| DB_FAILED)
        }
    });
}

/// PRIV-027, OPS-001, D-2162: a delete task that failed on its last attempt is dead-lettered and,
/// its dedupe key kept, never queued again, so it raises one High [`DELETE_FAILED_ALERT`] (no
/// subject: the payload names no job); `outcome` is returned unchanged.
///
/// # Errors
/// `outcome`'s own error, or `ALERT_FAILED` when the alert could not be stored.
async fn alert_if_last(
    pool: &PgPool,
    task: &Claimed,
    outcome: Result<(), HandlerError>,
) -> Result<(), HandlerError> {
    if outcome.is_ok() || task.attempts < DELETE_ATTEMPTS {
        return outcome;
    }
    let alert = NewAlert {
        kind: DELETE_FAILED_ALERT,
        severity: Severity::High,
        subject: None,
    };
    let mut conn = pool.acquire().await.map_err(|_| ALERT_FAILED)?;
    raise_alert(&mut conn, &alert)
        .await
        .map_err(|_| ALERT_FAILED)?;
    outcome
}
