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
//! one High alert at [`EVENTS_PER_HOUR_ALERT`], at most one per kind per hour (SEC-048).
//! Autoescaping is never turned off (SEC-011): the template scan in `scripts/check_rust_rules.py`
//! and this file's test guard it.
//!
//! [`admit_security_event`] caps the rows before any pool connection is taken (SEC-048, D-2207,
//! D-3445): at most N rows per minute per (kind, client address as SEC-040 reads it, route), and
//! at most N per (kind, route) across all addresses, so a forged client-address header buys no
//! fresh cap. The rest are only counted, one log line per minute with the count and no address.
//! N is [`configure_security_events`]'s setting, never below [`EVENTS_PER_HOUR_ALERT`], so the
//! hourly burst alert still fires from the stored rows.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, RwLock};

use askhava_core::api::{ClientIpPolicy, Clock};
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use axum::extract::{ConnectInfo, Form, FromRef, FromRequest, MatchedPath, Request, State};
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

use crate::session::{PortalsError, csrf_matches, current_session};

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

/// The default and the floor of the security-event cap: [`EVENTS_PER_HOUR_ALERT`] as a row
/// count (SEC-048, D-2207).
pub const SECURITY_EVENT_CAP_FLOOR: u32 = 50;
const _: () = assert!(SECURITY_EVENT_CAP_FLOOR as i64 == EVENTS_PER_HOUR_ALERT);

/// The rows per minute each cap key may store; [`configure_security_events`] sets it.
static EVENT_CAP: AtomicU32 = AtomicU32::new(SECURITY_EVENT_CAP_FLOOR);
/// How the client address is read (SEC-040); unset, the default policy.
static CLIENT_IP: RwLock<Option<ClientIpPolicy>> = RwLock::new(None);
/// This process's per-minute counts.
static WINDOW: LazyLock<Mutex<CapWindow>> = LazyLock::new(Mutex::default);

/// The cap a setting gives: `None` is [`SECURITY_EVENT_CAP_FLOOR`], and a lower value is raised
/// to it, since the burst alert counts stored rows (SEC-048, D-2207).
#[must_use]
pub fn security_event_cap(setting: Option<u32>) -> u32 {
    setting.map_or(SECURITY_EVENT_CAP_FLOOR, |n| {
        n.max(SECURITY_EVENT_CAP_FLOOR)
    })
}

/// The process-wide setter `server`'s `main` calls once at start-up (D-3134): the cap from
/// `ASKHAVA_SECURITY_EVENTS_PER_MINUTE` (a value below the floor is raised and logged) and the
/// SEC-040 client-address policy. Never called (tests), the floor and the default policy hold.
/// Returns the cap in force.
pub fn configure_security_events(setting: Option<u32>, client_ip: ClientIpPolicy) -> u32 {
    let cap = security_event_cap(setting);
    if let Some(asked) = setting.filter(|&n| n < cap) {
        tracing::warn!(
            asked,
            cap,
            "security event cap raised to the alert threshold"
        );
    }
    EVENT_CAP.store(cap, Ordering::Relaxed);
    *CLIENT_IP.write().unwrap_or_else(PoisonError::into_inner) = Some(client_ip);
    cap
}

/// The request's client address as SEC-040 takes it: the configured policy over the headers and
/// the socket peer (`0.0.0.0` when the router was served without connect info).
#[must_use]
pub fn security_event_client(request: &Request) -> IpAddr {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |info| info.0.ip());
    let policy = CLIENT_IP.read().unwrap_or_else(PoisonError::into_inner);
    match policy.as_ref() {
        Some(policy) => policy.client_ip(request.headers(), peer),
        None => ClientIpPolicy::from_pairs(std::iter::empty())
            .map_or(peer, |policy| policy.client_ip(request.headers(), peer)),
    }
}

/// Whether a security event of `kind` from `client` on `route` at `now` may store its row: true
/// for the first N per (kind, client, route) and per (kind, route) in its minute; the rest are
/// counted and logged once per minute, count only (SEC-048, SEC-016, D-2207). Checked before any
/// pool connection is taken; the 403 is the caller's either way. The count of a minute is logged
/// when the first event of a later minute arrives (no timer, D-3472): after a flood's last minute
/// it waits for the next security event, and none is lost from the rows or the hourly alert.
pub fn admit_security_event(
    kind: SecurityEventKind,
    client: IpAddr,
    route: &str,
    now: Timestamp,
) -> bool {
    let minute = now.as_second().div_euclid(60);
    let cap = EVENT_CAP.load(Ordering::Relaxed);
    let mut window = WINDOW.lock().unwrap_or_else(PoisonError::into_inner);
    window.roll(minute);
    window.admit(minute, (kind, client, route), cap)
}

/// The per-minute counts. Keys carry their minute, so a request timed by an earlier clock counts
/// in its own minute; a new latest minute drops keys older than the minute before it and logs
/// the held counts of earlier minutes (only then, D-3472). A key per address is made only while
/// its route is under the ceiling, so a minute holds at most N address keys per kind and route.
#[derive(Default)]
struct CapWindow {
    latest: i64,
    rows: HashMap<(i64, SecurityEventKind, IpAddr, String), u32>,
    routes: HashMap<(i64, SecurityEventKind, String), u32>,
    held: HashMap<i64, u64>,
}

