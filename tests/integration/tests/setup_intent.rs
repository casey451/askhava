//! TD-FEE-10 and TD-FEE-32's saved-card case (FEE-014, FEE-015, FEE-036, FEE-048; D-073, D-089,
//! D-1409, D-1410): on a fresh database, a business's Stripe customer is made once with no card,
//! a saved card is stored with the FEE-015 authorization text, version and time, cards only, for
//! off-session use, and the authorization rows are append-only for the app role.
//!
//! Stripe is a recording fake of the `PaymentsAdapter` trait, so the test sees every call
//! `payments` makes; the stripe-mock round trip of the SetupIntent is T132's (D-1410). The
//! Confirm-page cases are TD-FEE-40's. All IDs are fake.

use std::sync::Mutex;

use askhava_adapters::api::{AdapterError, Charge, IdempotencyKey, PaymentIntent, PaymentsAdapter};
use askhava_payments::api::{
    AUTHORIZATION_TEXT, AUTHORIZATION_VERSION, BusinessId, CUSTOMER_ACTION, CardError, CustomerId,
    Saved, SavedCard, SetupIntentRequest, customer_of, ensure_customer, save_card,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

const BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb059_0001));
const OTHER: BusinessId = BusinessId(Uuid::from_u128(0xb059_0002));

/// A fake Stripe that records each customer create (key, business) and refuses everything else.
struct FakeStripe {
    calls: Mutex<Vec<(String, String)>>,
    fail: bool,
}

impl FakeStripe {
    fn new(fail: bool) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            fail,
        }
    }

    fn calls(&self) -> Vec<(String, String)> {
        self.calls.lock().expect("lock").clone()
    }
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(
        &self,
        idempotency: &IdempotencyKey,
        business_id: &str,
    ) -> Result<String, AdapterError> {
        let mut calls = self.calls.lock().expect("lock");
        calls.push((idempotency.as_str().to_owned(), business_id.to_owned()));
        if self.fail {
            return Err(AdapterError::OutcomeUnknown);
        }
        Ok(format!("cus_Fake{}", calls.len()))
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        panic!("saving a card must never create a PaymentIntent (FEE-036)")
    }

    async fn fetch_payment_intent(&self, _: &str) -> Result<PaymentIntent, AdapterError> {
        panic!("no PaymentIntent is fetched here")
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no refund is made here")
    }
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t059".to_owned())
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_790_000_000 + minute * 60).expect("time")
}

fn card(pm: &str) -> SavedCard {
    SavedCard::new(pm, "card", "visa", "4242").expect("card")
}

const CUSTOMERS: &str = "SELECT count(*) FROM payments.customer";
const CARDS: &str = "SELECT count(*) FROM payments.payment_method";
const AUTHORIZATIONS: &str = "SELECT count(*) FROM payments.card_authorization";

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// TD-FEE-10 (FEE-036, D-089, SEC-018): sign-up makes one Stripe customer with no card, keyed by
/// the business ID only; a repeat returns it without a second Stripe call.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_10_customer_at_sign_up_has_no_card(pool: PgPool) {
    let stripe = FakeStripe::new(false);
    let first = ensure_customer(&pool, &stripe, &secret(), BUSINESS)
        .await
        .expect("customer");
    let again = ensure_customer(&pool, &stripe, &secret(), BUSINESS)
        .await
        .expect("again");
    assert_eq!(first, again);
    let id = BUSINESS.0.to_string();
    let key = IdempotencyKey::derive(&secret(), CUSTOMER_ACTION, &[&id]);
    assert_eq!(stripe.calls(), [(key.as_str().to_owned(), id)]);
    assert_eq!(count(&pool, CUSTOMERS).await, 1);
    assert_eq!(count(&pool, CARDS).await, 0);
    assert_eq!(count(&pool, AUTHORIZATIONS).await, 0);
}

/// TD-FEE-10 (FEE-036): a failed Stripe call stores nothing; a card cannot be saved for a business
/// with no customer.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_10_stripe_failure_stores_nothing(pool: PgPool) {
    let failed = ensure_customer(&pool, &FakeStripe::new(true), &secret(), BUSINESS).await;
    assert_eq!(failed, Err(CardError::Stripe));
    assert_eq!(count(&pool, CUSTOMERS).await, 0);
    let mut db = pool.acquire().await.expect("connection");
    let saved = save_card(&mut db, BUSINESS, &card("pm_Fake01"), at(1)).await;
    assert_eq!(saved, Err(CardError::NoCustomer));
    assert_eq!(customer_of(&mut db, BUSINESS).await, Ok(None));
}

