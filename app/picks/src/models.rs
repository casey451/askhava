//! The `picks` rows and their queries, all in the `picks` schema (PICK-013, PICK-016, PICK-031,
//! PICK-032, PICK-055, CON-029).
//!
//! `picks.pick` holds each pick (job, business, quote number, channel, the inbound text of a text
//! pick, status); `picks.code_attempt` one row per Verify code sent, by job and vault contact ID,
//! never the number (PRIV-002); `picks.verify_cap_day` the days the global cap was hit (one High
//! alert each); `picks.terms_line` each queued text that ends with the Terms line (PICK-016).
//! T063 adds locking a waiting pick and expiring it at the Confirm window's end (PICK-019).
//! T069 adds `picks.release` and `picks.address_wait` (PICK-022, INT-029). T253 binds each release
//! to its contact ID and email choice and reads that binding back for the released view (D-1739).

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

/// The waiting pick `id`, locked: job and business; none once it no longer waits (PICK-019).
pub(crate) async fn lock_waiting(
    tx: &mut PgConnection,
    id: Uuid,
) -> Result<Option<(Uuid, Uuid)>, PicksError> {
    let row = db(sqlx::query!(
        "SELECT job_id, business_id FROM picks.pick WHERE id = $1 AND status = $2 FOR UPDATE",
        id,
        PickStatus::AwaitingConfirm.code(),
    )
    .fetch_optional(tx)
    .await)?;
    Ok(row.map(|r| (r.job_id, r.business_id)))
}

/// Ends waiting pick `id` as expired at `at` (PICK-019, PICK-035).
pub(crate) async fn expire_pick(
    tx: &mut PgConnection,
    id: Uuid,
    at: Timestamp,
) -> Result<(), PicksError> {
    db(sqlx::query!(
        "UPDATE picks.pick SET status = $2, ended_at = $3 WHERE id = $1 AND status = $4",
        id,
        PickStatus::Expired.code(),
        at.to_sqlx() as _,
        PickStatus::AwaitingConfirm.code(),
    )
    .execute(tx)
    .await)?;
    Ok(())
}

/// What [`release_target`] reads of a pick for a release (PICK-022).
pub(crate) struct ReleaseTarget {
    /// The pick's job.
    pub job: Uuid,
    /// The picked business.
    pub business: Uuid,
    /// The pick ended (`ended_at` set).
    pub ended: bool,
    /// The pick's status is the one the proof may release from (`status::release_status`).
    pub confirmed: bool,
    /// The pick already has its `picks.release` row.
    pub released: bool,
}

/// Pick `id` for a release: job, business, ended, whether its status is `want` and whether it
/// was released (PICK-022, D-1564).
pub(crate) async fn release_target(
    db_conn: &mut PgConnection,
    id: Uuid,
    want: PickStatus,
) -> Result<Option<ReleaseTarget>, PicksError> {
    let row = db(sqlx::query!(
        r#"SELECT p.job_id, p.business_id, p.ended_at IS NOT NULL AS "ended!",
             p.status = $2 AS "confirmed!",
             EXISTS (SELECT 1 FROM picks.release r WHERE r.pick_id = p.id) AS "released!"
           FROM picks.pick p WHERE p.id = $1"#,
        id,
        want.code(),
    )
    .fetch_optional(db_conn)
    .await)?;
    Ok(row.map(|r| ReleaseTarget {
        job: r.job_id,
        business: r.business_id,
        ended: r.ended,
        confirmed: r.confirmed,
        released: r.released,
    }))
}

/// Records pick `id`'s one release, bound to the contact the proof named and her PICK-018 email
/// choice (an ID and a flag, never a value, PRIV-002, D-1739); false when another call already did
/// (PICK-022, D-365).
pub(crate) async fn insert_release(
    tx: &mut PgConnection,
    (id, job, business): (Uuid, Uuid, Uuid),
    (basis, contact, email_chosen): (&str, Uuid, bool),
    at: Timestamp,
) -> Result<bool, PicksError> {
    let done = db(sqlx::query!(
        "INSERT INTO picks.release (pick_id, job_id, business_id, basis, released_at, \
         contact_id, email_chosen) VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (pick_id) DO NOTHING",
        id,
        job,
        business,
        basis,
        at.to_sqlx() as _,
        contact,
        email_chosen,
    )
    .execute(tx)
    .await)?;
    Ok(done.rows_affected() == 1)
}

/// What [`release_binding`] reads of a released pick for the released view (D-1739).
pub(crate) struct ReleaseBinding {
    /// The picked business.
    pub business: Uuid,
    /// The pick's status is one [`crate::status::shows_release`] allows (paid or free).
    pub shown_status: bool,
    /// The homeowner contact the release proof named, an ID only.
    pub contact: Uuid,
    /// Her PICK-018 choice of email contact at the release.
    pub email_chosen: bool,
}

/// Pick `id`'s release binding, or `None` when the pick has no `picks.release` row; `shown` are
/// the status codes the view may show from (PICK-022, D-1739). Reads nothing in the vault.
pub(crate) async fn release_binding(
    db_conn: &mut PgConnection,
    id: Uuid,
    shown: &[String],
) -> Result<Option<ReleaseBinding>, PicksError> {
    let row = db(sqlx::query!(
        r#"SELECT p.business_id, p.status = ANY($2) AS "shown!", r.contact_id, r.email_chosen
           FROM picks.pick p JOIN picks.release r ON r.pick_id = p.id WHERE p.id = $1"#,
        id,
        shown,
    )
    .fetch_optional(db_conn)
    .await)?;
    Ok(row.map(|r| ReleaseBinding {
        business: r.business_id,
        shown_status: r.shown,
        contact: r.contact_id,
        email_chosen: r.email_chosen,
    }))
}

/// Records an emergency acceptance that came before the address, once per pick; false when one
/// was recorded before (INT-029, TD-INT-34, D-1564).
pub(crate) async fn insert_address_wait(
    tx: &mut PgConnection,
    (id, job): (Uuid, Uuid),
    at: Timestamp,
) -> Result<bool, PicksError> {
    let done = db(sqlx::query!(
        "INSERT INTO picks.address_wait (pick_id, job_id, asked_at) VALUES ($1, $2, $3) \
         ON CONFLICT (pick_id) DO NOTHING",
        id,
        job,
        at.to_sqlx() as _,
    )
    .execute(tx)
    .await)?;
    Ok(done.rows_affected() == 1)
}
