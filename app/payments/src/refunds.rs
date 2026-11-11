//! Each automatic refund happens exactly once, a failed one is retried hourly with an alert, and a
//! refund on a disputed charge is blocked and flagged (FEE-026, FEE-052, D-1542).
//!
//! `picks` (T191) decides a refund with `refund_rules` and calls [`request_refund`]; an early
//! fraud warning comes through [`early_fraud_warning`]. A pick has one `payments.refund` row: the
//! claim step reads it under the pick's `pick_intent` row lock, asks `RefundStatus::next` what to
//! do, and writes the new status with its hourly [`RETRY_KIND`] task in one transaction; Stripe is
//! called after it commits (R11), keyed by the pick's `refund_label` and the try number. Stripe
//! refuses a second full refund of a charge, so a resend after an unanswered try cannot refund
//! twice, and the ledger's refund entry is keyed by the pick, so it is written once. Try `n > 1`
//! first replays try `n - 1`'s key, so a refund Stripe made on a try whose answer was lost comes
//! back as that try's `re_…` instead of a refusal of the new key (D-1545).
//!
//! T258 (D-1753, D-2067): Stripe's answer is read through [`RefundStripe`] and recorded as Stripe
//! reports it. Only `succeeded` records the refund and its ledger entry; `pending` or
//! `requires_action` stores the `re_…` as `pending`, appends nothing and queues the
//! [`REFUND_CHECK_KIND`] task that asks Stripe again every [`REFUND_CHECK_EVERY`]; `failed` or `canceled` is the
//! failure path (alert, hourly retry). [`refund_succeeded`], [`refund_failed`] and
//! [`refund_canceled`] (the webhook's, T259) move a `pending` row on; a failure after `succeeded`
//! appends one `refund-reversal` entry keyed by the `re_…`, and the retry's next success one new
//! `refund` entry keyed by its own `re_…`, so the ledger nets to what Stripe holds and no ledger
//! row is ever changed.
//!
//! T333 (D-2303, D-3018): a `succeeded` refund is booked at what Stripe reports
//! ([`Refund::amount_against`]): a `usd` amount other than the charge is booked as reported with
//! one [`REFUND_MISMATCH_ALERT`]; another currency, or an amount still missing after one re-read
//! (the request path re-reads; the webhook's and the check's `Refund` is already the re-read), is
//! booked at the charge with one [`REFUND_UNCONFIRMED_ALERT`]; each alert at most once open per
//! pick. A reversal reverses the amount its refund entry holds. [`refund_in_full`] tells `picks`
//! whether the "refunded in full" notice may go.

use askhava_adapters::api::{AdapterError, IdempotencyKey};
use askhava_core::api::{HandlerError, NewTask, enqueue};
use askhava_ops::api::{NewAlert, Severity, raise_alert, raise_alert_once};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::ledger::{EntryKind, LedgerEntry, NewEntry, ledger_append};
use crate::models::{MoneyEventError, RefundOutcome, RefundStep, RefundTrigger};
use crate::refund_rules::refund_label;
use crate::refund_stripe::{Refund, RefundAmount, RefundState, RefundStripe};
use crate::status::RefundStatus;

/// The idempotency action each refund try's Stripe key is derived under (SEC-018).
pub const REFUND_ACTION: &str = "payments.refund";
/// The `core.task` kind of the hourly refund retry (FEE-026).
pub const RETRY_KIND: &str = "payments.refund_retry";
/// FEE-026: a try with no success is sent again this long after it started.
pub const RETRY_EVERY: SignedDuration = SignedDuration::from_hours(1);
/// The `core.task` kind that asks Stripe again about a `pending` refund (FEE-026, D-1753).
pub const REFUND_CHECK_KIND: &str = "payments.refund_check";
/// D-1753: a `pending` refund is asked about again this long after the last answer.
pub const REFUND_CHECK_EVERY: SignedDuration = SignedDuration::from_mins(30);
/// The alert raised when a pick's refund first fails (FEE-026).
pub const REFUND_FAILED_ALERT: &str = "payments.refund_failed";
/// The alert raised once when a refund is blocked because its charge is disputed (FEE-026).
pub const REFUND_BLOCKED_ALERT: &str = "payments.refund_blocked";
/// The alert raised on every early fraud warning (FEE-052).
pub const FRAUD_WARNING_ALERT: &str = "payments.fraud_warning";
/// D-2303: a succeeded `usd` refund of another amount than the charge, booked as Stripe reported.
pub const REFUND_MISMATCH_ALERT: &str = "payments.refund_amount_mismatch";
/// D-2303: a succeeded refund in another currency, or with no amount, booked at the charge.
pub const REFUND_UNCONFIRMED_ALERT: &str = "payments.refund_amount_unconfirmed";
const PICK_SUBJECT: &str = "payments.pick_intent";

