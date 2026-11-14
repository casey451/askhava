//! T466: the shared form stylesheet at `/static/portals/forms.css` answers 200 as `text/css`
//! without a session, holding the native-box rules (A11Y-007, A11Y-009, D-3700).

use std::sync::Arc;

use askhava_core::api::SystemClock;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

use crate::feedback::{FORMS_CSS_ROUTE, feedback_routes};

/// A11Y-007, A11Y-009: no cookie, a pool that never connects, and the stylesheet still answers.
#[tokio::test]
async fn forms_css_is_served_without_a_session() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://fake@127.0.0.1:1/none")
        .expect("a lazy pool");
    let app = feedback_routes(pool, Arc::new(SystemClock));
    let request = Request::get(FORMS_CSS_ROUTE)
        .body(Body::empty())
        .expect("request");
    let response = app.oneshot(request).await.expect("infallible");
    assert_eq!(response.status(), StatusCode::OK);
    let kind = response.headers()[header::CONTENT_TYPE]
        .to_str()
        .expect("ascii");
    assert!(kind.starts_with("text/css"), "content type {kind}");
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let css = String::from_utf8(body.to_vec()).expect("utf-8");
    assert!(css.contains("appearance:none"), "{css}");
    assert!(css.contains("#1f1d1b"), "{css}");
}
