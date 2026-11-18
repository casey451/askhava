//! TD-FEE-72 (OPS-024, FEE-021, PICK-022, SEC-030, FEE-026; T706, D-4419): on PostgreSQL,
//! `held_release` re-clears a pick held while safe mode was on only from an intent re-fetched
//! from Stripe that passes `check_intent`, or, for a pick with no PaymentIntent, its recorded $0
//! decision. Every refused branch clears nothing; a pick ended in `payments` stores its success
//! and queues one late-success refund (FEE-021, D-2290), however often it is called; a pick
//! refunded or disputed while held clears nothing and queues no refund (FEE-026).
//!
//! Stripe is a fake: every fetch gives the same scripted answer and is counted. All IDs are fake.

use std::sync::atomic::{AtomicU32, Ordering};

use askhava_adapters::api::{
    AdapterError, Charge, IdempotencyKey, IntentMetadata, PaymentIntent, PaymentsAdapter,
};
use askhava_core::api::{NewTask, enqueue};
use askhava_privacy::api::ReleaseBasis;
use jiff::Timestamp;
use serde_json::json;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::api::{LATE_KIND, ReleaseError, StripeAccount, held_release};
use crate::handlers::record;
use crate::status::AttemptStatus;

const BUSINESS: Uuid = Uuid::from_u128(0xb706_0001);
const PICK: Uuid = Uuid::from_u128(0x706_0042);
const JOB: Uuid = Uuid::from_u128(0x706_0142);
const INTENT: &str = "pi_FakeT706";
const CHARGE: &str = "ch_FakeT706";
const NOW: i64 = 1_794_200_000;

/// A fake Stripe giving one answer to every fetch: `"down"` is unreachable, otherwise the status,
/// for the pick `pick_id`; fetches are counted.
struct FakeStripe {
    answer: &'static str,
    pick_id: Uuid,
    fetches: AtomicU32,
}

fn fake(answer: &'static str) -> FakeStripe {
    FakeStripe {
        answer,
        pick_id: PICK,
        fetches: AtomicU32::new(0),
    }
}

impl FakeStripe {
    fn fetched(&self) -> u32 {
        self.fetches.load(Ordering::SeqCst)
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

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        assert_eq!(id, INTENT);
        self.fetches.fetch_add(1, Ordering::SeqCst);
        if self.answer == "down" {
            return Err(AdapterError::NotSent);
        }
        Ok(PaymentIntent {
            id: INTENT.to_owned(),
            status: self.answer.to_owned(),
            amount: 7500,
            currency: "usd".to_owned(),
            customer: "cus_FakeT706".to_owned(),
            metadata: IntentMetadata {
                pick_id: self.pick_id.to_string(),
            },
            livemode: Some(false),
            latest_charge: (self.answer == "succeeded").then(|| CHARGE.to_owned()),
        })
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("held_release refunds nothing itself")
    }
}

fn at(offset: i64) -> Timestamp {
    Timestamp::from_second(NOW + offset).expect("time")
}

fn account() -> StripeAccount {
    StripeAccount {
        live: false,
        account: "acct_testfake".to_owned(),
    }
}

/// One $75 pick held with its one attempt `sent`, as a press during safe mode leaves it.
async fn seed_paid(pool: &PgPool) {
    sqlx::query!(
        "INSERT INTO payments.customer (business_id, stripe_customer_id)
         VALUES ($1, 'cus_FakeT706')",
        BUSINESS
    )
    .execute(pool)
    .await
    .expect("customer");
    sqlx::query!(
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at)
         VALUES ($1, $2, $3, 7500, $4, false, now() - interval '10 minutes')",
        PICK,
        BUSINESS,
        JOB,
        INTENT
    )
    .execute(pool)
    .await
    .expect("intent");
    sqlx::query!(
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at)
         VALUES ($1, 1, $2, 'sent', now() - interval '10 minutes')",
        PICK,
        BUSINESS
    )
    .execute(pool)
    .await
    .expect("attempt");
}

