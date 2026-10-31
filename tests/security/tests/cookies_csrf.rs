//! Session cookies, CSRF tokens, cross-site refusal and security events (TD-SEC-04, TD-SEC-21,
//! TD-SEC-26; SEC-009, SEC-042, SEC-048, D-099, D-480).
//!
//! Every cookie is `__Host-`, `Secure`, `HttpOnly`, `SameSite=Lax` with `Path=/` and no `Domain`;
//! a login gives a new cookie and the pre-login and earlier cookies stop working; a CSRF token
//! works only with the cookie it was made from; a portals test route refuses a post without its
//! token, with another session's token, and any cross-site post even with a good token; each
//! refusal is logged with request ID and route template, never the path; and 50 events of one
//! kind within an hour raise exactly one High alert. The realm-proof case of TD-SEC-25 runs in
//! `app/portals/src/session.rs`, since only `portals` may take the session mint. Every ID and
//! value here is fake.

use std::sync::{Arc, Mutex, PoisonError};

use askhava_core::api::{ManualClock, json_layer};
use askhava_portals::api::{
    ButtonForm, CookieJar, CsrfForm, PRE_SESSION_COOKIE, Realm, SESSION_COOKIE, SecurityEvent,
    SecurityEventKind, WebSecurity, csrf_matches, csrf_token, current_session, ensure_pre_session,
    record_security_event, refuse_cross_site, start_session,
};
use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode, header};
use axum::middleware::from_fn_with_state;
use axum::response::IntoResponse;
use axum::routing::post;
use jiff::{SignedDuration, Timestamp};
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::layer::SubscriberExt;

const HOST: &str = "askhava.test";
const FAKE_PHONE: &str = "9285550143";
const FAKE_REQUEST_ID: &str = "6f9619ff-8b86-4011-b42d-00c04fc964ff";

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Sink {
    fn text(&self) -> String {
        let bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl std::io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut bytes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn now() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("time")
}

fn jar_with(cookies: &[(&str, &str)]) -> CookieJar {
    let line: Vec<String> = cookies.iter().map(|(n, v)| format!("{n}={v}")).collect();
    let mut headers = HeaderMap::new();
    let value = HeaderValue::from_str(&line.join("; ")).expect("cookie header");
    headers.insert(header::COOKIE, value);
    CookieJar::from_headers(&headers)
}

/// The `Set-Cookie` lines `jar` sends, as `(name, value, attributes)`.
fn set_cookies(jar: CookieJar) -> Vec<(String, String, String)> {
    let response = (jar, ()).into_response();
    let lines = response.headers().get_all(header::SET_COOKIE).iter();
    lines
        .map(|line| {
            let line = line.to_str().expect("ascii").to_owned();
            let (pair, attributes) = line.split_once(';').unwrap_or((&line, ""));
            let (name, value) = pair.split_once('=').expect("name=value");
            (name.to_owned(), value.to_owned(), attributes.to_owned())
        })
        .collect()
}

fn assert_host_cookie(name: &str, attributes: &str) {
    assert!(name.starts_with("__Host-"), "{name}");
    for needed in ["Secure", "HttpOnly", "SameSite=Lax", "Path=/"] {
        assert!(
            attributes.contains(needed),
            "{name}: {needed} in {attributes}"
        );
    }
    assert!(
        !attributes.contains("Domain"),
        "{name}: a __Host- cookie has no Domain"
    );
}

fn cookie_of(lines: &[(String, String, String)], name: &str) -> (String, String) {
    let (_, value, attributes) = lines.iter().find(|line| line.0 == name).expect(name);
    (value.clone(), attributes.clone())
}

/// TD-SEC-04, TD-SEC-21 (SEC-009, SEC-042): the pre-login cookie and an earlier session's cookie
/// never survive a login, and every cookie carries the `__Host-` attributes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn login_rotates_the_cookie_and_drops_the_pre_login_one(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let lines = set_cookies(ensure_pre_session(CookieJar::new()).expect("pre"));
    let (pre, attributes) = cookie_of(&lines, PRE_SESSION_COOKIE);
    assert_host_cookie(PRE_SESSION_COOKIE, &attributes);
    let login = (Realm::Business, Uuid::from_u128(7));
    let day = SignedDuration::from_hours(24);
    let first = start_session(
        &mut db,
        jar_with(&[(PRE_SESSION_COOKIE, &pre)]),
        login,
        day,
        now(),
    )
    .await;
    let lines = set_cookies(first.expect("first login"));
    let (first, attributes) = cookie_of(&lines, SESSION_COOKIE);
    assert_host_cookie(SESSION_COOKIE, &attributes);
    assert!(attributes.contains("Max-Age=86400"));
    assert_ne!(first, pre, "the pre-login cookie is not the session cookie");
    let (gone, attributes) = cookie_of(&lines, PRE_SESSION_COOKIE);
    assert!(gone.is_empty() && attributes.contains("Max-Age=0"));
    let again = start_session(
        &mut db,
        jar_with(&[(SESSION_COOKIE, &first)]),
        login,
        day,
        now(),
    )
    .await;
    let (second, _) = cookie_of(&set_cookies(again.expect("second login")), SESSION_COOKIE);
    assert_ne!(second, first, "a login always issues a new cookie");
    let old = current_session(&mut db, &jar_with(&[(SESSION_COOKIE, &first)]), now()).await;
    assert!(
        old.expect("lookup").is_none(),
        "the earlier cookie no longer signs in"
    );
    let new = current_session(&mut db, &jar_with(&[(SESSION_COOKIE, &second)]), now()).await;
    let session = new.expect("lookup").expect("the new cookie signs in");
    assert_eq!(session.realm(), Realm::Business);
    let later = now() + SignedDuration::from_hours(25);
    let expired = current_session(&mut db, &jar_with(&[(SESSION_COOKIE, &second)]), later).await;
    assert!(
        expired.expect("lookup").is_none(),
        "an expired session signs nobody in"
    );
}

