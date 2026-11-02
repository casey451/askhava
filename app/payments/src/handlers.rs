//! What each charge answer starts, the payment timers and the `payments` events (FEE-016,
//! FEE-018, FEE-019, FEE-020, FEE-037, FEE-038, FEE-041, D-135, D-153, D-270, D-491).
//!
//! [`record`] stores an attempt's answer with its audit row and, in that transaction: a decline
//! or bank approval opens the card-fix window (1 business hour, 60 clock minutes on an emergency
//! job, PICK-044) and queues [`CARD_FIX_KIND`], which `picks::money_notices` turns into the
//! business's notice (FEE-020, D-492); no answer from Stripe starts the 4-hour release (FEE-018);
//! an undecided attempt queues [`CHECK_KIND`] for 30 minutes later (FEE-019). A timer is a
//! `payments.pay_timer` row plus a [`TIMER_KIND`] task at its due time; [`run_pay_timer`] asks
//! Stripe once more and, unless the pick is paid, ends it with [`PICK_ENDED_KIND`] for `picks` to
//! offer the next quote. [`sync_pause`] stops every running timer while safe mode or the charging
//! switch pauses charges and resumes each with the time it had left (FEE-038). `payments` calls
//! neither `messaging` nor `picks`: every event is a `core.task` of IDs and times (D-491).

use askhava_adapters::api::PaymentsAdapter;
use askhava_core::api::{HandlerError, NewTask, add_business_minutes, enqueue, is_business_hours};
use askhava_ops::api::{Actor, AuditEntry, Change, Switch, audit, is_paused};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::charge::{ATTEMPT_SUBJECT, ask_stripe, latest};
use crate::models::ChargeError;
use crate::status::AttemptStatus;

/// The `core.task` kind of the 30-minute "ask Stripe directly" check (FEE-019).
pub const CHECK_KIND: &str = "payments.charge_check";
/// The `core.task` kind of a payment timer's due time (FEE-018, FEE-020).
pub const TIMER_KIND: &str = "payments.pay_timer";
/// The event a decline or bank approval raises: `{pick, business, job, fix_by}` (FEE-020).
pub const CARD_FIX_KIND: &str = "payments.card_needs_fix";
/// The event of a pick `payments` ended: `{pick, business, job, reason}` (FEE-018, FEE-020,
/// FEE-041); `picks` offers the homeowner the next quote.
pub const PICK_ENDED_KIND: &str = "payments.pick_ended";
/// How long after an undecided attempt the app asks Stripe directly (FEE-019).
pub const CHECK_AFTER: SignedDuration = SignedDuration::from_mins(30);
/// The card-fix window: 1 business hour, or 60 clock minutes on an emergency job (FEE-020).
pub const CARD_FIX_SECONDS: i64 = 60 * 60;
/// How long Stripe may be unreachable before the pick is released (FEE-018).
pub const STRIPE_DOWN_SECONDS: i64 = 4 * 60 * 60;

/// Why `payments` ended a pick, as [`PICK_ENDED_KIND`] carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// Stripe unreachable for 4 hours (FEE-018).
    StripeDown,
    /// The card-fix window ran out (FEE-020).
    CardFix,
    /// The press would have gone over the monthly fee limit (FEE-041).
    FeeLimit,
}

impl EndReason {
    /// The code name in the event.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::StripeDown => "stripe-down",
            Self::CardFix => "card-fix",
            Self::FeeLimit => "fee-limit",
        }
    }
}

const CARD_FIX: &str = "card-fix";
const STRIPE_DOWN: &str = "stripe-down";

fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, ChargeError> {
    result.map_err(|_| ChargeError::Database)
}

