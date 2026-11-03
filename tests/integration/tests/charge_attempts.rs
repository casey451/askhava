//! TD-FEE-11, TD-FEE-20, TD-FEE-21 (monthly-limit case), TD-FEE-24 (emergency decline case),
//! TD-FEE-32 (one PaymentIntent case) and TD-FEE-36 (FEE-016 to FEE-020, FEE-037, FEE-038,
//! FEE-041, FEE-047; D-135, D-153, D-204, D-270, D-365, D-490): on a fresh database, every press
//! confirms the pick's one PaymentIntent with its own key, a repeated press reuses its attempt, a
//! timed-out attempt blocks a new try until Stripe says it failed, the 4-hour release and the
//! card-fix window end the pick, the charging pause stops and resumes both timers, and a press
//! over the monthly limit charges nothing and ends the pick.
//!
//! Stripe is a scripted fake of `IntentStripe` that records every call; the stripe-mock round
//! trip is T132's (D-1424). TD-FEE-20's reconcile refund of a second kept charge is T067's; the
//! limit field and the homeowner's text are TD-FEE-40's (D-492). All IDs are fake.

use std::collections::VecDeque;
use std::sync::Mutex;

use askhava_adapters::api::{
    AdapterError, Charge, IdempotencyKey, IntentMetadata, PaymentIntent, PaymentsAdapter,
};
use askhava_core::api::{Claimed, arizona};
use askhava_ops::api::{Actor, Switch, set_switch};
use askhava_payments::api::{
    ATTEMPT_ACTION, BusinessId, ChargeError, ChargeOutcome, INTENT_ACTION, IntentStripe, PickNow,
    Press, attempt_label, confirm_and_pay, intent_label, open_intent, run_charge_check,
    run_pause_watch, run_pay_timer, set_fee_limit, sync_pause,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::Value;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb062_0001));
type Answer = Result<PaymentIntent, AdapterError>;

/// A scripted Stripe: each confirm and fetch takes the next scripted answer.
struct FakeStripe {
    calls: Mutex<Vec<String>>,
    intent_id: String,
    confirms: Mutex<VecDeque<Answer>>,
    fetches: Mutex<VecDeque<Answer>>,
}

