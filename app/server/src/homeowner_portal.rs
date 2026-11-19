//! The served homeowner portal: sign-in by phone and texted code, her home page and the texts-off
//! switch (SEC-008, SEC-014, SEC-034, SEC-042, SEC-046, SEC-048, PRIV-021, CON-035; T307, D-2128,
//! D-3877, D-3967, D-4025).
//!
//! [`routes`] serves `portals`' homeowner portal (T087): `GET` [`HOMEOWNER_SIGNIN_ROUTE`] (the
//! phone form, with a pre-session cookie for its CSRF token), `POST` [`HOMEOWNER_SIGNIN_ROUTE`]
//! (send a code), `POST` [`HOMEOWNER_CODE_ROUTE`] (check it and start her 30-day session), `GET`
//! [`HOMEOWNER_HOME_ROUTE`] (her own record, `HomeFacts::read` with her session's proof) and `POST`
//! [`TEXTS_OFF_ROUTE`] (one suppression entry at once, confirmed on screen). Every answer is
//! `no-store`; every `POST` passes `refuse_cross_site`, then the CSRF form check (both refusals a
//! 403 and a `csrf_rejected` security event), then the realm, then its fields. No session, another
//! realm's or an ended one gets [`signed_out`], a 401 linking to `/h/sign-in` with no detail.
//! SEC-014: a send passes `admit_code_send` (the IP, then her contact when on file, so an unknown
//! number spends the IP limit and sends nothing) and a check `admit_code_try` first; a refusal
//! answers as always and sends or checks nothing. A number not on file gets the same code page.
//! The vault and ID secret are the Twilio webhooks'; the Verify client is the optional
//! [`HomeownerVerify`] part (`RouterParts::with_homeowner_verify`, from [`homeowner_verify`] in
//! `main`); without either a send sends nothing and a check calls nothing and answers the
//! wrong-code page, never a 5xx. `server` names no vault read: her number never leaves `portals`
//! except into the rendered page. The `/h/my-data` routes are T561's.

use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;

use askhava_adapters::api::{
    AdapterError, CodeChannel, IdempotencyKey, TwilioConfig, VerifyAdapter, twilio_clients,
};
use askhava_core::api::{Clock, Environment, Settings, SystemClock};
use askhava_intake::api::{Admission, conversation, conversations_of_contact};
use askhava_portals::api::{
    ButtonForm, CookieJar, CsrfField, CsrfForm, HOMEOWNER_CODE_ROUTE, HOMEOWNER_HOME_ROUTE,
    HOMEOWNER_SIGNIN_ROUTE, HomeFacts, HomeownerError, OnFile, PortalScreen, TEXTS_OFF_ROUTE,
    VERIFY_DAILY_THRESHOLD, WebSecurity, admit_code_send, admit_code_try, csrf_token,
    current_session, ensure_pre_session, finish_sign_in, homeowner_page, no_store,
    refuse_cross_site, security_event_client, send_sign_in_code, turn_texts_off,
};
use askhava_privacy::api::{HomeownerSessionProof, find_by_phone};
use axum::Router;
use axum::extract::{Extension, FromRef, Request, State};
use axum::http::StatusCode;
use axum::http::header::{HeaderValue, LOCATION};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::inbound_text::Webhooks;

/// A boxed Verify answer, so the part can be any [`VerifyAdapter`].
type Answer<'a, T> = Pin<Box<dyn Future<Output = Result<T, AdapterError>> + Send + 'a>>;

/// [`VerifyAdapter`] behind a box (its methods return `impl Future`, so it has no `dyn` form).
trait BoxedVerify: Send + Sync {
    fn send<'a>(
        &'a self,
        key: &'a IdempotencyKey,
        to: &'a str,
        by: CodeChannel,
    ) -> Answer<'a, String>;
    fn check<'a>(&'a self, to: &'a str, code: &'a str) -> Answer<'a, bool>;
}

