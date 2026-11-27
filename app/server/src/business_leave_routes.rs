//! The served leave screen and the pause question (BIZ-051, BIZ-054, SEC-038, PICK-047, PICK-046,
//! PICK-051, SEC-009, SEC-034, SEC-042; T734, D-4727, D-5670).
//!
//! [`routes`] serves `GET` and `POST` [`LEAVE_ROUTE`] and [`PAUSE_REASON_ROUTE`] over `portals`'
//! `leave_page`, `leave` and `pause_reason`, behind `refuse_cross_site` and `no_store`; every
//! `POST` passes the CSRF form check first. `business_principal` is read first: no business
//! person gets `signed_out`. `GET /b/leave` shows the owner the leave screen, with the BIZ-054
//! question while `businesses::api::reason_asked` is false; anyone else gets a 303 to the home.
//! `POST /b/leave` from a member who is not the owner is a bare 403, nothing stored
//! (`LeaveError::NotOwner`); without a code from the last 5 minutes it is a 303 to the step-up
//! form and back (SEC-038); on success the left screen, with the session cookie cleared because
//! `leave` already ended every session. Each ended pick's PICK-047 or PICK-046 text was queued by
//! `picks::api::end_for_leaving` inside `leave` (the sender T669's ended pending pick uses), so
//! nothing here queues a second one. A detach failure is counted in `Left::detach_failed` and
//! logged once by `leave`; the cards stay marked removed. `GET /b/pause-reason` shows the question
//! to the owner or a "Confirm and billing" member while `pause_ask_due` holds, else a 303 to the
//! home; its `POST` stores the answer through `pause_reason` (which re-checks both) and answers 303
//! to the home. The answer never changes routing or fees (D-463).
//!
//! `CardDetach` is `portals`' trait and `adapters` may name no other module, so [`ServedDetach`]
//! is the server-local wrapper over `adapters::api::StripeCardDetach` (the orphan rule rules out
//! the impl on the adapter type here); without Stripe settings (dev, CI) every detach is
//! `NotSent` and counted as failed.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;

use askhava_adapters::api::{AdapterError, StripeCardDetach};
use askhava_businesses::api::{PAUSE_ASK_AFTER_DAYS, ReasonAnswer, pause_ask_due, reason_asked};
use askhava_core::api::{Clock, Environment, Settings, SystemClock, check_base_url};
use askhava_portals::api::{
    BUSINESS_HOME_ROUTE, BUSINESS_STEP_UP_ROUTE, BusinessPrincipal, CardDetach, CookieJar,
    CsrfField, CsrfForm, LEAVE_ROUTE, LeaveError, LeaveScreen, Need, PAUSE_REASON_ROUTE,
    SESSION_COOKIE, WebSecurity, business_principal, csrf_token, leave, leave_page, no_store,
    pause_reason, refuse_cross_site, signed_out,
};
use axum::Router;
use axum::extract::{FromRef, State};
use axum::http::StatusCode;
use axum::http::header::{HeaderValue, LOCATION, SET_COOKIE};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use secrecy::SecretString;
use sqlx::PgPool;

/// The stand-in Stripe's base URL, as `release_sink` defaults it (D-453).
const STAND_IN_STRIPE: &str = "http://127.0.0.1:4010/stripe";
/// Stripe's API.
const STRIPE_API: &str = "https://api.stripe.com";

type DetachFuture = Pin<Box<dyn Future<Output = Result<(), AdapterError>> + Send>>;
type DetachFn = dyn Fn(String) -> DetachFuture + Send + Sync;

/// BIZ-051, D-5670: `portals`' `CardDetach` for the composition: the Stripe adapter's detach, a
/// caller's function (tests), or none (every detach `NotSent`).
#[derive(Clone, Default)]
pub struct ServedDetach(Option<Arc<DetachFn>>);

impl ServedDetach {
    /// Detaches through `client` (`POST /v1/payment_methods/{pm}/detach`).
    #[must_use]
    pub fn stripe(client: StripeCardDetach) -> Self {
        let client = Arc::new(client);
        Self::from_fn(move |pm: String| {
            let client = Arc::clone(&client);
            async move { client.detach_card(&pm).await }
        })
    }

    /// Detaches through `detach` (a fake in tests, fake IDs only).
    #[must_use]
    pub fn from_fn<F, Fut>(detach: F) -> Self
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), AdapterError>> + Send + 'static,
    {
        Self(Some(Arc::new(move |pm| Box::pin(detach(pm)))))
    }
}