/// The charge being refunded, from its FEE-042 evidence record.
struct Charged {
    business: Uuid,
    job: Uuid,
    intent: String,
    amount: i64,
}

fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, MoneyEventError> {
    result.map_err(|_| MoneyEventError::Database)
}

async fn alert(
    conn: &mut PgConnection,
    kind: &'static str,
    pick: Uuid,
) -> Result<(), MoneyEventError> {
    let alert = NewAlert {
        kind,
        severity: Severity::High,
        subject: Some((PICK_SUBJECT, pick)),
    };
    raise_alert(conn, &alert)
        .await
        .map(drop)
        .map_err(|_| MoneyEventError::Database)
}

/// FEE-026: refunds the pick's charge in full, once. A repeated or duplicated trigger sends
/// nothing while a try is in flight and answers `Unchanged` once refunded; a disputed charge is
/// `Blocked` with one alert; a failed try is `Failed`, alerted on first failure and retried hourly.
/// A refund Stripe holds as `pending` is `Failed` too (not refunded yet, D-2067), so no "refunded"
/// notice goes out before Stripe's `succeeded`.
///
/// # Errors
/// [`MoneyEventError::NothingCharged`] when the pick has no charge evidence record, else
/// `Ledger` or `Database`.
pub async fn request_refund(
    pool: &PgPool,
    stripe: &impl RefundStripe,
    secret: &SecretString,
    (pick, trigger): (Uuid, RefundTrigger),
    now: Timestamp,
) -> Result<RefundOutcome, MoneyEventError> {
    let (step, charged, flags) = claim(pool, pick, Some(trigger), None, now).await?;
    finish(pool, stripe, secret, (pick, step, flags), &charged, now).await
}

/// The [`RETRY_KIND`] handler (FEE-026, JOB-006): sends try `try + 1` only when try `try` is
/// still the latest and did not succeed; safe to run twice.
///
/// # Errors
/// A fixed sentence when the payload is malformed or the database fails.
pub async fn run_refund_retry(
    pool: &PgPool,
    stripe: &impl RefundStripe,
    secret: &SecretString,
    payload: &Value,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let pick = payload["pick"]
        .as_str()
        .and_then(|p| Uuid::parse_str(p).ok());
    let try_no = payload["try"].as_i64().and_then(|t| i32::try_from(t).ok());
    let (Some(pick), Some(try_no)) = (pick, try_no) else {
        return Err(HandlerError("refund retry task is malformed"));
    };
    let failed = HandlerError("refund retry could not be recorded");
    let (step, charged, flags) = claim(pool, pick, None, Some(try_no), now)
        .await
        .map_err(|_| failed)?;
    finish(pool, stripe, secret, (pick, step, flags), &charged, now)
        .await
        .map_err(|_| failed)?;
    Ok(())
}

/// FEE-052: alerts Casey; a charge whose details were not yet `released` is refunded at once
/// (T191 ends the pick and returns the job), one already released waits for Casey's decision.
///
/// # Errors
/// As [`request_refund`].
pub async fn early_fraud_warning(
    pool: &PgPool,
    stripe: &impl RefundStripe,
    secret: &SecretString,
    (pick, released): (Uuid, bool),
    now: Timestamp,
) -> Result<RefundOutcome, MoneyEventError> {
    let mut conn = db(pool.acquire().await)?;
    alert(&mut conn, FRAUD_WARNING_ALERT, pick).await?;
    drop(conn);
    if released {
        return Ok(RefundOutcome::Unchanged);
    }
    request_refund(
        pool,
        stripe,
        secret,
        (pick, RefundTrigger::FraudWarning),
        now,
    )
    .await
}

