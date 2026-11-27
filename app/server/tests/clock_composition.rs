//! D-5836, D-5955 (T849; SEC-035, SEC-017, JOB-017): the web binary's clock composition. The
//! `*_with_clock` constructors carry the clock `main` takes once from `TestControl::clock()`, so
//! under an advance a signed inbound text is stored at the advanced instant, a released pick's
//! Connected `status_at` is the release instant (day 3's check-in is due from it) and the pick
//! settles once; a Stripe success signed at real time is accepted after an advance and released
//! by the wired sink at the advanced instant, and a Confirm 100 days ahead shows once in the
//! statement CSV; the worker's session sweep schedules on `now()` but cuts off at `real_now()`, so
//! a live session is kept. The router composes with no webhook keys. Each case uses [`Ahead`], a
//! local clock whose `now()` is advanced and whose `real_now()` is system time, as the
//! test-control offset clock is (`ManualClock`'s `real_now()` equals its `now()`). Fake data only.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, AdapterMode, Charge, IdempotencyKey, IntentMetadata, MaskedPool, PaymentIntent,
    PaymentsAdapter, TwilioConfig,
};
use askhava_businesses::api::{AccountStatus, add_owner};
use askhava_core::api::{
    Clock, Lease, MIGRATOR, NewTask, Settings, TestControl, WorkerId, claim, enqueue,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_jobs::api::{JobStatus, create, current, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    BusinessId, ChargeOutcome, EntryKind, IntentStripe, LedgerEntry, NewEntry, PickNow, Press,
    STRIPE_WEBHOOK_PATH, StripeWebhook, business_statement, confirm_and_pay, ensure_customer,
    ledger_append, no_fee_release, stripe_webhook_routes,
};
use askhava_picks::api::{ConfirmSettings, PagePick, Released, page_pick};
use askhava_portals::api::statement_csv;
use askhava_portals::api::{CookieJar, Realm, SESSION_SWEEP_KIND, start_session};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, parse_quote,
    send_cards, submit_quote,
};
use askhava_server::release_sink::{PickReleaser, stripe_webhook_with_clock};
use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sha1::Sha1;
use sha2::Sha256;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use tower::ServiceExt;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key-0849";
const FAKE_DB: &str = "postgres://fake@127.0.0.1:1/none";
const WHSEC: &str = "whsec_fake_value_0849";
const INTENT: &str = "pi_fake849paid";

/// `now()` fixed `days` past the system time at creation (whole seconds, as the database keeps
/// them); `real_now()` the system time.
struct Ahead(Timestamp);

impl Ahead {
    fn days(days: i64) -> Arc<Self> {
        let at = Timestamp::from_second(Timestamp::now().as_second() + 86_400 * days);
        Arc::new(Self(at.expect("instant")))
    }
}

impl Clock for Ahead {
    fn now(&self) -> Timestamp {
        self.0
    }

    fn real_now(&self) -> Timestamp {
        Timestamp::now()
    }
}

fn key(byte: u8) -> String {
    format!("1:{}", STANDARD.encode([byte; 32]))
}

/// Dev settings with Twilio, the public base URL, `ID_SECRET`, the vault keys and storage.
fn pairs(with_hooks: bool) -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("t849");
    std::fs::create_dir_all(&dir).expect("storage dir");
    let mut all = vec![
        ("ASKHAVA_APP_ENV", "dev".to_owned()),
        ("ASKHAVA_DATABASE_URL", FAKE_DB.into()),
        ("ASKHAVA_ID_SECRET", ID_SECRET.into()),
        (
            "ASKHAVA_STORAGE_BASE_URL",
            format!("file://{}", dir.display()),
        ),
    ];
    if with_hooks {
        all.extend([
            ("ASKHAVA_PUBLIC_BASE_URL", BASE.to_owned()),
            ("ASKHAVA_TWILIO_ACCOUNT_SID", format!("AC{:032}", 0)),
            ("ASKHAVA_TWILIO_AUTH_TOKEN", TOKEN.into()),
            ("ASKHAVA_TWILIO_VERIFY_SERVICE_SID", format!("VA{:032}", 1)),
            ("ASKHAVA_MASKED_POOL_SIZE", "1".into()),
            ("ASKHAVA_MASKED_NUMBERS", "+15005550100".into()),
            ("VAULT_KEY", key(0x41)),
            ("VAULT_INDEX_KEY", key(0x42)),
        ]);
    }
    all.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// [`pairs`] with the hooks and the Stripe webhook secret, key and account.
