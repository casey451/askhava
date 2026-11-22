//! TD-SEC-65: the owner's waiting-phones page through the assembled router (T397; BIZ-033,
//! BIZ-038, SEC-038, SEC-004, SEC-042, SEC-048; D-3081, D-3744, D-3889).
//!
//! `server::build_router` over a real test database, with the vault the page reads the linking
//! member's sign-in address from (`RouterParts::with_vault`), and once without it (the fixed
//! line). Each person signs in through the sign-in link emailed to their own fake mailbox (as
//! `approve_link_route.rs` does); step-ups are recorded with `record_step_up`; every lookup goes
//! through `messaging::api::sender_business`. Fake numbers (555-01xx), fake addresses and fake
//! IDs only.

use std::sync::{Arc, Mutex, PoisonError};

use askhava_businesses::api::{
    Acting, Roles, SenderLink, TeamError, add_member, add_owner, change_phone_at, create,
    link_phone, phone_standing, record_step_up, waiting_phones,
};
use askhava_core::api::SystemClock;
use askhava_messaging::api::{OutboxError, sender_add, sender_business};
use askhava_portals::api::{
    CookieJar, PortalsError, SIGNIN_LINK_ROUTE, SignIn, SignInAddress, SignInMail, csrf_token,
    ensure_pre_session, send_sign_in_link, signin_post,
};
use askhava_privacy::api::{Vault, mailbox_for};
use askhava_server::{Hooks, RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Method, Request, header};
use axum::routing::post;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const HOST: &str = "askhava.example";
const PHONES: &str = "/b/phones";
const APPROVE: &str = "/b/phones/approve";
const REJECT: &str = "/b/phones/reject";
const CHANGED: &str = "A teammate changed this phone's number after you opened this page.";

/// A fake vault with fixed fake keys; every call builds the same keys.
fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x57), &key(0x67), &url).expect("vault")
}

/// The sender directory over the fake vault, as the composition links phones.
struct Directory(Vault);

impl SenderLink for Directory {
    async fn link(
        &self,
        db: &mut PgConnection,
        business_line: (Uuid, Uuid),
        number: &SecretString,
    ) -> Result<(), TeamError> {
        let added = sender_add(db, &self.0, business_line, number.expose_secret()).await;
        added.map_err(|e| match e {
            OutboxError::SenderTaken => TeamError::PhoneTaken,
            _ => TeamError::Database,
        })
    }
}

/// One business: its owner and a "Confirm and billing" member holding both roles.
#[derive(Clone, Copy)]
struct Team {
    owner: Acting,
    member: Acting,
    owner_box: Uuid,
    member_box: Uuid,
}

struct World {
    app: Router,
    pool: PgPool,
    dir: Directory,
    team: Team,
}

fn ago(minutes: i64) -> Timestamp {
    Timestamp::now() - SignedDuration::from_mins(minutes)
}

/// A business whose owner and member sign in at fake addresses tagged `tag`.
async fn team(db: &mut PgConnection, tag: &str) -> Team {
    let business = create(db, false, ago(60)).await.expect("business");
    let mailbox = |who: &str| format!("{who}.{tag}@example.com");
    let owner_box = mailbox_for(db, &vault(), &mailbox("owner")).await;
    let member_box = mailbox_for(db, &vault(), &mailbox("member")).await;
    let (owner_box, member_box) = (owner_box.expect("box"), member_box.expect("box"));
    let owner = add_owner(db, business, owner_box, ago(60)).await;
    let owner = Acting {
        business,
        member: owner.expect("owner"),
    };
    let roles = Roles::new(true, true).expect("roles");
    let member = add_member(db, owner, member_box, roles, ago(60)).await;
    let member = Acting {
        business,
        member: member.expect("member"),
    };
    Team {
        owner,
        member,
        owner_box,
        member_box,
    }
}

/// The router, with the portal's vault when `with_vault`.
async fn world(pool: &PgPool, with_vault: bool) -> World {
    let mut db = pool.acquire().await.expect("db");
    let team = team(&mut db, "a").await;
    let hooks: Hooks = (None, None, None, None);
    let parts = RouterParts::from(hooks).with_vault(with_vault.then(vault));
    World {
        app: build_router(pool.clone(), Router::new(), parts),
        pool: pool.clone(),
        dir: Directory(vault()),
        team,
    }
}

/// `by` links `number`, `minutes` ago: a member's phone waits for the owner.
async fn linked(w: &World, by: Acting, number: &str, minutes: i64) -> Uuid {
    let mut db = w.pool.acquire().await.expect("db");
    let number = SecretString::from(number.to_owned());
    let line = link_phone(&mut db, &w.dir, (by, false), &number, ago(minutes)).await;
    line.expect("linked")
}

