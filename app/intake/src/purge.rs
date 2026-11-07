//! `intake`'s part of the retention purge: on `privacy`'s [`PURGE_KIND`] event it clears the job's
//! homeowner contact (contact stage, 180 days after close) and deletes the job's intake answers
//! (text stage, 3 years after close) (PRIV-007, PRIV-008, D-013, D-491).
//!
//! `intake` knows which homeowner a job belongs to (`intake.conversation.job_id`), so it is the one
//! that calls `privacy::api::purge_contact`, which keeps the phone with its consent records
//! (D-079). A homeowner with another request open, or closed less than 180 days ago, keeps the
//! contact: that request's own purge clears it later. Consent records and terms acceptances are
//! never deleted here (CON-022, PRIV-010, D-492).
//!
//! [`PURGE_KIND`]: askhava_privacy::api::PURGE_KIND

use askhava_core::api::{Claimed, HandlerError};
use askhava_privacy::api::{PurgeStage, purge_contact, purge_target};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const DB_FAILED: HandlerError = HandlerError("intake purge database call failed");
const CONTACT_DAYS: SignedDuration = SignedDuration::from_hours(180 * 24);

/// PRIV-007: the vault contact of `job`'s homeowner when no other request of theirs is open or
/// closed after `now` less 180 days.
///
/// # Errors
/// [`DB_FAILED`] on a database failure.
pub async fn contact_to_purge(
    db: &mut PgConnection,
    job: Uuid,
    now: Timestamp,
) -> Result<Option<Uuid>, HandlerError> {
    sqlx::query_scalar!(
        "SELECT DISTINCT h.contact_id FROM intake.conversation c \
         JOIN intake.homeowner h ON h.id = c.homeowner_id WHERE c.job_id = $1 \
         AND NOT EXISTS (SELECT 1 FROM intake.conversation o WHERE o.homeowner_id = h.id \
         AND o.job_id IS DISTINCT FROM $1 AND (o.closed_at IS NULL OR o.closed_at > $2))",
        job,
        (now - CONTACT_DAYS).to_sqlx() as _,
    )
    .fetch_optional(db)
    .await
    .map_err(|_| DB_FAILED)
}

/// PRIV-008: deletes the intake answers of `job`'s requests; returns how many went.
///
/// # Errors
/// [`DB_FAILED`] on a database failure.
pub async fn purge_answers(db: &mut PgConnection, job: Uuid) -> Result<u64, HandlerError> {
    let gone = sqlx::query!(
        "DELETE FROM intake.intake_answer WHERE conversation_id IN \
         (SELECT id FROM intake.conversation WHERE job_id = $1)",
        job,
    )
    .execute(db)
    .await
    .map_err(|_| DB_FAILED)?;
    Ok(gone.rows_affected())
}

/// The [`PURGE_KIND`](askhava_privacy::api::PURGE_KIND) handler; safe to run twice.
///
/// # Errors
/// A fixed [`HandlerError`] for a bad payload or a database or vault failure.
pub async fn run_purge(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let (job, stage) = purge_target(&task)?;
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    match stage {
        PurgeStage::Photos => Ok(()),
        PurgeStage::Text => purge_answers(&mut conn, job).await.map(|_| ()),
        PurgeStage::Contact => match contact_to_purge(&mut conn, job, Timestamp::now()).await? {
            Some(contact) => purge_contact(&mut conn, contact)
                .await
                .map(|_| ())
                .map_err(|_| HandlerError("intake contact purge failed")),
            None => Ok(()),
        },
    }
}
