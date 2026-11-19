//! The served business step-up code page (SEC-004, SEC-014, SEC-020, SEC-038, SEC-042; T364,
//! D-2636, D-3744, D-4045).
//!
//! [`routes`] serves `GET` [`BUSINESS_STEP_UP_ROUTE`] (the page, changing nothing), `POST`
//! [`BUSINESS_STEP_UP_SEND_ROUTE`] (send a code) and `POST` [`BUSINESS_STEP_UP_CHECK_ROUTE`]
//! (check it) over `portals`' `send_step_up_code` and `check_step_up_code`, which keep the step-up
//! record's rules. Every route reads `business_principal` first (no business person:
//! `signed_out`), every answer is `no-store`, both `POST`s pass `refuse_cross_site` and the CSRF
//! form check, and each form's `return` is kept only as a same-site path (`ReturnPath`).
//! SEC-014: a send first passes `admit_code_send` for the client IP and the member, a check
//! `admit_code_try` for the client IP and the business; a refusal sends and checks nothing.
//! `build_router` always merges the routes; the Verify part comes from
//! `RouterParts::with_step_up`, which `main` sets from [`step_up_codes`]; without it a send
//! mints and sends nothing and the page says codes can't be sent (below 500).
//! [`MemberStepUpAddress`] sends the code to the member's own first texting line, else the
//! member's own sign-in address, opened under `BusinessPurpose::SignInCode`.

use std::io;
use std::net::IpAddr;
use std::sync::Arc;