/// The form fields naming `line` as `business`'s waiting list shows it now.
async fn shown_form(w: &World, business: Uuid, line: Uuid) -> String {
    let mut db = w.pool.acquire().await.expect("db");
    let phones = waiting_phones(&mut db, business).await.expect("waiting");
    let phone = phones.iter().find(|p| p.line == line).expect("waiting");
    format!("&line={line}&number_set_at={}", phone.number_set_at)
}

async fn waiting(w: &World, line: Uuid) -> bool {
    let mut db = w.pool.acquire().await.expect("db");
    let phones = waiting_phones(&mut db, w.team.owner.business).await;
    phones.expect("waiting").iter().any(|p| p.line == line)
}

async fn found(w: &World, number: &str) -> Option<Uuid> {
    let mut db = w.pool.acquire().await.expect("db");
    sender_business(&mut db, &w.dir.0, number)
        .await
        .expect("lookup")
}

/// The one sign-in address of one person; the token of the link it "emails" is kept.
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

/// `who` (sign-in `mailbox`) signed in through their emailed link, with a step-up a minute ago
/// when `step_up`: their cookies.
async fn session(w: &World, (who, mailbox): (Acting, Uuid), step_up: bool) -> CookieJar {
    let mut db = w.pool.acquire().await.expect("db");
    if step_up {
        record_step_up(&mut db, who, ago(1)).await.expect("step-up");
    }
    let to = SignInAddress {
        business: who.business,
        mailbox,
    };
    let mail = Arc::new(OneMail {
        to,
        sent: Mutex::default(),
    });
    send_sign_in_link(&mut db, &*mail, to, ago(0))
        .await
        .expect("link");
    let token = mail
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let path = format!("/b/sign-in/{}", token.expect("a link emailed"));
    let pre = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let press = Router::new().route(SIGNIN_LINK_ROUTE, post(signin_post::<OneMail>));
    let app = SignIn::new(w.pool.clone(), Arc::new(SystemClock), mail).into_router(press);
    let pressed = app
        .oneshot(request((Method::POST, &path), &pre, (Site::Own, "")))
        .await;
    let pressed = pressed.expect("pressed");
    let cookies: Vec<&str> = (pressed.headers().get_all(header::SET_COOKIE).iter())
        .filter_map(|v| v.to_str().ok()?.split(';').next())
        .filter(|pair| !pair.ends_with('='))
        .collect();
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookies.join("; ").parse().expect("cookies"));
    CookieJar::from_headers(&headers)
}

/// Same-origin with the right token, cross-site by `Sec-Fetch-Site` or by `Origin`, or a wrong token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Site {
    Own,
    Cross,
    OtherOrigin,
    WrongToken,
}

fn request(
    (method, path): (Method, &str),
    jar: &CookieJar,
    (site, extra): (Site, &str),
) -> Request<Body> {
    let csrf = match site {
        Site::WrongToken => "0".repeat(64),
        _ => csrf_token(jar).unwrap_or_default(),
    };
    let cookies: Vec<String> = jar
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect();
    let builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::COOKIE, cookies.join("; "))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    let builder = match site {
        Site::OtherOrigin => builder.header(header::ORIGIN, "https://elsewhere.example"),
        Site::Cross => builder.header("sec-fetch-site", "cross-site"),
        Site::Own | Site::WrongToken => builder
            .header("sec-fetch-site", "same-origin")
            .header(header::ORIGIN, format!("https://{HOST}")),
    };
    builder
        .body(Body::from(format!("csrf={csrf}{extra}")))
        .expect("request")
}

/// An answer as the cases judge it; every one must be `no-store` (SEC-034).
#[derive(Debug)]
struct Reply {
    status: u16,
    text: String,
    location: String,
}

async fn send(w: &World, asked: (Method, &str), jar: &CookieJar, site: (Site, &str)) -> Reply {
    let response = w.app.clone().oneshot(request(asked, jar, site)).await;
    let response = response.expect("response");
    let header = |name| {
        let value = response.headers().get(name).and_then(|v| v.to_str().ok());
        value.unwrap_or_default().to_owned()
    };
    let (store, location) = (header(header::CACHE_CONTROL), header(header::LOCATION));
    assert!(
        store.contains("no-store"),
        "every answer is no-store: {store:?}"
    );
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Reply {
        status,
        text,
        location,
    }
}

