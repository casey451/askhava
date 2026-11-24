//! The served service-area settings, "This inbox is shared" switch and homeowner-calls contact
//! card (BIZ-010, BIZ-058, SEC-027, BIZ-046, SEC-019, A11Y-030; T733, D-4727, D-4822, D-5318).
//!
//! [`routes`] serves `GET` and `POST` [`SERVICE_AREA_SETTINGS_ROUTE`] over `portals`'
//! `service_area_settings_page` and `change_service_area`, `POST` [`SHARED_INBOX_ROUTE`] over
//! `set_inbox_shared` and `GET` [`CONTACT_CARD_ROUTE`] over `contact_card`; `build_router` always
//! merges it. Every route reads `business_principal` first (no business person: `signed_out`), is
//! `no-store`, and each `POST` passes `refuse_cross_site` and the CSRF form check and parses its
//! form once (an unknown, repeated or bad field is a 422 that changes nothing, SEC-010).
//!
//! The settings page shows the stored base ZIP and radius to the owner or a "Confirm and billing"
//! member; anyone else gets the one not-found page and an `authz_denied` event (SEC-019,
//! SEC-048). A refused change re-renders with its problems and the typed values (422, A11Y-030);
//! a saved one renders with its status line. The switch acts on the signed-in member's own inbox:
//! with no step-up in the last 5 minutes it is a 303 to the step-up page returning to the portal
//! home, else (or when turning it on without a texting phone, which changes nothing) a 303 to the
//! home. Whether the member has a texting phone comes from `sign_in_lines` alone; no phone value
//! leaves the vault. The card holds only the current masked pool numbers, never a homeowner's.

use std::net::IpAddr;
use std::sync::Arc;

use askhava_adapters::api::PoolNumber;
use askhava_businesses::api::{service_area, sign_in_lines};
use askhava_core::api::{Clock, SystemClock};
use askhava_portals::api::{
    AreaChangeError, AreaSettings, BUSINESS_HOME_ROUTE, BusinessPrincipal, CONTACT_CARD_ROUTE,
    CodeTo, CookieJar, CsrfField, CsrfForm, Need, SERVICE_AREA_SETTINGS_ROUTE, SHARED_INBOX_ROUTE,
    SecurityEvent, SecurityEventKind, SharedInboxError, WebSecurity, admit_security_event,
    business_principal, change_service_area, contact_card, contact_card_name, csrf_token, no_store,
    record_security_event, refuse_cross_site, security_event_client, service_area_settings_page,
    set_inbox_shared, signed_out,
};
use axum::Router;
use axum::extract::{Extension, FromRef, Request, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, LOCATION};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use secrecy::SecretString;
use sqlx::PgPool;

use crate::inbound_voice::VoiceHooks;

/// The routes' state.
#[derive(Clone)]
struct Settings {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    numbers: Arc<[PoolNumber]>,
}

impl FromRef<Settings> for WebSecurity {
    fn from_ref(state: &Settings) -> Self {
        state.security.clone()
    }
}

/// The request's client address, for the SEC-048 event cap; never stored (SEC-040).
#[derive(Clone, Copy)]
struct ClientIp(IpAddr);

/// BIZ-010, SEC-027, BIZ-046: the three paths over `pool`, the card listing the masked `numbers`.
pub fn routes(pool: PgPool, numbers: &[PoolNumber]) -> Router {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Settings {
        pool,
        clock,
        security: security.clone(),
        numbers: numbers.into(),
    };
    Router::new()
        .route(SERVICE_AREA_SETTINGS_ROUTE, get(area_get).post(area_post))
        .route(SHARED_INBOX_ROUTE, post(shared_post))
        .route(CONTACT_CARD_ROUTE, get(card_get))
        .route_layer(from_fn(note_client))
        .with_state(state)
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store))
}

/// The routes `build_router` merges: the card lists the voice hooks' masked pool numbers, else
/// none (BIZ-046, CALL-017), as the portal home's calls card does.
pub fn served(pool: PgPool, voice: Option<&Arc<VoiceHooks>>) -> Router {
    let numbers = voice.map(|v| v.pool.numbers().to_vec()).unwrap_or_default();
    routes(pool, &numbers)
}