/// Reads the charge and the refund under the pick's row lock and stores the next step with its
/// retry task (FEE-026). A retry (`trigger` `None`) of a pick with no refund row does nothing.
/// Also returns whether a send replays the previous key (D-1545) and whether Stripe holds the
/// refund as `pending` (D-1753).
async fn claim(
    pool: &PgPool,
    pick: Uuid,
    trigger: Option<RefundTrigger>,
    retry_of: Option<i32>,
    now: Timestamp,
) -> Result<(RefundStep, Charged, (bool, bool)), MoneyEventError> {
    let mut tx = db(pool.begin().await)?;
    let charged = charged_of(&mut tx, pick).await?;
    let row = db(sqlx::query!(
        "SELECT status, tries, stripe_refund_id FROM payments.refund WHERE pick_id = $1",
        pick,
    )
    .fetch_optional(&mut *tx)
    .await)?;
    let disputed = is_disputed(&mut tx, pick).await?;
    // A refund `refund_failed` reopened keeps its `re_…` until the next send, so that send never
    // replays the key whose success Stripe later failed (D-1545).
    let replay = row.as_ref().is_some_and(|r| r.stripe_refund_id.is_none());
    let current = row.and_then(|r| RefundStatus::from_stored(&r.status).map(|s| (s, r.tries)));
    let pending = current.is_some_and(|(s, _)| s.is_pending());
    let step = RefundStatus::next(current, disputed, retry_of);
    let (status, tries) = match step {
        RefundStep::Send(n) => (RefundStatus::Sending, n),
        RefundStep::Block(true) => (RefundStatus::Blocked, current.map_or(0, |(_, t)| t)),
        RefundStep::Block(false) | RefundStep::Nothing => {
            return Ok((step, charged, (false, pending)));
        }
    };
    store(
        &mut tx,
        pick,
        (trigger, current.is_some()),
        (status, tries),
        now,
    )
    .await?;
    if let RefundStep::Send(n) = step {
        queue_retry(&mut tx, pick, n, now + RETRY_EVERY, "").await?;
    } else {
        alert(&mut tx, REFUND_BLOCKED_ALERT, pick).await?;
    }
    db(tx.commit().await)?;
    Ok((step, charged, (replay, false)))
}

/// The pick's charge from its FEE-042 evidence record, under the pick's `pick_intent` row lock.
async fn charged_of(conn: &mut PgConnection, pick: Uuid) -> Result<Charged, MoneyEventError> {
    db(sqlx::query_as!(
        Charged,
        "SELECT e.business_id AS business, e.job_id AS job, e.stripe_intent_id AS intent,
                e.amount_cents AS amount
           FROM payments.charge_evidence e JOIN payments.pick_intent p ON p.pick_id = e.pick_id
          WHERE e.pick_id = $1 FOR UPDATE OF p",
        pick,
    )
    .fetch_optional(conn)
    .await)?
    .ok_or(MoneyEventError::NothingCharged)
}

async fn is_disputed(conn: &mut PgConnection, pick: Uuid) -> Result<bool, MoneyEventError> {
    db(sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM payments.dispute WHERE pick_id = $1) AS "disputed!""#,
        pick,
    )
    .fetch_one(conn)
    .await)
}

/// Writes the refund row's new status: an update when it exists (clearing a `re_…` Stripe later
/// failed, so the next send replays its predecessor's key, D-1545), else an insert with `trigger`.
async fn store(
    conn: &mut PgConnection,
    pick: Uuid,
    (trigger, exists): (Option<RefundTrigger>, bool),
    (status, tries): (RefundStatus, i32),
    now: Timestamp,
) -> Result<(), MoneyEventError> {
    if exists {
        db(sqlx::query!(
            "UPDATE payments.refund SET status = $2, tries = $3, stripe_refund_id = NULL
          WHERE pick_id = $1",
            pick,
            status.code(),
            tries,
        )
        .execute(conn)
        .await)?;
        return Ok(());
    }
    let trigger = trigger.ok_or(MoneyEventError::Database)?;
    db(sqlx::query!(
        "INSERT INTO payments.refund (pick_id, trigger, status, tries, requested_at)
         VALUES ($1, $2, $3, $4, $5)",
        pick,
        trigger.code(),
        status.code(),
        tries,
        now.to_sqlx() as _,
    )
    .execute(conn)
    .await)?;
    Ok(())
}

