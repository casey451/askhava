//! T782 (D-5081 F21, D-5162, D-5585; PICK-024, BIZ-013, SEC-019): the released job's page
//! (`/b/picks/{pick}`) served by the assembled router over PostgreSQL shows the pick's real job
//! number, never job 0. A pick whose job has no number row is the page's failure answer; another
//! business's pick and a "Cards and quotes"-only member are the one not-found answer, the same as
//! an unknown pick. People sign in through `portals`' emailed link and the released pick is built
//! through module APIs only, as `business_portal_route.rs` does. Every ID, name and key is fake.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{MaskedPool, TwilioConfig};
use askhava_businesses::api::{AccountStatus, Acting, Roles, add_member, add_owner, create};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{Guarded, MIGRATOR, Settings, SystemClock};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_jobs::api::{JobStatus, create as create_job, job_number, move_to, record_job_facts};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor as AuditActor;
use askhava_payments::api::{
    BusinessId, EntryKind, FeeJob, FeeMode, FeeQuote, LedgerEntry, NewEntry, PeriodJob,
    RecurringCap, Service, VisitTier, fee_at_pick, fix_fee_at_pick, ledger_append, no_fee_release,
};
use askhava_picks::api::{PagePick, Released, Settled, page_pick, release, settle};
use askhava_portals::api::{
    CookieJar, PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress, SignInMail,
    csrf_token, ensure_pre_session, signin_post, signin_request,
};
use askhava_privacy::api::{NewContact, Vault, mailbox_for, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardJob, Format, OfferTown, Price, QuoteForm, QuoteRules, parse_quote,
    send_cards, submit_quote,
};
use askhava_server::{Hooks, RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, header};
use axum::routing::post;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.test";

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x51), &key(0x61), &url).expect("vault")
}

/// A same-origin request with the jar's cookies and CSRF token.
fn request(method: &Method, path: &str, jar: &CookieJar, extra: &str) -> Request<Body> {
    let csrf = csrf_token(jar).unwrap_or_default();
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let mut request = Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, HOST)
        .header("x-request-id", Uuid::new_v4().to_string())
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "same-origin")
        .header(header::ORIGIN, format!("https://{HOST}"))
        .body(Body::from(format!("csrf={csrf}{extra}")))
        .expect("request");
    let peer = SocketAddr::from(([203, 0, 113, 7], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// An answer's status, body and the cookies it set.
#[derive(Debug, PartialEq, Eq)]
struct Reply {
    status: u16,
    text: String,
    cookies: Vec<String>,
}

async fn answer(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.expect("answer");
    let (parts, body) = response.into_parts();
    let cookies = (parts.headers.get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next().map(str::to_owned))
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        text: String::from_utf8_lossy(&bytes).into_owned(),
        cookies,
    }
}

/// The one sign-in address of one person; the link it "emails" is kept.
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
async fn business_jar(pool: &PgPool, to: SignInAddress) -> CookieJar {
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
        &Method::POST,
        SIGNIN_ROUTE,
        &jar,
        "&email=person%40x.example",
    );
    answer(&app, asked).await;
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let pressed = answer(&app, request(&Method::POST, &path, &jar, "")).await;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        pressed.cookies.join("; ").parse().expect("cookies"),
    );
    CookieJar::from_headers(&headers)
}

/// A person of a business: their acting IDs and signed-in cookies.
struct Person {
    by: Acting,
    jar: CookieJar,
}

/// A new active business's owner at `address`.
async fn owner(pool: &PgPool, address: &str) -> Person {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let mailbox = mailbox_for(&mut db, &vault(), address)
        .await
        .expect("mailbox");
    let business = create(&mut db, false, now).await.expect("business");
    let member = add_owner(&mut db, business, mailbox, now)
        .await
        .expect("owner");
    drop(db);
    let jar = business_jar(pool, SignInAddress { business, mailbox }).await;
    Person {
        by: Acting { business, member },
        jar,
    }
}

/// A "Cards and quotes"-only member of `owner`'s business, signed in.
async fn cards_member(pool: &PgPool, owner: Acting) -> CookieJar {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let mailbox = mailbox_for(&mut db, &vault(), "quotes@x.example")
        .await
        .expect("mailbox");
    let roles = Roles::new(true, false).expect("roles");
    add_member(&mut db, owner, mailbox, roles, now)
        .await
        .expect("member");
    drop(db);
    let business = owner.business;
    business_jar(pool, SignInAddress { business, mailbox }).await
}

