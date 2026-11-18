//! T623 (FEE-018, FEE-020, FEE-037; D-4196, D-4280): a Stripe key refusal is Stripe down, never
//! the business's card. On a fresh database, a confirm Stripe refuses with 401 or 403 (a bad,
//! revoked or under-permissioned key), whose intent read is refused the same way, is stored
//! `unreachable` (the status T518's `stripe_errors` counts as a failed Stripe call) and answers
//! `StripeDown` with the 4-hour release and no card-fix deadline; a confirm 404 whose re-read of
//! our own stored `pi_` also answers 404 does the same; a 402, and a 400 whose read answers
//! `requires_payment_method`, still decline with the card-fix deadline. D-4196's known limit is
//! pinned: a restricted key refused at confirm becomes a decline when the release timer's ask
//! reads `requires_payment_method`. The "two attempts 31 minutes apart raise one alert" case is
//! T518's (D-4280).
//!
//! Stripe is a scripted fake of `IntentStripe` that records every call. All IDs are fake.

use std::collections::VecDeque;
use std::sync::Mutex;

use askhava_adapters::api::{
    AdapterError, Charge, IdempotencyKey, IntentMetadata, PaymentIntent, PaymentsAdapter,
};
use askhava_core::api::arizona;
use askhava_payments::api::{
    BusinessId, CARD_FIX_KIND, ChargeOutcome, FeeJob, FeeMode, FeeQuote, IntentStripe, PeriodJob,
    PickNow, Press, RecurringCap, Service, TIMER_KIND, VisitTier, confirm_and_pay, fee_at_pick,
    fix_fee_at_pick, run_pay_timer,
};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::Value;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb623_0001));
const INTENT: &str = "pi_FakeT623";
type Answer = Result<PaymentIntent, AdapterError>;

/// A scripted Stripe: each confirm and intent read takes the next scripted answer.
struct FakeStripe {
    calls: Mutex<Vec<String>>,
    confirms: Mutex<VecDeque<Answer>>,
    fetches: Mutex<VecDeque<Answer>>,
}

impl FakeStripe {
    fn new(confirms: Vec<Answer>, fetches: Vec<Answer>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            confirms: Mutex::new(confirms.into()),
            fetches: Mutex::new(fetches.into()),
        }
    }

    fn count(&self, call: &str) -> usize {
        let calls = self.calls.lock().expect("lock");
        calls.iter().filter(|c| c.starts_with(call)).count()
    }

    fn log(&self, call: String) {
        self.calls.lock().expect("lock").push(call);
    }
}

fn vendor(status: u16) -> Answer {
    Err(AdapterError::Vendor { status, code: None })
}

fn intent(status: &str) -> Answer {
    Ok(PaymentIntent {
        id: INTENT.to_owned(),
        status: status.to_owned(),
        amount: 2500,
        currency: "usd".to_owned(),
        customer: "cus_FakeT623".to_owned(),
        metadata: IntentMetadata {
            pick_id: "fake".to_owned(),
        },
        livemode: Some(false),
        latest_charge: None,
    })
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no customer is made here")
    }

    async fn create_payment_intent(&self, _: &IdempotencyKey, _: Charge<'_>) -> Answer {
        panic!("a charge never creates a second, confirmed intent (FEE-047)")
    }

    async fn fetch_payment_intent(&self, id: &str) -> Answer {
        self.log(format!("fetch {id}"));
        let next = self.fetches.lock().expect("lock").pop_front();
        next.unwrap_or(Err(AdapterError::NotSent))
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no refund is made here")
    }
}

impl IntentStripe for FakeStripe {
    async fn create_intent(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: u32,
        _: &str,
    ) -> Result<String, AdapterError> {
        self.log("create".to_owned());
        Ok(INTENT.to_owned())
    }

