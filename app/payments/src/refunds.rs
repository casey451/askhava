//! Each automatic refund happens exactly once, a failed one is retried hourly with an alert, and a
//! refund on a disputed charge is blocked and flagged (FEE-026, FEE-052, D-1542).
//!
//! `picks` (T191) decides a refund with `refund_rules` and calls [`request_refund`]; an early
//! fraud warning comes through [`early_fraud_warning`]. A pick has one `payments.refund` row: the
//! claim step reads it under the pick's `pick_intent` row lock, asks `RefundStatus::next` what to
//! do, and writes the new status with its hourly [`RETRY_KIND`] task in one transaction; Stripe is
//! called after it commits (R11), keyed by the pick's `refund_label` and the try number. Stripe
//! refuses a second full refund of a charge, so a resend after an unanswered try cannot refund
//! twice, and the ledger's refund entry is keyed by the pick, so it is written once.

use askhava_adapters::api::{IdempotencyKey, PaymentsAdapter};
use askhava_core::api::{HandlerError, NewTask, enqueue};
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::ledger::{EntryKind, LedgerEntry, NewEntry, ledger_append};
use crate::models::{MoneyEventError, RefundOutcome, RefundStep, RefundTrigger};
use crate::refund_rules::refund_label;
use crate::status::RefundStatus;

/// The idempotency action each refund try's Stripe key is derived under (SEC-018).
pub const REFUND_ACTION: &str = "payments.refund";
/// The `core.task` kind of the hourly refund retry (FEE-026).
pub const RETRY_KIND: &str = "payments.refund_retry";
/// FEE-026: a try with no success is sent again this long after it started.
pub const RETRY_EVERY: SignedDuration = SignedDuration::from_hours(1);
/// The alert raised when a pick's refund first fails (FEE-026).
pub const REFUND_FAILED_ALERT: &str = "payments.refund_failed";
/// The alert raised once when a refund is blocked because its charge is disputed (FEE-026).
pub const REFUND_BLOCKED_ALERT: &str = "payments.refund_blocked";
/// The alert raised on every early fraud warning (FEE-052).
pub const FRAUD_WARNING_ALERT: &str = "payments.fraud_warning";
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
///
/// # Errors
/// [`MoneyEventError::NothingCharged`] when the pick has no charge evidence record, else
/// `Ledger` or `Database`.
pub async fn request_refund(
    pool: &PgPool,
    stripe: &impl PaymentsAdapter,
    secret: &SecretString,
    (pick, trigger): (Uuid, RefundTrigger),
    now: Timestamp,
) -> Result<RefundOutcome, MoneyEventError> {
    let (step, charged) = claim(pool, pick, Some(trigger), None, now).await?;
    finish(pool, stripe, secret, (pick, step), &charged, now).await
}

/// The [`RETRY_KIND`] handler (FEE-026, JOB-006): sends try `try + 1` only when try `try` is
/// still the latest and did not succeed; safe to run twice.
///
/// # Errors
/// A fixed sentence when the payload is malformed or the database fails.
pub async fn run_refund_retry(
    pool: &PgPool,
    stripe: &impl PaymentsAdapter,
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
    let (step, charged) = claim(pool, pick, None, Some(try_no), now)
        .await
        .map_err(|_| failed)?;
    finish(pool, stripe, secret, (pick, step), &charged, now)
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
    stripe: &impl PaymentsAdapter,
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
async fn claim(
    pool: &PgPool,
    pick: Uuid,
    trigger: Option<RefundTrigger>,
    retry_of: Option<i32>,
    now: Timestamp,
) -> Result<(RefundStep, Charged), MoneyEventError> {
    let mut tx = db(pool.begin().await)?;
    let charged = db(sqlx::query_as!(
        Charged,
        "SELECT e.business_id AS business, e.job_id AS job, e.stripe_intent_id AS intent,
                e.amount_cents AS amount
           FROM payments.charge_evidence e JOIN payments.pick_intent p ON p.pick_id = e.pick_id
          WHERE e.pick_id = $1 FOR UPDATE OF p",
        pick,
    )
    .fetch_optional(&mut *tx)
    .await)?
    .ok_or(MoneyEventError::NothingCharged)?;
    let row = db(sqlx::query!(
        "SELECT status, tries FROM payments.refund WHERE pick_id = $1",
        pick,
    )
    .fetch_optional(&mut *tx)
    .await)?;
    let disputed = is_disputed(&mut tx, pick).await?;
    let current = row.and_then(|r| RefundStatus::from_code(&r.status).map(|s| (s, r.tries)));
    let step = RefundStatus::next(current, disputed, retry_of);
    let (status, tries) = match step {
        RefundStep::Send(n) => (RefundStatus::Sending, n),
        RefundStep::Block(true) => (RefundStatus::Blocked, current.map_or(0, |(_, t)| t)),
        RefundStep::Block(false) | RefundStep::Nothing => return Ok((step, charged)),
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
    Ok((step, charged))
}

async fn is_disputed(conn: &mut PgConnection, pick: Uuid) -> Result<bool, MoneyEventError> {
    db(sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM payments.dispute WHERE pick_id = $1) AS "disputed!""#,
        pick,
    )
    .fetch_one(conn)
    .await)
}

