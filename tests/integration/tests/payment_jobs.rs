//! TD-FEE-52 (T433; FEE-019, FEE-020, FEE-021, FEE-026, FEE-027, FEE-029; D-2286, D-3468): the
//! `payments` jobs run in the production worker registry against PostgreSQL and the stand-in
//! Stripe (`python3 -m stand_ins.server`, started per test on a free port).
//!
//! The registry is `askhava_server::worker_registry` from dev settings (no Stripe key: every
//! payments kind is registered and a run ends done), and for the money cases the same registry
//! with `payment_jobs::register` over the Stripe client `release_sink::stripe_webhook` builds
//! from dev settings naming the stand-in, as `worker_registry` does with the Stripe settings. The
//! composition root is compiled in place as `pick_ended.rs` does (D-2853's exception, logged for this file in D-3621). Each due task runs
//! through `Registry::run` on a manual clock. A pick ended by `payments` (the four-hour end,
//! FEE-018, through `payments.pay_timer` over an unreachable Stripe) that then succeeds by the
//! check round (`payments.charge_check`) or by the webhook gives one `late-success` refund and one
//! alert; the refund's ledger write failing once (the worker stopped after Stripe answered) is
//! finished by the retry with one refund and one ledger row. A success for a pick `picks` moved
//! on is refunded and releases nothing; two starts seed one reconcile. The stand-in sends no
//! `livemode`, so the webhook's re-fetch reads it as test mode (`TestMode`). All IDs are fake.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, Charge, Delivery, IdempotencyKey, PaymentIntent, PaymentsAdapter, TwilioConfig,
    WebhookVendor, verify_webhook,
};
use askhava_core::api::{Clock, Lease, ManualClock, NewTask, Registry, Settings, WorkerId, claim};
use askhava_payments::api::{
    CHECK_KIND, LATE_SUCCESS_ALERT, REFUND_CHECK_KIND, ReleaseError, StripeAccount, TIMER_KIND,
    VerifiedStripeEvent, paid_release,
};
use askhava_picks::api::{ConfirmSettings, Released};
use askhava_privacy::api::Vault;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(
    dead_code,
    reason = "only the worker registry and the release sink are called here"
)]
mod server;
use server::*;

const BUSINESS: Uuid = Uuid::from_u128(0xb433_0001);
const PICK: Uuid = Uuid::from_u128(0x433_0042);
const JOB: Uuid = Uuid::from_u128(0x433_0142);
const ID_SECRET: &str = "test-only-id-secret-for-t433-not-a-real-key";
const HOOK_SECRET: &str = "whsec_fake_t433_not_a_real_secret";
const NOW: i64 = 1_794_100_000;

fn at(offset: i64) -> Timestamp {
    Timestamp::from_second(NOW + offset).expect("time")
}

fn secret() -> SecretString {
    SecretString::from(ID_SECRET.to_owned())
}

/// The stand-in on the free port it bound itself, killed on drop.
struct StandIn(Child, String);

impl Drop for StandIn {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn stand_in() -> StandIn {
    let mut child = Command::new("python3")
        .args(["-m", "stand_ins.server", "--port", "0"])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .stdout(Stdio::piped())
        .spawn()
        .expect("python3 starts the stand-in");
    let mut line = String::new();
    let stdout = child.stdout.take().expect("piped stdout");
    BufReader::new(stdout).read_line(&mut line).expect("line");
    let base = line.trim_end().strip_prefix("stand-ins listening on ");
    StandIn(child, format!("{}/stripe", base.expect("listening")))
}

/// The production Stripe webhook state from dev settings whose Stripe base URL is `base`
/// (`release_sink::stripe_webhook`, as `worker_registry`'s `stripe_client` reads it); its
/// `stripe` is the one client the worker shares.
fn client(base: &str) -> release_sink::StripeHooks {
    let key = |b: u8| format!("1:{}", STANDARD.encode([b; 32]));
    let stripe = [
        ("ASKHAVA_STRIPE_WEBHOOK_SECRET", HOOK_SECRET.to_owned()),
        ("ASKHAVA_STRIPE_SECRET_KEY", "sk_test_fake".to_owned()),
        ("ASKHAVA_STRIPE_BASE_URL", base.to_owned()),
        ("ASKHAVA_STRIPE_ACCOUNT_ID", "acct_testfake".to_owned()),
        ("VAULT_KEY", key(0x59)),
        ("VAULT_INDEX_KEY", key(0x69)),
    ];
    let pairs: Vec<(String, String)> = twilio_pairs()
        .into_iter()
        .chain(stripe.map(|(k, v)| (k.to_owned(), v)))
        .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let lazy = PgPool::connect_lazy("postgres://fake@127.0.0.1:1/none").expect("lazy pool");
    let hooks = release_sink::stripe_webhook(&settings, lazy, pairs).expect("stripe settings");
    hooks.expect("the Stripe client")
}

/// The stand-in's answers with `livemode: false`, which it leaves out (the webhook's re-fetch).
struct TestMode<'a, S>(&'a S);

