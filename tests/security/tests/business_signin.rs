//! Business sign-in by a single-use 15-minute emailed link, the 30-day session and the business
//! principal routes are scoped by (TD-SEC-01 business cases, TD-A11Y-02 sign-in form case,
//! TD-CALL-07 sessions case; SEC-004, SEC-006, SEC-019, SEC-024, SEC-026, A11Y-032, PICK-033).
//!
//! Drives `business_signin_routes` over a fake `SignInMail` that keeps the links it was handed.
//! The sign-in page has no code field (the new-device code is T194's), so A11Y-032 has no case
//! here. Every address, ID and token is fake.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_businesses::api::{Acting, Roles, add_member, add_owner, create, remove_member};
use askhava_core::api::{Claimed, ManualClock};
use askhava_portals::api::{
    BUSINESS_SESSION_LIFE, CookieJar, LINK_LIFE, LINKS_PER_LIFE, Need, PortalsError, Realm,
    SignInAddress, SignInMail, business_principal, business_signin_routes, may_confirm,
    run_member_removed, start_session,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, HeaderValue, Request, header};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.test";

/// The fake address book and the links "emailed".
#[derive(Default)]
struct FakeMail {
    book: Vec<(&'static str, SignInAddress)>,
    sent: Mutex<Vec<(SignInAddress, String)>>,
}

impl FakeMail {
    fn links(&self) -> Vec<(SignInAddress, String)> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl SignInMail for FakeMail {
    async fn addresses(
        &self,
        _db: &mut PgConnection,
        address: &str,
    ) -> Result<Vec<SignInAddress>, PortalsError> {
        let found = self.book.iter().filter(|(a, _)| *a == address);
        Ok(found.map(|(_, to)| *to).collect())
    }

    async fn send_link(
        &self,
        _db: &mut PgConnection,
        to: SignInAddress,
        link: &SecretString,
    ) -> Result<(), PortalsError> {
        let path = link.expose_secret().replace("https://askhava.com", "");
        let mut sent = self.sent.lock().unwrap_or_else(PoisonError::into_inner);
        sent.push((to, path));
        Ok(())
    }
}

/// Two businesses: X with its owner and a "Cards and quotes" member, Y with its owner.
struct World {
    x: Uuid,
    y: Uuid,
    owner_x: Uuid,
    cards_x: Uuid,
    app: Router,
    mail: Arc<FakeMail>,
    clock: Arc<ManualClock>,
}

fn start() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("time")
}

async fn world(pool: &PgPool) -> World {
    let mut db = pool.acquire().await.expect("db");
    let x = create(&mut db, false, start()).await.expect("x");
    let y = create(&mut db, false, start()).await.expect("y");
    let (box_a, box_b, box_c) = (
        Uuid::from_u128(0xa1),
        Uuid::from_u128(0xa2),
        Uuid::from_u128(0xa3),
    );
    let owner_x = add_owner(&mut db, x, box_a, start())
        .await
        .expect("owner x");
    add_owner(&mut db, y, box_c, start())
        .await
        .expect("owner y");
    let by = Acting {
        business: x,
        member: owner_x,
    };
    let cards = Roles::new(true, false).expect("roles");
    let cards_x = add_member(&mut db, by, box_b, cards, start())
        .await
        .expect("member");
    let at = |business, mailbox| SignInAddress { business, mailbox };
    let mail = Arc::new(FakeMail {
        book: vec![
            ("owner@x.example", at(x, box_a)),
            ("crew@x.example", at(x, box_b)),
            ("owner@y.example", at(y, box_c)),
        ],
        sent: Mutex::default(),
    });
    let clock = Arc::new(ManualClock::new(start()));
    let app = business_signin_routes(pool.clone(), clock.clone(), Arc::clone(&mail));
    World {
        x,
        y,
        owner_x,
        cards_x,
        app,
        mail,
        clock,
    }
}

/// One browser: its cookies, kept from each `Set-Cookie`.
#[derive(Default)]
struct Browser(BTreeMap<String, String>);

impl Browser {
    fn jar(&self) -> CookieJar {
        let line: Vec<String> = self.0.iter().map(|(n, v)| format!("{n}={v}")).collect();
        let mut headers = HeaderMap::new();
        if !line.is_empty() {
            let value = HeaderValue::from_str(&line.join("; ")).expect("cookie");
            headers.insert(header::COOKIE, value);
        }
        CookieJar::from_headers(&headers)
    }

