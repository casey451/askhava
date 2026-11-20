//! A business's "Away until <time>" end (T543; BIZ-034, BIZ-038; D-285, D-309, D-3890).
//!
//! [`set_away`] stores the end of the business's away time in `businesses.business.away_until`
//! for the owner or a current "Confirm and billing" member, with one audit row naming the field,
//! in one transaction. The caller bounds the end first with `picks::api::away_until` (after now,
//! at most the setting's business hours ahead; `businesses` sits below `picks`, so it cannot call
//! it). [`away_until`] reads the end while it is still ahead; `picks::handlers::notice_sent` starts
//! a non-emergency pick's Confirm clock there.
//!
//! T463 (BIZ-028, BIZ-038, BIZ-010; D-309, D-3675, D-3890): [`set_busy`] is "Busy for <n> days"
//! (1 to 60). For the same people, in one transaction, it pauses job cards through `pause_self`
//! (so card routing already skips the business; a business that is not active is refused and
//! nothing is stored), stores the end in `businesses.business.busy_until` with its audit row and
//! queues one [`BUSY_END_KIND`] task at the end. The task's payload carries the pause's
//! `status_changed_at`; [`run_busy_end`] resumes through `resume_self` only while the business is
//! still self-paused with that same time and end, so a Resume, a later pause or a rule's pause
//! first leaves it alone (`resume_self` does not clear `busy_until`, so the end alone could match).

use askhava_core::api::{Claimed, HandlerError, NewTask, enqueue};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::models::{BusinessesError, db};
use crate::service::{pause_self, resume_self};
use crate::team::Acting;

/// The task that ends a "Busy for <n> days" pause: payload `{"business", "paused_at", "until"}`
/// (IDs and times only), due at the end (BIZ-028).
pub const BUSY_END_KIND: &str = "businesses.busy_end";

/// BIZ-028: the longest "Busy for <n> days" setting, in days.
const BUSY_DAYS_MAX: u32 = 60;

/// The audit row's subject kind (the business record).
const SUBJECT: &str = "businesses.business";

/// BIZ-034, D-285: stores `until` as `by`'s business's away end at `now`, with one audit row, when
/// `by` is a current owner or "Confirm and billing" member. `until` must already be bounded by
/// `picks::api::away_until`; an end at or before `now` is refused here too.
///
/// # Errors
/// [`BusinessesError::Refused`] for an end not after `now`, a removed member, a member without
/// "Confirm and billing" or one of another business (nothing stored); `Database` or `Audit`.
pub async fn set_away(
    conn: &mut PgConnection,
    by: Acting,
    until: Timestamp,
    now: Timestamp,
) -> Result<(), BusinessesError> {
    if until <= now {
        return Err(BusinessesError::Refused);
    }
    let mut tx = db(conn.begin().await)?;
    let stored = sqlx::query!(
        "UPDATE businesses.business b SET away_until = $3 \
         WHERE b.id = $1 AND EXISTS (SELECT 1 FROM businesses.team_member m \
         WHERE m.id = $2 AND m.business_id = b.id AND m.removed_at IS NULL \
         AND (m.is_owner OR m.confirm_and_billing))",
        by.business,
        by.member,
        until.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await;
    if db(stored)?.rows_affected() != 1 {
        return Err(BusinessesError::Refused);
    }
    field_audit(&mut tx, by.business, "away_until").await?;
    db(tx.commit().await)
}

/// JOB-005: one audit row naming `field` of `business`, the business as the actor.
async fn field_audit(
    tx: &mut PgConnection,
    business: Uuid,
    field: &'static str,
) -> Result<(), BusinessesError> {
    let entry = AuditEntry {
        actor: Actor::Business(business),
        action: "businesses.field_changed",
        subject_kind: SUBJECT,
        subject_id: business,
        change: Change::Field(field),
    };
    audit(tx, &entry)
        .await
        .map(|_| ())
        .map_err(|_| BusinessesError::Audit)
}

/// BIZ-028, BIZ-010, D-309, D-3890: "Busy for `days` days" set by `by` at `now`: job cards pause
/// through `pause_self`, the end (`days` × 24 hours on, the same Arizona date as
/// `portals::api::busy_until_line`) is stored in `busy_until` with one audit row, and one
/// [`BUSY_END_KIND`] task is queued at the end; all in one transaction. Returns the stored end.
///
/// # Errors
/// [`BusinessesError::Refused`] for `days` outside 1 to 60, a removed member, a member without
/// "Confirm and billing" or one of another business, or a business that is not active (already
/// paused by itself or a rule; nothing stored); `MovedOn`, `UnknownBusiness`, `Database`, `Audit`.
pub async fn set_busy(
    conn: &mut PgConnection,
    by: Acting,
    days: u32,
    now: Timestamp,
) -> Result<Timestamp, BusinessesError> {
    if !(1..=BUSY_DAYS_MAX).contains(&days) {
        return Err(BusinessesError::Refused);
    }
    let span = SignedDuration::from_hours(i64::from(days) * 24);
    let until = now
        .checked_add(span)
        .map_err(|_| BusinessesError::Refused)?;
    let mut tx = db(conn.begin().await)?;
    may_set(&mut tx, by).await?;
    pause_self(&mut tx, by.business, now).await?;
    let stored = sqlx::query!(
        r#"UPDATE businesses.business SET busy_until = $2 WHERE id = $1
           RETURNING status_changed_at, busy_until AS "busy_until!""#,
        by.business,
        until.to_sqlx() as _,
    )
    .fetch_one(&mut *tx)
    .await;
    let stored = db(stored)?;
    let (paused_at, until) = (
        stored.status_changed_at.to_jiff(),
        stored.busy_until.to_jiff(),
    );
    let payload = json!({
        "business": by.business,
        "paused_at": paused_at.to_string(),
        "until": until.to_string(),
    });
    let mut task = NewTask::new(BUSY_END_KIND, payload, until);
    task.dedupe_key = Some(format!("{BUSY_END_KIND}:{}:{paused_at}", by.business));
    enqueue(&mut tx, &task)
        .await
        .map_err(|_| BusinessesError::Database)?;
    field_audit(&mut tx, by.business, "busy_until").await?;
    db(tx.commit().await)?;
    Ok(until)
}

/// BIZ-028: `Refused` unless `by` is a current owner or "Confirm and billing" member of its
/// business.
async fn may_set(tx: &mut PgConnection, by: Acting) -> Result<(), BusinessesError> {
    let may = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM businesses.team_member WHERE id = $2
           AND business_id = $1 AND removed_at IS NULL
           AND (is_owner OR confirm_and_billing)) AS "may!""#,
        by.business,
        by.member,
    )
    .fetch_one(&mut *tx)
    .await;
    if db(may)? {
        Ok(())
    } else {
        Err(BusinessesError::Refused)
    }
}