/// True while safe mode or the owner's charging switch pauses charges (OPS-024, FEE-038). Safe
/// mode is the `all` switch until OPS-001's own flag exists (D-1427).
///
/// # Errors
/// [`ChargeError::Database`].
pub async fn charges_paused(conn: &mut PgConnection) -> Result<bool, ChargeError> {
    for switch in [Switch::Charging, Switch::All] {
        if is_paused(conn, switch)
            .await
            .map_err(|_| ChargeError::Database)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Stores Stripe's answer `status` on attempt `try_no` if it may still change, with its audit
/// row and the effects in the module docs, in one transaction; returns the status now stored.
pub(crate) async fn record(
    pool: &PgPool,
    pick: Uuid,
    try_no: i32,
    status: AttemptStatus,
    at: Timestamp,
) -> Result<AttemptStatus, ChargeError> {
    let mut tx = db(pool.begin().await)?;
    let from = db(sqlx::query_scalar!(
        "SELECT status FROM payments.charge_attempt WHERE pick_id = $1 AND try_no = $2 FOR UPDATE",
        pick,
        try_no
    )
    .fetch_one(&mut *tx)
    .await)?;
    let from = AttemptStatus::from_code(&from).ok_or(ChargeError::Database)?;
    if !from.may_change() {
        return Ok(from);
    }
    let decided = (!status.undecided()).then(|| at.to_sqlx());
    db(sqlx::query!(
        "UPDATE payments.charge_attempt SET status = $3, decided_at = $4
         WHERE pick_id = $1 AND try_no = $2",
        pick,
        try_no,
        status.code(),
        decided as _,
    )
    .execute(&mut *tx)
    .await)?;
    let entry = AuditEntry::status_change(
        Actor::System,
        (ATTEMPT_SUBJECT, pick),
        from.code(),
        status.code(),
    );
    audit(&mut tx, &entry)
        .await
        .map_err(|_| ChargeError::Database)?;
    effects(&mut tx, (pick, try_no), status, at).await?;
    db(tx.commit().await)?;
    Ok(status)
}

/// The timers and events an answer starts (FEE-018, FEE-019, FEE-020).
async fn effects(
    tx: &mut PgConnection,
    (pick, try_no): (Uuid, i32),
    status: AttemptStatus,
    at: Timestamp,
) -> Result<(), ChargeError> {
    if status.paid() {
        end_timers(tx, pick, None, at).await?;
    } else if status.stripe_answered() {
        end_timers(tx, pick, Some(STRIPE_DOWN), at).await?;
    } else {
        start_timer(tx, pick, (STRIPE_DOWN, false), STRIPE_DOWN_SECONDS, at).await?;
    }
    if status.needs_card_fix() {
        let row = db(sqlx::query!(
            "SELECT business_id, job_id, emergency FROM payments.pick_intent WHERE pick_id = $1",
            pick
        )
        .fetch_one(&mut *tx)
        .await)?;
        let timer = (CARD_FIX, !row.emergency);
        let fix_by = start_timer(tx, pick, timer, CARD_FIX_SECONDS, at).await?;
        let payload = json!({"pick": pick, "business": row.business_id, "job": row.job_id,
            "fix_by": fix_by.map(Timestamp::as_second)});
        queue(tx, CARD_FIX_KIND, payload, at, format!("{pick}.{try_no}")).await?;
    }
    if status.undecided() {
        let payload = json!({"pick": pick, "try": try_no});
        let check_at = at.saturating_add(CHECK_AFTER).unwrap_or(at);
        queue(
            tx,
            CHECK_KIND,
            payload,
            check_at,
            format!("{pick}.{try_no}"),
        )
        .await?;
    }
    Ok(())
}

async fn queue(
    tx: &mut PgConnection,
    kind: &'static str,
    payload: Value,
    at: Timestamp,
    dedupe: String,
) -> Result<(), ChargeError> {
    let mut task = NewTask::new(kind, payload, at);
    task.dedupe_key = Some(format!("{kind}.{dedupe}"));
    enqueue(tx, &task)
        .await
        .map_err(|_| ChargeError::Database)?;
    Ok(())
}

/// The instant `seconds` of timer time after `at`: business time 8 a.m. to 8 p.m. Arizona
/// (JOB-007), whole minutes rounded up, or clock time.
pub(crate) fn due_after(at: Timestamp, business_clock: bool, seconds: i64) -> Timestamp {
    if business_clock {
        let minutes = u32::try_from((seconds + 59) / 60).unwrap_or(u32::MAX);
        return add_business_minutes(at, minutes);
    }
    at.saturating_add(SignedDuration::from_secs(seconds))
        .unwrap_or(Timestamp::MAX)
}

/// The timer time left from `now` until `due` (FEE-038): business minutes counted one by one
/// (a card-fix window is at most a night away), or clock seconds.
pub(crate) fn time_left(now: Timestamp, due: Timestamp, business_clock: bool) -> i64 {
    if !business_clock {
        return (due.as_second() - now.as_second()).max(0);
    }
    let mut minute = now.as_second();
    let mut left = 0;
    while minute < due.as_second() {
        if Timestamp::from_second(minute).is_ok_and(is_business_hours) {
            left += 60;
        }
        minute += 60;
    }
    left
}

/// Starts timer `kind` for the pick unless one runs (a timer that ended starts again), paused at
/// once while charges are paused (FEE-038); returns the running timer's due time.
async fn start_timer(
    tx: &mut PgConnection,
    pick: Uuid,
    (kind, business_clock): (&str, bool),
    seconds: i64,
    at: Timestamp,
) -> Result<Option<Timestamp>, ChargeError> {
    let paused = charges_paused(tx).await?;
    let due = (!paused).then(|| due_after(at, business_clock, seconds));
    let started = db(sqlx::query_scalar!(
        "INSERT INTO payments.pay_timer (pick_id, kind, business_clock, due_at, left_seconds)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (pick_id, kind) DO UPDATE SET due_at = EXCLUDED.due_at,
           left_seconds = EXCLUDED.left_seconds, ended_at = NULL
           WHERE payments.pay_timer.ended_at IS NOT NULL
         RETURNING due_at",
        pick,
        kind,
        business_clock,
        due.map(|d| d.to_sqlx()) as _,
        paused.then_some(seconds),
    )
    .fetch_optional(&mut *tx)
    .await)?;
    if let Some(Some(due)) = started {
        queue_timer(tx, pick, kind, due.to_jiff()).await?;
    }
    let running = db(sqlx::query_scalar!(
        "SELECT due_at FROM payments.pay_timer WHERE pick_id = $1 AND kind = $2",
        pick,
        kind
    )
    .fetch_one(&mut *tx)
    .await)?;
    Ok(running.map(|d| d.to_jiff()))
}

async fn queue_timer(
    tx: &mut PgConnection,
    pick: Uuid,
    kind: &str,
    due: Timestamp,
) -> Result<(), ChargeError> {
    let payload = json!({"pick": pick, "kind": kind, "due": due.as_second()});
    let dedupe = format!("{pick}.{kind}.{}", due.as_second());
    queue(tx, TIMER_KIND, payload, due, dedupe).await
}

async fn end_timers(
    tx: &mut PgConnection,
    pick: Uuid,
    kind: Option<&str>,
    at: Timestamp,
) -> Result<(), ChargeError> {
    db(sqlx::query!(
        "UPDATE payments.pay_timer SET ended_at = $3
         WHERE pick_id = $1 AND ended_at IS NULL AND ($2::text IS NULL OR kind = $2)",
        pick,
        kind,
        at.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await)?;
    Ok(())
}

/// Ends the pick once, with its timers, its audit row and [`PICK_ENDED_KIND`] (FEE-018, FEE-020,
/// FEE-041); a pick already ended changes nothing.
pub(crate) async fn end_pick(
    tx: &mut PgConnection,
    pick: Uuid,
    reason: EndReason,
    at: Timestamp,
) -> Result<(), ChargeError> {
    let ended = db(sqlx::query!(
        "UPDATE payments.pick_intent SET ended_at = $2 WHERE pick_id = $1 AND ended_at IS NULL
         RETURNING business_id, job_id",
        pick,
        at.to_sqlx() as _,
    )
    .fetch_optional(&mut *tx)
    .await)?;
    let Some(row) = ended else {
        return Ok(());
    };
    end_timers(tx, pick, None, at).await?;
    let entry = AuditEntry {
        actor: Actor::System,
        action: PICK_ENDED_KIND,
        subject_kind: "payments.pick_intent",
        subject_id: pick,
        change: Change::Action,
    };
    audit(tx, &entry).await.map_err(|_| ChargeError::Database)?;
    let payload = json!({"pick": pick, "business": row.business_id, "job": row.job_id,
        "reason": reason.code()});
    queue(tx, PICK_ENDED_KIND, payload, at, pick.to_string()).await
}

fn pick_of(payload: &Value) -> Result<Uuid, HandlerError> {
    payload["pick"]
        .as_str()
        .and_then(|p| Uuid::parse_str(p).ok())
        .ok_or(HandlerError("payments task has no pick"))
}

const DB_FAILED: HandlerError = HandlerError("payments database statement failed");

/// The [`CHECK_KIND`] handler (FEE-019, FEE-037): if the pick's latest attempt is still
/// undecided 30 minutes on, ask Stripe directly; safe to run twice (JOB-006).
///
/// # Errors
/// A fixed sentence when Stripe cannot be reached (the queue asks again later) or the database
/// fails.
pub async fn run_charge_check(
    pool: &PgPool,
    stripe: &impl PaymentsAdapter,
    payload: &Value,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let pick = pick_of(payload)?;
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    let open = latest(&mut conn, pick).await.map_err(|_| DB_FAILED)?;
    drop(conn);
    if !open.is_some_and(|(_, status)| status.undecided()) {
        return Ok(());
    }
    match ask_stripe(pool, stripe, pick, now).await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(HandlerError("Stripe was not reached")),
        Err(_) => Err(DB_FAILED),
    }
}

/// The [`TIMER_KIND`] handler (FEE-018, FEE-019, FEE-020, FEE-038): a timer that ended, was
/// paused or re-timed does nothing; while charges are paused it is stopped; otherwise Stripe is
/// asked once more: a paid pick stays, an attempt Stripe has not decided waits (the queue runs
/// the task again), and otherwise the pick ends.
///
/// # Errors
/// A fixed sentence when the task is malformed, early or the database fails.
pub async fn run_pay_timer(
    pool: &PgPool,
    stripe: &impl PaymentsAdapter,
    payload: &Value,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let pick = pick_of(payload)?;
    let (kind, due) = (payload["kind"].as_str(), payload["due"].as_i64());
    let reason = match kind {
        Some(CARD_FIX) => EndReason::CardFix,
        Some(STRIPE_DOWN) => EndReason::StripeDown,
        _ => return Err(HandlerError("payments timer has no kind")),
    };
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    if running_due(&mut conn, pick, kind).await? != due {
        return Ok(());
    }
    if charges_paused(&mut conn).await.map_err(|_| DB_FAILED)? {
        drop(conn);
        return sync_pause(pool, now).await.map_err(|_| DB_FAILED);
    }
    if due.is_some_and(|d| now.as_second() < d) {
        return Err(HandlerError("payments timer ran early"));
    }
    drop(conn);
    let asked = ask_stripe(pool, stripe, pick, now)
        .await
        .map_err(|_| DB_FAILED)?;
    if asked.is_some_and(AttemptStatus::paid) {
        return Ok(());
    }
    if asked.is_some_and(AttemptStatus::undecided) {
        return Err(HandlerError("the charge attempt is still undecided"));
    }
    let mut tx = pool.begin().await.map_err(|_| DB_FAILED)?;
    end_pick(&mut tx, pick, reason, now)
        .await
        .map_err(|_| DB_FAILED)?;
    tx.commit().await.map_err(|_| DB_FAILED)
}

/// The due time, in Unix seconds, of the pick's running timer `kind`; `None` when it ended or is
/// stopped by the pause.
async fn running_due(
    conn: &mut PgConnection,
    pick: Uuid,
    kind: Option<&str>,
) -> Result<Option<i64>, HandlerError> {
    let running = sqlx::query_scalar!(
        "SELECT due_at FROM payments.pay_timer WHERE pick_id = $1 AND kind = $2
         AND ended_at IS NULL",
        pick,
        kind
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| DB_FAILED)?;
    Ok(running.flatten().map(|d| d.to_jiff().as_second()))
}

/// Brings every open payment timer in line with the charging pause (FEE-038, D-135): while
/// charges are paused each running timer stops and keeps its time left; once they resume each
/// stopped one runs again with that time. Whatever flips safe mode or the charging switch calls
/// it in its own request; the timer handler calls it too.
///
/// # Errors
/// [`ChargeError::Database`].
pub async fn sync_pause(pool: &PgPool, now: Timestamp) -> Result<(), ChargeError> {
    let mut tx = db(pool.begin().await)?;
    let paused = charges_paused(&mut tx).await?;
    let timers = db(sqlx::query!(
        "SELECT pick_id, kind, business_clock, due_at, left_seconds FROM payments.pay_timer
         WHERE ended_at IS NULL AND (due_at IS NULL) = $1 FOR UPDATE",
        !paused
    )
    .fetch_all(&mut *tx)
    .await)?;
    for t in timers {
        let (due, left) = match (t.due_at, t.left_seconds) {
            (Some(due), _) => (None, Some(time_left(now, due.to_jiff(), t.business_clock))),
            (None, left) => {
                let due = due_after(now, t.business_clock, left.unwrap_or(0));
                queue_timer(&mut tx, t.pick_id, &t.kind, due).await?;
                (Some(due), None)
            }
        };
        db(sqlx::query!(
            "UPDATE payments.pay_timer SET due_at = $3, left_seconds = $4
             WHERE pick_id = $1 AND kind = $2",
            t.pick_id,
            t.kind,
            due.map(|d| d.to_sqlx()) as _,
            left,
        )
        .execute(&mut *tx)
        .await)?;
    }
    db(tx.commit().await)
}
