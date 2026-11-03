//! TD-FEE-38, TD-SEC-18 (Stripe cases), TD-SEC-27 (re-fetch case), TD-FEE-25 (release cases) and
//! TD-FEE-32 (descriptor case) (FEE-014, FEE-015, FEE-036, FEE-042, FEE-049, PICK-021, PICK-022,
//! PICK-033, JOB-014, SEC-030, SEC-039, SEC-049; D-365, D-488, D-491, D-492): on a fresh database,
//! a successful "Confirm and pay" press writes one append-only FEE-042 evidence record at charge
//! time; a pick is cleared for release only from a signature-checked `payment_intent.succeeded`
//! event whose mode, account, re-fetched status, amount and pick all match, or from a recorded $0
//! decision; and every statement descriptor is 22 characters or fewer and ends "JOB <n>".
//!
//! Stripe is a scripted fake of `IntentStripe`; the stripe-mock round trip and the adapter form
//! carrying the description and descriptor are T132's, the Confirm page and the webhook route
//! T224's (D-1476). The mint itself is taken only in `payments`' unit test and `server`, so this
//! binary checks the clearing that is the only way to a proof (SEC-030). All IDs are fake.

use std::collections::VecDeque;
use std::sync::Mutex;

use askhava_adapters::api::{
    AdapterError, Charge, Delivery, IdempotencyKey, IntentMetadata, PaymentIntent, PaymentsAdapter,
    WebhookVendor, verify_webhook,
};
use askhava_payments::api::{
    BusinessId, ChargeError, ChargeOutcome, DESCRIPTOR_MAX, EntryKind, IntentStripe, IntentText,
    LedgerEntry, NewEntry, PickNow, Press, ReleaseError, StripeAccount, VerifiedStripeEvent,
    confirm_and_pay, ledger_append, no_fee_release, paid_release,
};
use askhava_privacy::api::ReleaseBasis;
use hmac::{Hmac, KeyInit, Mac};
use jiff::Timestamp;
use secrecy::SecretString;
use sha2::Sha256;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb189_0001));
const PICK: Uuid = Uuid::from_u128(0x189_0042);
const JOB: Uuid = Uuid::from_u128(0x189_0142);
const INTENT: &str = "pi_FakeT189";
const WEBHOOK_SECRET: &str = "whsec_fake_t189_not_a_real_secret";
const NOW: i64 = 1_793_700_000;
/// The FEE-015 version the business accepted before the press; it accepts `ACCEPTED + 1` after.
const ACCEPTED: i32 = 7;
type Answer = Result<PaymentIntent, AdapterError>;

/// A scripted Stripe: confirms answer `confirm`, fetches take the next scripted answer.
struct FakeStripe {
    confirm: &'static str,
    fetches: Mutex<VecDeque<Answer>>,
}

fn fake(confirm: &'static str, fetches: Vec<Answer>) -> FakeStripe {
    FakeStripe {
        confirm,
        fetches: Mutex::new(fetches.into()),
    }
}

fn intent(status: &str, amount: i64, pick: Uuid, livemode: Option<bool>) -> Answer {
    Ok(PaymentIntent {
        id: INTENT.to_owned(),
        status: status.to_owned(),
        amount,
        currency: "usd".to_owned(),
        customer: "cus_FakeT189".to_owned(),
        metadata: IntentMetadata {
            pick_id: pick.to_string(),
        },
        livemode,
    })
}

fn paid() -> Answer {
    intent("succeeded", 7500, PICK, Some(false))
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no customer is made here")
    }

    async fn create_payment_intent(&self, _: &IdempotencyKey, _: Charge<'_>) -> Answer {
        panic!("no confirmed intent is created here (FEE-047)")
    }

    async fn fetch_payment_intent(&self, _: &str) -> Answer {
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
        Ok(INTENT.to_owned())
    }

    async fn confirm_intent(&self, _: &IdempotencyKey, _: &str, _: &str) -> Answer {
        intent(self.confirm, 7500, PICK, Some(false))
    }
}

fn account() -> StripeAccount {
    StripeAccount {
        live: false,
        account: "acct_testfake".to_owned(),
    }
}

fn press(n: u128) -> Press<'static> {
    Press {
        pick: PICK,
        business: BUSINESS,
        job: JOB,
        amount_cents: 7500,
        payment_method: "pm_FakeCard",
        press: Uuid::from_u128(0x189_0900 + n),
        emergency: false,
        now: PickNow::Payable,
    }
}