impl<V: VerifyAdapter + Send + Sync> BoxedVerify for V {
    fn send<'a>(
        &'a self,
        key: &'a IdempotencyKey,
        to: &'a str,
        by: CodeChannel,
    ) -> Answer<'a, String> {
        Box::pin(self.send_code(key, to, by))
    }

    fn check<'a>(&'a self, to: &'a str, code: &'a str) -> Answer<'a, bool> {
        Box::pin(self.check_code(to, code))
    }
}

/// The homeowner sign-in's Verify client (SEC-008): Twilio Verify in `main`, a fake in this
/// task's own tests.
pub struct HomeownerVerify(Box<dyn BoxedVerify>);

impl HomeownerVerify {
    /// Codes sent and checked through `verify`.
    #[must_use]
    pub fn new<V: VerifyAdapter + Send + Sync + 'static>(verify: V) -> Self {
        Self(Box::new(verify))
    }
}

impl VerifyAdapter for HomeownerVerify {
    async fn send_code(
        &self,
        idempotency: &IdempotencyKey,
        to: &str,
        channel: CodeChannel,
    ) -> Result<String, AdapterError> {
        self.0.send(idempotency, to, channel).await
    }

    async fn check_code(&self, to: &str, code: &str) -> Result<bool, AdapterError> {
        self.0.check(to, code).await
    }
}

/// The Verify part `main` passes to `RouterParts::with_homeowner_verify`: the Twilio Verify
/// client from `pairs`; `None` in dev and CI without Twilio settings (SEC-008, D-3877).
///
/// # Errors
///
/// Bad Twilio settings in the test or live project. Never a value.
pub fn homeowner_verify<I>(settings: &Settings, pairs: I) -> io::Result<Option<HomeownerVerify>>
where
    I: IntoIterator<Item = (String, String)>,
{
    match TwilioConfig::from_settings(settings, pairs) {
        Ok(config) => {
            let (_, _, verify) = twilio_clients(&config).map_err(io::Error::other)?;
            Ok(Some(HomeownerVerify::new(verify)))
        }
        Err(error) => match settings.environment() {
            Environment::Dev | Environment::Ci => Ok(None),
            Environment::Test | Environment::Live => Err(io::Error::other(error)),
        },
    }
}

/// The portal routes' state.
#[derive(Clone)]
struct Portal {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    hooks: Option<Arc<Webhooks>>,
    verify: Option<Arc<HomeownerVerify>>,
}

impl FromRef<Portal> for WebSecurity {
    fn from_ref(state: &Portal) -> Self {
        state.security.clone()
    }
}

/// The request's client address, for the SEC-014 limits; never stored (SEC-040).
#[derive(Clone, Copy)]
struct ClientIp(IpAddr);

/// The five portal routes over `pool`, with the webhooks' vault and ID secret and `verify`.
pub fn routes(
    pool: PgPool,
    hooks: Option<&Arc<Webhooks>>,
    verify: Option<HomeownerVerify>,
) -> Router {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Portal {
        pool,
        clock,
        security: security.clone(),
        hooks: hooks.cloned(),
        verify: verify.map(Arc::new),
    };
    Router::new()
        .route(HOMEOWNER_SIGNIN_ROUTE, get(sign_in_page).post(send))
        .route(HOMEOWNER_CODE_ROUTE, post(check))
        .route(HOMEOWNER_HOME_ROUTE, get(home))
        .route(TEXTS_OFF_ROUTE, post(texts_off))
        .route_layer(from_fn(note_client))
        .with_state(state)
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store))
}

/// Notes the client address (SEC-040) for the limits.
async fn note_client(mut request: Request, next: Next) -> Response {
    let ip = security_event_client(&request);
    request.extensions_mut().insert(ClientIp(ip));
    next.run(request).await
}

/// A failed read or render: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "homeowner portal page failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// SEC-008, SEC-034: the answer to a request with no homeowner session: a 401 linking to
/// `/h/sign-in`, with no detail.
fn signed_out() -> Response {
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><title>Sign in</title></head><body><main>\
         <h1>Sign in</h1><p><a href=\"{HOMEOWNER_SIGNIN_ROUTE}\">Sign in</a> to see this page.\
         </p></main></body></html>"
    );
    (StatusCode::UNAUTHORIZED, Html(page)).into_response()
}

