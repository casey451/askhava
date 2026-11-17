//! TD-SEC-72 (CON-025, SEC-042, SEC-048; T595, D-4174): the request page and web chat at
//! `/request` served by the assembled router against PostgreSQL, behind `server`'s chat guard.
//! `GET /request` sets the pre-session cookie, puts its CSRF token in the form and says the user
//! must be 18 or older before the form; a same-origin `POST` with that cookie, its token and a
//! job message is answered (below 500, `no-store`, no `csrf_rejected` event) with the posted
//! words in the chat list; a missing, empty, wrong or other-cookie token, or a cross-site `POST`
//! with the right token, is a 403 with exactly one `csrf_rejected` event for `/request`. No code
//! keys, so no code is sent (D-3744). Fake job words only: no name, phone or address.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, PoisonError};

use askhava_core::api::MIGRATOR;
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, header};
use sqlx::PgPool;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const PATH: &str = "/request";
const PRE: &str = "__Host-askhava-pre";
/// A 32-hex chat key, not all zeros, and a fake PRIV-008 message ID.
const CHAT: &str = "0123456789abcdef0123456789abcdef";
const MESSAGE: &str = "00000000-0000-4000-8000-000000000072";
/// Fake job words with no name, number or address.
const WORDS: &str = "the kitchen sink drips under the cabinet";

/// The assembled router with no code keys and the security events it logs.
struct World {
    app: Router,
    events: Events,
    _guard: tracing::subscriber::DefaultGuard,
}

fn world(pool: &PgPool) -> World {
    let parts = RouterParts::from((None, None, None, None));
    let app = build_router(pool.clone(), Router::new(), parts);
    let events = Events::default();
    let guard = tracing::subscriber::set_default(events.clone());
    World {
        app,
        events,
        _guard: guard,
    }
}

/// One answer: status, body, whether it is `no-store` and the pre-session cookie it set.
struct Reply {
    status: u16,
    text: String,
    no_store: bool,
    pre: Option<String>,
}

/// Sends `method` to `/request` from client `n` with `cookie`, `Sec-Fetch-Site: site` and `form`.
async fn send(w: &World, method: &str, (cookie, site): (&str, &str), form: &str, n: u8) -> Reply {
    let mut request = Request::builder()
        .method(method)
        .uri(PATH)
        .header(header::HOST, HOST)
        .header("sec-fetch-site", site)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, cookie)
        .body(Body::from(form.to_owned()))
        .expect("request");
    let at = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, n)), 40_000);
    request.extensions_mut().insert(ConnectInfo(at));
    let response = w.app.clone().oneshot(request).await.expect("response");
    let status = response.status().as_u16();
    let control = response.headers().get(header::CACHE_CONTROL);
    let no_store = control.is_some_and(|v| v.to_str().is_ok_and(|v| v.contains("no-store")));
    let pre = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|c| c.to_str().ok()?.split(';').next().map(str::to_owned))
        .find(|pair| pair.starts_with(&format!("{PRE}=")));
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    let text = String::from_utf8_lossy(&body).into_owned();
    Reply {
        status,
        text,
        no_store,
        pre,
    }
}

/// Opens the page from client `n`: its pre-session cookie pair, its form's token and the page.
async fn opened(w: &World, n: u8) -> (String, String, Reply) {
    let page = send(w, "GET", ("", "same-origin"), "", n).await;
    let cookie = page.pre.clone().expect("a pre-session cookie");
    let token = page.text.split("name=\"csrf\" value=\"").nth(1);
    let token = token
        .and_then(|rest| rest.split('"').next())
        .expect("token");
    (cookie, token.to_owned(), page)
}

/// The message step's form with `csrf` (no `csrf` field at all when `None`).
fn form(csrf: Option<&str>) -> String {
    let words = WORDS.replace(' ', "+");
    let fields = format!("chat={CHAT}&message={MESSAGE}&g=&said=&step=message&words={words}");
    csrf.map_or(fields.clone(), |t| format!("csrf={t}&{fields}"))
}

