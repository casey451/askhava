//! The weekly digest email to the owner inbox (OPS-030, OPS-033, OPS-041, BIZ-054; D-219,
//! D-3880, D-3882, D-4932, D-5319, D-5526; T524).
//!
//! [`DIGEST_KIND`] runs daily, first queued from the worker's `schedule_checks` under a dedupe key
//! for its UTC day ([`schedule_digest`]); each run queues the next before it works. When no digest
//! went out in the last 7 days it records one (`ops::api::digest_sent`) and queues the email of
//! `portals::api::weekly_digest` for the week before through `messaging::api::queue_account_email`
//! to `OWNER_INBOX`, keyed by the digest row, so a repeat stores nothing; the drain sends it to
//! `ASKHAVA_OWNER_INBOX_EMAIL`. A digest still unopened 4 days after it went out is re-sent once
//! (`digest_reminded`, OPS-033). The email holds counts, job numbers and the one owner-portal link
//! (`OWNER_DIGEST_ROUTE`), never a vault field or a PROBLEM reply's words (PRIV-006). It keeps the
//! shared business footer (D-5526). The restore actions are T648's (`recovery::api`, D-4932).

use std::io;
use std::sync::Arc;

use askhava_core::api::{
    Clock, HandlerError, NewTask, Registry, Settings, TaskError, brand, enqueue,
};
use askhava_messaging::api::{MailboxKind, OWNER_INBOX, OnDomain, queue_account_email};
use askhava_ops::api::{digest_reminded, digest_sent, latest_digest};
use askhava_portals::api::{DIGEST_WEEK, Digest, OWNER_DIGEST_ROUTE, weekly_digest};
use askhava_recovery::api::{RESTORE_FAILED_ACTION, RESTORE_PASSED_ACTION};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::json;
use sqlx::{Connection, PgConnection};

/// The daily digest check's task kind.
pub const DIGEST_KIND: &str = "ops.weekly_digest";
/// The weekly digest email's template ID.
pub const DIGEST_TEMPLATE: &str = "owner_weekly_digest";
/// The one re-send's template ID (OPS-033).
pub const REMINDER_TEMPLATE: &str = "owner_weekly_digest_reminder";
/// OPS-033: an unopened digest is re-sent once after this long.
pub const REMIND_AFTER: SignedDuration = SignedDuration::from_hours(4 * 24);
const DAY: i64 = 86_400;
const FAILED: HandlerError = HandlerError("the weekly digest could not be queued");

/// What one run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestRun {
    /// A new digest was queued.
    Sent,
    /// The unopened digest was queued again.
    Reminded,
    /// Nothing was due.
    Nothing,
}

/// The email key secret and the checked postal line the digest is queued with.
pub struct DigestMailer {
    /// The email idempotency-key secret (SEC-018).
    pub secret: SecretString,
    /// The footer's postal line and its mailbox kind (SEC-028).
    pub postal: (String, MailboxKind),
}

/// Registers [`DIGEST_KIND`] when the email provider is set; with none (dev and CI only) the
/// worker's seed is skipped too, since it is queued only when the drain can send.
///
/// # Errors
///
/// A bad email provider setting, named, never its value.
pub fn register(
    registry: &mut Registry,
    settings: &Settings,
    clock: &Arc<dyn Clock>,
    pairs: Vec<(String, String)>,
) -> io::Result<()> {
    let Some(p) = crate::email_provider::email_provider(settings, pairs)? else {
        return Ok(());
    };
    let (postal, kind) = p.postal();
    let mailer = Arc::new(DigestMailer {
        secret: p.secret().clone(),
        postal: (postal.to_owned(), kind),
    });
    let clock = Arc::clone(clock);
    registry.register(DIGEST_KIND, move |pool, _task| {
        let (mailer, now) = (Arc::clone(&mailer), clock.now());
        async move {
            let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
            run_digest(&mut conn, &mailer, now).await.map(|_| ())
        }
    });
    Ok(())
}