/// Writes the refund row's new status: an update when it exists, else an insert with `trigger`.
async fn store(
    conn: &mut PgConnection,
    pick: Uuid,
    (trigger, exists): (Option<RefundTrigger>, bool),
    (status, tries): (RefundStatus, i32),
    now: Timestamp,
) -> Result<(), MoneyEventError> {
    if exists {
        db(sqlx::query!(
            "UPDATE payments.refund SET status = $2, tries = $3 WHERE pick_id = $1",
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

/// Sends a claimed try to Stripe outside any transaction, then records the answer (R11).
async fn finish(
    pool: &PgPool,
    stripe: &impl PaymentsAdapter,
    secret: &SecretString,
    (pick, step): (Uuid, RefundStep),
    charged: &Charged,
    now: Timestamp,
) -> Result<RefundOutcome, MoneyEventError> {
    let try_no = match step {
        RefundStep::Send(n) => n,
        RefundStep::Block(_) => return Ok(RefundOutcome::Blocked),
        RefundStep::Nothing => return Ok(RefundOutcome::Unchanged),
    };
    let label = refund_label(pick);
    let key = IdempotencyKey::derive(secret, REFUND_ACTION, &[&label, &try_no.to_string()]);
    let answer = stripe.create_refund(&key, &charged.intent).await;
    let mut tx = db(pool.begin().await)?;
    let outcome = if let Ok(refund) = answer {
        refunded(&mut tx, pick, charged, &refund, now).await?;
        RefundOutcome::Refunded(refund)
    } else {
        failed(&mut tx, pick, (RefundStatus::Sending, Some(try_no)), now).await?;
        RefundOutcome::Failed
    };
    db(tx.commit().await)?;
    Ok(outcome)
}

/// Stores Stripe's `re_…` and the pick's one ledger refund entry, keyed by the pick and naming the
/// refunded PaymentIntent, so a later successful retry appends nothing (FEE-026, FEE-028).
async fn refunded(
    conn: &mut PgConnection,
    pick: Uuid,
    charged: &Charged,
    refund: &str,
    now: Timestamp,
) -> Result<(), MoneyEventError> {
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
    let key = format!("refund.{pick}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::Refund,
        business: Some(charged.business),
        job: Some(charged.job),
        amount_cents: charged.amount,
        stripe_id: Some(&charged.intent),
        idempotency_key: &key,
    })?;
    ledger_append(conn, &entry).await?;
    Ok(())
}

/// Marks the refund failed when it still stands at `from` (`Sending` try `try_no`, or a
/// `Succeeded` refund Stripe later failed), so a refused resend of an already refunded charge
/// changes nothing; alerts Casey on the first failure. The hourly retry was queued with the try.
async fn failed(
    conn: &mut PgConnection,
    pick: Uuid,
    (from, try_no): (RefundStatus, Option<i32>),
    now: Timestamp,
) -> Result<Option<i32>, MoneyEventError> {
    let tries = db(sqlx::query_scalar!(
        "UPDATE payments.refund SET status = $2, decided_at = $4
          WHERE pick_id = $1 AND status = $5 AND ($3::int IS NULL OR tries = $3)
         RETURNING tries",
        pick,
        RefundStatus::Failed.code(),
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

/// FEE-026, P13: Stripe's `refund.failed` for `re_…` after it first took the refund. The refund
/// is marked failed, Casey alerted on its first failure, and a retry queued an hour on.
///
/// # Errors
/// [`MoneyEventError::Database`].
pub async fn refund_failed(
    pool: &PgPool,
    refund: &str,
    now: Timestamp,
) -> Result<bool, MoneyEventError> {
    let mut tx = db(pool.begin().await)?;
    let pick = db(sqlx::query_scalar!(
        "SELECT pick_id FROM payments.refund WHERE stripe_refund_id = $1",
        refund,
    )
    .fetch_optional(&mut *tx)
    .await)?;
    let Some(pick) = pick else {
        return Ok(false);
    };
    let Some(tries) = failed(&mut tx, pick, (RefundStatus::Succeeded, None), now).await? else {
        return Ok(false);
    };
    queue_retry(&mut tx, pick, tries, now + RETRY_EVERY, ".failed").await?;
    db(tx.commit().await)?;
    Ok(true)
}