fn stripe_pairs() -> Vec<(String, String)> {
    let mut pairs = pairs(true);
    pairs.extend(
        [
            ("ASKHAVA_STRIPE_WEBHOOK_SECRET", WHSEC),
            ("ASKHAVA_STRIPE_SECRET_KEY", "sk_test_fake_value_0849"),
            ("ASKHAVA_STRIPE_ACCOUNT_ID", "acct_testfake"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned())),
    );
    pairs
}

/// The router as `main` builds it on `clock`, with or without the Twilio webhooks.
fn router(pool: &PgPool, clock: &Arc<dyn Clock>, with_hooks: bool) -> Router {
    let pairs = pairs(with_hooks);
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let control = TestControl::from_settings(&settings, pairs.clone()).expect("control");
    let hooks = askhava_server::webhooks_with_clock(&settings, pool.clone(), pairs, clock.clone());
    let hooks = hooks.expect("webhooks");
    assert_eq!(hooks.is_some(), with_hooks);
    let parts = askhava_server::RouterParts::from((hooks, None, None, None));
    askhava_server::build_router_with_clock(pool.clone(), control.router(), parts, clock.clone())
}

/// One value of `sql` (a single `v` column) as text, through `query_to_xml` as
/// `text_pick_route.rs` does, so no SQL leaves the query macros.
async fn value(pool: &PgPool, sql: &str) -> String {
    sqlx::query_scalar!(
        r#"SELECT coalesce((xpath('/row/v/text()', query_to_xml($1::text, false, true, '')))[1]::text, '') AS "v!""#,
        sql
    )
    .fetch_one(pool)
    .await
    .expect("value")
}

/// A signed inbound text `words` from a fake homeowner number, through `app`.
async fn text(app: Router, words: &str) -> StatusCode {
    let fields = [
        ("Body", words),
        ("From", "+19285550142"),
        ("MessageSid", "SM00000000000000000000000000000849"),
        ("To", "+19285550100"),
    ];
    let path = "/webhooks/twilio/sms";
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(format!("{BASE}{path}").as_bytes());
    for (name, value) in fields {
        mac.update(format!("{name}{value}").as_bytes());
    }
    let body: Vec<String> = fields
        .iter()
        .map(|(n, v)| format!("{n}={}", v.replace('+', "%2B").replace(' ', "+")))
        .collect();
    let request = Request::post(path)
        .header(
            "X-Twilio-Signature",
            STANDARD.encode(mac.finalize().into_bytes()),
        )
        .header("Content-Type", "application/x-www-form-urlencoded")
        .extension(ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 9))))
        .body(Body::from(body.join("&")))
        .expect("request");
    app.oneshot(request).await.expect("answer").status()
}

/// D-5836, SEC-035: with no webhook keys the clocked router still composes and answers.
#[sqlx::test(migrator = "MIGRATOR")]
async fn the_router_composes_with_no_webhook_keys(pool: PgPool) {
    let clock: Arc<dyn Clock> = Ahead::days(3);
    let request = Request::get("/no-such-page-0849").body(Body::empty());
    let answer = router(&pool, &clock, false)
        .oneshot(request.expect("request"))
        .await;
    assert_eq!(answer.expect("answer").status(), StatusCode::NOT_FOUND);
}

