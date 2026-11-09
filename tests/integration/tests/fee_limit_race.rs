//! TD-FEE-46 (FEE-041, FEE-016, FEE-037; D-204, D-1427, D-1758): on PostgreSQL across separate
//! pooled connections, the monthly fee limit is a hard cap. Two presses at once on two picks of
//! one business, with room for one fee, start exactly one attempt; the undecided attempt reserves
//! its fee until it is decided, a failed one frees it and a success keeps it; a success decided
//! after midnight on the 1st counts in the month its press started; exactly at the limit is
//! charged; no transaction or row lock is open while the fake Stripe is called; and
//! `set_fee_limit` is one statement, so a change cut off part way keeps the old limit.
//!
//! Stripe is a scripted fake of `IntentStripe` that records every call and, on each confirm,
//! checks `pg_stat_activity` and `pg_locks` from its own connection. All IDs are fake.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use askhava_adapters::api::{
    AdapterError, Charge, IdempotencyKey, IntentMetadata, PaymentIntent, PaymentsAdapter,
};
use askhava_core::api::arizona;
use askhava_payments::api::{
    BusinessId, ChargeOutcome, IntentStripe, PickNow, Press, confirm_and_pay, set_fee_limit,
};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tokio::sync::Barrier;

const BUSINESS: BusinessId = BusinessId(Uuid::from_u128(0xb263_0001));
type Answer = Result<PaymentIntent, AdapterError>;

/// A scripted Stripe on its own pool connection. `gate` holds each intent create until both
/// racing presses passed the early limit check; `race` makes the confirm wait until one of the
/// two racing picks has ended, so it runs while the other press finishes its transaction.
struct Fake {
    pool: PgPool,
    gate: Option<Barrier>,
    race: Vec<Uuid>,
    confirms: Mutex<VecDeque<Answer>>,
    fetches: Mutex<VecDeque<Answer>>,
    calls: Mutex<Vec<String>>,
}

impl Fake {
    fn new(pool: &PgPool, confirms: Vec<Answer>, fetches: Vec<Answer>) -> Self {
        Self {
            pool: pool.clone(),
            gate: None,
            race: Vec::new(),
            confirms: Mutex::new(confirms.into()),
            fetches: Mutex::new(fetches.into()),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn count(&self, call: &str) -> usize {
        let calls = self.calls.lock().expect("lock");
        calls.iter().filter(|c| c.starts_with(call)).count()
    }

    fn log(&self, call: String) {
        self.calls.lock().expect("lock").push(call);
    }

    async fn wait_for_the_other_press(&self) {
        for _ in 0..500 {
            let ended: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM payments.pick_intent
                 WHERE pick_id = ANY($1) AND ended_at IS NOT NULL",
            )
            .bind(&self.race)
            .fetch_one(&self.pool)
            .await
            .expect("ended");
            if ended == 1 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the other press waited on a lock held across the Stripe call");
    }

    /// No other connection of this database is inside a transaction or holds a lock on the
    /// pick or fee-limit tables while Stripe is called (D-1758, Postgres brain P9).
    async fn assert_nothing_held(&self) {
        let open: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database()
             AND pid <> pg_backend_pid() AND xact_start IS NOT NULL",
        )
        .fetch_one(&self.pool)
        .await
        .expect("activity");
        let locks: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_locks WHERE pid <> pg_backend_pid() AND relation IN
             ('payments.fee_limit'::regclass, 'payments.pick_intent'::regclass)",
        )
        .fetch_one(&self.pool)
        .await
        .expect("locks");
        assert_eq!((open, locks), (0, 0), "a transaction or lock is open");
    }
}

fn intent(status: &str) -> Answer {
    Ok(PaymentIntent {
        id: "pi_FakeT263".to_owned(),
        status: status.to_owned(),
        amount: 2500,
        currency: "usd".to_owned(),
        customer: "cus_FakeT263".to_owned(),
        metadata: IntentMetadata {
            pick_id: "fake".to_owned(),
        },
        livemode: Some(false),
    })
}

