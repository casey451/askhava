//! The `picks` rows and their queries, all in the `picks` schema (PICK-013, PICK-016, PICK-031,
//! PICK-032, PICK-055, CON-029).
//!
//! `picks.pick` holds each pick (job, business, quote number, channel, the inbound text of a text
//! pick, status); `picks.code_attempt` one row per Verify code sent, by job and vault contact ID,
//! never the number (PRIV-002); `picks.verify_cap_day` the days the global cap was hit (one High
//! alert each); `picks.terms_line` each queued text that ends with the Terms line (PICK-016).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;
use thiserror::Error;

use crate::rules::{CodeRow, SendCounts};
use crate::status::PickStatus;

/// Why a `picks` call failed. No variant carries a value or a vendor message (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PicksError {
    /// The database refused or dropped the statement.
    #[error("the picks database statement failed")]
    Database,
    /// `jobs` failed to read or move the job.
    #[error("the job could not be read or moved")]
    Job,
    /// `quotes` failed to list the job's quotes.
    #[error("the job's quotes could not be read")]
    Quotes,
    /// A compiled-in text failed to render.
    #[error("a text template failed to render")]
    Template,
    /// The outbox refused the text.
    #[error("the text could not be queued")]
    Outbox,
    /// The audit row could not be written.
    #[error("the audit row could not be written")]
    Audit,
    /// Twilio Verify failed or could not be reached.
    #[error("the Verify call failed")]
    Verify,
    /// The job left the status the pick expected; nothing was kept (JOB-004).
    #[error("the job moved on")]
    MovedOn,
}

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, PicksError> {
    r.map_err(|_| PicksError::Database)
}

/// A new pick waiting for Confirm: job, business, quote number, inbound text (a text pick).
pub(crate) type NewPick = (Uuid, Uuid, i32, Option<Uuid>);

pub(crate) async fn insert_pick(
    tx: &mut PgConnection,
    (job, business, quote, inbox): NewPick,
    at: Timestamp,
) -> Result<Uuid, PicksError> {
    let channel = if inbox.is_some() { "text" } else { "page" };
    db(sqlx::query_scalar!(
        "INSERT INTO picks.pick (job_id, business_id, quote_number, channel, inbox_id, status, \
         picked_at) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        job,
        business,
        quote,
        channel,
        inbox,
        PickStatus::AwaitingConfirm.code(),
        at.to_sqlx() as _,
    )
    .fetch_one(tx)
    .await)
}

/// The job's pick waiting for Confirm: ID, quote number, business.
pub(crate) async fn waiting_pick(
    tx: &mut PgConnection,
    job: Uuid,
) -> Result<Option<(Uuid, i32, Uuid)>, PicksError> {
    let row = db(sqlx::query!(
        "SELECT id, quote_number, business_id FROM picks.pick WHERE job_id = $1 AND status = $2",
        job,
        PickStatus::AwaitingConfirm.code(),
    )
    .fetch_optional(tx)
    .await)?;
    Ok(row.map(|r| (r.id, r.quote_number, r.business_id)))
}

/// Ends a waiting pick as cancelled with `reason` (`replaced` or `paused`); `false` if it no longer
/// waits.
pub(crate) async fn end_pick(
    tx: &mut PgConnection,
    id: Uuid,
    reason: &str,
    at: Timestamp,
) -> Result<bool, PicksError> {
    let done = db(sqlx::query!(
        "UPDATE picks.pick SET status = $2, end_reason = $3, ended_at = $4 \
         WHERE id = $1 AND status = $5",
        id,
        PickStatus::Cancelled.code(),
        reason,
        at.to_sqlx() as _,
        PickStatus::AwaitingConfirm.code(),
    )
    .execute(tx)
    .await)?;
    Ok(done.rows_affected() == 1)
}

/// Records a queued text ending with the Terms line for `job` (PICK-016, D-492).
pub(crate) async fn insert_terms_line(
    tx: &mut PgConnection,
    (job, outbox): (Uuid, Uuid),
    version: &str,
    at: Timestamp,
) -> Result<(), PicksError> {
    db(sqlx::query!(
        "INSERT INTO picks.terms_line (job_id, outbox_id, template_version, queued_at) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (job_id, outbox_id) DO NOTHING",
        job,
        outbox,
        version,
        at.to_sqlx() as _,
    )
    .execute(tx)
    .await)?;
    Ok(())
}

/// Whether a text ending with the Terms line was queued for `job` (PICK-016, PICK-030).
pub(crate) async fn terms_sent(tx: &mut PgConnection, job: Uuid) -> Result<bool, PicksError> {
    let found = db(sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM picks.terms_line WHERE job_id = $1) AS \"found!\"",
        job
    )
    .fetch_one(tx)
    .await)?;
    Ok(found)
}

