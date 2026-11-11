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
//! [`OPEN_CHARGE_ALERT`] naming only its row, marked `alerted_at` in the same transaction. A
//! retry raises no [`CHARGE_FAILED_ALERT`] or [`REFUND_FAILED_ALERT`] of its own, so a stuck row
//! alerts at most for the nights it is listed plus that one alert (D-2364).
//!
//! T334 (D-2304, D-2303): each run reads every charge created since the earlier of now less
//! [`LOOKBACK`] and the start of the last finished run (`payments.reconcile_checkpoint`) less
//! [`CATCH_UP_MARGIN`], with no cap, so a reconcile outage of any length skips no charge; a gap of
//! over [`GAP_ALERT_AFTER`] raises one information-only High [`RECONCILE_GAP_ALERT`]. The read goes
//! only through [`ReconcileStripe::list_charges_page`], [`PAGE_SIZE`] charges at a time: each page
//! is handled (every charge in its own short transaction) and its FEE-030 totals folded before the
//! next page is read, and the match is compared once at the end. A listed charge with an unsettled
//! `payments.reconcile_open` row first seen before now less [`LOOKBACK`] is left to the retry,
//! which reads it through `fetch_charge` under the same keys (D-3064). The checkpoint moves only
//! when a run finishes. The reconcile's own refund is booked by T333's
//! [`Refund::amount_against`](crate::refund_stripe::Refund::amount_against): another `usd` amount at
//! Stripe's amount with one High `payments.refund_amount_mismatch`, another currency at the charge
//! with one High `payments.refund_amount_unconfirmed` (D-3065).

use std::future::Future;

use askhava_adapters::api::{AdapterError, IdempotencyKey, PaymentsAdapter};
use askhava_core::api::{HandlerError, NewTask, enqueue};
use askhava_ops::api::{NewAlert, Severity, raise_alert, raise_alert_once};
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
use crate::refund_state::{PickRefundState, refund_state};
use crate::refund_stripe::{Refund, RefundAmount, RefundState, RefundStripe};
use crate::refunds::{
    REFUND_FAILED_ALERT, REFUND_MISMATCH_ALERT, REFUND_UNCONFIRMED_ALERT, request_refund,
};
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
/// Raised once per run, as information only, when the last finished run started more than
/// [`GAP_ALERT_AFTER`] ago; the run still reads every charge back to it (FEE-029, D-2304).
pub const RECONCILE_GAP_ALERT: &str = "payments.reconcile_gap";
/// The gap since the last finished run past which [`RECONCILE_GAP_ALERT`] is raised (D-2304).
pub const GAP_ALERT_AFTER: SignedDuration = SignedDuration::from_hours(2 * 24);
/// How far before the last finished run's start a run reads, when that is earlier than now less
/// [`LOOKBACK`] (D-2304).
pub const CATCH_UP_MARGIN: SignedDuration = SignedDuration::from_hours(24);
/// The most charges one page holds: Stripe's list limit (D-2304).
pub const PAGE_SIZE: usize = 100;
/// The subject kind of an alert about one pick.
const PICK_SUBJECT: &str = "payments.pick_intent";