/// One $0 pick's fixed fee row (no PaymentIntent), with its $0 ledger decision when `decided`.
async fn seed_free(pool: &PgPool, decided: bool) {
    sqlx::query!(
        "INSERT INTO payments.pick_fee (pick_id, business_id, job_id, mode, kind, amount_cents,
           terms_version, fixed_at) VALUES ($1, $2, $3, 'tiers', 'fee_free', 0, 1, now())",
        PICK,
        BUSINESS,
        JOB
    )
    .execute(pool)
    .await
    .expect("fee");
    if decided {
        sqlx::query!(
            "INSERT INTO payments.ledger_entry (kind, business_id, job_id, amount_cents,
               idempotency_key) VALUES ('free-first-win', $1, $2, 0, 'free.t706')",
            BUSINESS,
            JOB
        )
        .execute(pool)
        .await
        .expect("decision");
    }
}

/// Whether the pick's one late-success refund task is queued: a probe with its dedupe key
/// queues nothing when one is (FEE-026, D-2290).
async fn late_queued(pool: &PgPool) -> bool {
    let mut probe = NewTask::new(LATE_KIND, json!({ "pick": PICK }), at(0));
    probe.dedupe_key = Some(format!("{LATE_KIND}.{PICK}"));
    let mut conn = pool.acquire().await.expect("conn");
    enqueue(&mut conn, &probe).await.expect("probe").is_none()
}

/// The pick's evidence records, its attempt's status and whether a late-success refund is queued.
async fn state(pool: &PgPool) -> (i64, String, bool) {
    let row = sqlx::query!(
        r#"SELECT (SELECT count(*) FROM payments.charge_evidence) AS "evidence!",
                  (SELECT status FROM payments.charge_attempt WHERE try_no = 1) AS "status!""#
    )
    .fetch_one(pool)
    .await
    .expect("state");
    (row.evidence, row.status, late_queued(pool).await)
}

async fn held(pool: &PgPool, stripe: &FakeStripe) -> Result<ReleaseBasis, ReleaseError> {
    held_release(pool, stripe, &account(), PICK, at(0))
        .await
        .map(|cleared| {
            assert_eq!(cleared.pick(), PICK);
            cleared.basis()
        })
}

/// A paid pick refused before any success is stored: nothing cleared, nothing written.
async fn refused_paid(pool: &PgPool, stripe: &FakeStripe, want: ReleaseError) {
    seed_paid(pool).await;
    assert_eq!(held(pool, stripe).await, Err(want));
    assert_eq!(stripe.fetched(), 1);
    assert_eq!(state(pool).await, (0, "sent".to_owned(), false));
}

/// TD-FEE-72 (OPS-024, SEC-049): Stripe unreachable clears nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_72_a_failed_fetch_clears_nothing(pool: PgPool) {
    refused_paid(&pool, &fake("down"), ReleaseError::Stripe).await;
}

/// TD-FEE-72 (OPS-024, PICK-022): an intent not succeeded clears nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_72_an_intent_not_succeeded_clears_nothing(pool: PgPool) {
    refused_paid(&pool, &fake("processing"), ReleaseError::NotPaid).await;
}

/// TD-FEE-72 (SEC-039): an intent whose metadata names another pick clears nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_72_an_intent_for_another_pick_clears_nothing(pool: PgPool) {
    let mut stripe = fake("succeeded");
    stripe.pick_id = Uuid::from_u128(0x706_0043);
    refused_paid(&pool, &stripe, ReleaseError::WrongPick).await;
}

/// TD-FEE-72 (FEE-036): no intent and no fee row: `NoFeeDecision`, Stripe not asked.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_72_a_pick_with_no_rows_clears_nothing(pool: PgPool) {
    let stripe = fake("succeeded");
    assert_eq!(held(&pool, &stripe).await, Err(ReleaseError::NoFeeDecision));
    assert_eq!(stripe.fetched(), 0);
}

