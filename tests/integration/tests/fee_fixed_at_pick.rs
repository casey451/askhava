//! TD-FEE-48 (FEE-005, FEE-007, FEE-016, FEE-047; D-072, D-1415, D-1871): on a fresh database,
//! `payments` stores the fee terms in force and the introduction fee by job size fixed at each
//! pick. A switch updates the terms with its audit row; a text pick and a page pick each store one
//! fee in the pick's transaction, and a pick whose fee can't be fixed stores no pick; a repeat
//! writes nothing and other values are refused; a press that does not match the stored fee is
//! `FeeMismatch` from `open_intent` and `confirm_and_pay` with no Stripe call; a switch after the
//! pick is `FeeModeChanged` before any charge, while one after the gate changes nothing for that
//! pick; a pick made after a switch needs the new authorization (D-072); the app role cannot change
//! a stored fee.
//!
//! Stripe is a fake `IntentStripe` that records every call. Fake numbers, IDs and wording only.

use std::sync::Mutex;

use askhava_adapters::api::{
    AdapterError, Charge, IdempotencyKey, IntentMetadata, PaymentIntent, PaymentsAdapter,
};
use askhava_core::api::Guarded;
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, PublishedTerms, TextIn, answer_text_pick};
use askhava_intake::api::{begin_job, receive_text};
use askhava_jobs::api::{JobStatus, move_to};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    AUTHORIZATION_TEXT, AUTHORIZATION_VERSION, Appended, BusinessId, ChargeError, ChargeOutcome,
    ConfirmError, Confirmed, FeeJob, FeeMode, FeeQuote, FeeTerms, IntentStripe, PeriodJob,
    PickFeeError, PickNow, PickedFee, Press, RecurringCap, Service, VisitTier,
    accept_authorization, confirm_and_pay, fee_at_pick, fee_terms_in_force, fix_fee_at_pick,
    may_confirm, open_intent, pick_fee, record_fee_at_confirm, switch_fee_mode,
};
use askhava_picks::api::{PagePick, PicksError, page_pick};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb287_0001));
const OWNER: Uuid = Uuid::from_u128(0x0a28_7001);
const TERMS: PublishedTerms<'static> = PublishedTerms {
    version: "2026-11-01",
    text: "Sample homeowner terms for tests only.",
};
/// A priced one-time job with the default facts (the $25 set tier, not fee-free).
const PRICED: PeriodJob = PeriodJob {
    job: FeeJob {
        diagnostic_visit: false,
        visit_tier: VisitTier::DEFAULT,
        fee_free: false,
    },
    service: Service::NotRecurring,
    cap: RecurringCap::DEFAULT,
};
/// A stored fee row: (pick, business, job, mode, kind, amount, terms version).
type FeeRow = (Uuid, Uuid, Uuid, String, String, i64, i32);

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-287".to_owned())
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_100_000 + minute * 60).expect("time")
}

/// The fee `fee_at_pick` gives a firm quote of `cents` in a priced job under `mode`.
fn fee(cents: u64, mode: FeeMode) -> PickedFee {
    fee_at_pick(FeeQuote::Fixed(cents), None, PRICED, mode, None).expect("fee")
}

async fn fee_rows(pool: &PgPool) -> Vec<FeeRow> {
    sqlx::query_as(
        "SELECT pick_id, business_id, job_id, mode, kind, amount_cents, terms_version \
         FROM payments.pick_fee ORDER BY fixed_at, pick_id",
    )
    .fetch_all(pool)
    .await
    .expect("pick fees")
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// A business with a Stripe customer that accepted authorization versions `1..=version`.
async fn ready(db: &mut PgConnection, version: i32) {
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)")
        .bind(BUSINESS.0)
        .bind("cus_FakeT287")
        .execute(&mut *db)
        .await
        .expect("customer");
    for v in 1..=version {
        let terms = FeeTerms::new(FeeMode::Tiers, v);
        accept_authorization(db, BUSINESS, terms, AUTHORIZATION_TEXT, at(0))
            .await
            .expect("accepted");
    }
}

