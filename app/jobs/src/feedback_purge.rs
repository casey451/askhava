//! Blanking the homeowner's feedback words (T315; PRIV-009, PRIV-011, JOB-017; D-2205).
//!
//! `jobs.feedback.body` is her own unscrubbed text, so it goes with the job text at the 3-year
//! purge and with "Delete my data". The row, its share tick, its removal and its author mark stay
//! as JOB-017's record of owner actions (D-1823); only the words become empty, with
//! `body_purged_at` set. Migration 0175's trigger lets that happen once and never undoes it, and
//! `api::feedback::shown` leaves a purged row out.
//!
//! On the text stage the composition asks `privacy` whether the job is held (an open dispute or
//! complaint) and passes the answer to [`feedback_text_stage`]: a held job keeps its words and
//! queues [`FEEDBACK_PURGE_KIND`] a day later, which asks again, until the hold is released
//! (D-3264). `jobs` never names `privacy` (D-1799).

use askhava_core::api::{NewTask, enqueue};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::JobsError;

/// The audit action of a blanked feedback (IDs only; the words are never logged, PRIV-028).
pub const FEEDBACK_PURGED: &str = "feedback_purged";

/// PRIV-009, PRIV-011, JOB-017: blanks job `job`'s feedback words and sets `body_purged_at`, with
/// one `ops` audit row by `actor` naming only the feedback's ID, in one transaction (a savepoint
/// inside the caller's). `true` when it blanked a body; `false` when the job has no feedback or
/// it was already blanked, so a second run writes nothing.
///
/// # Errors
/// [`JobsError::Database`].
pub async fn purge_feedback(
    db: &mut PgConnection,
    job: Uuid,
    actor: Actor,
) -> Result<bool, JobsError> {
    let mut tx = db.begin().await.map_err(|_| JobsError::Database)?;
    let blanked = sqlx::query_scalar!(
        "UPDATE jobs.feedback SET body = '', body_purged_at = now() \
         WHERE job_id = $1 AND body_purged_at IS NULL RETURNING id",
        job,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| JobsError::Database)?;
    let Some(id) = blanked else {
        return Ok(false);
    };
    let entry = AuditEntry {
        actor,
        action: FEEDBACK_PURGED,
        subject_kind: "jobs.feedback",
        subject_id: id,
        change: Change::Action,
    };
    audit(&mut tx, &entry)
        .await
        .map_err(|_| JobsError::Database)?;
    tx.commit().await.map_err(|_| JobsError::Database)?;
    Ok(true)
}

/// The task that asks again whether a held job's feedback words may go: payload `{"job"}`; the
/// composition registers it with the same hold check as the text stage (D-3264).
pub const FEEDBACK_PURGE_KIND: &str = "jobs.feedback_purge";

/// How long a held job's words wait before the hold is asked about again.
const HELD_WAIT: SignedDuration = SignedDuration::from_hours(24);

/// What [`feedback_text_stage`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackStage {
    /// The words were blanked now.
    Blanked,
    /// Nothing to blank: no feedback, or already blanked.
    Nothing,
    /// The job is held; the words stay and [`FEEDBACK_PURGE_KIND`] asks again in a day.
    Held,
}

/// PRIV-009, PRIV-007, JOB-017: the text stage's feedback step for job `job`, given `held` from
/// `privacy::api::is_held` read by the caller: unheld, [`purge_feedback`] as the system; held, the
/// words stay and one [`FEEDBACK_PURGE_KIND`] task is queued for `now` plus a day (deduped by
/// job and day, so a rerun queues nothing more). Safe to run twice (JOB-006).
///
/// # Errors
/// [`JobsError::Database`].
pub async fn feedback_text_stage(
    db: &mut PgConnection,
    (job, held): (Uuid, bool),
    now: Timestamp,
) -> Result<FeedbackStage, JobsError> {
    if !held {
        let blanked = purge_feedback(db, job, Actor::System).await?;
        return Ok(if blanked {
            FeedbackStage::Blanked
        } else {
            FeedbackStage::Nothing
        });
    }
    let at = now + HELD_WAIT;
    let mut again = NewTask::new(FEEDBACK_PURGE_KIND, json!({ "job": job }), at);
    let day = at.as_second().div_euclid(86_400);
    again.dedupe_key = Some(format!("{FEEDBACK_PURGE_KIND}:{job}:{day}"));
    enqueue(db, &again).await.map_err(|_| JobsError::Database)?;
    Ok(FeedbackStage::Held)
}
