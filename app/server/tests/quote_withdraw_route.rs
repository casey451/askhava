//! TD-PICK-73 (T732, D-4727; PICK-051, SEC-019, CARD-019) through the assembled router over
//! PostgreSQL: `POST /b/jobs/{j}/withdraw`, the business job page's "Withdraw your quote". Before
//! any pick it withdraws the business's quote and 303s to the job page, which then offers "Send a
//! new quote"; a repeat is the same answer. After a pick, once the job left the statuses that take
//! edits, or with no quote, the page comes back with one line saying why and nothing changes.
//! Another business's number, an unknown number and a UUID are the one not-found answer and change
//! nothing; a post without its CSRF token is refused. People sign in through `portals`' emailed
//! link, as `job_page_route.rs` does. Every name, phone, street, ID and key is fake.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_businesses::api::{AccountStatus, Acting, add_owner, create};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{MIGRATOR, Settings, SystemClock, TestControl};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{Answer, TextIn, receive_text, record_answer};
use askhava_jobs::api::{
    JobStatus, create as create_job, current, job_number, move_to, record_job_facts,
};
use askhava_licensing::api::{ClassMap, Holder, Job, LicenseKind};
use askhava_ops::api::Actor as AuditActor;
use askhava_picks::api::{PagePick, page_pick};
use askhava_portals::api::{
    CookieJar, PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress, SignInMail,
    csrf_token, ensure_pre_session, signin_post, signin_request,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::{
    CardBusiness, CardFacts, CardJob, CardParts, DEFAULT_MIN_WORDS, Format, KeptCard, OfferTown,
    Place, Price, QuoteForm, QuoteRules, QuoteStanding, build_card, card_digest, keep_card,
    mark_card_sent, parse_quote, quote_standing, quoted_prices, send_cards, submit_quote,
};
use askhava_server::{RouterParts, build_router, webhooks};
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
const ID_SECRET: &str = "test-only-id-secret-not-a-real-key-0732";
const NOT_FOUND: &str = "We can't find that page";
const CLOSED: &str = "The homeowner has already picked a quote, or the job has closed";
const NO_QUOTE: &str = "You haven't sent a quote on this job, so there's nothing to withdraw.";

/// An answer: status, `Location` and body.
struct Reply {
    status: u16,
    location: Option<String>,
    text: String,
    cookies: Vec<String>,
}

/// A same-origin request as a browser sends it, with `body` as a form.
fn request(method: &Method, path: &str, jar: &CookieJar, body: String) -> Request<Body> {
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let mut request = Request::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, HOST)
        .header("sec-fetch-site", "same-origin")
        .header(header::ORIGIN, format!("https://{HOST}"))
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .expect("request");
    let peer = SocketAddr::from(([203, 0, 113, 73], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

async fn answer(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.expect("answer");
    let (parts, body) = response.into_parts();
    let location = parts.headers.get(header::LOCATION);
    let location = location.and_then(|v| v.to_str().ok()).map(str::to_owned);
    let cookies = (parts.headers.get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next().map(str::to_owned))
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    Reply {
        status: parts.status.as_u16(),
        location,
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

/// A new active business's owner, signed in through `portals`' emailed link: (acting, jar).
async fn owner(pool: &PgPool) -> (Acting, CookieJar) {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let business = create(&mut db, false, now).await.expect("business");
    let mailbox = Uuid::new_v4();
    let member = add_owner(&mut db, business, mailbox, now).await;
    drop(db);
    let to = SignInAddress { business, mailbox };
    let mail = Arc::new(OneMail {
        to,
        sent: Mutex::default(),
    });
    let routes = Router::new()
        .route(SIGNIN_ROUTE, post(signin_request::<OneMail>))
        .route(SIGNIN_LINK_ROUTE, post(signin_post::<OneMail>));
    let app = SignIn::new(pool.clone(), Arc::new(SystemClock), Arc::clone(&mail));
    let app = app.into_router(routes);
    let jar = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let csrf = csrf_token(&jar).expect("csrf");
    let form = format!("csrf={csrf}&email=person%40x.example");
    answer(&app, request(&Method::POST, SIGNIN_ROUTE, &jar, form)).await;
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let body = format!("csrf={csrf}");
    let pressed = answer(&app, request(&Method::POST, &path, &jar, body)).await;
    let mut headers = HeaderMap::new();
    let cookies = pressed.cookies.join("; ").parse().expect("cookies");
    headers.insert(header::COOKIE, cookies);
    let acting = Acting {
        business,
        member: member.expect("owner"),
    };
    (acting, CookieJar::from_headers(&headers))
}

/// `business` able to take an AC repair job in Lake Havasu City.
fn able(business: Uuid) -> CardBusiness {
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

/// The AC repair job as matching reads it.
fn matched() -> CardJob {
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

/// A firm $325 quote from `business` on `job`, confirmed.
async fn quote(pool: &PgPool, (job, business): (Uuid, Uuid)) {
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
    let quote = parse_quote(form, &rules).expect("quote");
    let mut db = pool.acquire().await.expect("db");
    let at = Timestamp::now();
    let done = submit_quote(&mut db, (job, business), &quote, at).await;
    done.expect("quoted");
}

/// A fake key in the vault's format.
fn key(byte: u8) -> String {
    format!("1:{}", STANDARD.encode([byte; 32]))
}

/// A fake homeowner (fake phone ending `tail`) and her request tied to a new open job:
/// (request, job).
async fn homeowner(db: &mut PgConnection, tail: u8) -> (Uuid, Uuid) {
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    let vault = Vault::new(&key(0x51).into(), &key(0x52).into(), &url).expect("vault");
    let new = NewContact {
        name: Some(SecretString::from("Robin Fakename".to_owned())),
        phone: Some(SecretString::from(format!("+192855501{tail:02}"))),
        ..NewContact::default()
    };
    let contact = store_contact(db, &vault, &new).await.expect("contact");
    let now = Timestamp::now();
    let inbound = Inbound {
        words: "my AC blows warm air",
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
    let id_secret = SecretString::from(ID_SECRET.to_owned());
    let handled = receive_text(db, &id_secret, &text, IntakeSettings::default()).await;
    let conversation = handled.expect("request").conversation.expect("open");
    let job = create_job(db, now, false, AuditActor::System).await;
    let job = job.expect("job");
    let code = job.simple().to_string();
    let named = Answer {
        conversation,
        question: "job.plumbing.p0",
        options: Some(&[(code.as_str(), "fakejob")]),
        words: "fakejob",
        reply: (Uuid::new_v4(), now),
    };
    record_answer(db, &named).await.expect("her job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(db, job, open, now, AuditActor::System).await;
    assert!(moved.expect("open").applied().is_some(), "opened");
    (conversation, job)
}

/// A job of homeowner `tail` carded to `businesses`, its card kept and sent: (job, number).
async fn carded(pool: &PgPool, businesses: &[Uuid], tail: u8) -> (Uuid, i64) {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let (request, job) = homeowner(&mut db, tail).await;
    let to: Vec<CardBusiness> = businesses.iter().copied().map(able).collect();
    let map = ClassMap::shipped().expect("the shipped license map");
    let sent = send_cards(&mut db, map, job, &matched(), &to, now).await;
    sent.expect("cards");
    let parts = CardParts {
        category: "AC repair".to_owned(),
        town: "Lake Havasu City".to_owned(),
        place: Place::House,
        description: "The AC runs but the air from the vents stays warm.".to_owned(),
        answers: Vec::new(),
        timing: None,
        danger: None,
        photos: Vec::new(),
        concept: None,
    };
    let card = build_card(&parts, DEFAULT_MIN_WORDS);
    let digest = card_digest(card.text());
    let facts = CardFacts {
        category: "hvac_repair".to_owned(),
        candidates: vec!["AC not cooling".to_owned()],
        emergency: false,
    };
    let kept = keep_card(&mut db, (request, job), &digest, &card, &facts, now).await;
    assert_eq!(kept.expect("kept"), KeptCard::Stored);
    let marked = mark_card_sent(&mut db, job, &digest, now).await;
    assert!(marked.expect("sent"));
    (job, job_number(&mut db, job).await.expect("number"))
}

/// The assembled router with fake settings.
fn app(pool: &PgPool) -> Router {
    let mut pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        ("ASKHAVA_PUBLIC_BASE_URL", "https://askhava.example"),
        ("ASKHAVA_ID_SECRET", ID_SECRET),
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
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", "http://127.0.0.1:1"),
        ("ASKHAVA_TWILIO_BASE_URL", "http://127.0.0.1:1"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    pairs.push(("VAULT_KEY".to_owned(), key(0x51)));
    pairs.push(("VAULT_INDEX_KEY".to_owned(), key(0x52)));
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let control = TestControl::from_settings(&settings, pairs.clone()).expect("test control");
    let hooks = webhooks(&settings, pool.clone(), pairs).expect("hooks");
    let parts = RouterParts::from((hooks, None, None, None));
    build_router(pool.clone(), control.router(), parts)
}

/// `POST /b/jobs/<j>/withdraw` by `jar`'s person, with its CSRF token when `csrf`.
async fn withdraw(app: &Router, j: &str, jar: &CookieJar, csrf: bool) -> Reply {
    let path = format!("/b/jobs/{j}/withdraw");
    let token = csrf_token(jar).filter(|_| csrf).unwrap_or_default();
    let body = format!("csrf={token}");
    answer(app, request(&Method::POST, &path, jar, body)).await
}

/// `GET /b/jobs/<number>` by `jar`'s person.
async fn open(app: &Router, number: i64, jar: &CookieJar) -> Reply {
    let path = format!("/b/jobs/{number}");
    answer(app, request(&Method::GET, &path, jar, String::new())).await
}

/// Where `business`'s quote on `job` stands.
async fn standing(pool: &PgPool, (job, business): (Uuid, Uuid)) -> QuoteStanding {
    let mut db = pool.acquire().await.expect("db");
    quote_standing(&mut db, (job, business))
        .await
        .expect("standing")
}

/// Whether `business`'s quote on `job` is withdrawn, as `quoted_prices` reads its row.
async fn withdrawn(pool: &PgPool, (job, business): (Uuid, Uuid)) -> bool {
    let mut db = pool.acquire().await.expect("db");
    let quoted = quoted_prices(&mut db, job).await.expect("quotes");
    let own = quoted.iter().find(|q| q.business == business);
    own.expect("the quote's row").withdrawn
}

async fn catalog(pool: &PgPool) {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
}

/// TD-PICK-73 (PICK-051, CARD-019): before any pick, "Withdraw your quote" withdraws it and 303s
/// to the job page, which offers "Send a new quote"; a repeat is the same answer; no CSRF token
/// is refused and changes nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_73_withdraw_before_any_pick(pool: PgPool) {
    catalog(&pool).await;
    let (x, jar) = owner(&pool).await;
    let (job, number) = carded(&pool, &[x.business], 1).await;
    quote(&pool, (job, x.business)).await;
    let app = app(&pool);
    let j = number.to_string();
    let forged = withdraw(&app, &j, &jar, false).await;
    assert_eq!(forged.status, 403, "{}", forged.text);
    assert!(!withdrawn(&pool, (job, x.business)).await);
    let page = format!("/b/jobs/{number}");
    for _ in 0..2 {
        let done = withdraw(&app, &j, &jar, true).await;
        assert_eq!((done.status, done.location.as_deref()), (303, Some(&*page)));
        assert!(withdrawn(&pool, (job, x.business)).await);
    }
    let shown = open(&app, number, &jar).await;
    assert_eq!(shown.status, 200, "{}", shown.text);
    assert!(shown.text.contains("Send a new quote"));
    assert!(!shown.text.contains("Withdraw your quote"));
    let counts = "Sent to 1 business. Quotes so far: 0.";
    assert!(shown.text.contains(counts), "{}", shown.text);
}

/// TD-PICK-73 (PICK-051): with no quote, after a pick and once the job left the statuses that
/// take edits, the page comes back with one line saying why and nothing changes.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_73_refused_says_why(pool: PgPool) {
    catalog(&pool).await;
    let (x, x_jar) = owner(&pool).await;
    let (y, y_jar) = owner(&pool).await;
    let (job, number) = carded(&pool, &[x.business, y.business], 1).await;
    quote(&pool, (job, x.business)).await;
    let app = app(&pool);
    let none = withdraw(&app, &number.to_string(), &y_jar, true).await;
    assert_eq!(none.status, 409, "{}", none.text);
    assert_eq!(none.text.matches(NO_QUOTE).count(), 1, "{}", none.text);
    assert_eq!(
        standing(&pool, (job, y.business)).await,
        QuoteStanding::NotSent
    );
    let mut db = pool.acquire().await.expect("db");
    let (now, actor) = (Timestamp::now(), AuditActor::System);
    let was = current(&mut db, job).await.expect("status").status;
    let gone = (was, JobStatus::Cancelled);
    let moved = move_to(&mut db, job, gone, now, actor).await;
    assert!(moved.expect("cancelled").applied().is_some());
    drop(db);
    let closed = withdraw(&app, &number.to_string(), &x_jar, true).await;
    assert_eq!(closed.status, 409, "{}", closed.text);
    assert_eq!(closed.text.matches(CLOSED).count(), 1, "{}", closed.text);
    assert!(!withdrawn(&pool, (job, x.business)).await);
    let (picked, picked_number) = carded(&pool, &[x.business], 2).await;
    quote(&pool, (picked, x.business)).await;
    let mut db = pool.acquire().await.expect("db");
    let types = ["AC not cooling".to_owned()];
    let facts = record_job_facts(&mut db, picked, "hvac_repair", &types).await;
    facts.expect("facts");
    let made = page_pick(&mut db, (picked, false), 1, actor, now).await;
    assert!(matches!(made, Ok(PagePick::Picked(_))), "a page pick");
    drop(db);
    let after = withdraw(&app, &picked_number.to_string(), &x_jar, true).await;
    assert_eq!(after.status, 409, "{}", after.text);
    assert_eq!(after.text.matches(CLOSED).count(), 1, "{}", after.text);
    assert!(!withdrawn(&pool, (picked, x.business)).await);
}

/// TD-PICK-73 (SEC-019): another business's number, an unknown number and the job's UUID are
/// the one not-found answer; the quote stays.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_pick_73_the_one_not_found_answer(pool: PgPool) {
    catalog(&pool).await;
    let (x, _) = owner(&pool).await;
    let (_, z_jar) = owner(&pool).await;
    let (job, number) = carded(&pool, &[x.business], 1).await;
    quote(&pool, (job, x.business)).await;
    let app = app(&pool);
    let asked = [number.to_string(), "9999999".to_owned(), job.to_string()];
    for j in &asked {
        let refused = withdraw(&app, j, &z_jar, true).await;
        assert_eq!(refused.status, 404, "{j}: {}", refused.text);
        assert!(refused.text.contains(NOT_FOUND));
        assert!(!refused.text.contains("AC runs"));
    }
    assert!(!withdrawn(&pool, (job, x.business)).await);
    assert_eq!(
        standing(&pool, (job, x.business)).await,
        QuoteStanding::Standing
    );
}
