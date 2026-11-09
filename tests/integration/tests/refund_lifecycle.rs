//! TD-FEE-42 (FEE-026, FEE-028, FEE-029, FEE-030; D-1545, D-1753, D-2066 to D-2068): on a fresh
//! database, with a scripted Stripe answering each refund state, a refund is recorded as Stripe
//! reports it: `pending` stores the row `pending` with no ledger entry and no `Refunded` answer (the
//! only answer `picks` sends its "refunded" notice on), and the 30-minute check or the webhook's
//! `succeeded` then appends one `refund` entry; `failed` or `canceled` (at once or after acceptance)
//! is the FEE-026 alert and hourly retry with no entry; `failed` after `succeeded` appends one
//! `refund-reversal` of the same amount and the retry's later success one new `refund`, so the
//! ledger nets to Stripe; a lost answer then a retry is one refund; an unknown state string is an
//! adapter error, never a success; the reconcile's own refund follows the same rule; and no ledger
//! row is ever updated or deleted. All IDs are fake.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_payments::api::{
    EntryKind, LedgerEntry, LedgerError, MoneyEventError, NewEntry, REFUND_CHECK_EVERY,
    REFUND_CHECK_KIND, REFUND_FAILED_ALERT, REFUND_REVERSAL_CODE, RETRY_KIND, ReconcileStripe,
    Refund, RefundOutcome, RefundReason, RefundState, RefundStatus, RefundStep, RefundStripe,
    RefundTrigger, StripeCharge, StripeRefund, money_view, reconcile, refund_canceled,
    refund_failed, refund_succeeded, request_refund, run_refund_check, run_refund_retry,
};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::json;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: Uuid = Uuid::from_u128(0xb258_0001);
const PICK: Uuid = Uuid::from_u128(0x258_0042);
const JOB: Uuid = Uuid::from_u128(0x258_0142);
const INTENT: &str = "pi_FakeT258";
const NOW: i64 = 1_793_900_000;
const DAY: i64 = 86_400;

/// Refund answers from the script (an empty script refuses), reads from the map; every key kept.
#[derive(Default)]
struct FakeStripe {
    answers: Mutex<VecDeque<Result<StripeRefund, AdapterError>>>,
    reads: Mutex<HashMap<String, StripeRefund>>,
    keys: Mutex<Vec<String>>,
    charges: Vec<StripeCharge>,
}

fn answer(id: &str, status: &str) -> StripeRefund {
    answer_for(id, status, INTENT)
}

fn answer_for(id: &str, status: &str, intent: &str) -> StripeRefund {
    StripeRefund {
        id: id.to_owned(),
        status: status.to_owned(),
        amount: Some(7500),
        currency: Some("usd".to_owned()),
        payment_intent: Some(intent.to_owned()),
    }
}

fn stripe(script: &[(&str, &str)]) -> FakeStripe {
    let answers = script.iter().map(|(id, s)| Ok(answer(id, s))).collect();
    FakeStripe {
        answers: Mutex::new(answers),
        ..FakeStripe::default()
    }
}

impl FakeStripe {
    fn calls(&self) -> usize {
        self.keys.lock().expect("lock").len()
    }

    fn reads_as(&self, id: &str, status: &str) {
        let read = answer(id, status);
        self.reads.lock().expect("lock").insert(id.to_owned(), read);
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
    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("the typed refund is used")
    }
    async fn send_refund(
        &self,
        key: &IdempotencyKey,
        _: &str,
    ) -> Result<StripeRefund, AdapterError> {
        self.keys
            .lock()
            .expect("lock")
            .push(key.as_str().to_owned());
        let next = self.answers.lock().expect("lock").pop_front();
        next.unwrap_or(Err(AdapterError::Vendor {
            status: 400,
            code: None,
        }))
    }
    async fn read_refund(&self, id: &str) -> Result<StripeRefund, AdapterError> {
        let read = self.reads.lock().expect("lock").get(id).cloned();
        read.ok_or(AdapterError::OutcomeUnknown)
    }
}

