//! Saving, editing and withdrawing a business's quote before any pick (PICK-051, PICK-052,
//! REC-010; D-446, D-2912).
//!
//! [`save_quote`] (a first quote or an edit through `intake::submit_quote` and
//! `models::upsert_quote`, which sets `edited_at`) and [`withdraw_quote`] (a guarded
//! `UPDATE ... WHERE withdrawn_at IS NULL` setting `withdrawn_at`; the row is never deleted) each
//! run in one transaction that first takes the job's row lock through
//! `jobs::api::lock_for_quote_change`. A pick's status move updates that row, so neither commits
//! after the move: both re-read the status and `models::edit_facts` under the lock and refuse
//! once the job has left `JobStatus::takes_quote_edits` or any quote on it is unavailable. An
//! edit keeps the quote's first `received_at`, so its place in arrival order. Showing an edit or
//! a withdrawal to the homeowner is T384's.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::formats::{VisitSchedule, store_schedule};
use crate::intake::{Quote, Submitted, submit_quote};
use crate::models::{self, QuotesError};
use crate::notes::{NoteProblem, check_note};

/// What [`save_quote`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saved {
    /// `submit_quote`'s answer; the schedule was stored only with [`Submitted::Saved`].
    Stored(Submitted),
    /// The note goes back with this reason; nothing was stored (PICK-052).
    NoteBack(NoteProblem),
}

/// What [`withdraw_quote`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Withdrawal {
    /// `withdrawn_at` is set now.
    Withdrawn,
    /// It was already withdrawn; nothing changed.
    AlreadyWithdrawn,
    /// The business has no quote on the job.
    NoQuote,
    /// The job has had a pick, so the quote stays as it is (PICK-051).
    EditClosed,
}

/// Takes the job's row lock in `tx` and returns its status (D-2912).
async fn lock(
    tx: &mut PgConnection,
    job: Uuid,
) -> Result<askhava_jobs::api::JobStatus, QuotesError> {
    askhava_jobs::api::lock_for_quote_change(tx, job)
        .await
        .map_err(|_| QuotesError::Job)
}

/// PICK-051, PICK-052, REC-010, D-2912: stores `quote` from `business` on `job` at `now` with
/// its visit `schedule`, under the job's row lock: the note passes [`check_note`] first, then
/// `submit_quote` re-reads the window, the status and the edit facts and refuses an edit after a
/// pick ([`Submitted::EditClosed`]). One transaction, committed only when stored: the old
/// schedule is cleared before the upsert (migration 0157 keeps a schedule only beside a per-visit
/// period) and the new one set after it; nothing is kept on a refusal or error.
///
/// # Errors
///
/// [`QuotesError::Job`] or [`QuotesError::Database`].
pub async fn save_quote(
    db: &mut PgConnection,
    ids: (Uuid, Uuid),
    quote: &Quote,
    schedule: Option<VisitSchedule>,
    now: Timestamp,
) -> Result<Saved, QuotesError> {
    if let Err(problem) = check_note(&quote.form.notes) {
        return Ok(Saved::NoteBack(problem));
    }
    let mut tx = db.begin().await.map_err(|_| QuotesError::Database)?;
    lock(&mut tx, ids.0).await?;
    store_schedule(&mut tx, ids, None).await?;
    let done = submit_quote(&mut tx, ids, quote, now).await?;
    if done != Submitted::Saved {
        return Ok(Saved::Stored(done));
    }
    store_schedule(&mut tx, ids, schedule).await?;
    tx.commit().await.map_err(|_| QuotesError::Database)?;
    Ok(Saved::Stored(done))
}

/// PICK-051, D-2912: withdraws `business`'s quote on `job` at `now`, before any pick: under the
/// job's row lock, refused once the job has left the statuses that take edits or any quote on it
/// is unavailable. The row stays; a later [`save_quote`] is an edit that clears the withdrawal.
///
/// # Errors
///
/// [`QuotesError::Job`] or [`QuotesError::Database`].
pub async fn withdraw_quote(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    now: Timestamp,
) -> Result<Withdrawal, QuotesError> {
    let mut tx = db.begin().await.map_err(|_| QuotesError::Database)?;
    let status = lock(&mut tx, job).await?;
    let (own, picked) = models::edit_facts(&mut tx, job, business).await?;
    if !own {
        return Ok(Withdrawal::NoQuote);
    }
    if picked || !status.takes_quote_edits() {
        return Ok(Withdrawal::EditClosed);
    }
    let done = sqlx::query_scalar!(
        "UPDATE quotes.quote SET withdrawn_at = $3 \
         WHERE job_id = $1 AND business_id = $2 AND withdrawn_at IS NULL RETURNING true",
        job,
        business,
        now.to_sqlx()
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| QuotesError::Database)?;
    tx.commit().await.map_err(|_| QuotesError::Database)?;
    Ok(match done {
        Some(_) => Withdrawal::Withdrawn,
        None => Withdrawal::AlreadyWithdrawn,
    })
}
