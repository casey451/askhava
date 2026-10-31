//! Business record changes, each in one transaction with its audit row (BIZ-004 to BIZ-009,
//! JOB-005, R11).
//!
//! A status moves only by [`crate::status::next`] and only if still in the status it was read in
//! (JOB-004). Audit rows hold IDs, status codes and field names only, never a value (PRIV-028).
//! The after-hours tick is stored as given; T150 adds the BIZ-007 conditions on it (D-491).

use std::collections::BTreeSet;

use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, BusinessesError, Insurance, db};
use crate::rules::{self, Event, JobTypeChoice};
use crate::status::{AccountStatus, next};

const SUBJECT: &str = "businesses.business";

/// The ticks and answers a business sets for itself (BIZ-005 to BIZ-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticks {
    /// "Also send me jobs nobody else covers" (BIZ-005).
    pub overflow_on: bool,
    /// "We speak Spanish" (BIZ-006).
    pub speaks_spanish: bool,
    /// The business-reported insurance answer (BIZ-008).
    pub insurance: Option<Insurance>,
    /// "After-hours emergencies" (BIZ-007).
    pub after_hours: bool,
}

async fn log(
    tx: &mut PgConnection,
    actor: Actor,
    action: &'static str,
    id: Uuid,
    change: Change,
) -> Result<(), BusinessesError> {
    let entry = AuditEntry {
        actor,
        action,
        subject_kind: SUBJECT,
        subject_id: id,
        change,
    };
    audit(tx, &entry)
        .await
        .map(|_| ())
        .map_err(|_| BusinessesError::Audit)
}

/// Records a new business account: pending, or waitlist when it chose only coming-later
/// categories (BIZ-009, BIZ-021).
///
/// # Errors
///
/// [`BusinessesError::Database`] or [`BusinessesError::Audit`].
pub async fn create(
    conn: &mut PgConnection,
    waitlist: bool,
    now: Timestamp,
) -> Result<Uuid, BusinessesError> {
    let status = if waitlist {
        AccountStatus::Waitlist
    } else {
        AccountStatus::Pending
    };
    let mut tx = db(conn.begin().await)?;
    let id = models::insert(&mut tx, status, now).await?;
    log(
        &mut tx,
        Actor::Business(id),
        "businesses.created",
        id,
        Change::Action,
    )
    .await?;
    db(tx.commit().await)?;
    Ok(id)
}

/// Applies `event` to the business's status by the BIZ-009 rules (BIZ-003, BIZ-016), recording a
/// pause's reason, and returns the new status.
///
/// # Errors
///
/// [`BusinessesError::Refused`] when no rule allows the move; [`BusinessesError::MovedOn`] when
/// the status changed meanwhile; [`BusinessesError::UnknownBusiness`], `Database` or `Audit`.
pub async fn apply(
    conn: &mut PgConnection,
    id: Uuid,
    event: Event,
    actor: Actor,
    now: Timestamp,
) -> Result<AccountStatus, BusinessesError> {
    let mut tx = db(conn.begin().await)?;
    let from = models::read(&mut tx, id).await?.status;
    let to = next(from, event).map_err(|_| BusinessesError::Refused)?;
    let reason = match event {
        Event::Pause(r) => Some(r),
        Event::Checked(_)
        | Event::OwnerApproved(_)
        | Event::CategoryOpened
        | Event::Resume
        | Event::Remove => None,
    };
    if !models::update_status(&mut tx, id, from, to, reason, now).await? {
        return Err(BusinessesError::MovedOn);
    }
    let entry = AuditEntry::status_change(actor, (SUBJECT, id), from.code(), to.code());
    audit(&mut tx, &entry)
        .await
        .map_err(|_| BusinessesError::Audit)?;
    db(tx.commit().await)?;
    Ok(to)
}

/// Sets the overflow switch, the Spanish tick, the insurance answer and the after-hours tick
/// (BIZ-005, BIZ-006, BIZ-007, BIZ-008). Insurance never changes the status (D-061).
///
/// # Errors
///
/// [`BusinessesError::UnknownBusiness`], `Database` or `Audit`.
pub async fn set_ticks(
    conn: &mut PgConnection,
    id: Uuid,
    ticks: Ticks,
    actor: Actor,
) -> Result<(), BusinessesError> {
    let mut tx = db(conn.begin().await)?;
    let mut b = models::read(&mut tx, id).await?;
    let changed = [
        ("overflow_on", b.overflow_on != ticks.overflow_on),
        ("speaks_spanish", b.speaks_spanish != ticks.speaks_spanish),
        ("insured", b.insurance != ticks.insurance),
        ("after_hours", b.after_hours != ticks.after_hours),
    ];
    (b.overflow_on, b.speaks_spanish) = (ticks.overflow_on, ticks.speaks_spanish);
    (b.insurance, b.after_hours) = (ticks.insurance, ticks.after_hours);
    models::update_ticks(&mut tx, &b).await?;
    for (field, _) in changed.into_iter().filter(|(_, c)| *c) {
        log(
            &mut tx,
            actor,
            "businesses.field_changed",
            id,
            Change::Field(field),
        )
        .await?;
    }
    db(tx.commit().await)
}

/// Replaces the towns the business serves (BIZ-010).
///
/// # Errors
///
/// [`BusinessesError::BadCode`], `UnknownBusiness`, `Database` or `Audit`.
pub async fn set_towns(
    conn: &mut PgConnection,
    id: Uuid,
    towns: &[&str],
    actor: Actor,
) -> Result<(), BusinessesError> {
    let towns = rules::towns(towns)?;
    let mut tx = db(conn.begin().await)?;
    models::read(&mut tx, id).await?;
    models::replace_towns(&mut tx, id, &towns).await?;
    log(
        &mut tx,
        actor,
        "businesses.field_changed",
        id,
        Change::Field("towns"),
    )
    .await?;
    db(tx.commit().await)
}

/// Replaces the job-type checklist, each ticked job type within `allowed`: what the license covers
/// as the license map says plus the no-license job types (BIZ-004, BIZ-049, LIC-043).
///
/// # Errors
///
/// [`BusinessesError::NotCovered`], `BadCode`, `UnknownBusiness`, `Database` or `Audit`.
pub async fn set_job_types(
    conn: &mut PgConnection,
    id: Uuid,
    ticked: &[JobTypeChoice],
    allowed: &BTreeSet<JobTypeChoice>,
    actor: Actor,
) -> Result<(), BusinessesError> {
    let chosen = rules::checklist(ticked, allowed)?;
    let mut tx = db(conn.begin().await)?;
    models::read(&mut tx, id).await?;
    models::replace_job_types(&mut tx, id, &chosen).await?;
    log(
        &mut tx,
        actor,
        "businesses.field_changed",
        id,
        Change::Field("job_types"),
    )
    .await?;
    db(tx.commit().await)
}

/// How many active businesses with the after-hours tick serve `town` and ticked a job type in the
/// trade (catalog category) `category`, for the INT-033 count (BIZ-007, D-491). A paused business
/// is not active, so it never counts. T150 narrows this with the per-person switch.
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn night_count(
    conn: &mut PgConnection,
    category: &str,
    town: &str,
) -> Result<u32, BusinessesError> {
    let n = models::night_count(conn, category, town).await?;
    u32::try_from(n).map_err(|_| BusinessesError::Database)
}
