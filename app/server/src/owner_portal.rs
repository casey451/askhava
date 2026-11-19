//! The served owner home and settings pages and their switches (SEC-006, SEC-034, SEC-042,
//! SEC-043, SEC-048, OPS-001, OPS-003; T310, D-2128, D-2440, D-3155, D-3753, D-3890).
//!
//! [`routes`] serves `portals`' owner tools (T108, T484) over the pool: `GET` [`OWNER_HOME_ROUTE`]
//! and [`OWNER_SETTINGS_ROUTE`]; the change `POST`s [`OWNER_PAUSE_ROUTE`],
//! [`OWNER_FEE_MODE_ROUTE`], [`OWNER_CATEGORY_ROUTE`], [`OWNER_TEXTING_ROUTE`],
//! [`OWNER_BANNER_ROUTE`] and [`OWNER_ALERT_ACK_ROUTE`] (each read by `parse_change` for its own
//! route and made by `apply_owner_change`); the bridge press [`OWNER_BRIDGE_ROUTE`] (it only
//! queues `calls.owner_bridge`); the owner-recorded opt-out [`OWNER_OPT_OUT_ROUTE`] (through the
//! vault), the Stripe-answer record [`OWNER_STRIPE_ANSWER_ROUTE`] and the safety voice call
//! switch [`OWNER_SAFETY_CALL_ROUTE`] (T407, CALL-019, CALL-022: `portals::owner_safety_call`
//! with `calls::api::SAFETY_CONDITIONS`, its fresh factor `SafetyCallSwitch`; a refusal naming
//! what is missing is a 400 with the settings page). Every path is fixed under
//! `/owner/`, record IDs travel in the form body (D-3890). Every response is `no-store`, a
//! cross-site refusal too (the `no_store` layer is outermost); every
//! `POST` passes `refuse_cross_site` and then the CSRF form check before the owner-session check
//! (`owner_signin::require_owner`), so a cross-site or wrong-token `POST` is a 403 with one
//! `csrf_rejected` event that changes nothing. No session, another realm's or an ended one gets
//! `owner_signed_out`, the 401 linking to `/owner/sign-in`. `portals` asks for the SEC-043 fresh
//! factor (fee mode, settings, the bridge press, the opt-out, the Stripe answer; a pause or an
//! alert acknowledgement needs none) and logs a stale one as an `authz_denied` event; here that
//! refusal is a 401 showing the step-up form, which links to no sign-in page (D-3878); a 403 is
//! kept for the cross-site and CSRF refusals, as the door probe reads it (`door_matrix.rs:480`). A change
//! the rules refuse is a 400 with its page again; a made change is a 303 back to its page, the
//! opt-out and Stripe answer show the settings page with `portals`' confirmation line. T540
//! (PICK-022, PICK-023, D-3890, D-3892): `GET` and `POST` [`OWNER_RELEASE_ROUTE`] serve the
//! release button, which passes `owner_power` with `OwnerPower::Release` (no fresh factor,
//! SEC-043) and only re-sends a standing release (`picks::api::resend_release`); a refusal is a
//! 400 with the page and its error summary, a re-send a 200 with its line. Each change
//! writes its audit row in the owning crate (SEC-006, OPS-003). The BIZ-027 review target is the
//! sign-up settings' `ASKHAVA_BIZ_REVIEW_TARGET_DAYS` read from the environment at start, else 4.

use std::sync::Arc;

