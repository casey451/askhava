//! TD-BIZ-61 (T733, D-4727, D-5318, D-5319; BIZ-010, BIZ-058, SEC-027, BIZ-046, SEC-019,
//! A11Y-030): the service-area settings, the "This inbox is shared" switch and the contact card,
//! served by the assembled router (`build_router` with fake webhooks and the fake masked pool)
//! over PostgreSQL. People sign in through `portals`' emailed-link sign-in; a step-up is recorded
//! with `record_session_step_up` in the signed-in session (D-5314). Every ID, address and number
//! is fake.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_businesses::api::{
    Acting, ConsentChannel, Roles, add_member, add_owner, create, link_phone, record_consent,
    record_session_step_up, service_area,
};
use askhava_core::api::{MIGRATOR, Settings, SystemClock};
use askhava_portals::api::{
    CookieJar, PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress, SignInMail,
    confirm_code_needed, contact_card_name, csrf_token, current_session, ensure_pre_session,
    inbox_shared, signin_post, signin_request,
};
use askhava_privacy::api::{Vault, mailbox_for};
use askhava_server::business_contacts::ContactDirectory;
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, header};
use axum::routing::post;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const AREA: &str = "/b/settings/service-area";
const INBOX: &str = "/b/shared-inbox";
const CARD: &str = "/b/contact-card.vcf";
const STEP_UP: &str = "/b/step-up?return=%2Fb";
const POOL_NUMBER: &str = "+15005550100";
const NOT_FOUND: &str = "We can't find that page";

fn vault_key(byte: u8) -> String {
    format!("1:{}", STANDARD.encode([byte; 32]))
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(vault_key(b));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x53), &key(0x63), &url).expect("vault")
}

/// Dev settings with the Twilio webhooks and the one-number fake masked pool, every base URL
/// unreachable.
fn pairs() -> Vec<(String, String)> {
    let fixed = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        ("ASKHAVA_PUBLIC_BASE_URL", "https://askhava.example"),
        (
            "ASKHAVA_ID_SECRET",
            "test-only-id-secret-not-a-real-key-0002",
        ),
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
        ("ASKHAVA_MASKED_NUMBERS", POOL_NUMBER),
        ("ASKHAVA_TWILIO_BASE_URL", "http://127.0.0.1:9"),
        ("ASKHAVA_TWILIO_VERIFY_BASE_URL", "http://127.0.0.1:9"),
    ];
    let keys = [("VAULT_KEY", 0x53), ("VAULT_INDEX_KEY", 0x63)];
    let fixed = fixed
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()));
    let keys = keys.iter().map(|(k, b)| ((*k).to_owned(), vault_key(*b)));
    fixed.chain(keys).collect()
}

/// The assembled router with the fake webhooks and voice hooks (the masked pool).
fn app(pool: &PgPool) -> Router {
    let pairs = pairs();
    let settings = Settings::from_pairs(pairs.clone()).expect("fake settings");
    let webhooks = askhava_server::webhooks(&settings, pool.clone(), pairs.clone());
    let webhooks = webhooks.expect("webhooks");
    let voice = askhava_server::voice_hooks(&settings, webhooks.as_ref(), pairs);
    let voice = voice.expect("voice hooks");
    assert!(voice.is_some(), "the masked pool is configured");
    let parts = RouterParts::from((webhooks, voice, None, None));
    build_router(pool.clone(), Router::new(), parts)
}

