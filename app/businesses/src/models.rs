//! Row types and queries of the `businesses` schema (migration `0055_businesses_records.sql`).
//!
//! Every row holds IDs, codes, ticks and times only, never a name, phone, email or address
//! (PRIV-001). Each function takes the caller's connection, so `service` can group them in one
//! transaction with the audit row (JOB-005).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::rules::JobTypeChoice;
use crate::status::{AccountStatus, PauseReason};

/// Why a businesses call failed. No variant carries a value (rust brain R5, R6).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BusinessesError {
    /// A database call failed.
    #[error("businesses database call failed")]
    Database,
    /// The audit row could not be written (JOB-005).
    #[error("businesses audit row failed")]
    Audit,
    /// No business has the given ID.
    #[error("business not found")]
    UnknownBusiness,
    /// No status rule allows the move (BIZ-009).
    #[error("business status move refused")]
    Refused,
    /// The business left the expected status first (JOB-004).
    #[error("business status moved on")]
    MovedOn,
    /// A ticked job type is outside what the license covers (BIZ-004, LIC-043).
    #[error("job type not covered by the license")]
    NotCovered,
    /// A town, category or job type is not a valid code.
    #[error("invalid town, category or job type code")]
    BadCode,
}

pub(crate) fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, BusinessesError> {
    r.map_err(|_| BusinessesError::Database)
}

/// The business-reported insurance answer (BIZ-008): never verified, tracked for expiry or used
/// to pause (D-061).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Insurance {
    /// The business says it is insured.
    Yes,
    /// The business says it is not insured.
    No,
}

/// One business account as stored in `businesses.business` (BIZ-005 to BIZ-009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Business {
    /// Row ID.
    pub id: Uuid,
    /// The account status (BIZ-009).
    pub status: AccountStatus,
    /// Why it is paused; set exactly when the status is paused.
    pub pause_reason: Option<PauseReason>,
    /// "Also send me jobs nobody else covers" (BIZ-005).
    pub overflow_on: bool,
    /// "We speak Spanish", shown on its quote card (BIZ-006).
    pub speaks_spanish: bool,
    /// The insurance answer, `None` until given (BIZ-008).
    pub insurance: Option<Insurance>,
    /// The "after-hours emergencies" tick (BIZ-007).
    pub after_hours: bool,
    /// When the status last changed.
    pub status_changed_at: Timestamp,
}

pub(crate) async fn insert(
    db: &mut PgConnection,
    status: AccountStatus,
    now: Timestamp,
) -> Result<Uuid, BusinessesError> {
    let at = now.to_sqlx();
    self::db(
        sqlx::query_scalar!(
            "INSERT INTO businesses.business (status, created_at, status_changed_at) \
             VALUES ($1, $2, $2) RETURNING id",
            status.code(),
            at as _
        )
        .fetch_one(db)
        .await,
    )
}

pub(crate) async fn read(db: &mut PgConnection, id: Uuid) -> Result<Business, BusinessesError> {
    let row = self::db(
        sqlx::query!(
            "SELECT id, status, pause_reason, overflow_on, speaks_spanish, insured, after_hours, \
             status_changed_at FROM businesses.business WHERE id = $1",
            id
        )
        .fetch_optional(db)
        .await,
    )?
    .ok_or(BusinessesError::UnknownBusiness)?;
    Ok(Business {
        id: row.id,
        status: AccountStatus::from_code(&row.status).ok_or(BusinessesError::Database)?,
        pause_reason: row.pause_reason.as_deref().and_then(PauseReason::from_code),
        overflow_on: row.overflow_on,
        speaks_spanish: row.speaks_spanish,
        insurance: row
            .insured
            .map(|y| if y { Insurance::Yes } else { Insurance::No }),
        after_hours: row.after_hours,
        status_changed_at: row.status_changed_at.to_jiff(),
    })
}