/// A Stripe that records every call; each confirm succeeds.
#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<String>>,
}

impl Recorder {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("lock").clone()
    }

    fn log(&self, call: String) {
        self.calls.lock().expect("lock").push(call);
    }
}

fn succeeded(cents: i64) -> PaymentIntent {
    PaymentIntent {
        id: "pi_FakeT287".to_owned(),
        status: "succeeded".to_owned(),
        amount: cents,
        currency: "usd".to_owned(),
        customer: "cus_FakeT287".to_owned(),
        metadata: IntentMetadata {
            pick_id: "fake".to_owned(),
        },
        livemode: Some(false),
    }
}

impl PaymentsAdapter for Recorder {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no customer is made here")
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        panic!("a charge never creates a second, confirmed intent (FEE-047)")
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        self.log(format!("fetch {id}"));
        Err(AdapterError::NotSent)
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no refund is made here")
    }
}

impl IntentStripe for Recorder {
    async fn create_intent(
        &self,
        _: &IdempotencyKey,
        _: &str,
        cents: u32,
        _: &str,
    ) -> Result<String, AdapterError> {
        self.log(format!("create {cents}"));
        Ok("pi_FakeT287".to_owned())
    }

    async fn confirm_intent(
        &self,
        _: &IdempotencyKey,
        intent: &str,
        _: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        self.log(format!("confirm {intent}"));
        Ok(succeeded(2500))
    }
}

fn press(pick: Uuid, job: Uuid, cents: u32) -> Press<'static> {
    Press {
        pick,
        business: BUSINESS,
        job,
        amount_cents: cents,
        payment_method: "pm_FakeT287",
        press: Uuid::from_u128(pick.as_u128() + u128::from(cents)),
        emergency: false,
        now: PickNow::Payable,
    }
}

/// Opens her request and one job moved to Quotes ready, with firm quotes of `cents` by
/// businesses 1, 2, ... in that arrival order.
async fn job_ready(db: &mut PgConnection, her: Uuid, cents: &[i64]) -> Uuid {
    let first = text_in(her, "my kitchen sink is leaking under the cabinet", 0);
    let handled = receive_text(db, &secret(), &first, Settings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words: first.inbound.words,
        emergency: false,
        trade: "plumbing",
        at: at(1),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, at(2), Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    for (n, amount) in (1_i64..).zip(cents) {
        sqlx::query(
            "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
             confirm_on_site, notes, original, ai_read, confirmed, received_at) \
             VALUES ($1, $2, 'firm', $3, false, '', 'Sample wording', false, true, $4)",
        )
        .bind(job)
        .bind(quoter(n))
        .bind(amount)
        .bind(at(n + 3).to_sqlx())
        .execute(&mut *db)
        .await
        .expect("quote");
    }
    job
}

fn quoter(n: i64) -> Uuid {
    Uuid::from_u128(0xb287_0100 + u128::try_from(n).expect("small"))
}

fn text_in(contact: Uuid, words: &str, minute: i64) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128((contact.as_u128() << 32) + u128::from(minute.unsigned_abs())),
        contact,
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    }
}

/// The page pick of quote `n` on `job` in its own transaction, committed only when it picked.
async fn page_picked(pool: &PgPool, job: Uuid, n: usize) -> Result<PagePick, PicksError> {
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), n, Actor::System, at(20)).await;
    if picked.is_ok() {
        tx.commit().await.expect("commit");
    }
    picked
}

