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
//!
//! T258 (D-1753, D-2067): the reconcile's own refund reads Stripe's answer through
//! `RefundStripe` (every `ReconcileStripe` is one): only `succeeded` appends the `refund` entry and
//! counts as refunded; `pending` or `requires_action` appends nothing (the next night's list shows
//! it once Stripe finishes); `failed` or `canceled` is a failure with the alert. A
//! `refund-reversal` entry counts against the refunds in the match and in `ledger_facts`.
//!
//! T288 (D-1872): a charge a run reads but cannot settle (its writes failed, its refund was refused
//! or is still pending, or its ledger charge and refund differ from Stripe's) is stored in
//! `payments.reconcile_open` with the facts read, coded by [`OpenWhy`]. Every run then retries
//! each unsettled row that was not in its list, whatever its age: it reads the charge's current
//! refund and dispute state through [`ReconcileStripe::fetch_charge`], then plans it like a listed
//! charge under the same ledger and refund keys, so a retry never doubles an entry or a refund and a
//! charge refunded or disputed since is settled by that state. A settled row gets `settled_at` and
//! is never retried; a row open [`OPEN_ALERT_AFTER`] after `first_seen` raises one High
//! [`OPEN_CHARGE_ALERT`] naming only its row, marked `alerted_at` in the same transaction.

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
use crate::ledger::{EntryKind, LedgerEntry, NewEntry, REFUND_REVERSAL_CODE, ledger_append};
use crate::models::{
    MoneyEventError, MoneyTotals, MoneyView, PickFacts, RefundOutcome, RefundWhy, StripeCharge,
    plan_charge,
};
use crate::refund_stripe::{RefundState, RefundStripe};
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
/// Raised once per `payments.reconcile_open` row still unsettled [`OPEN_ALERT_AFTER`] after it
/// was first seen; its subject is the row (OPS-004's ledger difference, OPS-029, D-1872).
pub const OPEN_CHARGE_ALERT: &str = "payments.reconcile_open";
/// How long a stored charge may stay unsettled before its one alert (D-1872).
pub const OPEN_ALERT_AFTER: SignedDuration = SignedDuration::from_hours(7 * 24);

/// The Stripe reads the reconcile needs; T132's client implements it (FEE-029, FEE-051).
pub trait ReconcileStripe: PaymentsAdapter {
    /// Succeeded charges created at or after `since`, from Stripe's event list, each with its
    /// balance transaction's processing fee, refunded amount and dispute.
    fn list_charges(
        &self,
        since: Timestamp,
    ) -> impl Future<Output = Result<Vec<StripeCharge>, AdapterError>> + Send;

    /// One charge as Stripe holds it now, by its `ch_…` ID, or `None` when Stripe has no such
    /// charge; the retry of a stored charge reads its refund and dispute state here (D-1872).
    /// The provided body searches [`list_charges`](Self::list_charges) from the start of time,
    /// which suits a fake; T132's client overrides it with Stripe's retrieve-a-charge call.
    fn fetch_charge(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<Option<StripeCharge>, AdapterError>> + Send {
        let id = id.to_owned();
        let all = self.list_charges(Timestamp::MIN);
        async move { Ok(all.await?.into_iter().find(|c| c.id == id)) }
    }
}

/// Why a charge the reconcile read is still open, stored as `payments.reconcile_open.last_code`
/// (FEE-029, D-1872).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenWhy {
    /// A database write for the charge failed (for example a reused ledger key).
    WriteFailed,
    /// Stripe refused, failed or canceled the refund the charge is owed.
    RefundRefused,
    /// The refund is in flight: Stripe holds it pending, or T066's try is unanswered.
    RefundPending,
    /// The ledger's charge or refund for the charge differs from Stripe's.
    Mismatch,
    /// Stripe has no such charge any more.
    NotFound,
    /// Stripe could not be read for the charge, or answered with a malformed charge.
    StripeUnreadable,
}

impl OpenWhy {
    /// The stored code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::WriteFailed => "write-failed",
            Self::RefundRefused => "refund-refused",
            Self::RefundPending => "refund-pending",
            Self::Mismatch => "mismatch",
            Self::NotFound => "not-found",
            Self::StripeUnreadable => "stripe-unreadable",
        }
    }
}