impl ReconcileStripe for FakeStripe {
    async fn list_charges(&self, _: Timestamp) -> Result<Vec<StripeCharge>, AdapterError> {
        Ok(self.charges.clone())
    }
}

/// A fake written before the typed refund: only `create_refund`, as the four older fakes.
struct LegacyStripe;

impl PaymentsAdapter for LegacyStripe {
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
    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok("re_FakeLegacy".to_owned())
    }
}

fn at(offset: i64) -> Timestamp {
    Timestamp::from_second(NOW + offset).expect("time")
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t258".to_owned())
}

fn trigger() -> (Uuid, RefundTrigger) {
    (PICK, RefundTrigger::Rule(RefundReason::Unreachable))
}

/// One paid pick of $75 with its FEE-042 evidence record, as T062 and T189 write it.
async fn charged(pool: &PgPool) {
    for sql in [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, to_timestamp($5))",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at,
           decided_at) VALUES ($1, 1, $2, 'succeeded', to_timestamp($5), to_timestamp($5))",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
           idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version,
           amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1,
           'pick-' || $1::text || '-try1', 'cus_FakeT258', $4, 1, 7500, to_timestamp($5),
           to_timestamp($5))",
    ] {
        sqlx::query(sql)
            .bind(PICK)
            .bind(BUSINESS)
            .bind(JOB)
            .bind(INTENT)
            .bind(NOW - DAY)
            .execute(pool)
            .await
            .expect("seed");
    }
}

async fn scalar(pool: &PgPool, sql: &'static str, arg: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(arg)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn ledger(pool: &PgPool, kind: &str) -> i64 {
    let sql = "SELECT count(*) FROM payments.ledger_entry WHERE kind = $1";
    scalar(pool, sql, kind).await
}

async fn alerts(pool: &PgPool) -> i64 {
    let sql = "SELECT count(*) FROM ops.alert WHERE kind = $1";
    scalar(pool, sql, REFUND_FAILED_ALERT).await
}

async fn tasks(pool: &PgPool, kind: &str) -> i64 {
    scalar(pool, "SELECT count(*) FROM core.task WHERE kind = $1", kind).await
}

async fn status(pool: &PgPool) -> String {
    sqlx::query_scalar("SELECT status FROM payments.refund WHERE pick_id = $1")
        .bind(PICK)
        .fetch_one(pool)
        .await
        .expect("row")
}

/// Every ledger row as written; a later snapshot must start with an earlier one unchanged.
async fn rows(pool: &PgPool) -> Vec<(String, String, i64)> {
    sqlx::query_as(
        "SELECT idempotency_key, kind, amount_cents FROM payments.ledger_entry ORDER BY seq",
    )
    .fetch_all(pool)
    .await
    .expect("rows")
}

async fn retry(pool: &PgPool, fake: &FakeStripe, try_no: i32, offset: i64) {
    let task = json!({"pick": PICK, "try": try_no});
    run_refund_retry(pool, fake, &secret(), &task, at(offset))
        .await
        .expect("retry");
}

async fn check(pool: &PgPool, fake: &FakeStripe, re: &str, offset: i64) {
    let task = json!({"refund": re});
    run_refund_check(pool, fake, &task, at(offset))
        .await
        .expect("check");
}

