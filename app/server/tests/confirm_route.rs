//! The served Confirm route under the real middleware (T261; TD-FEE-45, TD-SEC-37, TD-FEE-58 and
//! the served halves of TD-PICK-43, TD-TERMS-11 and TD-SEC-32; FEE-014, FEE-016, FEE-036,
//! FEE-041, FEE-045, SEC-012, CON-029, PICK-024, BIZ-037; D-1871, D-3722, D-3813, D-3717).
//!
//! `build_router` with the Stripe hooks and no `with_confirm_settings` part serves the page with
//! "Card payments can't be taken right now.", opens no PaymentIntent and charges nothing; only
//! the page carries the Stripe CSP. The charging cases serve `portals::api::confirm_routes` with a
//! fake Stripe that counts its calls and the production `PickReleaser`, under the real
//! `security_headers` layer. Every name, phone, key and ID is fake.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{
    AdapterError, AdapterMode, Charge, IdempotencyKey, IntentMetadata, MaskedPool, PaymentIntent,
    PaymentsAdapter, StripePayments, TwilioConfig,
};
use askhava_businesses::api::{
    AccountStatus, Acting, ConfirmTerms, SignUpForm, add_owner, confirm_assent_line,
    record_confirm_terms, record_step_up, sign_up,
};
use askhava_core::api::{
    Claimed, ClientIpPolicy, Guarded, MIGRATOR, ManualClock, Settings, SystemClock, WorkerId, claim,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, on_opt_out, receive_text, record_answer};
use askhava_jobs::api::{JobStatus, TEXT_KIND, create, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor;
use askhava_payments::api::{
    AUTHORIZATION_TEXT, BusinessId, FeeJob, FeeMode, FeeQuote, IntentStripe, PeriodJob,
    RecurringCap, Service, StripeAccount, StripeWebhook, VisitTier, accept_authorization,
    ensure_customer, fee_at_pick, fee_terms_in_force, set_fee_limit, switch_fee_mode,
};
use askhava_picks::api::{BUSINESS_NOTICE_KIND, PagePick, page_pick};
use askhava_portals::api::{
    ApprovalSecret, CARD_OFF_LINE, CONFIRM_CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY,
    CodeTo, ConfirmFee, ConfirmRoute, ConfirmSettings, ConfirmedPick, CookieJar, FEE_TERMS_LINE,
    PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SecretAnswer, SignIn, SignInAddress, SignInMail,
    business_principal, confirm_routes, csrf_token, decide_at_confirm, ensure_pre_session,
    security_headers, set_inbox_shared, signin_post, signin_request,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, Submitted, parse_quote,
    send_cards, submit_quote,
};
use askhava_server::release_sink::PickReleaser;
use askhava_server::{RouterParts, build_router, confirm_settings};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, HeaderValue, Method, Request, header};
use axum::middleware::from_fn;
use axum::routing::post;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key-t261";
const CLIENT_SECRET: &str = "pi_fake261_secret_fake";

fn now() -> Timestamp {
    Timestamp::now()
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x61), &key(0x62), &url).expect("vault")
}