    async fn confirm_intent(&self, _: &IdempotencyKey, intent: &str, _: &str) -> Answer {
        self.log(format!("confirm {intent}"));
        let next = self.confirms.lock().expect("lock").pop_front();
        next.expect("a scripted confirm answer")
    }
}

/// Arizona wall-clock time on fake November 2026 days (the 3rd is a Tuesday).
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    jiff::civil::date(2026, 11, day)
        .at(hour, minute, 0, 0)
        .to_zoned(arizona())
        .expect("zoned")
        .timestamp()
}

fn press(pick: u128) -> Press<'static> {
    Press {
        pick: Uuid::from_u128(pick),
        business: BUSINESS,
        job: Uuid::from_u128(pick + 0x100),
        amount_cents: 2500,
        payment_method: "pm_FakeCardT623",
        press: Uuid::from_u128(pick * 100 + 1),
        emergency: false,
        now: PickNow::Payable,
    }
}

/// Stores the business's fake customer and the $25 fee fixed at `p`'s pick (D-1871).
async fn ready(pool: &PgPool, p: &Press<'_>) {
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)")
        .bind(BUSINESS.0)
        .bind("cus_FakeT623")
        .execute(pool)
        .await
        .expect("customer");
    let job = PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier: VisitTier::DEFAULT,
            fee_free: false,
        },
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    };
    let fee = fee_at_pick(FeeQuote::Fixed(10_000), None, job, FeeMode::Tiers, None).expect("fee");
    let mut conn = pool.acquire().await.expect("conn");
    fix_fee_at_pick(&mut conn, p.pick, BUSINESS, p.job, &fee, az(1, 9, 0))
        .await
        .expect("fee fixed");
}

/// One press at 9:00 on the 3rd with `stripe` answering.
async fn pay(pool: &PgPool, stripe: &FakeStripe, pick: u128) -> ChargeOutcome {
    let p = press(pick);
    ready(pool, &p).await;
    confirm_and_pay(
        pool,
        stripe,
        &SecretString::from("fake-secret-t623".to_owned()),
        &p,
        az(3, 9, 0),
    )
    .await
    .expect("press")
}

async fn attempt_codes(pool: &PgPool, pick: u128) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT status FROM payments.charge_attempt WHERE pick_id = $1 ORDER BY try_no",
    )
    .bind(Uuid::from_u128(pick))
    .fetch_all(pool)
    .await
    .expect("attempts")
}

async fn running_timers(pool: &PgPool, pick: u128) -> Vec<(String, Timestamp)> {
    let rows: Vec<(String, Option<jiff_sqlx::Timestamp>)> = sqlx::query_as(
        "SELECT kind, due_at FROM payments.pay_timer
         WHERE pick_id = $1 AND ended_at IS NULL ORDER BY kind",
    )
    .bind(Uuid::from_u128(pick))
    .fetch_all(pool)
    .await
    .expect("timers");
    rows.into_iter()
        .filter_map(|(kind, due)| due.map(|d| (kind, d.to_jiff())))
        .collect()
}

async fn tasks(pool: &PgPool, kind: &str) -> Vec<(Value, Timestamp)> {
    let rows: Vec<(Value, jiff_sqlx::Timestamp)> =
        sqlx::query_as("SELECT payload, run_at FROM core.task WHERE kind = $1 ORDER BY id")
            .bind(kind)
            .fetch_all(pool)
            .await
            .expect("tasks");
    rows.into_iter().map(|(p, t)| (p, t.to_jiff())).collect()
}

/// Asserts FEE-018's Stripe-down answer for `pick`: stored `unreachable`, `StripeDown`, the
/// 4-hour release running and no card-fix window or notice (FEE-020).
async fn assert_stripe_down(pool: &PgPool, outcome: ChargeOutcome, pick: u128) {
    assert_eq!(outcome, ChargeOutcome::StripeDown);
    assert_eq!(attempt_codes(pool, pick).await, ["unreachable"]);
    let timers = running_timers(pool, pick).await;
    assert!(
        timers.iter().all(|(kind, _)| kind != "card-fix"),
        "{timers:?}"
    );
    assert_eq!(tasks(pool, TIMER_KIND).await[0].1, az(3, 13, 0));
    assert!(tasks(pool, CARD_FIX_KIND).await.is_empty());
}

