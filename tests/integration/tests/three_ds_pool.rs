//! D-4052 (FEE-050, FEE-027; rust brain R11), against PostgreSQL: the `picks.dispute_opened`
//! handler holds no pooled connection across the Stripe 3-D Secure read.
//!
//! The pool has one connection and the fake `ThreeDsStripe` takes a connection from it inside its
//! call, so a handler that held its connection across Stripe would wait for ever; the run is
//! under `tokio::time::timeout`, so it fails instead of hanging. The run finishes, stores the
//! result once, and a second run asks Stripe nothing. Every ID is fake.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use askhava_adapters::api::{AdapterError, ThreeDs};
use askhava_core::api::Claimed;
use askhava_payments::api::ThreeDsStripe;
use askhava_picks::api::{StepParts, run_dispute_opened};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::types::Uuid;

const PICK: Uuid = Uuid::from_u128(0x587_0a01);
const BUSINESS: Uuid = Uuid::from_u128(0x587_0b01);
const JOB: Uuid = Uuid::from_u128(0x587_0c01);
const INTENT: &str = "pi_FakeT587n1";
const NOW: i64 = 1_793_000_000;

/// A fake Stripe that takes a connection from the handler's own pool while it answers.
struct PoolStripe {
    pool: PgPool,
    asked: AtomicUsize,
}

impl ThreeDsStripe for PoolStripe {
    async fn three_ds(&self, intent: &str) -> Result<ThreeDs, AdapterError> {
        assert_eq!(intent, INTENT);
        self.asked.fetch_add(1, Ordering::SeqCst);
        let held = self
            .pool
            .acquire()
            .await
            .map_err(|_| AdapterError::OutcomeUnknown)?;
        drop(held);
        Ok(ThreeDs::Authenticated)
    }
}

/// The pick's $75 charge with its FEE-042 evidence record, as T062 and T189 write it.
async fn charged(pool: &PgPool) {
    for sql in [
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false, to_timestamp($5))",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at,
           decided_at) VALUES ($1, 1, $2, 'succeeded', to_timestamp($5), to_timestamp($5))",
        "INSERT INTO payments.charge_evidence (pick_id, business_id, job_id, press_id, try_no,
           idempotency_label, stripe_customer_id, stripe_intent_id, authorization_version,
           amount_cents, pressed_at, charged_at) VALUES ($1, $2, $3, $2, 1,
           'pick-' || $1::text || '-try1', 'cus_FakeT587', $4, 1, 7500, to_timestamp($5),
           to_timestamp($5))",
    ] {
        sqlx::query(sql)
            .bind(PICK)
            .bind(BUSINESS)
            .bind(JOB)
            .bind(INTENT)
            .bind(NOW)
            .execute(pool)
            .await
            .expect("seed");
    }
}

/// One `picks.dispute_opened` run for the pick, failing rather than hanging.
async fn run(pool: &PgPool, parts: &Arc<StepParts<PoolStripe>>) {
    let task = Claimed {
        id: 1,
        kind: "picks.dispute_opened".to_owned(),
        payload: serde_json::json!({ "dispute": "dp_FakeT587", "pick": PICK.to_string() }),
        attempts: 1,
    };
    let now = Timestamp::from_second(NOW).expect("time");
    let ran = run_dispute_opened(pool.clone(), Arc::clone(parts), task, now);
    tokio::time::timeout(Duration::from_secs(10), ran)
        .await
        .expect("the run holds no connection across Stripe")
        .expect("the run succeeds");
}

/// D-4052, FEE-050: with one pooled connection the run finishes, the result is stored once and a
/// second run asks Stripe nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_dispute_run_holds_no_connection_across_stripe(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let pool = opts
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(30))
        .connect_with(conn)
        .await
        .expect("pool");
    charged(&pool).await;
    let parts = Arc::new(StepParts {
        stripe: Some(PoolStripe {
            pool: pool.clone(),
            asked: AtomicUsize::new(0),
        }),
        secret: SecretString::from("fake-t587-id-secret"),
        vault: None,
        postal: None,
    });
    run(&pool, &parts).await;
    let stored: Vec<bool> =
        sqlx::query_scalar("SELECT authenticated FROM payments.charge_three_ds WHERE pick_id = $1")
            .bind(PICK)
            .fetch_all(&pool)
            .await
            .expect("rows");
    assert_eq!(stored, vec![true]);
    run(&pool, &parts).await;
    let asked = parts
        .stripe
        .as_ref()
        .map(|s| s.asked.load(Ordering::SeqCst));
    assert_eq!(asked, Some(1));
}
