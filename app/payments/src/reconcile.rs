//! The nightly reconcile and the ledger-only money view and statements (FEE-029, FEE-030,
//! FEE-032, FEE-051, D-153, D-1552 to D-1555).
//!
//! [`reconcile`] reads Stripe's succeeded charges of the last [`LOOKBACK`] through
//! [`ReconcileStripe`] and, charge by charge, appends what [`plan_charge`] decides in one short
//! transaction: a missing `charge` entry (auto-match to the pick its PaymentIntent or metadata
//! names), the balance transaction's `processing-fee` and a refund Stripe already made. A charge on
//! a closed job, a second kept charge for a pick and a charge naming no pick are refunded after
//! the commit (R11): a charge with T066's evidence record through `request_refund` (its refund
//! row, row lock, hourly retry and alert), any other keyed by the charge and the night, so a rerun
//! the same night refunds once and a refused refund is a new try the next night, with
//! [`REFUND_FAILED_ALERT`] (D-1555). Each unknown charge is written once to
//! `payments.reconcile_charge` with one [`UNKNOWN_CHARGE_ALERT`]; a charge whose writes fail is
//! skipped with [`CHARGE_FAILED_ALERT`] and the run goes on. Last it compares gross charges,
//! refunds and disputes in the ledger with Stripe's to the cent and raises [`MISMATCH_ALERT`] when
//! they differ or Stripe listed a malformed charge. [`money_view`] and [`business_statement`]
//! read only `payments.ledger_entry`.

use std::future::Future;

use askhava_adapters::api::{AdapterError, IdempotencyKey, PaymentsAdapter};
use askhava_core::api::{HandlerError, NewTask, enqueue};
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::late::LATE_TRIGGER;
use crate::ledger::{EntryKind, LedgerEntry, NewEntry, ledger_append};
use crate::models::{
    MoneyEventError, MoneyTotals, MoneyView, PickFacts, RefundOutcome, RefundWhy, StripeCharge,
    plan_charge,
};
use crate::refunds::{REFUND_FAILED_ALERT, request_refund};
use crate::service::CONFIRM_FEE_ACTION;

/// The nightly task kind `server` registers with [`run_reconcile`].
pub const RECONCILE_KIND: &str = "payments.reconcile";
/// The idempotency action a reconcile refund's Stripe key derives under (SEC-018).
pub const RECONCILE_ACTION: &str = "payments.reconcile";
/// Raised once per unknown charge (FEE-029: the alert is kept).
pub const UNKNOWN_CHARGE_ALERT: &str = "payments.unknown_charge";
/// Raised when the ledger and Stripe differ by a cent or Stripe listed a malformed charge (FEE-030).
pub const MISMATCH_ALERT: &str = "payments.ledger_mismatch";
/// Raised once per run for each charge whose writes failed; the run goes on (D-1555).
pub const CHARGE_FAILED_ALERT: &str = "payments.reconcile_charge_failed";
/// How far back each run reads: inside Stripe's 30-day event list, past its 3-day retries.
pub const LOOKBACK: SignedDuration = SignedDuration::from_hours(7 * 24);
/// The gap between nightly runs.
pub const RECONCILE_EVERY: SignedDuration = SignedDuration::from_hours(24);

/// The Stripe reads the reconcile needs; T132's client implements it (FEE-029, FEE-051).
pub trait ReconcileStripe: PaymentsAdapter {
    /// Succeeded charges created at or after `since`, from Stripe's event list, each with its
    /// balance transaction's processing fee, refunded amount and dispute.
    fn list_charges(
        &self,
        since: Timestamp,
    ) -> impl Future<Output = Result<Vec<StripeCharge>, AdapterError>> + Send;
}

