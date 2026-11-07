//! The new-device code and per-person team sign-in (TD-BIZ-06 sign-in cases, TD-TXT-19 sign-in
//! case; BIZ-023, SEC-004, TXT-038, D-366, D-1773 to D-1775).
//!
//! Drives the business sign-in routes with `SignIn::with_device_codes` over a fake Verify that
//! records every send, a fake address book and a fake code-address lookup: the owner has a phone
//! that gets texts, the "Cards and quotes" member has none. No campaign is registered and nothing
//! here sends through the outbox. Every address, number, ID and code is fake.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use askhava_adapters::api::{AdapterError, CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_businesses::api::{Acting, Roles, add_member, add_owner, create, remove_member};
use askhava_core::api::ManualClock;
use askhava_portals::api::{
    CODE_TRIES, CodeAddress, CodeFuture, CodeTo, CookieJar, DEVICE_COOKIE, DeviceCodes,
    PortalsError, SIGNIN_LINK_ROUTE, SIGNIN_ROUTE, SignIn, SignInAddress, SignInMail,
    business_principal, signin_form, signin_open, signin_post, signin_request,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, HeaderValue, Request, header};
use axum::routing::get;
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const CODE: &str = "246810";
const OWNER_PHONE: &str = "+15005550101";

type Sends = Arc<Mutex<Vec<(String, CodeChannel)>>>;

/// A fake Verify: records each send and, like Twilio, ignores the idempotency key, so a second
/// send with the same key is a second code (D-1776); [`CODE`] is right for any contact.
struct FakeVerify(Sends);

impl VerifyAdapter for FakeVerify {
    async fn send_code(
        &self,
        _: &IdempotencyKey,
        to: &str,
        channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        let mut sent = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        sent.push((to.to_owned(), channel));
        Ok("VEfake".to_owned())
    }
    async fn check_code(&self, _: &str, code: &str) -> Result<bool, AdapterError> {
        Ok(code == CODE)
    }
}

/// The owner has a phone that gets texts; every other member gets the code by email.
struct FakeCodeTo(Uuid);

impl CodeAddress for FakeCodeTo {
    fn code_to<'a>(
        &'a self,
        _: &'a mut PgConnection,
        member: Uuid,
    ) -> CodeFuture<'a, Result<CodeTo, PortalsError>> {
        let to = if member == self.0 {
            CodeTo::Phone(SecretString::from(OWNER_PHONE.to_owned()))
        } else {
            CodeTo::Email(SecretString::from("crew@x.example".to_owned()))
        };
        Box::pin(async move { Ok(to) })
    }
}

/// The address book: owner and crew of one business; the links "emailed".
struct FakeMail {
    book: Vec<(&'static str, SignInAddress)>,
    sent: Mutex<Vec<String>>,
}

impl SignInMail for FakeMail {
    async fn addresses(
        &self,
        _: &mut PgConnection,
        address: &str,
    ) -> Result<Vec<SignInAddress>, PortalsError> {
        let found = self.book.iter().filter(|(a, _)| *a == address);
        Ok(found.map(|(_, to)| *to).collect())
    }
    async fn send_link(
        &self,
        _: &mut PgConnection,
        _: SignInAddress,
        link: &SecretString,
    ) -> Result<(), PortalsError> {
        let path = link.expose_secret().replace("https://askhava.com", "");
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(path);
        Ok(())
    }
}

struct World {
    business: Uuid,
    owner: Uuid,
    crew: Uuid,
    app: Router,
    mail: Arc<FakeMail>,
    sends: Sends,
}

fn start() -> Timestamp {
    "2026-11-07T17:00:00Z".parse().expect("time")
}

async fn world(pool: &PgPool) -> World {
    let mut db = pool.acquire().await.expect("db");
    let business = create(&mut db, false, start()).await.expect("business");
    let (box_a, box_b) = (Uuid::from_u128(0xb1), Uuid::from_u128(0xb2));
    let owner = add_owner(&mut db, business, box_a, start())
        .await
        .expect("owner");
    let by = Acting {
        business,
        member: owner,
    };
    let cards = Roles::new(true, false).expect("roles");
    let crew = add_member(&mut db, by, box_b, cards, start())
        .await
        .expect("crew");
    let at = |mailbox| SignInAddress { business, mailbox };
    let mail = Arc::new(FakeMail {
        book: vec![
            ("owner@x.example", at(box_a)),
            ("crew@x.example", at(box_b)),
        ],
        sent: Mutex::default(),
    });
    let sends = Sends::default();
    let key = SecretString::from("fake-key-for-tests-only-0123456789".to_owned());
    let codes = DeviceCodes::new(FakeVerify(Arc::clone(&sends)), FakeCodeTo(owner), key);
    let clock = Arc::new(ManualClock::new(start()));
    let state = SignIn::new(pool.clone(), clock, Arc::clone(&mail)).with_device_codes(codes);
    let routes = Router::new()
        .route(
            SIGNIN_ROUTE,
            get(signin_form).post(signin_request::<FakeMail>),
        )
        .route(
            SIGNIN_LINK_ROUTE,
            get(signin_open::<FakeMail>).post(signin_post::<FakeMail>),
        );
    let app = state.into_router(routes);
    World {
        business,
        owner,
        crew,
        app,
        mail,
        sends,
    }
}

/// One browser's cookies.
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