use askhava_adapters::api::{TwilioConfig, twilio_clients};
use askhava_businesses::api::sign_in_lines;
use askhava_core::api::{Clock, Environment, Settings, SystemClock};
use askhava_intake::api::Admission;
use askhava_portals::api::{
    BUSINESS_STEP_UP_CHECK_ROUTE, BUSINESS_STEP_UP_ROUTE, BUSINESS_STEP_UP_SEND_ROUTE,
    BusinessPrincipal, BusinessStepUpScreen, CodeAddress, CodeFuture, CodeTo, CookieJar, CsrfField,
    CsrfForm, PortalsError, ReturnPath, StepUpCodes, VERIFY_DAILY_THRESHOLD, WebSecurity,
    admit_code_send, admit_code_try, business_principal, business_step_up_page, check_step_up_code,
    csrf_token, no_store, refuse_cross_site, security_event_client, send_step_up_code, signed_out,
};
use askhava_privacy::api::{BusinessPurpose, Vault, read_business_mailbox, read_staff_phone};
use axum::Router;
use axum::extract::{Extension, FromRef, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The step-up routes' state.
#[derive(Clone)]
struct StepUp {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    codes: Option<Arc<StepUpCodes>>,
}

impl FromRef<StepUp> for WebSecurity {
    fn from_ref(state: &StepUp) -> Self {
        state.security.clone()
    }
}

/// The request's client address, for the SEC-014 limits; never stored (SEC-040).
#[derive(Clone, Copy)]
struct ClientIp(IpAddr);

/// The three step-up routes over `pool`, sending codes through `codes` when given (SEC-038).
pub fn routes(pool: PgPool, codes: Option<StepUpCodes>) -> Router {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = StepUp {
        pool,
        clock,
        security: security.clone(),
        codes: codes.map(Arc::new),
    };
    Router::new()
        .route(BUSINESS_STEP_UP_ROUTE, get(page))
        .route(BUSINESS_STEP_UP_SEND_ROUTE, post(send))
        .route(BUSINESS_STEP_UP_CHECK_ROUTE, post(check))
        .route_layer(from_fn(note_client))
        .with_state(state)
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// The Verify part `main` passes to `RouterParts::with_step_up`: `ASKHAVA_ID_SECRET`, the Twilio
/// Verify client and the vault from `pairs`; `None` in dev and CI without Twilio settings
/// (SEC-004, D-3216).
///
/// # Errors
///
/// A missing `ASKHAVA_ID_SECRET`, bad Twilio settings in the test or live project, or a bad vault
/// key pair. Never a value.
pub fn step_up_codes<I>(settings: &Settings, pairs: I) -> io::Result<Option<StepUpCodes>>
where
    I: IntoIterator<Item = (String, String)>,
{
    let pairs: Vec<(String, String)> = pairs.into_iter().collect();
    let secret = pairs
        .iter()
        .find(|(k, v)| k == "ASKHAVA_ID_SECRET" && !v.trim().is_empty())
        .map(|(_, v)| SecretString::from(v.trim().to_owned()))
        .ok_or_else(|| io::Error::other("missing ASKHAVA_ID_SECRET"))?;
    let vault = crate::vault_from(settings, pairs.clone())?.map(Arc::new);
    let to = MemberStepUpAddress { vault };
    match TwilioConfig::from_settings(settings, pairs) {
        Ok(config) => {
            let (_, _, verify) = twilio_clients(&config).map_err(io::Error::other)?;
            Ok(Some(StepUpCodes::new(verify, to, secret)))
        }
        Err(error) => match settings.environment() {
            Environment::Dev | Environment::Ci => Ok(None),
            Environment::Test | Environment::Live => Err(io::Error::other(error)),
        },
    }
}

/// Notes the client address (SEC-040) for the limits.
async fn note_client(mut request: Request, next: Next) -> Response {
    let ip = security_event_client(&request);
    request.extensions_mut().insert(ClientIp(ip));
    next.run(request).await
}

/// A failed read or render: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "business step-up page failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// SEC-004: the signed-in business person, else the signed-out answer.
async fn signed_in(state: &StepUp, jar: &CookieJar) -> Result<BusinessPrincipal, Box<Response>> {
    let Ok(mut db) = state.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    match business_principal(&mut db, jar, state.clock.now()).await {
        Ok(Some(who)) => Ok(who),
        Ok(None) => Err(Box::new(signed_out())),
        Err(_) => Err(Box::new(failed("business principal"))),
    }
}

/// The page for `screen` with the jar's CSRF token; a limit refusal is a 429.
fn shown(jar: &CookieJar, screen: BusinessStepUpScreen, back: &ReturnPath) -> Response {
    let Some(csrf) = csrf_token(jar) else {
        return failed("csrf token");
    };
    let status = if screen == BusinessStepUpScreen::Refused {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        StatusCode::OK
    };
    match business_step_up_page(&csrf, screen, back) {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => failed("step-up page"),
    }
}

/// The page's query: where to go back to; any other field is a 400 (SEC-010).
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BackQuery {
    #[serde(rename = "return")]
    back: Option<String>,
}

/// `GET` [`BUSINESS_STEP_UP_ROUTE`] (SEC-038): the "Send me a code" page. Changes nothing.
async fn page(State(state): State<StepUp>, jar: CookieJar, Query(q): Query<BackQuery>) -> Response {
    if let Err(answer) = signed_in(&state, &jar).await {
        return *answer;
    }
    shown(
        &jar,
        BusinessStepUpScreen::Ask,
        &ReturnPath::parse(q.back.as_deref()),
    )
}

/// The step-up forms: the CSRF token, the path to go back to and, on a check, the code. Any other
/// field name fails the parse, a 422 that does nothing (SEC-010).
#[derive(Debug, serde::Deserialize)]
#[serde(try_from = "Vec<(String, String)>")]
struct StepUpForm {
    csrf: String,
    back: Option<String>,
    code: String,
}

impl TryFrom<Vec<(String, String)>> for StepUpForm {
    type Error = String;

    /// Splits the posted pairs; refuses an unknown field name (SEC-010).
    fn try_from(pairs: Vec<(String, String)>) -> Result<Self, String> {
        let mut form = Self {
            csrf: String::new(),
            back: None,
            code: String::new(),
        };
        for (name, value) in pairs {
            match name.as_str() {
                "csrf" => form.csrf = value,
                "return" => form.back = Some(value),
                "code" => form.code = value,
                _ => return Err(format!("unknown field `{name}`")),
            }
        }
        Ok(form)
    }
}

impl CsrfField for StepUpForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// Whether `admitted` let the request through; an error refuses (D-1951).
fn passed<E>(admitted: Result<Admission, E>) -> bool {
    matches!(admitted, Ok(Admission::Admitted))
}

/// `POST` [`BUSINESS_STEP_UP_SEND_ROUTE`] (SEC-004, SEC-014, SEC-038): after the per-IP and
/// per-member send limits, one code for the signed-in member, reusing a live record.
async fn send(
    State(state): State<StepUp>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<StepUpForm>,
) -> Response {
    let who = match signed_in(&state, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let back = ReturnPath::parse(form.back.as_deref());
    let now = state.clock.now();
    let threshold = i32::try_from(VERIFY_DAILY_THRESHOLD).unwrap_or(i32::MAX);
    let limit = admit_code_send(&state.pool, (ip, Some(who.member())), (now, threshold)).await;
    if !passed(limit) {
        return shown(&jar, BusinessStepUpScreen::Refused, &back);
    }
    match send_step_up_code(&state.pool, state.codes.as_deref(), &who, now).await {
        Ok(screen) => shown(&jar, screen, &back),
        Err(_) => failed("step-up send"),
    }
}

/// `POST` [`BUSINESS_STEP_UP_CHECK_ROUTE`] (SEC-014, SEC-020, SEC-038): after the per-IP and
/// per-business try limit, checks the code; a right one records the step-up.
async fn check(
    State(state): State<StepUp>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<StepUpForm>,
) -> Response {
    let who = match signed_in(&state, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let back = ReturnPath::parse(form.back.as_deref());
    let now = state.clock.now();
    if !passed(admit_code_try(&state.pool, (ip, Some(who.business())), now).await) {
        return shown(&jar, BusinessStepUpScreen::Refused, &back);
    }
    let codes = state.codes.as_deref();
    match check_step_up_code(&state.pool, codes, &who, &form.code, now).await {
        Ok(screen) => shown(&jar, screen, &back),
        Err(_) => failed("step-up check"),
    }
}

/// The composition's step-up `CodeAddress`: the member's own first texting line, else own sign-in
/// address, opened under `SignInCode`; no vault or a removed member: no code (SEC-004, D-3562).
pub struct MemberStepUpAddress {
    vault: Option<Arc<Vault>>,
}

impl MemberStepUpAddress {
    /// SEC-004: where `member`'s step-up code goes.
    async fn find(
        &self,
        db: &mut sqlx::PgConnection,
        member: Uuid,
    ) -> Result<CodeTo, PortalsError> {
        let lines = sign_in_lines(db, member).await;
        let lines = lines.map_err(|_| PortalsError::Database)?;
        let lines = lines.ok_or(PortalsError::Database)?;
        let vault = self.vault.as_deref().ok_or(PortalsError::Database)?;
        let purpose = BusinessPurpose::SignInCode;
        let to = match lines.texting_lines.first() {
            Some(line) => read_staff_phone(db, vault, purpose, *line)
                .await
                .map(CodeTo::Phone),
            None => read_business_mailbox(db, vault, purpose, lines.mailbox)
                .await
                .map(CodeTo::Email),
        };
        to.map_err(|_| PortalsError::Database)
    }
}

impl CodeAddress for MemberStepUpAddress {
    fn code_to<'a>(
        &'a self,
        db: &'a mut sqlx::PgConnection,
        member: Uuid,
    ) -> CodeFuture<'a, Result<CodeTo, PortalsError>> {
        Box::pin(self.find(db, member))
    }
}