impl FakeStripe {
    fn new(confirms: Vec<Answer>, fetches: Vec<Answer>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            intent_id: "pi_FakeT062".to_owned(),
            confirms: Mutex::new(confirms.into()),
            fetches: Mutex::new(fetches.into()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("lock").clone()
    }

    fn log(&self, call: String) {
        self.calls.lock().expect("lock").push(call);
    }
}

fn intent(status: &str) -> Answer {
    Ok(PaymentIntent {
        id: "pi_FakeT062".to_owned(),
        status: status.to_owned(),
        amount: 2500,
        currency: "usd".to_owned(),
        customer: "cus_FakeT062".to_owned(),
        metadata: IntentMetadata {
            pick_id: "fake".to_owned(),
        },
        livemode: Some(false),
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
        key: &IdempotencyKey,
        customer: &str,
        cents: u32,
        _: &str,
    ) -> Result<String, AdapterError> {
        self.log(format!("create {} {customer} {cents}", key.as_str()));
        let made = self
            .calls()
            .iter()
            .filter(|c| c.starts_with("create"))
            .count();
        Ok(if made == 1 {
            self.intent_id.clone()
        } else {
            format!("{}_{made}", self.intent_id)
        })
    }

    async fn confirm_intent(&self, key: &IdempotencyKey, intent: &str, pm: &str) -> Answer {
        self.log(format!("confirm {} {intent} {pm}", key.as_str()));
        let next = self.confirms.lock().expect("lock").pop_front();
        next.expect("a scripted confirm answer")
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t062".to_owned())
}

/// Arizona wall-clock time on fake days in November 2026.
fn az(day: i8, hour: i8, minute: i8) -> Timestamp {
    jiff::civil::date(2026, 11, day)
        .at(hour, minute, 0, 0)
        .to_zoned(arizona())
        .expect("zoned")
        .timestamp()
}

fn press(pick: u128, n: u128, cents: u32) -> Press<'static> {
    Press {
        pick: Uuid::from_u128(pick),
        business: BUSINESS,
        job: Uuid::from_u128(pick + 0x100),
        amount_cents: cents,
        payment_method: "pm_FakeCard",
        press: Uuid::from_u128(pick * 100 + n),
        emergency: false,
        now: PickNow::Payable,
    }
}

fn key(action: &str, label: &str) -> String {
    IdempotencyKey::derive(&secret(), action, &[label])
        .as_str()
        .to_owned()
}

async fn with_customer(pool: &PgPool) {
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)")
        .bind(BUSINESS.0)
        .bind("cus_FakeT062")
        .execute(pool)
        .await
        .expect("customer");
}

async fn pay(pool: &PgPool, stripe: &FakeStripe, p: &Press<'_>, at: Timestamp) -> ChargeOutcome {
    confirm_and_pay(pool, stripe, &secret(), p, at)
        .await
        .expect("press")
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

/// TD-FEE-11, TD-FEE-32 (FEE-016, FEE-047): a decline then a press from another session reach
/// one PaymentIntent, created once under pick-<id>-intent, with two confirms keyed try1 and try2;
/// the repeated first press and a press after the success charge nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_11_td_fee_32_one_intent_and_a_key_per_attempt(pool: PgPool) {
    with_customer(&pool).await;
    let stripe = FakeStripe::new(
        vec![intent("requires_payment_method"), intent("succeeded")],
        vec![],
    );
    let (first, second) = (press(0x1042, 1, 2500), press(0x1042, 2, 2500));
    let declined = pay(&pool, &stripe, &first, az(3, 10, 0)).await;
    assert_eq!(declined, ChargeOutcome::Declined(Some(az(3, 11, 0))));
    assert_eq!(pay(&pool, &stripe, &first, az(3, 10, 1)).await, declined);
    let paid = pay(&pool, &stripe, &second, az(3, 10, 2)).await;
    assert_eq!(paid, ChargeOutcome::Succeeded("pi_FakeT062".to_owned()));
    let third = press(0x1042, 3, 2500);
    assert_eq!(
        pay(&pool, &stripe, &third, az(3, 10, 3)).await,
        ChargeOutcome::AlreadyPaid
    );
    let pick = first.pick;
    assert_eq!(
        stripe.calls(),
        [
            format!(
                "create {} cus_FakeT062 2500",
                key(INTENT_ACTION, &intent_label(pick))
            ),
            format!(
                "confirm {} pi_FakeT062 pm_FakeCard",
                key(ATTEMPT_ACTION, &attempt_label(pick, 1))
            ),
            format!(
                "confirm {} pi_FakeT062 pm_FakeCard",
                key(ATTEMPT_ACTION, &attempt_label(pick, 2))
            ),
        ]
    );
    assert_eq!(attempt_label(pick, 1), format!("pick-{pick}-try1"));
    let notices = tasks(&pool, askhava_payments::api::CARD_FIX_KIND).await;
    assert_eq!(notices.len(), 1, "one FEE-020 notice for the decline");
}

/// TD-FEE-11 (FEE-017): a press after the job moved on charges nothing and calls no confirm.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_11_a_press_after_the_job_moved_on_charges_nothing(pool: PgPool) {
    with_customer(&pool).await;
    let stripe = FakeStripe::new(vec![], vec![]);
    let mut late = press(0x1043, 1, 2500);
    late.now = PickNow::MovedOn;
    assert_eq!(
        pay(&pool, &stripe, &late, az(3, 10, 0)).await,
        ChargeOutcome::MovedOn
    );
    assert!(stripe.calls().iter().all(|c| !c.starts_with("confirm")));
}

/// TD-FEE-11 (FEE-018): Stripe unreachable starts the 4-hour release; its timer releases the
/// pick at 4 hours, and not before.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_11_stripe_down_4_hours_releases_the_pick(pool: PgPool) {
    with_customer(&pool).await;
    let stripe = FakeStripe::new(vec![Err(AdapterError::NotSent)], vec![]);
    let p = press(0x1044, 1, 2500);
    assert_eq!(
        pay(&pool, &stripe, &p, az(3, 9, 0)).await,
        ChargeOutcome::StripeDown
    );
    let timers = tasks(&pool, askhava_payments::api::TIMER_KIND).await;
    let (payload, due) = timers.first().expect("the 4-hour timer").clone();
    assert_eq!(due, az(3, 13, 0));
    assert!(
        run_pay_timer(&pool, &stripe, &payload, az(3, 12, 59))
            .await
            .is_err()
    );
    run_pay_timer(&pool, &stripe, &payload, due)
        .await
        .expect("timer");
    let ended = tasks(&pool, askhava_payments::api::PICK_ENDED_KIND).await;
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].0["reason"], "stripe-down");
}

/// TD-FEE-11 (FEE-019): no notice 30 minutes after a processing charge, the check asks Stripe.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_11_no_notice_in_30_minutes_asks_stripe_directly(pool: PgPool) {
    with_customer(&pool).await;
    let stripe = FakeStripe::new(vec![intent("processing")], vec![intent("succeeded")]);
    let p = press(0x1045, 1, 2500);
    assert_eq!(
        pay(&pool, &stripe, &p, az(3, 9, 0)).await,
        ChargeOutcome::Pending
    );
    let checks = tasks(&pool, askhava_payments::api::CHECK_KIND).await;
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].1, az(3, 9, 30));
    run_charge_check(&pool, &stripe, &checks[0].0, az(3, 9, 30))
        .await
        .expect("check");
    assert!(stripe.calls().contains(&"fetch pi_FakeT062".to_owned()));
    let again = press(0x1045, 2, 2500);
    assert_eq!(
        pay(&pool, &stripe, &again, az(3, 9, 31)).await,
        ChargeOutcome::AlreadyPaid
    );
}

