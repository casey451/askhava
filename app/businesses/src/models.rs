//! Row types and queries of the `businesses` schema (migration `0055_businesses_records.sql`).
//!
//! Every row holds IDs, codes, ticks and times only, never a name, phone, email or address
//! (PRIV-001). Each function takes the caller's connection, so `service` can group them in one
//! transaction with the audit row (JOB-005).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::rules::{Checks, JobTypeChoice};
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
        pause_reason: row
            .pause_reason
            .as_deref()
            .map(|c| PauseReason::from_code(c).ok_or(BusinessesError::Database))
            .transpose()?,
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

/// Locks the business row (`FOR UPDATE`), then replaces its towns, so concurrent saves apply one
/// after the other; `UnknownBusiness` when no such row.
pub(crate) async fn replace_towns(
    db: &mut PgConnection,
    id: Uuid,
    towns: &[String],
) -> Result<(), BusinessesError> {
    let lock = sqlx::query!(
        "SELECT id FROM businesses.business WHERE id = $1 FOR UPDATE",
        id
    );
    self::db(lock.fetch_optional(&mut *db).await)?.ok_or(BusinessesError::UnknownBusiness)?;
    let d = sqlx::query!("DELETE FROM businesses.town WHERE business_id = $1", id);
    self::db(d.execute(&mut *db).await)?;
    let q = sqlx::query!(
        "INSERT INTO businesses.town (business_id, town) SELECT $1, unnest($2::text[])",
        id,
        towns
    );
    self::db(q.execute(db).await).map(|_| ())
}

/// Locks the business row (`FOR UPDATE`), then replaces its checklist, so concurrent saves apply
/// one after the other; `UnknownBusiness` when no such row.
pub(crate) async fn replace_job_types(
    db: &mut PgConnection,
    id: Uuid,
    chosen: &[JobTypeChoice],
) -> Result<(), BusinessesError> {
    let lock = sqlx::query!(
        "SELECT id FROM businesses.business WHERE id = $1 FOR UPDATE",
        id
    );
    self::db(lock.fetch_optional(&mut *db).await)?.ok_or(BusinessesError::UnknownBusiness)?;
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

/// One sign-up's identity progress as stored in `businesses.signup` (BIZ-002, BIZ-016).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SignupState {
    pub(crate) step: Option<String>,
    pub(crate) code_done: bool,
    pub(crate) match_passed: bool,
    pub(crate) flagged: bool,
}

impl SignupState {
    /// What sign-up has proved, for [`crate::status::next`] (BIZ-003, BIZ-016).
    pub(crate) const fn checks(&self) -> Checks {
        Checks {
            code_done: self.code_done,
            match_passed: self.match_passed,
            flagged: self.flagged,
        }
    }
}

/// The sign-up row's values: mailbox, source answer, g code, submit time (BIZ-019, BIZ-053).
pub(crate) type NewSignup<'a> = (Uuid, Option<&'a str>, Option<&'a str>, Timestamp);

pub(crate) async fn insert_signup(
    db: &mut PgConnection,
    business: Uuid,
    (mailbox, heard, g_code, at): NewSignup<'_>,
) -> Result<(), BusinessesError> {
    let q = sqlx::query!(
        "INSERT INTO businesses.signup (business_id, mailbox_id, heard_from, g_code, submitted_at) \
         VALUES ($1, $2, $3, $4, $5)",
        business,
        mailbox,
        heard,
        g_code,
        at.to_sqlx() as _
    );
    self::db(q.execute(db).await).map(|_| ())
}

pub(crate) async fn signup_state(
    db: &mut PgConnection,
    business: Uuid,
) -> Result<SignupState, BusinessesError> {
    let q = sqlx::query!(
        "SELECT identity_step, code_done, match_passed, flag IS NOT NULL AS \"flagged!\" \
         FROM businesses.signup WHERE business_id = $1",
        business
    );
    let row = self::db(q.fetch_optional(db).await)?.ok_or(BusinessesError::UnknownBusiness)?;
    Ok(SignupState {
        step: row.identity_step,
        code_done: row.code_done,
        match_passed: row.match_passed,
        flagged: row.flagged,
    })
}