/// TD-FEE-42 (FEE-026, FEE-028): `pending` is no refund yet; the 30-minute check asks Stripe again
/// and its `succeeded` appends the one refund entry; nothing is sent twice meanwhile.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_42_pending_then_succeeded_on_the_check(pool: PgPool) {
    charged(&pool).await;
    let fake = stripe(&[("re_FakeA", "pending")]);
    let first = request_refund(&pool, &fake, &secret(), trigger(), at(0)).await;
    assert_eq!(first, Ok(RefundOutcome::Failed));
    assert_eq!(status(&pool).await, "pending");
    assert_eq!((ledger(&pool, "refund").await, alerts(&pool).await), (0, 0));
    assert_eq!(tasks(&pool, REFUND_CHECK_KIND).await, 1);
    let again = request_refund(&pool, &fake, &secret(), trigger(), at(60)).await;
    assert_eq!(again, Ok(RefundOutcome::Failed));
    retry(&pool, &fake, 1, 3600).await;
    assert_eq!(fake.calls(), 1);
    check(&pool, &fake, "re_FakeA", 1800).await;
    assert_eq!(
        tasks(&pool, REFUND_CHECK_KIND).await,
        2,
        "unreadable: asked again"
    );
    fake.reads_as("re_FakeA", "requires_action");
    let later = REFUND_CHECK_EVERY.as_secs() * 2;
    check(&pool, &fake, "re_FakeA", later).await;
    assert_eq!(
        tasks(&pool, REFUND_CHECK_KIND).await,
        3,
        "still pending: asked again"
    );
    fake.reads_as("re_FakeA", "succeeded");
    check(&pool, &fake, "re_FakeA", later * 2).await;
    assert_eq!(status(&pool).await, "succeeded");
    assert_eq!(ledger(&pool, "refund").await, 1);
    check(&pool, &fake, "re_FakeA", later * 3).await;
    assert_eq!(
        tasks(&pool, REFUND_CHECK_KIND).await,
        3,
        "no longer pending: left alone"
    );
    let done = request_refund(&pool, &fake, &secret(), trigger(), at(DAY)).await;
    assert_eq!(done, Ok(RefundOutcome::Unchanged));
    let bad = run_refund_check(&pool, &fake, &json!({}), at(0)).await;
    assert!(bad.is_err());
}

/// TD-FEE-42 (FEE-026, FEE-029): `requires_action`, then the webhook's `succeeded`, records one
/// refund, also when the nightly reconcile already recorded the refund Stripe listed; a repeated or
/// unknown `succeeded` changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_42_pending_then_succeeded_on_the_webhook(pool: PgPool) {
    charged(&pool).await;
    let own = StripeCharge {
        id: "ch_FakeT258".to_owned(),
        payment_intent: INTENT.to_owned(),
        refunded_cents: 7500,
        balance_transaction: "txn_FakeT258".to_owned(),
        ..second_charge()
    };
    let fake = FakeStripe {
        charges: vec![own],
        ..stripe(&[("re_FakeA", "requires_action")])
    };
    let first = request_refund(&pool, &fake, &secret(), trigger(), at(0)).await;
    assert_eq!(first, Ok(RefundOutcome::Failed));
    reconcile(&pool, &fake, &secret(), at(30))
        .await
        .expect("run");
    assert_eq!(ledger(&pool, "refund").await, 1);
    assert_eq!(refund_succeeded(&pool, "re_FakeA", at(60)).await, Ok(true));
    for again in ["re_FakeA", "re_FakeUnknown"] {
        assert_eq!(refund_succeeded(&pool, again, at(90)).await, Ok(false));
    }
    assert_eq!(ledger(&pool, "refund").await, 1);
    let view = money_view(&pool, (at(-999 * DAY), at(999 * DAY)))
        .await
        .expect("view");
    assert_eq!(view.refunds, 7500);
}