/// What a refund attempt came to: taken, nothing owed any more, or still open.
enum Sent {
    Refunded,
    Nothing,
    Open(OpenWhy),
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
    /// Stored unsettled charges from earlier runs, not in this run's list, that it retried
    /// (D-1872).
    pub retried: usize,
    /// `payments.reconcile_open` rows still unsettled when the run ended (D-1872).
    pub open: usize,
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

/// FEE-029, FEE-030, FEE-051, D-1872: one reconcile run (see the module comment). A malformed
/// charge is counted and raises [`MISMATCH_ALERT`]; a charge whose writes fail raises
/// [`CHARGE_FAILED_ALERT`] and the run goes on to the next (D-1555); a charge left unsettled is
/// stored and every stored one retried.
///
/// # Errors
/// [`MoneyEventError::Stripe`] when the list cannot be read, else `Database` when an alert, a
/// stored charge or the match cannot be read or written.
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
    let listed: Vec<&StripeCharge> = charges.iter().filter(|c| c.is_well_formed()).collect();
    for &charge in &listed {
        let open = settle(
            pool,
            stripe,
            id_secret,
            (charge, now),
            &mut report,
            &mut known,
        )
        .await?;
        record(pool, charge, open, now).await?;
    }
    let run = (now, &mut report, &mut known);
    retry_open(pool, stripe, id_secret, &listed, run).await?;
    report.open = alert_open(pool, now).await?;
    report.stripe = MoneyTotals::of_stripe(&known);
    report.ledger = ledger_totals(pool, &known).await?;
    if !report.matched() {
        alert(&mut *db(pool.acquire().await)?, MISMATCH_ALERT, None).await?;
    }
    Ok(report)
}

/// Runs [`one_charge`] and says why the charge is still open, if it is: a failed write is counted,
/// alerted with [`CHARGE_FAILED_ALERT`] and open as [`OpenWhy::WriteFailed`] (D-1555, D-1872). A
/// charge on a known pick joins `known` for the FEE-030 match.
async fn settle(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    (charge, now): (&StripeCharge, Timestamp),
    report: &mut ReconcileReport,
    known: &mut Vec<StripeCharge>,
) -> Result<Option<OpenWhy>, MoneyEventError> {
    match one_charge(pool, stripe, id_secret, charge, report, now).await {
        Ok((held, open)) => {
            known.extend(held);
            Ok(open)
        }
        Err(_) => {
            report.failed += 1;
            alert(&mut *db(pool.acquire().await)?, CHARGE_FAILED_ALERT, None).await?;
            Ok(Some(OpenWhy::WriteFailed))
        }
    }
}

/// Plans and appends one charge, then sends its refund after the commit. When the charge names a
/// known pick, returns it as Stripe now holds it (refunded if this run refunded it), for the
/// FEE-030 match, with why it is still open, if it is: a refund not taken, else its own ledger
/// charge and refund differing from Stripe's (D-1872).
async fn one_charge(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    charge: &StripeCharge,
    report: &mut ReconcileReport,
    now: Timestamp,
) -> Result<(Option<StripeCharge>, Option<OpenWhy>), MoneyEventError> {
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
    let sent = match plan.refund {
        Some(why) => refund(pool, stripe, id_secret, (charge, now), (pick, why), report).await?,
        None => Sent::Nothing,
    };
    let open = match sent {
        Sent::Open(why) => Some(why),
        Sent::Refunded => {
            held.refunded_cents = held.amount_cents;
            None
        }
        Sent::Nothing => None,
    };
    let Some(_) = pick else {
        return Ok((None, open));
    };
    if open.is_none() && !ledger_agrees(pool, &held).await? {
        return Ok((Some(held), Some(OpenWhy::Mismatch)));
    }
    Ok((Some(held), open))
}