impl<S: PaymentsAdapter + Sync> PaymentsAdapter for TestMode<'_, S> {
    async fn create_customer(&self, k: &IdempotencyKey, b: &str) -> Result<String, AdapterError> {
        self.0.create_customer(k, b).await
    }
    async fn create_payment_intent(
        &self,
        k: &IdempotencyKey,
        c: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        self.0.create_payment_intent(k, c).await
    }
    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        let mut intent = self.0.fetch_payment_intent(id).await?;
        intent.livemode = Some(false);
        Ok(intent)
    }
    async fn create_refund(&self, k: &IdempotencyKey, pi: &str) -> Result<String, AdapterError> {
        self.0.create_refund(k, pi).await
    }
}

/// The production registry from dev settings, with `stripe` registered as `worker_registry` does
/// when the Stripe settings are set (`None`: the dev registry as it is).
fn registry(clock: Arc<dyn Clock>, stripe: Option<release_sink::StripeHooks>) -> Registry {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t433-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("storage dir");
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev".to_owned()),
        (
            "ASKHAVA_DATABASE_URL",
            "postgres://fake@localhost/fake".into(),
        ),
        (
            "ASKHAVA_STORAGE_BASE_URL",
            format!("file://{}", dir.display()),
        ),
        ("ASKHAVA_ID_SECRET", ID_SECRET.into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let stream = offdb_stream::OffDbStream::from_settings(&settings, pairs.clone(), clock.clone())
        .expect("stream");
    let parts = (None, ConfirmSettings::default());
    let mut registry = worker_registry(&settings, stream, parts, pairs).expect("registry");
    if stripe.is_some() {
        let stripe = stripe.map(|h| h.stripe.clone());
        payment_jobs::register(&mut registry, stripe, secret(), clock);
    }
    registry
}

/// Runs every task due at the clock's now through `registry`, as the worker would, until none is.
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) {
    let owner = WorkerId::new(pool).await.expect("worker");
    loop {
        let lease = SignedDuration::from_secs(60);
        let tasks = claim(pool, owner, clock.now(), lease, 50)
            .await
            .expect("claim");
        if tasks.is_empty() {
            return;
        }
        for task in tasks {
            let ran = registry.run(pool, clock, owner, task, Lease::WORKER).await;
            ran.expect("run");
        }
    }
}

async fn queue(pool: &PgPool, kind: &'static str, payload: Value, when: Timestamp) {
    let mut conn = pool.acquire().await.expect("conn");
    askhava_core::api::enqueue(&mut conn, &NewTask::new(kind, payload, when))
        .await
        .expect("enqueue");
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

/// A $75 pick charged at once at the stand-in, its one attempt still `sent` with the four-hour
/// timer due a minute ago, as T062 writes them; the stand-in's intent ID.
async fn charged(pool: &PgPool, hooks: &release_sink::StripeHooks) -> String {
    let stripe = &hooks.stripe;
    let key = |l: &str| IdempotencyKey::derive(&secret(), "t433", &[l]);
    let customer = stripe.create_customer(&key("c"), "biz-fake-1").await;
    let customer = customer.expect("customer");
    let (pick, method) = (PICK.to_string(), "pm_card_visa");
    let charge = Charge {
        customer: &customer,
        payment_method: method,
        amount_cents: 7500,
        pick_id: &pick,
    };
    let intent = stripe.create_payment_intent(&key("i"), charge).await;
    let intent = intent.expect("intent").id;
    let sqls = [
        "INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($2, $5)",
        "INSERT INTO payments.pick_intent (pick_id, business_id, job_id, amount_cents,
           stripe_intent_id, emergency, created_at) VALUES ($1, $2, $3, 7500, $4, false,
           to_timestamp($6 - 18000))",
        "INSERT INTO payments.charge_attempt (pick_id, try_no, press_id, status, started_at)
           VALUES ($1, 1, $2, 'sent', to_timestamp($6 - 18000))",
        "INSERT INTO payments.pay_timer (pick_id, kind, business_clock, due_at)
           VALUES ($1, 'stripe-down', false, to_timestamp($6 - 60))",
    ];
    for sql in sqls {
        let q = sqlx::query(sql)
            .bind(PICK)
            .bind(BUSINESS)
            .bind(JOB)
            .bind(&intent);
        q.bind(&customer)
            .bind(NOW)
            .execute(pool)
            .await
            .expect("seed");
    }
    intent
}

