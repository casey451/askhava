//! TD-FEE-30, TD-FEE-31 and TD-SEC-17's auto-confirm case (PICK-048, PICK-056, FEE-014, FEE-016,
//! FEE-041, SEC-038; D-401, D-480, D-1758): on a fresh database, the quote form's box is absent
//! until Stripe's written answer is filed, a tick needs a Verify code from the last 5 minutes, a
//! proven pick of a ticked quote makes exactly one off-session confirm with its own key, a decline
//! or `authentication_required` falls back to the normal Confirm step with no card-fix window, an
//! untick before the pick and a text pick charge nothing, and a removed member, a fee change or a
//! card change voids the tick (the card change with a notice to the business). The off-session
//! charge passes the same hard monthly limit as a press.
//!
//! Stripe is a fake whose on-session confirm panics, so only the off-session confirm can charge.
//! Stripe's written answer is inserted directly (T108 files it later, D-491). All IDs are fake.

use std::sync::Mutex;

use askhava_adapters::api::{
    AdapterError, Charge, IdempotencyKey, IntentMetadata, PaymentIntent, PaymentsAdapter,
};
use askhava_businesses::api::{Acting, add_owner, create, record_step_up};
use askhava_payments::api::{
    ATTEMPT_ACTION, BusinessId, CardFixWindow, FeeJob, FeeMode, FeeQuote, IntentStripe,
    OffSessionOutcome, OffSessionStripe, PeriodJob, RecurringCap, SavedCard, Service, VisitTier,
    attempt_label, card_fix_window, fee_at_pick, fix_fee_at_pick, save_card, set_fee_limit,
};
use askhava_picks::api::{AutoConfirmed, PickProof, Skip, Void, auto_confirm};
use askhava_portals::api::{
    AUTO_CONFIRM_ROUTE, AutoConfirmBoxError, BUSINESS_SESSION_LIFE, BusinessPrincipal, CookieJar,
    Realm, auto_confirm_box, business_principal, set_auto_confirm, start_session,
};
use askhava_quotes::api::{AUTO_CONFIRM_VERSION, AutoConfirmTick, Untick, auto_confirm_of};
use askhava_quotes::api::{tick_auto_confirm, untick_auto_confirm};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const FEE: u64 = 7500;
const CARD: &str = "pm_FakeT209";
type Answer = Result<PaymentIntent, AdapterError>;

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000).expect("time") + SignedDuration::from_secs(secs)
}

fn secret() -> SecretString {
    SecretString::from("fake-id-secret-t209".to_owned())
}

/// A fake Stripe: the off-session confirm answers `answer` and is recorded; the on-session
/// confirm panics (FEE-014: only the press charges on-session, and no press runs here).
struct FakeStripe {
    calls: Mutex<Vec<String>>,
    answer: fn() -> Answer,
}

impl FakeStripe {
    fn new(answer: fn() -> Answer) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            answer,
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("lock").clone()
    }
}

fn intent(status: &str) -> Answer {
    Ok(PaymentIntent {
        id: "pi_FakeT209".to_owned(),
        status: status.to_owned(),
        amount: 7500,
        currency: "usd".to_owned(),
        customer: "cus_FakeT209".to_owned(),
        metadata: IntentMetadata {
            pick_id: "fake".to_owned(),
        },
        livemode: Some(false),
    })
}

fn succeeded() -> Answer {
    intent("succeeded")
}

fn declined() -> Answer {
    Err(AdapterError::Vendor {
        status: 402,
        code: None,
    })
}

fn needs_approval() -> Answer {
    intent("requires_action")
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok("cus_FakeT209".to_owned())
    }

    async fn create_payment_intent(&self, _: &IdempotencyKey, _: Charge<'_>) -> Answer {
        panic!("no confirmed intent is created")
    }

    async fn fetch_payment_intent(&self, _: &str) -> Answer {
        (self.answer)()
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no refund here")
    }
}

impl IntentStripe for FakeStripe {
    async fn create_intent(
        &self,
        _: &IdempotencyKey,
        _: &str,
        cents: u32,
        _: &str,
    ) -> Result<String, AdapterError> {
        self.calls
            .lock()
            .expect("lock")
            .push(format!("create {cents}"));
        Ok("pi_FakeT209".to_owned())
    }