impl PaymentsAdapter for Fake {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no customer is made here")
    }

    async fn create_payment_intent(&self, _: &IdempotencyKey, _: Charge<'_>) -> Answer {
        panic!("a charge never creates a confirmed intent (FEE-047)")
    }

    async fn fetch_payment_intent(&self, id: &str) -> Answer {
        self.log(format!("fetch {id}"));
        self.assert_nothing_held().await;
        let next = self.fetches.lock().expect("lock").pop_front();
        next.unwrap_or(Err(AdapterError::NotSent))
    }

    async fn create_refund(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        panic!("no refund is made here")
    }
}

impl IntentStripe for Fake {
    async fn create_intent(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: u32,
        pick: &str,
    ) -> Result<String, AdapterError> {
        self.log(format!("create {pick}"));
        if let Some(gate) = &self.gate {
            gate.wait().await;
        }
        Ok(format!("pi_FakeT263_{}", pick.replace('-', "")))
    }

    async fn confirm_intent(&self, _: &IdempotencyKey, intent: &str, _: &str) -> Answer {
        self.log(format!("confirm {intent}"));
        if !self.race.is_empty() {
            self.wait_for_the_other_press().await;
        }
        self.assert_nothing_held().await;
        let next = self.confirms.lock().expect("lock").pop_front();
        next.expect("a scripted confirm answer")
    }
}

/// Arizona wall-clock time on fake days in 2026.
fn az(month: i8, day: i8, hour: i8, minute: i8) -> Timestamp {
    jiff::civil::date(2026, month, day)
        .at(hour, minute, 0, 0)
        .to_zoned(arizona())
        .expect("zoned")
        .timestamp()
}

fn press(pick: u128, cents: u32) -> Press<'static> {
    Press {
        pick: Uuid::from_u128(pick),
        business: BUSINESS,
        job: Uuid::from_u128(pick + 0x100),
        amount_cents: cents,
        payment_method: "pm_FakeCard",
        press: Uuid::from_u128(pick * 100 + 1),
        emergency: false,
        now: PickNow::Payable,
    }
}

async fn with_limit(pool: &PgPool, cents: u32) {
    sqlx::query("INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)")
        .bind(BUSINESS.0)
        .bind("cus_FakeT263")
        .execute(pool)
        .await
        .expect("customer");
    let mut conn = pool.acquire().await.expect("conn");
    set_fee_limit(&mut conn, BUSINESS, Some(cents))
        .await
        .expect("limit");
}

async fn pay(pool: &PgPool, stripe: &Fake, p: &Press<'_>, at: Timestamp) -> ChargeOutcome {
    confirm_and_pay(pool, stripe, &SecretString::from("fake-secret-t263"), p, at)
        .await
        .expect("press")
}

fn paid(outcome: &ChargeOutcome) -> bool {
    matches!(outcome, ChargeOutcome::Succeeded(_))
}

/// TD-FEE-46 (FEE-041, FEE-016, D-1758): $100 limit, $75 charged; two $25 presses at once on two
/// picks start one attempt, which reaches Stripe while the other press ends over the limit; its
/// decline frees the $25, so a later press on another pick charges exactly at the limit, and that
/// success keeps it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_46_two_presses_at_once_start_one_attempt(pool: PgPool) {
    with_limit(&pool, 10_000).await;
    let first = Fake::new(&pool, vec![intent("succeeded")], vec![]);
    assert!(paid(
        &pay(&pool, &first, &press(0x2631, 7500), az(11, 3, 9, 0)).await
    ));
    let mut race = Fake::new(&pool, vec![intent("requires_payment_method")], vec![]);
    race.gate = Some(Barrier::new(2));
    race.race = vec![Uuid::from_u128(0x2632), Uuid::from_u128(0x2633)];
    let at = az(11, 3, 10, 0);
    let (b, c) = (press(0x2632, 2500), press(0x2633, 2500));
    let (b, c) = tokio::join!(pay(&pool, &race, &b, at), pay(&pool, &race, &c, at));
    let over = [&b, &c]
        .iter()
        .filter(|o| ***o == ChargeOutcome::OverLimit)
        .count();
    assert_eq!(over, 1, "{b:?} {c:?}");
    assert_eq!((race.count("create"), race.count("confirm")), (2, 1));
    let attempts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM payments.charge_attempt WHERE pick_id = ANY($1)")
            .bind(&race.race)
            .fetch_one(&pool)
            .await
            .expect("attempts");
    assert_eq!(attempts, 1);
    let later = Fake::new(&pool, vec![intent("succeeded")], vec![]);
    assert!(paid(
        &pay(&pool, &later, &press(0x2634, 2500), az(11, 3, 11, 0)).await
    ));
    let full = pay(&pool, &later, &press(0x2635, 2500), az(11, 3, 11, 5)).await;
    assert_eq!(full, ChargeOutcome::OverLimit);
    assert_eq!(
        later.count("confirm"),
        1,
        "the press over the limit reaches no confirm"
    );
}

