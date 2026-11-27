//! TD-CON-35's route cases (T522, D-3879, D-3881, D-3882; CON-019, BIZ-036, TXT-035, SEC-004,
//! SEC-009): `GET` and `POST /b/texts` served by the assembled router over PostgreSQL. People
//! sign in through `portals`' emailed-link sign-in; business texting is switched on with
//! `messaging::api::switch_on` after a fake allowed eligibility answer with a campaign reference.
//! A ticked box for the current number version makes `may_text` true; after `change_phone` the
//! page shows the box again and a box posted for the old version is refused; an unticked box,
//! another member's phone and a cross-site `POST` store nothing; while texting is off the page
//! shows no box and a `POST` stores nothing. Fake 555-01xx numbers, fake addresses and IDs only.

use std::sync::{Arc, Mutex, PoisonError};

use askhava_businesses::api::{
    Acting, Roles, add_member, add_owner, change_phone, create, link_phone, may_text,
};
use askhava_core::api::{MIGRATOR, SystemClock};
use askhava_messaging::api::{Audience, BusinessEligibility, file_business_eligibility, switch_on};
use askhava_portals::api::{
    CookieJar, PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress, SignInMail,
    csrf_token, ensure_pre_session, signin_post, signin_request,
};
use askhava_privacy::api::{Vault, mailbox_for};
use askhava_server::business_contacts::ContactDirectory;
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
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
const TEXTS: &str = "/b/texts";
const BOX: &str = "name=\"consent\"";
const CHANGED: &str = "number while you had this page open.";

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x52), &key(0x62), &url).expect("vault")
}

/// A request from the same site, or from another site when `cross`.
fn request(method: Method, jar: &CookieJar, body: &str, cross: bool) -> Request<Body> {
    let csrf = csrf_token(jar).unwrap_or_default();
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let (site, origin) = if cross {
        ("cross-site", "https://elsewhere.example".to_owned())
    } else {
        ("same-origin", format!("https://{HOST}"))
    };
    Request::builder()
        .method(method)
        .uri(TEXTS)
        .header(header::HOST, HOST)
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("sec-fetch-site", site)
        .header(header::ORIGIN, origin)
        .body(Body::from(format!("csrf={csrf}{body}")))
        .expect("request")
}