use askhava_businesses::api::SignupSettings;
use askhava_core::api::{Clock, SystemClock};
use askhava_portals::api::{
    BridgeForm, ChangeForm, CookieJar, CsrfField, CsrfForm, OWNER_ALERT_ACK_ROUTE,
    OWNER_BANNER_ROUTE, OWNER_BRIDGE_ROUTE, OWNER_CATEGORY_ROUTE, OWNER_FEE_MODE_ROUTE,
    OWNER_HOME_ROUTE, OWNER_OPT_OUT_ROUTE, OWNER_PAUSE_ROUTE, OWNER_RELEASE_ROUTE,
    OWNER_SAFETY_CALL_ROUTE, OWNER_SETTINGS_ROUTE, OWNER_STRIPE_ANSWER_ROUTE, OWNER_TEXTING_ROUTE,
    OptOutForm, OwnerPrincipal, OwnerToolError, ReleaseForm, ReleaseScreen, SafetyCallForm,
    SettingsNotice, StepUpScreen, StripeAnswerForm, WebSecurity, apply_owner_change, csrf_token,
    no_store, owner_bridge_press, owner_file_stripe_answer, owner_home_page, owner_record_opt_out,
    owner_release_page, owner_release_resend, owner_safety_call, owner_settings_page_with,
    owner_signed_out, owner_step_up_page, parse_change, refuse_cross_site,
};
use askhava_privacy::api::Vault;
use axum::Router;
use axum::extract::{Extension, FromRef, MatchedPath, State};
use axum::http::StatusCode;
use axum::http::header::{HeaderValue, LOCATION};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use jiff::Timestamp;
use sqlx::PgPool;

use crate::inbound_text::Webhooks;
use crate::owner_signin::{OwnerSite, note_owner_site, require_owner};

/// Where the opt-out's vault comes from: `main`'s own, else the Twilio webhooks', else none (the
/// opt-out then answers 500 and lists nothing).
#[derive(Clone)]
enum OwnerVault {
    None,
    Own(Arc<Vault>),
    Hooks(Arc<Webhooks>),
}

impl OwnerVault {
    fn get(&self) -> Option<&Vault> {
        match self {
            Self::None => None,
            Self::Own(vault) => Some(vault),
            Self::Hooks(hooks) => Some(&hooks.vault),
        }
    }
}

/// The owner tools' state.
#[derive(Clone)]
struct Tools {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    vault: OwnerVault,
    review_days: u32,
}

impl FromRef<Tools> for WebSecurity {
    fn from_ref(tools: &Tools) -> Self {
        tools.security.clone()
    }
}

/// A change form with its CSRF token; every field may be missing, so a bare CSRF post reaches the
/// CSRF and owner checks (D-3878).
#[derive(serde::Deserialize)]
struct ChangeBody {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    change: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    to: String,
}

/// The bridge press with its CSRF token.
#[derive(serde::Deserialize)]
struct BridgeBody {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    alert: String,
    #[serde(default)]
    press: String,
}

/// The opt-out form with its CSRF token. No `Debug`: it holds a phone number (PRIV-016).
#[derive(serde::Deserialize)]
struct OptOutBody {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    phone: String,
}

/// The Stripe-answer form with its CSRF token.
#[derive(serde::Deserialize)]
struct StripeBody {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    answered_on: String,
    #[serde(default)]
    reference: String,
    #[serde(default)]
    allows: String,
}

