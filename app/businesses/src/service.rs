//! Business record changes, each in one transaction with its audit row (BIZ-004 to BIZ-009,
//! JOB-005, R11).
//!
//! A status moves only by [`crate::status::next`] and only if still in the status it was read in
//! (JOB-004). Audit rows hold IDs, status codes and field names only, never a value (PRIV-028).
//! The after-hours tick is stored as given; T150 adds the BIZ-007 conditions on it (D-491).
//! T182 added the self-pause and self-resume and the business's own quiet times (BIZ-010, D-085,
//! D-968, D-969).
//! T105 reports coverage to `ops` (OPS-006, D-493): a status move into or out of active, and a
//! towns or job-type edit on an active business, count the active businesses of each town and
//! trade it touched (before and after an edit) under `ops`' one coverage lock, taken before the
//! count, so two changes at once never report a stale count and two reports in one transaction
//! cannot deadlock on key order ([`report_coverage`]). A license change that drops job types
//! through `capability.rs` reports through [`report_edit`] too (T585, D-4052).
//! T677 (CARD-018, D-4360, D-5075): a move into a status that `receives_cards` from one that does
//! not, and, on a business that receives cards, a towns or job-type edit that adds one or a tick
//! turned on (overflow, after hours), queue one [`BECAME_ELIGIBLE_KIND`] task in the same
//! transaction ([`queue_eligible`]); `quotes` sends the late cards.

use std::collections::BTreeSet;

use askhava_core::api::{NewTask, enqueue};
use askhava_ops::api::{
    Actor, AuditEntry, Change, CoverageCount, CoverageScope, audit, lock_coverage,
    record_business_counts,
};
use jiff::Timestamp;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, BusinessesError, Insurance, db};
use crate::rules::{self, Event, JobTypeChoice, OwnHours};
use crate::status::{AccountStatus, PauseReason, next, replaces_self_pause};

const SUBJECT: &str = "businesses.business";

/// The `core.task` kind queued when a business may newly get job cards (CARD-018, D-4360): its
/// payload is `{"business": <id>}` only, never a homeowner detail; `quotes` handles it.
pub const BECAME_ELIGIBLE_KIND: &str = "businesses.became_eligible";

/// CARD-018, D-4360, D-5075: queues one [`BECAME_ELIGIBLE_KIND`] task for business `id` in the
/// caller's transaction, one per business and minute of `now` (the dedupe key); it is due at the
/// start of the next minute, so a later change in the same minute is committed before it runs.
/// `quotes` calls it too, to look again once the welcome email has gone (D-5076).
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn queue_eligible(
    tx: &mut PgConnection,
    id: Uuid,
    now: Timestamp,
) -> Result<(), BusinessesError> {
    let minute = now.as_second().div_euclid(60);
    let due = Timestamp::from_second((minute + 1) * 60).unwrap_or(now);
    let mut task = NewTask::new(
        BECAME_ELIGIBLE_KIND,
        json!({ "business": id }),
        due.max(now),
    );
    task.dedupe_key = Some(format!("{BECAME_ELIGIBLE_KIND}.{id}.{minute}"));
    enqueue(tx, &task)
        .await
        .map(|_| ())
        .map_err(|_| BusinessesError::Database)
}

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
/// pause's reason, and returns the new status. A pause on a paused business is refused here;
/// a rule's pause goes through [`pause_for_rule`] (D-970).
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
    let to = move_status(&mut tx, id, event, actor, now).await?;
    db(tx.commit().await)?;
    Ok(to)
}

/// Moves the status inside the caller's transaction, with its audit row (BIZ-009, JOB-004,
/// JOB-005).
async fn move_status(
    tx: &mut PgConnection,
    id: Uuid,
    event: Event,
    actor: Actor,
    now: Timestamp,
) -> Result<AccountStatus, BusinessesError> {
    let from = models::read(tx, id).await?.status;
    let to = next(from, event).map_err(|_| BusinessesError::Refused)?;
    let reason = match event {
        Event::Pause(r) => Some(r),
        Event::Checked(_)
        | Event::OwnerApproved(_)
        | Event::CategoryOpened
        | Event::Resume
        | Event::Remove => None,
    };
    if !models::update_status(tx, id, from, to, reason, now).await? {
        return Err(BusinessesError::MovedOn);
    }
    let entry = AuditEntry::status_change(actor, (SUBJECT, id), from.code(), to.code());
    audit(tx, &entry)
        .await
        .map_err(|_| BusinessesError::Audit)?;
    if from.counts_for_coverage() || to.counts_for_coverage() {
        let keys = coverage_keys(tx, id).await?;
        report_coverage(tx, keys, now).await?;
    }
    if to.receives_cards() && !from.receives_cards() {
        queue_eligible(tx, id, now).await?;
    }
    Ok(to)
}