/// Moves the status only if it is still `from` (JOB-004); false when it moved on.
pub(crate) async fn update_status(
    db: &mut PgConnection,
    id: Uuid,
    from: AccountStatus,
    to: AccountStatus,
    reason: Option<PauseReason>,
    now: Timestamp,
) -> Result<bool, BusinessesError> {
    let done = self::db(
        sqlx::query!(
            "UPDATE businesses.business SET status = $3, pause_reason = $4, status_changed_at = $5 \
             WHERE id = $1 AND status = $2",
            id,
            from.code(),
            to.code(),
            reason.map(PauseReason::code),
            now.to_sqlx() as _
        )
        .execute(db)
        .await,
    )?;
    Ok(done.rows_affected() == 1)
}

/// Sets the four ticks and the insurance answer at once; false when no such business.
pub(crate) async fn update_ticks(
    db: &mut PgConnection,
    b: &Business,
) -> Result<bool, BusinessesError> {
    let insured = b.insurance.map(|i| matches!(i, Insurance::Yes));
    let done = self::db(
        sqlx::query!(
            "UPDATE businesses.business SET overflow_on = $2, speaks_spanish = $3, insured = $4, \
             after_hours = $5 WHERE id = $1",
            b.id,
            b.overflow_on,
            b.speaks_spanish,
            insured,
            b.after_hours
        )
        .execute(db)
        .await,
    )?;
    Ok(done.rows_affected() == 1)
}

pub(crate) async fn replace_towns(
    db: &mut PgConnection,
    id: Uuid,
    towns: &[String],
) -> Result<(), BusinessesError> {
    let d = sqlx::query!("DELETE FROM businesses.town WHERE business_id = $1", id);
    self::db(d.execute(&mut *db).await)?;
    let q = sqlx::query!(
        "INSERT INTO businesses.town (business_id, town) SELECT $1, unnest($2::text[])",
        id,
        towns
    );
    self::db(q.execute(db).await).map(|_| ())
}

pub(crate) async fn replace_job_types(
    db: &mut PgConnection,
    id: Uuid,
    chosen: &[JobTypeChoice],
) -> Result<(), BusinessesError> {
    let d = sqlx::query!(
        "DELETE FROM businesses.job_type_choice WHERE business_id = $1",
        id
    );
    self::db(d.execute(&mut *db).await)?;
    let categories: Vec<String> = chosen.iter().map(|c| c.category.clone()).collect();
    let job_types: Vec<String> = chosen.iter().map(|c| c.job_type.clone()).collect();
    let q = sqlx::query!(
        "INSERT INTO businesses.job_type_choice (business_id, category, job_type) \
         SELECT $1, c, j FROM unnest($2::text[], $3::text[]) AS t(c, j)",
        id,
        &categories,
        &job_types
    );
    self::db(q.execute(db).await).map(|_| ())
}

pub(crate) async fn towns(db: &mut PgConnection, id: Uuid) -> Result<Vec<String>, BusinessesError> {
    let q = sqlx::query_scalar!(
        "SELECT town FROM businesses.town WHERE business_id = $1 ORDER BY town",
        id
    );
    self::db(q.fetch_all(db).await)
}

pub(crate) async fn job_types(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<Vec<JobTypeChoice>, BusinessesError> {
    let rows = self::db(
        sqlx::query!(
            "SELECT category, job_type FROM businesses.job_type_choice WHERE business_id = $1 \
             ORDER BY category, job_type",
            id
        )
        .fetch_all(db)
        .await,
    )?;
    Ok(rows
        .into_iter()
        .map(|r| JobTypeChoice::new(&r.category, &r.job_type))
        .collect())
}

/// The active businesses ticking after-hours that serve `town` and ticked a job type in
/// `category` (BIZ-007, INT-033, D-491).
pub(crate) async fn night_count(
    db: &mut PgConnection,
    category: &str,
    town: &str,
) -> Result<i64, BusinessesError> {
    let q = sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM businesses.business b \
         WHERE b.status = 'active' AND b.after_hours \
         AND EXISTS (SELECT 1 FROM businesses.town t WHERE t.business_id = b.id AND t.town = $2) \
         AND EXISTS (SELECT 1 FROM businesses.job_type_choice c \
                     WHERE c.business_id = b.id AND c.category = $1)",
        category,
        town
    );
    self::db(q.fetch_one(db).await)
}