/// TD-FEE-11 (FEE-020, D-270): a decline at 7:30 p.m. gets 30 minutes that evening and 30 from
/// 8:00 a.m.; the window's end, after Stripe is asked again, ends the pick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_11_a_decline_at_7_30_pm_gets_one_business_hour(pool: PgPool) {
    with_customer(&pool).await;
    let declined = || intent("requires_payment_method");
    let stripe = FakeStripe::new(vec![declined()], vec![declined()]);
    let p = press(0x1046, 1, 7500);
    let outcome = pay(&pool, &stripe, &p, az(3, 19, 30)).await;
    assert_eq!(outcome, ChargeOutcome::Declined(Some(az(4, 8, 30))));
    let (payload, due) = tasks(&pool, askhava_payments::api::TIMER_KIND).await[0].clone();
    run_pay_timer(&pool, &stripe, &payload, due)
        .await
        .expect("timer");
    assert!(
        stripe.calls().contains(&"fetch pi_FakeT062".to_owned()),
        "asked again"
    );
    let ended = tasks(&pool, askhava_payments::api::PICK_ENDED_KIND).await;
    assert_eq!(ended[0].0["reason"], "card-fix");
    let late = press(0x1046, 2, 7500);
    assert_eq!(
        pay(&pool, &stripe, &late, az(4, 8, 31)).await,
        ChargeOutcome::MovedOn
    );
}

/// TD-FEE-24 (FEE-020, PICK-044): a declined card on an emergency job at 22:00 has 60 clock
/// minutes; the picks handler queues the business's card-fix notice with that deadline.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_24_an_emergency_decline_at_22_00_has_60_clock_minutes(pool: PgPool) {
    with_customer(&pool).await;
    let mut conn = pool.acquire().await.expect("conn");
    let job = askhava_jobs::api::create(&mut conn, az(3, 21, 0), true, Actor::System)
        .await
        .expect("job");
    let stripe = FakeStripe::new(
        vec![Err(AdapterError::Vendor {
            status: 402,
            code: None,
        })],
        vec![],
    );
    let mut p = press(0x1047, 1, 2500);
    (p.emergency, p.job) = (true, job);
    let outcome = pay(&pool, &stripe, &p, az(3, 22, 0)).await;
    assert_eq!(outcome, ChargeOutcome::Declined(Some(az(3, 23, 0))));
    let (event, _) = tasks(&pool, askhava_payments::api::CARD_FIX_KIND).await[0].clone();
    let task = Claimed {
        id: 1,
        kind: String::new(),
        payload: event,
        attempts: 1,
    };
    askhava_picks::api::run_card_fix_notice(pool.clone(), task)
        .await
        .expect("notice");
    let (notice, _) = tasks(&pool, askhava_picks::api::BUSINESS_NOTICE_KIND).await[0].clone();
    assert_eq!(notice["template"], "card_fix_notice");
    assert_eq!(notice["values"]["time"], az(3, 23, 0).as_second());
    assert!(notice["values"]["job"].as_i64().is_some_and(|n| n >= 1001));
}

/// TD-FEE-20 (FEE-037, D-153): a timed-out try1 blocks try2 while Stripe says nothing or
/// processing; once Stripe says it failed, try2 goes; a try1 found succeeded means no try2.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_20_no_try2_until_stripe_says_try1_failed(pool: PgPool) {
    with_customer(&pool).await;
    let fetches = vec![intent("processing"), intent("requires_payment_method")];
    let stripe = FakeStripe::new(
        vec![Err(AdapterError::OutcomeUnknown), intent("succeeded")],
        fetches,
    );
    let pick = 0x1048;
    assert_eq!(
        pay(&pool, &stripe, &press(pick, 1, 2500), az(3, 9, 0)).await,
        ChargeOutcome::Pending
    );
    assert_eq!(
        pay(&pool, &stripe, &press(pick, 2, 2500), az(3, 9, 1)).await,
        ChargeOutcome::Pending
    );
    let paid = pay(&pool, &stripe, &press(pick, 3, 2500), az(3, 9, 2)).await;
    assert_eq!(paid, ChargeOutcome::Succeeded("pi_FakeT062".to_owned()));
    assert_eq!(
        pay(&pool, &stripe, &press(pick, 4, 2500), az(3, 9, 3)).await,
        ChargeOutcome::AlreadyPaid
    );
    let confirms = stripe
        .calls()
        .iter()
        .filter(|c| c.starts_with("confirm"))
        .count();
    assert_eq!(
        confirms, 2,
        "try1, then try2 only after Stripe said try1 failed"
    );

    let mut found = FakeStripe::new(
        vec![Err(AdapterError::OutcomeUnknown)],
        vec![intent("succeeded")],
    );
    found.intent_id = "pi_FakeT062b".to_owned();
    let other = 0x1049;
    pay(&pool, &found, &press(other, 1, 2500), az(3, 9, 0)).await;
    let again = pay(&pool, &found, &press(other, 2, 2500), az(3, 9, 5)).await;
    assert_eq!(again, ChargeOutcome::AlreadyPaid);
    assert_eq!(
        found
            .calls()
            .iter()
            .filter(|c| c.starts_with("confirm"))
            .count(),
        1
    );
}