/// TD-FEE-42 (FEE-026, FEE-028, FEE-029, D-2067): the nightly reconcile appends the refund Stripe
/// lists while the row is still `pending`; Stripe's later `failed` reverses it once, decided from
/// the ledger alone, so the ledger nets to Stripe's no refund and no row is changed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_42_reconciled_while_pending_then_failed_reverses(pool: PgPool) {
    charged(&pool).await;
    let own = StripeCharge {
        id: "ch_FakeT258".to_owned(),
        payment_intent: INTENT.to_owned(),
        refunded_cents: 7500,
        balance_transaction: "txn_FakeT258".to_owned(),
        ..second_charge()
    };
    let fake = FakeStripe {
        charges: vec![own],
        ..stripe(&[("re_FakeA", "pending")])
    };
    let first = request_refund(&pool, &fake, &secret(), trigger(), at(0)).await;
    assert_eq!(first, Ok(RefundOutcome::Failed));
    reconcile(&pool, &fake, &secret(), at(30))
        .await
        .expect("run");
    assert_eq!(status(&pool).await, "pending");
    assert_eq!(ledger(&pool, "refund").await, 1);
    let before = rows(&pool).await;
    assert_eq!(refund_failed(&pool, "re_FakeA", at(60)).await, Ok(true));
    assert_eq!(refund_failed(&pool, "re_FakeA", at(90)).await, Ok(false));
    assert_eq!(status(&pool).await, "failed");
    assert_eq!(ledger(&pool, "refund-reversal").await, 1);
    let reversal = "SELECT amount_cents FROM payments.ledger_entry WHERE kind = $1";
    assert_eq!(scalar(&pool, reversal, "refund-reversal").await, 7500);
    let view = money_view(&pool, (at(-999 * DAY), at(999 * DAY)))
        .await
        .expect("view");
    assert_eq!(view.refunds, 0, "Stripe holds no refund");
    let after = rows(&pool).await;
    assert_eq!(after[..before.len()], before[..], "no ledger row changed");
}

/// TD-FEE-42 (FEE-026): `failed` or `canceled` after acceptance (from `pending`, through the
/// webhook or the check) is the alert and the hourly retry with no ledger entry.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_42_failed_or_canceled_after_acceptance(pool: PgPool) {
    charged(&pool).await;
    let fake = stripe(&[
        ("re_FakeA", "pending"),
        ("re_FakeB", "pending"),
        ("re_FakeC", "pending"),
        ("re_FakeD", "succeeded"),
    ]);
    request_refund(&pool, &fake, &secret(), trigger(), at(0))
        .await
        .expect("send");
    assert_eq!(refund_failed(&pool, "re_FakeA", at(60)).await, Ok(true));
    assert_eq!(status(&pool).await, "failed");
    assert_eq!((ledger(&pool, "refund").await, alerts(&pool).await), (0, 1));
    assert_eq!(tasks(&pool, RETRY_KIND).await, 2);
    retry(&pool, &fake, 1, 3660).await;
    assert_eq!(status(&pool).await, "pending");
    assert_eq!(refund_canceled(&pool, "re_FakeB", at(3700)).await, Ok(true));
    assert_eq!(status(&pool).await, "canceled");
    assert_eq!(
        refund_canceled(&pool, "re_FakeB", at(3800)).await,
        Ok(false)
    );
    retry(&pool, &fake, 2, 7300).await;
    fake.reads_as("re_FakeC", "canceled");
    check(&pool, &fake, "re_FakeC", 9100).await;
    assert_eq!(status(&pool).await, "canceled");
    assert_eq!(ledger(&pool, "refund").await, 0);
    retry(&pool, &fake, 3, 11_000).await;
    assert_eq!(status(&pool).await, "succeeded");
    assert_eq!((ledger(&pool, "refund").await, alerts(&pool).await), (1, 1));
}

/// TD-FEE-42 (FEE-026): `failed` and `canceled` answered at once are failures: alerted once, sent
/// again by the hourly retry; a `failed` read on the check fails a pending refund.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_42_failed_or_canceled_at_once(pool: PgPool) {
    charged(&pool).await;
    let fake = stripe(&[
        ("re_FakeA", "failed"),
        ("re_FakeA", "failed"),
        ("re_FakeB", "canceled"),
        ("re_FakeB", "canceled"),
        ("re_FakeC", "pending"),
    ]);
    let first = request_refund(&pool, &fake, &secret(), trigger(), at(0)).await;
    assert_eq!(first, Ok(RefundOutcome::Failed));
    assert_eq!(
        (status(&pool).await, alerts(&pool).await),
        ("failed".to_owned(), 1)
    );
    retry(&pool, &fake, 1, 3600).await;
    assert_eq!(status(&pool).await, "canceled");
    retry(&pool, &fake, 2, 7200).await;
    fake.reads_as("re_FakeC", "failed");
    check(&pool, &fake, "re_FakeC", 9000).await;
    assert_eq!(status(&pool).await, "failed");
    assert_eq!((ledger(&pool, "refund").await, alerts(&pool).await), (0, 1));
}