/// TD-SEC-65 (SEC-004, SEC-038): signed out, all three routes get the 4xx answer linking to
/// `/b/sign-in`; a "Confirm and billing" member holding both roles gets the not-found answer;
/// the owner with no recent step-up is sent to the step-up page; the phone stays waiting.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_65_signed_out_member_and_stale_owner_change_nothing(pool: PgPool) {
    let w = world(&pool, true).await;
    let line = linked(&w, w.team.member, "928-555-0160", 30).await;
    let form = shown_form(&w, w.team.owner.business, line).await;
    let all = [
        (Method::GET, PHONES),
        (Method::POST, APPROVE),
        (Method::POST, REJECT),
    ];
    let out = ensure_pre_session(CookieJar::new()).expect("pre-session");
    let member = session(&w, (w.team.member, w.team.member_box), true).await;
    for (method, path) in all.clone() {
        let r = send(&w, (method.clone(), path), &out, (Site::Own, &form)).await;
        assert!(
            (400..500).contains(&r.status) && r.text.contains("/b/sign-in"),
            "{path}: {r:?}"
        );
        let r = send(&w, (method, path), &member, (Site::Own, &form)).await;
        assert_eq!(r.status, 404, "{path}: only the owner (BIZ-033)");
    }
    let stale = session(&w, (w.team.owner, w.team.owner_box), false).await;
    for path in [APPROVE, REJECT] {
        let r = send(&w, (Method::POST, path), &stale, (Site::Own, &form)).await;
        assert_eq!(r.status, 303, "{r:?}");
        assert!(r.location.starts_with("/b/step-up"), "{r:?}");
    }
    assert!(waiting(&w, line).await, "nothing was approved or removed");
    assert_eq!(found(&w, "928-555-0160").await, None);
}

/// TD-SEC-65 (BIZ-033, BIZ-038): the owner's page names the linking member's sign-in address,
/// the last 4 digits and the linked time, never the whole number; Approve makes the number the
/// business's; Don't approve deletes the phone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_65_the_owner_sees_approves_and_declines(pool: PgPool) {
    let w = world(&pool, true).await;
    let kept = linked(&w, w.team.member, "928-555-0161", 30).await;
    let gone = linked(&w, w.team.member, "928-555-0162", 20).await;
    let owner = session(&w, (w.team.owner, w.team.owner_box), true).await;
    let r = send(&w, (Method::GET, PHONES), &owner, (Site::Own, "")).await;
    assert_eq!(r.status, 200, "{r:?}");
    assert!(
        r.text.contains("member.a@example.com"),
        "the member's address"
    );
    assert!(r.text.contains("ending in 0161") && r.text.contains("ending in 0162"));
    assert!(r.text.contains("Arizona time"));
    for whole in ["555-0161", "5550161", "555-0162", "5550162"] {
        assert!(!r.text.contains(whole), "no whole number: {whole}");
    }
    let form = shown_form(&w, w.team.owner.business, kept).await;
    let r = send(&w, (Method::POST, APPROVE), &owner, (Site::Own, &form)).await;
    assert_eq!((r.status, r.location.as_str()), (303, PHONES));
    assert_eq!(found(&w, "928-555-0161").await, Some(w.team.owner.business));
    let form = shown_form(&w, w.team.owner.business, gone).await;
    let r = send(&w, (Method::POST, REJECT), &owner, (Site::Own, &form)).await;
    assert_eq!((r.status, r.location.as_str()), (303, PHONES));
    let mut db = w.pool.acquire().await.expect("db");
    assert_eq!(
        phone_standing(&mut db, gone).await.expect("standing"),
        None,
        "deleted"
    );
    assert_eq!(found(&w, "928-555-0162").await, None);
    let r = send(&w, (Method::GET, PHONES), &owner, (Site::Own, "")).await;
    assert!(r.text.contains("No phones are waiting"), "the empty state");
}