/// TD-FEE-20 (FEE-038, D-135): the charging switch stops the card-fix timer with its time left,
/// refuses a press meanwhile, and resumes the timer with that time.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_20_the_charging_pause_stops_and_resumes_the_card_fix_timer(pool: PgPool) {
    with_customer(&pool).await;
    let stripe = FakeStripe::new(vec![intent("requires_payment_method")], vec![]);
    let pick = 0x1050;
    pay(&pool, &stripe, &press(pick, 1, 2500), az(3, 10, 0)).await;
    let mut conn = pool.acquire().await.expect("conn");
    let owner = Actor::Owner(Uuid::from_u128(0x0e));
    set_switch(&mut conn, Switch::Charging, true, owner)
        .await
        .expect("pause");
    sync_pause(&pool, az(3, 10, 20)).await.expect("stop");
    let (first, due) = tasks(&pool, askhava_payments::api::TIMER_KIND).await[0].clone();
    run_pay_timer(&pool, &stripe, &first, due)
        .await
        .expect("a stopped timer does nothing");
    assert_eq!(
        pay(&pool, &stripe, &press(pick, 2, 2500), az(3, 11, 0)).await,
        ChargeOutcome::Paused
    );
    set_switch(&mut conn, Switch::Charging, false, owner)
        .await
        .expect("resume");
    sync_pause(&pool, az(3, 15, 0)).await.expect("resume");
    let timers = tasks(&pool, askhava_payments::api::TIMER_KIND).await;
    assert_eq!(
        timers.last().expect("resumed").1,
        az(3, 15, 40),
        "40 minutes left"
    );
    assert!(
        tasks(&pool, askhava_payments::api::PICK_ENDED_KIND)
            .await
            .is_empty()
    );
}

/// TD-FEE-21, TD-FEE-36 (FEE-041, D-204): with a $100 limit and $75 charged this month, a $25
/// press is charged; a further one is not, ends its pick at once and calls no confirm.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_21_td_fee_36_a_press_over_the_monthly_limit_is_not_charged(pool: PgPool) {
    with_customer(&pool).await;
    let mut conn = pool.acquire().await.expect("conn");
    set_fee_limit(&mut conn, BUSINESS, Some(10_000))
        .await
        .expect("limit");
    let paid = || intent("succeeded");
    let stripe = FakeStripe::new(vec![paid(), paid()], vec![]);
    let ok = |o: ChargeOutcome| assert!(matches!(o, ChargeOutcome::Succeeded(_)), "{o:?}");
    ok(pay(&pool, &stripe, &press(0x1051, 1, 7500), az(3, 9, 0)).await);
    ok(pay(&pool, &stripe, &press(0x1052, 1, 2500), az(3, 9, 5)).await);
    let over = pay(&pool, &stripe, &press(0x1053, 1, 2500), az(3, 9, 10)).await;
    assert_eq!(over, ChargeOutcome::OverLimit);
    let creates = stripe
        .calls()
        .iter()
        .filter(|c| c.starts_with("create"))
        .count();
    assert_eq!(
        creates, 2,
        "Stripe is not called for the press over the limit"
    );
    assert_eq!(
        stripe
            .calls()
            .iter()
            .filter(|c| c.starts_with("confirm"))
            .count(),
        2
    );
    let ended = tasks(&pool, askhava_payments::api::PICK_ENDED_KIND).await;
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].0["reason"], "fee-limit");
}