async fn queue_retry(
    conn: &mut PgConnection,
    pick: Uuid,
    try_no: i32,
    at: Timestamp,
    suffix: &str,
) -> Result<(), MoneyEventError> {
    let mut task = NewTask::new(RETRY_KIND, json!({"pick": pick, "try": try_no}), at);
    task.dedupe_key = Some(format!("{RETRY_KIND}.{pick}.{try_no}{suffix}"));
    enqueue(conn, &task)
        .await
        .map_err(|_| MoneyEventError::Database)?;
    Ok(())
}

/// Sends a claimed try to Stripe outside any transaction, then records the answer (R11). A
/// repeated trigger on a refund Stripe holds as `pending` is `Failed`: not refunded yet (D-2067).
async fn finish(
    pool: &PgPool,
    stripe: &impl RefundStripe,
    secret: &SecretString,
    (pick, step, (replay, pending)): (Uuid, RefundStep, (bool, bool)),
    charged: &Charged,
    now: Timestamp,
) -> Result<RefundOutcome, MoneyEventError> {
    let try_no = match step {
        RefundStep::Send(n) => n,
        RefundStep::Block(_) => return Ok(RefundOutcome::Blocked),
        RefundStep::Nothing if pending => return Ok(RefundOutcome::Failed),
        RefundStep::Nothing => return Ok(RefundOutcome::Unchanged),
    };
    let answer = send(stripe, secret, (pick, try_no, replay), &charged.intent).await;
    let answer = reread_missing(stripe, answer, charged).await;
    let mut tx = db(pool.begin().await)?;
    charged_of(&mut tx, pick).await?;
    let outcome = record_answer(&mut tx, (pick, try_no), charged, answer, now).await?;
    db(tx.commit().await)?;
    Ok(outcome)
}

/// D-2303: a `succeeded` answer whose amount is missing ([`RefundAmount::Missing`]) is re-read
/// once with `fetch_refund`, outside any transaction (R11); the re-read replaces it only when it is
/// still `succeeded` for the same PaymentIntent, else the answer stands (booked at the charge with
/// the unconfirmed alert).
async fn reread_missing(
    stripe: &impl RefundStripe,
    answer: Result<Refund, AdapterError>,
    charged: &Charged,
) -> Result<Refund, AdapterError> {
    let refund = answer?;
    let missing = refund.amount_against(charged.amount) == RefundAmount::Missing;
    if refund.state != RefundState::Succeeded || !missing {
        return Ok(refund);
    }
    match stripe.fetch_refund(&refund.id).await {
        Ok(read)
            if read.state == RefundState::Succeeded && read.payment_intent == charged.intent =>
        {
            Ok(read)
        }
        _ => Ok(refund),
    }
}

/// FEE-026, D-1753: records try `try_no`'s answer as Stripe reports it. Only `succeeded` is
/// `Refunded`; `pending` and `requires_action` store the `re_…` as `pending` with the
/// [`REFUND_CHECK_KIND`] task; `failed`, `canceled`, an error or an unreadable answer is the failure path.
async fn record_answer(
    conn: &mut PgConnection,
    (pick, try_no): (Uuid, i32),
    charged: &Charged,
    answer: Result<Refund, AdapterError>,
    now: Timestamp,
) -> Result<RefundOutcome, MoneyEventError> {
    let sending = (RefundStatus::Sending, Some(try_no));
    let Ok(refund) = answer else {
        failed(conn, pick, sending, RefundStatus::Failed, now).await?;
        return Ok(RefundOutcome::Failed);
    };
    match refund.state {
        RefundState::Succeeded => {
            if succeeded(conn, pick, charged, &refund, now).await? {
                return Ok(RefundOutcome::Refunded(refund.id));
            }
            failed(conn, pick, sending, RefundStatus::Failed, now).await?;
        }
        RefundState::Pending | RefundState::RequiresAction => {
            pending(conn, pick, &refund.id, now).await?;
        }
        RefundState::Failed => {
            failed(conn, pick, sending, RefundStatus::Failed, now).await?;
        }
        RefundState::Canceled => {
            failed(conn, pick, sending, RefundStatus::Canceled, now).await?;
        }
    }
    Ok(RefundOutcome::Failed)
}