/// TD-FEE-48 (FEE-007, D-072, D-1871): the stored terms start at tiers and
/// `AUTHORIZATION_VERSION`; a switch stores the next terms with one audit row, and
/// `fee_terms_in_force` reads them back; choosing the mode in force changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_48_the_terms_in_force_are_stored_with_each_switch(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let start = FeeTerms::new(FeeMode::Tiers, AUTHORIZATION_VERSION);
    assert_eq!(fee_terms_in_force(&mut db).await, Ok(start));
    let same = switch_fee_mode(&mut db, OWNER, FeeMode::Tiers).await;
    assert_eq!(same, Ok(None));
    let flat = FeeTerms::new(FeeMode::Flat, AUTHORIZATION_VERSION + 1);
    let switched = switch_fee_mode(&mut db, OWNER, FeeMode::Flat).await;
    assert_eq!(switched, Ok(Some(flat)));
    assert_eq!(fee_terms_in_force(&mut db).await, Ok(flat));
    let audit = "SELECT count(*) FROM ops.audit_log WHERE subject_kind = 'payments.fee_mode'";
    assert_eq!(count(&pool, audit).await, 1);
    let back = switch_fee_mode(&mut db, OWNER, FeeMode::Tiers).await;
    let tiers = FeeTerms::new(FeeMode::Tiers, AUTHORIZATION_VERSION + 2);
    assert_eq!(back, Ok(Some(tiers)));
    assert_eq!(fee_terms_in_force(&mut db).await, Ok(tiers));
    assert_eq!(count(&pool, audit).await, 2);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM payments.fee_terms").await,
        1
    );
}

/// TD-FEE-48 (FEE-005, FEE-007, D-1415, D-1871): a text pick stores one fee in tiers mode, and a
/// page pick made after the switch to flat one in flat mode, each in the pick's transaction with
/// the fee `fee_at_pick` gives for the chosen quote and the terms version in force.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_48_a_text_pick_and_a_page_pick_each_fix_one_fee(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let her = Uuid::from_u128(0x0287_0001);
    let texted = job_ready(&mut db, her, &[19_000, 90_000]).await;
    for minute in [10, 11] {
        let msg = text_in(her, "2 YES", minute);
        answer_text_pick(&mut db, &secret(), &msg, (Settings::default(), &TERMS))
            .await
            .expect("text pick");
    }
    let rows = fee_rows(&pool).await;
    assert_eq!(rows.len(), 1);
    let pick: Uuid = sqlx::query_scalar("SELECT id FROM picks.pick WHERE job_id = $1")
        .bind(texted)
        .fetch_one(&pool)
        .await
        .expect("the text pick");
    let tiers_fee = fee(90_000, FeeMode::Tiers).cents();
    let row = (
        pick,
        quoter(2),
        texted,
        "tiers".to_owned(),
        "charged".to_owned(),
    );
    assert_eq!(
        (
            rows[0].0,
            rows[0].1,
            rows[0].2,
            rows[0].3.clone(),
            rows[0].4.clone()
        ),
        row
    );
    assert_eq!(
        (rows[0].5, rows[0].6),
        (i64::try_from(tiers_fee).expect("cents"), 1)
    );
    switch_fee_mode(&mut db, OWNER, FeeMode::Flat)
        .await
        .expect("switched");
    let paged = job_ready(&mut db, Uuid::from_u128(0x0287_0002), &[19_000, 90_000]).await;
    let Ok(PagePick::Picked(pick)) = page_picked(&pool, paged, 2).await else {
        panic!("a page pick");
    };
    let rows = fee_rows(&pool).await;
    assert_eq!(rows.len(), 2);
    let flat_fee = fee(90_000, FeeMode::Flat).cents();
    assert_eq!((rows[1].0, rows[1].1, rows[1].2), (pick, quoter(2), paged));
    assert_eq!(
        (rows[1].3.as_str(), rows[1].4.as_str()),
        ("flat", "charged")
    );
    assert_eq!(
        (rows[1].5, rows[1].6),
        (i64::try_from(flat_fee).expect("cents"), 2)
    );
    let stored = pick_fee(&mut db, pick).await.expect("read").expect("fee");
    assert_eq!(
        (stored.mode, stored.cents, stored.terms_version),
        (FeeMode::Flat, flat_fee, 2)
    );
}