fn count(stripe: &FakeStripe, call: &str) -> usize {
    stripe
        .calls()
        .iter()
        .filter(|c| c.starts_with(call))
        .count()
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

async fn running_timer(pool: &PgPool, pick: u128, kind: &str) -> Option<Timestamp> {
    let due: Option<Option<jiff_sqlx::Timestamp>> = sqlx::query_scalar(
        "SELECT due_at FROM payments.pay_timer WHERE pick_id = $1 AND kind = $2 AND ended_at IS NULL",
    )
    .bind(Uuid::from_u128(pick))
    .bind(kind)
    .fetch_optional(pool)
    .await
    .expect("timer");
    due.flatten().map(|d| d.to_jiff())
}

fn claimed(payload: Value) -> Claimed {
    Claimed {
        id: 1,
        kind: String::new(),
        payload,
        attempts: 1,
    }
}

/// TD-FEE-20 (FEE-016, FEE-037): a second press while the first confirm may still be on its way
/// sees an intent that still wants a card and waits, never recording a decline; a minute on,
/// the same answer is a failure and try2 goes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_20_a_press_while_a_confirm_is_in_flight_waits(pool: PgPool) {
    with_customer(&pool).await;
    let wants = || intent("requires_payment_method");
    let stripe = FakeStripe::new(
        vec![Err(AdapterError::OutcomeUnknown), intent("succeeded")],
        vec![wants(), wants()],
    );
    let pick = 0x1054;
    pay(&pool, &stripe, &press(pick, 1, 2500), az(3, 9, 0)).await;
    let soon = az(3, 9, 0)
        .saturating_add(SignedDuration::from_secs(30))
        .expect("time");
    assert_eq!(
        pay(&pool, &stripe, &press(pick, 2, 2500), soon).await,
        ChargeOutcome::Pending
    );
    assert_eq!(attempt_codes(&pool, pick).await, ["sent"]);
    let paid = pay(&pool, &stripe, &press(pick, 3, 2500), az(3, 9, 2)).await;
    assert!(matches!(paid, ChargeOutcome::Succeeded(_)), "{paid:?}");
    assert_eq!(attempt_codes(&pool, pick).await, ["declined", "succeeded"]);
}

/// TD-FEE-20 (FEE-016, FEE-019): a success from Stripe always wins: a decline Stripe later
/// reports as succeeded becomes the success at the card-fix window's end, and the pick stays.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_20_a_later_success_replaces_a_decline(pool: PgPool) {
    with_customer(&pool).await;
    let stripe = FakeStripe::new(
        vec![intent("requires_payment_method")],
        vec![intent("succeeded")],
    );
    let pick = 0x1055;
    pay(&pool, &stripe, &press(pick, 1, 2500), az(3, 10, 0)).await;
    let (payload, due) = tasks(&pool, askhava_payments::api::TIMER_KIND).await[0].clone();
    run_pay_timer(&pool, &stripe, &payload, due)
        .await
        .expect("timer");
    assert_eq!(attempt_codes(&pool, pick).await, ["succeeded"]);
    assert!(
        tasks(&pool, askhava_payments::api::PICK_ENDED_KIND)
            .await
            .is_empty()
    );
    assert_eq!(
        pay(&pool, &stripe, &press(pick, 2, 2500), az(3, 11, 1)).await,
        ChargeOutcome::AlreadyPaid
    );
}

/// TD-FEE-11 (FEE-018, FEE-020): at the 4-hour release Stripe answers that the card still needs
/// one: the pick is not released as Stripe down; the card-fix window opens instead, and its
/// notice is queued. At the window's end with Stripe unreachable, the pick is not ended either:
/// the window asks again 5 minutes on (FEE-019, D-153).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_11_a_stripe_answer_at_the_release_opens_the_card_fix_window(pool: PgPool) {
    with_customer(&pool).await;
    let stripe = FakeStripe::new(
        vec![Err(AdapterError::NotSent)],
        vec![intent("requires_payment_method")],
    );
    let pick = 0x1056;
    pay(&pool, &stripe, &press(pick, 1, 2500), az(3, 9, 0)).await;
    let (payload, due) = tasks(&pool, askhava_payments::api::TIMER_KIND).await[0].clone();
    run_pay_timer(&pool, &stripe, &payload, due)
        .await
        .expect("release");
    assert!(
        tasks(&pool, askhava_payments::api::PICK_ENDED_KIND)
            .await
            .is_empty()
    );
    assert_eq!(
        running_timer(&pool, pick, "card-fix").await,
        Some(az(3, 14, 0))
    );
    assert_eq!(
        tasks(&pool, askhava_payments::api::CARD_FIX_KIND)
            .await
            .len(),
        1
    );
    let (payload, due) = tasks(&pool, askhava_payments::api::TIMER_KIND)
        .await
        .last()
        .expect("card-fix timer")
        .clone();
    run_pay_timer(&pool, &stripe, &payload, due)
        .await
        .expect("asks again later");
    assert!(
        tasks(&pool, askhava_payments::api::PICK_ENDED_KIND)
            .await
            .is_empty()
    );
    assert_eq!(
        running_timer(&pool, pick, "card-fix").await,
        Some(az(3, 14, 5))
    );
}