/// D-5836, SEC-017: a signed inbound text after a 3-day advance is stored once, at the advanced
/// instant, never at system time.
#[sqlx::test(migrator = "MIGRATOR")]
async fn an_inbound_text_after_an_advance_is_stored_at_the_advanced_instant(pool: PgPool) {
    let ahead = Ahead::days(3);
    let clock: Arc<dyn Clock> = ahead.clone();
    for _ in 0..2 {
        assert_eq!(
            text(router(&pool, &clock, true), "my sink leaks").await,
            StatusCode::OK
        );
    }
    let sql = "SELECT count(*) || ':' || max(extract(epoch FROM received_at)::bigint) AS v \
               FROM messaging.inbox";
    let stored = value(&pool, sql).await;
    assert_eq!(stored, format!("1:{}", ahead.0.as_second()));
}

/// D-5836: the Twilio and Stripe hooks carry the clock they are built with.
#[tokio::test]
async fn the_hooks_carry_the_clock_they_are_given() {
    let ahead = Ahead::days(5);
    let clock: Arc<dyn Clock> = ahead.clone();
    let pairs = stripe_pairs();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let pool = PgPoolOptions::new().connect_lazy(FAKE_DB).expect("pool");
    let hooks =
        askhava_server::webhooks_with_clock(&settings, pool.clone(), pairs.clone(), clock.clone());
    assert_eq!(hooks.expect("ok").expect("set").clock.now(), ahead.0);
    let stripe = askhava_server::release_sink::stripe_webhook_with_clock(
        &settings,
        pool.clone(),
        pairs.clone(),
        clock,
    );
    assert_eq!(stripe.expect("ok").expect("set").clock.now(), ahead.0);
    let system = askhava_server::release_sink::stripe_webhook(&settings, pool, pairs);
    assert_ne!(system.expect("ok").expect("set").clock.now(), ahead.0);
}

/// SEC-004, D-5836: after a 100-day advance the worker's session sweep keeps a session issued
/// at real time (its cutoff is `real_now()`), and queues the next sweep from the advanced day.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_live_session_survives_the_sweep_after_an_advance(pool: PgPool) {
    let ahead = Ahead::days(100);
    let clock: Arc<dyn Clock> = ahead.clone();
    let mut db = pool.acquire().await.expect("db");
    let login = (Realm::Business, Uuid::new_v4());
    let life = SignedDuration::from_hours(2);
    let started = start_session(&mut db, CookieJar::new(), login, life, Timestamp::now()).await;
    assert!(started.is_ok(), "session");
    enqueue(
        &mut db,
        &NewTask::new(SESSION_SWEEP_KIND, serde_json::json!({}), ahead.0),
    )
    .await
    .expect("sweep");
    drop(db);
    let pairs = pairs(false);
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let stream = askhava_server::offdb_stream::OffDbStream::from_settings(
        &settings,
        pairs.clone(),
        clock.clone(),
    );
    let registry = askhava_server::worker_registry(
        &settings,
        stream.expect("stream"),
        (None, ConfirmSettings::default()),
        pairs,
    );
    let registry = registry.expect("registry");
    let owner = WorkerId::new(&pool).await.expect("worker");
    let lease = SignedDuration::from_secs(60);
    let tasks = claim(&pool, owner, ahead.0, lease, 5).await.expect("claim");
    assert_eq!(tasks.len(), 1);
    for task in tasks {
        let ran = registry
            .run(&pool, clock.as_ref(), owner, task, Lease::WORKER)
            .await;
        ran.expect("ran");
    }
    let kept = value(&pool, "SELECT count(*) AS v FROM portals.session").await;
    assert_eq!(kept, "1", "a live session is kept");
    let next = "SELECT count(*) AS v FROM core.task WHERE run_at > now() + interval '100 days'";
    assert_eq!(
        value(&pool, next).await,
        "1",
        "the next sweep is on the advanced day"
    );
}