/// FEE-026, D-1545: try `try_no` of `pick`'s refund. When `replay` (the refund has no `re_…`
/// yet), try `try_no - 1`'s key goes first: Stripe answers a key it already took with that
/// request's own answer, so a refund made on a try whose answer was lost comes back as its
/// `re_…`, and one never made is made now; only a refused replay sends the new key. Without the
/// replay the new key alone would meet Stripe's refusal of a second refund, and the pick would
/// stay failed with no ledger entry. A replayed refund Stripe failed or canceled is not this
/// try's answer, so the new key goes too (D-1753).
async fn send(
    stripe: &impl RefundStripe,
    secret: &SecretString,
    (pick, try_no, replay): (Uuid, i32, bool),
    intent: &str,
) -> Result<Refund, AdapterError> {
    let label = refund_label(pick);
    let key = |n: i32| IdempotencyKey::derive(secret, REFUND_ACTION, &[&label, &n.to_string()]);
    if replay && try_no > 1 {
        let earlier = stripe.refund(&key(try_no - 1), intent).await;
        let ended = |r: &Refund| matches!(r.state, RefundState::Failed | RefundState::Canceled);
        if earlier.as_ref().is_ok_and(|r| !ended(r)) {
            return earlier;
        }
    }
    stripe.refund(&key(try_no), intent).await
}

/// FEE-026, FEE-028, D-1753, D-2303: stores Stripe's `succeeded` for `re_…` (the caller holds the
/// pick's lock) and appends the refund entry the ledger then lacks, at [`booked`]'s amount with its
/// alert. A success naming a `re_…` already reversed is a stale answer (Stripe never moves a failed
/// or canceled refund on), so nothing is recorded and `false` comes back.
async fn succeeded(
    conn: &mut PgConnection,
    pick: Uuid,
    charged: &Charged,
    refund: &Refund,
    now: Timestamp,
) -> Result<bool, MoneyEventError> {
    if reversed(conn, pick, &refund.id).await? {
        return Ok(false);
    }
    let (amount, flagged) = booked(refund, charged.amount);
    if let Some(kind) = flagged {
        raise_alert_once(conn, kind, Severity::High, (PICK_SUBJECT, pick))
            .await
            .map_err(|_| MoneyEventError::Database)?;
    }
    let refund = refund.id.as_str();
    db(sqlx::query!(
        "UPDATE payments.refund SET status = $2, stripe_refund_id = $3, decided_at = $4
          WHERE pick_id = $1",
        pick,
        RefundStatus::Succeeded.code(),
        refund,
        now.to_sqlx() as _,
    )
    .execute(&mut *conn)
    .await)?;
    let (refunds, reversals) = held(conn, pick).await?;
    if refunds > reversals {
        return Ok(true);
    }
    let key = if refunds == 0 {
        format!("refund.{pick}")
    } else {
        format!("refund.{pick}.{refund}")
    };
    append(conn, charged, (&key, amount), false).await?;
    Ok(true)
}

/// FEE-026, FEE-028, FEE-053, D-2303: the amount a succeeded refund is booked at and the alert it
/// raises: the charge with none when full, Stripe's `usd` amount with [`REFUND_MISMATCH_ALERT`],
/// and the charge (never a foreign or missing figure) with [`REFUND_UNCONFIRMED_ALERT`].
fn booked(refund: &Refund, charged: i64) -> (i64, Option<&'static str>) {
    match refund.amount_against(charged) {
        RefundAmount::Full => (charged, None),
        RefundAmount::Differs(amount) => (amount, Some(REFUND_MISMATCH_ALERT)),
        RefundAmount::Foreign | RefundAmount::Missing => (charged, Some(REFUND_UNCONFIRMED_ALERT)),
    }
}

/// D-1753: stores the `re_…` Stripe holds as `pending` and queues the first [`REFUND_CHECK_KIND`] task.
async fn pending(
    conn: &mut PgConnection,
    pick: Uuid,
    refund: &str,
    now: Timestamp,
) -> Result<(), MoneyEventError> {
    db(sqlx::query!(
        "UPDATE payments.refund SET status = $2, stripe_refund_id = $3 WHERE pick_id = $1",
        pick,
        RefundStatus::Pending.code(),
        refund,
    )
    .execute(&mut *conn)
    .await)?;
    queue_check(conn, refund, now + REFUND_CHECK_EVERY).await
}

