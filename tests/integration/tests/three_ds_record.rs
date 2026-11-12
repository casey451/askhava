//! TD-FEE-69 (T405, D-3118; FEE-050, FEE-027), against PostgreSQL: each charged pick's 3-D Secure
//! result is read once through a fake `ThreeDsStripe` and stored in `payments.charge_three_ds`.
//! An `authenticated` payment stores true; an `attempt_acknowledged` one and one with no 3-D
//! Secure (read through `ThreeDs::from_result`, as the adapter reads them) store false; a second
//! record changes nothing; a pick with no row reads false; a pick with no charge evidence, or a
//! failed read, stores nothing. The stand-in's expanded `latest_charge` is
//! `tests/contract/test_stripe_three_ds.py`. Every ID is fake.

use std::sync::Mutex;

use askhava_adapters::api::AdapterError;
use askhava_payments::api::{
    MoneyEventError, ThreeDs, ThreeDsStripe, record_three_ds, three_ds_authenticated,
};
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: Uuid = Uuid::from_u128(0x405_0b01);
const NOW: i64 = 1_793_000_000;

/// A fake Stripe answering one scripted result (`None`: an unanswered read) for every read,
/// recording the PaymentIntents asked.
struct FakeSource {
    answer: Option<ThreeDs>,
    asked: Mutex<Vec<String>>,
}

impl FakeSource {
    fn new(answer: Option<ThreeDs>) -> Self {
        Self {
            answer,
            asked: Mutex::new(Vec::new()),
        }
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("lock").clone()
    }
}

impl ThreeDsStripe for FakeSource {
    async fn three_ds(&self, intent: &str) -> Result<ThreeDs, AdapterError> {
        self.asked.lock().expect("lock").push(intent.to_owned());
        self.answer.ok_or(AdapterError::OutcomeUnknown)
    }
}

fn pick(n: u128) -> Uuid {
    Uuid::from_u128(0x405_0a00 + n)
}

fn intent(n: u128) -> String {
    format!("pi_FakeT405n{n}")
}

/// Pick `n`'s $75 charge with its FEE-042 evidence record, as T062 and T189 write it.
async fn charged(pool: &PgPool, n: u128) {
    for sql in [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, to_timestamp($5))",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at,
           decided_at) VALUES ($1, 1, $2, 'succeeded', to_timestamp($5), to_timestamp($5))",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
           idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version,
           amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1,
           'pick-' || $1::text || '-try1', 'cus_FakeT405', $4, 1, 7500, to_timestamp($5),
           to_timestamp($5))",
    ] {
        sqlx::query(sql)
            .bind(pick(n))
            .bind(BUSINESS)
            .bind(Uuid::from_u128(0x405_0c00 + n))
            .bind(intent(n))
            .bind(NOW)
            .execute(pool)
            .await
            .expect("seed");
    }
}

async fn rows(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM payments.charge_three_ds")
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn authenticated(pool: &PgPool, n: u128) -> bool {
    let mut conn = pool.acquire().await.expect("conn");
    three_ds_authenticated(&mut conn, pick(n))
        .await
        .expect("read")
}

/// TD-FEE-69: authenticated stores true; `attempt_acknowledged` and no 3-D Secure store false.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_69_each_result_is_stored_as_read(pool: PgPool) {
    let cases = [
        (1, Some("authenticated"), true),
        (2, Some("attempt_acknowledged"), false),
        (3, None, false),
    ];
    for (n, answer, stored) in cases {
        charged(&pool, n).await;
        let source = FakeSource::new(Some(ThreeDs::from_result(answer)));
        let mut conn = pool.acquire().await.expect("conn");
        record_three_ds(&mut conn, &source, pick(n))
            .await
            .expect("recorded");
        assert_eq!(source.asked(), vec![intent(n)]);
        assert_eq!(authenticated(&pool, n).await, stored, "pick {n}");
    }
    assert_eq!(rows(&pool).await, 3);
}

/// TD-FEE-69: a second record asks nothing and changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_69_a_second_record_changes_nothing(pool: PgPool) {
    charged(&pool, 4).await;
    let mut conn = pool.acquire().await.expect("conn");
    let first = FakeSource::new(Some(ThreeDs::Authenticated));
    record_three_ds(&mut conn, &first, pick(4))
        .await
        .expect("first");
    let second = FakeSource::new(Some(ThreeDs::NotAuthenticated));
    record_three_ds(&mut conn, &second, pick(4))
        .await
        .expect("second");
    assert!(second.asked().is_empty());
    assert!(authenticated(&pool, 4).await);
    assert_eq!(rows(&pool).await, 1);
}

/// TD-FEE-69: no row reads false; no charge evidence and a failed read store nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_69_unknown_is_never_authenticated(pool: PgPool) {
    charged(&pool, 5).await;
    assert!(!authenticated(&pool, 5).await);
    let mut conn = pool.acquire().await.expect("conn");
    let source = FakeSource::new(Some(ThreeDs::Authenticated));
    let none = record_three_ds(&mut conn, &source, pick(6)).await;
    assert_eq!(none, Err(MoneyEventError::NothingCharged));
    assert!(source.asked().is_empty());
    let failing = FakeSource::new(None);
    let failed = record_three_ds(&mut conn, &failing, pick(5)).await;
    assert_eq!(failed, Err(MoneyEventError::Stripe));
    assert_eq!(rows(&pool).await, 0);
    assert!(!authenticated(&pool, 5).await);
}

/// TD-FEE-69: the app role may insert and select, never update or delete a stored result.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_69_the_stored_result_is_append_only(pool: PgPool) {
    charged(&pool, 7).await;
    let mut conn = pool.acquire().await.expect("conn");
    sqlx::query("SET ROLE askhava_app")
        .execute(&mut *conn)
        .await
        .expect("role");
    let source = FakeSource::new(Some(ThreeDs::NotAuthenticated));
    record_three_ds(&mut conn, &source, pick(7))
        .await
        .expect("as app");
    for sql in [
        "UPDATE payments.charge_three_ds SET authenticated = true",
        "DELETE FROM payments.charge_three_ds",
    ] {
        let refused = sqlx::query(sql).execute(&mut *conn).await.expect_err(sql);
        let code = refused.as_database_error().and_then(|e| e.code());
        assert_eq!(code.as_deref(), Some("42501"), "{sql}");
    }
}