/// `screen` with the jar's CSRF token, answered with `status`.
fn shown(jar: &CookieJar, screen: &PortalScreen, status: StatusCode) -> Response {
    let Some(csrf) = csrf_token(jar) else {
        return failed("csrf token");
    };
    match homeowner_page(screen, &csrf) {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => failed("homeowner page"),
    }
}

/// The code page for the number as typed; `failed` after a wrong or refused code.
fn code_page(jar: &CookieJar, phone: &str, failed: bool) -> Response {
    let phone = SecretString::from(phone.to_owned());
    let status = if failed {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };
    shown(jar, &PortalScreen::Code { phone, failed }, status)
}

/// `GET` [`HOMEOWNER_SIGNIN_ROUTE`] (SEC-008, SEC-042): the phone form, with a pre-session cookie
/// for its CSRF token. Changes nothing stored.
async fn sign_in_page(jar: CookieJar) -> Response {
    let Ok(jar) = ensure_pre_session(jar) else {
        return failed("pre-session");
    };
    let screen = PortalScreen::SignIn {
        phone: SecretString::from(String::new()),
        failed: false,
    };
    let page = shown(&jar, &screen, StatusCode::OK);
    (jar, page).into_response()
}

/// The sign-in forms: the CSRF token, the typed number and, on the code step, the code. A missing
/// field reads as empty, so the CSRF check is reached first; any other field fails the parse.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SignInForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    phone: String,
    #[serde(default)]
    code: String,
}

impl CsrfField for SignInForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// Whether `admitted` let the request through; an error refuses (D-1951).
fn passed<E>(admitted: Result<Admission, E>) -> bool {
    matches!(admitted, Ok(Admission::Admitted))
}

/// The contact holding `phone`, `None` for none, no vault or a number that is not a US number;
/// a failed lookup is logged once, without the number, and read as none.
async fn on_file(state: &Portal, phone: &str) -> Option<Uuid> {
    let hooks = state.hooks.as_deref()?;
    let mut db = state.pool.acquire().await.ok()?;
    match find_by_phone(&mut db, &hooks.vault, phone).await {
        Ok(contact) => contact,
        Err(error) => {
            tracing::debug!(%error, "homeowner sign-in lookup found no contact");
            None
        }
    }
}

/// `POST` [`HOMEOWNER_SIGNIN_ROUTE`] (SEC-008, SEC-014): after the per-IP and per-number send
/// limits, one code to a number on file; every admitted or refused number gets the same code page.
async fn send(
    State(state): State<Portal>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<SignInForm>,
) -> Response {
    if askhava_privacy::api::normalize_phone(&form.phone).is_err() {
        let phone = SecretString::from(form.phone);
        let screen = PortalScreen::SignIn {
            phone,
            failed: true,
        };
        return shown(&jar, &screen, StatusCode::BAD_REQUEST);
    }
    let now = state.clock.now();
    let contact = on_file(&state, &form.phone).await;
    let threshold = i32::try_from(VERIFY_DAILY_THRESHOLD).unwrap_or(i32::MAX);
    let limit = admit_code_send(&state.pool, (ip, contact), (now, threshold)).await;
    let parts = state.hooks.as_deref().zip(state.verify.as_deref());
    if let (true, Some((hooks, verify))) = (passed(limit), parts) {
        let keys = (&hooks.vault, verify, &hooks.id_secret);
        if let Err(error) = send_sign_in_code(&state.pool, keys, &form.phone, now).await {
            tracing::warn!(%error, "homeowner sign-in code not sent");
        }
    }
    code_page(&jar, &form.phone, false)
}