/// The status, the `Cache-Control` header and the body of `request` on `app`.
async fn answer(app: &Router, request: Request<Body>) -> (u16, String, String) {
    let response = app.clone().oneshot(request).await.expect("answer");
    let (parts, body) = response.into_parts();
    let cache = parts.headers.get(header::CACHE_CONTROL);
    let cache = cache.and_then(|v| v.to_str().ok()).unwrap_or_default();
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    let text = String::from_utf8_lossy(&bytes).into_owned();
    (parts.status.as_u16(), cache.to_owned(), text)
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

/// A same-site `POST` to `path` on the sign-in router.
fn signin_request_to(path: &str, jar: &CookieJar, body: &str) -> Request<Body> {
    let mut r = request(Method::POST, jar, body, false);
    *r.uri_mut() = path.parse().expect("path");
    r
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
    let ask = signin_request_to(SIGNIN_ROUTE, &jar, "&email=person%40x.example");
    signin.clone().oneshot(ask).await.expect("asked");
    let token = (mail.sent.lock().unwrap_or_else(PoisonError::into_inner)).clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let done = signin.oneshot(signin_request_to(&path, &jar, "")).await;
    let done = done.expect("signed in");
    let cookies: Vec<&str> = (done.headers().get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next())
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let mut sent = HeaderMap::new();
    sent.insert(header::COOKIE, cookies.join("; ").parse().expect("cookies"));
    CookieJar::from_headers(&sent)
}

/// A fake business: its owner and a "Confirm and billing" member, each signed in, each with one
/// linked phone.
struct Team {
    owner: (Acting, CookieJar, Uuid),
    member: (Acting, CookieJar, Uuid),
}

async fn linked(db: &mut PgConnection, by: Acting, number: &str) -> Uuid {
    let number = SecretString::from(number.to_owned());
    let dir = ContactDirectory::new(vault());
    let line = link_phone(db, &dir, (by, false), &number, Timestamp::now()).await;
    line.expect("linked")
}

async fn team(pool: &PgPool) -> Team {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let owner_box = mailbox_for(&mut db, &vault(), "owner@x.example").await;
    let member_box = mailbox_for(&mut db, &vault(), "member@x.example").await;
    let (owner_box, member_box) = (owner_box.expect("mailbox"), member_box.expect("mailbox"));
    let business = create(&mut db, false, now).await.expect("business");
    let owner = add_owner(&mut db, business, owner_box, now).await;
    let owner = Acting {
        business,
        member: owner.expect("owner"),
    };
    let cards = Roles::new(true, true).expect("roles");
    let member = add_member(&mut db, owner, member_box, cards, now).await;
    let member = Acting {
        business,
        member: member.expect("member"),
    };
    let owner_line = linked(&mut db, owner, "+19285550123").await;
    let member_line = linked(&mut db, member, "+19285550167").await;
    drop(db);
    let to = |mailbox| SignInAddress { business, mailbox };
    Team {
        owner: (owner, signed_in(pool, to(owner_box)).await, owner_line),
        member: (member, signed_in(pool, to(member_box)).await, member_line),
    }
}

/// TXT-035, D-3882: business texting switched on after an allowed answer naming a campaign.
async fn texting_on(pool: &PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (owner, now) = (Uuid::from_u128(0x522), Timestamp::now());
    let allowed = BusinessEligibility {
        answered_on: "2026-11-04".parse().expect("date"),
        reference: "FAKE-T522-CASE".to_owned(),
        answer: "FAKE-T522 answer text as received.".to_owned(),
        allowed: true,
        campaign_reference: "FAKE-T522-CAMPAIGN".to_owned(),
    };
    let filed = file_business_eligibility(&mut db, owner, &allowed, now).await;
    filed.expect("filed");
    let on = switch_on(&mut db, Audience::Business, (owner, now)).await;
    assert!(on.expect("switched"), "business texting on");
}

fn app(pool: &PgPool) -> Router {
    build_router(
        pool.clone(),
        Router::new(),
        RouterParts::from((None, None, None, None)),
    )
}

async fn may(pool: &PgPool, business: Uuid, line: Uuid) -> bool {
    let mut db = pool.acquire().await.expect("db");
    may_text(&mut db, business, line).await.expect("read")
}

/// The box's form fields for `line` at number version `version`, ticked when `ticked`.
fn posted(line: Uuid, version: i32, ticked: bool) -> String {
    let tick = if ticked { "&consent=yes" } else { "" };
    format!("&phone={line}&number_version={version}{tick}")
}

/// TD-CON-35: a ticked box for the current version makes `may_text` true; the page then says
/// "Texts on"; signed out, the 401 with the sign-in link; every answer `no-store`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_35_a_ticked_box_turns_texts_on(pool: PgPool) {
    let (app, t) = (app(&pool), team(&pool).await);
    texting_on(&pool).await;
    let ((_, jar, line), business) = (&t.owner, t.owner.0.business);
    let (status, cache, page) = answer(&app, request(Method::GET, jar, "", false)).await;
    assert_eq!((status, cache.as_str()), (200, "no-store"));
    assert!(
        page.contains("Phone ending in 0123") && page.contains(BOX),
        "{page}"
    );
    assert!(
        page.contains("value=\"1\"") && !page.contains("0167"),
        "own phone only"
    );
    assert!(!may(&pool, business, *line).await);
    let body = posted(*line, 1, true);
    let (status, cache, page) = answer(&app, request(Method::POST, jar, &body, false)).await;
    assert_eq!((status, cache.as_str()), (200, "no-store"));
    assert!(
        page.contains("Texts are on for the phone ending in 0123"),
        "{page}"
    );
    assert!(may(&pool, business, *line).await, "the ticked box counts");
    let (_, _, page) = answer(&app, request(Method::GET, jar, "", false)).await;
    assert!(page.contains("Texts on") && !page.contains(BOX), "{page}");
    let out = answer(&app, request(Method::GET, &CookieJar::new(), "", false)).await;
    assert_eq!((out.0, out.1.as_str()), (401, "no-store"));
    assert!(out.2.contains("/b/sign-in"), "{}", out.2);
}