pub(crate) async fn set_step(
    db: &mut PgConnection,
    business: Uuid,
    step: &str,
) -> Result<(), BusinessesError> {
    let q = sqlx::query!(
        "UPDATE businesses.signup SET identity_step = $2 WHERE business_id = $1",
        business,
        step
    );
    self::db(q.execute(db).await).map(|_| ())
}

pub(crate) async fn set_code_done(
    db: &mut PgConnection,
    business: Uuid,
) -> Result<(), BusinessesError> {
    let q = sqlx::query!(
        "UPDATE businesses.signup SET code_done = true WHERE business_id = $1",
        business
    );
    self::db(q.execute(db).await).map(|_| ())
}

/// Sets the match when `passed` is given and keeps the first flag (BIZ-016).
pub(crate) async fn set_match(
    db: &mut PgConnection,
    business: Uuid,
    passed: Option<bool>,
    flag: Option<&str>,
) -> Result<(), BusinessesError> {
    let q = sqlx::query!(
        "UPDATE businesses.signup SET match_passed = coalesce($2, match_passed), \
         flag = coalesce(flag, $3) WHERE business_id = $1",
        business,
        passed,
        flag
    );
    self::db(q.execute(db).await).map(|_| ())
}

/// Stamps when a needs-review sign-up began waiting, once (BIZ-020).
pub(crate) async fn start_review(
    db: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<(), BusinessesError> {
    let q = sqlx::query!(
        "UPDATE businesses.signup s SET review_since = $2 FROM businesses.business b \
         WHERE s.business_id = $1 AND b.id = s.business_id AND b.status = 'needs_review' \
         AND s.review_since IS NULL",
        business,
        now.to_sqlx() as _
    );
    self::db(q.execute(db).await).map(|_| ())
}

/// Needs-review sign-ups never told their status, or last told 7 days or more before `now`
/// (BIZ-020).
pub(crate) async fn review_due(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<Vec<Uuid>, BusinessesError> {
    let q = sqlx::query_scalar!(
        "SELECT s.business_id FROM businesses.signup s JOIN businesses.business b \
         ON b.id = s.business_id WHERE b.status = 'needs_review' AND s.review_since IS NOT NULL \
         AND (s.review_told_at IS NULL OR s.review_told_at <= $1::timestamptz - interval '7 days') \
         ORDER BY s.review_since",
        now.to_sqlx() as _
    );
    self::db(q.fetch_all(db).await)
}

pub(crate) async fn set_review_told(
    db: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<(), BusinessesError> {
    let q = sqlx::query!(
        "UPDATE businesses.signup SET review_told_at = $2 WHERE business_id = $1",
        business,
        now.to_sqlx() as _
    );
    self::db(q.execute(db).await).map(|_| ())
}

pub(crate) async fn set_about_us(
    db: &mut PgConnection,
    business: Uuid,
    line: Option<&str>,
) -> Result<bool, BusinessesError> {
    let q = sqlx::query!(
        "UPDATE businesses.signup SET about_us = $2 WHERE business_id = $1",
        business,
        line
    );
    self::db(q.execute(db).await).map(|r| r.rows_affected() == 1)
}

/// One TERMS-033 row: business, person, document, version, text hash, language, assent line, time.
pub(crate) type TermsRow<'a> = (Uuid, Uuid, [&'a str; 5], Timestamp);

pub(crate) async fn insert_terms(
    db: &mut PgConnection,
    (business, person, [doc, version, sha, lang, line], at): TermsRow<'_>,
) -> Result<Uuid, BusinessesError> {
    let q = sqlx::query_scalar!(
        "INSERT INTO businesses.terms_acceptance (business_id, person_id, document, version, \
         text_sha256, language, channel, assent_line, accepted_at) \
         VALUES ($1, $2, $3, $4, $5, $6, 'business_signup', $7, $8) RETURNING id",
        business,
        person,
        doc,
        version,
        sha,
        lang,
        line,
        at.to_sqlx() as _
    );
    self::db(q.fetch_one(db).await)
}