async fn queue_check(
    conn: &mut PgConnection,
    refund: &str,
    at: Timestamp,
) -> Result<(), MoneyEventError> {
    let mut task = NewTask::new(REFUND_CHECK_KIND, json!({"refund": refund}), at);
    task.dedupe_key = Some(format!("{REFUND_CHECK_KIND}.{refund}.{}", at.as_second()));
    enqueue(conn, &task)
        .await
        .map_err(|_| MoneyEventError::Database)?;
    Ok(())
}

/// The pick's `refund` and `refund-reversal` ledger entries so far (FEE-028, D-1753): the ledger
/// holds a refund when the first count is higher.
async fn held(conn: &mut PgConnection, pick: Uuid) -> Result<(i64, i64), MoneyEventError> {
    let row = db(sqlx::query!(
        r#"SELECT count(*) FILTER (WHERE kind = 'refund') AS "refunds!",
                  count(*) FILTER (WHERE kind = 'refund-reversal') AS "reversals!"
             FROM payments.ledger_entry
            WHERE idempotency_key = 'refund.' || $1::uuid::text
               OR idempotency_key LIKE 'refund.' || $1::uuid::text || '.%'
               OR idempotency_key LIKE 'refund-reversal.' || $1::uuid::text || '.%'"#,
        pick,
    )
    .fetch_one(conn)
    .await)?;
    Ok((row.refunds, row.reversals))
}

/// D-2303: the amount of the pick's latest `refund` entry, the one a reversal undoes (a refund
/// booked at Stripe's own amount is reversed at that amount); `None` when there is none.
async fn last_refund(conn: &mut PgConnection, pick: Uuid) -> Result<Option<i64>, MoneyEventError> {
    db(sqlx::query_scalar!(
        "SELECT amount_cents FROM payments.ledger_entry
          WHERE kind = 'refund'
            AND (idempotency_key = 'refund.' || $1::uuid::text
                 OR idempotency_key LIKE 'refund.' || $1::uuid::text || '.%')
          ORDER BY seq DESC LIMIT 1",
        pick,
    )
    .fetch_optional(conn)
    .await)
}

async fn reversed(
    conn: &mut PgConnection,
    pick: Uuid,
    refund: &str,
) -> Result<bool, MoneyEventError> {
    let key = format!("refund-reversal.{pick}.{refund}");
    db(sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM payments.ledger_entry WHERE idempotency_key = $1)
             AS "reversed!""#,
        key,
    )
    .fetch_one(conn)
    .await)
}

/// Appends one `refund` entry, or its `refund-reversal` when `reverse`, of `amount` naming the
/// refunded PaymentIntent (FEE-028, D-1753, D-2303).
async fn append(
    conn: &mut PgConnection,
    charged: &Charged,
    (key, amount): (&str, i64),
    reverse: bool,
) -> Result<(), MoneyEventError> {
    let mut entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::Refund,
        business: Some(charged.business),
        job: Some(charged.job),
        amount_cents: amount,
        stripe_id: Some(&charged.intent),
        idempotency_key: key,
    })?;
    if reverse {
        entry = entry.reversal(key)?;
    }
    ledger_append(conn, &entry).await?;
    Ok(())
}

/// Marks the refund `to` (`failed` or `canceled`) when it still stands at `from` (`Sending` try
/// `try_no`, a `Pending` refund, or a `Succeeded` refund Stripe later failed), so a refused
/// resend of an already refunded charge changes nothing; alerts Casey on the first failure. The
/// hourly retry was queued with the try.
async fn failed(
    conn: &mut PgConnection,
    pick: Uuid,
    (from, try_no): (RefundStatus, Option<i32>),
    to: RefundStatus,
    now: Timestamp,
) -> Result<Option<i32>, MoneyEventError> {
    let tries = db(sqlx::query_scalar!(
        "UPDATE payments.refund SET status = $2, decided_at = $4
          WHERE pick_id = $1 AND status = $5 AND ($3::int IS NULL OR tries = $3)
         RETURNING tries",
        pick,
        to.code(),
        try_no,
        now.to_sqlx() as _,
        from.code(),
    )
    .fetch_optional(&mut *conn)
    .await)?;
    if tries == Some(1) {
        alert(conn, REFUND_FAILED_ALERT, pick).await?;
    }
    Ok(tries)
}