/// What one run did and the FEE-030 match it ended with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    /// Charges read from Stripe.
    pub seen: usize,
    /// Unknown charges matched to a pick's job.
    pub auto_matched: usize,
    /// Refunds Stripe took this run.
    pub refunded: usize,
    /// Refunds Stripe refused or did not answer, each alerted: T066 retries its own hourly, the
    /// reconcile's own go again the next night under that night's key.
    pub refund_failed: usize,
    /// Charges Stripe listed that are not well formed; left out of the match, they raise
    /// [`MISMATCH_ALERT`].
    pub malformed: usize,
    /// Charges whose database writes failed (for example a reused ledger key), each alerted with
    /// [`CHARGE_FAILED_ALERT`] and left out of the match.
    pub failed: usize,
    /// Stripe's totals over the charges that name a known pick.
    pub stripe: MoneyTotals,
    /// The ledger's totals over the same charges.
    pub ledger: MoneyTotals,
}

impl ReconcileReport {
    /// FEE-030: the ledger matches Stripe to the cent and every listed charge was well formed.
    #[must_use]
    pub fn matched(&self) -> bool {
        self.stripe == self.ledger && self.malformed == 0
    }
}

fn db<T>(result: Result<T, sqlx::Error>) -> Result<T, MoneyEventError> {
    result.map_err(|_| MoneyEventError::Database)
}

async fn alert(
    conn: &mut PgConnection,
    kind: &'static str,
    pick: Option<Uuid>,
) -> Result<(), MoneyEventError> {
    let subject = pick.map(|p| ("payments.pick_intent", p));
    let alert = NewAlert {
        kind,
        severity: Severity::High,
        subject,
    };
    raise_alert(conn, &alert)
        .await
        .map(drop)
        .map_err(|_| MoneyEventError::Database)
}

/// FEE-029, FEE-030, FEE-051: one reconcile run (see the module comment). A malformed charge is
/// counted and raises [`MISMATCH_ALERT`]; a charge whose writes fail raises
/// [`CHARGE_FAILED_ALERT`] and the run goes on to the next (D-1555).
///
/// # Errors
/// [`MoneyEventError::Stripe`] when the list cannot be read, else `Database` when an alert or the
/// match cannot be read or written.
pub async fn reconcile(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    now: Timestamp,
) -> Result<ReconcileReport, MoneyEventError> {
    let charges = stripe
        .list_charges(now - LOOKBACK)
        .await
        .map_err(|_| MoneyEventError::Stripe)?;
    let mut report = ReconcileReport {
        seen: charges.len(),
        malformed: charges.iter().filter(|c| !c.is_well_formed()).count(),
        ..ReconcileReport::default()
    };
    let mut known = Vec::new();
    for charge in charges.iter().filter(|c| c.is_well_formed()) {
        match one_charge(pool, stripe, id_secret, charge, &mut report, now).await {
            Ok(Some(now_held)) => known.push(now_held),
            Ok(None) => {}
            Err(_) => {
                report.failed += 1;
                alert(&mut *db(pool.acquire().await)?, CHARGE_FAILED_ALERT, None).await?;
            }
        }
    }
    report.stripe = MoneyTotals::of_stripe(&known);
    report.ledger = ledger_totals(pool, &known).await?;
    if !report.matched() {
        alert(&mut *db(pool.acquire().await)?, MISMATCH_ALERT, None).await?;
    }
    Ok(report)
}

/// Plans and appends one charge, then sends its refund after the commit. When the charge names a
/// known pick, returns it as Stripe now holds it (refunded if this run refunded it), for the
/// FEE-030 match.
async fn one_charge(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    charge: &StripeCharge,
    report: &mut ReconcileReport,
    now: Timestamp,
) -> Result<Option<StripeCharge>, MoneyEventError> {
    let mut tx = db(pool.begin().await)?;
    let pick = pick_facts(&mut tx, charge).await?;
    let plan = plan_charge(charge, pick, ledger_facts(&mut tx, charge).await?);
    if let Some(facts) = pick {
        for (kind, cents) in &plan.lines {
            append(&mut tx, id_secret, charge, facts, (*kind, *cents)).await?;
        }
        report.auto_matched += usize::from(plan.unknown);
    }
    if plan.unknown && first_seen(&mut tx, charge, pick, plan.refund, now).await? {
        alert(&mut tx, UNKNOWN_CHARGE_ALERT, pick.map(|p| p.pick)).await?;
    }
    db(tx.commit().await)?;
    let mut held = charge.clone();
    if let Some(why) = plan.refund
        && refund(pool, stripe, id_secret, (charge, now), (pick, why), report).await?
    {
        held.refunded_cents = held.amount_cents;
    }
    Ok(pick.map(|_| held))
}

