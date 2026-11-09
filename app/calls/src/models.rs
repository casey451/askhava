//! The `calls` rows and their queries, all in the `calls` schema (CALL-001, CALL-017).
//!
//! `calls.number_pool` holds one row per connected job's masked line (job, homeowner, business,
//! the pool number, whether it is shared, when given and closed); `calls.pool_alert` each
//! number-pool alert raised and the assignment that raised it (append-only, D-231, D-1680).
//! T262 (CALL-003, CALL-025, SEC-017, D-1755, D-1836): each line's stored `pin`, the per-day
//! `calls.pin_try` count (a digest of the calling number, never the number), the per-call
//! `calls.pin_call` record and `calls.call_start`.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;
use thiserror::Error;

/// Why a `calls` call failed. No variant carries a number or any other value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CallsError {
    /// The database refused or dropped the statement.
    #[error("the calls database statement failed")]
    Database,
    /// `ops` refused the audit row or the alert.
    #[error("the audit row or alert could not be written")]
    Ops,
    /// `messaging` could not check the suppression list.
    #[error("the suppression list could not be checked")]
    Messaging,
    /// The job already has an open line with another business or homeowner.
    #[error("the job already has an open line with another business or homeowner")]
    LineOpen,
    /// The pool holds no number; settings refuse an empty pool, so this means a broken build.
    #[error("the masked-number pool holds no number")]
    PoolEmpty,
    /// The job's open line uses a number no longer in the settings' pool.
    #[error("the job's open line uses a number no longer in the pool")]
    NumberRetired,
    /// No unused PIN was drawn for the number within the bounded retries; the pool alert was
    /// raised and no line was stored (CALL-003, CALL-017, D-1755).
    #[error("no free PIN was found on the pool number")]
    NoFreePin,
}

/// One open line as the assignment rule sees it: its number, homeowner and business.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenLine {
    pub(crate) number: String,
    pub(crate) homeowner: Uuid,
    pub(crate) business: Uuid,
}

/// A job's open line as stored: what a repeat assignment checks and returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JobLine {
    pub(crate) id: Uuid,
    pub(crate) number: String,
    pub(crate) homeowner: Uuid,
    pub(crate) business: Uuid,
    pub(crate) shared: bool,
}

/// A stored line: its ID and its place in assignment order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stored {
    pub(crate) id: Uuid,
    pub(crate) seq: i64,
}

/// Maps any database error to [`CallsError::Database`], keeping no message (PRIV-016).
fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, CallsError> {
    result.map_err(|_| CallsError::Database)
}

/// Serializes number assignments, so two jobs connected at once never both take the last free
/// number and two pool alerts never open together (CALL-017, D-231).
pub(crate) async fn lock_pool(tx: &mut PgConnection) -> Result<(), CallsError> {
    db(
        sqlx::query!("SELECT pg_advisory_xact_lock(hashtextextended('calls.number_pool', 76))")
            .execute(tx)
            .await,
    )?;
    Ok(())
}

/// The job's open line, if any.
pub(crate) async fn open_line_of(
    tx: &mut PgConnection,
    job: Uuid,
) -> Result<Option<JobLine>, CallsError> {
    let row = db(sqlx::query!(
        "SELECT id, pool_number, homeowner_id, business_id, shared FROM calls.number_pool \
         WHERE job_id = $1 AND closed_at IS NULL",
        job
    )
    .fetch_optional(tx)
    .await)?;
    Ok(row.map(|r| JobLine {
        id: r.id,
        number: r.pool_number,
        homeowner: r.homeowner_id,
        business: r.business_id,
        shared: r.shared,
    }))
}

/// Every open line, read under the pool lock: the rule needs those of the same homeowner or
/// business, and the count on each number to spread load.
pub(crate) async fn open_lines(tx: &mut PgConnection) -> Result<Vec<OpenLine>, CallsError> {
    let rows = db(sqlx::query!(
        "SELECT pool_number, homeowner_id, business_id FROM calls.number_pool \
         WHERE closed_at IS NULL"
    )
    .fetch_all(tx)
    .await)?;
    Ok(rows
        .into_iter()
        .map(|r| OpenLine {
            number: r.pool_number,
            homeowner: r.homeowner_id,
            business: r.business_id,
        })
        .collect())
}

/// Stores a job's line on `number`, shared or not, with its `pin`, given at `at`.
pub(crate) async fn insert_line(
    tx: &mut PgConnection,
    (job, homeowner, business): (Uuid, Uuid, Uuid),
    (number, shared, pin): (&str, bool, &str),
    at: Timestamp,
) -> Result<Stored, CallsError> {
    let row = db(sqlx::query!(
        "INSERT INTO calls.number_pool \
         (job_id, homeowner_id, business_id, pool_number, shared, pin, assigned_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id, seq",
        job,
        homeowner,
        business,
        number,
        shared,
        pin,
        at.to_sqlx() as _
    )
    .fetch_one(tx)
    .await)?;
    Ok(Stored {
        id: row.id,
        seq: row.seq,
    })
}

/// Whether a pool alert is open: one was raised after the last assignment that found a free
/// number (D-1680).
pub(crate) async fn alert_open(tx: &mut PgConnection) -> Result<bool, CallsError> {
    db(sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM calls.pool_alert WHERE line_seq > \
         coalesce((SELECT max(seq) FROM calls.number_pool WHERE NOT shared), 0)) AS \"open!\""
    )
    .fetch_one(tx)
    .await)
}