    /// Sends one request; returns the body and every `Set-Cookie` line.
    async fn send(
        &mut self,
        app: &Router,
        method: &str,
        uri: &str,
        form: &str,
    ) -> (String, Vec<String>) {
        let line: Vec<String> = self.0.iter().map(|(n, v)| format!("{n}={v}")).collect();
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, format!("https://{HOST}"))
            .header("sec-fetch-site", "same-origin")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, line.join("; "))
            .body(Body::from(form.to_owned()))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("response");
        let store = response.headers().get(header::CACHE_CONTROL);
        assert_eq!(store.and_then(|v| v.to_str().ok()), Some("no-store"));
        let cookies: Vec<String> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().expect("ascii").to_owned())
            .collect();
        for cookie in &cookies {
            let pair = cookie.split(';').next().unwrap_or_default();
            let (name, value) = pair.split_once('=').expect("pair");
            if value.is_empty() {
                self.0.remove(name);
            } else {
                self.0.insert(name.to_owned(), value.to_owned());
            }
        }
        let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
        (String::from_utf8_lossy(&body).into_owned(), cookies)
    }

    /// Opens `uri` and presses its form with the page's CSRF token and `fields`.
    async fn press(
        &mut self,
        app: &Router,
        uri: &str,
        action: &str,
        fields: &str,
    ) -> (String, Vec<String>) {
        let (page, _) = self.send(app, "GET", uri, "").await;
        let csrf = page
            .split("name=\"csrf\" value=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_default()
            .to_owned();
        self.send(app, "POST", action, &format!("csrf={csrf}{fields}"))
            .await
    }
}

/// Asks for a link for `address` and returns the paths emailed since.
async fn ask(w: &World, address: &str) -> String {
    let before = w.mail.links().len();
    let (page, _) = Browser::default()
        .press(
            &w.app,
            "/b/sign-in",
            "/b/sign-in",
            &format!("&email={address}"),
        )
        .await;
    assert!(
        page.contains("Check your email"),
        "same reply for every address"
    );
    w.mail.links()[before..]
        .iter()
        .map(|(_, p)| p.clone())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Presses "Sign in" on `link` in `browser`, with a CSRF token from the form page, so a used or
/// expired link (whose page has no button) is posted too; returns the body and `Set-Cookie` lines.
async fn use_link(w: &World, browser: &mut Browser, link: &str) -> (String, Vec<String>) {
    browser.press(&w.app, "/b/sign-in", link, "").await
}

/// TD-SEC-01: the link works once, opening it uses nothing, and the session lasts 30 days in the
/// business realm, scoped to the member's own business.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_01_link_is_single_use_and_signs_in_for_30_days(pool: PgPool) {
    let w = world(&pool).await;
    let link = ask(&w, "owner@x.example").await;
    let mut browser = Browser::default();
    let (opened, _) = browser.send(&w.app, "GET", &link, "").await;
    assert!(
        opened.contains("signin-button"),
        "a GET only shows the button"
    );
    let (done, cookies) = use_link(&w, &mut browser, &link).await;
    assert!(done.contains("You're signed in"));
    let age = format!("Max-Age={}", BUSINESS_SESSION_LIFE.as_secs());
    assert!(
        cookies
            .iter()
            .any(|c| c.starts_with("__Host-askhava-session=") && c.contains(&age))
    );
    let mut db = pool.acquire().await.expect("db");
    let who = business_principal(&mut db, &browser.jar(), start())
        .await
        .expect("read");
    let who = who.expect("signed in");
    assert_eq!((who.member(), who.business()), (w.owner_x, w.x));
    assert!(who.may(w.x, Need::ConfirmAndBilling) && who.may(w.x, Need::CardsAndQuotes));
    assert!(
        !who.may(w.y, Need::Member),
        "never another business (SEC-019)"
    );
    let late = start().checked_add(BUSINESS_SESSION_LIFE).expect("time");
    let gone = business_principal(&mut db, &browser.jar(), late)
        .await
        .expect("read");
    assert!(gone.is_none(), "30 days, not more");
    let (reopened, _) = Browser::default().send(&w.app, "GET", &link, "").await;
    assert!(reopened.contains("stopped working") && !reopened.contains("signin-button"));
    let (again, _) = use_link(&w, &mut Browser::default(), &link).await;
    assert!(again.contains("stopped working"), "single use");
    let failed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM portals.security_event WHERE kind = 'auth_failed' \
         AND route = '/b/sign-in/{token}'",
    )
    .fetch_one(&mut *db)
    .await
    .expect("events");
    assert_eq!(failed, 1);
}

