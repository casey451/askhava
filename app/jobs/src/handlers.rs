//! The job events other modules act on: the homeowner-text event, the every-window-ended entry point
//! `quotes` calls, and a Connected job's check-in reply (JOB-010 to JOB-012, JOB-015, JOB-025,
//! TXT-042; `docs/architecture.md` section 4 rule 2, "events go up").
//!
//! `jobs` holds no homeowner contact, so a homeowner text is a `core.task` of kind [`TEXT_KIND`]
//! carrying only the job ID, the job's version, the template ID and the numbers the template needs
//! (the job number and, for TXT-042, the counts); it commits in the transaction of the move it
//! belongs to (R11). The handler that sends it is `intake`'s, which knows the homeowner: it fills the
//! quote-page link, applies quiet hours through `messaging::api::decide` (QH-001, QH-006) and calls
//! [`text_due`] first, so a text held overnight is dropped once the job moved on (TXT-042, D-872).
//! Each template is queued at most once per job (dedupe key `jobs.homeowner_text.<job>.<template>`).

use askhava_core::api::{Guarded, NewTask, enqueue};
use askhava_ops::api::Actor;
use jiff::Timestamp;
use serde_json::{Value, json};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{self, JobsError};
use crate::rules::{CheckIn, Contact, Reply, Windows, WindowsEnd, after_windows, connected_move};
use crate::service::{apply, job_number, move_to};
use crate::status::JobStatus;

/// The `core.task` kind of a homeowner text a job event asks for (D-871).
pub const TEXT_KIND: &str = "jobs.homeowner_text";
/// The TXT-042 template.
const ALL_QUOTES_IN: &str = "all_quotes_in";
/// The closing note when no choosable quote is left (JOB-025, D-045).
const CLOSED_NONE_LEFT: &str = "closed_no_quote_left";

/// Queues the homeowner text `template` about job `id` at `at`, tagged with the job's `version`
/// (D-871). `values` holds numbers only, never a name, phone or link (PRIV-002, SEC-037). A
/// template already queued for this job is not queued again (TXT-042 "sent once per job").
pub(crate) async fn queue_text(
    db: &mut PgConnection,
    id: Uuid,
    (template, version): (&str, i32),
    values: Value,
    at: Timestamp,
) -> Result<(), JobsError> {
    let payload = json!({"job": id, "version": version, "template": template, "values": values});
    let mut task = NewTask::new(TEXT_KIND, payload, at);
    task.dedupe_key = Some(format!("{TEXT_KIND}.{id}.{template}"));
    enqueue(db, &task).await.map_err(|_| JobsError::Database)?;
    Ok(())
}

/// True while job `id` is still at `version`, the version its text event carries: the sender calls
/// it before sending, so a held all-quotes-in text never goes after a pick or a close (TXT-042,
/// D-872). A closing note's version is a final status's, so it stays due (QH-007, D-461).
///
/// # Errors
///
/// [`JobsError::NotFound`] or [`JobsError::Database`].
pub async fn text_due(db: &mut PgConnection, id: Uuid, version: i32) -> Result<bool, JobsError> {
    Ok(models::load(db, id).await?.version == version)
}

/// What `quotes` calls once every card's 24-hour quote window has ended on job `id` (T046, D-491).
/// On a Quotes ready job with no choosable quote left the job closes with its closing note; with one
/// or more it queues the one all-quotes-in text (TXT-042) and the job keeps waiting for a pick under
/// its 3-day timer (JOB-012, JOB-025, D-140). Any other status, after a pick or a close, changes
/// nothing and sends nothing ([`Guarded::MovedOn`]). `picks` calls it again when a pick fails after
/// every window has ended, so the job then closes if that pick was the last choosable quote; the
/// all-quotes-in text is never queued twice.
///
/// # Errors
///
/// [`JobsError::NotFound`] or [`JobsError::Database`].
pub async fn all_windows_ended(
    db: &mut PgConnection,
    id: Uuid,
    windows: Windows,
    at: Timestamp,
) -> Result<Guarded<WindowsEnd>, JobsError> {
    let end = after_windows(windows);
    if end == WindowsEnd::Close {
        return close_none_left(db, id, at).await;
    }
    let job = models::load(db, id).await?;
    if !job.status.awaits_pick() {
        return Ok(Guarded::MovedOn);
    }
    let number = job_number(db, id).await?;
    let values = json!({"job": number, "quoted": windows.quoted, "count": windows.sent});
    queue_text(db, id, (ALL_QUOTES_IN, job.version), values, at).await?;
    Ok(Guarded::Applied(end))
}

/// Closes a job still awaiting a pick with the no-quote-left note (JOB-025, D-045, D-872). The
/// close is fenced on the version and timer version it loaded; if the row changed in between (a
/// restarted wait bumps the timer version) it loads once more and tries again while the job still
/// awaits a pick, so a moved fence alone never loses the close.
async fn close_none_left(
    db: &mut PgConnection,
    id: Uuid,
    at: Timestamp,
) -> Result<Guarded<WindowsEnd>, JobsError> {
    for _ in 0..2 {
        let job = models::load(db, id).await?;
        if !job.status.awaits_pick() {
            return Ok(Guarded::MovedOn);
        }
        let fence = Some((job.version, job.timer_version));
        let step = (job.status, fence, JobStatus::Closed);
        let moved = apply(db, id, step, (at, Actor::System), Some(CLOSED_NONE_LEFT)).await?;
        if moved.applied().is_some() {
            return Ok(Guarded::Applied(WindowsEnd::Close));
        }
    }
    Ok(Guarded::MovedOn)
}

/// Moves a Connected job on the homeowner's check-in `reply` (JOB-015, D-143): DONE to Done, NO or
/// PROBLEM to Closed, a day-3 NO under email contact back to Quotes ready (the no-show itself is
/// `picks`' record, FEE-035). A job no longer Connected changes nothing ([`Guarded::MovedOn`]).
///
/// # Errors
///
/// [`JobsError::Database`].
pub async fn connected_reply(
    db: &mut PgConnection,
    id: Uuid,
    (reply, check_in, contact): (Reply, CheckIn, Contact),
    (at, actor): (Timestamp, Actor),
) -> Result<Guarded<()>, JobsError> {
    let to = connected_move(reply, check_in, contact);
    move_to(db, id, (JobStatus::Connected, to), at, actor).await
}
