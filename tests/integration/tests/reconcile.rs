//! TD-FEE-16 (reconcile and Stripe-match cases), TD-FEE-20 (the second kept charge) and TD-FEE-33
//! (the processing fee and its money-view line) (FEE-029, FEE-030, FEE-032, FEE-051; D-153, D-490,
//! D-1552 to D-1554): on a fresh database, the nightly reconcile auto-matches an unknown charge on
//! an open pick, refunds one on a closed job, a second kept charge for a pick and one naming no
//! pick, each once with one alert however often it runs; never refunds a disputed charge; records
//! each balance transaction's fee; alerts when the ledger and Stripe differ; and the money view and
//! statements come only from the ledger and match Stripe to the cent.
//!
//! Stripe is a scripted fake whose list reflects the refunds it took; the stripe-mock round trip
//! is T132's and the architecture case (only `payments` holds the adapter) T175's. Fake IDs only.

use std::sync::Mutex;

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_payments::api::{
    EntryKind, LedgerEntry, MISMATCH_ALERT, MoneyEventError, NewEntry, RECONCILE_KIND,
    ReconcileStripe, StripeCharge, UNKNOWN_CHARGE_ALERT, business_statement, ledger_append,
    money_view, reconcile, run_reconcile,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::json;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: Uuid = Uuid::from_u128(0xb067_0001);
const PICK: Uuid = Uuid::from_u128(0x067_0042);
const JOB: Uuid = Uuid::from_u128(0x067_0142);
const NOW: i64 = 1_793_900_000;

/// Stripe's charges; a refund marks its intent's charge refunded, unless `fail` is set.
#[derive(Default)]
struct FakeStripe {
    charges: Mutex<Vec<StripeCharge>>,
    refunds: Mutex<Vec<String>>,
    fail: Mutex<bool>,
    down: bool,
}

impl FakeStripe {
    fn with(charges: Vec<StripeCharge>) -> Self {
        Self {
            charges: Mutex::new(charges),
            ..Self::default()
        }
    }

    fn refunds(&self) -> Vec<String> {
        self.refunds.lock().expect("lock").clone()
    }
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no customer is made here")
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        panic!("nothing is charged here")
    }

    async fn fetch_payment_intent(&self, _: &str) -> Result<PaymentIntent, AdapterError> {
        panic!("no intent is read here")
    }

    async fn create_refund(
        &self,
        _: &IdempotencyKey,
        intent: &str,
    ) -> Result<String, AdapterError> {
        if *self.fail.lock().expect("lock") {
            return Err(AdapterError::NotSent);
        }
        self.refunds.lock().expect("lock").push(intent.to_owned());
        for c in self.charges.lock().expect("lock").iter_mut() {
            if c.payment_intent == intent {
                c.refunded_cents = c.amount_cents;
            }
        }
        Ok(format!("re_{}", &intent[3..]))
    }
}

impl ReconcileStripe for FakeStripe {
    async fn list_charges(&self, _: Timestamp) -> Result<Vec<StripeCharge>, AdapterError> {
        if self.down {
            return Err(AdapterError::NotSent);
        }
        Ok(self.charges.lock().expect("lock").clone())
    }
}

fn charge(n: u8, intent: &str, pick: Option<Uuid>) -> StripeCharge {
    StripeCharge {
        id: format!("ch_FakeT067n{n}"),
        payment_intent: intent.to_owned(),
        pick,
        amount_cents: 7500,
        refunded_cents: 0,
        dispute: None,
        balance_transaction: format!("txn_FakeT067n{n}"),
        fee_cents: 248,
    }
}

fn at(offset: i64) -> Timestamp {
    Timestamp::from_second(NOW + offset).expect("time")
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t067".to_owned())
}

/// One $75 pick on `pi_FakeT067`, ended (job closed) when `ended`.
async fn pick(pool: &PgPool, ended: bool) {
    sqlx::query(
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at, ended_at)
         VALUES ($1, $2, $3, 7500, 'pi_FakeT067', false, now(), CASE WHEN $4 THEN now() END)",
    )
    .bind(PICK)
    .bind(BUSINESS)
    .bind(JOB)
    .bind(ended)
    .execute(pool)
    .await
    .expect("seed pick");
}