/// The pick of the refund `re_…`, its stored status and its charge, read under the pick's
/// `pick_intent` row lock (which every writer of the refund row holds), so the status cannot move
/// before the caller's transaction ends; `None` when no refund row holds that `re_…` now in a
/// status `wanted` accepts.
async fn refund_row(
    conn: &mut PgConnection,
    refund: &str,
    wanted: fn(RefundStatus) -> bool,
) -> Result<Option<(Uuid, RefundStatus, Charged)>, MoneyEventError> {
    let pick = db(sqlx::query_scalar!(
        "SELECT pick_id FROM payments.refund WHERE stripe_refund_id = $1",
        refund,
    )
    .fetch_optional(&mut *conn)
    .await)?;
    let Some(pick) = pick else {
        return Ok(None);
    };
    let charged = charged_of(conn, pick).await?;
    let status = db(sqlx::query_scalar!(
        "SELECT status FROM payments.refund WHERE pick_id = $1 AND stripe_refund_id = $2",
        pick,
        refund,
    )
    .fetch_optional(&mut *conn)
    .await)?;
    let status = status.and_then(|s| RefundStatus::from_stored(&s));
    Ok(status.filter(|s| wanted(*s)).map(|s| (pick, s, charged)))
}

/// FEE-026, FEE-028, D-1753, D-2303, D-3018: Stripe's `succeeded` `refund` as the caller re-read
/// it (the webhook's, T259, or the check's; that read is D-2303's one re-read, so nothing is
/// fetched again). A `pending` refund of the pick's own PaymentIntent becomes `succeeded` with its
/// ledger refund entry at the amount and with the alert [`Refund::amount_against`] gives; anything
/// else is unchanged.
///
/// # Errors
/// [`MoneyEventError::Database`] or `Ledger`.
pub async fn refund_succeeded(
    pool: &PgPool,
    refund: &Refund,
    now: Timestamp,
) -> Result<bool, MoneyEventError> {
    let mut tx = db(pool.begin().await)?;
    let row = refund_row(&mut tx, &refund.id, RefundStatus::is_pending).await?;
    let Some((pick, _, charged)) = row else {
        return Ok(false);
    };
    if charged.intent != refund.payment_intent {
        return Ok(false);
    }
    let done = succeeded(&mut tx, pick, &charged, refund, now).await?;
    db(tx.commit().await)?;
    Ok(done)
}

/// FEE-026, FEE-028, FEE-053, D-2303, D-3018: whether `pick`'s succeeded refund may be announced
/// as "refunded in full" (`picks`' `unreachable_refunded`). `false` only on positive evidence that
/// it is not the whole charge in `usd`: the ledger holds its refund at another amount, or a re-read
/// of its `re_…` reports another currency or another `usd` amount. `true` when there is no succeeded
/// refund, or the re-read reports no amount or no currency, or answers [`AdapterError::NotSent`]
/// (the provided `read_refund`): a missing amount's notice goes (D-2303).
///
/// # Errors
/// [`MoneyEventError::Stripe`] for any other read error (a foreign refund must never be announced
/// on a guess), or `Database`.
pub async fn refund_in_full(
    pool: &PgPool,
    stripe: &impl RefundStripe,
    pick: Uuid,
) -> Result<bool, MoneyEventError> {
    let mut conn = db(pool.acquire().await)?;
    let Some((refund, charged)) = succeeded_refund(&mut conn, pick).await? else {
        return Ok(true);
    };
    let (refunds, reversals) = held(&mut conn, pick).await?;
    let booked = last_refund(&mut conn, pick).await?;
    drop(conn);
    if refunds > reversals && booked.is_some_and(|a| a != charged) {
        return Ok(false);
    }
    match stripe.fetch_refund(&refund).await {
        Ok(read) => Ok(matches!(
            read.amount_against(charged),
            RefundAmount::Full | RefundAmount::Missing
        )),
        Err(AdapterError::NotSent) => Ok(true),
        Err(_) => Err(MoneyEventError::Stripe),
    }
}

/// The `re_…` of `pick`'s `succeeded` refund and the charged amount, from its FEE-042 record.
async fn succeeded_refund(
    conn: &mut PgConnection,
    pick: Uuid,
) -> Result<Option<(String, i64)>, MoneyEventError> {
    let row = db(sqlx::query!(
        r#"SELECT r.stripe_refund_id AS "refund!", e.amount_cents AS charged
             FROM payments.refund r JOIN payments.charge_evidence e ON e.pick_id = r.pick_id
            WHERE r.pick_id = $1 AND r.status = $2 AND r.stripe_refund_id IS NOT NULL"#,
        pick,
        RefundStatus::Succeeded.code(),
    )
    .fetch_optional(conn)
    .await)?;
    Ok(row.map(|r| (r.refund, r.charged)))
}