/// The towns and trades (catalog categories) business `id` serves now (OPS-006).
pub(crate) async fn coverage_keys(
    tx: &mut PgConnection,
    id: Uuid,
) -> Result<(Vec<String>, Vec<String>), BusinessesError> {
    let row = db(sqlx::query!(
        r#"SELECT ARRAY(SELECT town FROM businesses.town WHERE business_id = $1) AS "towns!",
             ARRAY(SELECT DISTINCT category FROM businesses.job_type_choice
                   WHERE business_id = $1) AS "trades!""#,
        id
    )
    .fetch_one(&mut *tx)
    .await)?;
    Ok((row.towns, row.trades))
}

/// OPS-006, D-493: under `ops`' coverage lock (taken before counting), counts the active
/// businesses of each town and trade in `keys` and reports them to `ops` at `now`.
async fn report_coverage(
    tx: &mut PgConnection,
    (towns, trades): (Vec<String>, Vec<String>),
    now: Timestamp,
) -> Result<(), BusinessesError> {
    lock_coverage(tx)
        .await
        .map_err(|_| BusinessesError::Database)?;
    let rows = db(sqlx::query!(
        r#"SELECT k.town AS "town!", k.code AS "code!", (SELECT count(*) FROM businesses.business b
             WHERE b.status = 'active' AND CASE WHEN k.town
               THEN EXISTS (SELECT 1 FROM businesses.town t WHERE t.business_id = b.id
                            AND t.town = k.code)
               ELSE EXISTS (SELECT 1 FROM businesses.job_type_choice c WHERE c.business_id = b.id
                            AND c.category = k.code) END) AS "active!"
           FROM (SELECT true AS town, unnest($1::text[]) AS code
                 UNION SELECT false, unnest($2::text[])) AS k"#,
        &towns,
        &trades
    )
    .fetch_all(&mut *tx)
    .await)?;
    let counts: Vec<CoverageCount> = rows
        .into_iter()
        .map(|r| CoverageCount {
            scope: if r.town {
                CoverageScope::Town
            } else {
                CoverageScope::Trade
            },
            code: r.code,
            active: u32::try_from(r.active).unwrap_or(u32::MAX),
        })
        .collect();
    record_business_counts(tx, &counts, now)
        .await
        .map(|_| ())
        .map_err(|_| BusinessesError::Database)
}

/// OPS-006: after an edit of an active business's towns or job types, reports the keys it served
/// before (`before`) and serves now, so a dropped town or trade is counted too.
pub(crate) async fn report_edit(
    tx: &mut PgConnection,
    id: Uuid,
    before: (Vec<String>, Vec<String>),
) -> Result<(), BusinessesError> {
    let (mut towns, mut trades) = coverage_keys(tx, id).await?;
    towns.extend(before.0);
    trades.extend(before.1);
    report_coverage(tx, (towns, trades), Timestamp::now()).await
}

/// Replaces a `self` pause reason with a rule's, only if the row still holds the `self` reason,
/// with one audit row naming the field (BIZ-009, PRIV-028, D-970).
async fn replace_self_pause(
    tx: &mut PgConnection,
    id: Uuid,
    rule: PauseReason,
    actor: Actor,
) -> Result<(), BusinessesError> {
    let done = db(sqlx::query!(
        "UPDATE businesses.business SET pause_reason = $2 \
         WHERE id = $1 AND status = 'paused' AND pause_reason = 'self'",
        id,
        rule.code()
    )
    .execute(&mut *tx)
    .await)?;
    if done.rows_affected() != 1 {
        return Err(BusinessesError::MovedOn);
    }
    log(
        tx,
        actor,
        "businesses.field_changed",
        id,
        Change::Field("pause_reason"),
    )
    .await
}

/// Locks the business row and returns its pause reason, so no other pause or resume slips in
/// before the caller's move (BIZ-009, D-969, D-970).
async fn lock_reason(
    tx: &mut PgConnection,
    id: Uuid,
) -> Result<Option<PauseReason>, BusinessesError> {
    let reason = db(sqlx::query_scalar!(
        "SELECT pause_reason FROM businesses.business WHERE id = $1 FOR UPDATE",
        id
    )
    .fetch_optional(&mut *tx)
    .await)?
    .ok_or(BusinessesError::UnknownBusiness)?;
    Ok(reason.as_deref().and_then(PauseReason::from_code))
}

/// Pauses the business for a rule's `reason` (complaints, the license, no covered job type or
/// Casey; BIZ-009): an active business becomes paused as by [`apply`]; a business paused only by
/// itself stays paused and the rule's reason replaces `self`, with one audit row naming the field,
/// so a self-resume can no longer reactivate it (D-969, D-970). Every rule pause must come here,
/// not through [`apply`], which refuses a pause on a paused business.
///
/// # Errors
///
/// [`BusinessesError::Refused`] for the `self` reason (use [`pause_self`]) or when the business is
/// neither active nor paused only by itself; `UnknownBusiness`, `MovedOn`, `Database` or `Audit`.
pub async fn pause_for_rule(
    conn: &mut PgConnection,
    id: Uuid,
    reason: PauseReason,
    actor: Actor,
    now: Timestamp,
) -> Result<AccountStatus, BusinessesError> {
    if reason.code() == "self" {
        return Err(BusinessesError::Refused);
    }
    let mut tx = db(conn.begin().await)?;
    let current = lock_reason(&mut tx, id).await?;
    let from = models::read(&mut tx, id).await?.status;
    let event = Event::Pause(reason);
    let to = if let Some(rule) = replaces_self_pause(from, current, event) {
        replace_self_pause(&mut tx, id, rule, actor).await?;
        from
    } else {
        move_status(&mut tx, id, event, actor, now).await?
    };
    db(tx.commit().await)?;
    Ok(to)
}

