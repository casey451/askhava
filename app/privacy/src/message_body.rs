//! `privacy.message_body`: the scrubbed words of an inbound text kept for a reason, today a day-3
//! check-in PROBLEM (JOB-016, TXT-020, PRIV-006; D-801, D-1840, T277).
//!
//! [`keep_problem_words`] stores one row per `messaging.inbox` row, bound to the job, in the
//! caller's transaction (the delivery's, so Twilio is answered only after the words are stored). It
//! takes only a [`ScrubbedText`], which only `scrub` makes, so raw words cannot be stored. The one
//! read is [`problem_words`], for Casey's [`OwnerSessionProof`] only (the weekly digest, T204, and
//! the owner portal); there is no business or homeowner read. Rows go 30 days after they are made
//! ([`CUT_WORDS_KIND`], the daily step, TXT-020) and with the job's PRIV-008 text stage
//! ([`run_words_purge`]).

use std::fmt;

use askhava_core::api::{Claimed, HandlerError, NewTask, TaskError, enqueue};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::api::OwnerSessionProof;
use crate::handlers::purge_target;
use crate::models::{PurgeStage, VaultError};
use crate::scrub::ScrubbedText;

/// The daily task that deletes kept words 30 days after they were made (TXT-020).
pub const CUT_WORDS_KIND: &str = "privacy.cut_problem_words";
/// How long kept words stay (TXT-020).
pub const WORDS_KEPT_FOR: SignedDuration = SignedDuration::from_hours(30 * 24);
const DAY: SignedDuration = SignedDuration::from_hours(24);
const FAILED: HandlerError = HandlerError("problem words database call failed");

/// A PROBLEM reply's kept words, as Casey's owner session reads them (JOB-016). `Debug` leaves the
/// words out (PRIV-016).
#[derive(Clone, PartialEq, Eq)]
pub struct ProblemWords {
    /// The job the reply answered.
    pub job: Uuid,
    /// The scrubbed words.
    pub words: String,
    /// True when the words hold a safety word (OPS-005, D-222).
    pub safety: bool,
}

impl fmt::Debug for ProblemWords {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProblemWords")
            .field("job", &self.job)
            .field("safety", &self.safety)
            .finish_non_exhaustive()
    }
}

/// JOB-016, PRIV-006, D-1840: keeps a PROBLEM reply's scrubbed `words` for inbox row `inbox` on
/// job `job`, with its safety flag, in the caller's transaction; a resend of the same inbox row
/// keeps the first row. `true` when a row was added.
///
/// # Errors
/// [`VaultError::Database`].
pub async fn keep_problem_words(
    db: &mut PgConnection,
    inbox: Uuid,
    job: Uuid,
    words: &ScrubbedText,
    safety: bool,
) -> Result<bool, VaultError> {
    let added = sqlx::query!(
        "INSERT INTO privacy.message_body (inbox_id, job_id, words, safety, created_at) \
         VALUES ($1, $2, $3, $4, now()) ON CONFLICT (inbox_id) DO NOTHING",
        inbox,
        job,
        words.as_str(),
        safety
    )
    .execute(db)
    .await
    .map_err(|_| VaultError::Database)?;
    Ok(added.rows_affected() == 1)
}

/// JOB-016, PRIV-006: the kept words of inbox row `inbox` (the subject of the PROBLEM alert), for
/// Casey's owner session only; `None` when none are kept (never kept, or already deleted).
///
/// # Errors
/// [`VaultError::Database`].
pub async fn problem_words(
    db: &mut PgConnection,
    _owner: &OwnerSessionProof,
    inbox: Uuid,
) -> Result<Option<ProblemWords>, VaultError> {
    let row = sqlx::query!(
        "SELECT job_id, words, safety FROM privacy.message_body WHERE inbox_id = $1",
        inbox
    )
    .fetch_optional(db)
    .await
    .map_err(|_| VaultError::Database)?;
    Ok(row.map(|r| ProblemWords {
        job: r.job_id,
        words: r.words,
        safety: r.safety,
    }))
}

/// TXT-020: deletes every kept word row made more than 30 days before `now`; the count deleted.
///
/// # Errors
/// [`VaultError::Database`].
pub async fn cut_problem_words(db: &mut PgConnection, now: Timestamp) -> Result<u64, VaultError> {
    let cutoff = now - WORDS_KEPT_FOR;
    sqlx::query!(
        "DELETE FROM privacy.message_body WHERE created_at < $1",
        cutoff.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|done| done.rows_affected())
    .map_err(|_| VaultError::Database)
}

/// Queues the next daily [`CUT_WORDS_KIND`] task, one per day (TXT-020).
///
/// # Errors
/// The [`TaskError`] of the insert.
pub async fn schedule_words_cut(db: &mut PgConnection, now: Timestamp) -> Result<(), TaskError> {
    let run_at = now + DAY;
    let mut task = NewTask::new(CUT_WORDS_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!("{CUT_WORDS_KIND}:{}", run_at.as_second() / 86_400));
    enqueue(db, &task).await.map(drop)
}

/// TXT-020: the [`CUT_WORDS_KIND`] handler at `now` (the worker's clock): queues the next day's
/// run first, so a run that fails for good still leaves it, then [`cut_problem_words`].
///
/// # Errors
/// A fixed [`HandlerError`] when a database call fails.
pub async fn run_words_cut_at(
    pool: PgPool,
    _task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let mut db = pool.acquire().await.map_err(|_| FAILED)?;
    schedule_words_cut(&mut db, now).await.map_err(|_| FAILED)?;
    cut_problem_words(&mut db, now)
        .await
        .map(drop)
        .map_err(|_| FAILED)
}

/// PRIV-008, D-1801: on the job's `text` purge stage, deletes the job's kept words; any other stage
/// does nothing. Safe to run twice.
///
/// # Errors
/// A fixed [`HandlerError`] on a bad payload or a failed delete.
pub async fn run_words_purge(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let (job, stage) = purge_target(&task)?;
    if stage != PurgeStage::Text {
        return Ok(());
    }
    let mut db = pool.acquire().await.map_err(|_| FAILED)?;
    sqlx::query!("DELETE FROM privacy.message_body WHERE job_id = $1", job)
        .execute(&mut *db)
        .await
        .map(drop)
        .map_err(|_| FAILED)
}
