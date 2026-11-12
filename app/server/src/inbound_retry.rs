//! The bounded retry of an inbound text a module error stopped (TXT-012, TXT-020, CON-008,
//! CON-018, QH-001; D-801, D-2307, D-3135 to D-3139; T336).
//!
//! The route (`inbound_text`) answers such a text 200, raises T325's alert and queues one
//! `messaging.inbound_retry` task (`inbound_text::RETRY_KIND`) whose payload is the inbox row ID
//! only. [`run_retry`] reads the row's MessageSid, reads the body back from Twilio's copy
//! (`SmsBodyRead::fetch_message_body`, outside any transaction) and runs `inbound_text::rerun`,
//! the route's steps after the inbox insert, in one transaction. The body is held only in memory,
//! as a `SecretString`, and is never stored or logged (D-801).
//!
//! Rules: (a) opt-out and HELP always run (CON-008, CON-018); (b) when a newer inbox row from the
//! same sender (its `From` read from Twilio) was already processed (no open T325 alert about it),
//! the retry runs only (a) and closes; (c) outside 8 a.m. to 8 p.m. in both Arizona and her
//! area-code time (QH-001) it runs only (a) now (an emergency text runs every step, its INT-004
//! 911 text being an any-hour text, QH-004) and queues the rest once for the next open instant,
//! so nothing but the any-hour texts goes out at night. A message Twilio no longer has
//! (redacted on day 31 or by `Delete my data`, or unknown) closes the task with no error. A module
//! error queues the next try (30 minutes, then 2 hours); the third adds nothing, and T325's alert
//! stays open. A listed sender's error is not retried. A database or Twilio error fails the claim,
//! which `core` retries.

use std::sync::Arc;

use askhava_adapters::api::{MessageBody, SmsBodyRead};
use askhava_core::api::{Claimed, Clock, HandlerError, Registry};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_messaging::api::{
    AreaCodes, PROVIDER_OPT_OUT_WORDS, ProviderKeyword, RESUBSCRIBE_WORDS, SendWindow, inbox_after,
    inbox_message, window,
};
use askhava_ops::api::alert_open;
use askhava_privacy::api::Vault;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::inbound_text::{
    Ctx, FAILED_ALERT, Fields, RETRY_KIND, Reach, Rerun, queue_held, queue_next_try, rerun,
};

/// How many newer inbox rows rule (b) looks at, newest first (D-2307).
const NEWER_LIMIT: i64 = 50;
const DB_FAILED: HandlerError = HandlerError("inbound retry database call failed");
const VENDOR_FAILED: HandlerError = HandlerError("inbound retry message read failed");
const BAD_PAYLOAD: HandlerError = HandlerError("inbound retry task payload has no inbox row ID");

/// What the retry needs besides the pool: the keys and settings the route's steps use and the
/// clock. No `Debug`: it holds secrets.
pub struct RetryKeys {
    /// The `ID_SECRET` for outbox keys (SEC-018).
    pub id_secret: SecretString,
    /// The vault (PRIV-002).
    pub vault: Vault,
    /// The owner settings `intake` reads.
    pub intake: IntakeSettings,
    /// The worker's clock (D-863).
    pub clock: Arc<dyn Clock>,
}

/// D-2307: registers `messaging.inbound_retry` over the Twilio reader `reader` and `keys`
/// (`worker_registry`, under the Twilio settings).
pub fn register<R>(registry: &mut Registry, reader: Arc<R>, keys: Arc<RetryKeys>)
where
    R: SmsBodyRead + Send + Sync + 'static,
{
    registry.register(RETRY_KIND, move |pool, task| {
        let (reader, keys) = (Arc::clone(&reader), Arc::clone(&keys));
        async move { run_retry(&pool, &*reader, &keys, &task).await }
    });
}