/// FEE-018 through the worker: the four-hour timer over an unreachable Stripe ends the pick.
async fn four_hour_end(pool: &PgPool, clock: &Arc<ManualClock>) {
    let down = registry(clock.clone(), Some(client("http://127.0.0.1:1/stripe")));
    let timer = json!({"pick": PICK, "kind": "stripe-down", "due": NOW - 60});
    queue(pool, TIMER_KIND, timer, at(0)).await;
    run_due(pool, &down, clock).await;
    let sql = "SELECT count(*) FROM payments.pick_intent WHERE ended_at IS NOT NULL";
    assert_eq!(count(pool, sql).await, 1, "the timer ended the pick");
}

/// A signature-checked `payment_intent.succeeded` for `intent`.
fn succeeded(intent: &str) -> VerifiedStripeEvent {
    let body = json!({"id": "evt_FakeT433", "type": "payment_intent.succeeded", "livemode": false,
        "data": {"object": {"id": intent, "object": "payment_intent"}}})
    .to_string();
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(HOOK_SECRET.as_bytes()).expect("key");
    mac.update(format!("{NOW}.{body}").as_bytes());
    let sig: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let header = format!("t={NOW},v1={sig}");
    let headers = [("Stripe-Signature", header.as_str())];
    let url = "https://hooks.example.test/webhooks/stripe";
    let delivery = Delivery {
        url,
        body: body.as_bytes(),
        headers: &headers,
    };
    let hook_secret = SecretString::from(HOOK_SECRET.to_owned());
    let hook = verify_webhook(WebhookVendor::Stripe, &hook_secret, delivery, NOW).expect("signed");
    VerifiedStripeEvent::from_webhook(&hook).expect("event")
}

fn account() -> StripeAccount {
    StripeAccount {
        live: false,
        account: "acct_testfake".to_owned(),
    }
}

/// One succeeded `late-success` refund, one refund ledger row, one late-success alert, no release.
async fn assert_refunded_once(pool: &PgPool) {
    let refunds = "SELECT count(*) FROM payments.refund
                    WHERE trigger = 'late-success' AND status = 'succeeded'";
    assert_eq!(count(pool, refunds).await, 1, "one late-success refund");
    assert_eq!(count(pool, "SELECT count(*) FROM payments.refund").await, 1);
    let ledger = "SELECT count(*) FROM payments.ledger_entry WHERE kind = 'refund'";
    assert_eq!(count(pool, ledger).await, 1, "one refund ledger row");
    let alerts: i64 = sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1")
        .bind(LATE_SUCCESS_ALERT)
        .fetch_one(pool)
        .await
        .expect("alerts");
    assert_eq!(alerts, 1, "one alert");
    assert_eq!(count(pool, "SELECT count(*) FROM picks.release").await, 0);
}