fn masked() -> MaskedPool {
    let pairs: Vec<(String, String)> = [
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
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let config = TwilioConfig::from_settings(&settings, pairs).expect("pool");
    config.pool().clone()
}

/// A Stripe that counts PaymentIntent creates and confirms; `approve` answers `requires_action`.
#[derive(Default)]
struct FakeStripe {
    creates: AtomicUsize,
    confirms: AtomicUsize,
    approve: AtomicBool,
}

fn intent(id: &str, state: &str) -> PaymentIntent {
    PaymentIntent {
        id: id.to_owned(),
        status: state.to_owned(),
        amount: 2500,
        currency: "usd".to_owned(),
        customer: "cus_fake261".to_owned(),
        metadata: IntentMetadata {
            pick_id: String::new(),
        },
        livemode: Some(false),
        latest_charge: (state == "succeeded").then(|| "ch_fake261".to_owned()),
    }
}

impl PaymentsAdapter for FakeStripe {
    async fn create_customer(&self, _: &IdempotencyKey, _: &str) -> Result<String, AdapterError> {
        Ok(format!("cus_fake{}", Uuid::new_v4().simple()))
    }

    async fn create_payment_intent(
        &self,
        _: &IdempotencyKey,
        _: Charge<'_>,
    ) -> Result<PaymentIntent, AdapterError> {
        Err(AdapterError::NotSent)
    }

    async fn fetch_payment_intent(&self, id: &str) -> Result<PaymentIntent, AdapterError> {
        Ok(intent(id, "succeeded"))
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
        pick: &str,
    ) -> Result<String, AdapterError> {
        self.creates.fetch_add(1, Ordering::SeqCst);
        Ok(format!("pi_fake{}", pick.replace('-', "")))
    }

    async fn confirm_intent(
        &self,
        _: &IdempotencyKey,
        id: &str,
        _: &str,
    ) -> Result<PaymentIntent, AdapterError> {
        self.confirms.fetch_add(1, Ordering::SeqCst);
        let approve = self.approve.load(Ordering::SeqCst);
        Ok(intent(
            id,
            if approve {
                "requires_action"
            } else {
                "succeeded"
            },
        ))
    }
}

/// The fake bank approval's client secret.
struct FakeApproval;

impl ApprovalSecret for FakeApproval {
    fn approval_secret<'a>(
        &'a self,
        _: &'a IdempotencyKey,
        _: &'a str,
        _: (u32, &'a str),
    ) -> SecretAnswer<'a> {
        Box::pin(async { Some(CLIENT_SECRET.to_owned()) })
    }
}

fn hooks<S>(pool: &PgPool, stripe: S) -> Arc<StripeWebhook<S, PickReleaser>> {
    let none: [(String, String); 0] = [];
    let clock = Arc::new(ManualClock::new(now()));
    Arc::new(StripeWebhook {
        pool: pool.clone(),
        stripe,
        secret: SecretString::from("whsec_fake_t261"),
        account: StripeAccount {
            live: false,
            account: "acct_fake261".to_owned(),
        },
        mode: AdapterMode::StandIn,
        stand_in_host: IpAddr::V4(Ipv4Addr::LOCALHOST),
        client_ip: ClientIpPolicy::from_pairs(none).expect("policy"),
        clock: clock.clone(),
        sink: PickReleaser::new(pool.clone(), vault(), masked(), clock),
    })
}

/// A Stripe whose calls the test reads after the router took its own.
struct Counted {
    app: Router,
    hooks: Arc<StripeWebhook<FakeStripe, PickReleaser>>,
}

fn counted(pool: &PgPool, launch: Option<Timestamp>) -> Counted {
    let hooks = hooks(pool, FakeStripe::default());
    let settings = launch.map(|launch| ConfirmSettings {
        publishable_key: "pk_test_fake261".to_owned(),
        launch,
        secret: SecretString::from(ID_SECRET),
    });
    let route = ConfirmRoute {
        hooks: Arc::clone(&hooks),
        settings,
        approval: Arc::new(FakeApproval),
    };
    let app = confirm_routes(route).layer(from_fn(security_headers));
    Counted { app, hooks }
}

impl Counted {
    fn creates(&self) -> usize {
        self.hooks.stripe.creates.load(Ordering::SeqCst)
    }

    fn confirms(&self) -> usize {
        self.hooks.stripe.confirms.load(Ordering::SeqCst)
    }
}

struct Reply {
    status: u16,
    csp: String,
    no_store: bool,
    vary: bool,
    text: String,
}

