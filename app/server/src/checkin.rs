//! The check-in answer page, served (TXT-034, SEC-026, T231, D-2128, D-2429).
//!
//! The emailed check-in's buttons (`messaging::api::render_check_in`) open `GET /c/{token}/{answer}`,
//! which shows the answer and changes nothing; only its `POST`, with the pre-session CSRF token,
//! records the answer, once. The page and its rules are `portals::api::checkin_routes` (a test crate
//! may not depend on `server`, D-736, so the authorization matrix merges the same router); this
//! module gives it the system clock, and [`build_router`](crate::build_router) merges it. Every
//! response is `no-store` and `noindex`, and the `POST` passes `refuse_cross_site` first.

use std::sync::Arc;

use askhava_core::api::SystemClock;
use askhava_portals::api::checkin_routes;
use axum::Router;
use sqlx::PgPool;

/// TXT-034, SEC-026: the check-in answer page's routes over `pool`, timed by the system clock.
pub fn routes(pool: PgPool) -> Router {
    checkin_routes(pool, Arc::new(SystemClock))
}