/// TD-FEE-10 (FEE-014, FEE-015, FEE-048, D-195, D-196): the saved card keeps the authorization's
/// text, version and time, cards only, for off-session use; a repeat stores nothing more and
/// another business cannot claim the card.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_10_saved_card_keeps_the_authorization(pool: PgPool) {
    let stripe = FakeStripe::new(false);
    for business in [BUSINESS, OTHER] {
        ensure_customer(&pool, &stripe, &secret(), business)
            .await
            .expect("customer");
    }
    let mut db = pool.acquire().await.expect("connection");
    let pm = card("pm_Fake02");
    assert_eq!(
        save_card(&mut db, BUSINESS, &pm, at(5)).await,
        Ok(Saved::New)
    );
    assert_eq!(
        save_card(&mut db, BUSINESS, &pm, at(6)).await,
        Ok(Saved::Already)
    );
    let other = save_card(&mut db, OTHER, &pm, at(7)).await;
    assert_eq!(other, Err(CardError::OtherBusiness));
    let (text, version, accepted): (String, i32, i64) = sqlx::query_as(
        "SELECT text, version, extract(epoch FROM accepted_at)::bigint \
         FROM payments.card_authorization WHERE business_id = $1",
    )
    .bind(BUSINESS.0)
    .fetch_one(&pool)
    .await
    .expect("authorization");
    assert_eq!(
        (text.as_str(), version),
        (AUTHORIZATION_TEXT, AUTHORIZATION_VERSION)
    );
    assert_eq!(accepted, at(5).as_second());
    assert!(text.contains("ongoing services counted at 4 months and capped at $75"));
    assert!(text.contains("within 90 days after my trade opens"));
    let row: (String, String, String, String) =
        sqlx::query_as("SELECT method_type, usage, brand, last4 FROM payments.payment_method")
            .fetch_one(&pool)
            .await
            .expect("card row");
    assert_eq!(
        row,
        (
            "card".into(),
            "off_session".into(),
            "visa".into(),
            "4242".into()
        )
    );
    assert_eq!(count(&pool, AUTHORIZATIONS).await, 1);
}

/// TD-FEE-10 (FEE-014, D-073): a bank debit is refused by the checked type and by the table.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_10_bank_debit_is_refused(pool: PgPool) {
    let debit = SavedCard::new("pm_Fake03", "us_bank_account", "visa", "4242");
    assert_eq!(debit, Err(CardError::CardsOnly));
    ensure_customer(&pool, &FakeStripe::new(false), &secret(), BUSINESS)
        .await
        .expect("customer");
    let auth: Uuid = sqlx::query_scalar(
        "INSERT INTO payments.card_authorization (business_id, version, text, accepted_at) \
         VALUES ($1, 1, 'fake', now()) RETURNING id",
    )
    .bind(BUSINESS.0)
    .fetch_one(&pool)
    .await
    .expect("authorization");
    let err = sqlx::query(
        "INSERT INTO payments.payment_method (stripe_payment_method_id, business_id, \
         authorization_id, method_type, usage, brand, last4, saved_at) \
         VALUES ('pm_Fake03', $1, $2, 'us_bank_account', 'off_session', 'visa', '4242', now())",
    )
    .bind(BUSINESS.0)
    .bind(auth)
    .execute(&pool)
    .await
    .expect_err("bank debit row");
    let code = err
        .as_database_error()
        .and_then(|e| e.code().map(|c| c.into_owned()));
    assert_eq!(code.as_deref(), Some("23514"));
}

/// TD-FEE-32 (FEE-048, D-490): the SetupIntent of a $0 Confirm page saves for off-session use and
/// takes cards only.
#[test]
fn td_fee_32_saved_card_is_for_off_session_use() {
    let customer = CustomerId::parse("cus_Fake1").expect("customer");
    let request = SetupIntentRequest::for_customer(customer);
    let form = request.form();
    assert!(form.contains(&("usage", "off_session")));
    assert!(form.contains(&("payment_method_types[]", "card")));
}

/// TD-FEE-10 (FEE-015, D-092, BIZ-051): the app role can add an authorization but never change or
/// remove one, and may change a saved card's `removed_at` only (SQLSTATE 42501).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_10_authorizations_are_append_only(pool: PgPool) {
    for sql in [
        "UPDATE payments.card_authorization SET version = 2",
        "DELETE FROM payments.card_authorization",
        "TRUNCATE payments.card_authorization",
        "DELETE FROM payments.customer",
        "UPDATE payments.payment_method SET brand = 'amex'",
        "UPDATE payments.payment_method SET usage = 'on_session'",
        "DELETE FROM payments.payment_method",
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

/// TD-FEE-10 (FEE-014, BIZ-051): the app role may mark a saved card removed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_10_app_role_may_remove_a_card(pool: PgPool) {
    let mut tx = pool.begin().await.expect("transaction");
    sqlx::query("SET LOCAL ROLE askhava_app")
        .execute(&mut *tx)
        .await
        .expect("role");
    sqlx::query("UPDATE payments.payment_method SET removed_at = now()")
        .execute(&mut *tx)
        .await
        .expect("removed_at is the app role's to set");
}

/// TD-FEE-10 (FEE-014): the longest ID the checked type takes, `pm_` and 200 characters, fits the
/// table's check, so the type and the table agree.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_10_longest_card_id_fits_the_table(pool: PgPool) {
    ensure_customer(&pool, &FakeStripe::new(false), &secret(), BUSINESS)
        .await
        .expect("customer");
    let longest = format!("pm_{}", "F".repeat(200));
    assert!(SavedCard::new(&format!("{longest}F"), "card", "visa", "4242").is_err());
    let mut db = pool.acquire().await.expect("connection");
    let saved = save_card(&mut db, BUSINESS, &card(&longest), at(9)).await;
    assert_eq!(saved, Ok(Saved::New));
}
