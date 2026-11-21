//! The unknown-license clock: warn, pause and resume on the ROC list (T655, D-4310, D-941;
//! LIC-008, CARD-015, OPS-030, BIZ-018, D-199).
//!
//! [`STANDING_KIND`] is one task chain: [`schedule_standing`] queues a run [`LAG`] after the next
//! slot of the daily list grid (the run hour and each LIC-039 retry slot, `licensing`'s
//! `next_slot`), keyed by that slot, and [`run_standing`] queues the next one first. Each run
//! ([`apply_standing`]) does two things.
//!
//! First, every active business whose latest ROC check is Active and not after the list's failure
//! start, once the clock reaches that start, stands where [`standing_at`] says: the pause moment
//! is the check's `checked_at + pause_after`, the instant the card pool stops its ROC cards
//! (`latest_check_allows_card`, T654), and the warning moment is the earlier of
//! `failure start + warn_after` and the pause moment. From the warning moment one
//! [`LICENSE_UNKNOWN_KIND`] email is queued per spell (keyed by the business and the failure
//! start) with a [`WARNED_ACTION`] audit row; from the pause moment the business is paused for
//! `license` through `pause_for_rule` (system actor) in the same transaction as that email, so
//! there is never a pause without a warning. A known Inactive or Not-listed check is not paused
//! here: its ROC cards stop and Confirm refuses it (PICK-020).
//!
//! Second, every business paused for `license` gets `recheck_paused` (a database read of the
//! latest recorded list; the list run checks only active businesses) and resumes with
//! `Event::Resume` and no review when its latest check is Active and later than the pause and no
//! pest report holds it; three complaints in the window keep it paused for `complaints`, and a
//! pause that began as the owner's own returns to `self` (`lift_no_cover`'s order). This is also
//! the way out of `lift_no_cover`'s `license` hold. Every pause and resume is a system-actor
//! status audit row, which the weekly digest counts (OPS-030, D-3882).