    async fn send(&mut self, app: &Router, method: &str, uri: &str, form: &str) -> String {
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
        for cookie in response.headers().get_all(header::SET_COOKIE) {
            let pair = cookie
                .to_str()
                .expect("ascii")
                .split(';')
                .next()
                .unwrap_or_default();
            let (name, value) = pair.split_once('=').expect("pair");
            if value.is_empty() {
                self.0.remove(name);
            } else {
                self.0.insert(name.to_owned(), value.to_owned());
            }
        }
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

/// Asks for a sign-in link for `address`; returns its path.
async fn link_for(w: &World, address: &str) -> String {
    let field = format!("&email={address}");
    Browser::default()
        .press(&w.app, SIGNIN_ROUTE, SIGNIN_ROUTE, &field)
        .await;
    let sent = w.mail.sent.lock().unwrap_or_else(PoisonError::into_inner);
    sent.last().expect("a link").clone()
}

fn sends(w: &World) -> Vec<(String, CodeChannel)> {
    w.sends
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// TD-BIZ-06, TD-TXT-19: the crew member signs in as themself; with no phone that gets texts the
/// code goes by email to their own address, never to the owner's phone and never by a call; only
/// the right code signs in; the device is then known; removing the member signs them out.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_biz_06_member_gets_own_code_by_email_and_signs_in_as_themself(pool: PgPool) {
    let w = world(&pool).await;
    let link = link_for(&w, "crew@x.example").await;
    let mut phone = Browser::default();
    let asked = phone.press(&w.app, &link, &link, "").await;
    assert!(asked.contains("autocomplete=\"one-time-code\"") && asked.contains("We emailed"));
    assert_eq!(
        sends(&w),
        vec![("crew@x.example".to_owned(), CodeChannel::Email)]
    );
    let wrong = phone
        .press(&w.app, SIGNIN_ROUTE, &link, "&code=111111")
        .await;
    assert!(
        wrong.contains("Error: That code didn't work"),
        "a wrong code signs no one in"
    );
    let done = phone
        .press(&w.app, SIGNIN_ROUTE, &link, &format!("&code={CODE}"))
        .await;
    assert!(done.contains("You're signed in"));
    assert!(
        phone.0.contains_key(DEVICE_COOKIE),
        "the device is now known"
    );
    let mut db = pool.acquire().await.expect("db");
    let me = business_principal(&mut db, &phone.jar(), start())
        .await
        .expect("read");
    assert_eq!(
        me.map(|p| p.member()),
        Some(w.crew),
        "signed in as the crew member"
    );
    let again = link_for(&w, "crew@x.example").await;
    let known = phone.press(&w.app, &again, &again, "").await;
    assert!(
        known.contains("You're signed in"),
        "a known device needs no code"
    );
    assert_eq!(sends(&w).len(), 1, "no second code");
    let outbox: i64 = sqlx::query_scalar("SELECT count(*) FROM messaging.outbox")
        .fetch_one(&mut *db)
        .await
        .expect("outbox");
    assert_eq!(outbox, 0, "TD-TXT-19: no code ever goes through the outbox");
    let by = Acting {
        business: w.business,
        member: w.owner,
    };
    remove_member(&mut db, by, w.crew, start())
        .await
        .expect("removed");
    let gone = business_principal(&mut db, &phone.jar(), start())
        .await
        .expect("read");
    assert!(
        gone.is_none(),
        "removing the member ends their session at once"
    );
}

/// TD-BIZ-06, TD-TXT-19: the owner's code goes by text to the owner's own phone; another
/// member's known device and a used emailed link never skip the owner's code (D-1773).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_19_owner_code_by_text_and_devices_are_per_person(pool: PgPool) {
    let w = world(&pool).await;
    let crew_link = link_for(&w, "crew@x.example").await;
    let mut shared = Browser::default();
    shared.press(&w.app, &crew_link, &crew_link, "").await;
    let code = format!("&code={CODE}");
    shared.press(&w.app, SIGNIN_ROUTE, &crew_link, &code).await;
    let link = link_for(&w, "owner@x.example").await;
    let asked = shared.press(&w.app, &link, &link, "").await;
    assert!(
        asked.contains("We texted"),
        "the crew's device is not the owner's"
    );
    let mut forged = Browser::default();
    let used = crew_link.trim_start_matches("/b/sign-in/").to_owned();
    forged.0.insert(DEVICE_COOKIE.to_owned(), used);
    assert!(
        forged
            .press(&w.app, &link, &link, "")
            .await
            .contains("We texted")
    );
    let sent = sends(&w);
    assert_eq!(
        sent.len(),
        2,
        "one code per link, however many browsers press"
    );
    assert_eq!(sent[1], (OWNER_PHONE.to_owned(), CodeChannel::Sms));
    assert!(
        sent.iter().all(|(_, c)| *c != CodeChannel::Call),
        "never a voice code"
    );
    let done = shared.press(&w.app, SIGNIN_ROUTE, &link, &code).await;
    assert!(done.contains("You're signed in"));
    let mut db = pool.acquire().await.expect("db");
    let me = business_principal(&mut db, &shared.jar(), start())
        .await
        .expect("read");
    assert_eq!(me.map(|p| p.member()), Some(w.owner));
}

/// D-1776, D-1777: pressing again never sends a second code; each wrong code is an
/// `auth_failed` event naming the link; after `CODE_TRIES` wrong codes even the right code is
/// refused, the link has stopped working and no one is signed in.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn one_code_per_link_and_wrong_codes_are_capped(pool: PgPool) {
    let w = world(&pool).await;
    let link = link_for(&w, "crew@x.example").await;
    let mut phone = Browser::default();
    phone.press(&w.app, &link, &link, "").await;
    let again = phone.press(&w.app, &link, &link, "").await;
    assert!(again.contains("We emailed"), "the code field again");
    assert_eq!(sends(&w).len(), 1, "pressing again sends no second code");
    for _ in 0..CODE_TRIES {
        let wrong = phone
            .press(&w.app, SIGNIN_ROUTE, &link, "&code=111111")
            .await;
        assert!(wrong.contains("Error: That code didn't work"));
    }
    let mut db = pool.acquire().await.expect("db");
    let named: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM portals.security_event e JOIN portals.login_link l \
         ON l.id = e.actor_ref WHERE e.kind = 'auth_failed' AND l.member_id = $1",
    )
    .bind(w.crew)
    .fetch_one(&mut *db)
    .await
    .expect("events");
    assert_eq!(
        named, CODE_TRIES,
        "each wrong code is an event naming the link"
    );
    let late = phone
        .press(&w.app, SIGNIN_ROUTE, &link, &format!("&code={CODE}"))
        .await;
    assert!(late.contains("This sign-in link has stopped working"));
    assert!(!phone.0.contains_key(DEVICE_COOKIE), "no device remembered");
    let me = business_principal(&mut db, &phone.jar(), start())
        .await
        .expect("read");
    assert!(me.is_none(), "the capped link signs no one in");
    let reopened = Browser::default().send(&w.app, "GET", &link, "").await;
    assert!(reopened.contains("This sign-in link has stopped working"));
}
