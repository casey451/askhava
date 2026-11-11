//! TD-FEE-56 (FEE-021, FEE-018, FEE-020, FEE-026; D-1547, D-2290): on a fresh database, a payment
//! that succeeds on a pick `payments` itself ended is refunded exactly once with one alert. A
//! late-success refund row stores the trigger `late-success`, which the table's check allows and
//! `RefundTrigger` reads back, and never `charged-after-cancel`. A pick ended by the FEE-018
//! four-hour end, and one ended by the FEE-020 card-fix window, each really ended by
//! `run_pay_timer`, then succeed: the success and its FEE-042 evidence are stored, nothing is
//! released (`paid_release` answers `PickEnded`, so no `ReleaseProof` can be built), and exactly
//! one `payments.late_success_refund` task and one `payments.late_success` alert exist, whether the
//! success arrives by the webhook, by the charge check round, or by both in either order. A
//! success on a pick still current is stored, cleared for release and queues no refund and no
//! alert, by either path (FEE-021 applies only once the job moved on).
//!
//! Stripe is a scripted fake: each fetch takes the next answer, `"down"` meaning unreachable, and
//! refunds are counted. The webhook is signed with a fake secret. All IDs are fake.

use std::collections::VecDeque;
use std::sync::Mutex;

use askhava_adapters::api::{
    AdapterError, Charge, Delivery, IdempotencyKey, IntentMetadata, PaymentIntent, PaymentsAdapter,
    WebhookVendor, verify_webhook,
};
use askhava_payments::api::{
    LATE_KIND, LATE_SUCCESS_ALERT, LATE_TRIGGER, RefundTrigger, ReleaseError, StripeAccount,
    VerifiedStripeEvent, paid_release, run_charge_check, run_late_refund, run_pay_timer,
};
use askhava_privacy::api::ReleaseBasis;
use hmac::{Hmac, KeyInit, Mac};
use jiff::Timestamp;
use secrecy::SecretString;
use serde_json::json;
use sha2::Sha256;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: Uuid = Uuid::from_u128(0xb332_0001);
const PICK: Uuid = Uuid::from_u128(0x332_0042);
const JOB: Uuid = Uuid::from_u128(0x332_0142);
const INTENT: &str = "pi_FakeT332";
const WEBHOOK_SECRET: &str = "whsec_fake_t332_not_a_real_secret";
const NOW: i64 = 1_794_100_000;

/// A scripted Stripe: fetches take the next answer (`"down"`: unreachable); refunds are counted.
struct FakeStripe {
    fetches: Mutex<VecDeque<&'static str>>,
    refunds: Mutex<u32>,
}

