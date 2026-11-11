//! The release notice's signed link opens business sign-in (TD-PICK-51; PICK-024, SEC-004;
//! D-1565, D-1761, T269).
//!
//! Signs links with `messaging`'s own `sign_in_link` (the URL the release email carries) and
//! drives the sign-in routes the way the composition serves them: `signin_page` on `GET`
//! `/b/sign-in`, `SignIn::with_device_codes` and `SignIn::with_notice_links`, over a fake Verify,
//! a fake address book and a fake code-address lookup. Every address, number, ID and code is fake.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_businesses::api::{Acting, Roles, add_member, add_owner, create};
use askhava_core::api::{IdSecret, ManualClock};
use askhava_messaging::api::sign_in_link;
use askhava_portals::api::{
    CodeAddress, CodeFuture, CodeTo, CookieJar, DeviceCodes, PortalsError, SESSION_COOKIE,
    SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress, SignInMail, business_principal,
    signin_open, signin_page, signin_post, signin_request,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, HeaderValue, Request, header};
use axum::routing::get;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgConnection;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const CODE: &str = "135790";
const JOB: i64 = 1042;
const OWNER_BOX: u128 = 0xb1;
const CREW_BOX: u128 = 0xb2;

/// A fake Verify: [`CODE`] is right for anyone.
struct FakeVerify;

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        _: &str,
        _: CodeChannel,
    ) -> Result<String, AdapterError> {
        Ok("VEfake".to_owned())
    }
    async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
        Ok(code == CODE)
    }
}

/// Every member's code goes by email to a fake address.
struct FakeCodeTo;

impl CodeAddress for FakeCodeTo {
    fn code_to<'a>(
        &'a self,
        _: &'a mut PgConnection,
        _: Uuid,
    ) -> CodeFuture<'a, Result<CodeTo, PortalsError>> {
        let to = CodeTo::Email(SecretString::from("crew@x.example".to_owned()));
        Box::pin(async move { Ok(to) })
    }
}

/// The address book (crew only) and the links "emailed", as paths.
struct FakeMail {
    crew: SignInAddress,
    sent: Mutex<Vec<String>>,
}

impl SignInMail for FakeMail {
    async fn addresses(
        &self,
        _: &mut PgConnection,
        address: &str,
    ) -> Result<Vec<SignInAddress>, PortalsError> {
        Ok(if address == "crew@x.example" {
            vec![self.crew]
        } else {
            Vec::new()
        })
    }
    async fn send_link(
        &self,
        _: &mut PgConnection,
        _: SignInAddress,
        link: &SecretString,
    ) -> Result<(), PortalsError> {
        let path = link.expose_secret().replace("https://askhava.com", "");
        let mut sent = self.sent.lock().unwrap_or_else(PoisonError::into_inner);
        sent.push(path);
        Ok(())
    }
}

fn start() -> Timestamp {
    "2026-11-07T17:00:00Z".parse().expect("time")
}

fn secret() -> IdSecret {
    let key = SecretString::from("fake-notice-secret-for-tests-0123456789".to_owned());
    IdSecret::new(1, key).expect("secret")
}

/// The notice link path of mailbox `person` for [`JOB`], ending at `expires`.
fn notice(person: u128, expires: Timestamp) -> String {
    let url = sign_in_link(&secret(), Uuid::from_u128(person), JOB, expires).expect("link");
    format!("/b/{}", url.split_once("/b/").expect("path").1)
}

