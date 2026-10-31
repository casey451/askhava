//! The shared web security every browser-facing router uses (SEC-009, SEC-011, SEC-012, SEC-013,
//! SEC-034, SEC-042, SEC-048, D-480, D-943).
//!
//! [`security_headers`] sets the SEC-012 headers on every response (`server` layers it over the
//! whole app); [`no_store`] adds `Cache-Control: no-store` on contact and photo pages (SEC-034).
//! The CSP allows scripts, styles, images and fonts from the app's own origin only, so no
//! third-party script or font host can load (SEC-013). [`refuse_cross_site`] turns away a
//! state-changing request a browser marks as cross-site by `Sec-Fetch-Site` or `Origin`; it is
//! layered on the portal and site routers, never on webhooks, whose callers send neither header.
//! [`CsrfForm`] parses a posted form and refuses it unless its `csrf` field matches the request's
//! own cookie. Each refusal is a 403 and a security event: one log line with kind, route template,
//! request ID and actor reference only, and a row whose count per kind over the last hour raises
//! one High alert at [`EVENTS_PER_HOUR_ALERT`] (SEC-048). Autoescaping is never turned off
//! (SEC-011): the template scan in `scripts/check_rust_rules.py` and this file's test guard it.

use std::sync::Arc;

use askhava_core::api::Clock;
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use axum::extract::{Form, FromRef, FromRequest, MatchedPath, Request, State};
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::CookieJar;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde::de::DeserializeOwned;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::session::{PortalsError, csrf_matches};

/// The `Content-Security-Policy`: own origin only, no inline or eval script, no plugins, no
/// framing (SEC-012, SEC-013). A page that needs an inline script adds a per-response nonce.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; \
    style-src 'self'; img-src 'self'; font-src 'self'; connect-src 'self'; form-action 'self'; \
    base-uri 'none'; object-src 'none'; frame-ancestors 'none'";
/// One year with subdomains and no `preload`: the app is the only HSTS source (SEC-012, SEC-054).
pub const STRICT_TRANSPORT_SECURITY: &str = "max-age=31536000; includeSubDomains";
/// Camera, microphone and geolocation off (SEC-012).
pub const PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=()";

const SECURITY_HEADERS: [(HeaderName, &str); 5] = [
    (header::CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY),
    (header::STRICT_TRANSPORT_SECURITY, STRICT_TRANSPORT_SECURITY),
    (
        HeaderName::from_static("permissions-policy"),
        PERMISSIONS_POLICY,
    ),
    (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
    // Sent on every page, which covers every token page (SEC-012, SEC-033).
    (header::REFERRER_POLICY, "no-referrer"),
];

/// Within an hour, the count of one security event kind that raises its High alert (SEC-048,
/// TD-SEC-26, D-943).
pub const EVENTS_PER_HOUR_ALERT: i64 = 50;

/// Middleware: the SEC-012 headers on every response, replacing any a handler set.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    for (name, value) in SECURITY_HEADERS {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    response
}

/// Middleware for every page showing a homeowner's contact details or job photos:
/// `Cache-Control: no-store` (SEC-034).
pub async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// What the refusals need: the pool for the security event and the clock (SEC-048).
#[derive(Clone)]
pub struct WebSecurity {
    pool: PgPool,
    clock: Arc<dyn Clock>,
}

impl WebSecurity {
    /// The refusals' state over `pool`, timed by `clock`.
    #[must_use]
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self { pool, clock }
    }

    async fn refuse(&self, kind: SecurityEventKind, site: EventSite) -> Response {
        let event = SecurityEvent {
            kind,
            route: &site.route,
            request_id: site.request_id,
            actor_ref: None,
        };
        let recorded = match self.pool.acquire().await {
            Ok(mut db) => record_security_event(&mut db, &event, self.clock.now()).await,
            Err(_) => Err(PortalsError::Database),
        };
        if let Err(error) = recorded {
            tracing::error!(%error, route = event.route, "security event not recorded");
        }
        StatusCode::FORBIDDEN.into_response()
    }
}

/// The route template and request ID of a request, never its path (SEC-048, SEC-033).
struct EventSite {
    route: String,
    request_id: Option<Uuid>,
}