    async fn confirm_intent(&self, _: &IdempotencyKey, _: &str, _: &str) -> Answer {
        panic!("an auto-confirm never confirms on-session")
    }
}

impl OffSessionStripe for FakeStripe {
    async fn confirm_off_session(&self, key: &IdempotencyKey, intent: &str, pm: &str) -> Answer {
        let call = format!("off_session {} {intent} {pm}", key.as_str());
        self.calls.lock().expect("lock").push(call);
        (self.answer)()
    }
}

/// One business, its owner signed in, a customer, a saved card, a job with a quote and a waiting
/// page pick whose fee ($75) is fixed.
struct World {
    pool: PgPool,
    who: Acting,
    principal: BusinessPrincipal,
    job: Uuid,
    pick: Uuid,
}

async fn signed_in(db: &mut PgConnection) -> (Acting, BusinessPrincipal) {
    let business = create(db, false, at(0)).await.expect("business");
    let member = add_owner(db, business, Uuid::from_u128(0x2090_00d1), at(0))
        .await
        .expect("owner");
    let jar = start_session(
        db,
        CookieJar::new(),
        (Realm::Business, member),
        BUSINESS_SESSION_LIFE,
        at(0),
    )
    .await
    .expect("session");
    sqlx::query(
        "INSERT INTO portals.business_session (session_id, business_id) \
         SELECT id, $2 FROM portals.session WHERE actor_id = $1",
    )
    .bind(member)
    .bind(business)
    .execute(&mut *db)
    .await
    .expect("business session");
    let principal = business_principal(db, &jar, at(1)).await.expect("read");
    (Acting { business, member }, principal.expect("signed in"))
}

async fn save(db: &mut PgConnection, business: Uuid, card: &str, when: Timestamp) {
    let card = SavedCard::new(card, "card", "visa", "4242").expect("card");
    save_card(db, BusinessId(business), &card, when)
        .await
        .expect("saved");
}

async fn world(pool: PgPool) -> World {
    let mut db = pool.acquire().await.expect("db");
    let (who, principal) = signed_in(&mut db).await;
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)")
        .bind(who.business)
        .bind("cus_FakeT209")
        .execute(&mut *db)
        .await
        .expect("customer");
    save(&mut db, who.business, CARD, at(2)).await;
    let (job, pick) = (Uuid::from_u128(0x2090_000b), Uuid::from_u128(0x2090_000c));
    seed_job(&mut db, (job, pick, who.business)).await;
    drop(db);
    World {
        pool,
        who,
        principal,
        job,
        pick,
    }
}

async fn seed_job(db: &mut PgConnection, (job, pick, business): (Uuid, Uuid, Uuid)) {
    let statements = [
        "INSERT INTO jobs.job (id, status, created_at, status_at, timer_from) \
         VALUES ($1, 'awaiting_confirm', $4, $4, $4)",
        "INSERT INTO jobs.job_number (number, job_id, issued_at) VALUES (1209, $1, $4)",
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, confirm_on_site, \
         notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $3, 'firm', 100000, false, '', 'Fake quote', false, true, $4)",
        "INSERT INTO picks.pick (id, job_id, business_id, quote_number, channel, status, \
         picked_at) VALUES ($2, $1, $3, 1, 'page', 'awaiting_confirm', $4)",
    ];
    for sql in statements {
        sqlx::query(sql)
            .bind(job)
            .bind(pick)
            .bind(business)
            .bind(at(3).to_sqlx())
            .execute(&mut *db)
            .await
            .expect(sql);
    }
    let fee = fee_at_pick(
        FeeQuote::Fixed(100_000),
        None,
        period_job(),
        FeeMode::Tiers,
        None,
    );
    let fee = fee.expect("fee");
    fix_fee_at_pick(db, pick, BusinessId(business), job, &fee, at(3))
        .await
        .expect("fee fixed");
}

fn period_job() -> PeriodJob {
    PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier: VisitTier::DEFAULT,
            fee_free: false,
        },
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    }
}

async fn file_answer(pool: &PgPool, allows: bool, when: Timestamp) {
    sqlx::query(
        "INSERT INTO catalog.stripe_written_answer (answered_on, reference, allows, filed_at) \
         VALUES ('2026-11-01', 'FAKE-REF-209', $1, $2)",
    )
    .bind(allows)
    .bind(when.to_sqlx())
    .execute(pool)
    .await
    .expect("answer filed");
}

