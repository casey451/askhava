//! The door probe's released pick (T401; D-3106, D-3134; PICK-022, PICK-024, FEE-010, FEE-012):
//! a waiting page pick released on the $0 path through module APIs only, as
//! `tests/integration/tests/no_fee_release_binding.rs` and `src/release_sink.rs` reach it. The
//! business's free first win is stored by `payments`, `payments::api::no_fee_release` clears the
//! pick for its own business and job, `picks::api::settle` moves it to `free`, the proof is minted
//! with `askhava_server::release_minter()` (never a second `privacy` release mint in the process),
//! and `picks::api::release` shows it with the masked pool the probe's router is built with.
//!
//! T404 (D-3112, D-3134; FEE-031, FEE-005, FEE-042, SEC-019): [`charge`] confirms and pays a
//! third page pick of each side through `payments::api::confirm_and_pay` with a test-local fake
//! `IntentStripe` answering `succeeded`, after the business's customer and saved card, so
//! `payments.charge_evidence` holds that pick's record (the "Why was I charged?" page's source)
//! and no vendor is reached.
//!
//! Every `payments` call of `door_matrix.rs` lives here, and this file names no homeowner mark,
//! so TD-FEE-07 (`tests/security/tests/no_homeowner_charge.rs`) holds for both; the contact the
//! release binds is passed in. Every ID is fake.

use askhava_adapters::api::{
    AdapterError, Charge, IdempotencyKey, MaskedPool, PaymentIntent, PaymentsAdapter, TwilioConfig,
};
use askhava_core::api::Settings;
use askhava_payments::api::{
    BusinessId, ChargeOutcome, EntryKind, IntentStripe, LedgerEntry, NewEntry, PickNow, Press,
    SavedCard, confirm_and_pay, ensure_customer, ledger_append, no_fee_release, pick_fee,
    receipt_for, save_card,
};
use askhava_picks::api::{Released, Settled, release, settle};
use askhava_privacy::api::Vault;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The masked pool of the router's own fake settings (`super::pairs`).
fn masked() -> MaskedPool {
    let pairs = super::pairs();
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let twilio = TwilioConfig::from_settings(&settings, pairs).expect("twilio");
    twilio.pool().clone()
}

/// The $0 free-first-win ledger decision for `business` on `job` (FEE-010, FEE-012).
async fn free_first_win(pool: &PgPool, (business, job): (Uuid, Uuid)) {
    let mut db = pool.acquire().await.expect("db");
    let key = format!("t401.free.{job}.{business}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::FreeFirstWin,
        business: Some(business),
        job: Some(job),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: &key,
    })
    .expect("entry");
    ledger_append(&mut db, &entry).await.expect("ledger");
}

/// Releases `pick`, the waiting page pick of `business` on `job`, to `contact` on the $0 path
/// (PICK-022, PICK-024, FEE-012, D-1739, D-3106): free first win, `no_fee_release`, settle to
/// `free`, the minted proof and the release, which must show it.
pub(super) async fn release_free(
    pool: &PgPool,
    vault: &Vault,
    (business, job, pick): (Uuid, Uuid, Uuid),
    contact: Uuid,
    now: Timestamp,
) {
    free_first_win(pool, (business, job)).await;
    let mut db = pool.acquire().await.expect("db");
    let cleared = no_fee_release(&mut db, pick, (business, job)).await;
    let cleared = cleared.expect("a $0 decision clears the pick");
    drop(db);
    let mut tx = pool.begin().await.expect("tx");
    let settled = settle(&mut tx, &cleared).await.expect("settle");
    tx.commit().await.expect("commit");
    assert_eq!(settled, Settled::Moved(job), "the pick moved to free");
    let proof = askhava_server::release_minter().mint(cleared, contact);
    let shown = release(pool, (vault, &masked()), (proof, false), now).await;
    assert!(matches!(shown.expect("release"), Released::Shown(_)));
}

/// A Stripe that never leaves the process: `business`'s one fake customer and one intent that
/// confirms `succeeded` for `pick` at `cents` (D-3134: built from JSON, with a fake `ch_` charge).
/// Customer and intent IDs are derived from the business and pick, since each is stored once.
struct FakeStripe {
    business: Uuid,
    pick: Uuid,
    cents: u64,
}

impl FakeStripe {
    fn customer(&self) -> String {
        format!("cus_fake{}", self.business.simple())
    }

    fn intent(&self) -> String {
        format!("pi_fake{}", self.pick.simple())
    }

    fn succeeded(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        let body = serde_json::json!({
            "id": id,
            "status": "succeeded",
            "amount": self.cents,
            "currency": "usd",
            "customer": self.customer(),
            "metadata": {"pick_id": self.pick.to_string()},
            "livemode": false,
            "latest_charge": format!("ch_fake{}", self.pick.simple()),
        });
        Ok(serde_json::from_value(body).expect("a fake intent"))
    }
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok(self.customer())
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        self.succeeded(id)
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Err(AdapterError::NotSent)
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
        Ok(self.intent())
    }

    async fn confirm_intent(
        &self,
        _: &IdempotencyKey,
        intent: &str,
        _: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        self.succeeded(intent)
    }
}

/// `business`'s Stripe customer and saved card `card`, its FEE-015 authorization accepted a
/// minute before `now` (FEE-014, FEE-015, FEE-036).
async fn customer_and_card(
    pool: &PgPool,
    stripe: &FakeStripe,
    (business, card): (Uuid, &str),
    now: Timestamp,
) {
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let id = BusinessId(business);
    ensure_customer(pool, stripe, &secret, id)
        .await
        .expect("customer");
    let saved = SavedCard::new(card, "card", "visa", "4242").expect("card");
    let accepted = now - SignedDuration::from_mins(1);
    let mut db = pool.acquire().await.expect("db");
    save_card(&mut db, id, &saved, accepted)
        .await
        .expect("saved");
}

/// Confirms and pays `pick`, the waiting page pick of `business` on `job`, at the fee `picks`
/// fixed at the pick, through the fake Stripe, and checks `charge_evidence` holds its record for
/// that business (FEE-031, FEE-005, FEE-042, D-3112).
pub(super) async fn charge(
    pool: &PgPool,
    (business, job, pick): (Uuid, Uuid, Uuid),
    now: Timestamp,
) {
    let mut db = pool.acquire().await.expect("db");
    let fixed = pick_fee(&mut db, pick).await.expect("read").expect("fixed");
    drop(db);
    let stripe = FakeStripe {
        business,
        pick,
        cents: fixed.cents,
    };
    let card = format!("pm_fake{}", business.simple());
    customer_and_card(pool, &stripe, (business, &card), now).await;
    let press = Press {
        pick,
        business: BusinessId(business),
        job,
        amount_cents: u32::try_from(fixed.cents).expect("cents"),
        payment_method: &card,
        press: Uuid::new_v4(),
        emergency: false,
        now: PickNow::Payable,
    };
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let paid = confirm_and_pay(pool, &stripe, &secret, &press, now).await;
    assert_eq!(
        paid.expect("charge"),
        ChargeOutcome::Succeeded(stripe.intent())
    );
    let mut db = pool.acquire().await.expect("db");
    let receipt = receipt_for(&mut db, pick, business).await.expect("receipt");
    assert_eq!(
        receipt.map(|r| r.job),
        Some(job),
        "the charge's evidence row"
    );
}