impl CsrfField for ChangeBody {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

impl CsrfField for BridgeBody {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

impl CsrfField for OptOutBody {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

impl CsrfField for StripeBody {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// The safety voice call form with its CSRF token (CALL-019).
#[derive(serde::Deserialize)]
struct SafetyBody {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    to: String,
    #[serde(default)]
    approved_on: String,
    #[serde(default)]
    reference: String,
}

impl CsrfField for SafetyBody {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// The release press with its CSRF token (D-3892).
#[derive(serde::Deserialize)]
struct ReleaseBody {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    job: String,
    #[serde(default)]
    press: String,
}

impl CsrfField for ReleaseBody {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// Which owner page a refusal shows again.
#[derive(Clone, Copy)]
enum Page {
    Home,
    Settings,
}

/// The routes `build_router` merges: `main`'s own `vault`, else the Twilio webhooks' vault, for
/// the opt-out (CON-035); the review target from the environment's sign-up settings (BIZ-027).
pub fn routes(pool: PgPool, vault: Option<Arc<Vault>>, webhooks: Option<&Arc<Webhooks>>) -> Router {
    let vault = match (vault, webhooks) {
        (Some(vault), _) => OwnerVault::Own(vault),
        (None, Some(hooks)) => OwnerVault::Hooks(Arc::clone(hooks)),
        (None, None) => OwnerVault::None,
    };
    let review_days = SignupSettings::from_pairs(std::env::vars())
        .unwrap_or_default()
        .review_target_days;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Tools {
        pool,
        clock,
        security: security.clone(),
        vault,
        review_days,
    };
    Router::new()
        .route(OWNER_HOME_ROUTE, get(home))
        .route(OWNER_SETTINGS_ROUTE, get(settings))
        .route(OWNER_PAUSE_ROUTE, post(change))
        .route(OWNER_FEE_MODE_ROUTE, post(change))
        .route(OWNER_CATEGORY_ROUTE, post(change))
        .route(OWNER_TEXTING_ROUTE, post(change))
        .route(OWNER_BANNER_ROUTE, post(change))
        .route(OWNER_ALERT_ACK_ROUTE, post(change))
        .route(OWNER_BRIDGE_ROUTE, post(bridge))
        .route(OWNER_OPT_OUT_ROUTE, post(opt_out))
        .route(OWNER_STRIPE_ANSWER_ROUTE, post(stripe_answer))
        .route(OWNER_SAFETY_CALL_ROUTE, post(safety_call))
        .route(OWNER_RELEASE_ROUTE, get(release_page).post(release_press))
        .route_layer(from_fn(note_owner_site))
        .with_state(state)
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store))
}

/// A failed read or render: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "owner tools failed");
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

/// The live owner at `now`, else the owner door's answer (SEC-005, SEC-006, SEC-048).
async fn owner(
    tools: &Tools,
    jar: &CookieJar,
    site: &OwnerSite,
) -> Result<(OwnerPrincipal, Timestamp), Box<Response>> {
    let now = tools.clock.now();
    let principal = require_owner(&tools.pool, jar, site, now).await?;
    Ok((principal, now))
}

/// `page` for the owner's `jar` at `now`, with `notice` on the settings page, answering `status`
/// (OPS-003, OPS-035, CON-024, BIZ-027, LIC-002; reads only).
async fn page_answer(
    tools: &Tools,
    jar: &CookieJar,
    (page, notice): (Page, Option<SettingsNotice>),
    status: StatusCode,
) -> Response {
    let Some(csrf) = csrf_token(jar) else {
        return failed("csrf token");
    };
    let Ok(mut db) = tools.pool.acquire().await else {
        return failed("database");
    };
    let now = tools.clock.now();
    let built = match page {
        Page::Home => owner_home_page(&mut db, &csrf, tools.review_days, now).await,
        Page::Settings => owner_settings_page_with(&mut db, &csrf, notice).await,
    };
    match built {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => failed("owner page"),
    }
}

/// The answer to a change `portals` did not make (SEC-043, D-3878): a stale factor is a 401 with
/// the step-up form (its `authz_denied` event already recorded by `portals`), a refusal a 400 with
/// `page` again, a database failure a 500. None links to the sign-in page.
async fn not_made(tools: &Tools, jar: &CookieJar, error: &OwnerToolError, page: Page) -> Response {
    match error {
        OwnerToolError::NeedsFreshFactor => {
            let Some(csrf) = csrf_token(jar) else {
                return failed("csrf token");
            };
            match owner_step_up_page(&csrf, StepUpScreen::Ask) {
                Ok(html) => (StatusCode::UNAUTHORIZED, Html(html)).into_response(),
                Err(_) => failed("step-up render"),
            }
        }
        OwnerToolError::Refused => {
            page_answer(tools, jar, (page, None), StatusCode::BAD_REQUEST).await
        }
        OwnerToolError::Database => failed("owner change"),
    }
}

/// `GET` [`OWNER_HOME_ROUTE`]: the owner home, for the owner only (SEC-006, SEC-034, OPS-001,
/// OPS-003, BIZ-027).
async fn home(
    State(tools): State<Tools>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
) -> Response {
    if let Err(refused) = owner(&tools, &jar, &site).await {
        return *refused;
    }
    page_answer(&tools, &jar, (Page::Home, None), StatusCode::OK).await
}

/// `GET` [`OWNER_SETTINGS_ROUTE`]: the category switches, the opt-out and Stripe-answer forms, for
/// the owner only (SEC-006, SEC-034, LIC-002).
async fn settings(
    State(tools): State<Tools>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
) -> Response {
    if let Err(refused) = owner(&tools, &jar, &site).await {
        return *refused;
    }
    page_answer(&tools, &jar, (Page::Settings, None), StatusCode::OK).await
}

/// `POST` of one change form to its own route (OPS-001, OPS-003, FEE-007, LIC-002, SEC-043,
/// SEC-048): `parse_change` reads it for the matched route (a form for another route is a 400),
/// `apply_owner_change` makes it after any fresh factor it needs; then a 303 back to its page.
async fn change(
    State(tools): State<Tools>,
    Extension(site): Extension<OwnerSite>,
    route: MatchedPath,
    jar: CookieJar,
    CsrfForm(body): CsrfForm<ChangeBody>,
) -> Response {
    let (principal, now) = match owner(&tools, &jar, &site).await {
        Ok(owner) => owner,
        Err(refused) => return *refused,
    };
    let (page, back) = if route.as_str() == OWNER_CATEGORY_ROUTE {
        (Page::Settings, OWNER_SETTINGS_ROUTE)
    } else {
        (Page::Home, OWNER_HOME_ROUTE)
    };
    let form = ChangeForm {
        change: body.change,
        target: body.target,
        to: body.to,
    };
    let Some(parsed) = parse_change(route.as_str(), &form) else {
        return not_made(&tools, &jar, &OwnerToolError::Refused, page).await;
    };
    let Ok(mut db) = tools.pool.acquire().await else {
        return failed("database");
    };
    match apply_owner_change(&mut db, &principal, parsed, now).await {
        Ok(_) => see_other(back),
        Err(error) => not_made(&tools, &jar, &error, page).await,
    }
}

/// `POST` [`OWNER_BRIDGE_ROUTE`] (OPS-028, SEC-043, D-3155, D-3753): after the fresh factor,
/// queues one `calls.owner_bridge` task for the alert and press; nothing is dialled here. Then a
/// 303 to the home page.
async fn bridge(
    State(tools): State<Tools>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    CsrfForm(body): CsrfForm<BridgeBody>,
) -> Response {
    let (principal, now) = match owner(&tools, &jar, &site).await {
        Ok(owner) => owner,
        Err(refused) => return *refused,
    };
    let Ok(mut db) = tools.pool.acquire().await else {
        return failed("database");
    };
    let form = BridgeForm {
        alert: body.alert,
        press: body.press,
    };
    match owner_bridge_press(&mut db, &principal, &form, now).await {
        Ok(_) => see_other(OWNER_HOME_ROUTE),
        Err(error) => not_made(&tools, &jar, &error, Page::Home).await,
    }
}

/// `POST` [`OWNER_OPT_OUT_ROUTE`] (CON-035, SEC-043, D-3753): after the fresh factor, lists the
/// number through the vault and shows the settings page with the confirmation; no vault is a 500
/// that lists nothing. The number is never logged.
async fn opt_out(
    State(tools): State<Tools>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    CsrfForm(body): CsrfForm<OptOutBody>,
) -> Response {
    let (principal, now) = match owner(&tools, &jar, &site).await {
        Ok(owner) => owner,
        Err(refused) => return *refused,
    };
    let Some(vault) = tools.vault.get() else {
        return failed("vault");
    };
    let Ok(mut db) = tools.pool.acquire().await else {
        return failed("database");
    };
    let form = OptOutForm { phone: body.phone };
    match owner_record_opt_out(&mut db, vault, &principal, &form, now).await {
        Ok(notice) => {
            let shown = (Page::Settings, Some(notice));
            page_answer(&tools, &jar, shown, StatusCode::OK).await
        }
        Err(error) => not_made(&tools, &jar, &error, Page::Settings).await,
    }
}

/// `POST` [`OWNER_STRIPE_ANSWER_ROUTE`] (PICK-048, SEC-043, D-3753): after the fresh factor, files
/// Stripe's written answer and shows the settings page with it.
async fn stripe_answer(
    State(tools): State<Tools>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    CsrfForm(body): CsrfForm<StripeBody>,
) -> Response {
    let (principal, now) = match owner(&tools, &jar, &site).await {
        Ok(owner) => owner,
        Err(refused) => return *refused,
    };
    let Ok(mut db) = tools.pool.acquire().await else {
        return failed("database");
    };
    let form = StripeAnswerForm {
        answered_on: body.answered_on,
        reference: body.reference,
        allows: body.allows,
    };
    match owner_file_stripe_answer(&mut db, &principal, &form, now).await {
        Ok(notice) => {
            let shown = (Page::Settings, Some(notice));
            page_answer(&tools, &jar, shown, StatusCode::OK).await
        }
        Err(error) => not_made(&tools, &jar, &error, Page::Settings).await,
    }
}

/// `POST` [`OWNER_SAFETY_CALL_ROUTE`] (CALL-019, CALL-022, SEC-043, D-3134): after the fresh
/// factor, turns the safety voice call on or off with `calls::api::SAFETY_CONDITIONS` and shows
/// the settings page with the outcome: 200 when made, 400 naming what is missing when refused.
async fn safety_call(
    State(tools): State<Tools>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    CsrfForm(body): CsrfForm<SafetyBody>,
) -> Response {
    let (principal, now) = match owner(&tools, &jar, &site).await {
        Ok(owner) => owner,
        Err(refused) => return *refused,
    };
    let Ok(mut db) = tools.pool.acquire().await else {
        return failed("database");
    };
    let form = SafetyCallForm {
        to: body.to,
        approved_on: body.approved_on,
        reference: body.reference,
    };
    let met = &askhava_calls::api::SAFETY_CONDITIONS;
    match owner_safety_call(&mut db, &principal, (&form, met), now).await {
        Ok(notice) => {
            let status = if notice.refused() {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::OK
            };
            page_answer(&tools, &jar, (Page::Settings, Some(notice)), status).await
        }
        Err(error) => not_made(&tools, &jar, &error, Page::Settings).await,
    }
}

/// The release page for `screen` with `typed` kept, answering 400 for a refusal, else 200.
fn release_answer(jar: &CookieJar, screen: ReleaseScreen, typed: &str) -> Response {
    let Some(csrf) = csrf_token(jar) else {
        return failed("csrf token");
    };
    let status = if screen.refused() {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };
    match owner_release_page(&csrf, screen, typed) {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => failed("release page"),
    }
}

/// `GET` [`OWNER_RELEASE_ROUTE`]: the release button's form with a fresh press ID, for the owner
/// only (SEC-006, PICK-023; reads nothing else).
async fn release_page(
    State(tools): State<Tools>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
) -> Response {
    if let Err(refused) = owner(&tools, &jar, &site).await {
        return *refused;
    }
    release_answer(&jar, ReleaseScreen::Ask, "")
}

/// `POST` [`OWNER_RELEASE_ROUTE`] (PICK-022, PICK-023, SEC-006, D-074, D-3892): re-sends the typed
/// job's standing release through `portals`' `owner_release_resend`; it never makes a release.
async fn release_press(
    State(tools): State<Tools>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    CsrfForm(body): CsrfForm<ReleaseBody>,
) -> Response {
    let now = match owner(&tools, &jar, &site).await {
        Ok((_, now)) => now,
        Err(refused) => return *refused,
    };
    let Ok(mut db) = tools.pool.acquire().await else {
        return failed("database");
    };
    let form = ReleaseForm {
        job: body.job,
        press: body.press,
    };
    match owner_release_resend(&mut db, &jar, &form, now).await {
        Ok(screen) => release_answer(&jar, screen, &form.job),
        Err(OwnerToolError::Database) => failed("owner release"),
        Err(OwnerToolError::Refused | OwnerToolError::NeedsFreshFactor) => owner_signed_out(),
    }
}
