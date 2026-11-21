//! TD-SEC-64 (T565, D-2636, D-4045, D-3890; SEC-038, SEC-014, SEC-019): the role change, the
//! removal and the monthly limit served by the assembled router over PostgreSQL. Without a
//! step-up each is a 303 to the step-up page and changes nothing; with one recorded within 5
//! minutes (`businesses::api::record_step_up` directly) it changes once; one recorded 6 minutes
//! ago asks again; a member without the role gets the not-found page. This crate may name no
//! schema (TD-SEC-10) and `payments` has no limit reader, so the stored limit is checked in
//! `tests/e2e/business_step_up_served.spec.ts`; here its answers are. People sign in through
//! `portals`' emailed-link sign-in, as `business_step_up_code_route.rs` does. Every ID and address
//! is fake.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_businesses::api::{
    Acting, Roles, TeamMember, add_member, add_owner, create, members, record_step_up,
};
use askhava_core::api::{MIGRATOR, SystemClock};
use askhava_portals::api::{
    BILLING_ROUTE, CookieJar, FEE_LIMIT_ROUTE, PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE,
    SignIn, SignInAddress, SignInMail, TEAM_REMOVE_ROUTE, TEAM_ROLE_ROUTE, csrf_token,
    ensure_pre_session, signin_post, signin_request,
};
use askhava_privacy::api::{Vault, mailbox_for};
use askhava_server::{Hooks, RouterParts, build_router};
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
const ASKED: &str = "/b/step-up?return=%2Fb";
const NOT_FOUND: &str = "We can't find that page";

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x52), &key(0x62), &url).expect("vault")
}

fn app(pool: &PgPool) -> Router {
    let hooks: Hooks = (None, None, None, None);
    build_router(pool.clone(), Router::new(), RouterParts::from(hooks))
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
    let peer = SocketAddr::from(([203, 0, 113, 7], 40_000));
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

/// The status, the `Location`, the cookies set and the body text.
async fn answer(app: &Router, request: Request<Body>) -> (u16, String, Vec<String>, String) {
    let response = app.clone().oneshot(request).await.expect("answer");
    let (parts, body) = response.into_parts();
    let location = (parts.headers.get(header::LOCATION))
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let cookies = (parts.headers.get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next().map(str::to_owned))
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let bytes = to_bytes(body, 1 << 22).await.expect("body");
    let text = String::from_utf8_lossy(&bytes).into_owned();
    (parts.status.as_u16(), location, cookies, text)
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
    let (_, _, cookies, _) = answer(&signin, request(Method::POST, &path, &jar, "")).await;
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookies.join("; ").parse().expect("cookies"));
    CookieJar::from_headers(&headers)
}

/// A fake business: its owner and a "Cards and quotes" member, each signed in, with their IDs.
struct Team {
    business: Uuid,
    owner: (Uuid, CookieJar),
    member: (Uuid, CookieJar),
}

async fn team(pool: &PgPool) -> Team {
    let (mut db, now) = (pool.acquire().await.expect("db"), Timestamp::now());
    let owner_box = mailbox_for(&mut db, &vault(), "owner@x.example").await;
    let member_box = mailbox_for(&mut db, &vault(), "member@x.example").await;
    let (owner_box, member_box) = (owner_box.expect("mailbox"), member_box.expect("mailbox"));
    let business = create(&mut db, false, now).await.expect("business");
    let owner = add_owner(&mut db, business, owner_box, now).await;
    let owner = owner.expect("owner");
    let by = Acting {
        business,
        member: owner,
    };
    let cards = Roles::new(true, false).expect("roles");
    let member = add_member(&mut db, by, member_box, cards, now).await;
    let member = member.expect("member");
    drop(db);
    let to = |mailbox| SignInAddress { business, mailbox };
    Team {
        business,
        owner: (owner, signed_in(pool, to(owner_box)).await),
        member: (member, signed_in(pool, to(member_box)).await),
    }
}

/// Records a passed step-up for `member` `ago` before now.
async fn stepped_up(pool: &PgPool, (business, member): (Uuid, Uuid), ago: SignedDuration) {
    let mut db = pool.acquire().await.expect("db");
    let at = Timestamp::now().checked_sub(ago).expect("time");
    record_step_up(&mut db, Acting { business, member }, at)
        .await
        .expect("step-up");
}

async fn roster(pool: &PgPool, business: Uuid) -> Vec<TeamMember> {
    let mut db = pool.acquire().await.expect("db");
    members(&mut db, business).await.expect("members")
}

async fn post_form(
    app: &Router,
    (path, body): (&str, &str),
    jar: &CookieJar,
) -> (u16, String, String) {
    let (status, location, _, text) = answer(app, request(Method::POST, path, jar, body)).await;
    (status, location, text)
}

