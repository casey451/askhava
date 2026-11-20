//! The Confirm reminders' run (PICK-037, PICK-043, PICK-044, BIZ-037, QH-004; T682, D-4361,
//! D-5034).
//!
//! `handlers::notice_sent` calls [`remind`] at the pick notice's send, which queues one
//! [`CONFIRM_REMINDER_KIND`] task `{pick, start}` at the first reminder's time. Each run
//! ([`run_confirm_reminder_at`]) re-reads the pick and lists its reminders again from the window's
//! current end, so a HOLD or the bounce grace moves them (PICK-043). It sends nothing unless the
//! pick still waits, its job is still Awaiting confirm (not cancelled, closed or pressed) and the
//! window is open. The newest due reminder is claimed once (`claim_reminder`) and goes as one
//! `BUSINESS_NOTICE_KIND` task carrying `reminder_lines` as its own `subject` and `first_line`
//! over the pick notice's words. The run then queues itself at the next reminder, or at the
//! window's end, where an extended window is read again. A non-emergency reminder never goes
//! between 8 p.m. and 8 a.m.: the run waits for 8 a.m. An emergency pick's one reminder goes at
//! any hour: by day to every person who gets picks, at night only to the people
//! `emergency_pick_plan` names, and to nobody when there are none.

use askhava_businesses::api::{NoticePurpose, notice_addresses};
use askhava_core::api::{
    Claimed, HandlerError, NewTask, add_business_minutes, enqueue, is_business_hours,
};
use askhava_jobs::api::{
    JobRow, JobStatus, current, entered_payment_pending_since, job_number, rules::timeout,
};
use askhava_messaging::api::Language;
use jiff::Timestamp;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::cant_take::cant_take_link;
use crate::confirm::{PICK_NOTICE_EMERGENCY_TEMPLATE, PICK_NOTICE_TEMPLATE, WindowRules};
use crate::confirm_reminders::{
    Reminder, claim_reminder, emergency_reminders, reminder_lines, reminders, window_open,
};
use crate::emergency_emails::emergency_pick_plan;
use crate::handlers::{HoldRow, hold_row, id, second};
use crate::models::{self, PicksError};
use crate::money_notices::BUSINESS_NOTICE_KIND;
use crate::status::awaits_confirm;

/// The `core.task` kind of a waiting pick's reminder run (PICK-037).
pub const CONFIRM_REMINDER_KIND: &str = "picks.confirm_reminder";

/// The [`CONFIRM_REMINDER_KIND`] handler (PICK-037, PICK-043, PICK-044, JOB-006): [`remind`] for
/// the task's pick and window start at the worker's `now`, in one transaction; safe to run twice.
///
/// # Errors
/// A fixed sentence for a malformed task or a failed statement; the queue tries again.
pub async fn run_confirm_reminder_at(
    pool: PgPool,
    rules: WindowRules,
    task: Claimed,
    now: Timestamp,
) -> Result<(), HandlerError> {
    const FAILED: HandlerError = HandlerError("confirm reminder database statement failed");
    let (pick, start) = (id(&task.payload, "pick")?, second(&task.payload, "start")?);
    let mut tx = pool.begin().await.map_err(|_| FAILED)?;
    remind(&mut tx, pick, (start, now), rules)
        .await
        .map_err(|_| FAILED)?;
    tx.commit().await.map_err(|_| FAILED)
}

/// PICK-037, PICK-043, PICK-044, QH-004: one run for pick `pick`, whose window started at `start`,
/// at `now`, inside the caller's transaction: nothing unless the pick waits on a job still
/// Awaiting confirm that never entered Payment pending since `start`; otherwise the newest due
/// reminder goes once, while the window is open and (off an emergency job) it is 8 a.m. to
/// 8 p.m., and the next run is queued.
///
/// # Errors
/// [`PicksError::Job`] or [`PicksError::Database`].
pub(crate) async fn remind(
    tx: &mut PgConnection,
    pick: Uuid,
    (start, now): (Timestamp, Timestamp),
    rules: WindowRules,
) -> Result<(), PicksError> {
    if models::lock_waiting(tx, pick).await?.is_none() {
        return Ok(());
    }
    let Some(row) = hold_row(tx, pick).await? else {
        return Ok(());
    };
    let job = current(tx, row.job).await.map_err(|_| PicksError::Job)?;
    let pressed = entered_payment_pending_since(tx, row.job, start).await;
    if !awaits_confirm(job.status) || pressed.map_err(|_| PicksError::Job)? {
        return Ok(());
    }
    let Some((due, list)) = window(tx, (&row, &job), start, rules).await? else {
        return Ok(());
    };
    let awake = job.emergency || is_business_hours(now);
    let newest = list.iter().rev().find(|r| r.due(now));
    if let Some(&r) = newest.filter(|_| awake && window_open(now, due))
        && claim_reminder(tx, pick, r, now).await?
    {
        send(tx, (pick, &row, job.emergency), (r, due, now)).await?;
    }
    let next = list.iter().map(|r| r.at).find(|at| *at > now);
    let wake = match awake {
        true => next.or(Some(due)).filter(|at| *at > now),
        false => Some(add_business_minutes(now, 0)),
    };
    match wake {
        Some(at) => queue_run(tx, pick, (start, at)).await,
        None => Ok(()),
    }
}

