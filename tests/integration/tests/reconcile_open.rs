//! TD-FEE-49 (FEE-029, FEE-051; D-1872): the nightly reconcile keeps every Stripe charge it read
//! but could not settle in `payments.reconcile_open` and retries it every night, whatever its age:
//! a charge whose ledger append failed settles the next night with one `charge` entry; a charge
//! left open is retried on night 10, after it left the 7-day list, and settles with one refund; a
//! charge refunded or disputed at Stripe since it was stored is settled by that state with no
//! refund sent; a row open 7 days raises one High alert naming only the row (`alerted_at`); a
//! settled row is never retried; the report counts open rows; no row holds a contact detail.
//!
//! Stripe is a scripted fake whose list holds only charges created in the run's window (as the
//! 7-day `LOOKBACK` read does), whose `fetch_charge` reads any charge and counts the reads, and
//! which answers a repeated idempotency key with its stored first answer. Fake IDs only.

use std::collections::HashMap;
use std::sync::Mutex;

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_payments::api::{
    CHARGE_FAILED_ALERT, OPEN_CHARGE_ALERT, ReconcileReport, ReconcileStripe, StripeCharge,
    reconcile,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: Uuid = Uuid::from_u128(0xb288_0001);
const PICK: Uuid = Uuid::from_u128(0x288_0042);
const JOB: Uuid = Uuid::from_u128(0x288_0142);
const NOW: i64 = 1_794_000_000;
const DAY: i64 = 86_400;

/// Charges with their creation second; refunds Stripe took; each key's first answer.
#[derive(Default)]
struct FakeStripe {
    charges: Mutex<Vec<(i64, StripeCharge)>>,
    refunds: Mutex<Vec<String>>,
    answers: Mutex<HashMap<String, Option<String>>>,
    fail: Mutex<bool>,
    fetched: Mutex<Vec<String>>,
    fetch_down: Mutex<bool>,
}

impl FakeStripe {
    fn with(charges: Vec<StripeCharge>) -> Self {
        let dated = charges.into_iter().map(|c| (NOW, c)).collect();
        Self {
            charges: Mutex::new(dated),
            ..Self::default()
        }
    }

    fn set_fail(&self, fail: bool) {
        *self.fail.lock().expect("lock") = fail;
    }

    fn refunds(&self) -> Vec<String> {
        self.refunds.lock().expect("lock").clone()
    }

    fn fetches(&self) -> usize {
        self.fetched.lock().expect("lock").len()
    }

    fn change(&self, id: &str, edit: impl Fn(&mut StripeCharge)) {
        for (_, c) in self.charges.lock().expect("lock").iter_mut() {
            if c.id == id {
                edit(c);
            }
        }
    }

    fn refund_now(&self, intent: &str) -> Option<String> {
        if *self.fail.lock().expect("lock") {
            return None;
        }
        self.refunds.lock().expect("lock").push(intent.to_owned());
        for (_, c) in self.charges.lock().expect("lock").iter_mut() {
            if c.payment_intent == intent {
                c.refunded_cents = c.amount_cents;
            }
        }
        Some(format!("re_{}", &intent[3..]))
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
        key: &IdempotencyKey,
        intent: &str,
    ) -> Result<String, AdapterError> {
        let mut answers = self.answers.lock().expect("lock");
        let answer = answers
            .entry(key.as_str().to_owned())
            .or_insert_with(|| self.refund_now(intent))
            .clone();
        answer.ok_or(AdapterError::Vendor {
            status: 400,
            code: None,
        })
    }
}

impl ReconcileStripe for FakeStripe {
    async fn list_charges(&self, since: Timestamp) -> Result<Vec<StripeCharge>, AdapterError> {
        let all = self.charges.lock().expect("lock");
        let since = since.as_second();
        Ok(all
            .iter()
            .filter(|(at, _)| *at >= since)
            .map(|(_, c)| c.clone())
            .collect())
    }

    async fn fetch_charge(&self, id: &str) -> Result<Option<StripeCharge>, AdapterError> {
        self.fetched.lock().expect("lock").push(id.to_owned());
        if *self.fetch_down.lock().expect("lock") {
            return Err(AdapterError::NotSent);
        }
        let all = self.charges.lock().expect("lock");
        Ok(all.iter().map(|(_, c)| c).find(|c| c.id == id).cloned())
    }
}

fn charge(n: u8, intent: &str, pick: Option<Uuid>) -> StripeCharge {
    StripeCharge {
        id: format!("ch_FakeT288n{n}"),
        payment_intent: intent.to_owned(),
        pick,
        amount_cents: 7500,
        refunded_cents: 0,
        dispute: None,
        balance_transaction: format!("txn_FakeT288n{n}"),
        fee_cents: 248,
    }
}

fn night(n: i64) -> Timestamp {
    Timestamp::from_second(NOW + n * DAY).expect("time")
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t288".to_owned())
}

/// One open $75 pick on `pi_FakeT288`.
async fn pick(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at)
         VALUES ($1, $2, $3, 7500, 'pi_FakeT288', false, now())",
    )
    .bind(PICK)
    .bind(BUSINESS)
    .bind(JOB)
    .execute(pool)
    .await
    .expect("seed pick");
}

