//! SEC-010 (T838, D-5574): through the assembled router, every form T838 made strict refuses a
//! post that adds one field name its served forms never send, with the form's own refusal answer
//! and nothing stored, while the same post without it answers as before. Each "as before" is the
//! cheapest answer the route already gives: an unissued sign-in or action-link token, a sign-up
//! step with no binding cookie, the capability step with no session. Fake data only.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use askhava_core::api::{MIGRATOR, Settings};
use askhava_portals::api::{CAPABILITY_ROUTE, CODE_SEND_ROUTE, SIGNIN_ROUTE};
use askhava_server::business_signin::ServedSignIn;
use askhava_server::business_signup::ServedSignUp;
use askhava_server::{RouterParts, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

const HOST: &str = "askhava.test";
const SECRET: &str = "test-only-id-secret-not-a-real-key-0001";
const SEAL: &str = "1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
const INDEX: &str = "1:BAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQ=";
/// The one field name no served form posts.
const UNKNOWN: &str = "&made_up=1";
/// An action-link or sign-in token nobody issued.
const UNISSUED: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// The dev settings both served parts start from.
fn pairs() -> Vec<(String, String)> {
    [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        ("ASKHAVA_ID_SECRET", SECRET),
        ("VAULT_KEY", SEAL),
        ("VAULT_INDEX_KEY", INDEX),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}

/// The router as `main` assembles it, with the sign-in and sign-up parts added.
fn app(pool: &PgPool) -> Router {
    let settings = Settings::from_pairs(pairs()).expect("fake settings");
    let signin = ServedSignIn::from_settings(&settings, pool.clone(), pairs()).expect("sign-in");
    let signup = ServedSignUp::from_settings(pool.clone(), pairs()).expect("sign-up");
    let parts = RouterParts::from((None, None, None, None))
        .with_sign_in(signin)
        .with_sign_up(signup);
    build_router(pool.clone(), Router::new(), parts)
}

/// A browser's pre-session cookie (`name=value`) and its CSRF token, from the sign-in page.
async fn browser(app: &Router) -> (String, String) {
    let request = Request::get(SIGNIN_ROUTE)
        .header(header::HOST, HOST)
        .body(Body::empty())
        .expect("request");
    let response = app.clone().oneshot(request).await.expect("answer");
    let mut lines = response.headers().get_all(header::SET_COOKIE).iter();
    let line = lines.find_map(|v| {
        v.to_str()
            .ok()
            .filter(|c| c.starts_with("__Host-askhava-pre="))
    });
    let cookie = line.and_then(|c| c.split(';').next()).map(str::to_owned);
    let cookie = cookie.expect("pre-session cookie");
    let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    let html = String::from_utf8_lossy(&body);
    let at = "name=\"csrf\" value=\"";
    let rest = &html[html.find(at).expect("csrf field") + at.len()..];
    (cookie, rest[..rest.find('"').expect("end")].to_owned())
}

/// A same-origin form `POST` of `body` to `path` from the browser holding `cookie`: its status.
async fn post(app: &Router, (path, cookie): (&str, &str), body: String) -> StatusCode {
    let mut request = Request::post(path)
        .header(header::HOST, HOST)
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .expect("request");
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 8)), 4000);
    request.extensions_mut().insert(ConnectInfo(peer));
    app.clone().oneshot(request).await.expect("answer").status()
}

/// Every table's row count in the app's own schemas, by table (each count run by `query_to_xml`,
/// as `home_request_box.rs` does, so the SQL is one checked statement), less the task queue, where
/// every request's own log line is queued (`offdb_stream`, PRIV-029) whatever the form did.
async fn traces(pool: &PgPool) -> Vec<(String, i64)> {
    let rows = sqlx::query!(
        r#"SELECT (table_schema || '.' || table_name)::text AS "name!",
             (xpath('/row/n/text()', query_to_xml(
                 format('SELECT count(*) AS n FROM %I.%I', table_schema, table_name),
                 false, true, '')))[1]::text::bigint AS "n!"
           FROM information_schema.tables
           WHERE table_type = 'BASE TABLE'
             AND table_schema::text NOT IN ('pg_catalog', 'information_schema')
             AND table_name::text <> 'task'
           ORDER BY 1"#
    )
    .fetch_all(pool)
    .await
    .expect("counts");
    assert!(rows.len() > 20, "the app's tables were found");
    rows.into_iter().map(|r| (r.name, r.n)).collect()
}

/// SEC-010: `path` refuses `fields` plus [`UNKNOWN`] with `refused` and stores nothing, and
/// answers `fields` alone with `before`.
async fn strict(pool: &PgPool, path: &str, fields: &str, (refused, before): (u16, u16)) {
    let app = app(pool);
    let (cookie, csrf) = browser(&app).await;
    let at = (path, cookie.as_str());
    let kept = traces(pool).await;
    let planted = post(&app, at, format!("csrf={csrf}{fields}{UNKNOWN}")).await;
    assert_eq!(planted.as_u16(), refused, "{path}: an unknown field");
    assert_eq!(
        traces(pool).await,
        kept,
        "{path}: a refused post stores nothing"
    );
    let plain = post(&app, at, format!("csrf={csrf}{fields}")).await;
    assert_eq!(plain.as_u16(), before, "{path}: the same post without it");
}

/// `AddressForm`: the sign-in email form.
#[sqlx::test(migrator = "MIGRATOR")]
async fn sec_010_signin_address_form_refuses_an_unknown_field(pool: PgPool) {
    let email = "&email=owner%40example.test";
    strict(&pool, SIGNIN_ROUTE, email, (422, 200)).await;
}

/// `CodeForm`: a sign-in link's button, on a token nobody issued.
#[sqlx::test(migrator = "MIGRATOR")]
async fn sec_010_signin_code_form_refuses_an_unknown_field(pool: PgPool) {
    let path = format!("{SIGNIN_ROUTE}/{UNISSUED}");
    strict(&pool, &path, "&code=", (422, 200)).await;
}

/// `Posted` (`action_links.rs`): a quote link nobody issued, with the quote form's hidden fields.
#[sqlx::test(migrator = "MIGRATOR")]
async fn sec_010_action_link_form_refuses_an_unknown_field(pool: PgPool) {
    let path = format!("/b/a/quote/{}/{UNISSUED}", Uuid::new_v4());
    let fields = "&format=fixed&amount=325&auto_confirm=yes&same=yes";
    strict(&pool, &path, fields, (422, 404)).await;
}

/// `same_site`: a sign-up step with no binding cookie; an unknown name and a repeated
/// single-value name are the 400 a body that is no form gets.
#[sqlx::test(migrator = "MIGRATOR")]
async fn sec_010_signup_step_refuses_an_unknown_and_a_repeated_field(pool: PgPool) {
    strict(&pool, CODE_SEND_ROUTE, "&lang=es", (400, 403)).await;
    let app = app(&pool);
    let (cookie, csrf) = browser(&app).await;
    let twice = format!("csrf={csrf}&lang=es&lang=es");
    let status = post(&app, (CODE_SEND_ROUTE, &cookie), twice).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a repeated single-value name"
    );
}

/// `CapabilityForm`: the capability step with no session.
#[sqlx::test(migrator = "MIGRATOR")]
async fn sec_010_capability_form_refuses_an_unknown_field(pool: PgPool) {
    let path = format!("{CAPABILITY_ROUTE}?step=check");
    let fields = "&job_type=electrical%3Apanel+upgrade&largest=none";
    strict(&pool, &path, fields, (422, 401)).await;
}