/// The pick the charge's PaymentIntent belongs to, else the one its metadata names (D-153).
async fn pick_facts(
    conn: &mut PgConnection,
    charge: &StripeCharge,
) -> Result<Option<PickFacts>, MoneyEventError> {
    let row = db(sqlx::query!(
        "SELECT pick_id, business_id, job_id, stripe_intent_id, ended_at IS NOT NULL AS \"ended!\"
           FROM payments.pick_intent
          WHERE stripe_intent_id = $1 OR pick_id = $2
          ORDER BY (stripe_intent_id = $1) DESC LIMIT 1",
        charge.payment_intent,
        charge.pick,
    )
    .fetch_optional(&mut *conn)
    .await)?;
    Ok(row.map(|r| PickFacts {
        pick: r.pick_id,
        business: r.business_id,
        job: r.job_id,
        own_intent: r.stripe_intent_id == charge.payment_intent,
        ended: r.ended,
    }))
}

/// Whether the ledger already holds this charge's `charge` entry and a `refund` of its intent.
async fn ledger_facts(
    conn: &mut PgConnection,
    charge: &StripeCharge,
) -> Result<(bool, bool), MoneyEventError> {
    let row = db(sqlx::query!(
        "SELECT bool_or(kind = 'charge' AND stripe_id = $1) AS has_charge,
                bool_or(kind = 'refund' AND stripe_id = $2) AS has_refund
           FROM payments.ledger_entry WHERE stripe_id IN ($1, $2)",
        charge.id,
        charge.payment_intent,
    )
    .fetch_one(&mut *conn)
    .await)?;
    Ok((
        row.has_charge.unwrap_or(false),
        row.has_refund.unwrap_or(false),
    ))
}

/// The ledger key and Stripe ID of a planned line. A charge or refund on the pick's own intent
/// uses the key `record_fee_at_confirm` or T066's refund uses, so neither can double it.
fn key_and_id(
    id_secret: &SecretString,
    charge: &StripeCharge,
    facts: PickFacts,
    kind: EntryKind,
) -> (String, String) {
    let (job, business) = (facts.job.to_string(), facts.business.to_string());
    match (kind, facts.own_intent) {
        (EntryKind::Charge, true) => {
            let key = IdempotencyKey::derive(id_secret, CONFIRM_FEE_ACTION, &[&job, &business]);
            (key.as_str().to_owned(), charge.id.clone())
        }
        (EntryKind::Charge, false) => {
            (format!("reconcile.charge.{}", charge.id), charge.id.clone())
        }
        (EntryKind::Refund, true) => (
            format!("refund.{}", facts.pick),
            charge.payment_intent.clone(),
        ),
        (EntryKind::Refund, false) => (
            format!("reconcile.refund.{}", charge.id),
            charge.payment_intent.clone(),
        ),
        _ => (
            format!("reconcile.fee.{}", charge.balance_transaction),
            charge.balance_transaction.clone(),
        ),
    }
}

async fn append(
    conn: &mut PgConnection,
    id_secret: &SecretString,
    charge: &StripeCharge,
    facts: PickFacts,
    (kind, cents): (EntryKind, i64),
) -> Result<(), MoneyEventError> {
    let (key, stripe_id) = key_and_id(id_secret, charge, facts, kind);
    let entry = LedgerEntry::new(NewEntry {
        kind,
        business: Some(facts.business),
        job: Some(facts.job),
        amount_cents: cents,
        stripe_id: Some(&stripe_id),
        idempotency_key: &key,
    })?;
    ledger_append(conn, &entry).await?;
    Ok(())
}