/// Queues the digest check for `now`'s UTC day, due at `now`, under `ops.weekly_digest.<day>`.
///
/// # Errors
///
/// [`TaskError`] when the queue insert fails.
pub async fn schedule_digest(db: &mut PgConnection, now: Timestamp) -> Result<(), TaskError> {
    let day = now.as_second().div_euclid(DAY);
    let mut task = NewTask::new(DIGEST_KIND, json!({}), now);
    task.dedupe_key = Some(format!("{DIGEST_KIND}.{day}"));
    enqueue(db, &task).await.map(|_| ())
}

/// OPS-030, OPS-033: queues tomorrow's check, then the week's digest when none went out in the
/// last 7 days, or the one re-send of a digest unopened for 4 days.
///
/// # Errors
///
/// A fixed [`HandlerError`] on a database or email-queue failure; the task is retried.
pub async fn run_digest(
    conn: &mut PgConnection,
    mailer: &DigestMailer,
    now: Timestamp,
) -> Result<DigestRun, HandlerError> {
    let next = Timestamp::from_second((now.as_second().div_euclid(DAY) + 1) * DAY);
    schedule_digest(conn, next.map_err(|_| FAILED)?)
        .await
        .map_err(|_| FAILED)?;
    let mut tx = conn.begin().await.map_err(|_| FAILED)?;
    let last = latest_digest(&mut tx).await.map_err(|_| FAILED)?;
    let run = match last {
        Some(row) if now < row.sent_at + DIGEST_WEEK => {
            let due = row.opened_at.is_none() && now >= row.sent_at + REMIND_AFTER;
            if !due
                || !digest_reminded(&mut tx, row.id, now)
                    .await
                    .map_err(|_| FAILED)?
            {
                return Ok(DigestRun::Nothing);
            }
            let window = (row.sent_at - DIGEST_WEEK, row.sent_at);
            queue(&mut tx, mailer, (REMINDER_TEMPLATE, row.id), window, now).await?;
            DigestRun::Reminded
        }
        _ => {
            let row = digest_sent(&mut tx, now).await.map_err(|_| FAILED)?;
            queue(
                &mut tx,
                mailer,
                (DIGEST_TEMPLATE, row),
                (now - DIGEST_WEEK, now),
                now,
            )
            .await?;
            DigestRun::Sent
        }
    };
    tx.commit().await.map_err(|_| FAILED)?;
    Ok(run)
}

/// Builds the digest of `window` and queues it to the owner inbox under `template` and `row`.
async fn queue(
    conn: &mut PgConnection,
    mailer: &DigestMailer,
    (template, row): (&str, sqlx::types::Uuid),
    window: (Timestamp, Timestamp),
    now: Timestamp,
) -> Result<(), HandlerError> {
    let actions = [RESTORE_PASSED_ACTION, RESTORE_FAILED_ACTION];
    let digest = weekly_digest(conn, window, actions)
        .await
        .map_err(|_| FAILED)?;
    let subject = format!("{} weekly digest", brand().short_name());
    let lines = digest_lines(&digest);
    let keys = (&mailer.secret, (mailer.postal.0.as_str(), mailer.postal.1));
    let ids = (template, OWNER_INBOX, &[row][..]);
    queue_account_email(conn, keys, ids, (&subject, &lines), now)
        .await
        .map_err(|_| FAILED)?;
    Ok(())
}

/// OPS-030: the email's paragraphs: the week, one line per item, and the owner-portal link.
#[must_use]
pub fn digest_lines(digest: &Digest) -> Vec<String> {
    let day = |t: Timestamp| askhava_core::api::arizona_time(t).date().to_string();
    let mut lines = vec![format!("Week {} to {}", day(digest.from), day(digest.to))];
    for l in &digest.lines {
        let count = l
            .count
            .map_or_else(|| "not counted".to_owned(), |n| n.to_string());
        let refs = if l.refs.is_empty() {
            String::new()
        } else {
            format!(" ({})", l.refs.join(", "))
        };
        lines.push(format!("{}: {count}{refs}", l.item));
    }
    lines.push(format!("Open it: {}", OnDomain(OWNER_DIGEST_ROUTE)));
    lines
}
