//! The served owner sign-in, step-up and sign-out (SEC-005, SEC-007, SEC-014, SEC-034, SEC-042,
//! SEC-043, SEC-044, SEC-046, SEC-048; T303, D-1951, D-2128, D-2206, D-3844, D-3878).
//!
//! [`routes`] serves `portals`' owner sign-in over `pool`: `GET` and `POST`
//! [`OWNER_SIGNIN_ROUTE`] (the password plus a TOTP code, a recovery code, or a passkey answer),
//! `POST` [`OWNER_PASSKEY_ROUTE`] (a passkey sign-in challenge for `static/passkey.ts`, which
//! `static_files` serves at `/static/portals/passkey.js`), `GET` and `POST` [`OWNER_STEP_UP_ROUTE`]
//! (a fresh TOTP code), `POST` [`OWNER_SIGN_OUT_ROUTE`] and `POST` [`OWNER_SIGN_OUT_ALL_ROUTE`]
//! ("Sign out everywhere"). The passkey relying party is the brand file's domain, origin
//! `https://<domain>` (D-3844), so `build_router` needs no new part. Every response is
//! `no-store`; every `POST` passes `refuse_cross_site` and then the CSRF form check before any
//! owner check, so a cross-site or wrong-token `POST` from anyone is a 403 with one
//! `csrf_rejected` event (D-3878). Every sign-in `POST`, a passkey answer too, admits
//! `admit_owner_ip` first; a passkey answer then goes to `owner_passkey_sign_in` (assertion
//! before password, never the account limit when verified), any other post admits
//! `admit_owner_account` before its argon2; a refusal answers as a wrong password does and checks
//! nothing (SEC-040, D-1951, D-2206, D-3962, D-4294). Each passkey challenge admits
//! `admit_owner_passkey_challenge` (429 when refused), and each step-up try
//! `admit_owner_step_up` before the TOTP check: a refused try checks nothing and ends the session
//! (SEC-020, SEC-043). A wrong password or code says nothing about which was wrong.
//!
//! [`require_owner`] is the owner-session check T304, T305 and T310 use (with [`note_owner_site`]
//! as a route layer): `portals::api::owner_principal` (12 hours at most, 30 minutes idle); no
//! session, a business or homeowner session, or an ended owner session gets
//! `portals::api::owner_signed_out`, a 401 linking only to the sign-in page, and a request that
//! carried a session is also one `authz_denied` event naming its session row (SEC-048).

use std::net::IpAddr;
use std::sync::Arc;

use askhava_core::api::{Clock, SystemClock, brand};
use askhava_intake::api::Admission;
use askhava_portals::api::{
    ButtonForm, CookieJar, CsrfField, CsrfForm, OWNER_PASSKEY_ROUTE, OWNER_SIGN_OUT_ALL_ROUTE,
    OWNER_SIGN_OUT_ROUTE, OWNER_SIGNIN_ROUTE, OWNER_STEP_UP_ROUTE, OwnerAuth, OwnerAuthError,
    OwnerPrincipal, SESSION_COOKIE, SecondFactor, SecurityEvent, SecurityEventKind, StepUpScreen,
    WebSecurity, admit_owner_account, admit_owner_ip, admit_owner_passkey_challenge,
    admit_owner_step_up, admit_security_event, csrf_token, current_session, ensure_pre_session,
    no_store, owner_passkey_sign_in, owner_principal, owner_sign_in, owner_sign_out,
    owner_signed_out, owner_signin_page, owner_step_up, owner_step_up_page, passkey_registered,
    passkey_sign_in_options, record_security_event, refuse_cross_site, security_event_client,
    sign_out_everywhere,
};
use axum::extract::{Extension, FromRef, MatchedPath, Request, State};
use axum::http::StatusCode;
use axum::http::header::{HeaderValue, LOCATION};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use jiff::Timestamp;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The owner routes' state.
#[derive(Clone)]
struct Owner {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    auth: Arc<OwnerAuth>,
}

impl FromRef<Owner> for WebSecurity {
    fn from_ref(owner: &Owner) -> Self {
        owner.security.clone()
    }
}

/// The route template, request ID and client address of an owner request, for its security
/// events and the password limit; the address is never stored (SEC-040, SEC-048).
#[derive(Clone, Debug)]
pub struct OwnerSite {
    route: String,
    request_id: Option<Uuid>,
    client: IpAddr,
}

/// The sign-in form: the password and a code, or the passkey answer to challenge `challenge`
/// (`portals`' `owner_signin.html` and `static/passkey.ts`). Every field may be missing, so a
/// bare CSRF post reaches the CSRF check (D-3878).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SignInForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    challenge: String,
    #[serde(default)]
    passkey: String,
}