/// TD-FEE-42 (FEE-026, FEE-028, FEE-030, D-1753): `failed` after `succeeded` appends one
/// `refund-reversal` of the same amount; a stale `succeeded` of that refund records nothing; the
/// retry's later success appends one new `refund`; `canceled` after `succeeded` reverses too. The
/// ledger nets to what Stripe holds and no row is changed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_42_failed_after_succeeded_reverses(pool: PgPool) {
    charged(&pool).await;
    let fake = stripe(&[
        ("re_FakeA", "succeeded"),
        ("re_FakeA", "succeeded"),
        ("re_FakeB", "succeeded"),
    ]);
    let first = request_refund(&pool, &fake, &secret(), trigger(), at(0)).await;
    assert_eq!(first, Ok(RefundOutcome::Refunded("re_FakeA".to_owned())));
    let before = rows(&pool).await;
    assert_eq!(refund_failed(&pool, "re_FakeA", at(60)).await, Ok(true));
    assert_eq!(refund_failed(&pool, "re_FakeA", at(90)).await, Ok(false));
    let reversal = "SELECT amount_cents FROM payments.ledger_entry WHERE kind = $1";
    assert_eq!(scalar(&pool, reversal, "refund-reversal").await, 7500);
    let view = money_view(&pool, (at(-999 * DAY), at(999 * DAY)))
        .await
        .expect("view");
    assert_eq!(view.refunds, 0, "Stripe holds no refund");
    retry(&pool, &fake, 1, 3660).await;
    assert_eq!(
        status(&pool).await,
        "failed",
        "a stale success records nothing"
    );
    retry(&pool, &fake, 2, 7300).await;
    assert_eq!(status(&pool).await, "succeeded");
    assert_eq!(ledger(&pool, "refund").await, 2);
    let view = money_view(&pool, (at(-999 * DAY), at(999 * DAY)))
        .await
        .expect("view");
    assert_eq!(view.refunds, 7500, "nets to Stripe's one refund");
    assert_eq!(refund_canceled(&pool, "re_FakeB", at(8000)).await, Ok(true));
    assert_eq!(ledger(&pool, "refund-reversal").await, 2);
    let after = rows(&pool).await;
    assert_eq!(after[..before.len()], before[..], "no ledger row changed");
}

/// TD-FEE-42 (FEE-026, D-1545): a lost answer then the retry is one refund, found by replaying
/// the first try's key.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_42_lost_answer_then_retry_refunds_once(pool: PgPool) {
    charged(&pool).await;
    let fake = FakeStripe {
        answers: Mutex::new(
            [
                Err(AdapterError::OutcomeUnknown),
                Ok(answer("re_FakeA", "succeeded")),
            ]
            .into(),
        ),
        ..FakeStripe::default()
    };
    let first = request_refund(&pool, &fake, &secret(), trigger(), at(0)).await;
    assert_eq!(first, Ok(RefundOutcome::Failed));
    retry(&pool, &fake, 1, 3600).await;
    let keys = fake.keys.lock().expect("lock").clone();
    assert_eq!((keys.len(), keys[0] == keys[1]), (2, true));
    assert_eq!(status(&pool).await, "succeeded");
    assert_eq!(ledger(&pool, "refund").await, 1);
}