/// `POST` [`HOMEOWNER_CODE_ROUTE`] (SEC-008, SEC-014, SEC-020): after the per-IP try limit, checks
/// the code; a right one starts her 30-day session and goes to her home (303).
async fn check(
    State(state): State<Portal>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<SignInForm>,
) -> Response {
    let now = state.clock.now();
    let parts = state.hooks.as_deref().zip(state.verify.as_deref());
    let admitted = passed(admit_code_try(&state.pool, (ip, None), now).await);
    let Some((hooks, verify)) = parts.filter(|_| admitted) else {
        return code_page(&jar, &form.phone, true);
    };
    let typed = (jar.clone(), form.phone.as_str(), form.code.as_str());
    match finish_sign_in(&state.pool, (&hooks.vault, verify), typed, now).await {
        Ok(jar) => {
            let to = HeaderValue::from_static(HOMEOWNER_HOME_ROUTE);
            (jar, (StatusCode::SEE_OTHER, [(LOCATION, to)])).into_response()
        }
        Err(HomeownerError::Database) => failed("sign-in check"),
        Err(HomeownerError::Refused | HomeownerError::Verify) => code_page(&jar, &form.phone, true),
    }
}

/// SEC-046: her session's proof, else the signed-out answer.
async fn signed_in(
    state: &Portal,
    jar: &CookieJar,
) -> Result<HomeownerSessionProof, Box<Response>> {
    let Ok(mut db) = state.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    match current_session(&mut db, jar, state.clock.now()).await {
        Ok(session) => session
            .and_then(|s| s.homeowner_proof(crate::session_minter()))
            .ok_or_else(|| Box::new(signed_out())),
        Err(_) => Err(Box::new(failed("session"))),
    }
}

/// CON-026: whether her latest request's texts are in Spanish; `false` with no request.
async fn spanish(db: &mut PgConnection, contact: Uuid) -> Result<bool, &'static str> {
    let ids = conversations_of_contact(db, contact).await;
    let Some(latest) = ids.map_err(|_| "requests")?.pop() else {
        return Ok(false);
    };
    let row = conversation(db, latest).await.map_err(|_| "request")?;
    Ok(row.is_some_and(|r| r.language == "es"))
}

/// PRIV-021: her home page, read through her own proof alone; no number on file is signed out.
async fn home_page(state: &Portal, jar: &CookieJar, proof: &HomeownerSessionProof) -> Response {
    let Some(hooks) = state.hooks.as_deref() else {
        return failed("vault");
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let her = match OnFile::read(&mut db, &hooks.vault, proof).await {
        Ok(Some(her)) => her,
        Ok(None) => return signed_out(),
        Err(_) => return failed("her record"),
    };
    let spanish = match spanish(&mut db, her.contact()).await {
        Ok(spanish) => spanish,
        Err(what) => return failed(what),
    };
    match HomeFacts::read(&mut db, &hooks.vault, proof, spanish).await {
        Ok(Some(facts)) => shown(jar, &PortalScreen::Home(facts), StatusCode::OK),
        Ok(None) => signed_out(),
        Err(_) => failed("home facts"),
    }
}

/// `GET` [`HOMEOWNER_HOME_ROUTE`] (PRIV-021, SEC-034, SEC-046): her own record. Changes nothing.
async fn home(State(state): State<Portal>, jar: CookieJar) -> Response {
    match signed_in(&state, &jar).await {
        Ok(proof) => home_page(&state, &jar, &proof).await,
        Err(answer) => *answer,
    }
}

/// `POST` [`TEXTS_OFF_ROUTE`] (CON-035, PRIV-021): puts her number on the suppression list at once
/// and shows her home with the on-screen confirmation; no text is sent.
async fn texts_off(
    State(state): State<Portal>,
    jar: CookieJar,
    CsrfForm(_): CsrfForm<ButtonForm>,
) -> Response {
    let proof = match signed_in(&state, &jar).await {
        Ok(proof) => proof,
        Err(answer) => return *answer,
    };
    let Some(hooks) = state.hooks.as_deref() else {
        return failed("vault");
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let her = match OnFile::read(&mut db, &hooks.vault, &proof).await {
        Ok(Some(her)) => her,
        Ok(None) => return signed_out(),
        Err(_) => return failed("her record"),
    };
    let now = state.clock.now();
    if turn_texts_off(&mut db, &hooks.vault, &her, now)
        .await
        .is_err()
    {
        return failed("texts off");
    }
    drop(db);
    home_page(&state, &jar, &proof).await
}
