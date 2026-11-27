//! D-5836 (T849; SEC-035, SEC-017, JOB-017): the web binary's composition on one test-control
//! clock. The router, the Twilio webhooks and the Stripe webhook are built with
//! `build_router_with_clock`, `webhooks_with_clock` and `stripe_webhook_with_clock` on [`Ahead`],
//! a clock whose `now()` is a fixed advanced instant and whose `real_now()` is system time, as the
//! test-control offset clock's is (`ManualClock`'s `real_now()` equals its `now()`). A signed text
//! is stored at the advanced instant; with no webhook keys the router still composes; a Stripe
//! event signed at real time is accepted and settles once, the job Connected at the release
//! instant; a business session issued at real time still opens Confirm and its charge shows in
//! this month's statement; the worker's sweep keeps a session live in real time. The Stripe calls
//! go to the stand-in (`python3 -m stand_ins.server`, started per test). Fake data only.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_businesses::api::{AccountStatus, Checks, Event, add_owner, apply};
use askhava_core::api::{
    Clock, Guarded, Lease, MIGRATOR, NewTask, Settings, SystemClock, WorkerId, claim, enqueue,
};
use askhava_jobs::api::{JobStatus, create, move_to, record_job_facts};
use askhava_licensing::api::{
    ClassMap, DailyCheck, Holder, Job, LicenseKind, arizona_day, check_license, import_roc_list,
};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    AUTHORIZATION_TEXT, BusinessId, STRIPE_WEBHOOK_PATH, accept_authorization, ensure_customer,
    fee_terms_in_force,
};
use askhava_picks::api::{ACCEPTED_QUOTE_KIND, ConfirmSettings, PagePick, page_pick};
use askhava_portals::api::{
    BUSINESS_SESSION_LIFE, CookieJar, PortalsError, Realm, SESSION_SWEEP_KIND, SIGNIN_LINK_ROUTE,
    SIGNIN_ROUTE, STATEMENT_ROUTE, SignIn, SignInAddress, SignInMail, csrf_token, current_session,
    ensure_pre_session, signin_post, signin_request, start_session,
};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, Submitted, parse_quote,
    send_cards, submit_quote,
};
use askhava_server::inbound_text::INBOUND_PATH;
use askhava_server::release_sink::stripe_webhook_with_clock;
use askhava_server::{
    RouterParts, build_router_with_clock, offdb_stream, webhooks_with_clock, worker_registry,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, header};
use axum::routing::post;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sha1::Sha1;
use sha2::Sha256;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

const BASE: &str = "https://askhava.example";
const HOST: &str = "askhava.example";
const TOKEN: &str = "fake_twilio_auth_token";
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key-t849";
const HOOK_SECRET: &str = "whsec_fake_t849_not_a_real_secret";

/// `now()` a fixed instant `days` ahead of system time (whole seconds, as stored), `real_now()`
/// system time: the test-control offset clock after an advance.
struct Ahead(Mutex<Timestamp>);

impl Ahead {
    fn by(days: i64) -> Arc<Self> {
        let at = Timestamp::now().as_second() + days * 86_400;
        Arc::new(Self(Mutex::new(Timestamp::from_second(at).expect("time"))))
    }

    /// Moves `now()` `days` further ahead, as `POST /__test/clock/advance` does.
    fn advance(&self, days: i64) {
        let mut at = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        *at += SignedDuration::from_hours(24 * days);
    }
}