impl World {
    async fn tick(&self, member: Uuid, fee: u64, card: &str) {
        let tick = AutoConfirmTick {
            member,
            fee_cents: fee,
            authorization_version: AUTO_CONFIRM_VERSION,
            payment_method: card.to_owned(),
        };
        let mut db = self.pool.acquire().await.expect("db");
        tick_auto_confirm(&mut db, (self.job, self.who.business), &tick, at(10))
            .await
            .expect("ticked");
    }

    async fn run(&self, stripe: &FakeStripe, proof: PickProof) -> AutoConfirmed {
        auto_confirm(
            &self.pool,
            stripe,
            &secret(),
            (self.pick, proof, false),
            at(60),
        )
        .await
        .expect("auto-confirm")
    }

    async fn count(&self, sql: &'static str) -> i64 {
        sqlx::query_scalar(sql)
            .bind(self.pick)
            .fetch_one(&self.pool)
            .await
            .expect(sql)
    }

    async fn void_notices(&self) -> i64 {
        self.count(
            "SELECT count(*) FROM core.task WHERE kind = 'picks.auto_confirm_voided' \
             AND payload->>'why' = 'card_changed' AND payload->>'pick' = $1::text",
        )
        .await
    }
}

fn key() -> String {
    let label = attempt_label(Uuid::from_u128(0x2090_000c), 1);
    IdempotencyKey::derive(&secret(), ATTEMPT_ACTION, &[&label])
        .as_str()
        .to_owned()
}

/// TD-FEE-30 (PICK-048, D-401): with no filed answer, or one saying no, the box is absent and a
/// ticked quote's pick charges nothing and calls no Stripe.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_30_no_filed_answer_no_box_and_no_charge(pool: PgPool) {
    let w = world(pool).await;
    let mut db = w.pool.acquire().await.expect("db");
    let shown = auto_confirm_box(&mut db, &w.principal, (w.job, Some(FEE))).await;
    assert_eq!(shown, Ok(None), "no record, no box");
    file_answer(&w.pool, false, at(4)).await;
    let shown = auto_confirm_box(&mut db, &w.principal, (w.job, Some(FEE))).await;
    assert_eq!(shown, Ok(None), "an answer saying no, no box");
    w.tick(w.who.member, FEE, CARD).await;
    let stripe = FakeStripe::new(succeeded);
    let done = w.run(&stripe, PickProof::PageCode).await;
    assert_eq!(done, AutoConfirmed::Skipped(Skip::NotFiled));
    assert!(stripe.calls().is_empty());
    file_answer(&w.pool, true, at(5)).await;
    let shown = auto_confirm_box(&mut db, &w.principal, (w.job, Some(FEE))).await;
    let shown = shown.expect("read").expect("box shown once filed");
    assert!(shown.ticked);
    assert!(
        shown
            .authorization
            .contains("$75 for this quote's introduction")
    );
}

/// TD-FEE-30 (PICK-048, FEE-016): a page-code pick of a ticked quote makes exactly one
/// off-session confirm of the fixed fee with its own attempt key; running it again sends nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_30_one_off_session_charge_with_its_own_key(pool: PgPool) {
    let w = world(pool).await;
    file_answer(&w.pool, true, at(4)).await;
    w.tick(w.who.member, FEE, CARD).await;
    let stripe = FakeStripe::new(succeeded);
    let done = w.run(&stripe, PickProof::PageCode).await;
    let paid = OffSessionOutcome::Charged("pi_FakeT209".to_owned());
    assert_eq!(done, AutoConfirmed::Tried(paid.clone()));
    let again = w.run(&stripe, PickProof::PortalSession).await;
    assert_eq!(again, AutoConfirmed::Tried(paid));
    let expected = vec![
        "create 7500".to_owned(),
        format!("off_session {} pi_FakeT209 {CARD}", key()),
    ];
    assert_eq!(
        stripe.calls(),
        expected,
        "one intent, one off-session confirm"
    );
    let attempts = w
        .count("SELECT count(*) FROM payments.charge_attempt WHERE pick_id = $1")
        .await;
    assert_eq!(attempts, 1);
}