impl CsrfField for SignInForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// The step-up form: one TOTP code.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StepUpForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    code: String,
}

impl CsrfField for StepUpForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// The passkey challenge `passkey.ts` reads: the challenge row's ID and webauthn-rs's options.
#[derive(serde::Serialize)]
struct Issued {
    challenge: String,
    options: String,
}

/// The owner routes `build_router` merges, with the relying party from the brand file's domain
/// (D-3844). A domain webauthn-rs refuses serves no owner route, logged once.
pub fn routes(pool: PgPool) -> Router {
    let domain = brand().domain();
    match OwnerAuth::new(domain, &format!("https://{domain}")) {
        Ok(auth) => served(pool, Arc::new(SystemClock), auth),
        Err(error) => {
            tracing::error!(%error, "owner sign-in not served");
            Router::new()
        }
    }
}

/// The routes over `pool`, timed by `clock`, behind `no-store` and the cross-site refusal.
fn served(pool: PgPool, clock: Arc<dyn Clock>, auth: OwnerAuth) -> Router {
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Owner {
        pool,
        clock,
        security: security.clone(),
        auth: Arc::new(auth),
    };
    Router::new()
        .route(OWNER_SIGNIN_ROUTE, get(sign_in_page).post(sign_in))
        .route(OWNER_PASSKEY_ROUTE, post(passkey_options))
        .route(OWNER_STEP_UP_ROUTE, get(step_up_page).post(step_up))
        .route(OWNER_SIGN_OUT_ROUTE, post(sign_out))
        .route(OWNER_SIGN_OUT_ALL_ROUTE, post(sign_out_all))
        .route_layer(from_fn(note_owner_site))
        .with_state(state)
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// Route layer: notes the matched route, the request ID and the client address as an
/// [`OwnerSite`] extension for [`require_owner`] and the handlers (SEC-048).
pub async fn note_owner_site(mut request: Request, next: Next) -> Response {
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
    request.extensions_mut().insert(OwnerSite {
        route,
        request_id,
        client,
    });
    next.run(request).await
}

/// SEC-005, SEC-006, SEC-043, SEC-048: the live owner principal for `jar` at `now`, else the
/// answer to send: [`owner_signed_out`] (a request that carried any session also records one
/// `authz_denied` event naming that session's row), or a bare 500 on a database failure.
///
/// # Errors
///
/// The refusal or failure response, boxed.
pub async fn require_owner(
    pool: &PgPool,
    jar: &CookieJar,
    site: &OwnerSite,
    now: Timestamp,
) -> Result<OwnerPrincipal, Box<Response>> {
    let Ok(mut db) = pool.acquire().await else {
        return Err(Box::new(failed("owner session")));
    };
    match owner_principal(&mut db, jar, now).await {
        Ok(Some(principal)) => Ok(principal),
        Ok(None) => {
            if jar.get(SESSION_COOKIE).is_some() {
                let actor = current_session(&mut db, jar, now).await.ok().flatten();
                denied(&mut db, site, actor.map(|s| s.id()), now).await;
            }
            Err(Box::new(owner_signed_out()))
        }
        Err(_) => Err(Box::new(failed("owner session"))),
    }
}

/// SEC-048: one `authz_denied` event on `site` with `actor`, under the per-client cap; a failure
/// to record is logged once and never changes the answer.
async fn denied(
    db: &mut sqlx::PgConnection,
    site: &OwnerSite,
    actor: Option<Uuid>,
    now: Timestamp,
) {
    let kind = SecurityEventKind::AuthzDenied;
    if !admit_security_event(kind, site.client, &site.route, now) {
        return;
    }
    let event = SecurityEvent {
        kind,
        route: &site.route,
        request_id: site.request_id,
        actor_ref: actor,
    };
    if record_security_event(db, &event, now).await.is_err() {
        tracing::error!(
            kind = kind.as_str(),
            route = site.route,
            "security event not recorded"
        );
    }
}

/// A failed read or render: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "owner page failed");
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

/// The sign-in page for `jar` (with a pre-session cookie for its CSRF token, SEC-042), its
/// "Use a passkey" button only when a passkey is registered, answering `status`.
async fn sign_in_answer(owner: &Owner, jar: CookieJar, status: StatusCode) -> Response {
    let Ok(jar) = ensure_pre_session(jar) else {
        return failed("pre-session cookie");
    };
    let Some(csrf) = csrf_token(&jar) else {
        return failed("csrf token");
    };
    let Ok(mut db) = owner.pool.acquire().await else {
        return failed("database");
    };
    let Ok(passkey) = passkey_registered(&mut db).await else {
        return failed("passkey read");
    };
    let failed_try = status != StatusCode::OK;
    match owner_signin_page(&csrf, passkey, failed_try) {
        Ok(page) => (status, jar, Html(page)).into_response(),
        Err(_) => failed("sign-in render"),
    }
}

/// `GET` [`OWNER_SIGNIN_ROUTE`]: the sign-in form; it stores nothing but the pre-session cookie
/// (SEC-005, SEC-026, D-1919).
async fn sign_in_page(State(owner): State<Owner>, jar: CookieJar) -> Response {
    sign_in_answer(&owner, jar, StatusCode::OK).await
}

/// The second factor the form carries: the passkey answer when it has one with a challenge ID,
/// else a 6-digit TOTP code, else a recovery code (SEC-005, SEC-007).
fn factor_of(form: &SignInForm) -> SecondFactor<'_> {
    let challenge = Uuid::try_parse(form.challenge.trim()).ok();
    match challenge.filter(|_| !form.passkey.is_empty()) {
        Some(challenge) => SecondFactor::Passkey {
            challenge,
            credential: &form.passkey,
        },
        None => {
            let code = form.code.trim();
            if code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()) {
                SecondFactor::Totp(code)
            } else {
                SecondFactor::RecoveryCode(code)
            }
        }
    }
}

