//! The web process's whole-request time limit and panic answer (T286, D-1870, D-2208; SEC-012).
//!
//! `build_router` puts both inside the security-headers and request-ID layers, so every answer
//! here still carries the SEC-012 headers and the `x-request-id`. A request whose handler or body
//! stalls past [`RequestTimeout`] is answered `503 Service Unavailable` with `Retry-After: 5` and
//! `Cache-Control: no-store`, and its handler future is dropped: every write a handler makes is
//! in a transaction or keyed, so nothing half-done commits, and a Confirm press cut during its
//! Stripe call leaves its attempt `sent`, never retried until Stripe says it failed (FEE-037). A
//! handler that panics is answered 500 with `no-store` and an empty body, so no panic message
//! reaches the client.

use std::any::Any;
use std::time::Duration;

use askhava_core::api::Settings;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tower_http::catch_panic::CatchPanicLayer;

/// Seconds a client is asked to wait before retrying a timed-out request (D-1870).
const RETRY_AFTER_SECS: &str = "5";

/// The whole-request time limit, from `ASKHAVA_REQUEST_TIMEOUT_SECS` (10 to 300 seconds, checked
/// by `Settings::from_pairs`; default 60, D-1870).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestTimeout(Duration);

impl RequestTimeout {
    /// The default limit, 60 seconds: longer than three Stripe calls at the adapters' 15-second
    /// limit, the longest chain of bounded vendor calls one handler makes (D-1870, FEE-037).
    pub const DEFAULT: Self = Self(Duration::from_secs(60));

    /// The limit the settings name (`ASKHAVA_REQUEST_TIMEOUT_SECS`, D-1870).
    #[must_use]
    pub fn from_settings(settings: &Settings) -> Self {
        Self(Duration::from_secs(settings.request_timeout_secs()))
    }

    /// The limit as a duration.
    #[must_use]
    pub fn duration(self) -> Duration {
        self.0
    }
}

/// Middleware (`axum::middleware::from_fn_with_state`) that cuts a request still unanswered at
/// `limit`, body reading included, with [`timed_out`] and drops its handler (D-1870, FEE-037).
pub async fn limit_request(
    State(limit): State<RequestTimeout>,
    request: Request,
    next: Next,
) -> Response {
    match tokio::time::timeout(limit.0, next.run(request)).await {
        Ok(response) => response,
        Err(_) => {
            tracing::warn!(
                limit_secs = limit.0.as_secs(),
                "a request was cut at the time limit"
            );
            timed_out()
        }
    }
}

/// The answer to a cut request: 503 with `Retry-After: 5` and `Cache-Control: no-store`, empty
/// body; the outer layers add the SEC-012 headers and the request ID (D-1870).
#[must_use]
pub fn timed_out() -> Response {
    let mut response = StatusCode::SERVICE_UNAVAILABLE.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::RETRY_AFTER,
        HeaderValue::from_static(RETRY_AFTER_SECS),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// The panic layer `build_router` puts innermost: a panicking handler is answered by
/// [`panicked`], and the process keeps serving (SEC-012, D-2208).
#[must_use]
pub fn catch_panic() -> CatchPanicLayer<fn(Box<dyn Any + Send + 'static>) -> Response> {
    CatchPanicLayer::custom(panicked as fn(Box<dyn Any + Send + 'static>) -> Response)
}

/// The answer to a panic: 500 with `Cache-Control: no-store` and an empty body; the panic payload
/// is dropped unread, so its message reaches neither the client nor the log (SEC-012, D-2208).
#[must_use]
pub fn panicked(_payload: Box<dyn Any + Send + 'static>) -> Response {
    tracing::error!("a request handler panicked");
    let mut response = StatusCode::INTERNAL_SERVER_ERROR.into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