/// TD-FEE-30 (PICK-048, PICK-019): a decline and an `authentication_required` answer each fall
/// back to the normal Confirm step: nothing charged, the pick still waits, no card-fix window or
/// card-fix notice runs.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_30_a_decline_falls_back_to_confirm(pool: PgPool) {
    let w = world(pool).await;
    file_answer(&w.pool, true, at(4)).await;
    w.tick(w.who.member, FEE, CARD).await;
    let stripe = FakeStripe::new(declined);
    let done = w.run(&stripe, PickProof::PageCode).await;
    assert_eq!(done, AutoConfirmed::Tried(OffSessionOutcome::FellBack));
    let mut db = w.pool.acquire().await.expect("db");
    let window = card_fix_window(&mut db, w.pick).await.expect("window");
    assert_eq!(window, CardFixWindow::Closed, "no card-fix window");
    let waiting = w
        .count("SELECT count(*) FROM picks.pick WHERE id = $1 AND status = 'awaiting_confirm'")
        .await;
    assert_eq!(waiting, 1, "the pick waits for the normal Confirm step");
}

/// TD-FEE-30 (PICK-048): a card the bank must approve off-session falls back the same way.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_30_authentication_required_falls_back(pool: PgPool) {
    let w = world(pool).await;
    file_answer(&w.pool, true, at(4)).await;
    w.tick(w.who.member, FEE, CARD).await;
    let stripe = FakeStripe::new(needs_approval);
    let done = w.run(&stripe, PickProof::PortalSession).await;
    assert_eq!(done, AutoConfirmed::Tried(OffSessionOutcome::FellBack));
    let mut db = w.pool.acquire().await.expect("db");
    let window = card_fix_window(&mut db, w.pick).await.expect("window");
    assert_eq!(window, CardFixWindow::Closed);
}

/// TD-FEE-30 (PICK-048): a box unticked before the pick changes nothing: no Stripe call.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_30_unticked_before_the_pick_changes_nothing(pool: PgPool) {
    let w = world(pool).await;
    file_answer(&w.pool, true, at(4)).await;
    w.tick(w.who.member, FEE, CARD).await;
    let mut db = w.pool.acquire().await.expect("db");
    let quote = (w.job, w.who.business);
    let ended = untick_auto_confirm(&mut db, quote, Untick::Business, at(20)).await;
    assert_eq!(ended, Ok(true));
    let stripe = FakeStripe::new(succeeded);
    let done = w.run(&stripe, PickProof::PageCode).await;
    assert_eq!(done, AutoConfirmed::Skipped(Skip::NotTicked));
    assert!(stripe.calls().is_empty());
}

/// TD-FEE-31 (PICK-056): a text pick of an auto-confirm quote makes no off-session charge; the
/// tick stays and the normal Confirm email goes as for any pick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_31_a_text_pick_never_auto_charges(pool: PgPool) {
    let w = world(pool).await;
    file_answer(&w.pool, true, at(4)).await;
    w.tick(w.who.member, FEE, CARD).await;
    let stripe = FakeStripe::new(succeeded);
    let done = w.run(&stripe, PickProof::Text).await;
    assert_eq!(done, AutoConfirmed::Skipped(Skip::NotProven));
    assert!(stripe.calls().is_empty());
    let mut db = w.pool.acquire().await.expect("db");
    let kept = auto_confirm_of(&mut db, (w.job, w.who.business)).await;
    assert!(kept.expect("read").is_some(), "the tick stays");
}

/// TD-FEE-31 (PICK-056): a tick by a member since removed does not fire, and is voided.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_31_a_removed_members_tick_does_not_fire(pool: PgPool) {
    let w = world(pool).await;
    file_answer(&w.pool, true, at(4)).await;
    let member = Uuid::from_u128(0x2090_00e1);
    sqlx::query(
        "INSERT INTO businesses.team_member (id, business_id, mailbox_id, cards_and_quotes, \
         confirm_and_billing, added_at, removed_at) VALUES ($1, $2, $3, false, true, $4, $5)",
    )
    .bind(member)
    .bind(w.who.business)
    .bind(Uuid::from_u128(0x2090_00e2))
    .bind(at(1).to_sqlx())
    .bind(at(30).to_sqlx())
    .execute(&w.pool)
    .await
    .expect("removed member");
    w.tick(member, FEE, CARD).await;
    let stripe = FakeStripe::new(succeeded);
    let done = w.run(&stripe, PickProof::PageCode).await;
    assert_eq!(done, AutoConfirmed::Voided(Void::AuthorGone));
    assert!(stripe.calls().is_empty());
}

