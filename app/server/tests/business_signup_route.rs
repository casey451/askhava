//! TD-SEC-45's form-page cases (BIZ-021, CON-019, SEC-042; T302, D-2128, D-3676): the
//! assembled router with the sign-up part serves `GET /b/signup` without a session, sets the
//! pre-session `__Host-` cookie and puts its CSRF token in the form, never ticks the E-SIGN box,
//! seals the shown channels for that token alone, lists the stored categories and answers
//! `no-store`. No setting is a real key; there is no personal data.

use askhava_catalog::api::{load, parse};
use askhava_core::api::{IdSecret, MIGRATOR, Settings, TestControl};
use askhava_portals::api::{ChannelPost, SIGNUP_ROUTE};
use askhava_server::business_signup::ServedSignUp;
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use secrecy::SecretString;
use sqlx::PgPool;
use tower::ServiceExt;

const SECRET: &str = "test-only-id-secret-not-a-real-key-0001";

fn pairs() -> Vec<(String, String)> {
    [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        ("ASKHAVA_ID_SECRET", SECRET),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect()
}

/// The router as `main` assembles it, with only the sign-up part added.
fn app(pool: &PgPool) -> Router {
    let settings = Settings::from_pairs(pairs()).expect("fake settings");
    let control = TestControl::from_settings(&settings, pairs()).expect("test control");
    let signup = ServedSignUp::from_settings(pool.clone(), pairs()).expect("sign-up part");
    let parts = RouterParts::from((None, None, None, None)).with_sign_up(signup);
    build_router(pool.clone(), control.router(), parts)
}

/// Stores the reviewed catalog, its categories switched on as the file says (LIC-003).
async fn store_catalog(pool: &PgPool) {
    let catalog = parse(include_str!("../../../docs/catalog/categories.yaml")).expect("catalog");
    let mut conn = pool.acquire().await.expect("connection");
    load(&mut conn, &catalog).await.expect("catalog stored");
}

/// `GET /b/signup` with no cookie: the status, the `Set-Cookie` lines, `Cache-Control` and the page.
async fn open(app: &Router) -> (StatusCode, Vec<String>, String, String) {
    let request = Request::get(SIGNUP_ROUTE)
        .header(header::HOST, "askhava.test")
        .body(Body::empty())
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("answer");
    let status = response.status();
    let headers = response.headers();
    let cookies = headers.get_all(header::SET_COOKIE).iter();
    let cookies = cookies.filter_map(|v| v.to_str().ok().map(str::to_owned));
    let cache = headers
        .get(header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok());
    let cache = cache.unwrap_or_default().to_owned();
    let cookies = cookies.collect();
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    let html = String::from_utf8(body.to_vec()).expect("utf-8");
    (status, cookies, cache, html)
}

/// The value of the first hidden field `name` on `html`.
fn hidden(html: &str, name: &str) -> String {
    let at = format!("name=\"{name}\" value=\"");
    let rest = &html[html.find(&at).expect("field") + at.len()..];
    rest[..rest.find('"').expect("end")].to_owned()
}

fn secret() -> IdSecret {
    IdSecret::new(1, SecretString::from(SECRET.to_owned())).expect("secret")
}

/// The form's shown channels as a post would carry them, checked for `csrf`.
fn shown(html: &str, csrf: &str) -> Option<Vec<askhava_businesses::api::ConsentChannel>> {
    let fields: Vec<(String, String)> = ["consent_channels", "consent_seal"]
        .iter()
        .map(|n| ((*n).to_owned(), hidden(html, n)))
        .collect();
    ChannelPost::parse(&fields, false, (&secret(), csrf)).shown
}

/// TD-SEC-45 (BIZ-021, CON-019, SEC-042): served without a session, with the pre-session cookie,
/// its CSRF token, the box unticked, the channels sealed for that token alone and `no-store`.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_45_the_form_is_served_without_a_session(pool: PgPool) {
    store_catalog(&pool).await;
    let (status, cookies, cache, html) = open(&app(&pool)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        cookies.iter().any(|c| c.starts_with("__Host-askhava-pre=")),
        "the pre-session cookie: {cookies:?}"
    );
    assert!(
        !cookies
            .iter()
            .any(|c| c.starts_with("__Host-askhava-session"))
    );
    assert_eq!(cache, "no-store");
    assert!(html.contains("Sign up your business"));
    let csrf = hidden(&html, "csrf");
    assert_eq!(
        csrf.len(),
        64,
        "a CSRF token bound to the pre-session cookie"
    );
    let tag = "<input type=\"checkbox\" id=\"consent\" name=\"consent\" value=\"yes\"";
    let tag_at = html.find(tag).expect("the E-SIGN box");
    let tag_end = tag_at + html[tag_at..].find('>').expect("tag end");
    assert!(
        !html[tag_at..tag_end].contains("checked"),
        "CON-019: unticked"
    );
    assert_eq!(hidden(&html, "consent_channels"), "email");
    let sealed = shown(&html, &csrf).expect("the seal verifies for the page's token");
    assert_eq!(sealed, vec![askhava_businesses::api::ConsentChannel::Email]);
    assert!(
        shown(&html, &"0".repeat(64)).is_none(),
        "not for another token"
    );
}

/// TD-SEC-45 (BIZ-021): each visit gets its own pre-session token; the stored switched-on
/// categories are listed with their "Open now" or "Coming later" label.
#[sqlx::test(migrator = "MIGRATOR")]
async fn td_sec_45_the_form_lists_the_stored_categories(pool: PgPool) {
    let app = app(&pool);
    let (_, _, _, empty) = open(&app).await;
    assert!(
        !empty.contains("Open now"),
        "no stored category, none listed"
    );
    store_catalog(&pool).await;
    let (status, _, _, first) = open(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert!(first.contains("Open now") || first.contains("Coming later"));
    let (_, _, _, second) = open(&app).await;
    assert_ne!(hidden(&first, "csrf"), hidden(&second, "csrf"));
}