/// A same-site request by `jar`'s person; `body` "json" is the JSON state `GET`, else the form
/// fields after the CSRF token.
fn request((method, path): (&Method, &str), jar: &CookieJar, body: &str) -> Request<Body> {
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let json = body == "json";
    let accept = if json {
        "application/json"
    } else {
        "text/html"
    };
    let csrf = csrf_token(jar).unwrap_or_default();
    let form = if json {
        String::new()
    } else {
        format!("csrf={csrf}{body}")
    };
    let mut request = Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "same-origin")
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header(header::ACCEPT, accept)
        .body(Body::from(form))
        .expect("request");
    let peer = SocketAddr::from(([203, 0, 113, 9], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

async fn send(app: &Router, (method, path): (&Method, &str), jar: &CookieJar, body: &str) -> Reply {
    let request = request((method, path), jar, body);
    let response = app.clone().oneshot(request).await.expect("answer");
    let (parts, body) = response.into_parts();
    let get = |name| {
        parts
            .headers
            .get(name)
            .and_then(|v: &HeaderValue| v.to_str().ok())
    };
    let csp = get(header::CONTENT_SECURITY_POLICY)
        .unwrap_or_default()
        .to_owned();
    let no_store = get(header::CACHE_CONTROL).is_some_and(|v| v.contains("no-store"));
    let vary = get(header::VARY).is_some_and(|v| v.contains("accept"));
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Reply {
        status: parts.status.as_u16(),
        csp,
        no_store,
        vary,
        text,
    }
}

/// The owner's one sign-in address; the link it "emails" is kept.
struct OneMail {
    to: SignInAddress,
    sent: Mutex<Option<String>>,
}

impl SignInMail for OneMail {
    async fn addresses(
        &self,
        _: &mut PgConnection,
        _: &str,
    ) -> Result<Vec<SignInAddress>, PortalsError> {
        Ok(vec![self.to])
    }

    async fn send_link(
        &self,
        _: &mut PgConnection,
        _: SignInAddress,
        link: &SecretString,
    ) -> Result<(), PortalsError> {
        let token = link.expose_secret().rsplit('/').next().map(str::to_owned);
        *self.sent.lock().unwrap_or_else(PoisonError::into_inner) = token;
        Ok(())
    }
}

/// `to`'s person signed in through `portals`' emailed link (D-1724).
async fn signed_in(pool: &PgPool, to: SignInAddress) -> CookieJar {
    let mail = Arc::new(OneMail {
        to,
        sent: Mutex::default(),
    });
    let routes = Router::new()
        .route(SIGNIN_ROUTE, post(signin_request::<OneMail>))
        .route(SIGNIN_LINK_ROUTE, post(signin_post::<OneMail>));
    let app =
        SignIn::new(pool.clone(), Arc::new(SystemClock), Arc::clone(&mail)).into_router(routes);
    let jar = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let asked = request(
        (&Method::POST, SIGNIN_ROUTE),
        &jar,
        "&email=person%40x.example",
    );
    app.clone().oneshot(asked).await.expect("asked");
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let pressed = request((&Method::POST, &path), &jar, "");
    let response = app.oneshot(pressed).await.expect("signed in");
    let cookies: Vec<&str> = (response.headers().get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next())
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let mut headers = HeaderMap::new();
    let joined = cookies.join("; ").parse().expect("cookies");
    headers.insert(header::COOKIE, joined);
    CookieJar::from_headers(&headers)
}

/// Her contact and request naming `job` (fake data); returns her contact.
async fn her_request(pool: &PgPool, job: Uuid) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let new = NewContact {
        name: Some(SecretString::from("Dana Testperson")),
        phone: Some(SecretString::from(format!(
            "+1928555{:04}",
            Uuid::new_v4().as_u128() % 10_000
        ))),
        email: None,
        address: Some(SecretString::from("100 Example Way, Lake Havasu City")),
        onsite_contact: None,
    };
    let contact = store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact");
    let inbound = Inbound {
        words: "my ac is out",
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let text = TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound,
        received_at: now(),
    };
    let secret = SecretString::from(ID_SECRET);
    let handled = receive_text(&mut db, &secret, &text, IntakeSettings::default()).await;
    let conversation = handled.expect("request").conversation.expect("open");
    let code = job.simple().to_string();
    let answer = Answer {
        conversation,
        question: "job.hvac.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::new_v4(), now()),
    };
    record_answer(&mut db, &answer).await.expect("job named");
    contact
}

fn card(business: Uuid) -> (CardJob, CardBusiness) {
    let job = Job {
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
    let job = CardJob {
        towns: vec![town],
        candidates: vec![job],
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
    (job, to)
}

/// A $325 firm quote, so the fee fixed at the pick is $25 (FEE-005).
async fn quoted(pool: &PgPool, business: Uuid) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let job = create(&mut db, now(), false, Actor::System)
        .await
        .expect("job");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let types = ["AC not cooling".to_owned()];
    record_job_facts(&mut db, job, "hvac_repair", &types)
        .await
        .expect("facts");
    let moved = move_to(
        &mut db,
        job,
        (JobStatus::Intake, JobStatus::Open),
        now(),
        Actor::System,
    );
    assert_eq!(moved.await.expect("open"), Guarded::Applied(()));
    let (to_job, to) = card(business);
    let map = ClassMap::shipped().expect("map");
    send_cards(&mut db, map, job, &to_job, &[to], now())
        .await
        .expect("cards");
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
    let quote = parse_quote(form, &rules).expect("quote");
    let saved = submit_quote(&mut db, (job, business), &quote, now()).await;
    assert_eq!(saved.expect("saved"), Submitted::Saved);
    job
}

/// A business, its owner signed in, a quoted job and her page pick, with the card authorization
/// accepted: `(business, owner, job, pick, jar)`.
struct Picked {
    business: Uuid,
    owner: Uuid,
    job: Uuid,
    pick: Uuid,
    contact: Uuid,
    jar: CookieJar,
}

impl Picked {
    fn path(&self) -> String {
        format!("/b/picks/{}/confirm", self.pick)
    }
}

/// A business; with `signup`, signed up for hvac repair by `mailbox` (its join date, FEE-010).
async fn new_business(
    pool: &PgPool,
    signup: bool,
    (secret, mailbox): (&SecretString, Uuid),
) -> Uuid {
    if signup {
        let form = SignUpForm {
            categories: vec!["hvac_repair".to_owned()],
            mailbox_id: Some(mailbox),
            heard_from: None,
            g_code: None,
        };
        let map = ClassMap::shipped().expect("map");
        let stripe = (&FakeStripe::default(), secret);
        let up = sign_up(pool, stripe, map, &form, now()).await;
        up.expect("signed up").business
    } else {
        let mut db = pool.acquire().await.expect("db");
        let made = askhava_businesses::api::create(&mut db, false, now()).await;
        made.expect("business")
    }
}

/// [`Picked`]; with `signup`, the business signed up for hvac repair first (its join date).
async fn picked(pool: &PgPool, signup: bool) -> Picked {
    let (secret, mailbox) = (SecretString::from(ID_SECRET), Uuid::new_v4());
    let business = new_business(pool, signup, (&secret, mailbox)).await;
    let mut db = pool.acquire().await.expect("db");
    let owner = add_owner(&mut db, business, mailbox, now()).await;
    let owner = owner.expect("owner");
    drop(db);
    let job = quoted(pool, business).await;
    let contact = her_request(pool, job).await;
    let mut tx = pool.begin().await.expect("tx");
    let made = page_pick(&mut tx, (job, false), 1, Actor::System, now()).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = made.expect("pick") else {
        panic!("no pick")
    };
    ensure_customer(pool, &FakeStripe::default(), &secret, BusinessId(business))
        .await
        .expect("customer");
    let mut db = pool.acquire().await.expect("db");
    let terms = fee_terms_in_force(&mut db).await.expect("terms");
    let accepted = accept_authorization(
        &mut db,
        BusinessId(business),
        terms,
        AUTHORIZATION_TEXT,
        now(),
    );
    accepted.await.expect("authorization");
    let jar = signed_in(pool, SignInAddress { business, mailbox }).await;
    Picked {
        business,
        owner,
        job,
        pick,
        contact,
        jar,
    }
}

/// Whether `p`'s pick has its business-terms row: a second store in a rolled-back transaction
/// stores nothing exactly then (TERMS-033).
async fn has_terms(pool: &PgPool, p: &Picked) -> bool {
    let mut tx = pool.begin().await.expect("tx");
    let line = confirm_assent_line(1);
    let terms = ConfirmTerms {
        version: 1,
        published_text: &line,
        language: "en",
    };
    let people = (p.business, p.owner);
    let stored = record_confirm_terms(&mut tx, people, p.pick, terms, now()).await;
    tx.rollback().await.expect("rollback");
    !stored.expect("terms")
}

/// Every queued task, claimed once.
async fn tasks(pool: &PgPool) -> Vec<Claimed> {
    let owner = WorkerId::new(pool).await.expect("worker");
    let later = now() + SignedDuration::from_hours(24 * 30);
    let claimed = claim(pool, owner, later, SignedDuration::from_secs(60), 1000).await;
    claimed.expect("claim")
}

/// The tasks of `kind` naming `pick` in their payload.
fn naming(tasks: &[Claimed], kind: &str, pick: Uuid) -> usize {
    let pick = pick.to_string();
    let named = |t: &&Claimed| t.kind == kind && t.payload.to_string().contains(&pick);
    tasks.iter().filter(named).count()
}

/// Her sole-word CANCEL: the job is paused and its waiting pick withdrawn (CON-029).
async fn pause(pool: &PgPool, p: &Picked) {
    let mut db = pool.acquire().await.expect("db");
    let inbound = Inbound {
        words: "CANCEL",
        opt_out: true,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let text = TextIn {
        inbox_id: Uuid::new_v4(),
        contact: p.contact,
        inbound,
        received_at: now(),
    };
    on_opt_out(&mut db, &text).await.expect("paused");
}

/// The waiver committed with no release after it, as a press that crashed between the two.
async fn crashed_after_the_waiver(pool: &PgPool, p: &Picked, launch: Timestamp) {
    let job = PeriodJob {
        job: FeeJob {
            diagnostic_visit: false,
            visit_tier: VisitTier::DEFAULT,
            fee_free: false,
        },
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    };
    let fee = fee_at_pick(FeeQuote::Fixed(32_500), None, job, FeeMode::Tiers, None);
    let fee = fee.expect("fee");
    let confirmed = ConfirmedPick {
        pick: p.pick,
        business: BusinessId(p.business),
        job: p.job,
        picked: &fee,
        same_customer_jobs: &[],
    };
    let mut tx = pool.begin().await.expect("tx");
    let secret = SecretString::from(ID_SECRET);
    let decided = decide_at_confirm(&mut tx, &secret, confirmed, Some(launch)).await;
    assert!(matches!(decided, Ok(ConfirmFee::NoFee(_))));
    tx.commit().await.expect("waiver committed");
}

/// `line` as the page's template escapes it.
fn html(line: &str) -> String {
    line.replace('\'', "&#39;")
}

fn press(card: &str) -> String {
    format!("&press={}&payment_method={card}", Uuid::new_v4())
}

/// TD-SEC-37 (served), D-3813: without the settings part the page answers 200 with the card-off
/// line, opens no PaymentIntent, and a press charges nothing; only the page gets the Stripe CSP.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_37_only_the_confirm_page_adds_stripe_and_no_part_charges_nothing(pool: PgPool) {
    let p = picked(&pool, false).await;
    let stripe = StripePayments::new("http://127.0.0.1:1", SecretString::from("sk_test_fake261"));
    let hooks = (
        None,
        None,
        None,
        Some(hooks(&pool, stripe.expect("client"))),
    );
    let app = build_router(pool.clone(), Router::new(), RouterParts::from(hooks));
    let page = send(&app, (&Method::GET, &p.path()), &p.jar, "").await;
    assert_eq!(
        (page.status, page.csp.as_str()),
        (200, CONFIRM_CONTENT_SECURITY_POLICY)
    );
    assert!(page.no_store && page.vary && page.text.contains(&html(CARD_OFF_LINE)));
    assert!(!page.text.contains("js.stripe.com") && !page.text.contains("card-element"));
    assert!(
        page.text
            .contains(&format!("/b/picks/{}/cant-take", p.pick)),
        "TD-PICK-43"
    );
    for directive in ["script-src", "frame-src"] {
        let found = page
            .csp
            .split(';')
            .find(|d| d.trim().starts_with(directive));
        assert!(found.expect(directive).contains("https://js.stripe.com"));
    }
    assert!(!page.csp.contains("unsafe-inline") && !page.csp.contains("unsafe-eval"));
    let pressed = send(
        &app,
        (&Method::POST, &p.path()),
        &p.jar,
        &press("pm_card_visa"),
    )
    .await;
    assert!(pressed.text.contains("\"try_later\"") && pressed.text.contains("Nothing was charged"));
    let bare = counted(&pool, None);
    let page = send(&bare.app, (&Method::GET, &p.path()), &p.jar, "").await;
    let pressed = send(
        &bare.app,
        (&Method::POST, &p.path()),
        &p.jar,
        &press("pm_x"),
    )
    .await;
    assert!(page.text.contains(&html(CARD_OFF_LINE)) && pressed.text.contains("\"try_later\""));
    assert_eq!((bare.creates(), bare.confirms()), (0, 0), "no Stripe call");
    let others = [
        format!("/b/picks/{}/confirm", Uuid::new_v4()),
        "/b".to_owned(),
        "/nowhere".to_owned(),
    ];
    for path in others {
        let other = send(&app, (&Method::GET, &path), &p.jar, "").await;
        assert_eq!(other.csp, CONTENT_SECURITY_POLICY, "{path}");
    }
    let json = send(&app, (&Method::GET, &p.path()), &p.jar, "json").await;
    assert_eq!(json.csp, CONTENT_SECURITY_POLICY);
    let signed_out = send(&app, (&Method::GET, &p.path()), &CookieJar::new(), "").await;
    assert_eq!(
        (signed_out.status, signed_out.csp.as_str()),
        (401, CONTENT_SECURITY_POLICY)
    );
    let own = Router::new()
        .route(
            "/own",
            axum::routing::get(|| async {
                ([(header::CONTENT_SECURITY_POLICY, "img-src *")], "own")
            }),
        )
        .layer(from_fn(security_headers));
    assert_eq!(
        send(&own, (&Method::GET, "/own"), &p.jar, "").await.csp,
        CONTENT_SECURITY_POLICY
    );
}

/// TD-FEE-45, TD-TERMS-11 (served): the first load opens the pick's one PaymentIntent; the JSON
/// state opens nothing, stores nothing and holds no client secret; a press charges exactly the
/// stored $25 and stores one terms row; a lost answer pressed again is one charge; a bank
/// approval alone carries the client secret.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_fee_45_a_press_charges_the_stored_fee_once(pool: PgPool) {
    let p = picked(&pool, false).await;
    let c = counted(&pool, Some(now() - SignedDuration::from_hours(24 * 400)));
    let page = send(&c.app, (&Method::GET, &p.path()), &p.jar, "").await;
    send(&c.app, (&Method::GET, &p.path()), &p.jar, "").await;
    assert!(page.status == 200 && page.text.contains("Confirm and pay $25"));
    assert_eq!((c.creates(), has_terms(&pool, &p).await), (1, false));
    let state = send(&c.app, (&Method::GET, &p.path()), &p.jar, "json").await;
    assert!(state.no_store && state.vary && !state.text.contains("client_secret"));
    assert_eq!(c.creates(), 1);
    let body = press("pm_card_visa");
    let first = send(&c.app, (&Method::POST, &p.path()), &p.jar, &body).await;
    assert!(first.text.contains("\"waiting\""), "{}", first.text);
    let again = send(&c.app, (&Method::POST, &p.path()), &p.jar, &body).await;
    assert!(!again.text.contains("client_secret"));
    assert_eq!((c.confirms(), has_terms(&pool, &p).await), (1, true));
    let q = picked(&pool, false).await;
    c.hooks.stripe.approve.store(true, Ordering::SeqCst);
    let approve = send(
        &c.app,
        (&Method::POST, &q.path()),
        &q.jar,
        &press("pm_card_3ds"),
    )
    .await;
    assert!(
        approve.text.contains("\"approve\"") && approve.text.contains(CLIENT_SECRET),
        "{}",
        approve.text
    );
    let read = send(&c.app, (&Method::GET, &q.path()), &q.jar, "json").await;
    assert!(read.text.contains("\"waiting\"") && !read.text.contains(CLIENT_SECRET));
}

/// TD-FEE-45 (D-1871, D-3722): after the owner's fee-mode switch a load opens no PaymentIntent
/// and shows the fee-terms line; a press calls no Stripe and ends the pick through
/// `end_for_fee_terms` (a reload shows it can't be confirmed), her PICK-046 text queued once (the
/// stored `fee_terms_changed` reason is T467's `picks` tests'; this file may read no schema).
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_fee_45_a_fee_mode_switch_charges_nothing_and_ends_the_pick(pool: PgPool) {
    let p = picked(&pool, false).await;
    let c = counted(&pool, Some(now()));
    let mut db = pool.acquire().await.expect("db");
    switch_fee_mode(&mut db, Uuid::new_v4(), FeeMode::Flat)
        .await
        .expect("switched");
    drop(db);
    let page = send(&c.app, (&Method::GET, &p.path()), &p.jar, "").await;
    assert!(page.status == 200 && page.text.contains(&html(FEE_TERMS_LINE)));
    assert!(page.text.contains("confirm-button"));
    let pressed = send(
        &c.app,
        (&Method::POST, &p.path()),
        &p.jar,
        &press("pm_card_visa"),
    )
    .await;
    assert!(
        pressed.text.contains("fee terms changed"),
        "{}",
        pressed.text
    );
    assert_eq!((c.creates(), c.confirms()), (0, 0));
    let after = send(&c.app, (&Method::GET, &p.path()), &p.jar, "").await;
    assert!(
        after.text.contains("anymore"),
        "the pick ended: {}",
        after.text
    );
    let queued = tasks(&pool).await;
    assert_eq!(naming(&queued, TEXT_KIND, p.pick), 1, "her PICK-046 text");
}

/// CON-029: her sole-word CANCEL pauses the job and withdraws the waiting pick, so the page and a
/// press refuse and charge nothing; a press racing a pause, with a charge attempt already made,
/// never says "Nothing was charged".
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_paused_pick_refuses_and_never_says_nothing_was_charged_while_charging(pool: PgPool) {
    let p = picked(&pool, false).await;
    let c = counted(&pool, Some(now()));
    pause(&pool, &p).await;
    let page = send(&c.app, (&Method::GET, &p.path()), &p.jar, "").await;
    assert!(
        page.text.contains("anymore. Nothing was charged."),
        "{}",
        page.text
    );
    let pressed = send(
        &c.app,
        (&Method::POST, &p.path()),
        &p.jar,
        &press("pm_card_visa"),
    )
    .await;
    assert!(
        pressed.text.contains("Nothing was charged"),
        "{}",
        pressed.text
    );
    assert_eq!((c.creates(), c.confirms()), (0, 0));
    let q = picked(&pool, false).await;
    let paid = send(
        &c.app,
        (&Method::POST, &q.path()),
        &q.jar,
        &press("pm_card_visa"),
    )
    .await;
    assert!(paid.text.contains("\"waiting\""));
    pause(&pool, &q).await;
    let racing = send(
        &c.app,
        (&Method::POST, &q.path()),
        &q.jar,
        &press("pm_card_visa"),
    )
    .await;
    assert!(
        !racing.text.contains("Nothing was charged"),
        "{}",
        racing.text
    );
    assert_eq!(c.confirms(), 1);
}

/// FEE-041: a press over the month's limit is not charged and gets the limit reply with the
/// portal link.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_press_over_the_fee_limit_is_not_charged(pool: PgPool) {
    let p = picked(&pool, false).await;
    let c = counted(&pool, Some(now()));
    let mut db = pool.acquire().await.expect("db");
    set_fee_limit(&mut db, BusinessId(p.business), Some(1_000))
        .await
        .expect("limit");
    drop(db);
    let pressed = send(
        &c.app,
        (&Method::POST, &p.path()),
        &p.jar,
        &press("pm_card_visa"),
    )
    .await;
    assert!(pressed.text.contains("monthly fee limit") && pressed.text.contains("\"link\":\"/b\""));
    assert_eq!(c.confirms(), 0);
}