const FRESH: SignedDuration = SignedDuration::from_secs(30);
const STALE: SignedDuration = SignedDuration::from_mins(6);

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_64_a_role_change_needs_a_step_up_within_5_minutes(pool: PgPool) {
    let app = app(&pool);
    let t = team(&pool).await;
    let before = roster(&pool, t.business).await;
    let both = format!(
        "&member={}&cards_and_quotes=yes&confirm_and_billing=yes",
        t.member.0
    );
    let change = (TEAM_ROLE_ROUTE, both.as_str());
    let (status, to, _) = post_form(&app, change, &t.owner.1).await;
    assert_eq!(
        (status, to.as_str()),
        (303, ASKED),
        "no step-up: to the page"
    );
    assert_eq!(
        roster(&pool, t.business).await,
        before,
        "and nothing changed"
    );

    stepped_up(&pool, (t.business, t.owner.0), FRESH).await;
    for _ in 0..2 {
        let (status, to, _) = post_form(&app, change, &t.owner.1).await;
        assert_eq!((status, to.as_str()), (303, "/b"));
    }
    let after = roster(&pool, t.business).await;
    assert_eq!(after.len(), 2);
    assert_eq!(after[1].roles, Roles::BOTH, "the role changed, once");

    stepped_up(&pool, (t.business, t.owner.0), STALE).await;
    let cards = format!("&member={}&cards_and_quotes=yes", t.member.0);
    let (status, to, _) = post_form(&app, (TEAM_ROLE_ROUTE, &cards), &t.owner.1).await;
    assert_eq!(
        (status, to.as_str()),
        (303, ASKED),
        "6 minutes on: asked again"
    );
    assert_eq!(roster(&pool, t.business).await, after);

    stepped_up(&pool, (t.business, t.member.0), FRESH).await;
    let owner_cards = format!("&member={}&cards_and_quotes=yes", t.owner.0);
    let (status, _, page) = post_form(&app, (TEAM_ROLE_ROUTE, &owner_cards), &t.member.1).await;
    assert!(status == 404 && page.contains(NOT_FOUND), "{status} {page}");
    assert_eq!(roster(&pool, t.business).await, after);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_64_a_removal_needs_a_step_up_within_5_minutes(pool: PgPool) {
    let app = app(&pool);
    let t = team(&pool).await;
    let body = format!("&member={}", t.member.0);
    let change = (TEAM_REMOVE_ROUTE, body.as_str());

    stepped_up(&pool, (t.business, t.member.0), FRESH).await;
    let owner = format!("&member={}", t.owner.0);
    let (status, _, page) = post_form(&app, (TEAM_REMOVE_ROUTE, &owner), &t.member.1).await;
    assert!(status == 404 && page.contains(NOT_FOUND), "{status} {page}");

    let (status, to, _) = post_form(&app, change, &t.owner.1).await;
    assert_eq!(
        (status, to.as_str()),
        (303, ASKED),
        "no step-up: to the page"
    );
    stepped_up(&pool, (t.business, t.owner.0), STALE).await;
    let (status, to, _) = post_form(&app, change, &t.owner.1).await;
    assert_eq!((status, to.as_str()), (303, ASKED), "6 minutes on: asked");
    assert_eq!(roster(&pool, t.business).await.len(), 2, "nothing changed");

    stepped_up(&pool, (t.business, t.owner.0), FRESH).await;
    let (status, to, _) = post_form(&app, change, &t.owner.1).await;
    assert_eq!((status, to.as_str()), (303, "/b"));
    assert_eq!(roster(&pool, t.business).await.len(), 1, "removed");
    let (status, _, page) = post_form(&app, change, &t.owner.1).await;
    assert!(
        status == 404 && page.contains(NOT_FOUND),
        "removed once: {status}"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_64_a_monthly_limit_needs_a_step_up_within_5_minutes(pool: PgPool) {
    let app = app(&pool);
    let t = team(&pool).await;
    let change = (FEE_LIMIT_ROUTE, "&limit=300");
    let asked = "/b/step-up?return=%2Fb%2Fbilling";
    let (status, to, _) = post_form(&app, change, &t.owner.1).await;
    assert_eq!(
        (status, to.as_str()),
        (303, asked),
        "no step-up: to the page"
    );

    stepped_up(&pool, (t.business, t.member.0), FRESH).await;
    let (status, _, page) = post_form(&app, change, &t.member.1).await;
    assert!(
        status == 404 && page.contains(NOT_FOUND),
        "no billing role: {status}"
    );

    stepped_up(&pool, (t.business, t.owner.0), FRESH).await;
    let (status, to, _) = post_form(&app, change, &t.owner.1).await;
    assert_eq!((status, to.as_str()), (303, BILLING_ROUTE));

    stepped_up(&pool, (t.business, t.owner.0), STALE).await;
    let (status, to, _) = post_form(&app, (FEE_LIMIT_ROUTE, "&limit="), &t.owner.1).await;
    assert_eq!(
        (status, to.as_str()),
        (303, asked),
        "6 minutes on: asked again"
    );

    let bad = post_form(&app, (FEE_LIMIT_ROUTE, "&limit=0"), &t.owner.1).await;
    assert_eq!(bad.0, 422, "a zero limit changes nothing");
}
