//! TD-SEC-55 (SEC-012) and TD-SEC-56 (SEC-016), T286, D-2208: a panicking route is answered
//! 500 with the security headers, `no-store` and the request ID, naming no panic message, and the
//! server keeps answering; `ASKHAVA_PUBLIC_BASE_URL` with a path stops start-up naming the key.
//! Fake data only.

use askhava_core::api::Settings;
use askhava_server::{Hooks, build_router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use axum::routing::get;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

const PANIC_TEXT: &str = "fake panic detail T286";

fn app() -> Router {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://fake@127.0.0.1:1/none")
        .expect("a lazy pool");
    let routes = Router::new()
        .route("/boom", get(boom))
        .route("/fine", get(|| async { "fine" }));
    let hooks: Hooks = (None, None, None, None);
    build_router(pool, routes, hooks)
}

async fn boom() -> &'static str {
    panic!("{PANIC_TEXT}")
}

/// TD-SEC-55 (SEC-012, D-2208): the panic is a 500 with every header and no panic text; the same
/// router then answers another request.
#[tokio::test]
async fn td_sec_55_a_panicking_route_is_a_plain_500_and_the_server_keeps_answering() {
    let app = app();
    let request = Request::get("/boom").body(Body::empty()).expect("request");
    let response = app.clone().oneshot(request).await.expect("infallible");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let headers = response.headers().clone();
    let value = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    };
    assert_eq!(value("cache-control"), "no-store");
    assert!(!value("x-request-id").is_empty(), "the request ID");
    assert_eq!(value("x-content-type-options"), "nosniff");
    assert!(value("content-security-policy").contains("object-src 'none'"));
    assert!(value("strict-transport-security").contains("includeSubDomains"));
    assert!(value("permissions-policy").contains("microphone=()"));
    let body = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    let body = String::from_utf8_lossy(&body);
    assert!(
        !body.contains("fake panic") && !body.contains("T286"),
        "{body}"
    );
    let request = Request::get("/fine").body(Body::empty()).expect("request");
    let response = app.oneshot(request).await.expect("infallible");
    assert_eq!(response.status(), StatusCode::OK);
}

fn with_base(url: &str) -> Result<Settings, String> {
    let pairs = [
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@127.0.0.1:1/none"),
        ("ASKHAVA_PUBLIC_BASE_URL", url),
    ];
    Settings::from_pairs(pairs.map(|(k, v)| (k.to_owned(), v.to_owned())))
        .map_err(|e| e.to_string())
}

/// TD-SEC-56 (SEC-016, D-2208): a base URL with a path stops start-up naming the key only; a
/// bare origin with a trailing slash is accepted without it.
#[test]
fn td_sec_56_a_base_url_with_a_path_stops_start_up() {
    for bad in [
        "https://x.example/app",
        "https://x.example/app/",
        "https://x.example/a/b",
    ] {
        let error = with_base(bad).expect_err(bad);
        assert!(error.contains("ASKHAVA_PUBLIC_BASE_URL"), "{error}");
        assert!(!error.contains("x.example"), "{error}");
    }
    let settings = with_base("https://x.example/").expect("a trailing slash only");
    assert_eq!(settings.public_base_url(), Some("https://x.example"));
}