/// TD-FEE-11 (FEE-019): a check that finds the attempt still processing queues the next round,
/// so a processing attempt never blocks the pick without a check.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_11_a_still_processing_attempt_is_checked_again(pool: PgPool) {
    with_customer(&pool).await;
    let stripe = FakeStripe::new(vec![intent("processing")], vec![intent("processing")]);
    pay(&pool, &stripe, &press(0x1057, 1, 2500), az(3, 9, 0)).await;
    let first = tasks(&pool, askhava_payments::api::CHECK_KIND).await[0].clone();
    run_charge_check(&pool, &stripe, &first.0, first.1)
        .await
        .expect("check");
    let checks = tasks(&pool, askhava_payments::api::CHECK_KIND).await;
    assert_eq!(checks.len(), 2);
    assert_eq!(
        (checks[1].0["round"].as_i64(), checks[1].1),
        (Some(1), az(3, 10, 0))
    );
}

/// TD-FEE-20 (FEE-038, D-135): the pause watch stops a running card-fix timer when the charging
/// switch goes on and resumes it with its time left when it goes off, with no call from the
/// switch's own code.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_20_the_pause_watch_follows_the_charging_switch(pool: PgPool) {
    with_customer(&pool).await;
    let stripe = FakeStripe::new(vec![intent("requires_payment_method")], vec![]);
    let pick = 0x1058;
    pay(&pool, &stripe, &press(pick, 1, 2500), az(3, 10, 0)).await;
    assert!(
        !tasks(&pool, askhava_payments::api::PAUSE_KIND)
            .await
            .is_empty()
    );
    let mut conn = pool.acquire().await.expect("conn");
    let owner = Actor::Owner(Uuid::from_u128(0x0e));
    set_switch(&mut conn, Switch::Charging, true, owner)
        .await
        .expect("pause");
    run_pause_watch(&pool, az(3, 10, 15)).await.expect("watch");
    assert_eq!(running_timer(&pool, pick, "card-fix").await, None);
    set_switch(&mut conn, Switch::Charging, false, owner)
        .await
        .expect("resume");
    run_pause_watch(&pool, az(3, 13, 0)).await.expect("watch");
    assert_eq!(
        running_timer(&pool, pick, "card-fix").await,
        Some(az(3, 13, 45))
    );
}

/// TD-FEE-11 (FEE-016, FEE-017): a confirm Stripe refuses with a 400 charged nothing: the intent
/// read again says the pick is paid; and a press from another business charges nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_11_a_refused_confirm_reads_the_intent_and_another_business_is_refused(
    pool: PgPool,
) {
    with_customer(&pool).await;
    let refused = Err(AdapterError::Vendor {
        status: 400,
        code: None,
    });
    let stripe = FakeStripe::new(vec![refused], vec![intent("succeeded")]);
    let pick = 0x1059;
    let paid = pay(&pool, &stripe, &press(pick, 1, 2500), az(3, 9, 0)).await;
    assert!(matches!(paid, ChargeOutcome::Succeeded(_)), "{paid:?}");
    let other = Press {
        business: BusinessId(Uuid::from_u128(0xb062_0002)),
        ..press(0x1060, 2, 2500)
    };
    let mut mine = FakeStripe::new(vec![intent("requires_payment_method")], vec![]);
    mine.intent_id = "pi_FakeT062c".to_owned();
    pay(&pool, &mine, &press(0x1060, 1, 2500), az(3, 9, 0)).await;
    assert_eq!(
        pay(&pool, &mine, &other, az(3, 9, 5)).await,
        ChargeOutcome::MovedOn
    );
    assert_eq!(count(&mine, "confirm"), 1);
}

/// TD-FEE-24 (FEE-020): the card-fix notice is not queued for a pick that ended before the
/// handler ran, and a bank approval uses its own notice.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_24_no_card_fix_notice_for_an_ended_pick_and_an_approval_notice(pool: PgPool) {
    with_customer(&pool).await;
    let mut conn = pool.acquire().await.expect("conn");
    let job = askhava_jobs::api::create(&mut conn, az(3, 9, 0), false, Actor::System)
        .await
        .expect("job");
    let declined = || intent("requires_payment_method");
    let stripe = FakeStripe::new(
        vec![declined(), intent("requires_action")],
        vec![declined()],
    );
    let mut p = press(0x1061, 1, 2500);
    p.job = job;
    pay(&pool, &stripe, &p, az(3, 10, 0)).await;
    let (payload, due) = tasks(&pool, askhava_payments::api::TIMER_KIND).await[0].clone();
    run_pay_timer(&pool, &stripe, &payload, due)
        .await
        .expect("ends");
    let (event, _) = tasks(&pool, askhava_payments::api::CARD_FIX_KIND).await[0].clone();
    askhava_picks::api::run_card_fix_notice(pool.clone(), claimed(event))
        .await
        .expect("notice");
    assert!(
        tasks(&pool, askhava_picks::api::BUSINESS_NOTICE_KIND)
            .await
            .is_empty()
    );
    let mut q = press(0x1062, 1, 2500);
    q.job = job;
    pay(&pool, &stripe, &q, az(3, 10, 0)).await;
    let (event, _) = tasks(&pool, askhava_payments::api::CARD_FIX_KIND).await[1].clone();
    askhava_picks::api::run_card_fix_notice(pool.clone(), claimed(event))
        .await
        .expect("notice");
    let (notice, _) = tasks(&pool, askhava_picks::api::BUSINESS_NOTICE_KIND).await[0].clone();
    assert_eq!(notice["template"], "card_approval_notice");
}

