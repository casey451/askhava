//! The every-window-ended check: the `core.task` that fires when a job's last card's 24-hour quote
//! window ends and hands the counts to `jobs::api::all_windows_ended` (PICK-001, TXT-042, JOB-025,
//! D-491).
//!
//! [`crate::intake::submit_quote`] schedules it for the last card's window end. When it runs, a
//! card sent later (CARD-018) may have moved that end on: it then schedules itself again for the
//! new end and does nothing more. Otherwise `jobs` decides: on a Quotes ready job it queues the one
//! all-quotes-in text, or closes the job when no choosable quote is left; any other status changes
//! nothing. The payload holds the job ID and the window end only (PRIV-002), and a second run
//! changes nothing more (JOB-006): `jobs` queues the text once per job and fences the close.

use askhava_core::api::{Claimed, Guarded, HandlerError, NewTask, enqueue};
use askhava_jobs::api::all_windows_ended;
use askhava_jobs::api::rules::WindowsEnd;
use jiff::Timestamp;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::intake::QUOTE_WINDOW;
use crate::models::{self, QuotesError, WindowState};

/// The `core.task` kind of the every-window-ended check, registered by `server`.
pub const WINDOWS_KIND: &str = "quotes.windows_end";
/// The handler's fixed failure sentences (no values, rust brain R5).
const BAD_PAYLOAD: &str = "quotes.windows_end payload is not a job and a time";
const DB: &str = "quotes.windows_end database call failed";

/// What judging a job's quote windows at a time did.
#[derive(Debug, PartialEq, Eq)]
pub enum WindowsCheck {
    /// The job has no card, so no window to end.
    NoCards,
    /// A card's window is still open until this time; the check is scheduled for it.
    Open(Timestamp),
    /// Every window has ended; what `jobs::api::all_windows_ended` did (TXT-042, JOB-025).
    Ended(Guarded<WindowsEnd>),
}

/// When the last of the job's card windows ends, or none when it has no card (PICK-001).
fn last_end(state: &WindowState) -> Option<Timestamp> {
    state
        .last_sent
        .and_then(|s| s.checked_add(QUOTE_WINDOW).ok())
}

/// Queues the check for the job's last window end; one task per job and end time (JOB-006).
pub(crate) async fn schedule(db: &mut PgConnection, job: Uuid) -> Result<(), QuotesError> {
    match last_end(&models::window_state(db, job).await?) {
        Some(end) => enqueue_at(db, job, end).await,
        None => Ok(()),
    }
}

/// Queues the check for `job` at `end`, its last window end.
async fn enqueue_at(db: &mut PgConnection, job: Uuid, end: Timestamp) -> Result<(), QuotesError> {
    let payload = json!({"job": job, "at": end.to_string()});
    let mut task = NewTask::new(WINDOWS_KIND, payload, end);
    task.dedupe_key = Some(format!("{WINDOWS_KIND}.{job}.{}", end.as_second()));
    enqueue(db, &task)
        .await
        .map_err(|_| QuotesError::Database)?;
    Ok(())
}

/// TXT-042, JOB-025, PICK-001, D-491: once every card's 24 hours have ended by `at`, passes the
/// job's counts (`quoted` and `sent` for the text, `choosable` for the close) to
/// `jobs::api::all_windows_ended`, stamped at the last window's end; while one is still open it
/// schedules itself for that end instead.
///
/// # Errors
///
/// [`QuotesError::Database`] or [`QuotesError::Job`].
pub async fn windows_ended(
    db: &mut PgConnection,
    job: Uuid,
    at: Timestamp,
) -> Result<WindowsCheck, QuotesError> {
    let state = models::window_state(db, job).await?;
    let Some(end) = last_end(&state) else {
        return Ok(WindowsCheck::NoCards);
    };
    if at < end {
        enqueue_at(db, job, end).await?;
        return Ok(WindowsCheck::Open(end));
    }
    let done = all_windows_ended(db, job, state.windows, end)
        .await
        .map_err(|_| QuotesError::Job)?;
    Ok(WindowsCheck::Ended(done))
}

/// The [`WINDOWS_KIND`] handler: runs [`windows_ended`] at the payload's time in one transaction.
///
/// # Errors
///
/// A fixed [`HandlerError`] sentence on a bad payload or a database failure.
pub async fn run_windows_end(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let p = &task.payload;
    let job = p["job"].as_str().and_then(|s| Uuid::parse_str(s).ok());
    let at = p["at"].as_str().and_then(|s| s.parse::<Timestamp>().ok());
    let (Some(job), Some(at)) = (job, at) else {
        return Err(HandlerError(BAD_PAYLOAD));
    };
    let mut tx = pool.begin().await.map_err(|_| HandlerError(DB))?;
    windows_ended(&mut tx, job, at)
        .await
        .map_err(|_| HandlerError(DB))?;
    tx.commit().await.map_err(|_| HandlerError(DB))
}