/// TD-FEE-72 (FEE-036): a $0 fee row without its $0 ledger decision clears nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_72_a_free_pick_without_its_decision_clears_nothing(pool: PgPool) {
    seed_free(&pool, false).await;
    let stripe = fake("succeeded");
    assert_eq!(held(&pool, &stripe).await, Err(ReleaseError::NoFeeDecision));
    assert_eq!(stripe.fetched(), 0);
}

/// TD-FEE-72 (OPS-024, FEE-010): a $0 pick with its recorded decision clears, Stripe not asked.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_72_a_free_pick_with_its_decision_clears(pool: PgPool) {
    seed_free(&pool, true).await;
    let stripe = fake("succeeded");
    assert_eq!(held(&pool, &stripe).await, Ok(ReleaseBasis::NoFeeDue));
    assert_eq!(stripe.fetched(), 0);
}

/// TD-FEE-72 (OPS-024, PICK-022): a verified, re-fetched success clears and is stored, with no
/// late-success refund.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_72_a_paid_pick_clears_on_a_verified_success(pool: PgPool) {
    seed_paid(&pool).await;
    let stripe = fake("succeeded");
    assert_eq!(held(&pool, &stripe).await, Ok(ReleaseBasis::PaidCharge));
    assert_eq!(state(&pool).await, (1, "succeeded".to_owned(), false));
}

/// TD-FEE-72 (FEE-021, D-2290): a pick ended in `payments` while held clears nothing, stores the
/// success and queues one late-success refund; a second call queues no second.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_72_an_ended_pick_queues_one_late_refund(pool: PgPool) {
    seed_paid(&pool).await;
    sqlx::query!(
        "UPDATE payments.pick_intent SET ended_at = now() - interval '1 minute'
         WHERE pick_id = $1",
        PICK
    )
    .execute(&pool)
    .await
    .expect("ended");
    let stripe = fake("succeeded");
    for _ in 0..2 {
        assert_eq!(held(&pool, &stripe).await, Err(ReleaseError::PickEnded));
        assert_eq!(state(&pool).await, (1, "succeeded".to_owned(), true));
    }
}

/// A paid pick whose success was stored before safe mode lifted, as a press's answer stores it.
async fn stored_success(pool: &PgPool) {
    seed_paid(pool).await;
    let paid = (AttemptStatus::Succeeded, Some(CHARGE));
    record(pool, PICK, 1, paid, at(-300)).await.expect("stored");
}

/// After a refund or a dispute landed while held: `PickEnded`, nothing cleared, no refund queued
/// (FEE-026).
async fn money_back_clears_nothing(pool: &PgPool) {
    let stripe = fake("succeeded");
    assert_eq!(held(pool, &stripe).await, Err(ReleaseError::PickEnded));
    assert_eq!(state(pool).await, (1, "succeeded".to_owned(), false));
}

/// TD-FEE-72 (FEE-026, D-4419): a pick refunded while held (an early fraud warning) clears nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_72_a_pick_refunded_while_held_clears_nothing(pool: PgPool) {
    stored_success(&pool).await;
    sqlx::query!(
        "INSERT INTO payments.refund (pick_id, trigger, status, tries, stripe_refund_id,
           requested_at) VALUES ($1, 'fraud-warning', 'succeeded', 1, 're_FakeT706', now())",
        PICK
    )
    .execute(&pool)
    .await
    .expect("refund");
    money_back_clears_nothing(&pool).await;
}

/// TD-FEE-72 (FEE-026, D-4419): a pick disputed while held clears nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_72_a_pick_disputed_while_held_clears_nothing(pool: PgPool) {
    stored_success(&pool).await;
    sqlx::query!(
        "INSERT INTO payments.dispute (stripe_dispute_id, pick_id, amount_cents, due_at, opened_at)
         VALUES ('dp_FakeT706', $1, 7500, now() + interval '7 days', now())",
        PICK
    )
    .execute(&pool)
    .await
    .expect("dispute");
    money_back_clears_nothing(&pool).await;
}