/// TD-SEC-21 (SEC-009, SEC-042): a token works only with the cookie it was made from; a token
/// from session A is refused on session B; a pre-session token needs its own `__Host-` cookie.
#[test]
fn csrf_tokens_work_only_with_their_own_cookie() {
    let (a, b) = ("0a".repeat(32), "0b".repeat(32));
    let session_a = jar_with(&[(SESSION_COOKIE, &a)]);
    let session_b = jar_with(&[(SESSION_COOKIE, &b)]);
    let token_a = csrf_token(&session_a).expect("token");
    assert!(csrf_matches(&session_a, &token_a));
    assert!(
        !csrf_matches(&session_b, &token_a),
        "session A's token on session B"
    );
    let pre_a = jar_with(&[(PRE_SESSION_COOKIE, &a)]);
    let pre_token = csrf_token(&pre_a).expect("pre token");
    assert!(csrf_matches(&pre_a, &pre_token));
    assert!(!csrf_matches(
        &jar_with(&[(PRE_SESSION_COOKIE, &b)]),
        &pre_token
    ));
    assert!(
        !csrf_matches(&session_a, &pre_token),
        "a pre-session token is not a session token"
    );
    assert!(
        !csrf_matches(&CookieJar::new(), &pre_token),
        "no cookie, no token"
    );
    assert!(csrf_token(&CookieJar::new()).is_none());
    assert!(!csrf_matches(&session_a, ""));
}

async fn press(_form: CsrfForm<ButtonForm>) -> &'static str {
    "pressed"
}

fn test_route(pool: PgPool) -> Router {
    let security = WebSecurity::new(pool, Arc::new(ManualClock::new(now())));
    Router::new()
        .route("/test/{token}", post(press))
        .layer(from_fn_with_state(security.clone(), refuse_cross_site))
        .with_state(security)
}