/// The homeowner's vault contact and request naming `job`, through `privacy` and `intake`.
async fn her_request(pool: &PgPool, vault: &Vault, job: Uuid, at: Timestamp) {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        name: Some(SecretString::from("Dana Testperson")),
        phone: Some(SecretString::from("+19285550190")),
        email: None,
        address: Some(SecretString::from("100 Example Way, Lake Havasu City")),
        onsite_contact: None,
    };
    let contact = store_contact(&mut db, vault, &new).await.expect("contact");
    let inbound = Inbound {
        words: "my ac is out",
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let text = TextIn {
        inbox_id: Uuid::from_u128(0x849e01),
        contact,
        inbound,
        received_at: at,
    };
    let secret = SecretString::from(ID_SECRET.to_owned());
    let handled = receive_text(&mut db, &secret, &text, IntakeSettings::default()).await;
    let code = job.simple().to_string();
    let answer = Answer {
        conversation: handled.expect("request").conversation.expect("open"),
        question: "job.hvac.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::from_u128(0x849e10), at),
    };
    record_answer(&mut db, &answer).await.expect("job named");
}

/// An open AC repair job quoted by `business` at `at`.
async fn quoted_job(pool: &PgPool, business: Uuid, at: Timestamp) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let job = create(&mut db, at, false, Actor::System)
        .await
        .expect("job");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let types = ["AC not cooling".to_owned()];
    record_job_facts(&mut db, job, "hvac_repair", &types)
        .await
        .expect("facts");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(&mut db, job, open, at, Actor::System).await;
    assert!(moved.expect("open").applied().is_some());
    let map = ClassMap::shipped().expect("map");
    send_cards(&mut db, map, job, &card(), &[shop(business)], at)
        .await
        .expect("cards");
    submit_quote(&mut db, (job, business), &quote(), at)
        .await
        .expect("quoted");
    job
}

/// The job as matching reads it: AC repair in Lake Havasu City.
fn card() -> CardJob {
    let candidate = Job {
        category: "hvac_repair".to_owned(),
        job_type: "AC not cooling".to_owned(),
        license: LicenseKind::Roc,
        handyman_allowed: false,
        listed: true,
    };
    let town = OfferTown {
        code: "lake_havasu_city".to_owned(),
        state: "AZ".to_owned(),
    };
    CardJob {
        towns: vec![town],
        candidates: vec![candidate],
        text: String::new(),
    }
}

/// An active, verified R-39 business serving the town that ticked the job type.
fn shop(business: Uuid) -> CardBusiness {
    CardBusiness {
        id: business,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([("hvac_repair".to_owned(), "AC not cooling".to_owned())]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    }
}

/// A $325 firm quote.
fn quote() -> askhava_quotes::api::Quote {
    let form = QuoteForm {
        price: Price::Firm(32_500),
        notes: "Parts and labor included.".to_owned(),
        original: "325 parts and labor included".to_owned(),
        ai_read: false,
        earliest_visit: None,
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: true,
        license: LicenseKind::Roc,
        handyman: false,
        certified: false,
    };
    parse_quote(form, &rules).expect("quote")
}

/// PICK-022, JOB-017, D-5836: a $0 pick released on an advanced clock is Connected at the
/// release instant, not at system time, and a second clearing settles nothing again.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_released_pick_is_connected_at_the_release_instant(pool: PgPool) {
    let ahead = Ahead::days(5);
    let (business, job, pick) = picked(&pool).await;
    let vault = Vault::new(&key(0x41).into(), &key(0x42).into(), &FAKE_DB.into());
    let vault = vault.expect("vault");
    let mut db = pool.acquire().await.expect("db");
    let key = format!("t849.free.{job}");
    let entry = LedgerEntry::new(NewEntry {
        kind: EntryKind::FreeFirstWin,
        business: Some(business),
        job: Some(job),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: &key,
    });
    ledger_append(&mut db, &entry.expect("entry"))
        .await
        .expect("ledger");
    let sink = PickReleaser::new(pool.clone(), vault, masked(), ahead.clone());
    for once in [true, false] {
        let cleared = no_fee_release(&mut db, pick, (business, job))
            .await
            .expect("cleared");
        let released = sink.settle_and_release(cleared).await.expect("release");
        assert_eq!(matches!(released, Released::Shown(_)), once);
        let row = current(&mut db, job).await.expect("job");
        let connected = JobStatus::Connected.code();
        assert_eq!((row.status.code(), row.status_at), (connected, ahead.0));
    }
}

