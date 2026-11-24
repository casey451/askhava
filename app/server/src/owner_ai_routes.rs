//! The served owner AI page: the account checklist's AI records and the "AI on scrubbed text"
//! switch (AI-008, AI-018, SEC-006, SEC-034, SEC-042, SEC-043, A11Y-030; T737, D-4727, D-5534).
//!
//! [`routes`] serves `portals`' page: `GET` [`OWNER_AI_ROUTE`] (what the switch still needs, the
//! forms and the switch; reads only) and the `POST`s [`OWNER_AI_TERMS_ROUTE`],
//! [`OWNER_AI_POLICY_ROUTE`] and [`OWNER_AI_SWITCH_ROUTE`] (`portals::owner_ai_change`). The
//! layers and their order are `owner_portal`'s: every response is `no-store`; every `POST` passes
//! `refuse_cross_site` and the CSRF form check before the owner-session check, so a cross-site or
//! wrong-token `POST` is a 403 that changes nothing; no owner session gets `owner_signed_out`.
//! A change without a code from the last 5 minutes is a 401 with the step-up form and records
//! nothing; a bad field, a repeated version or a refused on is a 400 with the page's error
//! summary and the typed values; a change made is a 200 with its line. No AI call is made here.

use std::sync::Arc;

use askhava_core::api::{Clock, SystemClock};
use askhava_portals::api::{
    AiChange, AiShown, CookieJar, CsrfField, CsrfForm, OWNER_AI_POLICY_ROUTE, OWNER_AI_ROUTE,
    OWNER_AI_SWITCH_ROUTE, OWNER_AI_TERMS_ROUTE, OwnerToolError, PolicyForm, StepUpScreen,
    TermsForm, WebSecurity, csrf_token, no_store, owner_ai_change, owner_ai_page,
    owner_step_up_page, refuse_cross_site,
};
use axum::Router;
use axum::extract::{Extension, FromRef, State};
use axum::http::StatusCode;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use sqlx::PgPool;

use crate::owner_signin::{OwnerSite, note_owner_site, require_owner};

#[derive(Clone)]
struct Ai {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
}

impl FromRef<Ai> for WebSecurity {
    fn from_ref(ai: &Ai) -> Self {
        ai.security.clone()
    }
}

/// The provider-terms form with its CSRF token.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TermsBody {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    checked_on: String,
    #[serde(default)]
    training: String,
    #[serde(default)]
    retention_days: String,
    #[serde(default)]
    flagged_days: String,
    #[serde(default)]
    spend_limit: String,
    #[serde(default)]
    expected_cost: String,
    #[serde(default)]
    zero_retention_on: String,
}

/// The policy-version form with its CSRF token.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyBody {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    effective_on: String,
    #[serde(default)]
    ai018: String,
}

/// The switch press: `to` is `on` or `off`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SwitchBody {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    to: String,
}

impl CsrfField for TermsBody {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

impl CsrfField for PolicyBody {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

impl CsrfField for SwitchBody {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// The routes `build_router` merges, over `pool`.
pub fn routes(pool: PgPool) -> Router {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Ai {
        pool,
        clock,
        security: security.clone(),
    };
    Router::new()
        .route(OWNER_AI_ROUTE, get(page))
        .route(OWNER_AI_TERMS_ROUTE, post(terms))
        .route(OWNER_AI_POLICY_ROUTE, post(policy))
        .route(OWNER_AI_SWITCH_ROUTE, post(switch))
        .route_layer(from_fn(note_owner_site))
        .with_state(state)
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store))
}

/// A failed read or render: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "owner ai page failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// The AI page showing `shown`: 400 after a refusal, else 200 (A11Y-030). Reads only.
async fn answer(ai: &Ai, jar: &CookieJar, shown: &AiShown) -> Response {
    let Some(csrf) = csrf_token(jar) else {
        return failed("csrf token");
    };
    let Ok(mut db) = ai.pool.acquire().await else {
        return failed("database");
    };
    let status = [StatusCode::OK, StatusCode::BAD_REQUEST][usize::from(shown.refused())];
    match owner_ai_page(&mut db, &csrf, shown).await {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => failed("owner ai page"),
    }
}

/// `GET` [`OWNER_AI_ROUTE`]: the AI page, for the owner only (AI-008, SEC-006, SEC-034).
async fn page(
    State(ai): State<Ai>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
) -> Response {
    if let Err(refused) = require_owner(&ai.pool, &jar, &site, ai.clock.now()).await {
        return *refused;
    }
    answer(&ai, &jar, &AiShown::default()).await
}

/// One change for the owner in `jar` (AI-008, SEC-043): a stale factor is a 401 with the step-up
/// form (its `authz_denied` event recorded by `portals`), a database failure a 500.
async fn change(ai: &Ai, jar: &CookieJar, site: &OwnerSite, change: AiChange) -> Response {
    let now = ai.clock.now();
    let principal = match require_owner(&ai.pool, jar, site, now).await {
        Ok(principal) => principal,
        Err(refused) => return *refused,
    };
    let Ok(mut db) = ai.pool.acquire().await else {
        return failed("database");
    };
    match owner_ai_change(&mut db, &principal, change, now).await {
        Ok(shown) => answer(ai, jar, &shown).await,
        Err(OwnerToolError::NeedsFreshFactor) => {
            let Some(csrf) = csrf_token(jar) else {
                return failed("csrf token");
            };
            match owner_step_up_page(&csrf, StepUpScreen::Ask) {
                Ok(html) => (StatusCode::UNAUTHORIZED, Html(html)).into_response(),
                Err(_) => failed("step-up render"),
            }
        }
        Err(_) => failed("owner ai change"),
    }
}

/// `POST` [`OWNER_AI_TERMS_ROUTE`]: records the provider's terms (AI-008, SEC-006, SEC-043).
async fn terms(
    State(ai): State<Ai>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    CsrfForm(body): CsrfForm<TermsBody>,
) -> Response {
    let form = TermsForm {
        url: body.url,
        checked_on: body.checked_on,
        training: body.training,
        retention_days: body.retention_days,
        flagged_days: body.flagged_days,
        spend_limit: body.spend_limit,
        expected_cost: body.expected_cost,
        zero_retention_on: body.zero_retention_on,
    };
    change(&ai, &jar, &site, AiChange::Terms(form)).await
}

/// `POST` [`OWNER_AI_POLICY_ROUTE`]: records a privacy-policy version (AI-008, AI-018, SEC-043).
async fn policy(
    State(ai): State<Ai>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    CsrfForm(body): CsrfForm<PolicyBody>,
) -> Response {
    let form = PolicyForm {
        version: body.version,
        effective_on: body.effective_on,
        ai018: body.ai018,
    };
    change(&ai, &jar, &site, AiChange::Policy(form)).await
}

/// `POST` [`OWNER_AI_SWITCH_ROUTE`]: `to=on` turns the switch on (refused naming the first missing
/// record), anything else turns it off, which always works (AI-008, D-418, SEC-043).
async fn switch(
    State(ai): State<Ai>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    CsrfForm(body): CsrfForm<SwitchBody>,
) -> Response {
    change(&ai, &jar, &site, AiChange::Switch(body.to == "on")).await
}