fn refused<T: std::fmt::Debug>(result: Result<T, ChargeError>) {
    assert_eq!(result.err(), Some(ChargeError::BadField));
}

/// TD-FEE-11, TD-FEE-32 (FEE-016, FEE-041, FEE-047): a press with a card ID that is not `pm_…`
/// or a $0 fee, a $0 intent, a Stripe answer that is not `pi_…` and a $0 limit are refused before
/// anything is stored; a limit can be removed again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_11_bad_fields_are_refused_and_a_limit_is_removed(pool: PgPool) {
    with_customer(&pool).await;
    let mut odd = FakeStripe::new(vec![], vec![]);
    odd.intent_id = "ch_FakeT062".to_owned();
    let p = press(0x1070, 1, 2500);
    let card = Press {
        payment_method: "card_FakeT062",
        ..p
    };
    refused(confirm_and_pay(&pool, &odd, &secret(), &card, az(3, 9, 0)).await);
    let free = Press {
        amount_cents: 0,
        ..p
    };
    refused(confirm_and_pay(&pool, &odd, &secret(), &free, az(3, 9, 0)).await);
    refused(open_intent(&pool, &odd, &secret(), &free, az(3, 9, 0)).await);
    refused(open_intent(&pool, &odd, &secret(), &p, az(3, 9, 0)).await);
    assert_eq!(count(&odd, "confirm"), 0);
    let mut conn = pool.acquire().await.expect("conn");
    refused(set_fee_limit(&mut conn, BUSINESS, Some(0)).await);
    set_fee_limit(&mut conn, BUSINESS, Some(100))
        .await
        .expect("limit");
    set_fee_limit(&mut conn, BUSINESS, None)
        .await
        .expect("removed");
    let limits: i64 = sqlx::query_scalar("SELECT count(*) FROM payments.fee_limit")
        .fetch_one(&pool)
        .await
        .expect("limits");
    assert_eq!(limits, 0);
}

/// TD-FEE-36 (FEE-041): a press over the limit on a pick whose Confirm page already made the
/// PaymentIntent ends that pick once and confirms nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_36_over_the_limit_after_the_page_made_the_intent(pool: PgPool) {
    with_customer(&pool).await;
    let stripe = FakeStripe::new(vec![], vec![]);
    let p = press(0x1071, 1, 2500);
    open_intent(&pool, &stripe, &secret(), &p, az(3, 9, 0))
        .await
        .expect("intent");
    let mut conn = pool.acquire().await.expect("conn");
    set_fee_limit(&mut conn, BUSINESS, Some(2000))
        .await
        .expect("limit");
    assert_eq!(
        pay(&pool, &stripe, &p, az(3, 9, 1)).await,
        ChargeOutcome::OverLimit
    );
    assert_eq!(count(&stripe, "confirm"), 0);
    let ended = tasks(&pool, askhava_payments::api::PICK_ENDED_KIND).await;
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].0["reason"], "fee-limit");
}

/// TD-FEE-11 (FEE-016, FEE-018, FEE-019): a second unreachable press keeps the running 4-hour
/// release (one timer); a repeated press of an undecided attempt asks Stripe and sends nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_11_a_second_unreachable_press_keeps_the_release_and_a_repeat_asks(pool: PgPool) {
    with_customer(&pool).await;
    let down = FakeStripe::new(
        vec![Err(AdapterError::NotSent), Err(AdapterError::NotSent)],
        vec![],
    );
    let pick = 0x1072;
    for (n, minute) in [(1, 0), (2, 30)] {
        let at = az(3, 9, minute);
        let outcome = pay(&pool, &down, &press(pick, n, 2500), at).await;
        assert_eq!(outcome, ChargeOutcome::StripeDown);
    }
    assert_eq!(
        attempt_codes(&pool, pick).await,
        ["unreachable", "unreachable"]
    );
    assert_eq!(
        tasks(&pool, askhava_payments::api::TIMER_KIND).await.len(),
        1
    );
    assert_eq!(
        running_timer(&pool, pick, "stripe-down").await,
        Some(az(3, 13, 0))
    );

    let mut slow = FakeStripe::new(
        vec![Err(AdapterError::OutcomeUnknown)],
        vec![intent("processing")],
    );
    slow.intent_id = "pi_FakeT062d".to_owned();
    let first = press(0x1073, 1, 2500);
    assert_eq!(
        pay(&pool, &slow, &first, az(3, 9, 0)).await,
        ChargeOutcome::Pending
    );
    assert_eq!(
        pay(&pool, &slow, &first, az(3, 9, 5)).await,
        ChargeOutcome::Pending
    );
    assert_eq!((count(&slow, "confirm"), count(&slow, "fetch")), (1, 1));
    assert_eq!(attempt_codes(&pool, 0x1073).await, ["processing"]);
}