/// TXT-012, TXT-020, CON-008, CON-018, QH-001, D-801, D-2307: one try of the retry of the task's
/// inbox row (see the module comment). Safe to run twice: every step it runs is keyed per inbox
/// row or MessageSid, and the next try's and the held run's dedupe keys queue each once.
///
/// # Errors
///
/// A fixed [`HandlerError`] for a bad payload, a database error or a failed Twilio read.
pub async fn run_retry<R: SmsBodyRead + Sync>(
    pool: &PgPool,
    reader: &R,
    keys: &RetryKeys,
    task: &Claimed,
) -> Result<(), HandlerError> {
    let inbox = task.payload["inbox"]
        .as_str()
        .and_then(|id| id.parse::<Uuid>().ok())
        .ok_or(BAD_PAYLOAD)?;
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    let found = inbox_message(&mut conn, inbox)
        .await
        .map_err(|_| DB_FAILED)?;
    let Some((sid, received_at)) = found else {
        return Ok(());
    };
    let read = reader.fetch_message_body(&sid).await;
    let MessageBody::Body(message) = read.map_err(|_| VENDOR_FAILED)? else {
        return Ok(());
    };
    let now = keys.clock.now();
    let newer = (inbox, received_at, message.from.as_str());
    let stale = newer_processed(&mut conn, reader, newer).await?;
    let held = night(&message.from, now);
    let body = message.body.expose_secret();
    let fields = Fields {
        from: Some(&message.from),
        to: Some(&message.to),
        body,
        provider: provider_keyword(body),
    };
    let reach = match (stale, held) {
        (true, _) => Reach::Screen,
        (false, Some(_)) => Reach::Night,
        (false, None) => Reach::All,
    };
    let ctx = Ctx {
        id_secret: &keys.id_secret,
        vault: &keys.vault,
        intake: keys.intake,
    };
    let mut tx = conn.begin().await.map_err(|_| DB_FAILED)?;
    let outcome = rerun(&ctx, &mut tx, &fields, (inbox, now, reach)).await;
    settle(&mut tx, inbox, outcome, (stale, held, now)).await?;
    tx.commit().await.map_err(|_| DB_FAILED)
}

/// What follows a run (D-2307): a database error fails the claim (the transaction rolls back); a
/// module error is logged with the step and variant name only and queues the next try unless the
/// sender is listed; a run its reach stopped at night, and not stale, queues the rest once for
/// `held`.
async fn settle(
    db: &mut PgConnection,
    inbox: Uuid,
    outcome: Rerun,
    (stale, held, now): (bool, Option<Timestamp>, Timestamp),
) -> Result<(), HandlerError> {
    match outcome {
        Rerun::Database => Err(DB_FAILED),
        Rerun::Failed(step, error, retry) => {
            tracing::warn!(step, error, "inbound retry step failed");
            if retry {
                queue_next_try(db, inbox, now)
                    .await
                    .map_err(|_| DB_FAILED)?;
            }
            Ok(())
        }
        Rerun::Stopped => match held {
            Some(until) if !stale => queue_held(db, inbox, until)
                .await
                .map(|_| ())
                .map_err(|_| DB_FAILED),
            _ => Ok(()),
        },
        Rerun::Done => Ok(()),
    }
}

/// Rule (b), D-2307: whether an inbox row newer than this one (`inbox`, received at `after`) and
/// from the same number `from` was processed, that is has no open T325 alert. Each newer row's
/// `From` is read from Twilio; its body is dropped unread. A message Twilio no longer has counts
/// as not hers.
async fn newer_processed<R: SmsBodyRead + Sync>(
    db: &mut PgConnection,
    reader: &R,
    (inbox, after, from): (Uuid, Timestamp, &str),
) -> Result<bool, HandlerError> {
    let rows = inbox_after(db, after, NEWER_LIMIT)
        .await
        .map_err(|_| DB_FAILED)?;
    for (id, sid) in rows.into_iter().filter(|(id, _)| *id != inbox) {
        let failed = alert_open(db, FAILED_ALERT, ("messaging.inbox", id))
            .await
            .map_err(|_| DB_FAILED)?;
        if failed {
            continue;
        }
        let read = reader.fetch_message_body(&sid).await;
        if let MessageBody::Body(newer) = read.map_err(|_| VENDOR_FAILED)?
            && newer.from == from
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Rule (c), QH-001, QH-006: the next instant both Arizona and `from`'s area-code time are open,
/// when `now` is outside them; `None` inside them or for a number the table cannot place (the
/// steps' own sends refuse it).
fn night(from: &str, now: Timestamp) -> Option<Timestamp> {
    let table = AreaCodes::embedded().ok()?;
    match window(&table, from, now) {
        Ok(SendWindow::Hold { until }) => Some(until),
        _ => None,
    }
}

/// CON-030, D-866: what Twilio's Advanced Opt-Out matched on this body, as its webhook's
/// `OptOutType` said: the body alone, trimmed and in any case, one of the provider's opt-out
/// words (it blocked the number and sent its reply) or START words; the retry has no webhook.
fn provider_keyword(body: &str) -> ProviderKeyword {
    let word = body.trim().to_lowercase();
    if PROVIDER_OPT_OUT_WORDS.contains(&word.as_str()) {
        ProviderKeyword::OptOut
    } else if RESUBSCRIBE_WORDS.contains(&word.as_str()) {
        ProviderKeyword::Start
    } else {
        ProviderKeyword::None
    }
}