impl EventSite {
    fn of(request: &Request) -> Self {
        let route = request
            .extensions()
            .get::<MatchedPath>()
            .map_or("unmatched", MatchedPath::as_str);
        let request_id = request
            .headers()
            .get("x-request-id")
            .and_then(|id| Uuid::try_parse_ascii(id.as_bytes()).ok());
        Self {
            route: route.to_owned(),
            request_id,
        }
    }
}

/// Whether a request that changes state comes from another site: a `Sec-Fetch-Site` other than
/// `same-origin`, an `Origin` whose host is not `host`, or neither header (SEC-042).
#[must_use]
pub fn is_cross_site(headers: &HeaderMap, host: Option<&str>) -> bool {
    let fetch_site = headers.get("sec-fetch-site");
    if fetch_site.is_some_and(|site| site.as_bytes() != b"same-origin") {
        return true;
    }
    let Some(origin) = headers.get(header::ORIGIN) else {
        return fetch_site.is_none();
    };
    let origin_host = origin.to_str().ok().and_then(|origin| {
        origin
            .strip_prefix("https://")
            .or(origin.strip_prefix("http://"))
    });
    !matches!((origin_host, host), (Some(origin), Some(host)) if origin.eq_ignore_ascii_case(host))
}

/// Middleware: a cross-site POST, PUT, PATCH or DELETE is refused with a 403 and a
/// `csrf_rejected` security event (SEC-042, SEC-048, TD-SEC-21).
pub async fn refuse_cross_site(
    State(security): State<WebSecurity>,
    request: Request,
    next: Next,
) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .or_else(|| request.uri().authority().map(|a| a.as_str()));
    if request.method().is_safe() || !is_cross_site(request.headers(), host) {
        return next.run(request).await;
    }
    let site = EventSite::of(&request);
    security.refuse(SecurityEventKind::CsrfRejected, site).await
}

/// A posted form type that carries the page's CSRF token in its `csrf` field (SEC-009).
pub trait CsrfField {
    /// The token the form posted.
    fn csrf(&self) -> &str;
}

/// A form whose only field is its CSRF token: an action page's single button (SEC-009, SEC-026).
/// A post without the field reaches the check as an empty token and is refused with a 403.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonForm {
    #[serde(default)]
    csrf: String,
}

impl CsrfField for ButtonForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// A posted form whose CSRF token matched the request's own session or pre-session cookie
/// (SEC-009, SEC-042). A missing or wrong token is a 403 and a `csrf_rejected` security event; a
/// form the server cannot parse is axum's own 4xx. Forms post with `novalidate` and keep their
/// submit button enabled; a double submit is the handler's idempotency key's to refuse (A11Y-030,
/// A11Y-031).
#[derive(Debug)]
pub struct CsrfForm<T>(pub T);

impl<S, T> FromRequest<S> for CsrfForm<T>
where
    S: Send + Sync,
    T: DeserializeOwned + CsrfField + Send,
    WebSecurity: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request(request: Request, state: &S) -> Result<Self, Response> {
        let jar = CookieJar::from_headers(request.headers());
        let site = EventSite::of(&request);
        let Form(form) = Form::<T>::from_request(request, state)
            .await
            .map_err(IntoResponse::into_response)?;
        if csrf_matches(&jar, form.csrf()) {
            return Ok(Self(form));
        }
        let security = WebSecurity::from_ref(state);
        Err(security.refuse(SecurityEventKind::CsrfRejected, site).await)
    }
}

/// The kinds of security event SEC-048 logs and counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityEventKind {
    /// A failed sign-in step.
    AuthFailed,
    /// A signed-in person asked for something not theirs.
    AuthzDenied,
    /// A cross-site request or a missing or wrong CSRF token.
    CsrfRejected,
    /// A webhook whose signature did not verify.
    WebhookSignatureFailed,
}

impl SecurityEventKind {
    /// The code name stored in `portals.security_event.kind` and logged.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AuthFailed => "auth_failed",
            Self::AuthzDenied => "authz_denied",
            Self::CsrfRejected => "csrf_rejected",
            Self::WebhookSignatureFailed => "webhook_signature_failed",
        }
    }

    fn alert_kind(self) -> &'static str {
        match self {
            Self::AuthFailed => "portals.auth_failed_burst",
            Self::AuthzDenied => "portals.authz_denied_burst",
            Self::CsrfRejected => "portals.csrf_rejected_burst",
            Self::WebhookSignatureFailed => "portals.webhook_signature_failed_burst",
        }
    }
}