use askhava_core::api::{HandlerError, NewTask, enqueue};
use askhava_licensing::api::{
    DailyListSettings, FailureLimits, RocStanding, UnknownStanding, next_slot, recheck_paused,
    roc_standing, unknown_standing,
};
use askhava_ops::api::{
    Actor, AuditEntry, Change, audit, field_changed_last, system_status_changes,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::holding::{Known, holding};
use crate::license_reapply::{complaints_hold, pest_reported};
use crate::models::{BusinessesError, db};
use crate::rules::Event;
use crate::service::{apply, pause_for_rule};
use crate::status::{AccountStatus, PauseReason};

/// The `core.task` kind of the unknown-license run (LIC-008).
pub const STANDING_KIND: &str = "businesses.license_standing";
/// LIC-008: the email task queued once per unknown spell; its payload holds `business`, `since`
/// and `pause_at` (Unix seconds) and `paused`. The sender renders `license_unknown_words`.
pub const LICENSE_UNKNOWN_KIND: &str = "businesses.license_unknown";
/// The audit action written with each queued [`LICENSE_UNKNOWN_KIND`] email.
pub const WARNED_ACTION: &str = "businesses.license_unknown_warned";
/// How long after a list slot the run is due, so the list run at that slot goes first.
pub const LAG: SignedDuration = SignedDuration::from_mins(5);
/// How far an audit row's time may lie from the pause it records (the worker and database clocks).
const SKEW: SignedDuration = SignedDuration::from_hours(1);
const SUBJECT: &str = "businesses.business";
const FAILED: HandlerError = HandlerError("the license standing database call failed");

/// LIC-008, JOB-006: queues the run due [`LAG`] after the first list slot after `now`, once per
/// slot (dedupe key by its instant).
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn schedule_standing(
    conn: &mut PgConnection,
    now: Timestamp,
    lists: DailyListSettings,
) -> Result<(), BusinessesError> {
    let slot = next_slot(now, lists.run_hour, lists.retry_every);
    let slot = slot.ok_or(BusinessesError::Database)?;
    let due = slot.checked_add(LAG);
    let due = due.map_err(|_| BusinessesError::Database)?;
    let mut task = NewTask::new(STANDING_KIND, json!({}), due);
    task.dedupe_key = Some(format!("{STANDING_KIND}.{}", slot.as_second()));
    enqueue(conn, &task)
        .await
        .map(|_| ())
        .map_err(|_| BusinessesError::Database)
}

/// The [`STANDING_KIND`] handler (LIC-008, JOB-006): queues the next run first, then
/// [`apply_standing`] at `now`.
///
/// # Errors
///
/// A fixed [`HandlerError`] on any failure; the task retries and warns or pauses nothing twice.
pub async fn run_standing(
    pool: &PgPool,
    now: Timestamp,
    lists: DailyListSettings,
) -> Result<(), HandlerError> {
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    let queued = schedule_standing(&mut conn, now, lists).await;
    queued.map_err(|_| FAILED)?;
    let applied = apply_standing(&mut conn, now, lists).await;
    applied.map_err(|_| FAILED)
}

/// LIC-008, D-4310: warns or pauses every active business whose ROC status is unknown at `now`,
/// then checks and resumes every business paused for `license` (the module doc has the rules).
///
/// # Errors
///
/// `Database`, `Audit` or `MovedOn`; what was done for earlier businesses stays.
pub async fn apply_standing(
    conn: &mut PgConnection,
    now: Timestamp,
    lists: DailyListSettings,
) -> Result<(), BusinessesError> {
    for id in in_status(conn, AccountStatus::Active, None).await? {
        stand(conn, id, now, lists).await?;
    }
    let reason = Some(PauseReason::License);
    for id in in_status(conn, AccountStatus::Paused, reason).await? {
        lift(conn, id, now, lists.run_hour).await?;
    }
    Ok(())
}

/// LIC-008, D-4310, D-199: where an unknown ROC status stands at `now`, with the failure start
/// and the pause moment; `None` unless `check` is Active, not after the list's failure start, and
/// `now` has reached that start. The pause moment is `checked_at + pause_after` (never more than
/// `pause_after` after the status became unknown); the warning moment is the earlier of
/// `failure start + warn_after` and the pause moment, which the pause, read first, gives with no
/// cap written here. `unknown_standing` from the failure start is the answer, brought forward to
/// those two moments.
#[must_use]
pub fn standing_at(
    check: RocStanding,
    now: Timestamp,
    limits: FailureLimits,
) -> Option<(UnknownStanding, Timestamp, Timestamp)> {
    let since = check.failing_since.filter(|s| check.active && now >= *s)?;
    let pause_at = check.checked_at.checked_add(limits.pause_after).ok()?;
    let warn_at = since.checked_add(limits.warn_after).ok()?;
    let by_list = unknown_standing(since, now, limits);
    let standing = if now >= pause_at || by_list == UnknownStanding::Paused {
        UnknownStanding::Paused
    } else if now >= warn_at || by_list == UnknownStanding::Warned {
        UnknownStanding::Warned
    } else {
        UnknownStanding::Kept
    };
    Some((standing, since, pause_at))
}

/// The businesses in `status`, with `reason` when one is given, by ID.
async fn in_status(
    conn: &mut PgConnection,
    status: AccountStatus,
    reason: Option<PauseReason>,
) -> Result<Vec<Uuid>, BusinessesError> {
    let ids = sqlx::query_scalar!(
        "SELECT id FROM businesses.business WHERE status = $1 \
         AND ($2::text IS NULL OR pause_reason = $2) ORDER BY id",
        status.code(),
        reason.map(PauseReason::code),
    );
    db(ids.fetch_all(&mut *conn).await)
}

/// LIC-008: warns or pauses active business `id` by [`standing_at`], in one transaction. A
/// business that moved on meanwhile is left as it is, with nothing queued.
async fn stand(
    conn: &mut PgConnection,
    id: Uuid,
    now: Timestamp,
    lists: DailyListSettings,
) -> Result<(), BusinessesError> {
    let read = roc_standing(conn, id, lists.run_hour).await;
    let check = read.map_err(|_| BusinessesError::Database)?;
    let stands = check.and_then(|c| standing_at(c, now, lists.limits));
    let (paused, spell) = match stands {
        Some((UnknownStanding::Warned, since, pause_at)) => (false, (since, pause_at)),
        Some((UnknownStanding::Paused, since, pause_at)) => (true, (since, pause_at)),
        Some((UnknownStanding::Kept, ..)) | None => return Ok(()),
    };
    let mut tx = db(conn.begin().await)?;
    notify(&mut tx, id, spell, paused, now).await?;
    if paused {
        let reason = PauseReason::License;
        match pause_for_rule(&mut tx, id, reason, Actor::System, now).await {
            Ok(_) => {}
            Err(BusinessesError::Refused | BusinessesError::MovedOn) => return Ok(()),
            Err(e) => return Err(e),
        }
    }
    db(tx.commit().await)
}

/// LIC-008, BIZ-018: queues the owner's [`LICENSE_UNKNOWN_KIND`] email once per spell (the
/// business and the failure start; no contact value or license number in the payload), with its
/// [`WARNED_ACTION`] audit row when it is new.
async fn notify(
    tx: &mut PgConnection,
    id: Uuid,
    (since, pause_at): (Timestamp, Timestamp),
    paused: bool,
    now: Timestamp,
) -> Result<(), BusinessesError> {
    let payload = json!({
        "business": id,
        "since": since.as_second(),
        "pause_at": pause_at.as_second(),
        "paused": paused,
    });
    let mut task = NewTask::new(LICENSE_UNKNOWN_KIND, payload, now);
    task.dedupe_key = Some(format!("{LICENSE_UNKNOWN_KIND}.{id}.{}", since.as_second()));
    let queued = enqueue(tx, &task).await;
    if queued.map_err(|_| BusinessesError::Database)?.is_none() {
        return Ok(());
    }
    let entry = AuditEntry {
        actor: Actor::System,
        action: WARNED_ACTION,
        subject_kind: SUBJECT,
        subject_id: id,
        change: Change::Action,
    };
    let logged = audit(tx, &entry).await;
    logged.map(|_| ()).map_err(|_| BusinessesError::Audit)
}

/// LIC-008, D-4310: records the daily check of `license`-paused business `id` against the latest
/// list, then lifts the pause with no review when its latest check is Active and later than the
/// pause and no pest report holds it (LIC-047): paused for `complaints` when that rule holds
/// (BIZ-012), back to its own pause when it began as one (D-969), otherwise resumed.
async fn lift(
    conn: &mut PgConnection,
    id: Uuid,
    now: Timestamp,
    run_hour: i8,
) -> Result<(), BusinessesError> {
    let checked = recheck_paused(conn, id).await;
    checked.map_err(|_| BusinessesError::Database)?;
    let read = roc_standing(conn, id, run_hour).await;
    let Some(check) = read.map_err(|_| BusinessesError::Database)? else {
        return Ok(());
    };
    let mut tx = db(conn.begin().await)?;
    let Some(paused_at) = license_paused_at(&mut tx, id).await? else {
        return Ok(());
    };
    let cleared = check.active && check.checked_at > paused_at;
    if !cleared || pest_reported(&holding(&mut tx, id, Known::default()).await?) {
        return Ok(());
    }
    if complaints_hold(&mut tx, id, now).await? {
        set_reason(&mut tx, id, PauseReason::Complaints).await?;
    } else if own_pause_under(&mut tx, id, paused_at).await? {
        set_reason(&mut tx, id, PauseReason::SelfPaused).await?;
    } else {
        apply(&mut tx, id, Event::Resume, Actor::System, now).await?;
    }
    db(tx.commit().await)
}

/// When business `id` was paused, if it is paused for `license` now (locked for the transaction).
async fn license_paused_at(
    tx: &mut PgConnection,
    id: Uuid,
) -> Result<Option<Timestamp>, BusinessesError> {
    let row = sqlx::query_scalar!(
        r#"SELECT status_changed_at AS "at: jiff_sqlx::Timestamp" FROM businesses.business
           WHERE id = $1 AND status = 'paused' AND pause_reason = 'license' FOR UPDATE"#,
        id
    );
    Ok(db(row.fetch_optional(&mut *tx).await)?.map(|at| at.to_jiff()))
}

/// D-969, D-970, D-5185: the `license` pause lies over the owner's own pause: the business's
/// latest status or pause-reason audit row is a `pause_reason` field change (a rule's reason put
/// over another) and no system-actor status row paused it within [`SKEW`] of `paused_at`, so the
/// pause it began as was not a rule's (a `no_covered_job_type` pause from active that
/// `lift_no_cover` then held for `license` is a rule's, and resumes).
async fn own_pause_under(
    tx: &mut PgConnection,
    id: Uuid,
    paused_at: Timestamp,
) -> Result<bool, BusinessesError> {
    let last = field_changed_last(tx, (SUBJECT, id), "pause_reason").await;
    if !last.map_err(|_| BusinessesError::Database)? {
        return Ok(false);
    }
    let (Ok(from), Ok(to)) = (paused_at.checked_sub(SKEW), paused_at.checked_add(SKEW)) else {
        return Err(BusinessesError::Database);
    };
    let rows = system_status_changes(tx, SUBJECT, from, to).await;
    let rows = rows.map_err(|_| BusinessesError::Database)?;
    let by_rule = |r: &askhava_ops::api::StatusChangeRow| {
        r.subject_id == id && r.to_status.as_deref() == Some("paused")
    };
    Ok(!rows.iter().any(by_rule))
}

/// Replaces the `license` reason with `reason`, the business staying paused, with one
/// `pause_reason` field audit row by the system (BIZ-009, PRIV-028).
async fn set_reason(
    tx: &mut PgConnection,
    id: Uuid,
    reason: PauseReason,
) -> Result<(), BusinessesError> {
    let done = db(sqlx::query!(
        "UPDATE businesses.business SET pause_reason = $2 \
         WHERE id = $1 AND status = 'paused' AND pause_reason = 'license'",
        id,
        reason.code()
    )
    .execute(&mut *tx)
    .await)?;
    if done.rows_affected() != 1 {
        return Err(BusinessesError::MovedOn);
    }
    let entry = AuditEntry {
        actor: Actor::System,
        action: "businesses.field_changed",
        subject_kind: SUBJECT,
        subject_id: id,
        change: Change::Field("pause_reason"),
    };
    let logged = audit(tx, &entry).await;
    logged.map(|_| ()).map_err(|_| BusinessesError::Audit)
}