/// TD-FEE-52 (FEE-018, FEE-019, FEE-021, FEE-026): the four-hour end, then the check round finds
/// the success; the late refund's ledger write fails once after the stand-in refunded (the worker
/// stopped there), and the retry finishes it with one refund and one ledger row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_52_check_round_late_success_survives_a_stop(pool: PgPool) {
    let server = stand_in();
    let clock = Arc::new(ManualClock::new(at(0)));
    charged(&pool, &client(&server.1)).await;
    four_hour_end(&pool, &clock).await;
    let stop = "CREATE FUNCTION payments.t433_stop() RETURNS trigger LANGUAGE plpgsql AS
        $$ BEGIN RAISE EXCEPTION 'worker stopped'; END $$;
        CREATE TRIGGER t433_stop BEFORE INSERT ON payments.ledger_entry FOR EACH ROW
        WHEN (NEW.kind = 'refund') EXECUTE FUNCTION payments.t433_stop();";
    sqlx::raw_sql(stop).execute(&pool).await.expect("stop");
    let worker = registry(clock.clone(), Some(client(&server.1)));
    queue(
        &pool,
        CHECK_KIND,
        json!({"pick": PICK, "try": 1, "round": 0}),
        at(60),
    )
    .await;
    clock.set(at(60));
    run_due(&pool, &worker, &clock).await;
    let ledger = "SELECT count(*) FROM payments.ledger_entry WHERE kind = 'refund'";
    assert_eq!(count(&pool, ledger).await, 0, "stopped before the write");
    sqlx::raw_sql("DROP TRIGGER t433_stop ON payments.ledger_entry")
        .execute(&pool)
        .await
        .expect("restart");
    clock.set(at(3 * 3_600));
    run_due(
        &pool,
        &registry(clock.clone(), Some(client(&server.1))),
        &clock,
    )
    .await;
    assert_refunded_once(&pool).await;
}

/// TD-FEE-52 (FEE-018, FEE-021): the four-hour end, then the webhook's success (twice, as Stripe
/// resends): nothing is cleared and the worker refunds once with one alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_52_webhook_late_success_refunds_once(pool: PgPool) {
    let server = stand_in();
    let (clock, stripe) = (Arc::new(ManualClock::new(at(0))), client(&server.1));
    let intent = charged(&pool, &stripe).await;
    let (mode, acct) = (TestMode(&stripe.stripe), account());
    four_hour_end(&pool, &clock).await;
    let worker = registry(clock.clone(), Some(client(&server.1)));
    for n in 1..=2 {
        clock.set(at(60 * n));
        let event = succeeded(&intent);
        let cleared = paid_release(&pool, &mode, &acct, &event, PICK, at(60 * n));
        assert_eq!(cleared.await.err(), Some(ReleaseError::PickEnded));
        run_due(&pool, &worker, &clock).await;
    }
    assert_refunded_once(&pool).await;
}

/// TD-FEE-52 (FEE-021, D-1547): a verified success for a pick `picks` moved on (no awaiting
/// confirm pick) settles `Left` in the release sink, which queues the late refund and alert; the
/// worker refunds once and nothing is released, also when the success arrives again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_52_picks_moved_on_success_is_refunded_unreleased(pool: PgPool) {
    let server = stand_in();
    let (clock, stripe) = (Arc::new(ManualClock::new(at(0))), client(&server.1));
    let intent = charged(&pool, &stripe).await;
    let (mode, acct) = (TestMode(&stripe.stripe), account());
    let sink = release_sink::PickReleaser::new(pool.clone(), vault(), masked(), clock.clone());
    let worker = registry(clock.clone(), Some(client(&server.1)));
    for n in 0..2 {
        let event = succeeded(&intent);
        let cleared = paid_release(&pool, &mode, &acct, &event, PICK, at(n));
        if let Ok(cleared) = cleared.await {
            let released = sink.settle_and_release(cleared).await;
            assert!(matches!(released, Ok(Released::MovedOn)));
        }
        run_due(&pool, &worker, &clock).await;
    }
    assert_refunded_once(&pool).await;
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x59), &key(0x69), &url).expect("vault")
}