/// The [`BUSY_END_KIND`] handler at `now` (BIZ-028, D-3890): resumes the business through
/// `resume_self` only while it is still self-paused with the payload's pause time and end;
/// otherwise (a Resume, a later pause, a rule's pause, no such business) it changes nothing.
///
/// # Errors
/// A fixed [`HandlerError`] for a malformed payload or a failed database call.
pub async fn run_busy_end(pool: PgPool, task: Claimed, now: Timestamp) -> Result<(), HandlerError> {
    let field = |name: &str| task.payload[name].as_str().map(str::to_owned);
    let business = field("business").and_then(|s| Uuid::parse_str(&s).ok());
    let paused_at = field("paused_at").and_then(|s| s.parse::<Timestamp>().ok());
    let until = field("until").and_then(|s| s.parse::<Timestamp>().ok());
    let (Some(business), Some(paused_at), Some(until)) = (business, paused_at, until) else {
        return Err(HandlerError("busy end payload is malformed"));
    };
    let failed = HandlerError("busy end database call failed");
    let mut conn = pool.acquire().await.map_err(|_| failed)?;
    end_busy(&mut conn, business, (paused_at, until), now)
        .await
        .map_err(|_| failed)
}

/// BIZ-028, BIZ-009, D-3890: locks the business row and resumes it at `now` only while it is
/// paused with the reason `self`, `status_changed_at` = `paused_at` and `busy_until` = `until`.
async fn end_busy(
    conn: &mut PgConnection,
    business: Uuid,
    (paused_at, until): (Timestamp, Timestamp),
    now: Timestamp,
) -> Result<(), BusinessesError> {
    let mut tx = db(conn.begin().await)?;
    let still = sqlx::query_scalar!(
        r#"SELECT COALESCE(status = 'paused' AND pause_reason = 'self'
             AND status_changed_at = $2 AND busy_until = $3, false) AS "still!"
           FROM businesses.business WHERE id = $1 FOR UPDATE"#,
        business,
        paused_at.to_sqlx() as _,
        until.to_sqlx() as _,
    )
    .fetch_optional(&mut *tx)
    .await;
    if db(still)? == Some(true) {
        resume_self(&mut tx, business, now).await?;
    }
    db(tx.commit().await)
}

/// BIZ-034: business `business`'s away end while it is after `now`; none when unset, past or no
/// such business.
///
/// # Errors
/// [`BusinessesError::Database`].
pub async fn away_until(
    conn: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<Option<Timestamp>, BusinessesError> {
    let until = sqlx::query_scalar!(
        "SELECT away_until FROM businesses.business WHERE id = $1 AND away_until > $2",
        business,
        now.to_sqlx() as _,
    )
    .fetch_optional(conn)
    .await;
    Ok(db(until)?.flatten().map(|t| t.to_jiff()))
}