impl CapWindow {
    fn roll(&mut self, minute: i64) {
        if minute <= self.latest {
            return;
        }
        self.latest = minute;
        self.rows.retain(|key, _| key.0 >= minute - 1);
        self.routes.retain(|key, _| key.0 >= minute - 1);
        for (held_minute, held) in self.held.extract_if(|key, _| *key < minute) {
            tracing::warn!(
                minute = held_minute,
                held,
                "security events over the cap counted, not stored"
            );
        }
    }

    fn admit(&mut self, minute: i64, key: (SecurityEventKind, IpAddr, &str), cap: u32) -> bool {
        let (kind, client, route) = key;
        let route_key = (minute, kind, route.to_owned());
        let route_rows = self.routes.get(&route_key).copied().unwrap_or(0);
        let client_key = (minute, kind, client, route.to_owned());
        let client_rows = self.rows.get(&client_key).copied().unwrap_or(0);
        if route_rows >= cap || client_rows >= cap {
            *self.held.entry(minute).or_default() += 1;
            return false;
        }
        self.routes.insert(route_key, route_rows + 1);
        self.rows.insert(client_key, client_rows + 1);
        true
    }
}

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
        let now = self.clock.now();
        if !admit_security_event(kind, site.client, &site.route, now) {
            return StatusCode::FORBIDDEN.into_response();
        }
        let recorded = match self.pool.acquire().await {
            Ok(mut db) => {
                // The session's row ID only, never the phone or name behind it (SEC-048).
                let actor_ref = match current_session(&mut db, &site.jar, now).await {
                    Ok(session) => session.map(|session| session.id()),
                    Err(_) => None,
                };
                let event = SecurityEvent {
                    kind,
                    route: &site.route,
                    request_id: site.request_id,
                    actor_ref,
                };
                record_security_event(&mut db, &event, now).await
            }
            Err(_) => Err(PortalsError::Database),
        };
        if let Err(error) = recorded {
            let request_id = site.request_id.map(|id| id.to_string());
            tracing::error!(
                %error,
                kind = kind.as_str(),
                route = site.route,
                request_id = request_id.as_deref(),
                "security event not recorded"
            );
        }
        StatusCode::FORBIDDEN.into_response()
    }
}

/// The route template, request ID and cookies of a request, never its path (SEC-048, SEC-033).
/// The cookies only find the session whose row ID is the event's actor reference; the client
/// address only keys the cap and is never stored or logged.
struct EventSite {
    route: String,
    request_id: Option<Uuid>,
    jar: CookieJar,
    client: IpAddr,
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
            jar: CookieJar::from_headers(request.headers()),
            client: security_event_client(request),
        }
    }
}

/// Whether a request that changes state comes from another site: a `Sec-Fetch-Site` other than
/// `same-origin`, an `Origin` that is not `https://` on `host` (a plain `http://` origin is
/// cross-site, since the app is served over HTTPS only, SEC-012), or neither header (SEC-042).
#[must_use]
pub fn is_cross_site(headers: &HeaderMap, host: Option<&str>) -> bool {
    let fetch_site = headers.get("sec-fetch-site");
    if fetch_site.is_some_and(|site| site.as_bytes() != b"same-origin") {
        return true;
    }
    let Some(origin) = headers.get(header::ORIGIN) else {
        return fetch_site.is_none();
    };
    let origin_host = origin
        .to_str()
        .ok()
        .and_then(|origin| origin.strip_prefix("https://"));
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
        let site = EventSite::of(&request);
        let Form(form) = Form::<T>::from_request(request, state)
            .await
            .map_err(IntoResponse::into_response)?;
        if csrf_matches(&site.jar, form.csrf()) {
            return Ok(Self(form));
        }
        let security = WebSecurity::from_ref(state);
        Err(security.refuse(SecurityEventKind::CsrfRejected, site).await)
    }
}

/// The kinds of security event SEC-048 logs and counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
/// at least [`EVENTS_PER_HOUR_ALERT`] raises one High alert, unless an event of that kind in the
/// last hour already raised one, so a steady attack raises at most one alert per kind per hour
/// (SEC-048, TD-SEC-26). Returns whether it did. One lock per kind makes concurrent events count
/// one at a time, so the alert is raised once.
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
    let alert = sqlx::query_scalar!(
        r#"WITH recent AS (
             SELECT count(*) AS events, coalesce(bool_or(raised_alert), false) AS raised
             FROM portals.security_event WHERE kind = $1 AND at > $6)
           INSERT INTO portals.security_event
             (kind, route, request_id, actor_ref, at, raised_alert)
           SELECT $1, $2, $3, $4, $5, recent.events + 1 >= $7 AND NOT recent.raised FROM recent
           RETURNING raised_alert"#,
        event.kind.as_str(),
        event.route,
        event.request_id,
        event.actor_ref,
        now.to_sqlx(),
        (now - SignedDuration::from_hours(1)).to_sqlx(),
        EVENTS_PER_HOUR_ALERT,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| PortalsError::Database)?;
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