/// TD-CON-35: after `change_phone` the page shows the box again with the new digits, a box posted
/// for the old version is refused and names the new number, and the new version's box counts.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_35_a_box_for_the_old_number_is_refused(pool: PgPool) {
    let (app, t) = (app(&pool), team(&pool).await);
    texting_on(&pool).await;
    let ((owner, jar, line), business) = (&t.owner, t.owner.0.business);
    answer(
        &app,
        request(Method::POST, jar, &posted(*line, 1, true), false),
    )
    .await;
    let mut db = pool.acquire().await.expect("db");
    let dir = ContactDirectory::new(vault());
    let number = SecretString::from("+19285550145".to_owned());
    let changed = change_phone(&mut db, &dir, (*owner, *line), &number).await;
    changed.expect("changed");
    drop(db);
    assert!(
        !may(&pool, business, *line).await,
        "no box for the new number"
    );
    let (_, _, page) = answer(&app, request(Method::GET, jar, "", false)).await;
    assert!(
        page.contains("Phone ending in 0145") && page.contains(BOX),
        "{page}"
    );
    let old = posted(*line, 1, true);
    let (status, _, page) = answer(&app, request(Method::POST, jar, &old, false)).await;
    assert_eq!(status, 200);
    assert!(
        page.contains(CHANGED) && page.contains("It now ends in 0145"),
        "{page}"
    );
    assert!(!may(&pool, business, *line).await, "the old box is refused");
    let new = posted(*line, 2, true);
    answer(&app, request(Method::POST, jar, &new, false)).await;
    assert!(
        may(&pool, business, *line).await,
        "the new number's box counts"
    );
}

/// TD-CON-35: an unticked box (422), another member's phone, no phone field (200) and a
/// cross-site `POST` (403) store nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_35_unticked_foreign_and_cross_site_store_nothing(pool: PgPool) {
    let (app, t) = (app(&pool), team(&pool).await);
    texting_on(&pool).await;
    let ((_, jar, line), business) = (&t.owner, t.owner.0.business);
    let member_line = t.member.2;
    let unticked = posted(*line, 1, false);
    let (status, _, page) = answer(&app, request(Method::POST, jar, &unticked, false)).await;
    assert_eq!(status, 422);
    assert!(
        page.contains("<title>Error: ") && page.contains("Tick the box"),
        "{page}"
    );
    let foreign = posted(member_line, 1, true);
    let (status, _, _) = answer(&app, request(Method::POST, jar, &foreign, false)).await;
    assert_eq!(status, 200);
    let (status, _, page) = answer(&app, request(Method::POST, jar, "&consent=yes", false)).await;
    assert_eq!(status, 200);
    assert!(page.contains(BOX), "the page again");
    let ticked = posted(*line, 1, true);
    let (status, _, _) = answer(&app, request(Method::POST, jar, &ticked, true)).await;
    assert_eq!(status, 403);
    assert!(
        !may(&pool, business, *line).await,
        "nothing stored for the owner's phone"
    );
    assert!(
        !may(&pool, business, member_line).await,
        "nothing for the member's phone"
    );
    let (_, member_jar, _) = &t.member;
    let (_, _, page) = answer(&app, request(Method::GET, member_jar, "", false)).await;
    assert!(
        page.contains("ending in 0167") && !page.contains("0123"),
        "{page}"
    );
}

/// TD-CON-35, TXT-035: while business texting is off the page shows no box and a ticked `POST`
/// stores nothing.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_con_35_texting_off_shows_no_box_and_stores_nothing(pool: PgPool) {
    let (app, t) = (app(&pool), team(&pool).await);
    let ((_, jar, line), business) = (&t.owner, t.owner.0.business);
    let (status, cache, page) = answer(&app, request(Method::GET, jar, "", false)).await;
    assert_eq!((status, cache.as_str()), (200, "no-store"));
    assert!(
        page.contains("texting businesses right now") && !page.contains(BOX),
        "{page}"
    );
    let ticked = posted(*line, 1, true);
    let (status, _, page) = answer(&app, request(Method::POST, jar, &ticked, false)).await;
    assert_eq!(status, 200);
    assert!(
        page.contains("texting businesses right now") && !page.contains(BOX),
        "{page}"
    );
    assert!(
        !may(&pool, business, *line).await,
        "nothing stored while texting is off"
    );
    let brand = askhava_core::api::brand().display_name();
    let calls = format!("{brand} never places recorded or automated calls to you.");
    assert!(page.contains(&calls), "{page}");
}