async fn world(pool: &PgPool) -> (Router, Arc<FakeMail>, Uuid) {
    let mut db = pool.acquire().await.expect("db");
    let business = create(&mut db, false, start()).await.expect("business");
    let owner = add_owner(&mut db, business, Uuid::from_u128(OWNER_BOX), start());
    let member = owner.await.expect("owner");
    let by = Acting { business, member };
    let cards = Roles::new(true, false).expect("roles");
    let crew_box = Uuid::from_u128(CREW_BOX);
    add_member(&mut db, by, crew_box, cards, start())
        .await
        .expect("crew");
    let crew = SignInAddress {
        business,
        mailbox: crew_box,
    };
    let mail = Arc::new(FakeMail {
        crew,
        sent: Mutex::default(),
    });
    let key = SecretString::from("fake-key-for-tests-only-0123456789".to_owned());
    let codes = DeviceCodes::new(FakeVerify, FakeCodeTo, key);
    let clock = Arc::new(ManualClock::new(start()));
    let state = SignIn::new(pool.clone(), clock, Arc::clone(&mail))
        .with_device_codes(codes)
        .with_notice_links(secret());
    let routes = Router::new()
        .route(
            SIGNIN_ROUTE,
            get(signin_page::<FakeMail>).post(signin_request::<FakeMail>),
        )
        .route(
            SIGNIN_LINK_ROUTE,
            get(signin_open::<FakeMail>).post(signin_post::<FakeMail>),
        );
    (state.into_router(routes), mail, member)
}

/// One browser's cookies and the last redirect.
#[derive(Default)]
struct Browser {
    cookies: BTreeMap<String, String>,
    location: Option<String>,
}

impl Browser {
    fn jar(&self) -> CookieJar {
        let line: Vec<String> = self
            .cookies
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect();
        let mut headers = HeaderMap::new();
        if !line.is_empty() {
            let value = HeaderValue::from_str(&line.join("; ")).expect("cookie");
            headers.insert(header::COOKIE, value);
        }
        CookieJar::from_headers(&headers)
    }

    async fn send(&mut self, app: &Router, method: &str, uri: &str, form: &str) -> String {
        let line: Vec<String> = self
            .cookies
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect();
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
        for cookie in response.headers().get_all(header::SET_COOKIE) {
            let text = cookie.to_str().expect("ascii");
            let pair = text.split(';').next().unwrap_or_default();
            let (name, value) = pair.split_once('=').expect("pair");
            if value.is_empty() {
                self.cookies.remove(name);
            } else {
                self.cookies.insert(name.to_owned(), value.to_owned());
            }
        }
        let location = response.headers().get(header::LOCATION);
        self.location = location.map(|l| l.to_str().expect("ascii").to_owned());
        let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
        String::from_utf8_lossy(&body).into_owned()
    }

    /// Opens `uri` and posts its form to `action` with the page's CSRF token and `fields`.
    async fn press(&mut self, app: &Router, uri: &str, action: &str, fields: &str) -> String {
        let page = self.send(app, "GET", uri, "").await;
        let csrf = page
            .split("name=\"csrf\" value=\"")
            .nth(1)
            .and_then(|r| r.split('"').next());
        let form = format!("csrf={}{fields}", csrf.unwrap_or_default());
        self.send(app, "POST", action, &form).await
    }
}

/// The notice button's action for a link path.
fn button(link: &str) -> String {
    link.replace("/b/sign-in?", "/b/sign-in/notice?")
}

async fn sessions(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM portals.business_session")
        .fetch_one(pool)
        .await
        .expect("sessions")
}

fn last_link(mail: &FakeMail) -> Option<String> {
    let sent = mail.sent.lock().unwrap_or_else(PoisonError::into_inner);
    sent.last().cloned()
}