/// TD-FEE-31 (PICK-056): a fee change between the tick and the pick falls back to Confirm.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_31_a_fee_change_falls_back(pool: PgPool) {
    let w = world(pool).await;
    file_answer(&w.pool, true, at(4)).await;
    w.tick(w.who.member, 2500, CARD).await;
    let stripe = FakeStripe::new(succeeded);
    let done = w.run(&stripe, PickProof::PageCode).await;
    assert_eq!(done, AutoConfirmed::Voided(Void::FeeChanged));
    assert!(stripe.calls().is_empty());
}

/// TD-FEE-31 (PICK-056): a card change voids the tick and the business is told, once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_31_a_card_change_voids_and_tells_the_business(pool: PgPool) {
    let w = world(pool).await;
    file_answer(&w.pool, true, at(4)).await;
    w.tick(w.who.member, FEE, CARD).await;
    let mut db = w.pool.acquire().await.expect("db");
    save(&mut db, w.who.business, "pm_FakeT209New", at(40)).await;
    let stripe = FakeStripe::new(succeeded);
    let done = w.run(&stripe, PickProof::PageCode).await;
    assert_eq!(done, AutoConfirmed::Voided(Void::CardChanged));
    assert!(stripe.calls().is_empty());
    assert_eq!(w.void_notices().await, 1, "the business is told");
    let tick = auto_confirm_of(&mut db, (w.job, w.who.business)).await;
    assert_eq!(tick, Ok(None), "the tick is void");
}

/// D-1758, FEE-041: the off-session charge passes the same hard monthly limit as a press: over
/// the limit nothing is confirmed.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn fee_041_the_off_session_charge_keeps_the_monthly_limit(pool: PgPool) {
    let w = world(pool).await;
    file_answer(&w.pool, true, at(4)).await;
    w.tick(w.who.member, FEE, CARD).await;
    let mut db = w.pool.acquire().await.expect("db");
    set_fee_limit(&mut db, BusinessId(w.who.business), Some(5000))
        .await
        .expect("limit");
    let stripe = FakeStripe::new(succeeded);
    let done = w.run(&stripe, PickProof::PageCode).await;
    assert_eq!(done, AutoConfirmed::Tried(OffSessionOutcome::OverLimit));
    assert!(stripe.calls().iter().all(|c| !c.starts_with("off_session")));
}

/// TD-SEC-17 (SEC-038, D-480): ticking the box with a live session but no Verify code in the last
/// 5 minutes is refused and logged; with a 4-minute-old code it is stored; at 5:01 it is refused.
/// An untick needs no code.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_17_ticking_needs_a_fresh_code(pool: PgPool) {
    let w = world(pool).await;
    file_answer(&w.pool, true, at(4)).await;
    let mut db = w.pool.acquire().await.expect("db");
    let tick = |on: bool| (w.job, FEE, on);
    let none = set_auto_confirm(&mut db, &w.principal, tick(true), at(100)).await;
    assert_eq!(none, Err(AutoConfirmBoxError::NeedsCode));
    let logged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM portals.security_event WHERE kind = 'authz_denied' AND route = $1",
    )
    .bind(AUTO_CONFIRM_ROUTE)
    .fetch_one(&mut *db)
    .await
    .expect("events");
    assert_eq!(logged, 1, "the refusal is logged");
    record_step_up(&mut db, w.who, at(110)).await.expect("code");
    let late = set_auto_confirm(&mut db, &w.principal, tick(true), at(411)).await;
    assert_eq!(late, Err(AutoConfirmBoxError::NeedsCode), "5:01 refused");
    let fresh = set_auto_confirm(&mut db, &w.principal, tick(true), at(350)).await;
    assert_eq!(fresh, Ok(()), "4 minutes passes");
    let stored = auto_confirm_of(&mut db, (w.job, w.who.business)).await;
    let stored = stored.expect("read").expect("ticked");
    assert_eq!((stored.member, stored.fee_cents), (w.who.member, FEE));
    assert_eq!(stored.payment_method, CARD);
    let off = set_auto_confirm(&mut db, &w.principal, tick(false), at(900)).await;
    assert_eq!(off, Ok(()), "an untick needs no code");
    let gone = auto_confirm_of(&mut db, (w.job, w.who.business)).await;
    assert_eq!(gone, Ok(None));
}