/// An AC repair job in Lake Havasu City as matching reads it, and `business` able to take it.
fn card_for(business: Uuid) -> (CardJob, CardBusiness) {
    let kind = ("hvac_repair".to_owned(), "AC not cooling".to_owned());
    let to = CardBusiness {
        id: business,
        status: AccountStatus::Active,
        verified: true,
        towns: BTreeSet::from(["lake_havasu_city".to_owned()]),
        ticked: BTreeSet::from([kind.clone()]),
        classes: vec!["R-39".to_owned()],
        licenses: Holder::default(),
    };
    let candidate = Job {
        category: kind.0,
        job_type: kind.1,
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
        candidates: vec![candidate],
        text: String::new(),
    };
    (card, to)
}

/// A firm $325 quote form and its rules.
fn firm_quote() -> (QuoteForm, QuoteRules) {
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
    (form, rules)
}

/// The job opened (with its number, `jobs::api::create`), carded to `business` and quoted.
async fn quoted_job(db: &mut PgConnection, business: Uuid, now: Timestamp) -> Uuid {
    let job = create_job(db, now, false, AuditActor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, open, now, AuditActor::System).await;
    assert_eq!(moved.expect("open"), Guarded::Applied(()));
    let (card, to) = card_for(business);
    let map = ClassMap::shipped().expect("the shipped license map");
    let sent = send_cards(db, map, job, &card, &[to], now).await;
    assert_eq!(sent.expect("card").len(), 1);
    let (form, rules) = firm_quote();
    let quote = parse_quote(form, &rules).expect("quote");
    submit_quote(db, (job, business), &quote, now)
        .await
        .expect("quoted");
    job
}

/// Her vault contact (fake name, phone and street) and her text request naming `job`.
async fn her_request(db: &mut PgConnection, job: Uuid, now: Timestamp) -> Uuid {
    let new = NewContact {
        name: Some(SecretString::from("Dana Testperson")),
        phone: Some(SecretString::from("+19285550192")),
        email: None,
        address: Some(SecretString::from("300 Fake Lane")),
        onsite_contact: None,
    };
    let contact = store_contact(db, &vault(), &new).await.expect("contact");
    let inbound = Inbound {
        words: "my AC is warm",
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let text = TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound,
        received_at: now,
    };
    let secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let handled = receive_text(db, &secret, &text, IntakeSettings::default()).await;
    let conversation = handled.expect("request").conversation.expect("open");
    let code = job.simple().to_string();
    let answer = Answer {
        conversation,
        question: "job.plumbing.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::new_v4(), now),
    };
    record_answer(db, &answer).await.expect("job named");
    contact
}

/// The fake masked pool the release shows (CALL-017).
fn masked() -> MaskedPool {
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        (
            "ASKHAVA_TWILIO_ACCOUNT_SID",
            "AC00000000000000000000000000000000",
        ),
        ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake_twilio_auth_token"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_POOL_SIZE", "1"),
        ("ASKHAVA_MASKED_NUMBERS", "+15005550100"),
        ("ASKHAVA_TWILIO_BASE_URL", "http://127.0.0.1:1"),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", "http://127.0.0.1:1"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    TwilioConfig::from_settings(&settings, pairs)
        .expect("twilio")
        .pool()
        .clone()
}

/// `business`'s page pick on a new quoted job, released to her on the $0 path, and its job.
async fn released_job(pool: &PgPool, business: Uuid) -> (Uuid, Uuid) {
    let now = Timestamp::now();
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
    let job = quoted_job(&mut db, business, now).await;
    let contact = her_request(&mut db, job, now).await;
    let types = ["AC not cooling".to_owned()];
    record_job_facts(&mut db, job, "hvac_repair", &types)
        .await
        .expect("job facts");
    let mut tx = pool.begin().await.expect("tx");
    let picked = page_pick(&mut tx, (job, false), 1, AuditActor::System, now).await;
    tx.commit().await.expect("commit");
    let PagePick::Picked(pick) = picked.expect("pick") else {
        panic!("no new pick")
    };
    let key = format!("t782.free.{job}.{business}");
    let entry = NewEntry {
        kind: EntryKind::FreeFirstWin,
        business: Some(business),
        job: Some(job),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: &key,
    };
    ledger_append(&mut db, &LedgerEntry::new(entry).expect("entry"))
        .await
        .expect("ledger");
    let cleared = no_fee_release(&mut db, pick, (business, job))
        .await
        .expect("cleared");
    drop(db);
    let mut tx = pool.begin().await.expect("tx");
    assert_eq!(
        settle(&mut tx, &cleared).await.expect("settle"),
        Settled::Moved(job)
    );
    tx.commit().await.expect("commit");
    let proof = askhava_server::release_minter().mint(cleared, contact);
    let shown = release(pool, (&vault(), &masked()), (proof, false), now).await;
    assert!(matches!(shown.expect("release"), Released::Shown(_)));
    (pick, job)
}

