//! What each charge answer starts, the payment timers and the `payments` events (FEE-016,
//! FEE-018, FEE-019, FEE-020, FEE-037, FEE-038, FEE-041, D-135, D-153, D-270, D-491).
//!
//! [`record`] stores an attempt's answer with its audit row and, in that transaction: a decline
//! or bank approval opens the card-fix window (1 business hour, 60 clock minutes on an emergency
//! job, PICK-044) and queues [`CARD_FIX_KIND`], which `picks::money_notices` turns into the
//! business's notice (FEE-020, D-492); a success writes the pick's one FEE-042 evidence record
//! ([`write_evidence`], D-1474); no answer from Stripe starts the 4-hour release (FEE-018);
//! an undecided attempt queues [`CHECK_KIND`] for 30 minutes later (FEE-019). A timer is a
//! `payments.pay_timer` row plus a [`TIMER_KIND`] task at its due time; [`run_pay_timer`] asks
//! Stripe once more: the 4-hour release ends the pick only while Stripe still cannot be reached,
//! the card-fix window only on Stripe's word that the card still needs fixing, a paid pick stays,
//! and anything Stripe has not settled asks again in 5 minutes; an ended pick raises
//! [`PICK_ENDED_KIND`] for `picks` to offer the next quote. Every attempt has a check queued at
//! its start, and a check that finds it still undecided queues the next (FEE-019). [`sync_pause`]
//! stops every running timer while safe mode or the charging switch pauses charges and resumes
//! each with the time it had left (FEE-038); [`run_pause_watch`] runs it every minute while a
//! timer is open. `payments` calls neither `messaging` nor `picks`: every event is a `core.task`
//! of IDs and times (D-491).

use askhava_adapters::api::PaymentsAdapter;
use askhava_core::api::{HandlerError, NewTask, add_business_minutes, enqueue, is_business_hours};
use askhava_ops::api::{Actor, AuditEntry, Change, Switch, audit, is_paused};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::charge::{ATTEMPT_SUBJECT, ask_stripe, latest, write_evidence};
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
    if !from.may_become(status) {
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
        write_evidence(tx, pick, try_no, at).await?;
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
            "try": try_no, "approval": status.needs_approval(),
            "fix_by": fix_by.map(Timestamp::as_second)});
        queue(tx, CARD_FIX_KIND, payload, at, format!("{pick}.{try_no}")).await?;
    }
    if status.undecided() {
        let check_at = at.saturating_add(CHECK_AFTER).unwrap_or(at);
        queue_check(tx, pick, try_no, 0, check_at).await?;
    }
    Ok(())
}