async fn run(pool: &PgPool, stripe: &FakeStripe, n: i64) -> ReconcileReport {
    reconcile(pool, stripe, &secret(), night(n))
        .await
        .expect("run")
}

async fn scalar(pool: &PgPool, sql: &'static str, bind: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(bind)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn entries(pool: &PgPool, kind: &str) -> i64 {
    let sql = "SELECT count(*) FROM payments.ledger_entry WHERE kind = $1";
    scalar(pool, sql, kind).await
}

async fn alerts(pool: &PgPool, kind: &str) -> i64 {
    scalar(pool, "SELECT count(*) FROM ops.alert WHERE kind = $1", kind).await
}

/// The stored row of a charge: tries, last code, settled.
async fn row(pool: &PgPool, id: &str) -> (i32, String, bool) {
    sqlx::query_as(
        "SELECT tries, last_code, settled_at IS NOT NULL FROM payments.reconcile_open
          WHERE stripe_charge_id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("row")
}

/// TD-FEE-49 (FEE-029): a charge whose ledger append fails on night 1 is stored, alerted as a
/// failed charge, and settled on night 2 with exactly one `charge` entry.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_49_a_failed_append_is_stored_and_settled_the_next_night(pool: PgPool) {
    pick(&pool).await;
    let stripe = FakeStripe::with(vec![charge(1, "pi_FakeT288", None)]);
    let block =
        "ALTER TABLE payments.ledger_entry ADD CONSTRAINT t288_block CHECK (kind <> 'charge')";
    sqlx::query(block).execute(&pool).await.expect("block");
    let report = run(&pool, &stripe, 1).await;
    assert_eq!((report.failed, report.open), (1, 1), "{report:?}");
    assert_eq!(alerts(&pool, CHARGE_FAILED_ALERT).await, 1);
    assert_eq!(entries(&pool, "charge").await, 0);
    assert_eq!(
        row(&pool, "ch_FakeT288n1").await,
        (1, "write-failed".into(), false)
    );
    let unblock = "ALTER TABLE payments.ledger_entry DROP CONSTRAINT t288_block";
    sqlx::query(unblock).execute(&pool).await.expect("unblock");
    let report = run(&pool, &stripe, 2).await;
    assert_eq!((report.failed, report.open), (0, 0), "{report:?}");
    assert!(report.matched(), "{report:?}");
    assert_eq!(entries(&pool, "charge").await, 1);
    assert_eq!(
        row(&pool, "ch_FakeT288n1").await,
        (2, "write-failed".into(), true)
    );
    run(&pool, &stripe, 3).await;
    assert_eq!(entries(&pool, "charge").await, 1);
    assert!(stripe.refunds().is_empty());
}