/// A customer who accepted FEE-015 version `ACCEPTED` before the press and the next one after,
/// then one press Stripe answers with `confirm`.
async fn pressed(pool: &PgPool, confirm: &'static str) -> Result<ChargeOutcome, ChargeError> {
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)")
        .bind(BUSINESS.0)
        .bind("cus_FakeT189")
        .execute(pool)
        .await
        .expect("customer");
    for (version, offset) in [(ACCEPTED, -60_i64), (ACCEPTED + 1, 30)] {
        sqlx::query(
            "INSERT INTO payments.card_authorization (business_id, version, text, accepted_at)
             VALUES ($1, $2, 'Fake authorization text', to_timestamp($3))",
        )
        .bind(BUSINESS.0)
        .bind(version)
        .bind(NOW + offset)
        .execute(pool)
        .await
        .expect("authorization");
    }
    let secret = SecretString::from("fake-id-secret-t189".to_owned());
    let at = Timestamp::from_second(NOW).expect("time");
    confirm_and_pay(pool, &fake(confirm, vec![]), &secret, &press(1), at).await
}

/// A Stripe event as `adapters` hands it over after the signature check (SEC-016).
fn event(kind: &str, livemode: bool, account: Option<&str>, object: &str) -> VerifiedStripeEvent {
    let mut body = serde_json::json!({"id": "evt_FakeT189", "type": kind, "livemode": livemode,
        "data": {"object": {"id": object, "object": "payment_intent"}}});
    if let Some(account) = account {
        body["account"] = account.into();
    }
    let body = body.to_string();
    let message = format!("{NOW}.{body}");
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(WEBHOOK_SECRET.as_bytes()).expect("key");
    mac.update(message.as_bytes());
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
    VerifiedStripeEvent::from_webhook(&hook).expect("a Stripe event")
}

fn succeeded() -> VerifiedStripeEvent {
    event("payment_intent.succeeded", false, None, INTENT)
}

async fn clear(
    pool: &PgPool,
    ev: &VerifiedStripeEvent,
    fetch: Answer,
) -> Result<Uuid, ReleaseError> {
    let at = Timestamp::from_second(NOW + 60).expect("time");
    let stripe = fake("succeeded", vec![fetch]);
    let cleared = paid_release(pool, &stripe, &account(), ev, PICK, at).await?;
    assert_eq!(cleared.basis(), ReleaseBasis::PaidCharge);
    Ok(cleared.pick())
}

async fn evidence(pool: &PgPool) -> Vec<(Uuid, Uuid, i32, String, String, String, i32, i64)> {
    sqlx::query_as(
        "SELECT business_id, press_id, try_no, idempotency_label, stripe_customer_id,
           stripe_intent_id, authorization_version, amount_cents FROM payments.charge_evidence",
    )
    .fetch_all(pool)
    .await
    .expect("evidence")
}

/// TD-FEE-38 (FEE-042, FEE-015, D-1478): a successful press writes one evidence record of IDs and
/// timestamps at charge time, with the authorization version accepted before the press (not one
/// accepted after); a verified success arriving later writes no second one.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_38_a_success_writes_one_evidence_record(pool: PgPool) {
    let outcome = pressed(&pool, "succeeded").await.expect("press");
    assert_eq!(outcome, ChargeOutcome::Succeeded(INTENT.to_owned()));
    let label = format!("pick-{PICK}-try1");
    let want = (
        BUSINESS.0,
        Uuid::from_u128(0x189_0901),
        1,
        label,
        "cus_FakeT189".to_owned(),
    );
    let want = (
        want.0,
        want.1,
        want.2,
        want.3,
        want.4,
        INTENT.to_owned(),
        ACCEPTED,
        7500,
    );
    assert_eq!(evidence(&pool).await, vec![want.clone()]);
    assert_eq!(clear(&pool, &succeeded(), paid()).await, Ok(PICK));
    assert_eq!(evidence(&pool).await, vec![want]);
    let (pressed_at, charged_at): (jiff_sqlx::Timestamp, jiff_sqlx::Timestamp) =
        sqlx::query_as("SELECT pressed_at, charged_at FROM payments.charge_evidence")
            .fetch_one(&pool)
            .await
            .expect("times");
    assert_eq!(pressed_at.to_jiff().as_second(), NOW);
    assert_eq!(charged_at.to_jiff().as_second(), NOW);
}

/// TD-FEE-38 (FEE-042): the app role may never change or remove an evidence record (42501).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_38_evidence_is_append_only(pool: PgPool) {
    pressed(&pool, "succeeded").await.expect("press");
    for sql in [
        "UPDATE payments.charge_evidence SET amount_cents = 1",
        "DELETE FROM payments.charge_evidence",
        "TRUNCATE payments.charge_evidence",
    ] {
        let mut tx = pool.begin().await.expect("transaction");
        sqlx::query("SET LOCAL ROLE askhava_app")
            .execute(&mut *tx)
            .await
            .expect("role");
        let err = sqlx::query(sql).execute(&mut *tx).await.expect_err(sql);
        let code = err
            .as_database_error()
            .and_then(|e| e.code().map(|c| c.into_owned()));
        assert_eq!(code.as_deref(), Some("42501"), "{sql}");
    }
}