fn request(method: Method, path: &str, jar: &CookieJar, body: &str) -> Request<Body> {
    let csrf = csrf_token(jar).unwrap_or_default();
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "same-origin")
        .header(header::ORIGIN, format!("https://{HOST}"))
        .body(Body::from(format!("csrf={csrf}{body}")))
        .expect("request");
    let peer = SocketAddr::from(([203, 0, 113, 8], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// The status, the headers and the body text.
async fn answer(app: &Router, request: Request<Body>) -> (u16, HeaderMap, String) {
    let response = app.clone().oneshot(request).await.expect("answer");
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    let text = String::from_utf8_lossy(&bytes).into_owned();
    (parts.status.as_u16(), parts.headers, text)
}

/// One request's status, `Location` and body.
async fn send(app: &Router, (method, path): (Method, &str), jar: &CookieJar, body: &str) -> Reply {
    let (status, headers, text) = answer(app, request(method, path, jar, body)).await;
    let to = headers.get(header::LOCATION).and_then(|v| v.to_str().ok());
    (status, to.unwrap_or_default().to_owned(), text)
}

type Reply = (u16, String, String);

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

/// The person at `to` signed in through the emailed link.
async fn signed_in(pool: &PgPool, to: SignInAddress) -> CookieJar {
    let mail = Arc::new(OneMail {
        to,
        sent: Mutex::default(),
    });
    let routes = Router::new()
        .route(SIGNIN_ROUTE, post(signin_request::<OneMail>))
        .route(SIGNIN_LINK_ROUTE, post(signin_post::<OneMail>));
    let signin =
        SignIn::new(pool.clone(), Arc::new(SystemClock), Arc::clone(&mail)).into_router(routes);
    let jar = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let ask = request(
        Method::POST,
        SIGNIN_ROUTE,
        &jar,
        "&email=person%40x.example",
    );
    answer(&signin, ask).await;
    let token = (mail.sent.lock().unwrap_or_else(PoisonError::into_inner)).clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let (_, headers, _) = answer(&signin, request(Method::POST, &path, &jar, "")).await;
    let cookies: Vec<&str> = (headers.get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next())
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let mut sent = HeaderMap::new();
    sent.insert(header::COOKIE, cookies.join("; ").parse().expect("cookies"));
    CookieJar::from_headers(&sent)
}

/// A fake business: its owner and a "Cards and quotes" member, each signed in.
struct Team {
    owner: (Acting, CookieJar),
    member: (Acting, CookieJar),
}

async fn team(pool: &PgPool) -> Team {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let owner_box = mailbox_for(&mut db, &vault(), "owner@x.example").await;
    let member_box = mailbox_for(&mut db, &vault(), "member@x.example").await;
    let (owner_box, member_box) = (owner_box.expect("mailbox"), member_box.expect("mailbox"));
    let business = create(&mut db, false, now).await.expect("business");
    let owner = add_owner(&mut db, business, owner_box, now)
        .await
        .expect("owner");
    let owner = Acting {
        business,
        member: owner,
    };
    let cards = Roles::new(true, false).expect("roles");
    let member = add_member(&mut db, owner, member_box, cards, now).await;
    let member = Acting {
        business,
        member: member.expect("member"),
    };
    drop(db);
    let to = |mailbox| SignInAddress { business, mailbox };
    Team {
        owner: (owner, signed_in(pool, to(owner_box)).await),
        member: (member, signed_in(pool, to(member_box)).await),
    }
}

/// Links a fake phone for `by` with its text box ticked, so it gets texts (SEC-027).
async fn texting_phone(pool: &PgPool, by: Acting) {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let dir = ContactDirectory::new(vault());
    let number = SecretString::from("+19285550177".to_owned());
    let line = link_phone(&mut db, &dir, (by, false), &number, now).await;
    let at = (by.business, by.member, Some(line.expect("linked")));
    let ticked = record_consent(&mut db, at, &[ConsentChannel::Text], true, now).await;
    ticked.expect("consent");
}

/// Records a passed step-up for `by` `ago` before now, in the session of `jar` (D-5314).
async fn stepped_up(pool: &PgPool, (by, jar): &(Acting, CookieJar), ago: SignedDuration) {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let signed = current_session(&mut db, jar, now).await.expect("read");
    let id = signed.expect("signed in").id();
    let at = now.checked_sub(ago).expect("time");
    record_session_step_up(&mut db, *by, id, at)
        .await
        .expect("step-up");
}

/// The stored base ZIP and radius of `business`.
async fn stored(pool: &PgPool, business: Uuid) -> Option<(String, u8)> {
    let mut db = pool.acquire().await.expect("db");
    let area = service_area(&mut db, business).await.expect("read");
    area.map(|s| (s.area.base_zip().to_owned(), s.area.radius_miles()))
}

/// Whether `member`'s inbox is marked shared, and whether a Confirm then owes a code.
async fn shared(pool: &PgPool, member: Uuid) -> (bool, bool) {
    let mut db = pool.acquire().await.expect("db");
    let marked = inbox_shared(&mut db, member).await.expect("read");
    let owed = confirm_code_needed(&mut db, member, true)
        .await
        .expect("read");
    (marked, owed)
}

/// TD-BIZ-61: the settings page shows, saves and refuses (A11Y-030), for the owner only.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_61_service_area_settings_show_save_and_refuse(pool: PgPool) {
    let (app, t) = (app(&pool), team(&pool).await);
    let ((owner, jar), business) = (&t.owner, t.owner.0.business);
    let saved = send(&app, (Method::POST, AREA), jar, "&base_zip=86403&radius=25").await;
    assert_eq!(saved.0, 200);
    assert!(saved.2.contains("Saved. New job cards") && !saved.2.contains("Error: "));
    assert_eq!(
        stored(&pool, owner.business).await,
        Some(("86403".into(), 25))
    );
    let page = send(&app, (Method::GET, AREA), jar, "").await;
    assert_eq!(page.0, 200);
    assert!(page.2.contains("value=\"86403\"") && page.2.contains("value=\"25\""));
    assert!(!page.2.contains("Saved. New job cards"));
    for (zip, radius) in [("8640", "25"), ("86404", "0"), ("86404", "101")] {
        let body = format!("&base_zip={zip}&radius={radius}");
        let refused = send(&app, (Method::POST, AREA), jar, &body).await;
        assert_eq!(refused.0, 422, "{zip} {radius}");
        let text = &refused.2;
        assert!(text.contains("<title>Error: ") && text.contains("id=\"error-summary\""));
        assert!(
            text.contains(&format!("value=\"{zip}\""))
                && text.contains(&format!("value=\"{radius}\""))
        );
        assert_eq!(stored(&pool, business).await, Some(("86403".into(), 25)));
    }
    let (_, member_jar) = &t.member;
    let get = send(&app, (Method::GET, AREA), member_jar, "").await;
    let post = send(
        &app,
        (Method::POST, AREA),
        member_jar,
        "&base_zip=86404&radius=9",
    )
    .await;
    assert_eq!((get.0, post.0), (404, 404));
    assert!(get.2.contains(NOT_FOUND) && !get.2.contains("86403"));
    assert_eq!(stored(&pool, business).await, Some(("86403".into(), 25)));
    let odd = send(
        &app,
        (Method::POST, AREA),
        jar,
        "&base_zip=86404&radius=9&x=1",
    )
    .await;
    assert_eq!(odd.0, 422);
    let signed_out = send(&app, (Method::GET, AREA), &CookieJar::new(), "").await;
    assert_eq!(signed_out.0, 401);
}

/// TD-BIZ-61, SEC-027: the switch needs a fresh code and a texting phone; once on, a Confirm in a
/// live session owes a code (T810's code step).
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_61_shared_inbox_switch_needs_a_fresh_code_and_a_texting_phone(pool: PgPool) {
    let (app, t) = (app(&pool), team(&pool).await);
    let ((owner, jar), member) = (&t.owner, t.member.0.member);
    texting_phone(&pool, *owner).await;
    let asked = send(&app, (Method::POST, INBOX), jar, "&shared=yes").await;
    assert_eq!((asked.0, asked.1.as_str()), (303, STEP_UP));
    assert_eq!(shared(&pool, owner.member).await, (false, false));
    stepped_up(&pool, &t.owner, SignedDuration::from_mins(6)).await;
    let stale = send(&app, (Method::POST, INBOX), jar, "&shared=yes").await;
    assert_eq!((stale.0, stale.1.as_str()), (303, STEP_UP));
    stepped_up(&pool, &t.owner, SignedDuration::ZERO).await;
    let on = send(&app, (Method::POST, INBOX), jar, "&shared=yes").await;
    assert_eq!((on.0, on.1.as_str()), (303, "/b"));
    assert_eq!(shared(&pool, owner.member).await, (true, true));
    assert_eq!(shared(&pool, member).await, (false, false));
    let off = send(&app, (Method::POST, INBOX), jar, "&shared=no").await;
    assert_eq!((off.0, off.1.as_str()), (303, "/b"));
    assert_eq!(shared(&pool, owner.member).await, (false, false));
    let bad = send(&app, (Method::POST, INBOX), jar, "&shared=maybe").await;
    assert_eq!(bad.0, 422);
    stepped_up(&pool, &t.member, SignedDuration::ZERO).await;
    let no_phone = send(&app, (Method::POST, INBOX), &t.member.1, "&shared=yes").await;
    assert_eq!((no_phone.0, no_phone.1.as_str()), (303, "/b"));
    assert_eq!(shared(&pool, member).await, (false, false));
    let home = send(&app, (Method::GET, "/b"), &t.member.1, "").await;
    assert!(home.2.contains("first add a mobile number that gets texts"));
    assert!(
        home.2
            .contains(&format!("href=\"{AREA}\">Service area</a>"))
    );
}

/// TD-BIZ-61, BIZ-046: the card is a vCard attachment named "<brand> homeowner calls" holding the
/// current pool number and nothing else; signed out it is the sign-in answer.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_biz_61_contact_card_holds_the_pool_numbers_only(pool: PgPool) {
    let (app, t) = (app(&pool), team(&pool).await);
    let (status, headers, text) = answer(&app, request(Method::GET, CARD, &t.member.1, "")).await;
    assert_eq!(status, 200);
    let head = |name| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    };
    assert!(head(header::CONTENT_TYPE).starts_with("text/vcard"));
    let disposition = format!("attachment; filename=\"{}.vcf\"", contact_card_name());
    assert_eq!(head(header::CONTENT_DISPOSITION), disposition);
    assert_eq!(head(header::CACHE_CONTROL), "no-store");
    assert!(text.starts_with("BEGIN:VCARD") && text.contains(POOL_NUMBER));
    let tel = text.lines().filter(|l| l.starts_with("TEL")).count();
    assert_eq!(tel, 1);
    assert!(!text.contains("x.example") && !text.contains("5550177"));
    let out = answer(&app, request(Method::GET, CARD, &CookieJar::new(), "")).await;
    assert_eq!(out.0, 401);
    assert!(!out.2.contains(POOL_NUMBER));
}