/// TD-FEE-49 (FEE-029, OPS-029): a charge naming no pick whose refund Stripe refuses every night
/// raises one High alert, naming only its row, on night 7 and none later; it is retried after it
/// left the list, refunded once on night 10, and the settled row is never retried again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_49_an_open_charge_is_retried_past_the_window_and_alerted_once(pool: PgPool) {
    let stripe = FakeStripe::with(vec![charge(2, "pi_FakeT288b", None)]);
    stripe.set_fail(true);
    for n in 0..10 {
        let report = run(&pool, &stripe, n).await;
        assert_eq!(report.open, 1, "night {n}");
        assert_eq!(report.retried, usize::from(n > 7), "night {n}");
        assert_eq!(
            alerts(&pool, OPEN_CHARGE_ALERT).await,
            i64::from(n >= 7),
            "night {n}"
        );
    }
    let (id, kind, severity): (Uuid, String, String) =
        sqlx::query_as("SELECT subject_id, subject_kind, severity FROM ops.alert WHERE kind = $1")
            .bind(OPEN_CHARGE_ALERT)
            .fetch_one(&pool)
            .await
            .expect("alert");
    let stored = "SELECT id FROM payments.reconcile_open WHERE alerted_at IS NOT NULL";
    let row_id: Uuid = sqlx::query_scalar(stored)
        .fetch_one(&pool)
        .await
        .expect("id");
    assert_eq!(
        (id, kind.as_str(), severity.as_str()),
        (row_id, OPEN_CHARGE_ALERT, "high")
    );
    stripe.set_fail(false);
    let report = run(&pool, &stripe, 10).await;
    assert_eq!((report.retried, report.refunded, report.open), (1, 1, 0));
    assert_eq!(stripe.refunds(), ["pi_FakeT288b"]);
    assert_eq!(
        row(&pool, "ch_FakeT288n2").await,
        (11, "refund-refused".into(), true)
    );
    let fetched = stripe.fetches();
    let report = run(&pool, &stripe, 11).await;
    assert_eq!((report.retried, report.open), (0, 0));
    assert_eq!(stripe.fetches(), fetched, "a settled row is never retried");
    assert_eq!(stripe.refunds(), ["pi_FakeT288b"]);
    assert_eq!(alerts(&pool, OPEN_CHARGE_ALERT).await, 1);
    assert_eq!(
        entries(&pool, "charge").await + entries(&pool, "refund").await,
        0
    );
}

/// TD-FEE-49 (FEE-026, FEE-029): a second kept charge disputed and a no-pick charge refunded at
/// Stripe since they were stored are settled by that state on their retry, with no refund sent.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_49_a_charge_refunded_or_disputed_since_is_settled_by_that_state(pool: PgPool) {
    pick(&pool).await;
    let second = charge(3, "pi_FakeT288s", Some(PICK));
    let stray = charge(4, "pi_FakeT288n", None);
    let stripe = FakeStripe::with(vec![second, stray]);
    stripe.set_fail(true);
    let report = run(&pool, &stripe, 0).await;
    assert_eq!((report.refund_failed, report.open), (2, 2), "{report:?}");
    stripe.change("ch_FakeT288n3", |c| {
        c.dispute = Some(("dp_FakeT288".into(), 7500))
    });
    stripe.change("ch_FakeT288n4", |c| c.refunded_cents = c.amount_cents);
    stripe.set_fail(false);
    let report = run(&pool, &stripe, 8).await;
    assert_eq!(
        (report.seen, report.retried, report.open),
        (0, 2, 0),
        "{report:?}"
    );
    assert!(stripe.refunds().is_empty(), "no refund is sent");
    assert_eq!(entries(&pool, "charge").await, 1);
    assert_eq!(entries(&pool, "refund").await, 0);
    for id in ["ch_FakeT288n3", "ch_FakeT288n4"] {
        assert_eq!(row(&pool, id).await, (2, "refund-refused".into(), true));
    }
}

