//! D-5836, D-5955 (T849; SEC-035, SEC-017, JOB-017): the web binary's clock composition. The
//! `*_with_clock` constructors carry the clock `main` takes once from `TestControl::clock()`, so
//! under an advance a signed inbound text is stored at the advanced instant, a released pick's
//! Connected `status_at` is the release instant (day 3's check-in is due from it) and the pick
//! settles once; the worker's session sweep schedules on `now()` but cuts off at `real_now()`, so
//! a live session is kept. The router composes with no webhook keys. Each case uses [`Ahead`], a
//! local clock whose `now()` is advanced and whose `real_now()` is system time, as the
//! test-control offset clock is (`ManualClock`'s `real_now()` equals its `now()`). Fake data only.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_businesses::api::{AccountStatus, add_owner};
use askhava_core::api::{
    Clock, Lease, MIGRATOR, NewTask, Settings, TestControl, WorkerId, claim, enqueue,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_jobs::api::{JobStatus, create, current, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor;
use askhava_payments::api::{EntryKind, LedgerEntry, NewEntry, ledger_append, no_fee_release};
use askhava_picks::api::{ConfirmSettings, PagePick, Released, page_pick};
use askhava_portals::api::{CookieJar, Realm, SESSION_SWEEP_KIND, start_session};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, parse_quote,
    send_cards, submit_quote,
};
use askhava_server::release_sink::PickReleaser;
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
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use tower::ServiceExt;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key-0849";

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
        (
            "ASKHAVA_DATABASE_URL",
            "postgres://fake@127.0.0.1:1/none".into(),
        ),
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
    let mut pairs = pairs(true);
    pairs.extend(
        [
            ("ASKHAVA_STRIPE_WEBHOOK_SECRET", "whsec_fake_value_0849"),
            ("ASKHAVA_STRIPE_SECRET_KEY", "sk_test_fake_value_0849"),
            ("ASKHAVA_STRIPE_ACCOUNT_ID", "acct_testfake"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned())),
    );
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://fake@127.0.0.1:1/none")
        .expect("pool");
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
    let (at, ahead) = (Timestamp::now(), Ahead::days(5));
    let vault = Vault::new(
        &SecretString::from(key(0x59)),
        &SecretString::from(key(0x69)),
        &SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned()),
    )
    .expect("vault");
    let mut db = pool.acquire().await.expect("db");
    let business = askhava_businesses::api::create(&mut db, false, at)
        .await
        .expect("business");
    add_owner(&mut db, business, Uuid::new_v4(), at)
        .await
        .expect("owner");
    drop(db);
    let job = quoted_job(&pool, business, at).await;
    her_request(&pool, &vault, job, at).await;
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, Actor::System, at).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = picked.expect("pick") else {
        panic!("no new pick")
    };
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