/// A masked pool of one fake number.
fn masked() -> MaskedPool {
    let pairs = pairs(true);
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    TwilioConfig::from_settings(&settings, pairs)
        .expect("pool")
        .pool()
        .clone()
}

/// A business, its $325 firm quote and her request, paged and picked at system time; the
/// business, job and pick. Her contact is sealed under the vault keys of [`pairs`].
async fn picked(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let at = Timestamp::now();
    let vault = Vault::new(&key(0x41).into(), &key(0x42).into(), &FAKE_DB.into());
    let mut db = pool.acquire().await.expect("db");
    let business = askhava_businesses::api::create(&mut db, false, at)
        .await
        .expect("business");
    add_owner(&mut db, business, Uuid::new_v4(), at)
        .await
        .expect("owner");
    drop(db);
    let job = quoted_job(pool, business, at).await;
    her_request(pool, &vault.expect("vault"), job, at).await;
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, Actor::System, at).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = picked.expect("pick") else {
        panic!("no new pick")
    };
    (business, job, pick)
}

/// The hooks `stripe_webhook_with_clock` builds on `clock`, its own `PickReleaser` kept and only
/// the Stripe client swapped for [`FakeStripe`] from this host, so the wired sink is the one run.
fn wired_hooks(pool: &PgPool, clock: Arc<dyn Clock>, pick: Uuid) -> FakeHooks {
    let pairs = stripe_pairs();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let built = stripe_webhook_with_clock(&settings, pool.clone(), pairs, clock);
    let Ok(hooks) = Arc::try_unwrap(built.expect("ok").expect("set")) else {
        panic!("one owner")
    };
    Arc::new(StripeWebhook {
        pool: hooks.pool,
        stripe: FakeStripe { pick },
        secret: hooks.secret,
        account: hooks.account,
        mode: AdapterMode::StandIn,
        stand_in_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        client_ip: hooks.client_ip,
        clock: hooks.clock,
        sink: hooks.sink,
    })
}

type FakeHooks = Arc<StripeWebhook<FakeStripe, PickReleaser>>;

/// The $25 Confirm-and-pay press at `at` with the fake Stripe (FEE-016).
async fn confirm(pool: &PgPool, (business, job, pick): (Uuid, Uuid, Uuid), at: Timestamp) {
    let stripe = FakeStripe { pick };
    let secret = SecretString::from(ID_SECRET);
    ensure_customer(pool, &stripe, &secret, BusinessId(business))
        .await
        .expect("customer");
    let press = Press {
        pick,
        business: BusinessId(business),
        job,
        amount_cents: 2500,
        payment_method: "pm_fake849",
        press: Uuid::new_v4(),
        emergency: false,
        now: PickNow::Payable,
    };
    let charged = confirm_and_pay(pool, &stripe, &secret, &press, at).await;
    assert!(matches!(
        charged.expect("charge"),
        ChargeOutcome::Succeeded(_)
    ));
}