/// Queues round `round` of the "ask Stripe directly" check of attempt `try_no` at `at`
/// (FEE-019); each round has its own dedupe key, so a check that finds the attempt still
/// undecided queues the next one and an attempt never waits without a check.
pub(crate) async fn queue_check(
    tx: &mut PgConnection,
    pick: Uuid,
    try_no: i32,
    round: i64,
    at: Timestamp,
) -> Result<(), ChargeError> {
    let payload = json!({"pick": pick, "try": try_no, "round": round});
    let dedupe = format!("{pick}.{try_no}.{round}");
    queue(tx, CHECK_KIND, payload, at, dedupe).await
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
    if started.is_some() {
        queue_watch(tx, at.saturating_add(PAUSE_EVERY).unwrap_or(at)).await?;
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
/// FEE-041); returns false, changing nothing, for a pick already ended or with no PaymentIntent.
pub(crate) async fn end_pick(
    tx: &mut PgConnection,
    pick: Uuid,
    reason: EndReason,
    at: Timestamp,
) -> Result<bool, ChargeError> {
    let ended = db(sqlx::query!(
        "UPDATE payments.pick_intent SET ended_at = $2 WHERE pick_id = $1 AND ended_at IS NULL
         RETURNING business_id, job_id",
        pick,
        at.to_sqlx() as _,
    )
    .fetch_optional(&mut *tx)
    .await)?;
    let Some(row) = ended else {
        return Ok(false);
    };
    end_timers(tx, pick, None, at).await?;
    announce_end(tx, pick, (row.business_id, row.job_id), reason, at).await?;
    Ok(true)
}

/// The audit row and the [`PICK_ENDED_KIND`] event of an ended pick, `parties` its business and
/// job; also for a pick ended before its PaymentIntent was made (FEE-041). The event's dedupe key
/// is the pick, so it is raised once.
pub(crate) async fn announce_end(
    tx: &mut PgConnection,
    pick: Uuid,
    (business, job): (Uuid, Uuid),
    reason: EndReason,
    at: Timestamp,
) -> Result<(), ChargeError> {
    let entry = AuditEntry {
        actor: Actor::System,
        action: PICK_ENDED_KIND,
        subject_kind: "payments.pick_intent",
        subject_id: pick,
        change: Change::Action,
    };
    audit(tx, &entry).await.map_err(|_| ChargeError::Database)?;
    let payload = json!({"pick": pick, "business": business, "job": job,
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

/// The [`CHECK_KIND`] handler (FEE-019, FEE-037): if attempt `try` is still the pick's latest
/// and undecided, ask Stripe directly; while Stripe is unreachable or the attempt is still
/// undecided (for example `processing`), the next round is queued 30 minutes on, so an attempt
/// never blocks the pick without a check. Safe to run twice (JOB-006).
///
/// # Errors
/// A fixed sentence when the task is malformed or the database fails.
pub async fn run_charge_check(
    pool: &PgPool,
    stripe: &impl PaymentsAdapter,
    payload: &Value,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let pick = pick_of(payload)?;
    let try_no = payload["try"].as_i64().and_then(|t| i32::try_from(t).ok());
    let round = payload["round"].as_i64().unwrap_or(0);
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    let open = latest(&mut conn, pick).await.map_err(|_| DB_FAILED)?;
    drop(conn);
    if !open.is_some_and(|l| Some(l.try_no) == try_no && l.status.undecided()) {
        return Ok(());
    }
    let asked = ask_stripe(pool, stripe, pick, now)
        .await
        .map_err(|_| DB_FAILED)?;
    if asked.is_some_and(|s| !s.undecided()) {
        return Ok(());
    }
    let mut tx = pool.begin().await.map_err(|_| DB_FAILED)?;
    let next = now.saturating_add(CHECK_AFTER).unwrap_or(now);
    queue_check(&mut tx, pick, try_no.unwrap_or(0), round + 1, next)
        .await
        .map_err(|_| DB_FAILED)?;
    tx.commit().await.map_err(|_| DB_FAILED)
}

/// What a payment timer's end comes to once Stripe was asked (FEE-018, FEE-019, FEE-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// The pick stays: paid, or Stripe answered and that answer's own effects took over.
    Stay,
    /// Stripe has not decided, or (for the card-fix window) could not be reached: ask again in
    /// [`RECHECK_AFTER`], never giving up without Stripe's answer (FEE-019, D-153).
    Retime,
    /// The pick ends with the timer's reason.
    End,
}

/// The [`Verdict`] of timer `reason` given Stripe's answer `asked` (`None`: unreachable). The
/// 4-hour release ends the pick only while Stripe still cannot be reached; once it answers, the
/// answer's effects (the card-fix window, the 30-minute check) run instead. The card-fix window
/// ends the pick only on Stripe's word that the card still needs fixing.
fn verdict(reason: EndReason, asked: Option<AttemptStatus>) -> Verdict {
    let card_fix = reason == EndReason::CardFix;
    match asked {
        None if card_fix => Verdict::Retime,
        None => Verdict::End,
        Some(s) if s.paid() => Verdict::Stay,
        Some(s) if card_fix && s.needs_card_fix() => Verdict::End,
        Some(s) if !card_fix && s.stripe_answered() => Verdict::Stay,
        Some(_) => Verdict::Retime,
    }
}

/// How long a timer whose end Stripe has not settled waits before asking again (FEE-019).
pub const RECHECK_AFTER: SignedDuration = SignedDuration::from_mins(5);

/// The [`TIMER_KIND`] handler (FEE-018, FEE-019, FEE-020, FEE-038): a timer that ended, was
/// paused or re-timed does nothing; while charges are paused it is stopped; otherwise Stripe is
/// asked once more and [`verdict`] decides: the pick stays, the timer asks again in
/// [`RECHECK_AFTER`], or the pick ends.
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
    let (reason, Some(due)) = (
        match kind {
            Some(CARD_FIX) => EndReason::CardFix,
            Some(STRIPE_DOWN) => EndReason::StripeDown,
            _ => return Err(HandlerError("payments timer has no kind")),
        },
        due,
    ) else {
        return Err(HandlerError("payments timer has no due time"));
    };
    let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
    if running_due(&mut conn, pick, kind).await? != Some(due) {
        return Ok(());
    }
    if charges_paused(&mut conn).await.map_err(|_| DB_FAILED)? {
        drop(conn);
        return sync_pause(pool, now).await.map_err(|_| DB_FAILED);
    }
    if now.as_second() < due {
        return Err(HandlerError("payments timer ran early"));
    }
    drop(conn);
    let asked = ask_stripe(pool, stripe, pick, now)
        .await
        .map_err(|_| DB_FAILED)?;
    let verdict = verdict(reason, asked);
    if verdict == Verdict::Stay {
        return Ok(());
    }
    let mut tx = pool.begin().await.map_err(|_| DB_FAILED)?;
    if verdict == Verdict::Retime {
        retime(&mut tx, pick, kind, now).await?;
    } else {
        end_pick(&mut tx, pick, reason, now)
            .await
            .map_err(|_| DB_FAILED)?;
    }
    tx.commit().await.map_err(|_| DB_FAILED)
}

/// Moves running timer `kind` to [`RECHECK_AFTER`] from `now` and queues it there.
async fn retime(
    tx: &mut PgConnection,
    pick: Uuid,
    kind: Option<&str>,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let due = now.saturating_add(RECHECK_AFTER).unwrap_or(now);
    let moved = sqlx::query!(
        "UPDATE payments.pay_timer SET due_at = $3 WHERE pick_id = $1 AND kind = $2
         AND ended_at IS NULL AND due_at IS NOT NULL",
        pick,
        kind,
        due.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| DB_FAILED)?;
    if let (1, Some(kind)) = (moved.rows_affected(), kind) {
        queue_timer(tx, pick, kind, due)
            .await
            .map_err(|_| DB_FAILED)?;
    }
    Ok(())
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
/// stopped one runs again with that time. [`run_pause_watch`] calls it every minute while any
/// timer is open, and the timer handler calls it when it fires during a pause.
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

/// The `core.task` kind of the pause watch (FEE-038): every [`PAUSE_EVERY`] while any payment
/// timer is open, it brings the timers in line with the charging pause.
pub const PAUSE_KIND: &str = "payments.pause_watch";
/// How often the pause watch runs; a timer stops or resumes within this of a switch flip.
pub const PAUSE_EVERY: SignedDuration = SignedDuration::from_mins(1);

async fn queue_watch(tx: &mut PgConnection, at: Timestamp) -> Result<(), ChargeError> {
    let minute = at.as_second().div_euclid(60);
    queue(tx, PAUSE_KIND, json!({}), at, minute.to_string()).await
}

/// The [`PAUSE_KIND`] handler (FEE-038, D-135): runs [`sync_pause`], then queues itself a minute
/// on while any payment timer is still open, so a flip of safe mode or the charging switch stops
/// and resumes the timers without the switch's code calling `payments`. One task per minute
/// (its dedupe key), however many timers started it.
///
/// # Errors
/// A fixed sentence when the database fails.
pub async fn run_pause_watch(pool: &PgPool, now: Timestamp) -> Result<(), HandlerError> {
    sync_pause(pool, now).await.map_err(|_| DB_FAILED)?;
    let mut tx = pool.begin().await.map_err(|_| DB_FAILED)?;
    let open = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM payments.pay_timer WHERE ended_at IS NULL) AS "open!""#
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| DB_FAILED)?;
    if open {
        let next = now.saturating_add(PAUSE_EVERY).unwrap_or(now);
        queue_watch(&mut tx, next).await.map_err(|_| DB_FAILED)?;
    }
    tx.commit().await.map_err(|_| DB_FAILED)
}

/// Where a pick's card-fix window stands, for the FEE-020 notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardFixWindow {
    /// No fix is needed now: the pick is paid or ended, or no window is open.
    Closed,
    /// The window runs until this instant.
    Running(Timestamp),
    /// The window is stopped by the charging pause (FEE-038).
    Stopped,
}

/// The pick's card-fix window now (FEE-020): `picks`' notice handler sends nothing for a pick
/// that no longer needs a fix.
///
/// # Errors
/// [`ChargeError::Database`].
pub async fn card_fix_window(
    conn: &mut PgConnection,
    pick: Uuid,
) -> Result<CardFixWindow, ChargeError> {
    let due = db(sqlx::query_scalar!(
        "SELECT t.due_at FROM payments.pay_timer t JOIN payments.pick_intent i USING (pick_id)
         WHERE t.pick_id = $1 AND t.kind = 'card-fix' AND t.ended_at IS NULL
           AND i.ended_at IS NULL AND NOT EXISTS (SELECT 1 FROM payments.charge_attempt a
             WHERE a.pick_id = $1 AND a.status = 'succeeded')",
        pick
    )
    .fetch_optional(conn)
    .await)?;
    Ok(match due {
        None => CardFixWindow::Closed,
        Some(Some(due)) => CardFixWindow::Running(due.to_jiff()),
        Some(None) => CardFixWindow::Stopped,
    })
}