/// PICK-037, PICK-043, PICK-044: the window's current end and its reminders: an emergency job's
/// 60 clock minutes of `jobs`' timer with the one reminder counted from `start`, else the stored
/// end (after any HOLD or grace) with the Hold line only while a HOLD is still allowed; none
/// before a non-emergency window has an end.
async fn window(
    tx: &mut PgConnection,
    (row, job): (&HoldRow, &JobRow),
    start: Timestamp,
    rules: WindowRules,
) -> Result<Option<(Timestamp, Vec<Reminder>)>, PicksError> {
    if job.emergency {
        let due = timeout(JobStatus::AwaitingConfirm, job.timer_from, true);
        let plan = emergency_pick_plan(tx, row.business, (&[], start), rules).await?;
        let list = emergency_reminders(&plan.plan);
        return Ok(Some((due.ok_or(PicksError::Job)?.at, list)));
    }
    let hold_offered = !row.held && !row.away_delayed;
    Ok(row
        .due
        .map(|due| (due, reminders(start, due, hold_offered, rules))))
}

/// PICK-037, PICK-044, BIZ-037, D-5034: queues reminder `r` of `pick` as one
/// `BUSINESS_NOTICE_KIND` task at `now`, to the owner and every "Confirm and billing" member; an
/// emergency one sent between 8 p.m. and 8 a.m. only to the people [`emergency_pick_plan`] names
/// then (switched on, at an after-hours business not suspended), and nothing to nobody. It carries `emergency: true` with its own `subject` and `first_line`
/// ([`reminder_lines`]; an emergency one is never "Last reminder", whose time left counts
/// business minutes), so `messaging` sends it when due; its `job` is the reminder's own ID (the
/// pick's ID mixed with the reminder's second), because `messaging` keys an email by template,
/// person and record IDs and every reminder of one pick would otherwise be one email.
async fn send(
    tx: &mut PgConnection,
    (pick, row, emergency): (Uuid, &HoldRow, bool),
    (r, due, now): (Reminder, Timestamp, Timestamp),
) -> Result<(), PicksError> {
    let day = notice_addresses(tx, row.business, NoticePurpose::PicksAndRelease).await;
    let day = day.map_err(|_| PicksError::Database)?;
    let (template, to, shown) = if emergency {
        let rules = WindowRules::default();
        let night = emergency_pick_plan(tx, row.business, (&[], now), rules).await?;
        let to = if is_business_hours(now) {
            day
        } else {
            night.to
        };
        (
            PICK_NOTICE_EMERGENCY_TEMPLATE,
            to,
            Reminder { last: false, ..r },
        )
    } else {
        (PICK_NOTICE_TEMPLATE, day, r)
    };
    if to.is_empty() {
        return Ok(());
    }
    let number = job_number(tx, row.job).await.map_err(|_| PicksError::Job)?;
    let short = u32::try_from(number).map_err(|_| PicksError::Job)?;
    let lines = reminder_lines(short, due, shown, Language::En);
    let at = r.at.as_second();
    let own_id = Uuid::from_u128(pick.as_u128() ^ u128::from(at.unsigned_abs()));
    let payload = json!({
        "template": template, "business": row.business, "pick": pick, "job": own_id, "to": to,
        "emergency": true, "subject": lines[0], "first_line": lines.join(" "),
        "values": {"job": number, "time": due.as_second(), "cant_take": cant_take_link(pick)},
    });
    let mut notice = NewTask::new(BUSINESS_NOTICE_KIND, payload, now);
    notice.dedupe_key = Some(format!(
        "{BUSINESS_NOTICE_KIND}.confirm_reminder.{pick}.{at}"
    ));
    let queued = enqueue(tx, &notice).await;
    queued.map_err(|_| PicksError::Database).map(drop)
}

/// One [`CONFIRM_REMINDER_KIND`] task for `pick` at `at`, carrying the window's `start`; deduped
/// by the pick and `at`.
async fn queue_run(
    tx: &mut PgConnection,
    pick: Uuid,
    (start, at): (Timestamp, Timestamp),
) -> Result<(), PicksError> {
    let payload = json!({"pick": pick, "start": start.as_second()});
    let mut task = NewTask::new(CONFIRM_REMINDER_KIND, payload, at);
    task.dedupe_key = Some(format!("{CONFIRM_REMINDER_KIND}.{pick}.{}", at.as_second()));
    let queued = enqueue(tx, &task).await;
    queued.map_err(|_| PicksError::Database).map(drop)
}