/// One `payment_intent.succeeded` for `pick`, signed at system time, from the stand-in's host.
async fn deliver(hooks: FakeHooks, pick: Uuid) -> StatusCode {
    let body = format!(
        r#"{{"id":"evt_fake849","type":"payment_intent.succeeded","livemode":false,"data":{{"object":{{"id":"{INTENT}","metadata":{{"pick_id":"{pick}"}}}}}}}}"#
    );
    let signed = Timestamp::now().as_second();
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(WHSEC.as_bytes()).expect("key");
    mac.update(format!("{signed}.{body}").as_bytes());
    let sig: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut request = Request::post(STRIPE_WEBHOOK_PATH)
        .header("stripe-signature", format!("t={signed},v1={sig}"))
        .body(Body::from(body))
        .expect("request");
    let peer = SocketAddr::from(([127, 0, 0, 1], 443));
    request.extensions_mut().insert(ConnectInfo(peer));
    let routes = stripe_webhook_routes(hooks);
    routes.oneshot(request).await.expect("answer").status()
}

/// Confirmed at `confirm_at`, then the success delivered twice to the wired hooks on `ahead`:
/// both accepted (signature freshness on `real_now()`), the job Connected at `ahead`'s instant;
/// the business's statement CSV from system time to past `ahead`.
async fn paid_and_released(pool: &PgPool, ahead: &Arc<Ahead>, confirm_at: Timestamp) -> String {
    let (business, job, pick) = picked(pool).await;
    confirm(pool, (business, job, pick), confirm_at).await;
    let hooks = wired_hooks(pool, ahead.clone(), pick);
    for _ in 0..2 {
        assert_eq!(deliver(Arc::clone(&hooks), pick).await, StatusCode::OK);
    }
    let mut db = pool.acquire().await.expect("db");
    let row = current(&mut db, job).await.expect("job");
    let connected = JobStatus::Connected.code();
    assert_eq!((row.status.code(), row.status_at), (connected, ahead.0));
    let from = Timestamp::now() - SignedDuration::from_hours(24);
    let span = (from, ahead.0 + SignedDuration::from_hours(24));
    let statement = business_statement(pool, business, span).await;
    statement_csv(&statement.expect("statement"))
}

/// SEC-017, PICK-022, D-5836: a Stripe success signed at real time after a 5-day advance is
/// accepted by the hooks `stripe_webhook_with_clock` builds, settles once, and its sink releases
/// at the advanced instant.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_stripe_success_signed_at_real_time_after_an_advance_settles_once(pool: PgPool) {
    let ahead = Ahead::days(5);
    let csv = paid_and_released(&pool, &ahead, Timestamp::now()).await;
    assert_eq!(csv.lines().filter(|l| l.ends_with(",25.00")).count(), 1);
}

/// FEE-016, BIZ-015, D-5836: Confirm at the instant 100 days ahead, then its success, charges
/// once and the statement CSV holds that one $25 charge.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_confirm_after_a_100_day_advance_shows_once_in_the_statement_csv(pool: PgPool) {
    let ahead = Ahead::days(100);
    let csv = paid_and_released(&pool, &ahead, ahead.0).await;
    assert_eq!(csv.lines().filter(|l| l.ends_with(",25.00")).count(), 1);
    assert!(csv.starts_with("date,entry,job,amount\n"));
}

/// A Stripe fake whose intents all succeed for `pick` at $25.
struct FakeStripe {
    pick: Uuid,
}

impl FakeStripe {
    fn succeeded(&self, id: &str) -> PaymentIntent {
        PaymentIntent {
            id: id.to_owned(),
            status: "succeeded".to_owned(),
            amount: 2500,
            currency: "usd".to_owned(),
            customer: "cus_fake849".to_owned(),
            metadata: IntentMetadata {
                pick_id: self.pick.to_string(),
            },
            livemode: Some(false),
            latest_charge: Some(id.replacen("pi_", "ch_", 1)),
        }
    }
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok("cus_fake849".to_owned())
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        Ok(self.succeeded(id))
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
        Ok(INTENT.to_owned())
    }

    async fn confirm_intent(
        &self,
        _: &IdempotencyKey,
        intent: &str,
        _: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        Ok(self.succeeded(intent))
    }
}