/// The `security event` log lines captured, `[kind, route]` (SEC-048), as
/// `owner_signin_route.rs` reads them.
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<[String; 2]>>>);

/// The fields of one log event.
#[derive(Default)]
struct Fields(Vec<(String, String)>);

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
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
        let mut fields = Fields::default();
        event.record(&mut fields);
        let get = |name: &str| {
            let found = fields.0.iter().find(|(n, _)| n == name);
            found.map(|(_, v)| v.clone()).unwrap_or_default()
        };
        if get("message") == "security event" {
            let mut all = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            all.push([get("kind"), get("route")]);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// How many `csrf_rejected` events on `/request` the test has logged.
fn rejected(w: &World) -> usize {
    let all = w.events.0.lock().unwrap_or_else(PoisonError::into_inner);
    let want = ["csrf_rejected".to_owned(), PATH.to_owned()];
    all.iter().filter(|e| **e == want).count()
}

/// TD-SEC-72 (SEC-042, CON-025): the page sets the pre-session cookie, puts its token in the
/// form and says the user must be 18 or older before the form.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_72_page_sets_cookie_token_and_age_line(pool: PgPool) {
    let w = world(&pool);
    let (_, token, page) = opened(&w, 1).await;
    assert_eq!(page.status, 200);
    assert!(page.no_store, "the page is no-store");
    assert_eq!(token.len(), 64, "a 64-hex token");
    assert!(token.bytes().all(|b| b.is_ascii_hexdigit()), "hex token");
    let brand = askhava_core::api::brand().display_name();
    let line = format!("<p>You must be 18 or older to use {brand}.</p>");
    let at = page.text.find(&line).expect("the 18-or-older line");
    let form = page.text.find("<form").expect("the chat form");
    assert!(at < form, "the 18-or-older line is above the chat box");
}

/// TD-SEC-72 (SEC-042, SEC-048): a same-origin `POST` with the cookie's own token is answered,
/// `no-store`, with no `csrf_rejected` event and the posted words in the chat list.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_72_same_origin_post_with_its_token_is_answered(pool: PgPool) {
    let w = world(&pool);
    let (cookie, token, _) = opened(&w, 2).await;
    let reply = send(&w, "POST", (&cookie, "same-origin"), &form(Some(&token)), 2).await;
    assert_ne!(reply.status, 403, "a right token is not refused");
    assert!(reply.status < 500, "status {}", reply.status);
    assert!(reply.no_store, "the answer is no-store");
    assert_eq!(rejected(&w), 0, "no csrf_rejected event");
    let list = reply.text.split("<ol>").nth(1).expect("the chat list");
    let list = list.split("</ol>").next().expect("its end");
    assert!(list.contains(&format!("You: {WORDS}")), "the posted words");
}

/// TD-SEC-72 (SEC-042, SEC-048): a missing, empty, all-zero or other-cookie token, or a
/// cross-site `POST` with the right token, is a 403 with one `csrf_rejected` event and changes
/// nothing: no cookie set, no chat list.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_72_wrong_token_or_cross_site_post_is_refused(pool: PgPool) {
    let w = world(&pool);
    let (cookie, token, _) = opened(&w, 3).await;
    let (_, other, _) = opened(&w, 4).await;
    let zeros = "0".repeat(64);
    let cases = [
        ("no token", form(None), "same-origin"),
        ("empty token", form(Some("")), "same-origin"),
        ("zero token", form(Some(&zeros)), "same-origin"),
        ("other cookie's token", form(Some(&other)), "same-origin"),
        ("cross-site", form(Some(&token)), "cross-site"),
    ];
    for (n, (case, body, site)) in (10u8..).zip(cases) {
        let before = rejected(&w);
        let reply = send(&w, "POST", (&cookie, site), &body, n).await;
        assert_eq!(reply.status, 403, "{case}");
        assert_eq!(rejected(&w), before + 1, "{case}: one csrf_rejected event");
        assert!(reply.pre.is_none(), "{case}: no cookie set");
        assert!(!reply.text.contains(WORDS), "{case}: no chat list");
    }
}