/// TD-FEE-46 (FEE-041, FEE-037, D-1758): an attempt left undecided at 11:55 p.m. on October 31
/// reserves its $75, so a $50 press stops; decided as a success at 12:10 a.m. on November 1, it
/// counts in October, not November, where a $100 press is charged exactly at the limit.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_46_a_success_counts_in_the_month_its_press_started(pool: PgPool) {
    with_limit(&pool, 10_000).await;
    let late = Fake::new(
        &pool,
        vec![Err(AdapterError::OutcomeUnknown), intent("succeeded")],
        vec![intent("succeeded")],
    );
    let a = press(0x2641, 7500);
    assert_eq!(
        pay(&pool, &late, &a, az(10, 31, 23, 55)).await,
        ChargeOutcome::Pending
    );
    let held = pay(&pool, &late, &press(0x2642, 5000), az(10, 31, 23, 56)).await;
    assert_eq!(
        held,
        ChargeOutcome::OverLimit,
        "the undecided $75 is reserved"
    );
    assert!(paid(&pay(&pool, &late, &a, az(11, 1, 0, 10)).await));
    assert!(paid(
        &pay(&pool, &late, &press(0x2643, 10_000), az(11, 1, 0, 20)).await
    ));
    let october = pay(&pool, &late, &press(0x2644, 5000), az(10, 31, 23, 59)).await;
    assert_eq!(october, ChargeOutcome::OverLimit, "October holds the $75");
    assert_eq!(late.count("confirm"), 2);
}

/// TD-FEE-46 (FEE-041, D-1758): a limit change that fails part way (a trigger refuses the new
/// value) keeps the old limit, because `set_fee_limit` is one statement; `None` removes it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_46_set_fee_limit_is_one_statement(pool: PgPool) {
    with_limit(&pool, 10_000).await;
    sqlx::raw_sql(
        "CREATE FUNCTION public.t263_cut() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN IF NEW.limit_cents = 99999 THEN RAISE EXCEPTION 'cut off'; END IF;
         RETURN NEW; END $$;
         CREATE TRIGGER t263_cut BEFORE INSERT OR UPDATE ON payments.fee_limit
         FOR EACH ROW EXECUTE FUNCTION public.t263_cut();",
    )
    .execute(&pool)
    .await
    .expect("trigger");
    let mut conn = pool.acquire().await.expect("conn");
    let limit = "SELECT limit_cents FROM payments.fee_limit WHERE business_id = $1";
    let read = |pool: PgPool| async move {
        let found: Option<i64> = sqlx::query_scalar(limit)
            .bind(BUSINESS.0)
            .fetch_optional(&pool)
            .await
            .expect("limit");
        found
    };
    assert!(
        set_fee_limit(&mut conn, BUSINESS, Some(99_999))
            .await
            .is_err()
    );
    assert_eq!(read(pool.clone()).await, Some(10_000));
    set_fee_limit(&mut conn, BUSINESS, Some(20_000))
        .await
        .expect("raised");
    assert_eq!(read(pool.clone()).await, Some(20_000));
    set_fee_limit(&mut conn, BUSINESS, None)
        .await
        .expect("removed");
    assert_eq!(read(pool.clone()).await, None);
}