/// TD-FEE-48 (FEE-005, D-1871): a tiered quote names no one price, so its fee can't be fixed:
/// the page pick fails, its transaction rolls back and no pick, fee or move is stored.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_48_a_pick_whose_fee_cannot_be_fixed_stores_no_pick(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let job = job_ready(&mut db, Uuid::from_u128(0x0287_0003), &[]).await;
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, tier_names, tier_cents, \
         confirm_on_site, notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'tiers', ARRAY['Basic', 'Plus'], ARRAY[10000, 20000]::bigint[], \
         false, '', 'Sample wording', false, true, $3)",
    )
    .bind(job)
    .bind(quoter(1))
    .bind(at(4).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("tiered quote");
    assert_eq!(page_picked(&pool, job, 1).await, Err(PicksError::Quotes));
    assert_eq!(count(&pool, "SELECT count(*) FROM picks.pick").await, 0);
    assert!(fee_rows(&pool).await.is_empty());
    let status: String = sqlx::query_scalar("SELECT status FROM jobs.job WHERE id = $1")
        .bind(job)
        .fetch_one(&pool)
        .await
        .expect("status");
    assert_eq!(status, "quotes_ready");
}

/// TD-FEE-48 (FEE-005, D-1871): the fee is written once; a repeat with the same values writes
/// nothing, other values or a fee fixed under another mode are refused, and the app role can
/// neither update nor delete a stored fee (SQLSTATE 42501).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_48_a_fee_is_fixed_once_and_never_changed(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (pick, job) = (Uuid::from_u128(0x0287_0010), Uuid::from_u128(0x0287_0110));
    let small = fee(19_000, FeeMode::Tiers);
    let fix = |f: PickedFee, business: BusinessId| {
        let pool = pool.clone();
        async move {
            let mut conn = pool.acquire().await.expect("connection");
            fix_fee_at_pick(&mut conn, pick, business, job, &f, at(1)).await
        }
    };
    assert_eq!(fix(small, BUSINESS).await, Ok(true));
    assert_eq!(fix(small, BUSINESS).await, Ok(false));
    let other = BusinessId(Uuid::from_u128(0xb287_0002));
    assert_eq!(fix(small, other).await, Err(PickFeeError::Conflict));
    let large = fee(200_000, FeeMode::Tiers);
    assert_eq!(fix(large, BUSINESS).await, Err(PickFeeError::Conflict));
    let flat = fee(19_000, FeeMode::Flat);
    assert_eq!(fix(flat, BUSINESS).await, Err(PickFeeError::TermsChanged));
    assert_eq!(fee_rows(&pool).await.len(), 1);
    for change in [
        "UPDATE payments.pick_fee SET amount_cents = 19900",
        "DELETE FROM payments.pick_fee",
    ] {
        let mut tx = pool.begin().await.expect("tx");
        sqlx::query("SET LOCAL ROLE askhava_app")
            .execute(&mut *tx)
            .await
            .expect("role");
        let refused = sqlx::query(change)
            .execute(&mut *tx)
            .await
            .expect_err(change);
        let code = refused.as_database_error().and_then(|e| e.code());
        assert_eq!(code.as_deref(), Some("42501"), "{change}");
    }
    let stored = pick_fee(&mut db, pick).await.expect("read").expect("fee");
    assert_eq!(
        (stored.business, stored.job, stored.cents),
        (BUSINESS, job, 2500)
    );
}