/// TD-FEE-49 (FEE-029, PRIV-002, D-1872): the table holds Stripe IDs, amounts, a code and times
/// only, and the app role may update only the try, alert and settle columns and never delete.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_49_no_row_holds_a_contact_detail_and_the_grants_are_narrow(pool: PgPool) {
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns
          WHERE table_schema = 'payments' AND table_name = 'reconcile_open' ORDER BY column_name",
    )
    .fetch_all(&pool)
    .await
    .expect("columns");
    let expected = [
        "alerted_at",
        "amount_cents",
        "balance_transaction",
        "fee_cents",
        "first_seen",
        "id",
        "last_code",
        "last_tried",
        "pick_id",
        "refunded_cents",
        "settled_at",
        "stripe_charge_id",
        "stripe_intent_id",
        "tries",
    ];
    assert_eq!(columns, expected);
    let mut conn = pool.acquire().await.expect("conn");
    let ok = [
        "SET ROLE askhava_app",
        "INSERT INTO payments.reconcile_open (stripe_charge_id, stripe_intent_id, amount_cents,
           refunded_cents, fee_cents, balance_transaction, first_seen, last_tried, tries, last_code)
         VALUES ('ch_FakeT288g', 'pi_FakeT288g', 2500, 0, 103, 'txn_FakeT288g', now(), now(), 1,
           'mismatch')",
        "UPDATE payments.reconcile_open SET tries = 2, last_tried = now(), last_code = 'not-found',
           alerted_at = now(), settled_at = now()",
    ];
    for sql in ok {
        sqlx::query(sql).execute(&mut *conn).await.expect(sql);
    }
    let refused = [
        "UPDATE payments.reconcile_open SET amount_cents = 1",
        "UPDATE payments.reconcile_open SET first_seen = now()",
        "DELETE FROM payments.reconcile_open",
        "TRUNCATE payments.reconcile_open",
    ];
    for sql in refused {
        let err = sqlx::query(sql).execute(&mut *conn).await.expect_err(sql);
        let code = err.as_database_error().and_then(|e| e.code());
        assert_eq!(code.as_deref(), Some("42501"), "{sql}");
    }
    sqlx::query("RESET ROLE")
        .execute(&mut *conn)
        .await
        .expect("reset");
}

/// TD-FEE-49 (FEE-029): a stored charge Stripe cannot be read for, answers malformed, or no longer
/// has stays open with that code and one more try each night, and nothing is refunded.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_49_an_unreadable_or_missing_charge_stays_open(pool: PgPool) {
    let stripe = FakeStripe::with(vec![charge(5, "pi_FakeT288u", None)]);
    stripe.set_fail(true);
    run(&pool, &stripe, 0).await;
    stripe.set_fail(false);
    *stripe.fetch_down.lock().expect("lock") = true;
    assert_eq!(run(&pool, &stripe, 8).await.open, 1);
    assert_eq!(
        row(&pool, "ch_FakeT288n5").await,
        (2, "stripe-unreadable".into(), false)
    );
    *stripe.fetch_down.lock().expect("lock") = false;
    stripe.change("ch_FakeT288n5", |c| c.refunded_cents = 9999);
    assert_eq!(run(&pool, &stripe, 9).await.open, 1);
    assert_eq!(
        row(&pool, "ch_FakeT288n5").await,
        (3, "stripe-unreadable".into(), false)
    );
    stripe.charges.lock().expect("lock").clear();
    let report = run(&pool, &stripe, 10).await;
    assert_eq!((report.retried, report.open), (1, 1));
    assert_eq!(
        row(&pool, "ch_FakeT288n5").await,
        (4, "not-found".into(), false)
    );
    assert!(stripe.refunds().is_empty());
}