/// FEE-026, P13: Stripe's `refund.failed` for `re_…`, after it took the refund or while it was
/// pending. The refund is marked failed, Casey alerted on its first failure, a retry queued an
/// hour on, and a `refund` entry the ledger still holds unreversed (from `succeeded`, or the nightly
/// reconcile's while the row was `pending`) gets one `refund-reversal` entry (D-1753, D-2067).
///
/// # Errors
/// [`MoneyEventError::Database`] or `Ledger`.
pub async fn refund_failed(
    pool: &PgPool,
    refund: &str,
    now: Timestamp,
) -> Result<bool, MoneyEventError> {
    refund_ended(pool, refund, RefundStatus::Failed, now).await
}

/// FEE-026, D-1753: Stripe canceled the refund `re_…`; as [`refund_failed`], stored `canceled`.
///
/// # Errors
/// [`MoneyEventError::Database`] or `Ledger`.
pub async fn refund_canceled(
    pool: &PgPool,
    refund: &str,
    now: Timestamp,
) -> Result<bool, MoneyEventError> {
    refund_ended(pool, refund, RefundStatus::Canceled, now).await
}

async fn refund_ended(
    pool: &PgPool,
    refund: &str,
    to: RefundStatus,
    now: Timestamp,
) -> Result<bool, MoneyEventError> {
    let mut tx = db(pool.begin().await)?;
    let row = refund_row(&mut tx, refund, RefundStatus::may_fail).await?;
    let Some((pick, from, charged)) = row else {
        return Ok(false);
    };
    let (refunds, reversals) = held(&mut tx, pick).await?;
    if refunds > reversals {
        let key = format!("refund-reversal.{pick}.{refund}");
        let amount = last_refund(&mut tx, pick).await?.unwrap_or(charged.amount);
        append(&mut tx, &charged, (&key, amount), true).await?;
    }
    let tries = failed(&mut tx, pick, (from, None), to, now)
        .await?
        .ok_or(MoneyEventError::Database)?;
    queue_retry(
        &mut tx,
        pick,
        tries,
        now + RETRY_EVERY,
        &format!(".{}", to.code()),
    )
    .await?;
    db(tx.commit().await)?;
    Ok(true)
}

/// The [`REFUND_CHECK_KIND`] handler (FEE-026, D-1753, JOB-006): asks Stripe about a refund still
/// `pending` and moves it on by Stripe's answer; while Stripe still holds it pending, or cannot be
/// read, the next check is queued [`REFUND_CHECK_EVERY`] on. A refund no longer pending is left alone.
///
/// # Errors
/// A fixed sentence when the payload is malformed or the database fails.
pub async fn run_refund_check(
    pool: &PgPool,
    stripe: &impl RefundStripe,
    payload: &Value,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let refund = payload["refund"]
        .as_str()
        .ok_or(HandlerError("refund check task is malformed"))?;
    let failed = HandlerError("refund check could not be recorded");
    let mut conn = pool.acquire().await.map_err(|_| failed)?;
    let row = refund_row(&mut conn, refund, RefundStatus::is_pending)
        .await
        .map_err(|_| failed)?;
    let Some((_, _, charged)) = row else {
        return Ok(());
    };
    let answer = stripe.fetch_refund(refund).await;
    let answer = answer.ok().filter(|r| r.payment_intent == charged.intent);
    let moved = match answer.as_ref().map(|r| (r, r.state)) {
        Some((read, RefundState::Succeeded)) => refund_succeeded(pool, read, now).await,
        Some((_, RefundState::Failed)) => refund_failed(pool, refund, now).await,
        Some((_, RefundState::Canceled)) => refund_canceled(pool, refund, now).await,
        _ => Ok(false),
    };
    if !moved.map_err(|_| failed)? {
        queue_check(&mut conn, refund, now + REFUND_CHECK_EVERY)
            .await
            .map_err(|_| failed)?;
    }
    Ok(())
}