/// One page of Stripe's charge list and the cursor of the next, `None` after the last (D-2304).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChargePage {
    /// The page's charges.
    pub charges: Vec<StripeCharge>,
    /// The cursor to pass for the next page; `None` when this page is the last.
    pub next: Option<String>,
}

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

    /// FEE-029, D-2304, D-3055: one page of the charges created at or after `since`: at most
    /// `limit` (1 to [`PAGE_SIZE`]) charges after the cursor `after`, with the next page's cursor.
    /// The provided body orders [`list_charges`](Self::list_charges) by charge ID: the cursor is
    /// the last ID of the previous page and a page holds the charges with a greater ID. T335's
    /// client overrides it with Stripe's paged `GET /v1/charges`, its own order and cursor.
    fn list_charges_page(
        &self,
        since: Timestamp,
        after: Option<&str>,
        limit: usize,
    ) -> impl Future<Output = Result<ChargePage, AdapterError>> + Send {
        let after = after.map(str::to_owned);
        let limit = limit.clamp(1, PAGE_SIZE);
        let all = self.list_charges(since);
        async move {
            let mut charges = all.await?;
            charges.retain(|c| after.as_deref().is_none_or(|a| c.id.as_str() > a));
            charges.sort_by(|a, b| a.id.cmp(&b.id));
            let next = (charges.len() > limit).then(|| charges[limit - 1].id.clone());
            charges.truncate(limit);
            Ok(ChargePage { charges, next })
        }
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

/// What a refund attempt came to: taken (with the cents booked), nothing owed any more, or still
/// open.
enum Sent {
    Refunded(i64),
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
    let subject = pick.map(|p| (PICK_SUBJECT, p));
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
/// stored and every stored one retried. T334 (D-2304): it reads from [`read_from`]'s time, page by
/// page, folds each page's FEE-030 totals before the next read, compares them once at the end and
/// only then moves the checkpoint to `now`.
///
/// # Errors
/// [`MoneyEventError::Stripe`] when a page cannot be read or Stripe repeats a cursor, else
/// `Database` when an alert, a stored charge, the match or the checkpoint cannot be read or
/// written. Any error leaves the checkpoint where it was.
pub async fn reconcile(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    now: Timestamp,
) -> Result<ReconcileReport, MoneyEventError> {
    let since = read_from(pool, now).await?;
    let mut report = ReconcileReport::default();
    let mut cursor: Option<String> = None;
    loop {
        let page = stripe
            .list_charges_page(since, cursor.as_deref(), PAGE_SIZE)
            .await
            .map_err(|_| MoneyEventError::Stripe)?;
        one_page(pool, stripe, id_secret, (&page.charges, now), &mut report).await?;
        match page.next {
            None => break,
            Some(next) if cursor.as_deref() == Some(next.as_str()) => {
                return Err(MoneyEventError::Stripe);
            }
            Some(next) => cursor = Some(next),
        }
    }
    let mut known = Vec::new();
    retry_open(pool, stripe, id_secret, (now, &mut report, &mut known)).await?;
    add_totals(pool, &known, &mut report).await?;
    report.open = alert_open(pool, now).await?;
    if !report.matched() {
        alert(&mut *db(pool.acquire().await)?, MISMATCH_ALERT, None).await?;
    }
    finish(pool, now).await?;
    Ok(report)
}

/// FEE-029, D-2304: the time this run reads from: the earlier of `now` less [`LOOKBACK`] and the
/// last finished run's start less [`CATCH_UP_MARGIN`], with no cap. A gap since that start of over
/// [`GAP_ALERT_AFTER`] raises one High [`RECONCILE_GAP_ALERT`], as information only.
async fn read_from(pool: &PgPool, now: Timestamp) -> Result<Timestamp, MoneyEventError> {
    let last = db(
        sqlx::query_scalar!("SELECT run_started_at FROM payments.reconcile_checkpoint")
            .fetch_optional(pool)
            .await,
    )?;
    let window = now - LOOKBACK;
    let Some(last) = last.map(|t| t.to_jiff()) else {
        return Ok(window);
    };
    if now.duration_since(last) > GAP_ALERT_AFTER {
        alert(&mut *db(pool.acquire().await)?, RECONCILE_GAP_ALERT, None).await?;
    }
    Ok(window.min(last - CATCH_UP_MARGIN))
}

/// D-2304: a finished run moves the checkpoint to its start, never backwards.
async fn finish(pool: &PgPool, now: Timestamp) -> Result<(), MoneyEventError> {
    db(sqlx::query!(
        "INSERT INTO payments.reconcile_checkpoint (id, run_started_at) VALUES (true, $1)
         ON CONFLICT (id) DO UPDATE SET run_started_at =
           GREATEST(reconcile_checkpoint.run_started_at, EXCLUDED.run_started_at)",
        now.to_sqlx() as _,
    )
    .execute(pool)
    .await)?;
    Ok(())
}

/// FEE-029, FEE-030, D-2304, D-3064: handles one page (at most [`PAGE_SIZE`] charges, each in its
/// own short transaction) and folds its totals into `report`. A charge whose unsettled
/// `payments.reconcile_open` row was first seen before `now` less [`LOOKBACK`] is only in the
/// catch-up range; it is left out of the page and not counted in `seen`, and the retry settles it.
async fn one_page(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    (charges, now): (&[StripeCharge], Timestamp),
    report: &mut ReconcileReport,
) -> Result<(), MoneyEventError> {
    let well: Vec<&StripeCharge> = charges.iter().filter(|c| c.is_well_formed()).collect();
    let carried = carried_over(pool, &well, now).await?;
    report.malformed += charges.len() - well.len();
    report.seen += charges.len() - carried.len();
    let mut known = Vec::new();
    for &charge in well.iter().filter(|c| !carried.contains(&c.id)) {
        let attempt = (charge, now, false);
        let open = settle(pool, stripe, id_secret, attempt, report, &mut known).await?;
        record(pool, charge, open, now).await?;
    }
    add_totals(pool, &known, report).await
}

/// D-3064: the IDs among `charges` with an unsettled `payments.reconcile_open` row first seen
/// before `now` less [`LOOKBACK`].
async fn carried_over(
    pool: &PgPool,
    charges: &[&StripeCharge],
    now: Timestamp,
) -> Result<Vec<String>, MoneyEventError> {
    let ids: Vec<String> = charges.iter().map(|c| c.id.clone()).collect();
    db(sqlx::query_scalar!(
        "SELECT stripe_charge_id FROM payments.reconcile_open
          WHERE settled_at IS NULL AND first_seen < $2 AND stripe_charge_id = ANY($1)",
        &ids,
        (now - LOOKBACK).to_sqlx() as _,
    )
    .fetch_all(pool)
    .await)
}

/// FEE-030, FEE-051, D-2304: adds Stripe's and the ledger's totals over `known` to the run's.
async fn add_totals(
    pool: &PgPool,
    known: &[StripeCharge],
    report: &mut ReconcileReport,
) -> Result<(), MoneyEventError> {
    if known.is_empty() {
        return Ok(());
    }
    let sum = |a: MoneyTotals, b: MoneyTotals| MoneyTotals {
        charged: a.charged + b.charged,
        refunded: a.refunded + b.refunded,
        disputed: a.disputed + b.disputed,
    };
    report.stripe = sum(report.stripe, MoneyTotals::of_stripe(known));
    report.ledger = sum(report.ledger, ledger_totals(pool, known).await?);
    Ok(())
}

/// Runs [`one_charge`] and says why the charge is still open, if it is: a failed write is counted,
/// alerted with [`CHARGE_FAILED_ALERT`] and open as [`OpenWhy::WriteFailed`] (D-1555, D-1872). A
/// `retry` of a stored row raises no per-charge alert: [`OPEN_CHARGE_ALERT`] is its one alert
/// (D-2364). A charge on a known pick joins `known` for the FEE-030 match.
async fn settle(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    attempt: (&StripeCharge, Timestamp, bool),
    report: &mut ReconcileReport,
    known: &mut Vec<StripeCharge>,
) -> Result<Option<OpenWhy>, MoneyEventError> {
    match one_charge(pool, stripe, id_secret, attempt, report).await {
        Ok((held, open)) => {
            known.extend(held);
            Ok(open)
        }
        Err(_) => {
            report.failed += 1;
            if !attempt.2 {
                alert(&mut *db(pool.acquire().await)?, CHARGE_FAILED_ALERT, None).await?;
            }
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
    (charge, now, retry): (&StripeCharge, Timestamp, bool),
    report: &mut ReconcileReport,
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
        Some(why) => {
            let attempt = (charge, now, retry);
            refund(pool, stripe, id_secret, attempt, (pick, why), report).await?
        }
        None => Sent::Nothing,
    };
    let open = match sent {
        Sent::Open(why) => Some(why),
        Sent::Refunded(cents) => {
            held.refunded_cents = cents;
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
/// `Unchanged` (a try in flight) is [`OpenWhy::RefundPending`], as is a `Failed` answer whose
/// refund row Stripe holds `pending` ([`refund_state`] `InFlight`, D-2364), and `Blocked`
/// (disputed) owes nothing more (D-1872). T066's `Refunded` is held at the amount T066 booked (its
/// latest refund entry via [`booked_refund`], Stripe's own `usd` amount since T333), not the charge (D-3067).
async fn refund(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    attempt: (&StripeCharge, Timestamp, bool),
    (pick, why): (Option<PickFacts>, RefundWhy),
    report: &mut ReconcileReport,
) -> Result<Sent, MoneyEventError> {
    let now = attempt.1;
    if let Some(facts) = pick.filter(|f| f.own_intent) {
        match request_refund(pool, stripe, id_secret, (facts.pick, LATE_TRIGGER), now).await {
            Err(MoneyEventError::NothingCharged) => {}
            Ok(RefundOutcome::Refunded(_)) => {
                report.refunded += 1;
                let mut conn = db(pool.acquire().await)?;
                let booked = booked_refund(&mut conn, facts.pick).await?;
                return Ok(Sent::Refunded(booked.unwrap_or(attempt.0.amount_cents)));
            }
            Ok(RefundOutcome::Failed) => {
                report.refund_failed += 1;
                return failed_why(pool, facts.pick).await;
            }
            Ok(RefundOutcome::Unchanged) => return Ok(Sent::Open(OpenWhy::RefundPending)),
            Ok(RefundOutcome::Blocked) => return Ok(Sent::Nothing),
            Err(e) => return Err(e),
        }
    }
    own_refund(pool, stripe, id_secret, attempt, (pick, why), report).await
}

/// D-3067: the amount of the pick's latest `refund` entry (the one T066 just booked at Stripe's own
/// `usd` amount), read the same way as T066's own lookup for a reversal (D-2303); `None` when
/// there is none.
async fn booked_refund(
    conn: &mut PgConnection,
    pick: Uuid,
) -> Result<Option<i64>, MoneyEventError> {
    db(sqlx::query_scalar!(
        "SELECT amount_cents FROM payments.ledger_entry
          WHERE kind = 'refund'
            AND (idempotency_key = 'refund.' || $1::uuid::text
                 OR idempotency_key LIKE 'refund.' || $1::uuid::text || '.%')
          ORDER BY seq DESC
          LIMIT 1",
        pick,
    )
    .fetch_optional(conn)
    .await)
}

/// D-2364: T066 answers `Failed` both for a refused refund and for one Stripe holds `pending`
/// (D-2067); the pick's refund row tells them apart.
async fn failed_why(pool: &PgPool, pick: Uuid) -> Result<Sent, MoneyEventError> {
    let state = refund_state(&mut *db(pool.acquire().await)?, pick).await?;
    Ok(Sent::Open(if state == PickRefundState::InFlight {
        OpenWhy::RefundPending
    } else {
        OpenWhy::RefundRefused
    }))
}

/// The reconcile's own refund of a charge T066 has no record of. The Stripe key names the charge,
/// the reason and the night, so a rerun the same night gets Stripe's first answer and a refused
/// refund (which Stripe would replay for the same key) is a new try the next night; a refund made
/// on a lost answer shows as refunded in the next list and is not sent again. A failure raises
/// [`REFUND_FAILED_ALERT`], except on a `retry` of a stored row, whose one alert is
/// [`OPEN_CHARGE_ALERT`] (D-2364); the ledger `refund` entry follows Stripe's `succeeded` only, and a
/// `pending` answer appends nothing and counts neither way (D-1555, D-1753). A `succeeded` refund
/// is booked by [`own_booked`] (T334, D-2303, D-3065).
async fn own_refund(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    (charge, now, retry): (&StripeCharge, Timestamp, bool),
    (pick, why): (Option<PickFacts>, RefundWhy),
    report: &mut ReconcileReport,
) -> Result<Sent, MoneyEventError> {
    let night = now.as_second().div_euclid(86_400).to_string();
    let key = IdempotencyKey::derive(
        id_secret,
        RECONCILE_ACTION,
        &[&charge.id, why.code(), &night],
    );
    let refund = match stripe.refund(&key, &charge.payment_intent).await {
        Ok(r) if r.state == RefundState::Succeeded => r,
        Ok(r) if matches!(r.state, RefundState::Pending | RefundState::RequiresAction) => {
            return Ok(Sent::Open(OpenWhy::RefundPending));
        }
        _ => {
            report.refund_failed += 1;
            if !retry {
                let mut conn = db(pool.acquire().await)?;
                alert(&mut conn, REFUND_FAILED_ALERT, pick.map(|p| p.pick)).await?;
            }
            return Ok(Sent::Open(OpenWhy::RefundRefused));
        }
    };
    report.refunded += 1;
    let (cents, flagged) = own_booked(&refund, charge.amount_cents);
    let mut tx = db(pool.begin().await)?;
    if let Some(facts) = pick {
        append(
            &mut tx,
            id_secret,
            charge,
            facts,
            (EntryKind::Refund, cents),
        )
        .await?;
    }
    if let Some(kind) = flagged {
        flag_refund(&mut tx, kind, pick).await?;
    }
    db(tx.commit().await)?;
    Ok(Sent::Refunded(cents))
}

/// FEE-026, FEE-028, D-2303, D-3065: the cents the reconcile books for its own succeeded refund
/// and the alert it raises, by T333's [`Refund::amount_against`]: another `usd` amount at Stripe's
/// amount with [`REFUND_MISMATCH_ALERT`]; another currency at the charge, never the foreign
/// figure, with [`REFUND_UNCONFIRMED_ALERT`]; the charge with none when full or not reported (the
/// next night's list or retry compares the ledger's refund with Stripe's refunded amount).
fn own_booked(refund: &Refund, charged: i64) -> (i64, Option<&'static str>) {
    match refund.amount_against(charged) {
        RefundAmount::Full | RefundAmount::Missing => (charged, None),
        RefundAmount::Differs(cents) => (cents, Some(REFUND_MISMATCH_ALERT)),
        RefundAmount::Foreign => (charged, Some(REFUND_UNCONFIRMED_ALERT)),
    }
}

/// D-2303, D-3065: one High refund-amount alert: on a known pick only while none is open for it
/// (as T333's), else with no subject.
async fn flag_refund(
    conn: &mut PgConnection,
    kind: &'static str,
    pick: Option<PickFacts>,
) -> Result<(), MoneyEventError> {
    let Some(facts) = pick else {
        return alert(conn, kind, None).await;
    };
    raise_alert_once(conn, kind, Severity::High, (PICK_SUBJECT, facts.pick))
        .await
        .map(drop)
        .map_err(|_| MoneyEventError::Database)
}

/// Each stored, unsettled charge not tried at `now`, as the run first read it (D-1872, D-3064).
async fn open_rows(pool: &PgPool, now: Timestamp) -> Result<Vec<StripeCharge>, MoneyEventError> {
    let rows = db(sqlx::query!(
        "SELECT stripe_charge_id, stripe_intent_id, pick_id, amount_cents, refunded_cents,
                fee_cents, balance_transaction
           FROM payments.reconcile_open WHERE settled_at IS NULL AND last_tried <> $1
          ORDER BY first_seen, id",
        now.to_sqlx() as _,
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

/// FEE-029, D-1872, D-3064: retries every unsettled stored charge this run's pages did not settle
/// or store (its `last_tried` is not `now`), whatever its age, from its stored facts with
/// Stripe's current refund and dispute state, through the listed charges' own path and keys, and
/// records what came of it.
async fn retry_open(
    pool: &PgPool,
    stripe: &impl ReconcileStripe,
    id_secret: &SecretString,
    (now, report, known): (Timestamp, &mut ReconcileReport, &mut Vec<StripeCharge>),
) -> Result<(), MoneyEventError> {
    for charge in &open_rows(pool, now).await? {
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
                    settle(pool, stripe, id_secret, (&fresh, now, true), report, known).await?
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