fn fake(fetches: &[&'static str]) -> FakeStripe {
    FakeStripe {
        fetches: Mutex::new(fetches.iter().copied().collect()),
        refunds: Mutex::new(0),
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
        let status = self
            .fetches
            .lock()
            .expect("lock")
            .pop_front()
            .expect("scripted");
        if status == "down" {
            return Err(AdapterError::NotSent);
        }
        Ok(PaymentIntent {
            id: INTENT.to_owned(),
            status: status.to_owned(),
            amount: 7500,
            currency: "usd".to_owned(),
            customer: "cus_FakeT332".to_owned(),
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
        Ok("re_FakeT332".to_owned())
    }
}

/// How `payments` ended the pick.
#[derive(Clone, Copy)]
enum End {
    /// FEE-018: the 4-hour release, Stripe still unreachable.
    FourHours,
    /// FEE-020: the card-fix window, Stripe still saying the card needs fixing.
    CardFix,
}

/// How the success reaches `payments`.
#[derive(Clone, Copy)]
enum Path {
    Webhook,
    Check,
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

/// A signature-checked `payment_intent.succeeded` for the pick's own intent.
fn succeeded() -> VerifiedStripeEvent {
    let body = json!({"id": "evt_FakeT332", "type": "payment_intent.succeeded", "livemode": false,
        "data": {"object": {"id": INTENT, "object": "payment_intent"}}})
    .to_string();
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(WEBHOOK_SECRET.as_bytes()).expect("key");
    mac.update(format!("{NOW}.{body}").as_bytes());
    let sig: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let header = format!("t={NOW},v1={sig}");
    let headers = [("Stripe-Signature", header.as_str())];
    let delivery = Delivery {
        url: "https://hooks.example.test/webhooks/stripe",
        body: body.as_bytes(),
        headers: &headers,
    };
    let secret = SecretString::from(WEBHOOK_SECRET.to_owned());
    let hook = verify_webhook(WebhookVendor::Stripe, &secret, delivery, NOW).expect("signed");
    VerifiedStripeEvent::from_webhook(&hook).expect("event")
}

/// One $75 pick whose one attempt has `status`, with its `kind` timer due a minute ago, as T062
/// writes them; the pick is still current.
async fn seed(pool: &PgPool, status: &str, kind: &str) {
    let sqls = [
        "INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($2, 'cus_FakeT332')",
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false,
           to_timestamp($5 - 18000))",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at)
           VALUES ($1, 1, $2, $6, to_timestamp($5 - 18000))",
        "INSERT INTO payments.pay_timer (pick_id, kind, business_clock, due_at)
           VALUES ($1, $7, false, to_timestamp($5 - 60))",
    ];
    for sql in sqls {
        sqlx::query(sql)
            .bind(PICK)
            .bind(BUSINESS)
            .bind(JOB)
            .bind(INTENT)
            .bind(NOW)
            .bind(status)
            .bind(kind)
            .execute(pool)
            .await
            .expect("seed");
    }
}

/// One $75 pick whose one attempt is `sent` (no answer, the 4-hour release running) or
/// `declined` (the card-fix window running), with that timer due a minute ago, as T062 writes
/// them; then the timer's own handler ends the pick.
async fn ended(pool: &PgPool, end: End) {
    let (status, kind, answer) = match end {
        End::FourHours => ("sent", "stripe-down", "down"),
        End::CardFix => ("declined", "card-fix", "requires_payment_method"),
    };
    seed(pool, status, kind).await;
    let timer = json!({"pick": PICK, "kind": kind, "due": NOW - 60});
    run_pay_timer(pool, &fake(&[answer]), &timer, at(0))
        .await
        .expect("timer");
    let ended: bool = sqlx::query_scalar(
        "SELECT ended_at IS NOT NULL FROM payments.pick_intent WHERE pick_id = $1",
    )
    .bind(PICK)
    .fetch_one(pool)
    .await
    .expect("ended");
    assert!(ended, "the timer ended the pick");
}

/// Stripe's success arrives by `path`; the webhook never clears an ended pick.
async fn deliver(pool: &PgPool, path: Path, offset: i64) {
    let stripe = fake(&["succeeded"]);
    match path {
        Path::Webhook => {
            let (account, event) = (account(), succeeded());
            let cleared = paid_release(pool, &stripe, &account, &event, PICK, at(offset)).await;
            assert_eq!(cleared.err(), Some(ReleaseError::PickEnded));
        }
        Path::Check => {
            let check = json!({"pick": PICK, "try": 1, "round": 0});
            run_charge_check(pool, &stripe, &check, at(offset))
                .await
                .expect("check");
        }
    }
}

/// The evidence records, the attempt's status, the late-success tasks and alerts.
async fn state(pool: &PgPool) -> (i64, String, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM payments.charge_evidence),
                (SELECT status FROM payments.charge_attempt WHERE try_no = 1),
                (SELECT count(*) FROM core.task WHERE kind = $1),
                (SELECT count(*) FROM ops.alert WHERE kind = $2)",
    )
    .bind(LATE_KIND)
    .bind(LATE_SUCCESS_ALERT)
    .fetch_one(pool)
    .await
    .expect("state")
}

async fn assert_one_late_success(pool: &PgPool) {
    let want = (1, "succeeded".to_owned(), 1, 1);
    assert_eq!(state(pool).await, want);
}

async fn run(pool: &PgPool, end: End, paths: &[Path]) {
    ended(pool, end).await;
    for (n, path) in (0_i64..).zip(paths) {
        deliver(pool, *path, 60 * (n + 1)).await;
        assert_one_late_success(pool).await;
    }
}

/// TD-FEE-56 (FEE-018, FEE-021): the four-hour end, then the webhook (twice, as Stripe resends).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_56_four_hour_end_then_webhook(pool: PgPool) {
    run(&pool, End::FourHours, &[Path::Webhook, Path::Webhook]).await;
}

/// TD-FEE-56 (FEE-018, FEE-019, FEE-021): the four-hour end, then the charge check round.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_56_four_hour_end_then_check(pool: PgPool) {
    run(&pool, End::FourHours, &[Path::Check]).await;
}

/// TD-FEE-56 (FEE-018, FEE-021, FEE-026): the four-hour end, then the check, then the webhook.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_56_four_hour_end_check_then_webhook(pool: PgPool) {
    run(&pool, End::FourHours, &[Path::Check, Path::Webhook]).await;
}

/// TD-FEE-56 (FEE-018, FEE-021, FEE-026): the four-hour end, then the webhook, then the check.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_56_four_hour_end_webhook_then_check(pool: PgPool) {
    run(&pool, End::FourHours, &[Path::Webhook, Path::Check]).await;
}

/// TD-FEE-56 (FEE-020, FEE-021): the card-fix window ends a declined pick, whose later success
/// comes by the webhook (twice); a check round finds nothing undecided and adds nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_56_card_fix_end_then_webhook(pool: PgPool) {
    run(&pool, End::CardFix, &[Path::Webhook, Path::Webhook]).await;
    let check = json!({"pick": PICK, "try": 1, "round": 0});
    run_charge_check(&pool, &fake(&[]), &check, at(600))
        .await
        .expect("check");
    assert_one_late_success(&pool).await;
}

