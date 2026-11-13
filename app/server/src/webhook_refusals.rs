//! Webhook refusals as security events (SEC-048, SEC-016, D-2207, T319).
//!
//! [`layer`] goes on the Stripe, Twilio text (inbound and status), Twilio voice and Twilio
//! call-callback (`voice_status`: status and safety key) routers only, as a route layer. On those
//! routes a 403 means one thing: a source or signature refusal (`adapters::api::refuse`'s
//! `REJECTED_STATUS`, the only 403 any of their handlers answers; every other failure is 200 or
//! 500), so the layer reads the status alone, never the body. Each 403 records one
//! `webhook_signature_failed` event with the route template and request ID and no actor, through
//! `portals::api::record_security_event`, under the same per-minute cap (`admit_security_event`)
//! the portals' refusals use, checked before the pool is touched. The refusal's own log line
//! (vendor and reason) stays the handler's; the 403 is unchanged.

use std::sync::Arc;

use askhava_core::api::Clock;
use askhava_portals::api::{
    SecurityEvent, SecurityEventKind, admit_security_event, record_security_event,
    security_event_client,
};
use axum::extract::{MatchedPath, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// What the layer needs: the pool for the event row and the clock for its time and the cap's
/// minute.
#[derive(Clone)]
pub struct RefusalLog {
    pool: PgPool,
    clock: Arc<dyn Clock>,
}

impl RefusalLog {
    /// The layer's state over `pool`, timed by `clock`.
    #[must_use]
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self { pool, clock }
    }
}

/// Puts the refusal layer on `router`'s routes (a route layer, so no other route sees it).
pub fn layer(router: axum::Router, log: RefusalLog) -> axum::Router {
    router.route_layer(axum::middleware::from_fn_with_state(log, record_refusal))
}

/// Middleware: runs the webhook handler and, when it answers 403, records one capped
/// `webhook_signature_failed` event (SEC-048, SEC-016). The answer is passed on unchanged.
pub async fn record_refusal(
    State(log): State<RefusalLog>,
    request: Request,
    next: Next,
) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", MatchedPath::as_str)
        .to_owned();
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|id| Uuid::try_parse_ascii(id.as_bytes()).ok());
    let client = security_event_client(&request);
    let response = next.run(request).await;
    if response.status() != StatusCode::FORBIDDEN {
        return response;
    }
    let kind = SecurityEventKind::WebhookSignatureFailed;
    let now = log.clock.now();
    if !admit_security_event(kind, client, &route, now) {
        return response;
    }
    let event = SecurityEvent {
        kind,
        route: &route,
        request_id,
        actor_ref: None,
    };
    let recorded = match log.pool.acquire().await {
        Ok(mut db) => record_security_event(&mut db, &event, now)
            .await
            .map(|_| ()),
        Err(_) => Err(askhava_portals::api::PortalsError::Database),
    };
    if let Err(error) = recorded {
        let request_id = request_id.map(|id| id.to_string());
        tracing::error!(
            %error,
            kind = kind.as_str(),
            route,
            request_id = request_id.as_deref(),
            "security event not recorded"
        );
    }
    response
}