/// TD-FEE-48 (FEE-005, FEE-016, FEE-047, D-1871): a press with no stored fee, for another
/// business or job, on a $0 fee, or one cent above or below the stored fee is `FeeMismatch` from
/// both `open_intent` and `confirm_and_pay`, and so is a press on a pick whose PaymentIntent was
/// opened for another amount; Stripe is never called.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_48_a_press_must_match_the_stored_fee(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    ready(&mut db, 1).await;
    let (pick, job) = (Uuid::from_u128(0x0287_0020), Uuid::from_u128(0x0287_0120));
    fix_fee_at_pick(
        &mut db,
        pick,
        BUSINESS,
        job,
        &fee(19_000, FeeMode::Tiers),
        at(1),
    )
    .await
    .expect("fixed");
    let free_job = PeriodJob {
        job: FeeJob {
            fee_free: true,
            ..PRICED.job
        },
        ..PRICED
    };
    let free = fee_at_pick(
        FeeQuote::Fixed(19_000),
        None,
        free_job,
        FeeMode::Tiers,
        None,
    );
    let free_pick = Uuid::from_u128(0x0287_0021);
    fix_fee_at_pick(
        &mut db,
        free_pick,
        BUSINESS,
        job,
        &free.expect("fee"),
        at(1),
    )
    .await
    .expect("fixed");
    let opened = (Uuid::from_u128(0x0287_0022), Uuid::from_u128(0x0287_0122));
    fix_fee_at_pick(
        &mut db,
        opened.0,
        BUSINESS,
        opened.1,
        &fee(19_000, FeeMode::Tiers),
        at(1),
    )
    .await
    .expect("fixed");
    sqlx::query(
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents, \
         stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, 'pi_FakeOther', \
         false, now())",
    )
    .bind(opened.0)
    .bind(BUSINESS.0)
    .bind(opened.1)
    .execute(&mut *db)
    .await
    .expect("intent");
    let stripe = Recorder::default();
    let other_business = Press {
        business: BusinessId(Uuid::from_u128(0xb287_0002)),
        ..press(pick, job, 2500)
    };
    for p in [
        press(Uuid::from_u128(0x0287_0029), job, 2500),
        other_business,
        press(pick, Uuid::from_u128(0x0287_0129), 2500),
        press(free_pick, job, 2500),
        press(pick, job, 2501),
        press(pick, job, 2499),
        press(opened.0, opened.1, 2500),
    ] {
        let page = open_intent(&pool, &stripe, &secret(), &p, at(30)).await;
        assert_eq!(page, Err(ChargeError::FeeMismatch), "{p:?}");
        let charged = confirm_and_pay(&pool, &stripe, &secret(), &p, at(30)).await;
        assert_eq!(charged, Err(ChargeError::FeeMismatch), "{p:?}");
    }
    let stranger = Press {
        business: BusinessId(Uuid::from_u128(0xb287_0002)),
        ..press(opened.0, opened.1, 7500)
    };
    let page = open_intent(&pool, &stripe, &secret(), &stranger, at(30)).await;
    assert_eq!(page, Err(ChargeError::FeeMismatch), "D-2211");
    let charged = confirm_and_pay(&pool, &stripe, &secret(), &stranger, at(30)).await;
    assert_eq!(charged, Ok(ChargeOutcome::MovedOn), "D-2211");
    assert!(stripe.calls().is_empty(), "{:?}", stripe.calls());
    let attempts = "SELECT count(*) FROM payments.charge_attempt";
    assert_eq!(count(&pool, attempts).await, 0);
}

