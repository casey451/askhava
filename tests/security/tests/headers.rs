//! Every response's security headers and `no-store` on contact and photo pages (TD-SEC-04;
//! SEC-012, SEC-034, SEC-054, D-492).
//!
//! A router built like the app's, with `portals`' `security_headers` outermost, answers a page, a
//! missing path, a handler that tries to send a weaker policy and a contact page behind
//! `no_store`. Each response must carry the full SEC-012 set: a CSP with no `'unsafe-inline'` or
//! `'unsafe-eval'` in `script-src`, `object-src 'none'` and `frame-ancestors 'none'`; HSTS of at
//! least a year with `includeSubDomains` and no `preload`; camera, microphone and geolocation off;
//! `nosniff`; and `Referrer-Policy: no-referrer`, which covers every token page. A source check
//! proves `server`'s `build_router` puts the same layer over the whole app.

use askhava_portals::api::{no_store, security_headers};
use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::middleware::from_fn;
use axum::response::IntoResponse;
use axum::routing::get;
use tower::ServiceExt;

fn app() -> Router {
    let contact = Router::new()
        .route("/contact", get(|| async { "contact" }))
        .layer(from_fn(no_store));
    Router::new()
        .route("/page", get(|| async { "page" }))
        .route(
            "/weak",
            get(|| async {
                (
                    [(
                        header::CONTENT_SECURITY_POLICY,
                        "script-src 'unsafe-inline'",
                    )],
                    "weak",
                )
                    .into_response()
            }),
        )
        .merge(contact)
        .layer(from_fn(security_headers))
}

async fn headers_of(path: &str) -> (StatusCode, HeaderMap) {
    let request = Request::get(path).body(Body::empty()).expect("request");
    let response = app().oneshot(request).await.expect("response");
    (response.status(), response.headers().clone())
}

fn value(headers: &HeaderMap, name: &str) -> String {
    let found = headers
        .get(name)
        .unwrap_or_else(|| panic!("{name} missing"));
    found.to_str().expect("ascii header").to_owned()
}

fn directive<'a>(policy: &'a str, name: &str) -> &'a str {
    policy
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix(name).filter(|rest| rest.starts_with(' ')))
        .unwrap_or_else(|| panic!("CSP has no {name}"))
        .trim()
}

fn assert_security_headers(path: &str, headers: &HeaderMap) {
    let csp = value(headers, "content-security-policy");
    let script = directive(&csp, "script-src");
    assert!(
        !script.contains("'unsafe-inline'") && !script.contains("'unsafe-eval'"),
        "{path}"
    );
    assert_eq!(directive(&csp, "object-src"), "'none'", "{path}");
    assert_eq!(directive(&csp, "frame-ancestors"), "'none'", "{path}");
    let hsts = value(headers, "strict-transport-security");
    let max_age: u64 = hsts
        .split(';')
        .find_map(|part| part.trim().strip_prefix("max-age="))
        .and_then(|age| age.parse().ok())
        .expect("HSTS max-age");
    assert!(max_age >= 31_536_000, "{path}: HSTS max-age {max_age}");
    assert!(hsts.contains("includeSubDomains"), "{path}");
    assert!(
        !hsts.to_ascii_lowercase().contains("preload"),
        "{path}: HSTS preload (SEC-054)"
    );
    let permissions = value(headers, "permissions-policy");
    for feature in ["camera=()", "microphone=()", "geolocation=()"] {
        assert!(permissions.contains(feature), "{path}: {feature}");
    }
    assert_eq!(
        value(headers, "x-content-type-options"),
        "nosniff",
        "{path}"
    );
    assert_eq!(value(headers, "referrer-policy"), "no-referrer", "{path}");
}

/// TD-SEC-04 (SEC-012, SEC-054): a page, a 404, a handler's weaker policy and a contact page all
/// leave with the full header set; the weaker policy is replaced.
#[tokio::test]
async fn every_response_carries_the_security_headers() {
    for (path, status) in [
        ("/page", StatusCode::OK),
        ("/missing", StatusCode::NOT_FOUND),
        ("/weak", StatusCode::OK),
        ("/contact", StatusCode::OK),
    ] {
        let (got, headers) = headers_of(path).await;
        assert_eq!(got, status, "{path}");
        assert_security_headers(path, &headers);
    }
}

/// TD-SEC-04 (SEC-034): a contact or photo page is `no-store`; other pages leave caching alone.
#[tokio::test]
async fn contact_pages_are_never_stored() {
    let (_, contact) = headers_of("/contact").await;
    assert_eq!(value(&contact, "cache-control"), "no-store");
    let (_, page) = headers_of("/page").await;
    assert!(page.get(header::CACHE_CONTROL).is_none());
}

/// TD-SEC-04 (SEC-012): `build_router` layers the headers outermost, over every route it merges.
#[test]
fn the_server_layers_the_headers_over_the_whole_app() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../app/server/src/lib.rs");
    let source = std::fs::read_to_string(path).expect("server lib.rs");
    let body = source
        .split("pub fn build_router(")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("build_router");
    let last_layer = body.rsplit(".layer(").next().expect("a layer");
    assert!(
        last_layer.starts_with("axum::middleware::from_fn(security_headers))"),
        "security_headers must be build_router's last (outermost) layer"
    );
}