/// TD-SEC-65 (BIZ-033, D-3081): an Approve carrying the `number_set_at` from before a member's
/// change changes nothing and shows the page again with the new digits and the changed-number
/// line; once the new number is approved, the stale form is the not-found answer.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_65_a_changed_number_is_shown_again_not_approved(pool: PgPool) {
    let w = world(&pool, true).await;
    let line = linked(&w, w.team.member, "928-555-0163", 30).await;
    let old = shown_form(&w, w.team.owner.business, line).await;
    let mut db = w.pool.acquire().await.expect("db");
    let number = SecretString::from("928-555-0164".to_owned());
    let by = (w.team.member, line);
    change_phone_at(&mut db, &w.dir, by, &number, ago(10))
        .await
        .expect("changed");
    let owner = session(&w, (w.team.owner, w.team.owner_box), true).await;
    let r = send(&w, (Method::POST, APPROVE), &owner, (Site::Own, &old)).await;
    assert_eq!(r.status, 409, "{r:?}");
    assert!(
        r.text.contains(CHANGED) && r.text.contains("ending in 0164"),
        "{}",
        r.text
    );
    assert!(waiting(&w, line).await);
    assert_eq!(found(&w, "928-555-0164").await, None);
    let new = shown_form(&w, w.team.owner.business, line).await;
    let r = send(&w, (Method::POST, APPROVE), &owner, (Site::Own, &new)).await;
    assert_eq!(r.status, 303, "{r:?}");
    assert_eq!(found(&w, "928-555-0164").await, Some(w.team.owner.business));
    for form in [&old, &new] {
        let r = send(&w, (Method::POST, APPROVE), &owner, (Site::Own, form)).await;
        assert_eq!(r.status, 404, "no longer waiting: {r:?}");
    }
}

/// TD-SEC-65 (SEC-019): another business's owner posting the line gets the not-found answer and
/// the phone stays waiting.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_65_another_business_owner_gets_not_found(pool: PgPool) {
    let w = world(&pool, true).await;
    let line = linked(&w, w.team.member, "928-555-0165", 30).await;
    let form = shown_form(&w, w.team.owner.business, line).await;
    let mut db = w.pool.acquire().await.expect("db");
    let other = team(&mut db, "b").await;
    let theirs = session(&w, (other.owner, other.owner_box), true).await;
    for path in [APPROVE, REJECT] {
        let r = send(&w, (Method::POST, path), &theirs, (Site::Own, &form)).await;
        assert_eq!(r.status, 404, "{path}: {r:?}");
    }
    let r = send(&w, (Method::GET, PHONES), &theirs, (Site::Own, "")).await;
    assert!(!r.text.contains("ending in 0165"), "only its own phones");
    assert!(waiting(&w, line).await);
}

/// TD-SEC-65 (SEC-042, SEC-048): a cross-site `POST` (by `Sec-Fetch-Site` or `Origin`) or a wrong
/// CSRF token is a 403 that changes nothing and is one security event each.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_65_cross_site_and_wrong_token_are_refused_and_logged(pool: PgPool) {
    let w = world(&pool, true).await;
    let line = linked(&w, w.team.member, "928-555-0166", 30).await;
    let form = shown_form(&w, w.team.owner.business, line).await;
    let owner = session(&w, (w.team.owner, w.team.owner_box), true).await;
    let events = Events::default();
    let _guard = tracing::subscriber::set_default(events.clone());
    let sites = [Site::Cross, Site::OtherOrigin, Site::WrongToken];
    for path in [APPROVE, REJECT] {
        for site in sites {
            let r = send(&w, (Method::POST, path), &owner, (site, &form)).await;
            assert_eq!(r.status, 403, "{path} {site:?}: {r:?}");
        }
    }
    assert_eq!(
        events.count(),
        2 * sites.len(),
        "each refusal is a security event"
    );
    assert!(waiting(&w, line).await);
    assert_eq!(found(&w, "928-555-0166").await, None);
}

/// TD-SEC-65 (D-3744): without the vault, each waiting phone shows the one fixed line in place
/// of the address, and the page is otherwise the same.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_65_without_a_vault_the_fixed_line_shows(pool: PgPool) {
    let w = world(&pool, false).await;
    linked(&w, w.team.member, "928-555-0167", 30).await;
    let owner = session(&w, (w.team.owner, w.team.owner_box), true).await;
    let r = send(&w, (Method::GET, PHONES), &owner, (Site::Own, "")).await;
    assert_eq!(r.status, 200);
    assert!(r.text.contains("someone on your team") && r.text.contains("ending in 0167"));
    assert!(!r.text.contains("@example.com"));
}

/// The security events logged while it is the default subscriber (SEC-048).
#[derive(Clone, Default)]
struct Events(Arc<Mutex<usize>>);

impl Events {
    fn count(&self) -> usize {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Whether one log event's message is "security event".
#[derive(Default)]
struct IsEvent(bool);

impl tracing::field::Visit for IsEvent {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" && format!("{value:?}") == "security event" {
            self.0 = true;
        }
    }
}

impl tracing::Subscriber for Events {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut seen = IsEvent::default();
        event.record(&mut seen);
        if seen.0 {
            *self.0.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}