impl CardDetach for ServedDetach {
    fn detach(
        &self,
        payment_method: &str,
    ) -> impl Future<Output = Result<(), AdapterError>> + Send {
        let detach = self.0.clone();
        let payment_method = payment_method.to_owned();
        async move {
            match detach {
                Some(detach) => detach(payment_method).await,
                None => Err(AdapterError::NotSent),
            }
        }
    }
}

/// BIZ-051, SEC-032: the detach from the Stripe settings `release_sink` reads: the secret key,
/// `ASKHAVA_STRIPE_BASE_URL` (else the stand-in for `stand_in` mode outside live, else Stripe's
/// API), checked by `check_base_url`. With no key in dev or CI it is none.
///
/// # Errors
/// A missing key in test or live, or a bad base URL, named, never a value.
pub fn card_detach<I>(settings: &Settings, pairs: I) -> io::Result<ServedDetach>
where
    I: IntoIterator<Item = (String, String)>,
{
    let pairs: Vec<(String, String)> = pairs.into_iter().collect();
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(k, v)| k == key && !v.trim().is_empty())
            .map(|(_, v)| v.trim().to_owned())
    };
    let environment = settings.environment();
    let Some(key) = settings.stripe_secret_key() else {
        return match environment {
            Environment::Dev | Environment::Ci => Ok(ServedDetach::default()),
            Environment::Test | Environment::Live => {
                Err(io::Error::other("missing ASKHAVA_STRIPE_SECRET_KEY"))
            }
        };
    };
    let stand_in = environment != Environment::Live
        && get(crate::release_sink::STRIPE_MODE).is_none_or(|m| m == "stand_in");
    let base = get(crate::release_sink::STRIPE_BASE_URL).unwrap_or_else(|| {
        (if stand_in {
            STAND_IN_STRIPE
        } else {
            STRIPE_API
        })
        .to_owned()
    });
    let key_name = crate::release_sink::STRIPE_BASE_URL;
    check_base_url(environment, key_name, &base).map_err(io::Error::other)?;
    let key = SecretString::from(secrecy::ExposeSecret::expose_secret(key).to_owned());
    let client = StripeCardDetach::new(&base, key).map_err(io::Error::other)?;
    Ok(ServedDetach::stripe(client))
}

/// The routes' state.
#[derive(Clone)]
struct Served {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    detach: ServedDetach,
}

impl FromRef<Served> for WebSecurity {
    fn from_ref(served: &Served) -> Self {
        served.security.clone()
    }
}

/// The posted reason form (BIZ-054): the choice code and the "Something else" box, both optional.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReasonForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    other: String,
}

impl CsrfField for ReasonForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

impl ReasonForm {
    /// BIZ-054: the parsed answer; no choice is no answer.
    fn answer(&self) -> Option<ReasonAnswer> {
        ReasonAnswer::parse(&self.reason, &self.other)
    }
}