/// TD-FEE-56 (FEE-020, FEE-021): a check round before the success finds the declined attempt
/// decided and calls nothing; the webhook then stores the success and queues the one refund.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_56_card_fix_end_check_then_webhook(pool: PgPool) {
    ended(&pool, End::CardFix).await;
    let check = json!({"pick": PICK, "try": 1, "round": 0});
    run_charge_check(&pool, &fake(&[]), &check, at(30))
        .await
        .expect("check");
    let none = (0, "declined".to_owned(), 0, 0);
    assert_eq!(state(&pool).await, none);
    deliver(&pool, Path::Webhook, 60).await;
    assert_one_late_success(&pool).await;
}

/// A success on the still-current pick reaches `payments` by `path`; the webhook clears it.
async fn deliver_current(pool: &PgPool, path: Path, offset: i64) {
    let stripe = fake(&["succeeded"]);
    match path {
        Path::Webhook => {
            let (account, event) = (account(), succeeded());
            let cleared = paid_release(pool, &stripe, &account, &event, PICK, at(offset)).await;
            let cleared = cleared.expect("a current pick is cleared");
            assert_eq!(
                (cleared.pick(), cleared.basis()),
                (PICK, ReleaseBasis::PaidCharge)
            );
        }
        Path::Check => {
            let check = json!({"pick": PICK, "try": 1, "round": 0});
            run_charge_check(pool, &stripe, &check, at(offset))
                .await
                .expect("check");
        }
    }
}

/// A success on a pick `payments` never ended is stored with its one evidence record and queues
/// no late-success refund and no alert, by `paths` in order.
async fn run_current(pool: &PgPool, paths: &[Path]) {
    seed(pool, "sent", "stripe-down").await;
    for (n, path) in (0_i64..).zip(paths) {
        deliver_current(pool, *path, 60 * (n + 1)).await;
        assert_eq!(state(pool).await, (1, "succeeded".to_owned(), 0, 0));
    }
}

/// TD-FEE-56 (FEE-021, FEE-042): a success on a current pick by the webhook is cleared for
/// release and refunds nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_56_current_pick_webhook_refunds_nothing(pool: PgPool) {
    run_current(&pool, &[Path::Webhook]).await;
}

/// TD-FEE-56 (FEE-019, FEE-021, FEE-042): a success on a current pick recorded by the check round
/// refunds nothing, and the webhook after it still clears the pick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_56_current_pick_check_refunds_nothing(pool: PgPool) {
    run_current(&pool, &[Path::Check, Path::Webhook]).await;
}

/// TD-FEE-56 (FEE-021, FEE-026; D-2290): the queued task refunds once under `late-success`
/// (`LATE_TRIGGER`), never `charged-after-cancel`, however often it runs.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_56_late_success_refund_has_its_own_trigger(pool: PgPool) {
    run(&pool, End::FourHours, &[Path::Webhook]).await;
    assert_eq!(LATE_TRIGGER, RefundTrigger::LateSuccess);
    let stripe = fake(&[]);
    let secret = SecretString::from("fake-id-secret-t332".to_owned());
    for offset in [600, 4200] {
        run_late_refund(&pool, &stripe, &secret, &json!({"pick": PICK}), at(offset))
            .await
            .expect("refund task");
    }
    assert_eq!(*stripe.refunds.lock().expect("lock"), 1);
    let triggers: Vec<String> = sqlx::query_scalar("SELECT trigger FROM payments.refund")
        .fetch_all(&pool)
        .await
        .expect("refunds");
    assert_eq!(triggers, ["late-success"]);
    let read = triggers.iter().map(|t| RefundTrigger::from_code(t));
    assert_eq!(read.collect::<Vec<_>>(), [Some(RefundTrigger::LateSuccess)]);
}

/// TD-FEE-56 (FEE-026; D-2290): every trigger's code reads back to it and the table's check
/// allows it; a code no trigger has is neither read nor stored.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_56_every_trigger_code_is_allowed_and_read_back(pool: PgPool) {
    run(&pool, End::FourHours, &[Path::Check]).await;
    assert_eq!(RefundTrigger::from_code("late-success-ish"), None);
    let insert = "INSERT INTO payments.refund (pick_id, trigger, status, tries, requested_at)
                  VALUES ($1, $2, 'sending', 0, now())";
    for trigger in RefundTrigger::ALL {
        assert_eq!(RefundTrigger::from_code(trigger.code()), Some(trigger));
        let mut tx = pool.begin().await.expect("begin");
        sqlx::query(insert)
            .bind(PICK)
            .bind(trigger.code())
            .execute(&mut *tx)
            .await
            .expect("the check allows every trigger");
        tx.rollback().await.expect("rollback");
    }
    let refused = sqlx::query(insert)
        .bind(PICK)
        .bind("late-success-ish")
        .execute(&pool)
        .await
        .expect_err("an unknown code is refused");
    let code = refused.as_database_error().and_then(|e| e.code());
    assert_eq!(code.as_deref(), Some("23514"));
}