impl Clock for Ahead {
    fn now(&self) -> Timestamp {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn real_now(&self) -> Timestamp {
        Timestamp::now()
    }
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

/// Dev settings with the public base URL, Twilio, the vault keys and `ID_SECRET`; with `stripe`
/// (the stand-in's base URL) the Stripe webhook's settings too. Fake values.
fn pairs(stripe: Option<&str>) -> Vec<(String, String)> {
    let key = |b: u8| format!("1:{}", STANDARD.encode([b; 32]));
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t849-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("storage dir");
    let mut pairs = vec![
        ("ASKHAVA_APP_ENV", "dev".to_owned()),
        (
            "ASKHAVA_DATABASE_URL",
            "postgres://fake@127.0.0.1:1/none".into(),
        ),
        ("ASKHAVA_PUBLIC_BASE_URL", BASE.into()),
        ("ASKHAVA_ID_SECRET", ID_SECRET.into()),
        ("ASKHAVA_TWILIO_ACCOUNT_SID", format!("AC{:032}", 0)),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", TOKEN.into()),
        ("ASKHAVA_TWILIO_VERIFY_SERVICE_SID", format!("VA{:032}", 1)),
        ("ASKHAVA_MASKED_POOL_SIZE", "1".into()),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100".into()),
        (
            "ASKHAVA_STORAGE_BASE_URL",
            format!("file://{}", dir.display()),
        ),
        ("VAULT_KEY", key(0x49)),
        ("VAULT_INDEX_KEY", key(0x4a)),
    ];
    if let Some(base) = stripe {
        pairs.extend([
            ("ASKHAVA_STRIPE_WEBHOOK_SECRET", HOOK_SECRET.to_owned()),
            ("ASKHAVA_STRIPE_SECRET_KEY", "sk_test_fake849".into()),
            ("ASKHAVA_STRIPE_BASE_URL", base.to_owned()),
            ("ASKHAVA_STRIPE_ACCOUNT_ID", "acct_testfake".into()),
        ]);
    }
    pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// The web binary's composition over `pool` on `clock`, with the Stripe stand-in at `stripe`.
fn app(pool: &PgPool, clock: &Arc<Ahead>, stripe: Option<&str>) -> Router {
    let pairs = pairs(stripe);
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let clock: Arc<dyn Clock> = Arc::clone(clock) as Arc<dyn Clock>;
    let hooks = webhooks_with_clock(&settings, pool.clone(), pairs.clone(), Arc::clone(&clock));
    let stripe = stripe_webhook_with_clock(&settings, pool.clone(), pairs, Arc::clone(&clock));
    let launch = Timestamp::now() - SignedDuration::from_hours(24 * 400);
    let hooks = (hooks.expect("hooks"), None, None, stripe.expect("stripe"));
    let parts = RouterParts::from(hooks).with_confirm_settings(
        "pk_test_fake849".to_owned(),
        launch,
        SecretString::from(ID_SECRET),
    );
    build_router_with_clock(pool.clone(), Router::new(), parts, clock)
}

/// A same-site request by `jar`'s person; a non-empty `body` is the form after the CSRF token.
fn request((method, path): (Method, &str), jar: &CookieJar, body: &str) -> Request<Body> {
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let csrf = csrf_token(jar).unwrap_or_default();
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "same-origin")
        .header(header::ORIGIN, BASE)
        .header(header::ACCEPT, "text/html")
        .body(Body::from(format!("csrf={csrf}{body}")))
        .expect("request");
    let peer = SocketAddr::from(([127, 0, 0, 1], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// The answer's status and body text.
async fn send(app: &Router, request: Request<Body>) -> (u16, String) {
    let response = app.clone().oneshot(request).await.expect("answer");
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1 << 22).await.expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// The text of column `v` of `sql`'s first row, empty when it has none (as `text_pick_route.rs`).
async fn value(pool: &PgPool, sql: String) -> String {
    sqlx::query_scalar!(
        r#"SELECT coalesce((xpath('/row/v/text()', query_to_xml($1::text, false, true, '')))[1]::text, '') AS "v!""#,
        sql
    )
    .fetch_one(pool)
    .await
    .expect("value")
}

/// The owner's one sign-in address; the link it "emails" is kept.
struct OneMail(SignInAddress, Mutex<Option<String>>);

impl SignInMail for OneMail {
    async fn addresses(
        &self,
        _: &mut sqlx::PgConnection,
        _: &str,
    ) -> Result<Vec<SignInAddress>, PortalsError> {
        Ok(vec![self.0])
    }

    async fn send_link(
        &self,
        _: &mut sqlx::PgConnection,
        _: SignInAddress,
        link: &SecretString,
    ) -> Result<(), PortalsError> {
        let token = link.expose_secret().rsplit('/').next().map(str::to_owned);
        *self.1.lock().unwrap_or_else(PoisonError::into_inner) = token;
        Ok(())
    }
}

/// `to`'s person signed in at real time through `portals`' emailed link (D-1724).
async fn signed_in(pool: &PgPool, to: SignInAddress) -> CookieJar {
    let mail = Arc::new(OneMail(to, Mutex::default()));
    let routes = Router::new()
        .route(SIGNIN_ROUTE, post(signin_request::<OneMail>))
        .route(SIGNIN_LINK_ROUTE, post(signin_post::<OneMail>));
    let app =
        SignIn::new(pool.clone(), Arc::new(SystemClock), Arc::clone(&mail)).into_router(routes);
    let jar = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let asked = request(
        (Method::POST, SIGNIN_ROUTE),
        &jar,
        "&email=person%40x.example",
    );
    app.clone().oneshot(asked).await.expect("asked");
    let token = mail
        .1
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let response = app
        .oneshot(request((Method::POST, &path), &jar, ""))
        .await
        .expect("in");
    let cookies: Vec<&str> = (response.headers().get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next())
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookies.join("; ").parse().expect("cookies"));
    CookieJar::from_headers(&headers)
}

/// An Active business with a fresh Active ROC check (PICK-020) and its owner:
/// `(business, mailbox)`.
async fn business(pool: &PgPool) -> (Uuid, Uuid) {
    let (now, mailbox) = (Timestamp::now(), Uuid::new_v4());
    let mut db = pool.acquire().await.expect("db");
    let business = askhava_businesses::api::create(&mut db, false, now)
        .await
        .expect("business");
    add_owner(&mut db, business, mailbox, now)
        .await
        .expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    apply(&mut db, business, Event::Checked(ok), Actor::System, now)
        .await
        .expect("active");
    let list = "license_number,class,status,issued_on\nROC900849,R-39,Active,2016-01-01\n";
    import_roc_list(&mut db, arizona_day(now), list)
        .await
        .expect("ROC list");
    let check = check_license(&mut db, business, "ROC900849").await;
    assert_eq!(check.expect("license check"), DailyCheck::Active);
    (business, mailbox)
}

/// The hvac card for the job and `business`, which takes it.
fn card(business: Uuid) -> (CardJob, CardBusiness) {
    let kind = Job {
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
    let card = CardJob {
        towns: vec![town],
        candidates: vec![kind],
        text: String::new(),
    };
    let to = CardBusiness {
        id: business,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([("hvac_repair".to_owned(), "AC not cooling".to_owned())]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    };
    (card, to)
}

/// A $325 firm quote's form and rules.
fn firm() -> (QuoteForm, QuoteRules) {
    let form = QuoteForm {
        price: Price::Firm(32_500),
        notes: "Parts and labor included.".to_owned(),
        original: "325 parts and labor".to_owned(),
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
    (form, rules)
}

/// An open hvac job with `business`'s $325 firm quote (a $25 fee, FEE-005) and her page pick,
/// all at `now`.
async fn picked(pool: &PgPool, business: Uuid, now: Timestamp) -> (Uuid, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let job = create(&mut db, now, false, Actor::System)
        .await
        .expect("job");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let types = ["AC not cooling".to_owned()];
    record_job_facts(&mut db, job, "hvac_repair", &types)
        .await
        .expect("facts");
    let open = move_to(
        &mut db,
        job,
        (JobStatus::Intake, JobStatus::Open),
        now,
        Actor::System,
    );
    assert_eq!(open.await.expect("open"), Guarded::Applied(()));
    let (card, to) = card(business);
    let map = ClassMap::shipped().expect("map");
    send_cards(&mut db, map, job, &card, &[to], now)
        .await
        .expect("cards");
    let (form, rules) = firm();
    let quote = parse_quote(form, &rules).expect("quote");
    let saved = submit_quote(&mut db, (job, business), &quote, now).await;
    assert_eq!(saved.expect("saved"), Submitted::Saved);
    drop(db);
    let mut tx = pool.begin().await.expect("tx");
    let made = page_pick(&mut tx, (job, false), 1, Actor::System, now).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = made.expect("pick") else {
        panic!("no pick")
    };
    (job, pick)
}

/// A business signed in at real time, its Stripe customer at the stand-in and the card
/// authorization accepted, then a pick of its quote pressed through Confirm on `app`:
/// `(business, job, pick, jar)`, all at real time, before any advance (the ROC check is stamped
/// by the database, so PICK-020's 24-hour freshness holds only then).
async fn confirmed(pool: &PgPool, app: &Router, stripe: &str) -> Confirmed {
    let (business, mailbox) = business(pool).await;
    let (job, pick) = picked(pool, business, Timestamp::now()).await;
    let settings = Settings::from_pairs(pairs(Some(stripe))).expect("settings");
    let lazy = PgPool::connect_lazy("postgres://fake@127.0.0.1:1/none").expect("lazy pool");
    let hooks =
        stripe_webhook_with_clock(&settings, lazy, pairs(Some(stripe)), Arc::new(SystemClock));
    let client = hooks.expect("settings").expect("hooks");
    let secret = SecretString::from(ID_SECRET);
    ensure_customer(pool, &client.stripe, &secret, BusinessId(business))
        .await
        .expect("customer");
    let mut db = pool.acquire().await.expect("db");
    let terms = fee_terms_in_force(&mut db).await.expect("terms");
    let now = Timestamp::now();
    let id = BusinessId(business);
    accept_authorization(&mut db, id, terms, AUTHORIZATION_TEXT, now)
        .await
        .expect("authorized");
    let jar = signed_in(pool, SignInAddress { business, mailbox }).await;
    let path = format!("/b/picks/{pick}/confirm");
    let (status, page) = send(app, request((Method::GET, &path), &jar, "")).await;
    assert!(
        status == 200 && page.contains("Confirm and pay $25"),
        "{status} {page}"
    );
    let press = format!("&press={}&payment_method=pm_card_visa", Uuid::new_v4());
    let (status, pressed) = send(app, request((Method::POST, &path), &jar, &press)).await;
    assert!(
        status == 200 && pressed.contains("\"waiting\""),
        "{pressed}"
    );
    (business, job, pick, jar)
}

/// A confirmed pick's business, job, pick and signed-in jar.
type Confirmed = (Uuid, Uuid, Uuid, CookieJar);

/// Stripe's `payment_intent.succeeded` for `pick`'s intent, signed at real time.
fn succeeded(intent: &str, pick: Uuid) -> Request<Body> {
    let body = format!(
        r#"{{"id":"evt_fake849","type":"payment_intent.succeeded","livemode":false,"data":{{"object":{{"id":"{intent}","metadata":{{"pick_id":"{pick}"}}}}}}}}"#
    );
    let t = Timestamp::now().as_second();
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(HOOK_SECRET.as_bytes()).expect("key");
    mac.update(format!("{t}.{body}").as_bytes());
    let sig: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut request = Request::post(STRIPE_WEBHOOK_PATH)
        .header("stripe-signature", format!("t={t},v1={sig}"))
        .body(Body::from(body))
        .expect("request");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 443))));
    request
}

/// SEC-016, D-5836: a signed inbound text after a 3-day advance is stored at the advanced instant.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_signed_text_after_an_advance_is_stored_at_the_advanced_instant(pool: PgPool) {
    let clock = Ahead::by(3);
    let app = app(&pool, &clock, None);
    let sid = format!("SM{:032}", 849);
    let mut fields = [
        ("MessageSid", sid.as_str()),
        ("From", "+19285550184"),
        ("To", "+19285550100"),
        ("Body", "hello"),
    ];
    let body: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{k}={}", v.replace('+', "%2B")))
        .collect();
    fields.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(format!("{BASE}{INBOUND_PATH}").as_bytes());
    for (name, value) in fields {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let request = Request::post(INBOUND_PATH)
        .header(
            "X-Twilio-Signature",
            STANDARD.encode(mac.finalize().into_bytes()),
        )
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let (status, _) = send(&app, request).await;
    assert_eq!(status, 200);
    let stored = format!(
        "SELECT extract(epoch FROM received_at)::bigint AS v FROM messaging.inbox \
         WHERE message_sid = '{sid}'"
    );
    let stored = value(&pool, stored).await;
    assert_eq!(stored, clock.now().as_second().to_string());
}

/// SEC-035, D-5836: with no webhook keys the clocked constructors build nothing and the router
/// still composes and serves.
#[sqlx::test(migrator = "MIGRATOR")]
async fn the_router_composes_with_no_webhook_keys(pool: PgPool) {
    let pairs: Vec<(String, String)> = pairs(None)
        .into_iter()
        .filter(|(k, _)| k != "ASKHAVA_PUBLIC_BASE_URL")
        .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let clock: Arc<dyn Clock> = Ahead::by(3);
    let hooks = webhooks_with_clock(&settings, pool.clone(), pairs.clone(), Arc::clone(&clock));
    let stripe = stripe_webhook_with_clock(&settings, pool.clone(), pairs, Arc::clone(&clock));
    let (hooks, stripe) = (hooks.expect("hooks"), stripe.expect("stripe"));
    assert!(hooks.is_none() && stripe.is_none());
    let app = build_router_with_clock(pool, Router::new(), (None, None, None, None), clock);
    let (status, _) = send(
        &app,
        request((Method::GET, "/nowhere"), &CookieJar::new(), ""),
    )
    .await;
    assert_eq!(status, 404);
}

/// SEC-017, PICK-022, D-5836: after a 5-day advance a Stripe event signed at real time is
/// accepted, twice, and settles the pick once; the job is Connected at the release instant.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_stripe_event_signed_at_real_time_settles_once_at_the_release_instant(pool: PgPool) {
    let stand_in = stand_in();
    let clock = Ahead::by(0);
    let app = app(&pool, &clock, Some(&stand_in.1));
    let (_, job, pick, _) = confirmed(&pool, &app, &stand_in.1).await;
    clock.advance(5);
    let sql =
        format!("SELECT stripe_intent_id AS v FROM payments.pick_intent WHERE pick_id = '{pick}'");
    let intent = value(&pool, sql).await;
    for _ in 0..2 {
        let (status, text) = send(&app, succeeded(&intent, pick)).await;
        assert_eq!(status, 200, "{text}");
    }
    let status = format!("SELECT status AS v FROM picks.pick WHERE id = '{pick}'");
    let status = value(&pool, status).await;
    assert_eq!(status, "paid");
    let connected = format!(
        "SELECT extract(epoch FROM status_at)::bigint AS v FROM jobs.job \
         WHERE id = '{job}' AND status = 'connected'"
    );
    let at = value(&pool, connected).await;
    assert_eq!(
        at,
        clock.now().as_second().to_string(),
        "Connected at the release instant"
    );
    let queued = format!(
        "SELECT string_agg(extract(epoch FROM run_at)::bigint::text, ',') AS v FROM core.task \
         WHERE kind = '{ACCEPTED_QUOTE_KIND}' AND payload->>'pick' = '{pick}'"
    );
    let runs = value(&pool, queued).await;
    assert_eq!(runs, at, "settled once, its email due from the release");
}

/// SEC-004, BIZ-015, FEE-030, D-5836: after a 100-day advance a session issued at real time
/// opens Confirm, the charge lands, and its ledger row is this month's statement, CSV and all.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_real_time_session_confirms_after_an_advance_and_the_statement_shows_it(pool: PgPool) {
    let stand_in = stand_in();
    let clock = Ahead::by(0);
    let app = app(&pool, &clock, Some(&stand_in.1));
    let (business, _, pick, jar) = confirmed(&pool, &app, &stand_in.1).await;
    clock.advance(100);
    let path = format!("/b/picks/{pick}/confirm");
    let (status, page) = send(&app, request((Method::GET, &path), &jar, "")).await;
    assert_eq!(status, 200, "the real-time session opens Confirm: {page}");
    let sql =
        format!("SELECT stripe_intent_id AS v FROM payments.pick_intent WHERE pick_id = '{pick}'");
    let (status, text) = send(&app, succeeded(&value(&pool, sql).await, pick)).await;
    assert_eq!(status, 200, "{text}");
    let rows = format!(
        "SELECT string_agg(amount_cents::text, ',') AS v FROM payments.ledger_entry \
         WHERE business_id = '{business}' AND kind = 'charge'"
    );
    assert_eq!(value(&pool, rows).await, "2500", "the charge landed");
    let (status, csv) = send(&app, request((Method::GET, STATEMENT_ROUTE), &jar, "")).await;
    assert_eq!(status, 200, "{csv}");
    let lines: Vec<&str> = csv.lines().skip(1).collect();
    assert!(lines.len() == 1 && lines[0].contains("25.00"), "{csv}");
}

/// SEC-004, D-5836: the worker's session sweep after a 100-day advance keeps a session issued
/// at real time, its cut-off on `real_now()`, and still schedules the next run on `now()`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn the_sweep_after_an_advance_keeps_a_real_time_session(pool: PgPool) {
    let clock = Ahead::by(100);
    let pairs = pairs(None);
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let dyn_clock: Arc<dyn Clock> = Arc::clone(&clock) as Arc<dyn Clock>;
    let stream = offdb_stream::OffDbStream::from_settings(&settings, pairs.clone(), dyn_clock)
        .expect("stream");
    let registry = worker_registry(&settings, stream, (None, ConfirmSettings::default()), pairs)
        .expect("registry");
    let mut db = pool.acquire().await.expect("db");
    let jar = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let login = (Realm::Business, Uuid::new_v4());
    let jar = start_session(&mut db, jar, login, BUSINESS_SESSION_LIFE, Timestamp::now()).await;
    let jar = jar.expect("session");
    let task = NewTask::new(SESSION_SWEEP_KIND, serde_json::json!({}), clock.now());
    enqueue(&mut db, &task).await.expect("queued");
    let owner = WorkerId::new(&pool).await.expect("worker");
    let lease = SignedDuration::from_secs(60);
    let claimed = claim(&pool, owner, clock.now(), lease, 10)
        .await
        .expect("claim");
    assert_eq!(claimed.len(), 1);
    for task in claimed {
        let ran = registry
            .run(&pool, clock.as_ref(), owner, task, Lease::WORKER)
            .await;
        ran.expect("ran");
    }
    let kept = current_session(&mut db, &jar, Timestamp::now())
        .await
        .expect("read");
    assert!(kept.is_some(), "a session live in real time is kept");
    let next = clock.now() + SignedDuration::from_hours(24);
    let due = format!(
        "SELECT count(*) AS v FROM core.task WHERE kind = '{SESSION_SWEEP_KIND}' \
         AND extract(epoch FROM run_at) >= {}",
        next.as_second()
    );
    let due = value(&pool, due).await;
    assert_eq!(due, "1", "the next sweep is scheduled from now()");
}
