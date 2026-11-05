//! Bank disputes: Casey is alerted, the ledger records each dispute once as its amount plus the
//! $15 Stripe fee, and the evidence pack, built from the charge's FEE-042 record with IDs and
//! timestamps only, is sent 2 days before the due date when T191 marked the dispute for auto-submit
//! (FEE-026, FEE-027, FEE-042, FEE-050, PRIV-027, D-1543, D-1544).
//!
//! `payments` cannot read picks or calls (D-491), so FEE-050's condition (a recorded release and a
//! call attempt, or a 3DS-authenticated payment) is T191's, which calls [`mark_auto_submit`]; an
//! unmarked dispute waits for Casey, who counters in Stripe and is recorded by
//! [`record_owner_submission`], so it is never sent again (D-229). A dispute row also blocks
//! every refund of its charge (`refunds`, FEE-026). [`list_disputes`] feeds the weekly digest and
//! the owner portal, with the countering fee at stake.

use std::future::Future;

use askhava_adapters::api::{AdapterError, IdempotencyKey};
use askhava_core::api::{HandlerError, NewTask, enqueue};
use askhava_ops::api::{Actor, AuditEntry, Change, NewAlert, Severity, audit, raise_alert};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::ledger::{EntryKind, LedgerEntry, NewEntry, ledger_append};
use crate::models::{DisputeOpened, DisputeView, EvidencePack, MoneyEventError};

/// FEE-027, FEE-050: Stripe's dispute fee, recorded with the disputed amount, and the countering
/// fee recorded when evidence is submitted.
pub const DISPUTE_FEE_CENTS: i64 = 1500;
/// FEE-027: auto-submitted evidence goes this long before Stripe's due date.
pub const SUBMIT_BEFORE: SignedDuration = SignedDuration::from_hours(48);
/// The `core.task` kind that submits a marked dispute's evidence (FEE-027).
pub const SUBMIT_KIND: &str = "payments.dispute_submit";
/// The audit action and alert kind of a new dispute (FEE-027).
pub const DISPUTE_ACTION: &str = "payments.dispute_opened";
/// The idempotency action the evidence submission's Stripe key is derived under (SEC-018).
pub const EVIDENCE_ACTION: &str = "payments.dispute_evidence";
const DISPUTE_SUBJECT: &str = "payments.dispute";

/// Submitting a dispute's evidence to Stripe (`POST /v1/disputes/{id}` with `submit=true`).
pub trait DisputeStripe {
    /// Sends `form` as the evidence of `dispute` (`dp_…`) and submits it.
    fn submit_evidence(
        &self,
        idempotency: &IdempotencyKey,
        dispute: &str,
        form: &[(&'static str, String)],
    ) -> impl Future<Output = Result<(), AdapterError>> + Send;
}

fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, MoneyEventError> {
    result.map_err(|_| MoneyEventError::Database)
}

fn vendor_id(id: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| {
        id.strip_prefix(p).is_some_and(|rest| {
            !rest.is_empty() && rest.len() <= 200 && rest.bytes().all(|b| b.is_ascii_alphanumeric())
        })
    })
}

/// FEE-027: records a `charge.dispute.created` once. A new dispute writes one ledger `dispute`
/// entry of the amount plus [`DISPUTE_FEE_CENTS`], an audit row, a High alert to Casey and the
/// [`SUBMIT_KIND`] task at the due date less [`SUBMIT_BEFORE`]; a repeated event returns `false`
/// and writes nothing. From then on the charge's refunds are blocked (FEE-026): the insert holds
/// the pick's `pick_intent` row lock, as a refund claim does, so the two never interleave.
///
/// # Errors
/// [`MoneyEventError::UnknownCharge`] for a malformed ID, an amount of 0 or less, or a
/// PaymentIntent no pick was charged through; else `Ledger` or `Database`.
pub async fn record_dispute(
    pool: &PgPool,
    opened: &DisputeOpened<'_>,
    now: Timestamp,
) -> Result<bool, MoneyEventError> {
    let ids_ok =
        vendor_id(opened.dispute, &["dp_", "du_"]) && vendor_id(opened.payment_intent, &["pi_"]);
    if !ids_ok || opened.amount_cents <= 0 {
        return Err(MoneyEventError::UnknownCharge);
    }
    let mut tx = db(pool.begin().await)?;
    let charge = db(sqlx::query!(
        "SELECT e.pick_id, e.business_id, e.job_id
           FROM payments.charge_evidence e JOIN payments.pick_intent p ON p.pick_id = e.pick_id
          WHERE e.stripe_intent_id = $1 FOR UPDATE OF p",
        opened.payment_intent,
    )
    .fetch_optional(&mut *tx)
    .await)?
    .ok_or(MoneyEventError::UnknownCharge)?;
    let inserted = db(sqlx::query_scalar!(
        "INSERT INTO payments.dispute (stripe_dispute_id, pick_id, amount_cents, due_at, opened_at)
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (stripe_dispute_id) DO NOTHING RETURNING id",
        opened.dispute,
        charge.pick_id,
        opened.amount_cents,
        opened.due_at.to_sqlx() as _,
        now.to_sqlx() as _,
    )
    .fetch_optional(&mut *tx)
    .await)?;
    let Some(id) = inserted else {
        return Ok(false);
    };
    let key = format!("dispute.{}", opened.dispute);
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::Dispute,
        business: Some(charge.business_id),
        job: Some(charge.job_id),
        amount_cents: opened.amount_cents + DISPUTE_FEE_CENTS,
        stripe_id: Some(opened.dispute),
        idempotency_key: &key,
    })?;
    ledger_append(&mut tx, &entry).await?;
    announce(&mut tx, id).await?;
    queue_submit(&mut tx, id, opened.due_at - SUBMIT_BEFORE, "").await?;
    db(tx.commit().await)?;
    Ok(true)
}