async fn append(pool: &PgPool, kind: EntryKind, cents: i64, stripe_id: &str) {
    let key = format!("seed.{}.{stripe_id}", kind.code());
    let entry = LedgerEntry::new(NewEntry {
        kind,
        business: Some(BUSINESS),
        job: Some(JOB),
        amount_cents: cents,
        stripe_id: Some(stripe_id),
        idempotency_key: &key,
    })
    .expect("entry");
    let mut conn = pool.acquire().await.expect("conn");
    ledger_append(&mut conn, &entry).await.expect("append");
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn alerts(pool: &PgPool, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1")
        .bind(kind)
        .fetch_one(pool)
        .await
        .expect("alerts")
}

async fn kinds(pool: &PgPool) -> Vec<(String, i64)> {
    sqlx::query_as("SELECT kind, amount_cents FROM payments.ledger_entry ORDER BY seq")
        .fetch_all(pool)
        .await
        .expect("ledger")
}

/// TD-FEE-16 (FEE-029, FEE-051): an unknown charge on an open pick's own intent is matched to its
/// job with its processing fee, alerted once, never refunded; a second run changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_16_an_unknown_charge_on_an_open_pick_is_auto_matched(pool: PgPool) {
    pick(&pool, false).await;
    let stripe = FakeStripe::with(vec![charge(1, "pi_FakeT067", None)]);
    for run in 0..2 {
        let report = reconcile(&pool, &stripe, &secret(), at(run))
            .await
            .expect("run");
        assert!(report.matched(), "{report:?}");
        assert_eq!(report.stripe.charged, 7500);
        assert_eq!(report.auto_matched, 1 - usize::try_from(run).expect("run"));
    }
    let expected = vec![
        ("charge".to_owned(), 7500),
        ("processing-fee".to_owned(), 248),
    ];
    assert_eq!(kinds(&pool).await, expected);
    assert!(stripe.refunds().is_empty());
    assert_eq!(alerts(&pool, UNKNOWN_CHARGE_ALERT).await, 1);
    assert_eq!(alerts(&pool, MISMATCH_ALERT).await, 0);
}

/// TD-FEE-16 (FEE-029): an unknown charge on a closed job is always refunded, once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_16_a_charge_on_a_closed_job_is_refunded_once(pool: PgPool) {
    pick(&pool, true).await;
    let stripe = FakeStripe::with(vec![charge(1, "pi_FakeT067", Some(PICK))]);
    for run in 0..3 {
        let report = reconcile(&pool, &stripe, &secret(), at(run))
            .await
            .expect("run");
        assert!(report.matched(), "{report:?}");
    }
    assert_eq!(stripe.refunds(), ["pi_FakeT067"]);
    let refunds = "SELECT count(*) FROM payments.ledger_entry
                    WHERE kind = 'refund' AND idempotency_key = 'refund.00000000-0000-0000-0000-000000670042'";
    assert_eq!(count(&pool, refunds).await, 1);
    let reason =
        "SELECT count(*) FROM payments.reconcile_charge WHERE refund_reason = 'closed-job'";
    assert_eq!(count(&pool, reason).await, 1);
    assert_eq!(alerts(&pool, UNKNOWN_CHARGE_ALERT).await, 1);
}

/// TD-FEE-20 (FEE-037, D-153): two kept charges for one pick: the second is refunded once and the
/// pick's own charge stays.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_20_a_second_kept_charge_is_refunded_once(pool: PgPool) {
    pick(&pool, false).await;
    append(&pool, EntryKind::Charge, 7500, "ch_FakeT067n1").await;
    let first = charge(1, "pi_FakeT067", Some(PICK));
    let second = charge(2, "pi_FakeT067second", Some(PICK));
    let stripe = FakeStripe::with(vec![first, second]);
    for run in 0..2 {
        let report = reconcile(&pool, &stripe, &secret(), at(run))
            .await
            .expect("run");
        assert!(report.matched(), "{report:?}");
        assert_eq!(report.stripe.charged, 15_000);
        assert_eq!(report.stripe.refunded, 7500);
    }
    assert_eq!(stripe.refunds(), ["pi_FakeT067second"]);
    let reason = "SELECT count(*) FROM payments.reconcile_charge
                   WHERE refund_reason = 'second-charge' AND stripe_charge_id = 'ch_FakeT067n2'";
    assert_eq!(count(&pool, reason).await, 1);
    assert_eq!(alerts(&pool, UNKNOWN_CHARGE_ALERT).await, 1);
}

/// TD-FEE-16 (FEE-029): a charge naming no pick is refunded once and alerted, outside the ledger.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_16_a_charge_naming_no_pick_is_refunded(pool: PgPool) {
    let stripe = FakeStripe::with(vec![charge(3, "pi_FakeT067orphan", None)]);
    for run in 0..2 {
        let report = reconcile(&pool, &stripe, &secret(), at(run))
            .await
            .expect("run");
        assert!(report.matched());
        assert_eq!(report.stripe.charged, 0);
    }
    assert_eq!(stripe.refunds(), ["pi_FakeT067orphan"]);
    assert!(kinds(&pool).await.is_empty());
    let row = "SELECT count(*) FROM payments.reconcile_charge
                WHERE pick_id IS NULL AND refund_reason = 'no-pick'";
    assert_eq!(count(&pool, row).await, 1);
    assert_eq!(alerts(&pool, UNKNOWN_CHARGE_ALERT).await, 1);
}