/// The assembled router, with the vault the portal's vault pages read.
fn app(pool: &PgPool) -> Router {
    let hooks: Hooks = (None, None, None, None);
    let parts = RouterParts::from(hooks).with_vault(Some(vault()));
    build_router(pool.clone(), Router::new(), parts)
}

async fn get(app: &Router, path: &str, jar: &CookieJar) -> Reply {
    answer(app, request(&Method::GET, path, jar, "")).await
}

/// PICK-024, BIZ-013: the owner's released pick shows its job's real number, never job 0.
#[sqlx::test(migrator = "MIGRATOR")]
async fn released_pick_page_shows_the_real_job_number(pool: PgPool) {
    let x = owner(&pool, "owner@x.example").await;
    let (pick, job) = released_job(&pool, x.by.business).await;
    let mut db = pool.acquire().await.expect("db");
    let number = job_number(&mut db, job).await.expect("number");
    drop(db);
    let page = get(&app(&pool), &format!("/b/picks/{pick}"), &x.jar).await;
    assert_eq!(page.status, 200, "{}", page.text);
    assert!(page.text.contains(&format!("<h1>Job {number}</h1>")));
    assert!(!page.text.contains("Job 0<"), "never job 0");
}

/// The $100-quote fee fixed for a new pick of `business` on a job with no number row (any job ID:
/// `payments.pick_fee` names the job by ID only), through the payments API only (TD-SEC-10).
async fn pick_without_a_number(pool: &PgPool, business: Uuid) -> Uuid {
    let job = FeeJob {
        diagnostic_visit: false,
        visit_tier: VisitTier::DEFAULT,
        fee_free: false,
    };
    let job = PeriodJob {
        job,
        service: Service::NotRecurring,
        cap: RecurringCap::DEFAULT,
    };
    let fee = fee_at_pick(FeeQuote::Fixed(10_000), None, job, FeeMode::Tiers, None).expect("fee");
    let (pick, mut db) = (Uuid::new_v4(), pool.acquire().await.expect("db"));
    let (to, now) = (BusinessId(business), Timestamp::now());
    fix_fee_at_pick(&mut db, pick, to, Uuid::new_v4(), &fee, now)
        .await
        .expect("fee fixed");
    pick
}

/// D-5081: a pick whose job has no number row is the page's failure answer, never job 0.
#[sqlx::test(migrator = "MIGRATOR")]
async fn a_job_without_a_number_is_the_failure_answer(pool: PgPool) {
    let x = owner(&pool, "owner@x.example").await;
    let pick = pick_without_a_number(&pool, x.by.business).await;
    let page = get(&app(&pool), &format!("/b/picks/{pick}"), &x.jar).await;
    assert_eq!(page.status, 500);
    assert!(!page.text.contains("Job 0"), "never job 0");
}

/// SEC-019, D-5585: another business's pick and a "Cards and quotes"-only member on the pick are
/// the one not-found answer, the same as an unknown pick.
#[sqlx::test(migrator = "MIGRATOR")]
async fn another_business_and_a_cards_member_get_the_one_not_found(pool: PgPool) {
    let x = owner(&pool, "owner@x.example").await;
    let y = owner(&pool, "owner@y.example").await;
    let quoter = cards_member(&pool, x.by).await;
    let (pick, _) = released_job(&pool, x.by.business).await;
    let app = app(&pool);
    let unknown = get(&app, &format!("/b/picks/{}", Uuid::new_v4()), &x.jar).await;
    assert_eq!(unknown.status, 404);
    let path = format!("/b/picks/{pick}");
    assert_eq!(get(&app, &path, &y.jar).await, unknown, "another business");
    assert_eq!(
        get(&app, &path, &quoter).await,
        unknown,
        "a cards-only member"
    );
}