/// TD-SEC-32 (served): with "This inbox is shared" on, a Confirm inside a live session still
/// needs a fresh code: without one nothing is charged.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_32_a_shared_inbox_press_needs_a_fresh_code(pool: PgPool) {
    let p = picked(&pool, false).await;
    let c = counted(&pool, Some(now()));
    let mut db = pool.acquire().await.expect("db");
    let by = Acting {
        business: p.business,
        member: p.owner,
    };
    record_step_up(&mut db, by, now()).await.expect("step-up");
    let who = business_principal(&mut db, &p.jar, now()).await;
    let who = who.expect("read").expect("signed in");
    let phone = CodeTo::Phone(SecretString::from("+19285550142"));
    let shared = set_inbox_shared(&mut db, &who, (true, &phone), now()).await;
    shared.expect("shared");
    drop(db);
    let pressed = send(
        &c.app,
        (&Method::POST, &p.path()),
        &p.jar,
        &press("pm_card_visa"),
    )
    .await;
    assert!(
        pressed.text.contains("This inbox is shared")
            && pressed.text.contains("Nothing was charged")
    );
    assert_eq!((c.confirms(), has_terms(&pool, &p).await), (0, false));
}

/// TD-FEE-58 (served): a free first win in the launch window shows "Confirm" with no
/// PaymentIntent; a press after a crash between the waiver and the release releases once, and a
/// repeated press releases nothing more.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_fee_58_a_zero_confirm_releases_once_through_the_production_sink(pool: PgPool) {
    let p = picked(&pool, true).await;
    let launch = now() - SignedDuration::from_hours(24);
    let app = counted(&pool, Some(launch)).app;
    crashed_after_the_waiver(&pool, &p, launch).await;
    let page = send(&app, (&Method::GET, &p.path()), &p.jar, "").await;
    assert!(page.text.contains(">Confirm</button>") && page.text.contains("data-paid=\"no\""));
    let first = send(&app, (&Method::POST, &p.path()), &p.jar, &press("")).await;
    assert!(first.text.contains("\"confirmed\""), "{}", first.text);
    let again = send(&app, (&Method::POST, &p.path()), &p.jar, &press("")).await;
    assert!(!again.text.contains("\"confirmed\""));
    let queued = tasks(&pool).await;
    assert_eq!(
        naming(&queued, BUSINESS_NOTICE_KIND, p.pick),
        1,
        "one release"
    );
}