/// Queues the [`SUBMIT_KIND`] task of dispute `id` at `at`, once per `suffix`.
async fn queue_submit(
    conn: &mut PgConnection,
    id: Uuid,
    at: Timestamp,
    suffix: &str,
) -> Result<(), MoneyEventError> {
    let mut task = NewTask::new(SUBMIT_KIND, json!({"dispute": id}), at);
    task.dedupe_key = Some(format!("{SUBMIT_KIND}.{id}{suffix}"));
    enqueue(conn, &task)
        .await
        .map_err(|_| MoneyEventError::Database)?;
    Ok(())
}

/// The audit row (the alert's evidence) and Casey's High alert of dispute `id`.
async fn announce(conn: &mut PgConnection, id: Uuid) -> Result<(), MoneyEventError> {
    let entry = AuditEntry {
        actor: Actor::System,
        action: DISPUTE_ACTION,
        subject_kind: DISPUTE_SUBJECT,
        subject_id: id,
        change: Change::Action,
    };
    audit(conn, &entry)
        .await
        .map_err(|_| MoneyEventError::Database)?;
    let alert = NewAlert {
        kind: DISPUTE_ACTION,
        severity: Severity::High,
        subject: Some((DISPUTE_SUBJECT, id)),
    };
    raise_alert(conn, &alert)
        .await
        .map_err(|_| MoneyEventError::Database)?;
    Ok(())
}

/// FEE-050: T191 marks `dispute` (`dp_…`) for auto-submit once the charge has a recorded release
/// and a call attempt or a 3DS-authenticated payment. A new mark queues its own [`SUBMIT_KIND`]
/// task at the later of `now` and the due date less [`SUBMIT_BEFORE`] (none once due), so a
/// dispute marked after its first task ran, or opened under 2 days before its due date, is still
/// sent (FEE-027, D-1545). `false` when it is unknown, already marked or already in.
///
/// # Errors
/// [`MoneyEventError::Database`].
pub async fn mark_auto_submit(
    conn: &mut PgConnection,
    dispute: &str,
    now: Timestamp,
) -> Result<bool, MoneyEventError> {
    let marked = db(sqlx::query!(
        "UPDATE payments.dispute SET auto_submit = true
          WHERE stripe_dispute_id = $1 AND submitted_at IS NULL AND NOT auto_submit
         RETURNING id, due_at",
        dispute,
    )
    .fetch_optional(&mut *conn)
    .await)?;
    let Some(row) = marked else {
        return Ok(false);
    };
    let due = row.due_at.to_jiff();
    if now < due {
        queue_submit(conn, row.id, now.max(due - SUBMIT_BEFORE), ".marked").await?;
    }
    Ok(true)
}

/// D-229, FEE-050: Casey submitted `dispute`'s evidence himself; it is never auto-submitted after,
/// and the countering fee is recorded once. `false` when it was already in.
///
/// # Errors
/// `Ledger` or `Database`.
pub async fn record_owner_submission(
    conn: &mut PgConnection,
    dispute: &str,
    at: Timestamp,
) -> Result<bool, MoneyEventError> {
    submitted(conn, dispute, "owner", at).await
}

/// Marks the evidence in, `by` `auto` or `owner`, and appends the countering fee (FEE-050).
async fn submitted(
    conn: &mut PgConnection,
    dispute: &str,
    by: &str,
    at: Timestamp,
) -> Result<bool, MoneyEventError> {
    let row = db(sqlx::query!(
        "UPDATE payments.dispute d SET submitted_by = $2, submitted_at = $3
           FROM payments.charge_evidence e
          WHERE d.stripe_dispute_id = $1 AND d.submitted_at IS NULL AND e.pick_id = d.pick_id
         RETURNING e.business_id, e.job_id",
        dispute,
        by,
        at.to_sqlx() as _,
    )
    .fetch_optional(&mut *conn)
    .await)?;
    let Some(row) = row else {
        return Ok(false);
    };
    let key = format!("countering.{dispute}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::CounteringFee,
        business: Some(row.business_id),
        job: Some(row.job_id),
        amount_cents: DISPUTE_FEE_CENTS,
        stripe_id: Some(dispute),
        idempotency_key: &key,
    })?;
    ledger_append(conn, &entry).await?;
    Ok(true)
}