fn twilio_pairs() -> Vec<(String, String)> {
    [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake-auth-token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}

fn masked() -> askhava_adapters::api::MaskedPool {
    let pairs = twilio_pairs();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    TwilioConfig::from_settings(&settings, pairs)
        .expect("pool")
        .pool()
        .clone()
}

/// TD-FEE-52 (FEE-029, JOB-006): two starts seed one reconcile; its run queues the next night
/// under the key a later start seeds, so a third start adds nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_52_two_starts_seed_one_reconcile(pool: PgPool) {
    let server = stand_in();
    let mut conn = pool.acquire().await.expect("conn");
    payment_jobs::schedule_reconcile(&mut conn, at(0))
        .await
        .expect("first start");
    payment_jobs::schedule_reconcile(&mut conn, at(3_600))
        .await
        .expect("restart");
    drop(conn);
    let sql = "SELECT count(*) FROM core.task WHERE kind = 'payments.reconcile'";
    assert_eq!(count(&pool, sql).await, 1, "one seeded run");
    let epoch = "SELECT extract(epoch FROM run_at)::bigint FROM core.task \
                 WHERE kind = 'payments.reconcile'";
    let due = Timestamp::from_second(count(&pool, epoch).await).expect("run_at");
    let clock = Arc::new(ManualClock::new(due));
    run_due(
        &pool,
        &registry(clock.clone(), Some(client(&server.1))),
        &clock,
    )
    .await;
    let done =
        "SELECT count(*) FROM core.task WHERE kind = 'payments.reconcile' AND done_at IS NOT NULL";
    assert_eq!(count(&pool, done).await, 1, "the reconcile ran");
    let mut conn = pool.acquire().await.expect("conn");
    let later = due + SignedDuration::from_mins(5);
    payment_jobs::schedule_reconcile(&mut conn, later)
        .await
        .expect("third start");
    assert_eq!(count(&pool, sql).await, 2, "the run's next night only");
}

/// TD-FEE-52 (D-3134): without a Stripe key (dev and CI) every payments kind is registered and
/// a run ends done, never dead-lettered.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_52_every_kind_is_registered_and_done_without_stripe(pool: PgPool) {
    let clock = Arc::new(ManualClock::new(at(0)));
    for kind in payment_jobs::KINDS {
        queue(&pool, kind, json!({"pick": PICK, "try": 1}), at(0)).await;
    }
    run_due(&pool, &registry(clock.clone(), None), &clock).await;
    let done =
        "SELECT count(*) FROM core.task WHERE kind LIKE 'payments.%' AND done_at IS NOT NULL";
    assert_eq!(count(&pool, done).await, 7);
}

/// TD-FEE-52 (FEE-019, FEE-026, D-1753): the check round stores a current pick's success with
/// its evidence and releases nothing itself; a refund still `pending` here that the stand-in
/// already made is moved on by `payments.refund_check` to `succeeded` with one ledger row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_fee_52_check_round_and_refund_check(pool: PgPool) {
    let server = stand_in();
    let (clock, stripe) = (Arc::new(ManualClock::new(at(0))), client(&server.1));
    let intent = charged(&pool, &stripe).await;
    let worker = registry(clock.clone(), Some(client(&server.1)));
    queue(
        &pool,
        CHECK_KIND,
        json!({"pick": PICK, "try": 1, "round": 0}),
        at(0),
    )
    .await;
    run_due(&pool, &worker, &clock).await;
    let evidence = "SELECT count(*) FROM payments.charge_evidence";
    assert_eq!(count(&pool, evidence).await, 1, "the success is stored");
    let key = IdempotencyKey::derive(&secret(), "t433", &["refund"]);
    let refund = stripe
        .stripe
        .create_refund(&key, &intent)
        .await
        .expect("refund");
    sqlx::query(
        "INSERT INTO payments.refund (pick_id, trigger, status, tries, stripe_refund_id,
           requested_at) VALUES ($1, 'unreachable', 'pending', 1, $2, to_timestamp($3))",
    )
    .bind(PICK)
    .bind(&refund)
    .bind(NOW)
    .execute(&pool)
    .await
    .expect("pending refund");
    queue(&pool, REFUND_CHECK_KIND, json!({"refund": refund}), at(0)).await;
    run_due(&pool, &worker, &clock).await;
    let done = "SELECT count(*) FROM payments.refund WHERE status = 'succeeded'";
    assert_eq!(count(&pool, done).await, 1, "the check moved it on");
    let ledger = "SELECT count(*) FROM payments.ledger_entry WHERE kind = 'refund'";
    assert_eq!(count(&pool, ledger).await, 1, "one refund ledger row");
}