/// TD-FEE-48 (FEE-005, FEE-007, D-072, D-1871): a pick made in tiers mode and pressed after the
/// switch to flat is `FeeModeChanged` at the gate, whatever the business accepted, so no
/// PaymentIntent is opened and no ledger row written; a pick made after the switch needs the new
/// authorization version before it may confirm (D-072).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_48_a_switch_after_the_pick_is_refused_before_any_charge(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    ready(&mut db, 1).await;
    let (pick, job) = (Uuid::from_u128(0x0287_0030), Uuid::from_u128(0x0287_0130));
    fix_fee_at_pick(
        &mut db,
        pick,
        BUSINESS,
        job,
        &fee(19_000, FeeMode::Tiers),
        at(1),
    )
    .await
    .expect("fixed");
    switch_fee_mode(&mut db, OWNER, FeeMode::Flat)
        .await
        .expect("switched");
    let flat = FeeTerms::new(FeeMode::Flat, AUTHORIZATION_VERSION + 1);
    accept_authorization(
        &mut db,
        BUSINESS,
        flat,
        "Fake flat-mode authorization",
        at(2),
    )
    .await
    .expect("accepted");
    assert_eq!(
        may_confirm(&mut db, BUSINESS, pick).await,
        Err(ConfirmError::FeeModeChanged)
    );
    let after = (Uuid::from_u128(0x0287_0031), Uuid::from_u128(0x0287_0131));
    fix_fee_at_pick(
        &mut db,
        after.0,
        BUSINESS,
        after.1,
        &fee(19_000, FeeMode::Flat),
        at(3),
    )
    .await
    .expect("fixed");
    let other = BusinessId(Uuid::from_u128(0xb287_0003));
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)")
        .bind(other.0)
        .bind("cus_FakeT287b")
        .execute(&mut *db)
        .await
        .expect("customer");
    let old = FeeTerms::new(FeeMode::Tiers, AUTHORIZATION_VERSION);
    accept_authorization(&mut db, other, old, AUTHORIZATION_TEXT, at(0))
        .await
        .expect("accepted");
    let (late, late_job) = (Uuid::from_u128(0x0287_0032), Uuid::from_u128(0x0287_0132));
    fix_fee_at_pick(
        &mut db,
        late,
        other,
        late_job,
        &fee(19_000, FeeMode::Flat),
        at(3),
    )
    .await
    .expect("fixed");
    assert_eq!(
        may_confirm(&mut db, other, late).await,
        Err(ConfirmError::NewAuthorizationNeeded)
    );
    let ok = may_confirm(&mut db, BUSINESS, after.0)
        .await
        .expect("authorized");
    assert_eq!((ok.pick(), ok.terms()), (after.0, flat));
    assert_eq!(
        may_confirm(&mut db, other, pick).await,
        Err(ConfirmError::Mismatch)
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM payments.ledger_entry").await,
        0
    );
    assert_eq!(
        count(&pool, "SELECT count(*) FROM payments.pick_intent").await,
        0
    );
}

/// TD-FEE-48 (FEE-005, FEE-007, FEE-016, D-1415, D-1871): a switch committed after the gate
/// passed leaves that press's charge and its ledger entry at the stored fee, and
/// `record_fee_at_confirm` writes it; a proof for another pick is still `Mismatch`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_48_a_switch_after_the_gate_changes_nothing_for_that_pick(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    ready(&mut db, 1).await;
    let (pick, job) = (Uuid::from_u128(0x0287_0040), Uuid::from_u128(0x0287_0140));
    fix_fee_at_pick(
        &mut db,
        pick,
        BUSINESS,
        job,
        &fee(19_000, FeeMode::Tiers),
        at(1),
    )
    .await
    .expect("fixed");
    let authorized = may_confirm(&mut db, BUSINESS, pick).await.expect("gate");
    let stripe = Recorder::default();
    let charged = confirm_and_pay(&pool, &stripe, &secret(), &press(pick, job, 2500), at(30)).await;
    assert_eq!(
        charged,
        Ok(ChargeOutcome::Succeeded("pi_FakeT287".to_owned()))
    );
    assert_eq!(stripe.calls(), ["create 2500", "confirm pi_FakeT287"]);
    switch_fee_mode(&mut db, OWNER, FeeMode::Flat)
        .await
        .expect("switched");
    let confirmed = Confirmed {
        business: BUSINESS,
        job,
        charge: Some("ch_FakeT287"),
    };
    let other = (Uuid::from_u128(0x0287_0041), authorized);
    let refused = record_fee_at_confirm(&mut db, &secret(), other, confirmed).await;
    assert_eq!(refused, Err(ConfirmError::Mismatch));
    let recorded = record_fee_at_confirm(&mut db, &secret(), (pick, authorized), confirmed).await;
    assert!(matches!(recorded, Ok(Appended::New(_))), "{recorded:?}");
    let amount: i64 = sqlx::query_scalar("SELECT amount_cents FROM payments.ledger_entry")
        .fetch_one(&pool)
        .await
        .expect("ledger");
    assert_eq!(amount, 2500);
    let intent: i64 = sqlx::query_scalar("SELECT amount_cents FROM payments.pick_intent")
        .fetch_one(&pool)
        .await
        .expect("intent");
    assert_eq!(intent, 2500);
}