/// The business pauses itself in the portal (BIZ-010): an active business becomes paused with the
/// reason `self`, the business as the audit actor.
///
/// # Errors
///
/// As [`apply`]: `Refused` unless active, `MovedOn`, `UnknownBusiness`, `Database` or `Audit`.
pub async fn pause_self(
    conn: &mut PgConnection,
    id: Uuid,
    now: Timestamp,
) -> Result<AccountStatus, BusinessesError> {
    let event = Event::Pause(PauseReason::SelfPaused);
    apply(conn, id, event, Actor::Business(id), now).await
}

/// The business resumes itself (BIZ-010), only from its own pause: the row is locked first, so a
/// pause for another reason can't slip in between the check and the move (BIZ-009, D-969).
///
/// # Errors
///
/// [`BusinessesError::Refused`] unless paused by itself; `UnknownBusiness`, `Database` or `Audit`.
pub async fn resume_self(
    conn: &mut PgConnection,
    id: Uuid,
    now: Timestamp,
) -> Result<AccountStatus, BusinessesError> {
    let mut tx = db(conn.begin().await)?;
    let reason = lock_reason(&mut tx, id).await?;
    if !rules::may_resume_itself(reason) {
        return Err(BusinessesError::Refused);
    }
    let to = move_status(&mut tx, id, Event::Resume, Actor::Business(id), now).await?;
    db(tx.commit().await)?;
    Ok(to)
}

/// Sets or, with `None`, clears the business's own quiet times; [`OwnHours`] already holds them
/// inside the app-wide window, and the table's check refuses anything wider (BIZ-010, D-085). A
/// removed business's times are never changed (removed is final, BIZ-009).
///
/// # Errors
///
/// [`BusinessesError::UnknownBusiness`] for no such business or a removed one; `Database` or
/// `Audit`.
pub async fn set_own_hours(
    conn: &mut PgConnection,
    id: Uuid,
    hours: Option<OwnHours>,
    actor: Actor,
) -> Result<(), BusinessesError> {
    let mut tx = db(conn.begin().await)?;
    let done = db(sqlx::query!(
        "UPDATE businesses.business SET own_open_minute = $2, own_close_minute = $3 \
         WHERE id = $1 AND status <> 'removed'",
        id,
        hours.map(OwnHours::open_minute),
        hours.map(OwnHours::close_minute)
    )
    .execute(&mut *tx)
    .await)?;
    if done.rows_affected() != 1 {
        return Err(BusinessesError::UnknownBusiness);
    }
    let change = Change::Field("own_hours");
    log(&mut tx, actor, "businesses.field_changed", id, change).await?;
    db(tx.commit().await)
}

/// The business's own quiet times, `None` when it set none, for the notice senders to pass to
/// [`crate::rules::may_notify`] (BIZ-010, D-085).
///
/// # Errors
///
/// [`BusinessesError::UnknownBusiness`] or `Database` (also for a stored window the rule refuses).
pub async fn own_hours(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<OwnHours>, BusinessesError> {
    let row = db(sqlx::query!(
        "SELECT own_open_minute, own_close_minute FROM businesses.business WHERE id = $1",
        id
    )
    .fetch_optional(conn)
    .await)?
    .ok_or(BusinessesError::UnknownBusiness)?;
    match (row.own_open_minute, row.own_close_minute) {
        (Some(o), Some(c)) => OwnHours::from_minutes(o, c)
            .map(Some)
            .map_err(|_| BusinessesError::Database),
        (None, None) => Ok(None),
        _ => Err(BusinessesError::Database),
    }
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
    let ticked_on = (ticks.overflow_on && !b.overflow_on) || (ticks.after_hours && !b.after_hours);
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
    if ticked_on && b.status.receives_cards() {
        queue_eligible(&mut tx, id, Timestamp::now()).await?;
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
    let status = models::read(&mut tx, id).await?.status;
    let before = coverage_keys(&mut tx, id).await?;
    let added = towns.iter().any(|t| !before.0.contains(t));
    models::replace_towns(&mut tx, id, &towns).await?;
    if status.counts_for_coverage() {
        report_edit(&mut tx, id, before).await?;
    }
    if added && status.receives_cards() {
        queue_eligible(&mut tx, id, Timestamp::now()).await?;
    }
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
    let status = models::read(&mut tx, id).await?.status;
    let before = coverage_keys(&mut tx, id).await?;
    let had = models::job_types(&mut tx, id).await?;
    let added = chosen.iter().any(|c| !had.contains(c));
    models::replace_job_types(&mut tx, id, &chosen).await?;
    if status.counts_for_coverage() {
        report_edit(&mut tx, id, before).await?;
    }
    if added && status.receives_cards() {
        queue_eligible(&mut tx, id, Timestamp::now()).await?;
    }
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