/// Notes the client address (SEC-040) for the event cap.
async fn note_client(mut request: Request, next: Next) -> Response {
    let ip = security_event_client(&request);
    request.extensions_mut().insert(ClientIp(ip));
    next.run(request).await
}

/// A failed read, change or render: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "business settings route failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// A 303 to `path`.
fn see_other(path: &'static str) -> Response {
    (
        StatusCode::SEE_OTHER,
        [(LOCATION, HeaderValue::from_static(path))],
    )
        .into_response()
}

/// SEC-019, SEC-048: the portal's one not-found answer, recorded as an `authz_denied` event for
/// `who` under the per-client cap; a failure to record is logged once and never changes it.
async fn denied(
    state: &Settings,
    (ip, route): (IpAddr, &str),
    who: &BusinessPrincipal,
) -> Response {
    let now = state.clock.now();
    if admit_security_event(SecurityEventKind::AuthzDenied, ip, route, now) {
        let event = SecurityEvent {
            kind: SecurityEventKind::AuthzDenied,
            route,
            request_id: None,
            actor_ref: Some(who.member()),
        };
        let recorded = match state.pool.acquire().await {
            Ok(mut db) => record_security_event(&mut db, &event, now).await.is_ok(),
            Err(_) => false,
        };
        if !recorded {
            tracing::error!(route, "security event not recorded");
        }
    }
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><title>Page not found</title></head><body><main>\
         <h1>We can't find that page</h1><p>The link may be old, or it may be for a different \
         business account. <a href=\"{BUSINESS_HOME_ROUTE}\">Go to your business home</a>.</p>\
         </main></body></html>"
    );
    (StatusCode::NOT_FOUND, Html(page)).into_response()
}

/// SEC-004: the signed-in business person, else the signed-out answer.
async fn signed_in(state: &Settings, jar: &CookieJar) -> Result<BusinessPrincipal, Box<Response>> {
    let Ok(mut db) = state.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    match business_principal(&mut db, jar, state.clock.now()).await {
        Ok(Some(who)) => Ok(who),
        Ok(None) => Err(Box::new(signed_out())),
        Err(_) => Err(Box::new(failed("business principal"))),
    }
}

/// The posted service-area form: a missing field is empty, so `ServiceArea::parse` refuses it.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AreaForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    base_zip: String,
    #[serde(default)]
    radius: String,
}

impl CsrfField for AreaForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// The posted switch: `shared` is `yes` (mark it) or `no` (unmark it); a missing one is empty, so
/// the person is read before it is refused.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InboxForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    shared: String,
}

impl CsrfField for InboxForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// The settings page for `area` with `status`, or a 500 if it does not render.
fn area_page(status: StatusCode, area: &AreaSettings<'_>, jar: &CookieJar) -> Response {
    let csrf = csrf_token(jar).unwrap_or_default();
    match service_area_settings_page(area, &csrf) {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => failed("service area page"),
    }
}