/// One security event. Every field is a code name, a route template or an ID: there is no
/// place for a phone, a name or a path (SEC-048, PRIV-016).
#[derive(Debug, Clone, Copy)]
pub struct SecurityEvent<'a> {
    /// What happened.
    pub kind: SecurityEventKind,
    /// The matched route template, such as `/q/{token}`, or `unmatched`.
    pub route: &'a str,
    /// The request's `x-request-id`, when it was a UUID.
    pub request_id: Option<Uuid>,
    /// The session or record ID of the actor, when known.
    pub actor_ref: Option<Uuid>,
}

/// Logs `event` once and stores it; the event that brings its kind's count over the last hour to
/// [`EVENTS_PER_HOUR_ALERT`] raises one High alert (SEC-048, TD-SEC-26). Returns whether it did.
/// One lock per kind makes concurrent events count one at a time, so the alert is raised once.
///
/// # Errors
///
/// [`PortalsError::Database`] or [`PortalsError::Alert`]; nothing is stored then.
pub async fn record_security_event(
    db: &mut PgConnection,
    event: &SecurityEvent<'_>,
    now: Timestamp,
) -> Result<bool, PortalsError> {
    let request_id = event.request_id.map(|id| id.to_string());
    let actor_ref = event.actor_ref.map(|id| id.to_string());
    tracing::warn!(
        kind = event.kind.as_str(),
        route = event.route,
        request_id = request_id.as_deref(),
        actor_ref = actor_ref.as_deref(),
        "security event"
    );
    let since = now - SignedDuration::from_hours(1);
    let mut tx = db.begin().await.map_err(|_| PortalsError::Database)?;
    // The lock comes first, in its own statement, so the count below sees every event committed
    // before it.
    sqlx::query_scalar!(
        r#"SELECT 1 AS "locked!" FROM (SELECT pg_advisory_xact_lock(hashtext($1))) AS lock"#,
        event.kind.alert_kind(),
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| PortalsError::Database)?;
    let count = sqlx::query_scalar!(
        r#"INSERT INTO portals.security_event (kind, route, request_id, actor_ref, at)
           VALUES ($1, $2, $3, $4, $5)
           RETURNING (SELECT count(*) FROM portals.security_event
                      WHERE kind = $1 AND at > $6) + 1 AS "count!""#,
        event.kind.as_str(),
        event.route,
        event.request_id,
        event.actor_ref,
        now.to_sqlx(),
        since.to_sqlx(),
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| PortalsError::Database)?;
    let alert = count == EVENTS_PER_HOUR_ALERT;
    if alert {
        let burst = NewAlert {
            kind: event.kind.alert_kind(),
            severity: Severity::High,
            subject: None,
        };
        raise_alert(&mut tx, &burst)
            .await
            .map_err(|_| PortalsError::Alert)?;
    }
    tx.commit().await.map_err(|_| PortalsError::Database)?;
    Ok(alert)
}

#[cfg(test)]
mod tests {
    //! Autoescaping is on through the shared layout (TD-SEC-04; SEC-011).

    use askama::Template;

    #[derive(Template)]
    #[template(
        source = "{% extends \"base.html\" %}{% block title %}Test{% endblock %}\
                  {% block main %}<p>{{ text }}</p>{% endblock %}",
        ext = "html"
    )]
    struct Echo<'a> {
        text: &'a str,
    }

    /// Homeowner text with a script tag renders as text inside the shared layout (TD-SEC-04).
    #[test]
    fn homeowner_text_renders_as_text() {
        let page = Echo {
            text: "<script>alert(1)</script> & \"Fake Name\"",
        }
        .render()
        .unwrap_or_default();
        assert!(
            page.contains("<main id=\"main\""),
            "the shared layout rendered"
        );
        assert!(
            page.contains("&#60;script&#62;alert(1)&#60;/script&#62; &#38; &#34;Fake Name&#34;")
        );
        assert!(!page.contains("<script"));
    }
}