/// BIZ-051, BIZ-054, SEC-034, SEC-042: `GET` and `POST` [`LEAVE_ROUTE`] and
/// [`PAUSE_REASON_ROUTE`] over `pool`, detaching cards through `detach`.
pub fn routes(pool: PgPool, detach: ServedDetach) -> Router {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Served {
        pool,
        clock,
        security: security.clone(),
        detach,
    };
    Router::new()
        .route(LEAVE_ROUTE, get(leave_screen).post(leave_press))
        .route(PAUSE_REASON_ROUTE, get(pause_screen).post(pause_press))
        .with_state(state)
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// A failed read or render: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "leave page failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

fn see_other(path: &str) -> Response {
    match HeaderValue::from_str(path) {
        Ok(to) => (StatusCode::SEE_OTHER, [(LOCATION, to)]).into_response(),
        Err(_) => failed("location"),
    }
}

/// SEC-004: the signed-in business person, else the sign-in answer.
async fn signed_in(served: &Served, jar: &CookieJar) -> Result<BusinessPrincipal, Box<Response>> {
    let Ok(mut db) = served.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    match business_principal(&mut db, jar, served.clock.now()).await {
        Ok(Some(who)) => Ok(who),
        Ok(None) => Err(Box::new(signed_out())),
        Err(_) => Err(Box::new(failed("business principal"))),
    }
}

/// `screen` with the session's CSRF token.
fn render(screen: LeaveScreen, jar: &CookieJar) -> Response {
    let Some(csrf) = csrf_token(jar) else {
        return failed("csrf token");
    };
    match leave_page(screen, &csrf) {
        Ok(html) => Html(html).into_response(),
        Err(_) => failed("render"),
    }
}

/// `GET` [`LEAVE_ROUTE`] (BIZ-051, BIZ-054, SEC-026): the owner's leave screen, with the question
/// while the business was never asked; anyone else a 303 to the home. Changes nothing.
async fn leave_screen(State(served): State<Served>, jar: CookieJar) -> Response {
    let who = match signed_in(&served, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    if !who.is_owner() {
        return see_other(BUSINESS_HOME_ROUTE);
    }
    let Ok(mut db) = served.pool.acquire().await else {
        return failed("connection");
    };
    let asked = reason_asked(&mut db, who.business()).await;
    let Ok(asked) = asked else {
        return failed("reason asked");
    };
    render(LeaveScreen::Leave { ask: !asked }, &jar)
}

/// `POST` [`LEAVE_ROUTE`] (BIZ-051, BIZ-054, SEC-038, PICK-047, PICK-046, PICK-051): `leave`
/// with the optional answer and [`ServedDetach`]; see the module comment for each answer.
async fn leave_press(
    State(served): State<Served>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<ReasonForm>,
) -> Response {
    let who = match signed_in(&served, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    if !who.is_owner() {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(mut db) = served.pool.acquire().await else {
        return failed("connection");
    };
    let answer = form.answer();
    let pair = (answer.as_ref(), &served.detach);
    match leave(&mut db, &who, pair, served.clock.now()).await {
        Ok(_) => left_screen(),
        Err(LeaveError::NeedsCode) => {
            let back = LEAVE_ROUTE.replace('/', "%2F");
            see_other(&format!("{BUSINESS_STEP_UP_ROUTE}?return={back}"))
        }
        Err(LeaveError::NotOwner) => StatusCode::FORBIDDEN.into_response(),
        Err(LeaveError::AlreadyLeft) => see_other(BUSINESS_HOME_ROUTE),
        Err(LeaveError::Database) => failed("leave"),
    }
}

/// BIZ-051, SEC-009: the left screen; every session already ended, so the browser's session
/// cookie is cleared with the same `__Host-` attributes it was set with.
fn left_screen() -> Response {
    let Ok(html) = leave_page(LeaveScreen::Left, "") else {
        return failed("render");
    };
    let clear = format!("{SESSION_COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0");
    match HeaderValue::from_str(&clear) {
        Ok(cookie) => ([(SET_COOKIE, cookie)], Html(html)).into_response(),
        Err(_) => failed("cookie"),
    }
}

/// BIZ-054: whether `who` is shown the pause question now: the owner or a "Confirm and billing"
/// member, while `pause_ask_due` holds.
async fn pause_due(served: &Served, who: &BusinessPrincipal) -> Result<bool, &'static str> {
    if !who.may(who.business(), Need::ConfirmAndBilling) {
        return Ok(false);
    }
    let mut db = served.pool.acquire().await.map_err(|_| "connection")?;
    let now = served.clock.now();
    pause_ask_due(&mut db, who.business(), PAUSE_ASK_AFTER_DAYS, now)
        .await
        .map_err(|_| "pause due")
}

/// `GET` [`PAUSE_REASON_ROUTE`] (BIZ-054, SEC-026): the question once it is due, else a 303 to
/// the home. Changes nothing.
async fn pause_screen(State(served): State<Served>, jar: CookieJar) -> Response {
    let who = match signed_in(&served, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    match pause_due(&served, &who).await {
        Ok(true) => render(LeaveScreen::PauseAsk, &jar),
        Ok(false) => see_other(BUSINESS_HOME_ROUTE),
        Err(what) => failed(what),
    }
}

/// `POST` [`PAUSE_REASON_ROUTE`] (BIZ-054, D-463): stores the answer (or the skip) once through
/// `pause_reason`, which re-checks who and when; a 303 to the home either way.
async fn pause_press(
    State(served): State<Served>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<ReasonForm>,
) -> Response {
    let who = match signed_in(&served, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Ok(mut db) = served.pool.acquire().await else {
        return failed("connection");
    };
    let answer = form.answer();
    match pause_reason(&mut db, &who, answer.as_ref(), served.clock.now()).await {
        Ok(_) => see_other(BUSINESS_HOME_ROUTE),
        Err(_) => failed("pause reason"),
    }
}
