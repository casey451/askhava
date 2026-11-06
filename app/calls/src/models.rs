//! The `calls` rows and their queries, all in the `calls` schema (CALL-001, CALL-017).
//!
//! `calls.number_pool` holds one row per connected job's masked line (job, homeowner, business,
//! the pool number, whether it is shared, when given and closed); `calls.pool_alert` each
//! number-pool alert raised and the assignment that raised it (append-only, D-231, D-1680).

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

/// Stores a job's line on `number`, shared or not, given at `at`.
pub(crate) async fn insert_line(
    tx: &mut PgConnection,
    (job, homeowner, business): (Uuid, Uuid, Uuid),
    (number, shared): (&str, bool),
    at: Timestamp,
) -> Result<Stored, CallsError> {
    let row = db(sqlx::query!(
        "INSERT INTO calls.number_pool \
         (job_id, homeowner_id, business_id, pool_number, shared, assigned_at) \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, seq",
        job,
        homeowner,
        business,
        number,
        shared,
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