/// The ledger's gross charge and refund for one charge equal Stripe's; disputes are the webhook's
/// (`record_dispute`) and stay in the run-wide FEE-030 match only (D-1872).
async fn ledger_agrees(pool: &PgPool, held: &StripeCharge) -> Result<bool, MoneyEventError> {
    let ledger = ledger_totals(pool, std::slice::from_ref(held)).await?;
    let stripe = MoneyTotals::of_stripe([held]);
    Ok(ledger.charged == stripe.charged && ledger.refunded == stripe.refunded)
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

/// Whether the ledger already holds this charge's `charge` entry and a `refund` of its intent
/// not reversed since (D-1753).
async fn ledger_facts(
    conn: &mut PgConnection,
    charge: &StripeCharge,
) -> Result<(bool, bool), MoneyEventError> {
    let row = db(sqlx::query!(
        "SELECT bool_or(kind = 'charge' AND stripe_id = $1) AS has_charge,
                count(*) FILTER (WHERE kind = 'refund' AND stripe_id = $2)
                  > count(*) FILTER (WHERE kind = 'refund-reversal' AND stripe_id = $2)
                  AS has_refund
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
/// when T066 has the pick's evidence record; otherwise [`own_refund`] sends it. T066's
/// `Unchanged` (a try in flight) is [`OpenWhy::RefundPending`] and `Blocked` (disputed) owes
/// nothing more (D-1872).
async fn refund(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    (charge, now): (&StripeCharge, Timestamp),
    (pick, why): (Option<PickFacts>, RefundWhy),
    report: &mut ReconcileReport,
) -> Result<Sent, MoneyEventError> {
    if let Some(facts) = pick.filter(|f| f.own_intent) {
        match request_refund(pool, stripe, id_secret, (facts.pick, LATE_TRIGGER), now).await {
            Err(MoneyEventError::NothingCharged) => {}
            Ok(RefundOutcome::Refunded(_)) => {
                report.refunded += 1;
                return Ok(Sent::Refunded);
            }
            Ok(RefundOutcome::Failed) => {
                report.refund_failed += 1;
                return Ok(Sent::Open(OpenWhy::RefundRefused));
            }
            Ok(RefundOutcome::Unchanged) => return Ok(Sent::Open(OpenWhy::RefundPending)),
            Ok(RefundOutcome::Blocked) => return Ok(Sent::Nothing),
            Err(e) => return Err(e),
        }
    }
    own_refund(pool, stripe, id_secret, (charge, now), (pick, why), report).await
}

/// The reconcile's own refund of a charge T066 has no record of. The Stripe key names the charge,
/// the reason and the night, so a rerun the same night gets Stripe's first answer and a refused
/// refund (which Stripe would replay for the same key) is a new try the next night; a refund made
/// on a lost answer shows as refunded in the next list and is not sent again. A failure raises
/// [`REFUND_FAILED_ALERT`]; the ledger `refund` entry follows Stripe's `succeeded` only, and a
/// `pending` answer appends nothing and counts neither way (D-1555, D-1753).
async fn own_refund(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    (charge, now): (&StripeCharge, Timestamp),
    (pick, why): (Option<PickFacts>, RefundWhy),
    report: &mut ReconcileReport,
) -> Result<Sent, MoneyEventError> {
    let night = now.as_second().div_euclid(86_400).to_string();
    let key = IdempotencyKey::derive(
        id_secret,
        RECONCILE_ACTION,
        &[&charge.id, why.code(), &night],
    );
    let state = stripe
        .refund(&key, &charge.payment_intent)
        .await
        .map(|r| r.state);
    if matches!(
        state,
        Ok(RefundState::Pending | RefundState::RequiresAction)
    ) {
        return Ok(Sent::Open(OpenWhy::RefundPending));
    }
    if state != Ok(RefundState::Succeeded) {
        report.refund_failed += 1;
        let mut conn = db(pool.acquire().await)?;
        alert(&mut conn, REFUND_FAILED_ALERT, pick.map(|p| p.pick)).await?;
        return Ok(Sent::Open(OpenWhy::RefundRefused));
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
    Ok(Sent::Refunded)
}

/// A stored, unsettled charge as the run first read it (D-1872).
async fn open_rows(pool: &PgPool) -> Result<Vec<StripeCharge>, MoneyEventError> {
    let rows = db(sqlx::query!(
        "SELECT stripe_charge_id, stripe_intent_id, pick_id, amount_cents, refunded_cents,
                fee_cents, balance_transaction
           FROM payments.reconcile_open WHERE settled_at IS NULL ORDER BY first_seen, id"
    )
    .fetch_all(pool)
    .await)?;
    Ok(rows
        .into_iter()
        .map(|r| StripeCharge {
            id: r.stripe_charge_id,
            payment_intent: r.stripe_intent_id,
            pick: r.pick_id,
            amount_cents: r.amount_cents,
            refunded_cents: r.refunded_cents,
            dispute: None,
            balance_transaction: r.balance_transaction,
            fee_cents: r.fee_cents,
        })
        .collect())
}

/// FEE-029, D-1872: retries every unsettled stored charge this run's list did not hold, whatever
/// its age, from its stored facts with Stripe's current refund and dispute state, through the
/// listed charges' own path and keys, and records what came of it.
async fn retry_open(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    listed: &[&StripeCharge],
    (now, report, known): (Timestamp, &mut ReconcileReport, &mut Vec<StripeCharge>),
) -> Result<(), MoneyEventError> {
    let stored = open_rows(pool).await?;
    for charge in stored
        .iter()
        .filter(|s| listed.iter().all(|c| c.id != s.id))
    {
        report.retried += 1;
        let open = match stripe.fetch_charge(&charge.id).await {
            Err(_) => Some(OpenWhy::StripeUnreadable),
            Ok(None) => Some(OpenWhy::NotFound),
            Ok(Some(current)) => {
                let fresh = StripeCharge {
                    refunded_cents: current.refunded_cents,
                    dispute: current.dispute,
                    ..charge.clone()
                };
                if fresh.is_well_formed() {
                    settle(pool, stripe, id_secret, (&fresh, now), report, known).await?
                } else {
                    Some(OpenWhy::StripeUnreadable)
                }
            }
        };
        record(pool, charge, open, now).await?;
    }
    Ok(())
}

/// D-1872: stores a charge left open (a new row, or the next try of its row, reopened if it had
/// settled), or marks its unsettled row settled. A settled charge with no row writes nothing.
async fn record(
    pool: &PgPool,
    charge: &StripeCharge,
    open: Option<OpenWhy>,
    now: Timestamp,
) -> Result<(), MoneyEventError> {
    let Some(why) = open else {
        db(sqlx::query!(
            "UPDATE payments.reconcile_open
                SET settled_at = $2, last_tried = $2, tries = tries + 1
              WHERE stripe_charge_id = $1 AND settled_at IS NULL",
            charge.id,
            now.to_sqlx() as _,
        )
        .execute(pool)
        .await)?;
        return Ok(());
    };
    db(sqlx::query!(
        "INSERT INTO payments.reconcile_open (stripe_charge_id, stripe_intent_id, pick_id,
           amount_cents, refunded_cents, fee_cents, balance_transaction, first_seen, last_tried,
           tries, last_code)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8, 1, $9)
         ON CONFLICT (stripe_charge_id) DO UPDATE
            SET last_tried = EXCLUDED.last_tried, tries = reconcile_open.tries + 1,
                last_code = EXCLUDED.last_code, settled_at = NULL",
        charge.id,
        charge.payment_intent,
        charge.pick,
        charge.amount_cents,
        charge.refunded_cents,
        charge.fee_cents,
        charge.balance_transaction,
        now.to_sqlx() as _,
        why.code(),
    )
    .execute(pool)
    .await)?;
    Ok(())
}

/// D-1872, OPS-004, OPS-029: in one transaction, marks `alerted_at` on each unsettled row first
/// seen [`OPEN_ALERT_AFTER`] or more ago and not yet alerted, raises one High
/// [`OPEN_CHARGE_ALERT`] per row naming only the row, and counts the rows still open.
async fn alert_open(pool: &PgPool, now: Timestamp) -> Result<usize, MoneyEventError> {
    let mut tx = db(pool.begin().await)?;
    let due = db(sqlx::query_scalar!(
        "UPDATE payments.reconcile_open SET alerted_at = $1
          WHERE settled_at IS NULL AND alerted_at IS NULL AND first_seen <= $2
          RETURNING id",
        now.to_sqlx() as _,
        (now - OPEN_ALERT_AFTER).to_sqlx() as _,
    )
    .fetch_all(&mut *tx)
    .await)?;
    for id in due {
        let row = NewAlert {
            kind: OPEN_CHARGE_ALERT,
            severity: Severity::High,
            subject: Some((OPEN_CHARGE_ALERT, id)),
        };
        raise_alert(&mut tx, &row)
            .await
            .map_err(|_| MoneyEventError::Database)?;
    }
    let open = db(sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM payments.reconcile_open WHERE settled_at IS NULL"
    )
    .fetch_one(&mut *tx)
    .await)?;
    db(tx.commit().await)?;
    Ok(usize::try_from(open).unwrap_or(usize::MAX))
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
             OR (kind IN ('refund', 'refund-reversal') AND stripe_id = ANY($2))
             OR (kind = 'dispute' AND stripe_id = ANY($3))",
        &ids,
        &intents,
        &disputes,
    )
    .fetch_all(pool)
    .await)?;
    let entries = rows.iter().filter_map(|r| fold(&r.kind, r.amount_cents));
    Ok(MoneyView::from_entries(entries).gross())
}

/// A stored entry as the money view folds it: its kind and cents, a `refund-reversal` as a
/// negative refund (FEE-028, FEE-030, D-1753, D-2068); `None` for an unknown code.
fn fold(code: &str, cents: i64) -> Option<(EntryKind, i64)> {
    if code == REFUND_REVERSAL_CODE {
        return Some((EntryKind::Refund, -cents));
    }
    EntryKind::ALL
        .into_iter()
        .find(|k| k.code() == code)
        .map(|k| (k, cents))
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
    /// Whole cents; a `refund-reversal` is a `Refund` line with negative cents (D-2068).
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
    let entries = rows.iter().filter_map(|r| fold(&r.kind, r.amount_cents));
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
            let (kind, amount_cents) = fold(&r.kind, r.amount_cents)?;
            Some(StatementLine {
                kind,
                job: r.job_id,
                amount_cents,
                recorded_at: r.recorded_at.to_jiff(),
            })
        })
        .collect();
    let totals = MoneyView::from_entries(lines.iter().map(|l| (l.kind, l.amount_cents)));
    Ok(Statement { lines, totals })
}