/// TD-PICK-51: the owner's 7-day link opens sign-in for the owner; the button only emails the
/// one-time link (no session, no job detail); that link and the new-device code sign the owner in
/// and the next page is the job page; opened again while signed in, it goes to the job page.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_51_a_signed_link_opens_sign_in_with_the_job_page_next(pool: PgPool) {
    let (app, mail, owner) = world(&pool).await;
    let link = notice(OWNER_BOX, start() + SignedDuration::from_hours(7 * 24));
    let mut b = Browser::default();
    let opened = b.send(&app, "GET", &link, "").await;
    assert!(opened.contains("action=\"/b/sign-in/notice?p="), "{opened}");
    assert!(!opened.contains("/b/jobs/") && !opened.contains("id=\"email\""));
    let asked = b.press(&app, &link, &button(&link), "").await;
    assert!(asked.contains("Check your email"), "{asked}");
    assert!(
        !b.cookies.contains_key(SESSION_COOKIE),
        "the link signs no one in"
    );
    assert_eq!(sessions(&pool).await, 0);
    let emailed = last_link(&mail).expect("a one-time link");
    assert!(emailed.ends_with(&format!("?job={JOB}")), "{emailed}");
    let code = b.press(&app, &emailed, &emailed, "").await;
    assert!(
        code.contains(&format!("?job={JOB}\"")),
        "the code step keeps the job: {code}"
    );
    assert_eq!(sessions(&pool).await, 0, "no session before the code");
    let field = format!("&code={CODE}");
    b.press(&app, SIGNIN_ROUTE, &emailed, &field).await;
    assert_eq!(
        b.location.as_deref(),
        Some("/b/jobs/1042"),
        "the job page is next"
    );
    let mut db = pool.acquire().await.expect("db");
    let me = business_principal(&mut db, &b.jar(), start()).await;
    assert_eq!(me.expect("read").map(|p| p.member()), Some(owner));
    b.send(&app, "GET", &link, "").await;
    assert_eq!(b.location.as_deref(), Some("/b/jobs/1042"));
}

/// TD-PICK-51: a changed job, person or expiry, or an expired link, opens plain sign-in on `GET`
/// and on the button's `POST`, emails nothing and starts no session.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_51_a_changed_or_expired_link_opens_plain_sign_in(pool: PgPool) {
    let (app, mail, _) = world(&pool).await;
    let link = notice(OWNER_BOX, start() + SignedDuration::from_hours(7 * 24));
    let owner = Uuid::from_u128(OWNER_BOX).to_string();
    let crew = Uuid::from_u128(CREW_BOX).to_string();
    let e = (start() + SignedDuration::from_hours(7 * 24)).as_second();
    let bad = [
        link.replace(&format!("j={JOB}"), "j=1043"),
        link.replace(&owner, &crew),
        link.replace(&format!("e={e}"), &format!("e={}", e + 1)),
        notice(OWNER_BOX, start()),
        link.replace("&t=", "&t=0"),
    ];
    for link in bad {
        let mut b = Browser::default();
        let opened = b.send(&app, "GET", &link, "").await;
        assert!(
            opened.contains("id=\"email\"") && !opened.contains("notice?"),
            "{link}"
        );
        let posted = b.press(&app, &link, &button(&link), "").await;
        assert!(posted.contains("id=\"email\""), "{link}");
        assert!(!b.cookies.contains_key(SESSION_COOKIE));
    }
    assert_eq!(last_link(&mail), None, "no link emailed");
    assert_eq!(sessions(&pool).await, 0);
}

/// TD-PICK-51: the owner's link opened where the crew member is signed in opens plain sign-in and
/// changes neither the session nor who is signed in.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_51_another_signed_in_person_gets_plain_sign_in(pool: PgPool) {
    let (app, mail, owner) = world(&pool).await;
    let mut b = Browser::default();
    let ask = "&email=crew@x.example";
    b.press(&app, SIGNIN_ROUTE, SIGNIN_ROUTE, ask).await;
    let emailed = last_link(&mail).expect("crew link");
    b.press(&app, &emailed, &emailed, "").await;
    let done = b
        .press(&app, SIGNIN_ROUTE, &emailed, &format!("&code={CODE}"))
        .await;
    assert!(done.contains("You're signed in"), "{done}");
    let link = notice(OWNER_BOX, start() + SignedDuration::from_hours(7 * 24));
    let opened = b.send(&app, "GET", &link, "").await;
    assert!(
        opened.contains("id=\"email\"") && b.location.is_none(),
        "{opened}"
    );
    let posted = b.press(&app, &link, &button(&link), "").await;
    assert!(posted.contains("id=\"email\"") && b.location.is_none());
    assert_eq!(
        last_link(&mail),
        Some(emailed),
        "nothing emailed to the owner"
    );
    let mut db = pool.acquire().await.expect("db");
    let me = business_principal(&mut db, &b.jar(), start())
        .await
        .expect("read");
    let who = me.map(|p| p.member());
    assert!(who.is_some() && who != Some(owner), "still the crew member");
    assert_eq!(sessions(&pool).await, 1);
}
