//! T493: the Confirm page's CSP marker (SEC-012, SEC-013, FEE-014, D-1480 (c), D-3813). The
//! [`security_headers`] layer gives a response carrying [`StripeConfirmCsp`] the own-origin
//! policy plus only Stripe.js's host in `script-src` and `frame-src` and Stripe's API host in
//! `connect-src`; every other response, an error page and a handler that writes its own CSP
//! header included, gets the strict policy. The served Confirm route's own cases are T261's.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::middleware::from_fn;
use axum::response::IntoResponse;
use axum::routing::get;
use tower::ServiceExt;

use crate::security::{
    CONFIRM_CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY, StripeConfirmCsp, security_headers,
};

const LOOSE: &str = "default-src *; script-src * 'unsafe-inline' 'unsafe-eval'";

/// A router under the layer: the marked Confirm stand-in and four unmarked pages.
fn app() -> Router {
    Router::new()
        .route("/confirm", get(|| async { (StripeConfirmCsp, "confirm") }))
        .route(
            "/confirm-own-header",
            get(|| async {
                (
                    StripeConfirmCsp,
                    [(header::CONTENT_SECURITY_POLICY, LOOSE)],
                    "x",
                )
            }),
        )
        .route("/plain", get(|| async { "plain" }))
        .route(
            "/own-header",
            get(|| async { ([(header::CONTENT_SECURITY_POLICY, LOOSE)], "own") }),
        )
        .route(
            "/broken",
            get(|| async { StatusCode::INTERNAL_SERVER_ERROR.into_response() }),
        )
        .layer(from_fn(security_headers))
}

/// The one CSP header `path` answers with, and the answer's status.
async fn csp_of(path: &str) -> (StatusCode, String) {
    let request = Request::get(path).body(Body::empty()).expect("request");
    let response = app().oneshot(request).await.expect("infallible");
    let values: Vec<_> = response
        .headers()
        .get_all(header::CONTENT_SECURITY_POLICY)
        .iter()
        .map(|value| value.to_str().expect("ascii").to_owned())
        .collect();
    assert_eq!(values.len(), 1, "{path}: one CSP header, got {values:?}");
    (response.status(), values.concat())
}

/// The sources of `directive` in `policy`, or `None` when the policy does not name it.
fn sources<'a>(policy: &'a str, directive: &str) -> Option<Vec<&'a str>> {
    policy.split(';').find_map(|part| {
        let mut words = part.split_whitespace();
        (words.next() == Some(directive)).then(|| words.collect())
    })
}

/// SEC-012, FEE-014: the marked page adds Stripe's hosts and nothing else to the strict policy.
#[tokio::test]
async fn the_marked_page_adds_only_stripe() {
    for path in ["/confirm", "/confirm-own-header"] {
        let (status, policy) = csp_of(path).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(policy, CONFIRM_CONTENT_SECURITY_POLICY, "{path}");
    }
    let confirm = CONFIRM_CONTENT_SECURITY_POLICY;
    assert!(!confirm.contains("'unsafe-inline'") && !confirm.contains("'unsafe-eval'"));
    let stripe = "https://js.stripe.com";
    assert_eq!(sources(confirm, "script-src"), Some(vec!["'self'", stripe]));
    assert_eq!(sources(confirm, "frame-src"), Some(vec!["'self'", stripe]));
    let api = "https://api.stripe.com";
    assert_eq!(sources(confirm, "connect-src"), Some(vec!["'self'", api]));
    assert_eq!(sources(CONTENT_SECURITY_POLICY, "frame-src"), None);
    let mut want = names(CONTENT_SECURITY_POLICY);
    want.push("frame-src");
    want.sort_unstable();
    let mut got = names(confirm);
    got.sort_unstable();
    assert_eq!(
        got, want,
        "the strict policy's directives plus frame-src, each once"
    );
    for name in names(confirm) {
        if !["script-src", "connect-src", "frame-src"].contains(&name) {
            let strict = sources(CONTENT_SECURITY_POLICY, name);
            assert_eq!(sources(confirm, name), strict, "{name} unchanged");
        }
    }
}

/// The directive names of `policy`, in order, repeats kept.
fn names(policy: &str) -> Vec<&str> {
    policy
        .split(';')
        .filter_map(|part| part.split_whitespace().next())
        .collect()
}

/// SEC-012, SEC-013: every unmarked response, an error page and a handler's own CSP header
/// included, gets the strict own-origin policy.
#[tokio::test]
async fn every_other_page_keeps_the_strict_policy() {
    for (path, want) in [
        ("/plain", StatusCode::OK),
        ("/own-header", StatusCode::OK),
        ("/broken", StatusCode::INTERNAL_SERVER_ERROR),
        ("/missing", StatusCode::NOT_FOUND),
    ] {
        let (status, policy) = csp_of(path).await;
        assert_eq!(status, want, "{path}");
        assert_eq!(policy, CONTENT_SECURITY_POLICY, "{path}");
        assert!(!policy.contains("stripe"), "{path}");
    }
}