/// Whether `admitted` let the try through; a refusal or a counter failure is a refusal.
fn passed(admitted: Result<Admission, askhava_intake::api::LimitError>) -> bool {
    matches!(admitted, Ok(Admission::Admitted))
}

/// `POST` [`OWNER_SIGNIN_ROUTE`] (SEC-005, SEC-007, SEC-014, SEC-020, SEC-040, SEC-042,
/// SEC-043): every try admits `admit_owner_ip` first; a passkey answer is checked before the
/// password by `owner_passkey_sign_in`, which counts only an unverified one for the account; any
/// other try admits `admit_owner_account` before its argon2 (D-1951, D-2206, D-4294). A refusal
/// checks nothing. Then a 303 to the step-up page with the new session cookie; any failure is the
/// same 401 page.
async fn sign_in(
    State(owner): State<Owner>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<SignInForm>,
) -> Response {
    let now = owner.clock.now();
    if !passed(admit_owner_ip(&owner.pool, site.client, now).await) {
        return sign_in_answer(&owner, jar, StatusCode::UNAUTHORIZED).await;
    }
    let password = form.password.as_str();
    let signed = match factor_of(&form) {
        SecondFactor::Passkey {
            challenge,
            credential,
        } => {
            let answer = (password, challenge, credential);
            owner_passkey_sign_in(&owner.pool, jar.clone(), answer, &owner.auth, now).await
        }
        factor => {
            if !passed(admit_owner_account(&owner.pool, now).await) {
                return sign_in_answer(&owner, jar, StatusCode::UNAUTHORIZED).await;
            }
            let Ok(mut db) = owner.pool.acquire().await else {
                return failed("database");
            };
            owner_sign_in(&mut db, jar.clone(), (password, factor), &owner.auth, now).await
        }
    };
    match signed {
        Ok(jar) => (jar, see_other(OWNER_STEP_UP_ROUTE)).into_response(),
        Err(OwnerAuthError::Refused) => sign_in_answer(&owner, jar, StatusCode::UNAUTHORIZED).await,
        Err(_) => failed("owner sign-in"),
    }
}

/// `POST` [`OWNER_PASSKEY_ROUTE`] (SEC-014, SEC-040, D-4294): admits
/// `admit_owner_passkey_challenge` for the client address first (a refusal is a 429 that issues
/// nothing), then a passkey sign-in challenge as JSON for `passkey.ts`; 404 when no passkey is
/// registered, 429 at the table-size bound (D-098, D-1919).
async fn passkey_options(
    State(owner): State<Owner>,
    Extension(site): Extension<OwnerSite>,
    CsrfForm(_): CsrfForm<ButtonForm>,
) -> Response {
    let now = owner.clock.now();
    if !passed(admit_owner_passkey_challenge(&owner.pool, site.client, now).await) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let Ok(mut db) = owner.pool.acquire().await else {
        return failed("database");
    };
    match passkey_sign_in_options(&mut db, &owner.auth, now).await {
        Ok(Some((challenge, options))) => Json(Issued {
            challenge: challenge.to_string(),
            options,
        })
        .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(OwnerAuthError::Passkey) => StatusCode::TOO_MANY_REQUESTS.into_response(),
        Err(_) => failed("passkey challenge"),
    }
}

