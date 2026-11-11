//! TD-FEE-12 (FEE-021, FEE-026; D-491, D-1547): on a fresh database, a payment that succeeds after
//! its job moved on is refunded exactly once and Casey is alerted once, however often `picks`
//! reports it or the refund task runs; a payment on a current pick is left alone, and `payments`'
//! own record of an ended pick counts as moved on, in the real order (the pick ends, then the
//! charge check records the success, then `picks` reports it); a pick already refunded or disputed
//! is not alerted again. `picks` passes the pick's state as `PickNow`.
//!
//! Stripe is a fake that counts refunds and says the intent succeeded; all IDs are fake.

use std::sync::Mutex;

use askhava_adapters::api::{
    AdapterError, Charge, IdempotencyKey, IntentMetadata, PaymentIntent, PaymentsAdapter,
};
use askhava_payments::api::{
    LATE_KIND, LATE_SUCCESS_ALERT, LateSuccess, MoneyEventError, PickNow, late_success,
    run_charge_check, run_late_refund,
};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::json;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: Uuid = Uuid::from_u128(0xb064_0001);
const PICK: Uuid = Uuid::from_u128(0x064_0042);
const JOB: Uuid = Uuid::from_u128(0x064_0142);
const INTENT: &str = "pi_FakeT064";
const NOW: i64 = 1_793_900_000;

#[derive(Default)]
struct FakeStripe {
    refunds: Mutex<u32>,
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

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        assert_eq!(id, INTENT);
        Ok(PaymentIntent {
            id: INTENT.to_owned(),
            status: "succeeded".to_owned(),
            amount: 7500,
            currency: "usd".to_owned(),
            customer: "cus_FakeT064".to_owned(),
            metadata: IntentMetadata {
                pick_id: PICK.to_string(),
            },
            livemode: Some(false),
        })
    }

    async fn create_refund(
        &self,
        _: &IdempotencyKey,
        intent: &str,
    ) -> Result<String, AdapterError> {
        assert_eq!(intent, INTENT);
        *self.refunds.lock().expect("lock") += 1;
        Ok("re_FakeT064".to_owned())
    }
}

fn at(offset: i64) -> Timestamp {
    Timestamp::from_second(NOW + offset).expect("time")
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t064".to_owned())
}

/// One $75 payment whose one attempt was sent an hour ago and is still undecided, as T062 writes it.
async fn sent(pool: &PgPool) {
    seed(pool, &[
        "INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($2, 'cus_FakeT064')",
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, to_timestamp($5))",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at)
           VALUES ($1, 1, $2, 'sent', to_timestamp($5))",
    ], NOW - 3600)
    .await;
}

/// One $75 payment that succeeded, with its FEE-042 evidence record, as T062 and T189 write it.
async fn charged(pool: &PgPool) {
    seed(pool, &[
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, to_timestamp($5))",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at,
           decided_at) VALUES ($1, 1, $2, 'succeeded', to_timestamp($5), to_timestamp($5))",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
           idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version,
           amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1,
           'pick-' || $1::text || '-try1', 'cus_FakeT064', $4, 1, 7500, to_timestamp($5),
           to_timestamp($5))",
    ], NOW - 60)
    .await;
}

async fn seed(pool: &PgPool, sqls: &[&'static str], at: i64) {
    for sql in sqls {
        sqlx::query(*sql)
            .bind(PICK)
            .bind(BUSINESS)
            .bind(JOB)
            .bind(INTENT)
            .bind(at)
            .execute(pool)
            .await
            .expect("seed");
    }
}

async fn count(pool: &PgPool, sql: &'static str, arg: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(arg)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn report(pool: &PgPool, pick_now: PickNow) -> Result<LateSuccess, MoneyEventError> {
    let mut tx = pool.begin().await.expect("begin");
    let answer = late_success(&mut tx, PICK, pick_now, at(0)).await;
    tx.commit().await.expect("commit");
    answer
}

async fn alerts(pool: &PgPool) -> i64 {
    let sql = "SELECT count(*) FROM ops.alert WHERE kind = $1";
    count(pool, sql, LATE_SUCCESS_ALERT).await
}

async fn tasks(pool: &PgPool) -> i64 {
    count(
        pool,
        "SELECT count(*) FROM core.task WHERE kind = $1",
        LATE_KIND,
    )
    .await
}

/// TD-FEE-12 (FEE-021): the job moved on before Stripe's success arrived; `picks` reports it
/// three times and the task runs twice: one refund in Stripe and the ledger, one alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_12_late_success_is_refunded_once_and_alerted_once(pool: PgPool) {
    charged(&pool).await;
    assert_eq!(
        report(&pool, PickNow::MovedOn).await,
        Ok(LateSuccess::Queued)
    );
    for _ in 0..2 {
        let again = report(&pool, PickNow::MovedOn).await;
        assert_eq!(again, Ok(LateSuccess::AlreadyQueued));
    }
    assert_eq!((alerts(&pool).await, tasks(&pool).await), (1, 1));
    let fake = FakeStripe::default();
    let payload = json!({ "pick": PICK });
    for offset in [0, 3600] {
        run_late_refund(&pool, &fake, &secret(), &payload, at(offset))
            .await
            .expect("refund task");
    }
    assert_eq!(*fake.refunds.lock().expect("lock"), 1);
    let ledger =
        "SELECT count(*) FROM payments.ledger_entry WHERE kind = 'refund' AND stripe_id = $1";
    assert_eq!(count(&pool, ledger, INTENT).await, 1);
    let row = "SELECT count(*) FROM payments.refund WHERE status = 'succeeded'
                 AND stripe_refund_id = $1 AND trigger = 'late-success'";
    assert_eq!(count(&pool, row, "re_FakeT064").await, 1);
    assert_eq!(alerts(&pool).await, 1);
}