/// Records an unknown charge once; true when this run wrote the row (so its alert is new).
async fn first_seen(
    conn: &mut PgConnection,
    charge: &StripeCharge,
    pick: Option<PickFacts>,
    why: Option<RefundWhy>,
    now: Timestamp,
) -> Result<bool, MoneyEventError> {
    let done = db(sqlx::query!(
        "INSERT INTO payments.reconcile_charge
           (stripe_charge_id, stripe_intent_id, pick_id, refund_reason, seen_at)
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (stripe_charge_id) DO NOTHING",
        charge.id,
        charge.payment_intent,
        pick.map(|p| p.pick),
        why.map(RefundWhy::code),
        now.to_sqlx() as _,
    )
    .execute(&mut *conn)
    .await)?;
    Ok(done.rows_affected() == 1)
}

/// FEE-029, FEE-026, D-153, D-1555: refunds the charge in full once. A charge on the pick's own
/// intent goes through T066's `request_refund` (its refund row under the pick's lock, so T064's
/// late-success refund and this one are one refund, with T066's try keys, hourly retry and alert)
/// when T066 has the pick's evidence record; otherwise [`own_refund`] sends it. True when Stripe
/// took the refund.
async fn refund(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    (charge, now): (&StripeCharge, Timestamp),
    (pick, why): (Option<PickFacts>, RefundWhy),
    report: &mut ReconcileReport,
) -> Result<bool, MoneyEventError> {
    if let Some(facts) = pick.filter(|f| f.own_intent) {
        match request_refund(pool, stripe, id_secret, (facts.pick, LATE_TRIGGER), now).await {
            Err(MoneyEventError::NothingCharged) => {}
            Ok(RefundOutcome::Refunded(_)) => {
                report.refunded += 1;
                return Ok(true);
            }
            Ok(RefundOutcome::Failed) => {
                report.refund_failed += 1;
                return Ok(false);
            }
            other => return other.map(|_| false),
        }
    }
    own_refund(pool, stripe, id_secret, (charge, now), (pick, why), report).await
}

/// The reconcile's own refund of a charge T066 has no record of. The Stripe key names the charge,
/// the reason and the night, so a rerun the same night gets Stripe's first answer and a refused
/// refund (which Stripe would replay for the same key) is a new try the next night; a refund made
/// on a lost answer shows as refunded in the next list and is not sent again. A failure raises
/// [`REFUND_FAILED_ALERT`]; the ledger `refund` entry follows Stripe's success (D-1555).
async fn own_refund(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    (charge, now): (&StripeCharge, Timestamp),
    (pick, why): (Option<PickFacts>, RefundWhy),
    report: &mut ReconcileReport,
) -> Result<bool, MoneyEventError> {
    let night = now.as_second().div_euclid(86_400).to_string();
    let key = IdempotencyKey::derive(
        id_secret,
        RECONCILE_ACTION,
        &[&charge.id, why.code(), &night],
    );
    if stripe
        .create_refund(&key, &charge.payment_intent)
        .await
        .is_err()
    {
        report.refund_failed += 1;
        let mut conn = db(pool.acquire().await)?;
        alert(&mut conn, REFUND_FAILED_ALERT, pick.map(|p| p.pick)).await?;
        return Ok(false);
    }
    report.refunded += 1;
    if let Some(facts) = pick {
        let mut tx = db(pool.begin().await)?;
        append(
            &mut tx,
            id_secret,
            charge,
            facts,
            (EntryKind::Refund, charge.amount_cents),
        )
        .await?;
        db(tx.commit().await)?;
    }
    Ok(true)
}

/// The ledger's gross charges, refunds and disputes over the given charges' IDs (FEE-030).
async fn ledger_totals(
    pool: &PgPool,
    charges: &[StripeCharge],
) -> Result<MoneyTotals, MoneyEventError> {
    let ids: Vec<String> = charges.iter().map(|c| c.id.clone()).collect();
    let intents: Vec<String> = charges.iter().map(|c| c.payment_intent.clone()).collect();
    let disputes: Vec<String> = charges
        .iter()
        .filter_map(|c| c.dispute.as_ref().map(|(d, _)| d.clone()))
        .collect();
    let rows = db(sqlx::query!(
        "SELECT kind, amount_cents FROM payments.ledger_entry
          WHERE (kind = 'charge' AND stripe_id = ANY($1))
             OR (kind = 'refund' AND stripe_id = ANY($2))
             OR (kind = 'dispute' AND stripe_id = ANY($3))",
        &ids,
        &intents,
        &disputes,
    )
    .fetch_all(pool)
    .await)?;
    let entries = rows
        .iter()
        .filter_map(|r| Some((kind_of(&r.kind)?, r.amount_cents)));
    Ok(MoneyView::from_entries(entries).gross())
}