/// D-4196: a 401 on the confirm and on the intent read is Stripe down, not a decline.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_401_key_refusal_is_stripe_down(pool: PgPool) {
    let stripe = FakeStripe::new(vec![vendor(401)], vec![vendor(401)]);
    let outcome = pay(&pool, &stripe, 0x6231).await;
    assert_stripe_down(&pool, outcome, 0x6231).await;
    assert_eq!(stripe.count("confirm"), 1);
}

/// D-4196: a 403 on the confirm and on the intent read is Stripe down, not a decline.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_403_key_refusal_is_stripe_down(pool: PgPool) {
    let stripe = FakeStripe::new(vec![vendor(403)], vec![vendor(403)]);
    let outcome = pay(&pool, &stripe, 0x6232).await;
    assert_stripe_down(&pool, outcome, 0x6232).await;
}

/// D-4196: a confirm 404 reads the intent again; a 404 on our own stored `pi_` is our key or
/// account, so Stripe down.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_confirm_404_whose_read_is_404_is_stripe_down(pool: PgPool) {
    let stripe = FakeStripe::new(vec![vendor(404)], vec![vendor(404)]);
    let outcome = pay(&pool, &stripe, 0x6233).await;
    assert_eq!(stripe.count("fetch"), 1, "the confirm 404 reads the intent");
    assert_stripe_down(&pool, outcome, 0x6233).await;
}

/// D-4196: a confirm 400 whose read 401s or 403s is Stripe down too.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_refused_read_after_a_400_is_stripe_down(pool: PgPool) {
    let stripe = FakeStripe::new(vec![vendor(400)], vec![vendor(403)]);
    let outcome = pay(&pool, &stripe, 0x6234).await;
    assert_stripe_down(&pool, outcome, 0x6234).await;
}

/// FEE-020: a 402 still declines with the 1-business-hour card-fix deadline.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_402_still_declines(pool: PgPool) {
    let stripe = FakeStripe::new(vec![vendor(402)], vec![]);
    let outcome = pay(&pool, &stripe, 0x6235).await;
    assert_eq!(outcome, ChargeOutcome::Declined(Some(az(3, 10, 0))));
    assert_eq!(attempt_codes(&pool, 0x6235).await, ["declined"]);
}

/// FEE-020: a 400 whose read answers `requires_payment_method` still declines.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_400_read_as_requires_payment_method_still_declines(pool: PgPool) {
    let stripe = FakeStripe::new(vec![vendor(400)], vec![intent("requires_payment_method")]);
    let outcome = pay(&pool, &stripe, 0x6236).await;
    assert_eq!(outcome, ChargeOutcome::Declined(Some(az(3, 10, 0))));
    assert_eq!(attempt_codes(&pool, 0x6236).await, ["declined"]);
}

/// D-4196's known limit: a restricted key that can read but not confirm is Stripe down at first,
/// and becomes a decline, with its card-fix window, when the 4-hour release asks Stripe.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_read_only_key_becomes_a_decline_at_the_release(pool: PgPool) {
    let stripe = FakeStripe::new(vec![vendor(403)], vec![intent("requires_payment_method")]);
    let outcome = pay(&pool, &stripe, 0x6237).await;
    assert_stripe_down(&pool, outcome, 0x6237).await;
    let (payload, due) = tasks(&pool, TIMER_KIND).await[0].clone();
    run_pay_timer(&pool, &stripe, &payload, due)
        .await
        .expect("release");
    assert_eq!(attempt_codes(&pool, 0x6237).await, ["declined"]);
    assert_eq!(tasks(&pool, CARD_FIX_KIND).await.len(), 1);
}
