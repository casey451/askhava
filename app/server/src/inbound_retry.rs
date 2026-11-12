//! The bounded retry of an inbound text a module error stopped (TXT-012, TXT-020, CON-008,
//! CON-018, QH-001; D-801, D-2307, D-3135 to D-3141; T336).
//!
//! The route (`inbound_text`) answers such a text 200, raises T325's alert and queues one
//! `messaging.inbound_retry` task (`inbound_text::RETRY_KIND`) whose payload is the inbox row ID
//! and the webhook's `OptOutType` code (D-3140). [`run_retry`] reads the row's MessageSid, reads
//! the body back from Twilio's copy (`SmsBodyRead::fetch_message_body`, outside any transaction)
//! and runs `inbound_text::rerun`, the route's steps after the inbox insert, in one transaction.
//! The body is held only in memory, as a `SecretString`, and is never stored or logged (D-801).
//! A retry that ran every step writes one [`HANDLED`] audit row about its inbox row.
//!
//! Rules: (a) opt-out and HELP always run (CON-008, CON-018); (b) when a newer inbox row from the
//! same sender was processed (no open T325 alert about it, or a [`HANDLED`] mark) the retry runs
//! only (a) and closes; only processed newer rows are checked, by their `From` alone
//! (`fetch_message_from`, the body never parsed); more than [`NEWER_LIMIT`] newer rows count as
//! processed, and a failed read of one runs (a), commits and fails the claim so `core` tries the
//! rest again (D-3141); (c) outside 8 a.m. to 8 p.m. in both Arizona and her area-code time
//! (QH-001) it runs only (a) now (an emergency text runs every step, its INT-004 911 text being an
//! any-hour text, QH-004) and queues the rest once for the next open instant, so nothing but the
//! any-hour texts goes out at night. A message Twilio no longer has (redacted on day 31 or by
//! `Delete my data`, or unknown; a photo-only MMS also reads as no words, D-3141) closes the task
//! with no error. A module error queues the next try (30 minutes, then 2 hours); the third adds
//! nothing, and T325's alert stays open. A listed sender's error is not retried. A database error
//! or a failed read of its own message fails the claim, which `core` retries.

use std::sync::Arc;

use askhava_adapters::api::{MessageBody, SmsBodyRead};
use askhava_core::api::{Claimed, Clock, HandlerError, Registry};
use askhava_intake::api::rules::Settings as IntakeSettings;
use askhava_messaging::api::{
    AreaCodes, ProviderKeyword, SendWindow, inbox_after, inbox_message, window,
};
use askhava_ops::api::{Actor, AuditEntry, Change, alert_open, audit_once, audited};
use askhava_privacy::api::Vault;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::inbound_text::{
    Ctx, FAILED_ALERT, Fields, RETRY_KIND, Reach, Rerun, queue_held, queue_next_try, rerun,
};

/// How many newer inbox rows rule (b) checks, newest first; more count as processed (D-3141).
const NEWER_LIMIT: usize = 50;
/// [`NEWER_LIMIT`] plus one, the rows read to see whether there are more.
const NEWER_READ: i64 = 51;
/// The audit action a retry that ran every step writes once about its inbox row, so rule (b)
/// counts the row as processed although T325's alert stays open (D-3140).
pub const HANDLED: &str = "messaging.inbound_retry_done";
const INBOX: &str = "messaging.inbox";
const DB_FAILED: HandlerError = HandlerError("inbound retry database call failed");
const VENDOR_FAILED: HandlerError = HandlerError("inbound retry message read failed");
const BAD_PAYLOAD: HandlerError = HandlerError("inbound retry task payload has no inbox row ID");

/// What rule (b) found among the newer inbox rows (D-2307, D-3141).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Newer {
    /// No processed newer row is hers.
    None,
    /// A processed newer row is hers, or there were too many to check.
    Processed,
    /// A newer row's `From` could not be read: run (a) only, then fail the claim.
    Unread,
}

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
    let (inbox, provider) = payload(task)?;
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
    let found = (inbox, received_at, message.from.as_str());
    let newer = newer_processed(&mut conn, reader, found).await?;
    let held = night(&message.from, now);
    let fields = Fields {
        from: Some(&message.from),
        to: Some(&message.to),
        body: message.body.expose_secret(),
        provider,
    };
    let reach = match (newer, held) {
        (Newer::Processed | Newer::Unread, _) => Reach::Screen,
        (Newer::None, Some(_)) => Reach::Night,
        (Newer::None, None) => Reach::All,
    };
    let ctx = Ctx {
        id_secret: &keys.id_secret,
        vault: &keys.vault,
        intake: keys.intake,
    };
    let mut tx = conn.begin().await.map_err(|_| DB_FAILED)?;
    let outcome = rerun(&ctx, &mut tx, &fields, (inbox, now, reach)).await;
    let run = Run {
        inbox,
        provider,
        reach,
        held,
        now,
    };
    settle(&mut tx, outcome, &run).await?;
    tx.commit().await.map_err(|_| DB_FAILED)?;
    if newer == Newer::Unread {
        return Err(VENDOR_FAILED);
    }
    Ok(())
}