/// TD-FEE-16 (FEE-026, FEE-029, FEE-030): a disputed charge on a closed job is never refunded; its
/// dispute matches when the ledger has it and raises the mismatch alert when it does not.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_16_a_disputed_charge_is_matched_never_refunded(pool: PgPool) {
    pick(&pool, true).await;
    let mut disputed = charge(1, "pi_FakeT067", Some(PICK));
    disputed.dispute = Some(("dp_FakeT067".to_owned(), 7500));
    let stripe = FakeStripe::with(vec![disputed]);
    let report = reconcile(&pool, &stripe, &secret(), at(0))
        .await
        .expect("run");
    assert!(!report.matched());
    assert_eq!(alerts(&pool, MISMATCH_ALERT).await, 1);
    append(&pool, EntryKind::Dispute, 7500 + 1500, "dp_FakeT067").await;
    let report = reconcile(&pool, &stripe, &secret(), at(1))
        .await
        .expect("run");
    assert!(report.matched(), "{report:?}");
    assert_eq!(report.ledger.disputed, 7500);
    assert!(stripe.refunds().is_empty());
}

/// TD-FEE-16 (FEE-029): a refused refund leaves no ledger refund and goes again next run; a list
/// Stripe cannot give is `Stripe`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_16_a_failed_refund_is_sent_again_next_run(pool: PgPool) {
    pick(&pool, true).await;
    let stripe = FakeStripe::with(vec![charge(1, "pi_FakeT067", Some(PICK))]);
    *stripe.fail.lock().expect("lock") = true;
    let report = reconcile(&pool, &stripe, &secret(), at(0))
        .await
        .expect("run");
    assert_eq!((report.refunded, report.refund_failed), (0, 1));
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) FROM payments.ledger_entry WHERE kind = 'refund'"
        )
        .await,
        0
    );
    *stripe.fail.lock().expect("lock") = false;
    let report = reconcile(&pool, &stripe, &secret(), at(1))
        .await
        .expect("run");
    assert_eq!((report.refunded, report.refund_failed), (1, 0));
    assert!(report.matched(), "{report:?}");
    let down = FakeStripe {
        down: true,
        ..FakeStripe::default()
    };
    let refused = reconcile(&pool, &down, &secret(), at(2)).await;
    assert_eq!(refused, Err(MoneyEventError::Stripe));
}

/// TD-FEE-33 (FEE-051), TD-FEE-16 (FEE-030): the processing fee is read per balance transaction,
/// the money view shows it as its own line, and the statement and view come from the ledger only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_33_the_money_view_shows_the_fee_line(pool: PgPool) {
    pick(&pool, false).await;
    let stripe = FakeStripe::with(vec![charge(1, "pi_FakeT067", None)]);
    reconcile(&pool, &stripe, &secret(), at(0))
        .await
        .expect("run");
    let window = (
        at(-86_400),
        Timestamp::now() + SignedDuration::from_hours(1),
    );
    let view = money_view(&pool, window).await.expect("view");
    assert_eq!((view.charges, view.processing_fees), (7500, 248));
    assert_eq!(view.net(), 7500 - 248);
    let statement = business_statement(&pool, BUSINESS, window)
        .await
        .expect("statement");
    let lines: Vec<(EntryKind, i64)> = statement
        .lines
        .iter()
        .map(|l| (l.kind, l.amount_cents))
        .collect();
    assert_eq!(
        lines,
        [(EntryKind::Charge, 7500), (EntryKind::ProcessingFee, 248)]
    );
    assert_eq!(statement.totals, view);
    let other = business_statement(&pool, Uuid::from_u128(1), window)
        .await
        .expect("other");
    assert!(other.lines.is_empty());
}

/// FEE-029: the nightly task runs and queues the next night once, however often it runs.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_nightly_task_queues_the_next_run_once(pool: PgPool) {
    let stripe = FakeStripe::default();
    for _ in 0..2 {
        run_reconcile(&pool, &stripe, &secret(), &json!({}), at(0))
            .await
            .expect("task");
    }
    let queued = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(RECONCILE_KIND)
        .fetch_one(&pool)
        .await
        .expect("tasks");
    assert_eq!(queued, 1);
    let down = FakeStripe {
        down: true,
        ..FakeStripe::default()
    };
    assert!(
        run_reconcile(&pool, &down, &secret(), &json!({}), at(0))
            .await
            .is_err()
    );
}