/// Serializes code sends and tries for one vault contact (PICK-055's caps hold under races).
pub(crate) async fn lock_contact(tx: &mut PgConnection, contact: Uuid) -> Result<(), PicksError> {
    db(sqlx::query!(
        "SELECT pg_advisory_xact_lock(hashtextextended($1::text, 55))",
        contact.to_string()
    )
    .execute(tx)
    .await)?;
    Ok(())
}

/// The latest code sent for `job` to `contact`, locked.
pub(crate) async fn latest_code(
    tx: &mut PgConnection,
    (job, contact): (Uuid, Uuid),
) -> Result<Option<(Uuid, CodeRow)>, PicksError> {
    let row = db(sqlx::query!(
        "SELECT id, sent_at, tries, last_try_at, verified_at IS NOT NULL AS \"verified!\" \
         FROM picks.code_attempt WHERE job_id = $1 AND contact_id = $2 \
         ORDER BY sent_at DESC, id DESC LIMIT 1 FOR UPDATE",
        job,
        contact
    )
    .fetch_optional(tx)
    .await)?;
    Ok(row.map(|r| {
        let code = CodeRow {
            sent_at: r.sent_at.to_jiff(),
            tries: r.tries,
            last_try_at: r.last_try_at.map(|t| t.to_jiff()),
            verified: r.verified,
        };
        (r.id, code)
    }))
}

/// The sends since `since` (the start of the Arizona day) by number, voice, job and in all.
pub(crate) async fn send_counts(
    tx: &mut PgConnection,
    (job, contact): (Uuid, Uuid),
    since: Timestamp,
) -> Result<SendCounts, PicksError> {
    let r = db(sqlx::query!(
        "SELECT count(*) FILTER (WHERE contact_id = $2) AS \"to_number!\", \
         count(*) FILTER (WHERE contact_id = $2 AND channel = 'call') AS \"calls!\", \
         count(*) FILTER (WHERE job_id = $1) AS \"for_job!\", count(*) AS \"global!\" \
         FROM picks.code_attempt WHERE sent_at >= $3",
        job,
        contact,
        since.to_sqlx() as _,
    )
    .fetch_one(tx)
    .await)?;
    Ok(SendCounts {
        to_number: r.to_number,
        calls_to_number: r.calls,
        for_job: r.for_job,
        global: r.global,
    })
}

/// Records one code sent for `job` to `contact` by `channel` (`sms` or `call`).
pub(crate) async fn insert_code(
    tx: &mut PgConnection,
    (job, contact): (Uuid, Uuid),
    channel: &str,
    at: Timestamp,
) -> Result<Uuid, PicksError> {
    db(sqlx::query_scalar!(
        "INSERT INTO picks.code_attempt (job_id, contact_id, channel, sent_at) \
         VALUES ($1, $2, $3, $4) RETURNING id",
        job,
        contact,
        channel,
        at.to_sqlx() as _,
    )
    .fetch_one(tx)
    .await)
}

/// Counts one try on code `id` at `at`.
pub(crate) async fn count_try(
    tx: &mut PgConnection,
    id: Uuid,
    at: Timestamp,
) -> Result<i32, PicksError> {
    db(sqlx::query_scalar!(
        "UPDATE picks.code_attempt SET tries = tries + 1, last_try_at = $2 WHERE id = $1 \
         RETURNING tries",
        id,
        at.to_sqlx() as _,
    )
    .fetch_one(tx)
    .await)
}

/// Marks code `id` as passed at `at`.
pub(crate) async fn mark_verified(
    tx: &mut PgConnection,
    id: Uuid,
    at: Timestamp,
) -> Result<(), PicksError> {
    db(sqlx::query!(
        "UPDATE picks.code_attempt SET verified_at = $2 WHERE id = $1 AND verified_at IS NULL",
        id,
        at.to_sqlx() as _,
    )
    .execute(tx)
    .await)?;
    Ok(())
}

/// Records that the global cap was hit on Arizona day `day` (`YYYY-MM-DD`); `true` only the first
/// time that day, so one High alert goes per day.
pub(crate) async fn cap_hit(
    tx: &mut PgConnection,
    day: &str,
    at: Timestamp,
) -> Result<bool, PicksError> {
    let done = db(sqlx::query!(
        "INSERT INTO picks.verify_cap_day (day, hit_at) VALUES ($1::text::date, $2) \
         ON CONFLICT (day) DO NOTHING",
        day,
        at.to_sqlx() as _,
    )
    .execute(tx)
    .await)?;
    Ok(done.rows_affected() == 1)
}