/// TD-FEE-11, TD-FEE-20 (FEE-018, FEE-019, FEE-020, FEE-037): at the 4-hour release Stripe gives
/// an answer that decides nothing, and at a bank-approval window's end Stripe says processing:
/// neither ends the pick, both ask again 5 minutes on; at a decline's window end Stripe's
/// processing cannot undo the decline, so the pick ends.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_20_timer_ends_wait_on_an_answer_that_decides_nothing(pool: PgPool) {
    with_customer(&pool).await;
    let stripe = FakeStripe::new(
        vec![Err(AdapterError::NotSent)],
        vec![intent("something_new")],
    );
    pay(&pool, &stripe, &press(0x1074, 1, 2500), az(3, 9, 0)).await;
    let (payload, due) = tasks(&pool, askhava_payments::api::TIMER_KIND).await[0].clone();
    run_pay_timer(&pool, &stripe, &payload, due)
        .await
        .expect("timer");
    assert_eq!(
        running_timer(&pool, 0x1074, "stripe-down").await,
        Some(az(3, 13, 5))
    );

    let mut bank = FakeStripe::new(vec![intent("requires_action")], vec![intent("processing")]);
    bank.intent_id = "pi_FakeT062e".to_owned();
    pay(&pool, &bank, &press(0x1075, 1, 2500), az(3, 10, 0)).await;
    let timers = tasks(&pool, askhava_payments::api::TIMER_KIND).await;
    let (payload, due) = timers.last().expect("card-fix timer").clone();
    run_pay_timer(&pool, &bank, &payload, due)
        .await
        .expect("timer");
    assert_eq!(
        running_timer(&pool, 0x1075, "card-fix").await,
        Some(az(3, 11, 5))
    );
    assert_eq!(attempt_codes(&pool, 0x1075).await, ["processing"]);

    let mut card = FakeStripe::new(
        vec![intent("requires_payment_method")],
        vec![intent("processing")],
    );
    card.intent_id = "pi_FakeT062f".to_owned();
    pay(&pool, &card, &press(0x1076, 1, 2500), az(3, 10, 0)).await;
    let timers = tasks(&pool, askhava_payments::api::TIMER_KIND).await;
    let (payload, due) = timers.last().expect("card-fix timer").clone();
    run_pay_timer(&pool, &card, &payload, due)
        .await
        .expect("timer");
    assert_eq!(attempt_codes(&pool, 0x1076).await, ["declined"]);
    let ended = tasks(&pool, askhava_payments::api::PICK_ENDED_KIND).await;
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].0["reason"], "card-fix");
}

/// TD-FEE-20 (FEE-019, FEE-038, D-135): a timer task with no due time is refused; a check for an
/// attempt that is no longer the latest asks nothing; a timer that fires while charges are paused
/// stops with its time left; the pause watch with no open timer queues no next round.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_20_stale_tasks_a_paused_timer_and_an_idle_watch(pool: PgPool) {
    run_pause_watch(&pool, az(3, 8, 0)).await.expect("watch");
    assert!(
        tasks(&pool, askhava_payments::api::PAUSE_KIND)
            .await
            .is_empty()
    );
    with_customer(&pool).await;
    let stripe = FakeStripe::new(vec![Err(AdapterError::OutcomeUnknown)], vec![]);
    let pick = 0x1077;
    pay(&pool, &stripe, &press(pick, 1, 2500), az(3, 9, 0)).await;
    let id = Uuid::from_u128(pick).to_string();
    let bad = serde_json::json!({"pick": id, "kind": "card-fix"});
    assert!(
        run_pay_timer(&pool, &stripe, &bad, az(3, 9, 1))
            .await
            .is_err()
    );
    let stale = serde_json::json!({"pick": id, "try": 9, "round": 0});
    run_charge_check(&pool, &stripe, &stale, az(3, 9, 30))
        .await
        .expect("stale check");
    let mut conn = pool.acquire().await.expect("conn");
    let owner = Actor::Owner(Uuid::from_u128(0x0e));
    set_switch(&mut conn, Switch::All, true, owner)
        .await
        .expect("safe mode");
    let (payload, due) = tasks(&pool, askhava_payments::api::TIMER_KIND).await[0].clone();
    run_pay_timer(&pool, &stripe, &payload, due)
        .await
        .expect("stopped");
    assert_eq!(running_timer(&pool, pick, "stripe-down").await, None);
    assert_eq!(count(&stripe, "fetch"), 0);
}