/// TD-FEE-25, TD-SEC-27 (PICK-022, SEC-049): a press Stripe has not settled is cleared only once
/// the verified event's re-fetch says succeeded, which also stores the success and its evidence.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_27_only_a_refetched_success_clears(pool: PgPool) {
    assert_eq!(
        pressed(&pool, "processing").await,
        Ok(ChargeOutcome::Pending)
    );
    let processing = intent("processing", 7500, PICK, Some(false));
    assert_eq!(
        clear(&pool, &succeeded(), processing).await,
        Err(ReleaseError::NotPaid)
    );
    let down = Err(AdapterError::NotSent);
    assert_eq!(
        clear(&pool, &succeeded(), down).await,
        Err(ReleaseError::Stripe)
    );
    assert!(evidence(&pool).await.is_empty());
    assert_eq!(clear(&pool, &succeeded(), paid()).await, Ok(PICK));
    assert_eq!(evidence(&pool).await.len(), 1);
}

/// TD-SEC-18 (SEC-039): a test-mode event in live, an event from another account, an amount one
/// cent off the server fee, metadata naming another pick, another event type, another intent and
/// an intent that does not say its mode each clear nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_18_a_mismatched_event_clears_nothing(pool: PgPool) {
    pressed(&pool, "processing").await.expect("press");
    let at = Timestamp::from_second(NOW + 60).expect("time");
    let live = StripeAccount {
        live: true,
        ..account()
    };
    let stripe = fake("succeeded", vec![paid()]);
    let test_mode = paid_release(&pool, &stripe, &live, &succeeded(), PICK, at).await;
    assert_eq!(test_mode.map(|c| c.pick()), Err(ReleaseError::WrongMode));
    let ok = "payment_intent.succeeded";
    let cases = [
        (
            event(ok, false, Some("acct_otherfake"), INTENT),
            paid(),
            ReleaseError::WrongAccount,
        ),
        (
            succeeded(),
            intent("succeeded", 7499, PICK, Some(false)),
            ReleaseError::WrongAmount,
        ),
        (
            succeeded(),
            intent("succeeded", 7500, JOB, Some(false)),
            ReleaseError::WrongPick,
        ),
        (
            succeeded(),
            intent("succeeded", 7500, PICK, None),
            ReleaseError::WrongMode,
        ),
        (
            event("payment_intent.processing", false, None, INTENT),
            paid(),
            ReleaseError::NotSuccess,
        ),
        (
            event(ok, false, None, "pi_OtherFake"),
            paid(),
            ReleaseError::WrongIntent,
        ),
    ];
    for (ev, fetch, want) in cases {
        assert_eq!(clear(&pool, &ev, fetch).await, Err(want));
    }
    let own = event(ok, false, Some("acct_testfake"), INTENT);
    assert_eq!(clear(&pool, &own, paid()).await, Ok(PICK));
}

/// TD-FEE-25, TD-SEC-18 (FEE-018, FEE-020, FEE-041, JOB-014, D-1478): once the pick has ended, a
/// verified success event for its own intent clears nothing and stores nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_25_an_ended_pick_is_never_cleared(pool: PgPool) {
    pressed(&pool, "processing").await.expect("press");
    sqlx::query("UPDATE payments.pick_intent SET ended_at = to_timestamp($2) WHERE pick_id = $1")
        .bind(PICK)
        .bind(NOW + 30)
        .execute(&pool)
        .await
        .expect("end");
    assert_eq!(
        clear(&pool, &succeeded(), paid()).await,
        Err(ReleaseError::PickEnded)
    );
    assert!(evidence(&pool).await.is_empty());
    let status: String = sqlx::query_scalar("SELECT status FROM payments.charge_attempt")
        .fetch_one(&pool)
        .await
        .expect("attempt");
    assert_eq!(status, "processing");
}

/// TD-FEE-25 (FEE-036, PICK-022, D-1478): a pick with a PaymentIntent has a fee to pay, so a $0
/// decision recorded for another job of its business never clears it; a pick with no intent
/// passed the same parties is cleared.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_25_a_paid_pick_is_never_cleared_as_no_fee(pool: PgPool) {
    pressed(&pool, "processing").await.expect("press");
    let other = Uuid::from_u128(0x189_0243);
    let mut conn = pool.acquire().await.expect("connection");
    let raw = NewEntry {
        kind: EntryKind::FeeFreeJobType,
        business: Some(BUSINESS.0),
        job: Some(other),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: "t189-free-other",
    };
    ledger_append(&mut conn, &LedgerEntry::new(raw).expect("entry"))
        .await
        .expect("append");
    let paid_pick = no_fee_release(&mut conn, PICK, (BUSINESS.0, other))
        .await
        .map(|c| c.pick());
    assert_eq!(paid_pick, Err(ReleaseError::NoFeeDecision));
    let free_pick = Uuid::from_u128(0x189_0043);
    let cleared = no_fee_release(&mut conn, free_pick, (BUSINESS.0, other))
        .await
        .map(|c| c.pick());
    assert_eq!(cleared, Ok(free_pick));
}