/// D-3813: in test and live `main` does not start without the publishable key or the launch
/// date, naming the missing one; in dev and CI no part is built.
#[test]
fn main_needs_the_confirm_keys_in_test_and_live() {
    let pairs = |env: &str, extra: &[(&str, &str)]| -> Vec<(String, String)> {
        let base = [
            ("ASKHAVA_APP_ENV", env),
            ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
            (
                "ASKHAVA_STORAGE_BASE_URL",
                "file:///nonexistent/askhava-fake",
            ),
            (
                "ASKHAVA_ALLOW_LIST_TEST",
                "twilio:ACtestfake01,stripe:acct_testfake,resend:test.example.com,bucket:askhava-test",
            ),
            ("ASKHAVA_ID_SECRET", ID_SECRET),
        ];
        base.iter()
            .chain(extra)
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    };
    let built = |env: &str, extra: &[(&str, &str)]| {
        let all = pairs(env, extra);
        let settings = Settings::from_pairs(all.clone()).expect("settings");
        confirm_settings(&settings, all)
            .map(|part| part.is_some())
            .map_err(|e| e.to_string())
    };
    let key = ("ASKHAVA_STRIPE_PUBLISHABLE_KEY", "pk_test_fake261");
    let launch = ("ASKHAVA_LAUNCH_DATE", "2026-12-01T07:00:00Z");
    assert_eq!(
        built("test", &[launch]),
        Err("missing ASKHAVA_STRIPE_PUBLISHABLE_KEY".to_owned())
    );
    assert_eq!(
        built("test", &[key]),
        Err("missing ASKHAVA_LAUNCH_DATE".to_owned())
    );
    assert_eq!(built("test", &[key, launch]), Ok(true));
    assert_eq!(built("dev", &[]), Ok(false));
}