/// TD-SEC-01: a link works up to 15 minutes and not at 15 minutes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_01_link_stops_at_15_minutes(pool: PgPool) {
    let w = world(&pool).await;
    let first = ask(&w, "owner@x.example").await;
    let second = ask(&w, "owner@x.example").await;
    w.clock.advance(
        LINK_LIFE
            .checked_sub(SignedDuration::from_secs(1))
            .expect("d"),
    );
    let (ok, _) = use_link(&w, &mut Browser::default(), &first).await;
    assert!(ok.contains("You're signed in"));
    w.clock.advance(SignedDuration::from_secs(1));
    let (late, _) = use_link(&w, &mut Browser::default(), &second).await;
    assert!(late.contains("stopped working"));
}

/// TD-SEC-01: an address on file for no one gets the same reply and no email; at most
/// `LINKS_PER_LIFE` links go to one member in 15 minutes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_01_no_account_hint_and_links_are_capped(pool: PgPool) {
    let w = world(&pool).await;
    assert!(ask(&w, "nobody@z.example").await.is_empty());
    for _ in 0..=LINKS_PER_LIFE {
        ask(&w, "crew@x.example").await;
    }
    let cap = usize::try_from(LINKS_PER_LIFE).expect("cap");
    assert_eq!(w.mail.links().len(), cap);
}

/// TD-SEC-01, PICK-033: a "Cards and quotes" member signs in as their own person and may not
/// Confirm; an owner session is never a business principal (SEC-006).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_01_roles_and_realms_scope_every_route(pool: PgPool) {
    let w = world(&pool).await;
    let link = ask(&w, "crew@x.example").await;
    let mut browser = Browser::default();
    use_link(&w, &mut browser, &link).await;
    let mut db = pool.acquire().await.expect("db");
    let who = business_principal(&mut db, &browser.jar(), start())
        .await
        .expect("read");
    let who = who.expect("signed in");
    assert_eq!(who.member(), w.cards_x);
    assert!(who.may(w.x, Need::CardsAndQuotes));
    assert!(!who.may(w.x, Need::ConfirmAndBilling));
    assert!(!may_confirm(Some(&who), w.x));
    let owner = start_session(
        &mut db,
        CookieJar::new(),
        (Realm::Owner, w.owner_x),
        SignedDuration::from_hours(12),
        start(),
    )
    .await
    .expect("owner session");
    let none = business_principal(&mut db, &owner, start())
        .await
        .expect("read");
    assert!(none.is_none(), "the owner realm is separate");
}

/// TD-CALL-07, TD-BIZ-06's sessions case (SEC-004, SEC-024): removing a member ends their
/// sessions at once and voids their unused links; the handler is safe to run twice.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_call_07_removing_a_member_ends_their_sessions(pool: PgPool) {
    let w = world(&pool).await;
    let used = ask(&w, "crew@x.example").await;
    let unused = ask(&w, "crew@x.example").await;
    let mut browser = Browser::default();
    use_link(&w, &mut browser, &used).await;
    let mut db = pool.acquire().await.expect("db");
    let by = Acting {
        business: w.x,
        member: w.owner_x,
    };
    remove_member(&mut db, by, w.cards_x, start())
        .await
        .expect("removed");
    let gone = business_principal(&mut db, &browser.jar(), start())
        .await
        .expect("read");
    assert!(gone.is_none(), "a removed member is signed out at once");
    let payload = serde_json::json!({"business": w.x, "member": w.cards_x});
    for id in [1, 2] {
        let task = Claimed {
            id,
            kind: "businesses.member_removed".to_owned(),
            payload: payload.clone(),
            attempts: 1,
        };
        run_member_removed(pool.clone(), task)
            .await
            .expect("handled");
    }
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM portals.session WHERE actor_id = $1")
        .bind(w.cards_x)
        .fetch_one(&mut *db)
        .await
        .expect("count");
    assert_eq!(left, 0);
    let (late, _) = use_link(&w, &mut Browser::default(), &unused).await;
    assert!(late.contains("stopped working"));
}

/// TD-A11Y-02: a failed submit is titled "Error: ", puts a focused summary first, keeps the value
/// and never disables the button.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_a11y_02_a_failed_submit_keeps_the_value(pool: PgPool) {
    let w = world(&pool).await;
    let (page, _) = Browser::default()
        .press(&w.app, "/b/sign-in", "/b/sign-in", "&email=not-an-address")
        .await;
    assert!(page.contains("<title>Error: Sign in to your business account"));
    let main = page.split("<main").nth(1).expect("main");
    let summary = main.find("id=\"error-summary\"").expect("summary");
    assert!(summary < main.find("<form").expect("form"));
    assert!(page.contains("value=\"not-an-address\"") && page.contains("autocomplete=\"email\""));
    assert!(!page.contains("disabled"));
    assert!(w.mail.links().is_empty());
}