/// TD-FEE-12 (FEE-021, FEE-017): a success on a current pick is the normal payment, so nothing is
/// queued or alerted.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_12_current_pick_is_kept(pool: PgPool) {
    charged(&pool).await;
    assert_eq!(
        report(&pool, PickNow::Payable).await,
        Ok(LateSuccess::Current)
    );
    assert_eq!((alerts(&pool).await, tasks(&pool).await), (0, 0));
}

/// TD-FEE-12 (FEE-021, FEE-018, FEE-019; D-1547), in the real order: `payments` ends the pick while
/// its attempt is undecided and a report finds nothing
/// charged; the 30-minute check then records Stripe's success, and the next report is a late
/// success even though `picks` still says `Payable`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_12_own_end_counts_once_the_check_records_the_success(pool: PgPool) {
    sent(&pool).await;
    sqlx::query("UPDATE payments.pick_intent SET ended_at = to_timestamp($2) WHERE pick_id = $1")
        .bind(PICK)
        .bind(NOW - 30)
        .execute(&pool)
        .await
        .expect("end");
    let early = report(&pool, PickNow::Payable).await;
    assert_eq!(early, Err(MoneyEventError::NothingCharged));
    let check = json!({ "pick": PICK, "try": 1, "round": 0 });
    run_charge_check(&pool, &FakeStripe::default(), &check, at(0))
        .await
        .expect("check");
    assert_eq!(
        report(&pool, PickNow::Payable).await,
        Ok(LateSuccess::AlreadyQueued)
    );
    assert_eq!((alerts(&pool).await, tasks(&pool).await), (1, 1));
}

/// TD-FEE-12 (FEE-021, FEE-026, FEE-027): a moved-on pick that already has a refund or a bank
/// dispute queues nothing and alerts nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_12_refunded_or_disputed_pick_is_not_alerted_again(pool: PgPool) {
    charged(&pool).await;
    seed(
        &pool,
        &[
            "INSERT INTO payments.dispute (stripe_dispute_id, pick_id, amount_cents, due_at,
         opened_at) VALUES ('dp_FakeT064', $1, 7500, to_timestamp($5), to_timestamp($5))",
        ],
        NOW,
    )
    .await;
    let answer = report(&pool, PickNow::MovedOn).await;
    assert_eq!(answer, Ok(LateSuccess::AlreadyQueued));
    sqlx::query("DELETE FROM payments.dispute")
        .execute(&pool)
        .await
        .expect("clear");
    seed(
        &pool,
        &[
            "INSERT INTO payments.refund (pick_id, trigger, status, tries, requested_at)
         VALUES ($1, 'unreachable', 'sending', 1, to_timestamp($5))",
        ],
        NOW,
    )
    .await;
    let answer = report(&pool, PickNow::MovedOn).await;
    assert_eq!(answer, Ok(LateSuccess::AlreadyQueued));
    assert_eq!((alerts(&pool).await, tasks(&pool).await), (0, 0));
}

/// TD-FEE-12 (FEE-021, FEE-026, JOB-006; D-1547): a pick with no succeeded charge has nothing to
/// refund and raises nothing; a malformed task fails with a fixed sentence, and a task for an
/// uncharged pick is done without calling Stripe, not dead-lettered.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_12_nothing_charged_is_not_refunded(pool: PgPool) {
    let none = report(&pool, PickNow::MovedOn).await;
    assert_eq!(none, Err(MoneyEventError::NothingCharged));
    assert_eq!((alerts(&pool).await, tasks(&pool).await), (0, 0));
    let fake = FakeStripe::default();
    let malformed = json!({ "pick": "not-a-uuid" });
    let run = run_late_refund(&pool, &fake, &secret(), &malformed, at(0)).await;
    assert!(run.is_err());
    let uncharged = json!({ "pick": PICK });
    let run = run_late_refund(&pool, &fake, &secret(), &uncharged, at(0)).await;
    assert!(run.is_ok());
    assert_eq!(*fake.refunds.lock().expect("lock"), 0);
}