/// The step-up page on `screen` for the signed-in owner's `jar`, answering `status`.
fn step_up_answer(jar: &CookieJar, screen: StepUpScreen, status: StatusCode) -> Response {
    let Some(csrf) = csrf_token(jar) else {
        return failed("csrf token");
    };
    match owner_step_up_page(&csrf, screen) {
        Ok(page) => (status, Html(page)).into_response(),
        Err(_) => failed("step-up render"),
    }
}

/// `GET` [`OWNER_STEP_UP_ROUTE`]: the step-up form, for the owner only (SEC-043).
async fn step_up_page(
    State(owner): State<Owner>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
) -> Response {
    let now = owner.clock.now();
    if let Err(refused) = require_owner(&owner.pool, &jar, &site, now).await {
        return *refused;
    }
    step_up_answer(&jar, StepUpScreen::Ask, StatusCode::OK)
}

/// `POST` [`OWNER_STEP_UP_ROUTE`] (SEC-020, SEC-043, SEC-048): `admit_owner_step_up` first (5
/// tries per session, 10 per address an hour, D-4294); a refused try checks no code, ends the
/// session on the server and is [`owner_signed_out`] with the cookie cleared. A right TOTP code
/// restarts the 5-minute window; a wrong, old or reused one is `portals`' `auth_failed` event and
/// the form again, a 400 with no sign-in link (D-3878).
async fn step_up(
    State(owner): State<Owner>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<StepUpForm>,
) -> Response {
    let now = owner.clock.now();
    let mut principal = match require_owner(&owner.pool, &jar, &site, now).await {
        Ok(principal) => principal,
        Err(refused) => return *refused,
    };
    let Ok(mut db) = owner.pool.acquire().await else {
        return failed("database");
    };
    let tries = (principal.session(), site.client);
    if !passed(admit_owner_step_up(&owner.pool, tries, now).await) {
        return match owner_sign_out(&mut db, jar).await {
            Ok(jar) => (jar, owner_signed_out()).into_response(),
            Err(_) => failed("owner step-up limit"),
        };
    }
    let factor = SecondFactor::Totp(form.code.trim());
    let step = (owner.auth.as_ref(), factor, site.route.as_str());
    match owner_step_up(&mut db, &mut principal, step, now).await {
        Ok(()) => step_up_answer(&jar, StepUpScreen::Done, StatusCode::OK),
        Err(OwnerAuthError::Refused) => {
            step_up_answer(&jar, StepUpScreen::Failed, StatusCode::BAD_REQUEST)
        }
        Err(_) => failed("owner step-up"),
    }
}

/// `POST` [`OWNER_SIGN_OUT_ROUTE`] (SEC-043): ends this browser's owner session on the server,
/// clears the cookie and goes to the sign-in page.
async fn sign_out(
    State(owner): State<Owner>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    CsrfForm(_): CsrfForm<ButtonForm>,
) -> Response {
    let now = owner.clock.now();
    if let Err(refused) = require_owner(&owner.pool, &jar, &site, now).await {
        return *refused;
    }
    let Ok(mut db) = owner.pool.acquire().await else {
        return failed("database");
    };
    match owner_sign_out(&mut db, jar).await {
        Ok(jar) => (jar, see_other(OWNER_SIGNIN_ROUTE)).into_response(),
        Err(_) => failed("owner sign-out"),
    }
}

/// `POST` [`OWNER_SIGN_OUT_ALL_ROUTE`] (SEC-043): "Sign out everywhere" ends every owner session
/// with an audit row, clears this browser's cookie and goes to the sign-in page.
async fn sign_out_all(
    State(owner): State<Owner>,
    Extension(site): Extension<OwnerSite>,
    jar: CookieJar,
    CsrfForm(_): CsrfForm<ButtonForm>,
) -> Response {
    let now = owner.clock.now();
    let principal = match require_owner(&owner.pool, &jar, &site, now).await {
        Ok(principal) => principal,
        Err(refused) => return *refused,
    };
    let Ok(mut db) = owner.pool.acquire().await else {
        return failed("database");
    };
    if sign_out_everywhere(&mut db, &principal).await.is_err() {
        return failed("sign out everywhere");
    }
    match owner_sign_out(&mut db, jar).await {
        Ok(jar) => (jar, see_other(OWNER_SIGNIN_ROUTE)).into_response(),
        Err(_) => failed("owner sign-out"),
    }
}