fn form_post(cookie: &str, headers: &[(&str, &str)], body: &str) -> Request<Body> {
    let mut request = Request::post(format!("/test/{FAKE_PHONE}"))
        .header(header::HOST, HOST)
        .header(header::COOKIE, format!("{PRE_SESSION_COOKIE}={cookie}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("x-request-id", FAKE_REQUEST_ID);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.body(Body::from(body.to_owned())).expect("request")
}

/// TD-SEC-04, TD-SEC-21, TD-SEC-26 (SEC-009, SEC-042, SEC-048): the portals test route takes a
/// same-origin post with its token, and refuses a missing or foreign token and every cross-site
/// post; each refusal is logged with request ID and route template, never the path's phone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_test_route_refuses_cross_site_and_tokenless_posts(pool: PgPool) {
    let (mine, other) = ("1c".repeat(32), "2d".repeat(32));
    let token = csrf_token(&jar_with(&[(PRE_SESSION_COOKIE, &mine)])).expect("token");
    let good = format!("csrf={token}");
    let same = [
        ("sec-fetch-site", "same-origin"),
        ("origin", "https://askhava.test"),
    ];
    let cases = [
        (form_post(&mine, &same, &good), StatusCode::OK),
        (form_post(&mine, &same, ""), StatusCode::FORBIDDEN),
        (form_post(&other, &same, &good), StatusCode::FORBIDDEN),
        (
            form_post(&mine, &[("sec-fetch-site", "cross-site")], &good),
            StatusCode::FORBIDDEN,
        ),
        (
            form_post(&mine, &[("origin", "https://evil.test")], &good),
            StatusCode::FORBIDDEN,
        ),
        (form_post(&mine, &[], &good), StatusCode::FORBIDDEN),
        (
            form_post(&mine, &[("origin", "https://askhava.test")], &good),
            StatusCode::OK,
        ),
    ];
    let log = Sink::default();
    let writer = log.clone();
    let subscriber = tracing_subscriber::registry().with(json_layer(move || writer.clone()));
    let subscriber = tracing::Dispatch::new(subscriber);
    for (number, (request, want)) in cases.into_iter().enumerate() {
        let app = test_route(pool.clone());
        let got = app
            .oneshot(request)
            .with_subscriber(subscriber.clone())
            .await;
        assert_eq!(got.expect("response").status(), want, "case {number}");
    }
    let stored: Vec<(String, String)> =
        sqlx::query_as("SELECT kind, route FROM portals.security_event ORDER BY at, id")
            .fetch_all(&pool)
            .await
            .expect("events");
    assert_eq!(stored.len(), 5, "one event per refusal");
    assert!(
        stored
            .iter()
            .all(|(kind, route)| kind == "csrf_rejected" && route == "/test/{token}")
    );
    let text = log.text();
    assert_eq!(
        text.matches("security event").count(),
        5,
        "one log line per refusal"
    );
    assert!(text.contains(FAKE_REQUEST_ID) && text.contains("/test/{token}"));
    assert!(!text.contains(FAKE_PHONE), "the path never reaches the log");
}

async fn record(pool: &PgPool, kind: SecurityEventKind, at: Timestamp) -> bool {
    let mut db = pool.acquire().await.expect("connection");
    let event = SecurityEvent {
        kind,
        route: "/sign-in",
        request_id: FAKE_REQUEST_ID.parse().ok(),
        actor_ref: None,
    };
    record_security_event(&mut db, &event, at)
        .await
        .expect("recorded")
}

async fn alerts(pool: &PgPool, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.alert WHERE kind = $1 AND severity = 'high'")
        .bind(kind)
        .fetch_one(pool)
        .await
        .expect("alerts")
}

/// TD-SEC-26 (SEC-048): 50 failed sign-ins within an hour raise exactly one High alert; events
/// older than an hour do not count, and another kind keeps its own count.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn fifty_failed_sign_ins_in_an_hour_raise_one_high_alert(pool: PgPool) {
    let stale = now() - SignedDuration::from_mins(61);
    for _ in 0..49 {
        assert!(!record(&pool, SecurityEventKind::AuthFailed, stale).await);
    }
    for minute in 0..49 {
        let at = now() + SignedDuration::from_mins(minute);
        assert!(
            !record(&pool, SecurityEventKind::AuthFailed, at).await,
            "event {minute}"
        );
    }
    assert!(!record(&pool, SecurityEventKind::CsrfRejected, now()).await);
    assert_eq!(alerts(&pool, "portals.auth_failed_burst").await, 0);
    let fiftieth = now() + SignedDuration::from_mins(50);
    assert!(record(&pool, SecurityEventKind::AuthFailed, fiftieth).await);
    assert!(!record(&pool, SecurityEventKind::AuthFailed, fiftieth).await);
    assert_eq!(alerts(&pool, "portals.auth_failed_burst").await, 1);
    assert_eq!(alerts(&pool, "portals.csrf_rejected_burst").await, 0);
}