/// TD-FEE-42 (FEE-026, D-1753): an unknown state string, a refund of another PaymentIntent or
/// another refund's read is an adapter error, never a success.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_42_unknown_state_is_an_error(pool: PgPool) {
    charged(&pool).await;
    let fake = stripe(&[("re_FakeA", "refunded"), ("re_FakeB", "pending")]);
    let first = request_refund(&pool, &fake, &secret(), trigger(), at(0)).await;
    assert_eq!(first, Ok(RefundOutcome::Failed));
    assert_eq!(
        (status(&pool).await, ledger(&pool, "refund").await),
        ("failed".to_owned(), 0)
    );
    retry(&pool, &fake, 1, 3600).await;
    let mut other = answer("re_FakeB", "succeeded");
    other.payment_intent = Some("pi_FakeOther".to_owned());
    fake.reads
        .lock()
        .expect("lock")
        .insert("re_FakeB".to_owned(), other);
    check(&pool, &fake, "re_FakeB", 5400).await;
    assert_eq!(
        status(&pool).await,
        "pending",
        "another intent's refund is not this one"
    );
    let key = IdempotencyKey::derive(&secret(), "test", &["x"]);
    let checked = |id: &str, s: &str, pi: Option<&str>| {
        let raw = StripeRefund {
            payment_intent: pi.map(str::to_owned),
            ..answer(id, s)
        };
        Refund::checked(raw, Some(INTENT))
    };
    assert_eq!(
        checked("re_X", "weird", Some(INTENT)),
        Err(AdapterError::Decode)
    );
    assert_eq!(checked("re_X", "pending", None), Err(AdapterError::Decode));
    assert_eq!(
        checked("ch_X", "pending", Some(INTENT)),
        Err(AdapterError::InvalidId)
    );
    assert_eq!(
        checked("re_", "pending", Some(INTENT)),
        Err(AdapterError::InvalidId)
    );
    let ok = checked("re_X", "succeeded", Some(INTENT)).expect("checked");
    assert_eq!(
        (ok.state, ok.amount_cents),
        (RefundState::Succeeded, Some(7500))
    );
    let swapped = answer("re_FakeOther", "succeeded");
    fake.reads
        .lock()
        .expect("lock")
        .insert("re_FakeB".to_owned(), swapped);
    assert_eq!(
        fake.fetch_refund("re_FakeB").await,
        Err(AdapterError::Decode)
    );
    let legacy = LegacyStripe.refund(&key, INTENT).await.expect("legacy");
    assert_eq!(
        (legacy.id.as_str(), legacy.state),
        ("re_FakeLegacy", RefundState::Succeeded)
    );
    assert_eq!(
        LegacyStripe.fetch_refund("re_FakeLegacy").await,
        Err(AdapterError::NotSent)
    );
}

fn second_charge() -> StripeCharge {
    StripeCharge {
        id: "ch_FakeT258b".to_owned(),
        payment_intent: "pi_FakeT258b".to_owned(),
        pick: Some(PICK),
        amount_cents: 7500,
        refunded_cents: 0,
        dispute: None,
        balance_transaction: "txn_FakeT258b".to_owned(),
        fee_cents: 248,
    }
}

/// TD-FEE-42 (FEE-029, FEE-026, D-1753): the reconcile's own refund of a second charge appends
/// its `refund` entry only on `succeeded`: `pending` counts neither way, `failed` is alerted.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_42_reconcile_own_refund_follows_the_rule(pool: PgPool) {
    charged(&pool).await;
    let other = "pi_FakeT258b";
    let script = [
        ("re_FakeA", "pending"),
        ("re_FakeB", "failed"),
        ("re_FakeC", "succeeded"),
    ];
    let fake = FakeStripe {
        answers: Mutex::new(
            script
                .iter()
                .map(|(id, s)| Ok(answer_for(id, s, other)))
                .collect(),
        ),
        charges: vec![second_charge()],
        ..FakeStripe::default()
    };
    let report = reconcile(&pool, &fake, &secret(), at(0))
        .await
        .expect("run");
    assert_eq!((report.refunded, report.refund_failed), (0, 0));
    assert_eq!((ledger(&pool, "refund").await, alerts(&pool).await), (0, 0));
    let night = |n: i64| at(n * DAY);
    let report = reconcile(&pool, &fake, &secret(), night(1))
        .await
        .expect("run");
    assert_eq!((report.refunded, report.refund_failed), (0, 1));
    assert_eq!((ledger(&pool, "refund").await, alerts(&pool).await), (0, 1));
    let report = reconcile(&pool, &fake, &secret(), night(2))
        .await
        .expect("run");
    assert_eq!((report.refunded, report.refund_failed), (1, 0));
    assert_eq!(ledger(&pool, "refund").await, 1);
}