/// Records the pool alert `alert` raised by the line numbered `seq`.
pub(crate) async fn insert_alert(
    tx: &mut PgConnection,
    alert: Uuid,
    seq: i64,
    at: Timestamp,
) -> Result<(), CallsError> {
    db(sqlx::query!(
        "INSERT INTO calls.pool_alert (alert_id, line_seq, raised_at) VALUES ($1, $2, $3)",
        alert,
        seq,
        at.to_sqlx() as _
    )
    .execute(tx)
    .await)?;
    Ok(())
}

/// The PINs of the open lines on `number`, read under the pool lock (CALL-003, D-1755).
pub(crate) async fn used_pins(
    tx: &mut PgConnection,
    number: &str,
) -> Result<Vec<String>, CallsError> {
    db(sqlx::query_scalar!(
        "SELECT pin AS \"pin!\" FROM calls.number_pool \
         WHERE pool_number = $1 AND closed_at IS NULL AND pin IS NOT NULL",
        number
    )
    .fetch_all(tx)
    .await)
}

/// The stored PIN of line `line`, open or closed.
pub(crate) async fn line_pin(
    conn: &mut PgConnection,
    line: Uuid,
) -> Result<Option<String>, CallsError> {
    let row = db(
        sqlx::query_scalar!("SELECT pin FROM calls.number_pool WHERE id = $1", line)
            .fetch_optional(conn)
            .await,
    )?;
    Ok(row.flatten())
}

/// Who tried, where and on which Arizona day: the `calls.pin_try` key (CALL-025).
pub(crate) struct TryKey<'a> {
    pub(crate) digest: &'a [u8],
    pub(crate) version: i16,
    pub(crate) number: &'a str,
    pub(crate) day: jiff::civil::Date,
}

/// The wrong tries counted today for `key` (0 when none).
pub(crate) async fn tries_today(
    conn: &mut PgConnection,
    key: &TryKey<'_>,
) -> Result<i16, CallsError> {
    let row = db(sqlx::query_scalar!(
        "SELECT tries FROM calls.pin_try \
         WHERE caller_digest = $1 AND pool_number = $2 AND az_day = $3",
        key.digest,
        key.number,
        key.day.to_sqlx() as _
    )
    .fetch_optional(conn)
    .await)?;
    Ok(row.unwrap_or(0))
}

/// Counts one wrong try for `key`, never past `cap`: the day's count after it, or `None` when the
/// count was already at `cap`. One atomic statement, so concurrent tries cannot pass the cap.
pub(crate) async fn count_try(
    tx: &mut PgConnection,
    key: &TryKey<'_>,
    cap: i16,
) -> Result<Option<i16>, CallsError> {
    db(sqlx::query_scalar!(
        "INSERT INTO calls.pin_try (caller_digest, key_version, pool_number, az_day, tries) \
         VALUES ($1, $2, $3, $4, 1) \
         ON CONFLICT (caller_digest, pool_number, az_day) \
         DO UPDATE SET tries = calls.pin_try.tries + 1 WHERE calls.pin_try.tries < $5 \
         RETURNING tries",
        key.digest,
        key.version,
        key.number,
        key.day.to_sqlx() as _,
        cap
    )
    .fetch_optional(tx)
    .await)
}

/// Records try `try_no` of call `sid`, judged on `before` tries that day; `false` when that try
/// was already recorded (a repeated callback, SEC-017).
pub(crate) async fn record_call_try(
    tx: &mut PgConnection,
    (sid, try_no): (&str, i16),
    before: i16,
) -> Result<bool, CallsError> {
    let row = db(sqlx::query_scalar!(
        "INSERT INTO calls.pin_call (call_sid, try_no, today_before) VALUES ($1, $2, $3) \
         ON CONFLICT DO NOTHING RETURNING today_before",
        sid,
        try_no,
        before
    )
    .fetch_optional(tx)
    .await)?;
    Ok(row.is_some())
}

/// The day's count try `try_no` of call `sid` was judged on, when it was recorded.
pub(crate) async fn call_try(
    conn: &mut PgConnection,
    (sid, try_no): (&str, i16),
) -> Result<Option<i16>, CallsError> {
    db(sqlx::query_scalar!(
        "SELECT today_before FROM calls.pin_call WHERE call_sid = $1 AND try_no = $2",
        sid,
        try_no
    )
    .fetch_optional(conn)
    .await)
}

/// Stores the start of call `parent` on `line` at `at`, once per parent CallSid; `false` when it
/// was already stored (FEE-022, D-1836).
pub(crate) async fn insert_call_start(
    conn: &mut PgConnection,
    parent: &str,
    line: Uuid,
    at: Timestamp,
) -> Result<bool, CallsError> {
    let row = db(sqlx::query_scalar!(
        "INSERT INTO calls.call_start (parent_sid, line_id, started_at) VALUES ($1, $2, $3) \
         ON CONFLICT DO NOTHING RETURNING line_id",
        parent,
        line,
        at.to_sqlx() as _
    )
    .fetch_optional(conn)
    .await)?;
    Ok(row.is_some())
}

/// The stored start of the call with parent CallSid `parent`: its line and start time.
pub(crate) async fn call_start(
    conn: &mut PgConnection,
    parent: &str,
) -> Result<Option<(Uuid, Timestamp)>, CallsError> {
    let row = db(sqlx::query!(
        "SELECT line_id, started_at FROM calls.call_start WHERE parent_sid = $1",
        parent
    )
    .fetch_optional(conn)
    .await)?;
    Ok(row.map(|r| (r.line_id, r.started_at.to_jiff())))
}
