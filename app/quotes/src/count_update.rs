//! The quote-count update trigger (TXT-017, D-4052, D-4064): each quote that becomes shown on a
//! job already Quotes ready queues one `quotes.count_update` `core.task`, in the same transaction
//! as the quote that became shown.
//!
//! [`crate::intake::submit_quote`] (a quote confirmed at once) and
//! [`crate::intake::confirm_quote_at`] (an AI-read quote confirmed on the read-back screen) call
//! [`queue_if_new`]. The first quote's text stays `jobs::api::first_quote`'s (TXT-016): a job that
//! was Open or No quotes yet under its row lock queues nothing, and so does Awaiting confirm (no
//! update follows a pick, TXT-042). A quote already shown before the write (a re-confirm, a form
//! edit) queues nothing. The payload is the job ID only (PRIV-002); one task per job and business
//! (the dedupe key). The handler that applies TXT-017's 3-hour gap and sends the text is T583's;
//! until then the kind is listed in `tests/task_kinds_deferred.json`.

use askhava_core::api::{NewTask, enqueue};
use askhava_jobs::api::JobStatus;
use jiff::Timestamp;
use serde_json::json;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{self, QuotesError};

/// The `core.task` kind of the quote-count update event (TXT-017); its handler is T583's.
pub const COUNT_UPDATE_KIND: &str = "quotes.count_update";

/// TXT-017, PICK-006: true when `business`'s quote on `job` is shown now (confirmed, not
/// withdrawn); read before a write to tell a newly shown quote from one shown already.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub(crate) async fn shown_now(
    db: &mut PgConnection,
    job: Uuid,
    business: Uuid,
) -> Result<bool, QuotesError> {
    Ok(models::shown_quotes(db, job).await?.contains(&business))
}

/// TXT-017, TXT-016, TXT-042, D-4052: queues one count-update event for `job`, due at `now`, when
/// `business`'s quote was not shown before this write (`was_shown`) and is shown after it, and
/// the job's status read under its row lock (`status`) is Quotes ready. Anything else queues
/// nothing. A second queue for the same job and business is refused by the dedupe key.
///
/// # Errors
///
/// [`QuotesError::Database`].
pub(crate) async fn queue_if_new(
    db: &mut PgConnection,
    (job, business): (Uuid, Uuid),
    (status, was_shown): (JobStatus, bool),
    now: Timestamp,
) -> Result<(), QuotesError> {
    if was_shown || !status.gets_count_updates() || !shown_now(db, job, business).await? {
        return Ok(());
    }
    let mut task = NewTask::new(COUNT_UPDATE_KIND, json!({ "job": job }), now);
    task.dedupe_key = Some(format!("{COUNT_UPDATE_KIND}.{job}.{business}"));
    enqueue(db, &task)
        .await
        .map_err(|_| QuotesError::Database)?;
    Ok(())
}