/// `GET` [`SERVICE_AREA_SETTINGS_ROUTE`] (BIZ-010, BIZ-058, SEC-019): the stored base ZIP and
/// radius, empty before sign-up asked them, for the owner or a "Confirm and billing" member.
/// Changes nothing.
async fn area_get(
    State(state): State<Settings>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
) -> Response {
    let who = match signed_in(&state, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    if !who.may(who.business(), Need::ConfirmAndBilling) {
        return denied(&state, (ip, SERVICE_AREA_SETTINGS_ROUTE), &who).await;
    }
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let Ok(stored) = service_area(&mut db, who.business()).await else {
        return failed("service area");
    };
    let (zip, radius) = stored.map_or_else(Default::default, |s| {
        (
            s.area.base_zip().to_owned(),
            s.area.radius_miles().to_string(),
        )
    });
    let area = AreaSettings {
        base_zip: &zip,
        radius: &radius,
        ..AreaSettings::default()
    };
    area_page(StatusCode::OK, &area, &jar)
}

/// `POST` [`SERVICE_AREA_SETTINGS_ROUTE`] (BIZ-010, BIZ-058, A11Y-030, SEC-019): saves the base
/// ZIP and radius through `change_service_area`; a refusal is a 422 with its problems and the
/// typed values kept, nothing stored; a save shows the stored values with its status line.
async fn area_post(
    State(state): State<Settings>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<AreaForm>,
) -> Response {
    let who = match signed_in(&state, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let typed = (form.base_zip.as_str(), form.radius.as_str());
    let done = change_service_area(&mut db, &who, typed, state.clock.now()).await;
    drop(db);
    match done {
        Ok(saved) => {
            let radius = saved.radius_miles().to_string();
            let area = AreaSettings {
                base_zip: saved.base_zip(),
                radius: &radius,
                problems: &[],
                saved: true,
            };
            area_page(StatusCode::OK, &area, &jar)
        }
        Err(AreaChangeError::Refused(problems)) => {
            let area = AreaSettings {
                base_zip: typed.0,
                radius: typed.1,
                problems: &problems,
                saved: false,
            };
            area_page(StatusCode::UNPROCESSABLE_ENTITY, &area, &jar)
        }
        Err(AreaChangeError::NotAllowed) => {
            denied(&state, (ip, SERVICE_AREA_SETTINGS_ROUTE), &who).await
        }
        Err(AreaChangeError::Database) => failed("service area change"),
    }
}

/// `POST` [`SHARED_INBOX_ROUTE`] (SEC-027, SEC-038, D-333, D-478): marks or unmarks the signed-in
/// member's own inbox shared through `set_inbox_shared`; a stale step-up is a 303 to the step-up
/// page, anything else a 303 to the portal home (where a refused "mark" still shows unmarked).
async fn shared_post(
    State(state): State<Settings>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<InboxForm>,
) -> Response {
    let who = match signed_in(&state, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let shared = match form.shared.as_str() {
        "yes" => true,
        "no" => false,
        _ => return StatusCode::UNPROCESSABLE_ENTITY.into_response(),
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let Ok(lines) = sign_in_lines(&mut db, who.member()).await else {
        return failed("sign-in lines");
    };
    let code_to = if lines.is_some_and(|l| !l.texting_lines.is_empty()) {
        CodeTo::Phone(SecretString::default())
    } else {
        CodeTo::Email(SecretString::default())
    };
    let now = state.clock.now();
    match set_inbox_shared(&mut db, &who, (shared, &code_to), now).await {
        Ok(()) | Err(SharedInboxError::NeedsTextPhone) => see_other(BUSINESS_HOME_ROUTE),
        Err(SharedInboxError::NeedsCode) => see_other(STEP_UP_TO_HOME),
        Err(SharedInboxError::Database) => failed("shared inbox"),
    }
}

/// SEC-038: the step-up page, returning to the portal home after the code.
const STEP_UP_TO_HOME: &str = "/b/step-up?return=%2Fb";

/// `GET` [`CONTACT_CARD_ROUTE`] (BIZ-046, CALL-017, D-344): the "<brand> homeowner calls" vCard
/// download holding exactly the current masked pool numbers; no homeowner detail. Changes nothing.
async fn card_get(State(state): State<Settings>, jar: CookieJar) -> Response {
    if let Err(answer) = signed_in(&state, &jar).await {
        return *answer;
    }
    let name = format!("attachment; filename=\"{}.vcf\"", contact_card_name());
    let Ok(disposition) = HeaderValue::from_str(&name) else {
        return failed("card name");
    };
    let kind = HeaderValue::from_static("text/vcard; charset=utf-8");
    let headers = [(CONTENT_TYPE, kind), (CONTENT_DISPOSITION, disposition)];
    (headers, contact_card(&state.numbers)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use askhava_portals::api::BUSINESS_STEP_UP_ROUTE;

    #[test]
    fn the_step_up_return_is_the_portal_home() {
        let back = BUSINESS_HOME_ROUTE.replace('/', "%2F");
        assert_eq!(
            STEP_UP_TO_HOME,
            format!("{BUSINESS_STEP_UP_ROUTE}?return={back}")
        );
    }
}