/// TD-FEE-42 (FEE-028, D-1545): the app role can never update or delete a ledger row, and the
/// webhook's event record is append-only too; a nothing-charged pick is never refunded.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_42_ledger_and_events_are_append_only(pool: PgPool) {
    let fake = stripe(&[]);
    let none = request_refund(&pool, &fake, &secret(), trigger(), at(0)).await;
    assert_eq!(none, Err(MoneyEventError::NothingCharged));
    let mut tx = pool.begin().await.expect("tx");
    sqlx::query("SET LOCAL ROLE askhava_app")
        .execute(&mut *tx)
        .await
        .expect("role");
    sqlx::query(
        "INSERT INTO payments.webhook_event (stripe_event_id, event_type, received_at)
         VALUES ('evt_FakeT258', 'refund.updated', now())",
    )
    .execute(&mut *tx)
    .await
    .expect("insert");
    for sql in [
        "UPDATE payments.webhook_event SET event_type = 'refund.failed'",
        "DELETE FROM payments.webhook_event",
        "UPDATE payments.ledger_entry SET amount_cents = 1",
        "DELETE FROM payments.ledger_entry",
    ] {
        sqlx::query("SAVEPOINT s")
            .execute(&mut *tx)
            .await
            .expect("sp");
        let refused = sqlx::query(sql).execute(&mut *tx).await.expect_err(sql);
        let code = refused.as_database_error().and_then(|e| e.code());
        assert_eq!(code.as_deref(), Some("42501"), "{sql}");
        sqlx::query("ROLLBACK TO SAVEPOINT s")
            .execute(&mut *tx)
            .await
            .expect("rb");
    }
}

/// TD-FEE-42 (FEE-026, FEE-028): the state machine never resends a pending refund, resends a
/// canceled one like a failed one and blocks it on a dispute; only a refund entry has a reversal.
#[test]
fn td_fee_42_states_and_reversals() {
    let canceled = Some((RefundStatus::Canceled, 2));
    let next = RefundStatus::next;
    assert_eq!(next(canceled, true, None), RefundStep::Block(true));
    assert_eq!(next(canceled, false, Some(1)), RefundStep::Nothing);
    assert_eq!(next(canceled, false, None), RefundStep::Send(3));
    let pending = RefundStatus::from_stored("pending");
    assert_eq!(
        next(pending.map(|s| (s, 1)), true, Some(1)),
        RefundStep::Nothing
    );
    assert!(pending.is_some_and(RefundStatus::is_pending));
    let stored = |code: &str| RefundStatus::from_stored(code).map(RefundStatus::code);
    assert_eq!(
        (stored("pending"), stored("canceled")),
        (Some("pending"), Some("canceled"))
    );
    let entry = |kind: EntryKind| {
        LedgerEntry::new(NewEntry {
            kind,
            business: Some(BUSINESS),
            job: Some(JOB),
            amount_cents: 7500,
            stripe_id: Some(INTENT),
            idempotency_key: "refund.fake",
        })
        .expect("entry")
    };
    let refund = entry(EntryKind::Refund);
    let reversal = refund.reversal("refund-reversal.fake").expect("reversal");
    assert_eq!(
        (reversal.code(), reversal.amount_cents()),
        (REFUND_REVERSAL_CODE, 7500)
    );
    assert_eq!(reversal.reversal("again"), Err(LedgerError::Parties));
    assert_eq!(refund.reversal("bad key"), Err(LedgerError::Key));
    let charge = entry(EntryKind::Charge);
    assert_eq!(
        charge.reversal("refund-reversal.x"),
        Err(LedgerError::Parties)
    );
    assert_eq!(charge.code(), "charge");
}