/// TD-SEC-11 (SEC-030): only a signature-checked Stripe webhook becomes a `VerifiedStripeEvent`.
#[test]
fn td_sec_11_only_a_stripe_webhook_is_a_stripe_event() {
    let body = br#"{"id":"evt_FakeT189","livemode":false}"#;
    let message = [format!("{NOW}.").as_bytes(), body].concat();
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(WEBHOOK_SECRET.as_bytes()).expect("key");
    mac.update(&message);
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
        body,
        headers: &headers,
    };
    let secret = SecretString::from(WEBHOOK_SECRET.to_owned());
    let hook = verify_webhook(WebhookVendor::Stripe, &secret, delivery, NOW).expect("signed");
    assert_eq!(
        VerifiedStripeEvent::from_webhook(&hook),
        Err(ReleaseError::NotStripeEvent)
    );
    let lob_sig = &sig;
    let stamp = NOW.to_string();
    let lob = [
        ("Lob-Signature", lob_sig.as_str()),
        ("Lob-Signature-Timestamp", stamp.as_str()),
    ];
    let delivery = Delivery {
        url: "https://hooks.example.test/webhooks/lob",
        body,
        headers: &lob,
    };
    let hook = verify_webhook(WebhookVendor::Lob, &secret, delivery, NOW).expect("signed");
    assert_eq!(
        VerifiedStripeEvent::from_webhook(&hook),
        Err(ReleaseError::NotStripeEvent)
    );
    assert_eq!(succeeded().kind(), "payment_intent.succeeded");
}

/// TD-FEE-25, TD-SEC-11 (FEE-036, PICK-022): a pick is cleared with no fee only on a recorded $0
/// decision for its business and job; a charge entry or none clears nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_25_a_recorded_no_fee_decision_clears(pool: PgPool) {
    let mut conn = pool.acquire().await.expect("connection");
    let parties = (BUSINESS.0, JOB);
    let none = no_fee_release(&mut conn, PICK, parties)
        .await
        .map(|c| c.pick());
    assert_eq!(none, Err(ReleaseError::NoFeeDecision));
    for (kind, cents, stripe, key, job) in [
        (
            EntryKind::Charge,
            7500,
            Some("ch_FakeT189"),
            "t189-charge",
            JOB,
        ),
        (
            EntryKind::FeeFreeJobType,
            0,
            None,
            "t189-free",
            Uuid::from_u128(0x189_0242),
        ),
    ] {
        let raw = NewEntry {
            kind,
            business: Some(BUSINESS.0),
            job: Some(job),
            amount_cents: cents,
            stripe_id: stripe,
            idempotency_key: key,
        };
        ledger_append(&mut conn, &LedgerEntry::new(raw).expect("entry"))
            .await
            .expect("append");
    }
    let charged = no_fee_release(&mut conn, PICK, parties)
        .await
        .map(|c| c.pick());
    assert_eq!(charged, Err(ReleaseError::NoFeeDecision));
    let other = Uuid::from_u128(0x189_0242);
    let cleared = no_fee_release(&mut conn, PICK, (BUSINESS.0, other))
        .await
        .expect("cleared");
    assert_eq!(
        (cleared.pick(), cleared.basis()),
        (PICK, ReleaseBasis::NoFeeDue)
    );
}

/// TD-FEE-38, TD-FEE-32 (FEE-049, D-491): the PaymentIntent's description names the job, and
/// every descriptor is 22 characters or fewer and ends "JOB <n>"; a longer one is refused.
#[test]
fn td_fee_38_td_fee_32_descriptors_fit() {
    for number in [1, 42, 4_096, 999_999_999] {
        let text = IntentText::for_job(number).expect("fits");
        let descriptor = text.statement_descriptor();
        assert!(descriptor.len() <= DESCRIPTOR_MAX, "{descriptor}");
        assert!(
            descriptor.ends_with(&format!("JOB {number}")),
            "{descriptor}"
        );
        assert_eq!(text.statement_suffix(), format!("JOB {number}"));
        let description = format!("Ask Hava introduction service, job {number}");
        assert_eq!(text.description(), description);
    }
    assert_eq!(
        IntentText::for_job(1_000_000_000),
        Err(ChargeError::BadField)
    );
    assert_eq!(IntentText::for_job(0), Err(ChargeError::BadField));
}