/// FEE-042, PRIV-027: the evidence pack of the charge of `pick`, from its evidence record and its
/// refund, IDs and timestamps only.
///
/// # Errors
/// [`MoneyEventError::NothingCharged`] or `Database`.
pub async fn evidence_pack(
    conn: &mut PgConnection,
    pick: Uuid,
) -> Result<EvidencePack, MoneyEventError> {
    let row = db(sqlx::query!(
        "SELECT e.stripe_customer_id, e.stripe_intent_id, e.idempotency_label,
                e.authorization_version, e.amount_cents, e.pressed_at, e.charged_at,
                r.stripe_refund_id AS \"refund?\"
           FROM payments.charge_evidence e LEFT JOIN payments.refund r ON r.pick_id = e.pick_id
          WHERE e.pick_id = $1",
        pick,
    )
    .fetch_optional(conn)
    .await)?
    .ok_or(MoneyEventError::NothingCharged)?;
    Ok(EvidencePack {
        customer: row.stripe_customer_id,
        payment_intent: row.stripe_intent_id,
        attempt_label: row.idempotency_label,
        authorization_version: row.authorization_version,
        amount_cents: row.amount_cents,
        pressed_at: row.pressed_at.to_jiff(),
        charged_at: row.charged_at.to_jiff(),
        refund: row.refund,
    })
}

/// The [`SUBMIT_KIND`] handler (FEE-027, FEE-050, JOB-006): a dispute marked for auto-submit and
/// not yet in has its evidence pack sent, then is marked in with its countering fee; an unmarked
/// one waits for Casey and one already in is never sent again. Safe to run twice: the Stripe key
/// is derived from the dispute.
///
/// # Errors
/// A fixed sentence when the payload is malformed, Stripe fails (the task runs again) or the
/// database fails.
pub async fn run_dispute_submit(
    pool: &PgPool,
    stripe: &impl DisputeStripe,
    secret: &SecretString,
    payload: &Value,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let failed = HandlerError("dispute evidence could not be submitted");
    let id = payload["dispute"]
        .as_str()
        .and_then(|d| Uuid::parse_str(d).ok());
    let id = id.ok_or(HandlerError("dispute submit task is malformed"))?;
    let mut conn = pool.acquire().await.map_err(|_| failed)?;
    let row = sqlx::query!(
        "SELECT stripe_dispute_id, pick_id FROM payments.dispute
          WHERE id = $1 AND auto_submit AND submitted_at IS NULL",
        id,
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| failed)?;
    let Some(row) = row else {
        return Ok(());
    };
    let pack = evidence_pack(&mut conn, row.pick_id)
        .await
        .map_err(|_| failed)?;
    drop(conn);
    let key = IdempotencyKey::derive(secret, EVIDENCE_ACTION, &[&row.stripe_dispute_id]);
    stripe
        .submit_evidence(&key, &row.stripe_dispute_id, &pack.form())
        .await
        .map_err(|_| failed)?;
    let mut tx = pool.begin().await.map_err(|_| failed)?;
    submitted(&mut tx, &row.stripe_dispute_id, "auto", now)
        .await
        .map_err(|_| failed)?;
    tx.commit().await.map_err(|_| failed)
}

/// FEE-027, FEE-050: every dispute opened at or after `since`, and every earlier one still open
/// (evidence not in, due date not passed before `since`), oldest first, for the weekly digest and
/// the owner portal, each with the countering fee at stake (D-1545).
///
/// # Errors
/// [`MoneyEventError::Database`].
pub async fn list_disputes(
    conn: &mut PgConnection,
    since: Timestamp,
) -> Result<Vec<DisputeView>, MoneyEventError> {
    let rows = db(sqlx::query!(
        "SELECT stripe_dispute_id, amount_cents, due_at, auto_submit, submitted_at
           FROM payments.dispute
          WHERE opened_at >= $1 OR (submitted_at IS NULL AND due_at >= $1)
          ORDER BY opened_at, stripe_dispute_id",
        since.to_sqlx() as _,
    )
    .fetch_all(conn)
    .await)?;
    Ok(rows
        .into_iter()
        .map(|r| DisputeView {
            dispute: r.stripe_dispute_id,
            amount_cents: r.amount_cents,
            due_at: r.due_at.to_jiff(),
            auto_submit: r.auto_submit,
            submitted_at: r.submitted_at.map(|t| t.to_jiff()),
            fee_at_stake_cents: DISPUTE_FEE_CENTS,
        })
        .collect())
}
