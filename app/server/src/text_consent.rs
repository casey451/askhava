//! The served business text-consent page (CON-019, BIZ-036, TXT-035, SEC-004, SEC-009; T522,
//! D-3879, D-3881).
//!
//! [`routes`] serves `GET` and `POST` [`TEXT_CONSENT_ROUTE`] over `portals`' `text_consent_page`
//! and `record_text_consent`; `build_router` always merges it. Both read `business_principal`
//! first (no business person: `signed_out`, the 4xx with the sign-in link), are `no-store`, and
//! the `POST` passes `refuse_cross_site` and the CSRF form check before anything else; an unknown
//! field is a 422 that changes nothing (SEC-010). The business-texting switch is
//! `messaging::api::load_lines`' stored business line, never a constant: while it is off the page
//! shows no box and a `POST` stores nothing. A `POST` stores a ticked box only for a phone the
//! signed-in member linked and only for the number version shown; a changed number shows the page
//! again with the new last 4 digits. No whole number is read here (PRIV-002).

use std::sync::Arc;

use askhava_businesses::api::member_text_lines;
use askhava_core::api::{Clock, SystemClock};
use askhava_messaging::api::load_lines;
use askhava_portals::api::{
    BusinessPrincipal, CookieJar, CsrfField, CsrfForm, TEXT_CONSENT_ROUTE, TextConsentNote,
    TextConsentOutcome, WebSecurity, business_principal, csrf_token, no_store, record_text_consent,
    refuse_cross_site, signed_out, text_consent_page,
};
use axum::Router;
use axum::extract::{FromRef, State};
use axum::http::StatusCode;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The routes' state.
#[derive(Clone)]
struct Texts {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
}

impl FromRef<Texts> for WebSecurity {
    fn from_ref(state: &Texts) -> Self {
        state.security.clone()
    }
}

/// CON-019, BIZ-036: `GET` and `POST` [`TEXT_CONSENT_ROUTE`] over `pool`.
pub fn routes(pool: PgPool) -> Router {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Texts {
        pool,
        clock,
        security: security.clone(),
    };
    Router::new()
        .route(TEXT_CONSENT_ROUTE, get(page_get).post(page_post))
        .with_state(state)
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store))
}

/// A failed read, change or render: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "business text consent route failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// SEC-004: the signed-in business person, else the signed-out answer.
async fn signed_in(state: &Texts, jar: &CookieJar) -> Result<BusinessPrincipal, Box<Response>> {
    let Ok(mut db) = state.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    match business_principal(&mut db, jar, state.clock.now()).await {
        Ok(Some(who)) => Ok(who),
        Ok(None) => Err(Box::new(signed_out())),
        Err(_) => Err(Box::new(failed("business principal"))),
    }
}

/// TXT-035: the stored business-texting switch.
async fn texting_on(db: &mut PgConnection) -> Option<bool> {
    let lines = load_lines(db, (None, None)).await.ok()?;
    Some(lines.business.texting_on)
}

/// The page for `who` with `note` and `status`, its lines read now; texting off shows no box.
async fn page(
    db: &mut PgConnection,
    (who, jar): (&BusinessPrincipal, &CookieJar),
    (on, note): (bool, &TextConsentNote),
    status: StatusCode,
) -> Response {
    let lines = if on {
        match member_text_lines(db, who.business(), who.member()).await {
            Ok(lines) => lines,
            Err(_) => return failed("text lines"),
        }
    } else {
        Vec::new()
    };
    let csrf = csrf_token(jar).unwrap_or_default();
    match text_consent_page(&lines, on, note, &csrf) {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => failed("text consent page"),
    }
}

/// `GET` [`TEXT_CONSENT_ROUTE`] (CON-019, BIZ-036, TXT-035): the member's own phones, each with
/// "Texts on" or the unticked box. Changes nothing.
async fn page_get(State(state): State<Texts>, jar: CookieJar) -> Response {
    let who = match signed_in(&state, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let Some(on) = texting_on(&mut db).await else {
        return failed("texting switch");
    };
    let note = TextConsentNote::Nothing;
    page(&mut db, (&who, &jar), (on, &note), StatusCode::OK).await
}

/// The posted box: the phone's line ID and the number version shown, and `consent` when ticked;
/// a missing field is empty.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsentForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    phone: String,
    #[serde(default)]
    number_version: String,
    #[serde(default)]
    consent: String,
}

impl CsrfField for ConsentForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

impl ConsentForm {
    /// The phone and the number version shown, when both parse.
    fn phone(&self) -> Option<(Uuid, i32)> {
        let line = Uuid::try_parse(&self.phone).ok()?;
        Some((line, self.number_version.parse().ok()?))
    }
}

/// `POST` [`TEXT_CONSENT_ROUTE`] (CON-019, BIZ-036, TXT-035, D-2707): stores the ticked box for
/// one of the member's own phones at the number version shown; an unticked box is a 422 with the
/// error, a changed number or another phone shows the page again, and texting off stores nothing.
async fn page_post(
    State(state): State<Texts>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<ConsentForm>,
) -> Response {
    let who = match signed_in(&state, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let Some(on) = texting_on(&mut db).await else {
        return failed("texting switch");
    };
    let posted = (on, form.phone(), form.consent == "yes");
    let done = record_text_consent(&mut db, &who, posted, state.clock.now()).await;
    let note = match done {
        Ok(TextConsentOutcome::TextingOff) => TextConsentNote::Nothing,
        Ok(TextConsentOutcome::Page(note)) => note,
        Err(_) => return failed("record consent"),
    };
    let status = if matches!(note, TextConsentNote::NotTicked(_)) {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::OK
    };
    page(&mut db, (&who, &jar), (on, &note), status).await
}