/// The task's inbox row ID and the webhook's `OptOutType` it carries (CON-030, D-3140).
fn payload(task: &Claimed) -> Result<(Uuid, ProviderKeyword), HandlerError> {
    let inbox = task.payload["inbox"]
        .as_str()
        .and_then(|id| id.parse::<Uuid>().ok())
        .ok_or(BAD_PAYLOAD)?;
    let provider = ProviderKeyword::from_opt_out_type(task.payload["opt_out_type"].as_str());
    Ok((inbox, provider))
}

/// One try's facts for [`settle`].
struct Run {
    inbox: Uuid,
    provider: ProviderKeyword,
    reach: Reach,
    held: Option<Timestamp>,
    now: Timestamp,
}

/// What follows a run (D-2307, D-3140): a database error fails the claim (the transaction rolls
/// back); a module error is logged with the step and variant name only and queues the next try
/// unless the sender is listed; a run its night reach stopped queues the rest once for `held`; a
/// run that went as far as it could, not stale, writes the [`HANDLED`] mark once.
async fn settle(db: &mut PgConnection, outcome: Rerun, run: &Run) -> Result<(), HandlerError> {
    match outcome {
        Rerun::Database => Err(DB_FAILED),
        Rerun::Failed(step, error, retry) => {
            tracing::warn!(step, error, "inbound retry step failed");
            if retry {
                queue_next_try(db, (run.inbox, run.provider), run.now)
                    .await
                    .map_err(|_| DB_FAILED)?;
            }
            Ok(())
        }
        Rerun::Stopped => match (run.reach, run.held) {
            (Reach::Night, Some(until)) => queue_held(db, (run.inbox, run.provider), until)
                .await
                .map(|_| ())
                .map_err(|_| DB_FAILED),
            _ => Ok(()),
        },
        Rerun::Done if run.reach == Reach::Screen => Ok(()),
        Rerun::Done => mark_handled(db, run.inbox).await,
    }
}

/// D-3140: the one [`HANDLED`] audit row about inbox row `inbox` (record IDs and code names
/// only, PRIV-028).
async fn mark_handled(db: &mut PgConnection, inbox: Uuid) -> Result<(), HandlerError> {
    let entry = AuditEntry {
        actor: Actor::System,
        action: HANDLED,
        subject_kind: INBOX,
        subject_id: inbox,
        change: Change::Action,
    };
    audit_once(db, &entry)
        .await
        .map(|_| ())
        .map_err(|_| DB_FAILED)
}

/// Rule (b), D-2307, D-3141: whether an inbox row newer than this one (`inbox`, received at
/// `after`) and from the same number `from` was processed: no open T325 alert about it, or a
/// [`HANDLED`] mark. Only processed rows are read from Twilio, and only for their `From`; past
/// [`NEWER_LIMIT`] rows the answer is `Processed` (stale content never overrides), and a failed
/// read is `Unread` unless a later row is hers.
async fn newer_processed<R: SmsBodyRead + Sync>(
    db: &mut PgConnection,
    reader: &R,
    (inbox, after, from): (Uuid, Timestamp, &str),
) -> Result<Newer, HandlerError> {
    let rows = inbox_after(db, after, NEWER_READ)
        .await
        .map_err(|_| DB_FAILED)?;
    let rows: Vec<_> = rows.into_iter().filter(|(id, _)| *id != inbox).collect();
    if rows.len() > NEWER_LIMIT {
        return Ok(Newer::Processed);
    }
    let mut found = Newer::None;
    for (id, sid) in rows {
        if !processed(db, id).await? {
            continue;
        }
        match reader.fetch_message_from(&sid).await {
            Ok(Some(newer)) if newer == from => return Ok(Newer::Processed),
            Ok(_) => {}
            Err(_) => found = Newer::Unread,
        }
    }
    Ok(found)
}

/// Whether inbox row `id` was processed: its T325 alert is not open, or a retry marked it
/// [`HANDLED`] (D-3140).
async fn processed(db: &mut PgConnection, id: Uuid) -> Result<bool, HandlerError> {
    let failed = alert_open(db, FAILED_ALERT, (INBOX, id))
        .await
        .map_err(|_| DB_FAILED)?;
    if !failed {
        return Ok(true);
    }
    audited(db, HANDLED, (INBOX, id))
        .await
        .map_err(|_| DB_FAILED)
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