fn kind_of(code: &str) -> Option<EntryKind> {
    EntryKind::ALL.into_iter().find(|k| k.code() == code)
}

/// The [`RECONCILE_KIND`] handler: queues the next night's task first, then runs, so a failed run
/// (a Stripe outage) never ends the nightly chain (D-1555; JOB-006: the dedupe key names the next
/// run's day, so a rerun or a task retry queues it once).
///
/// # Errors
/// A fixed sentence when the queueing or the run fails; the task's own retries rerun it.
pub async fn run_reconcile(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    _payload: &Value,
    now: Timestamp,
) -> Result<(), HandlerError> {
    let failed = HandlerError("reconcile could not finish");
    let next = now + RECONCILE_EVERY;
    let mut task = NewTask::new(RECONCILE_KIND, json!({}), next);
    let day = next.as_second().div_euclid(86_400);
    task.dedupe_key = Some(format!("{RECONCILE_KIND}.{day}"));
    let mut conn = pool.acquire().await.map_err(|_| failed)?;
    enqueue(&mut conn, &task).await.map_err(|_| failed)?;
    drop(conn);
    reconcile(pool, stripe, id_secret, now)
        .await
        .map_err(|_| failed)?;
    Ok(())
}

/// One ledger line of a statement (FEE-030).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementLine {
    /// The entry type.
    pub kind: EntryKind,
    /// The job, if the entry has one.
    pub job: Option<Uuid>,
    /// Whole cents.
    pub amount_cents: i64,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// A business statement: its ledger lines in order and their totals, from the ledger only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    /// Every entry of the business in `[from, to)`, by ledger order.
    pub lines: Vec<StatementLine>,
    /// The lines folded into a [`MoneyView`].
    pub totals: MoneyView,
}

/// FEE-030, FEE-046, FEE-051: the owner money view over `[from, to)`, from the ledger only.
///
/// # Errors
/// [`MoneyEventError::Database`].
pub async fn money_view(
    pool: &PgPool,
    (from, to): (Timestamp, Timestamp),
) -> Result<MoneyView, MoneyEventError> {
    let rows = db(sqlx::query!(
        "SELECT kind, amount_cents FROM payments.ledger_entry
          WHERE recorded_at >= $1 AND recorded_at < $2",
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(pool)
    .await)?;
    let entries = rows
        .iter()
        .filter_map(|r| Some((kind_of(&r.kind)?, r.amount_cents)));
    Ok(MoneyView::from_entries(entries))
}

/// FEE-030: one business's statement over `[from, to)`, from the ledger only.
///
/// # Errors
/// [`MoneyEventError::Database`].
pub async fn business_statement(
    pool: &PgPool,
    business: Uuid,
    (from, to): (Timestamp, Timestamp),
) -> Result<Statement, MoneyEventError> {
    let rows = db(sqlx::query!(
        "SELECT kind, job_id, amount_cents, recorded_at FROM payments.ledger_entry
          WHERE business_id = $1 AND recorded_at >= $2 AND recorded_at < $3 ORDER BY seq",
        business,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(pool)
    .await)?;
    let lines: Vec<StatementLine> = rows
        .into_iter()
        .filter_map(|r| {
            Some(StatementLine {
                kind: kind_of(&r.kind)?,
                job: r.job_id,
                amount_cents: r.amount_cents,
                recorded_at: r.recorded_at.to_jiff(),
            })
        })
        .collect();
    let totals = MoneyView::from_entries(lines.iter().map(|l| (l.kind, l.amount_cents)));
    Ok(Statement { lines, totals })
}
