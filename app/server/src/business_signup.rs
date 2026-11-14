//! The served business sign-up (BIZ-021, BIZ-030, CON-019, SEC-042; T302, D-2128, D-3641,
//! D-3676).
//!
//! [`ServedSignUp`] holds the sign-up routes `build_router` merges through
//! `RouterParts::with_sign_up`. This part serves only the form page, `GET` [`SIGNUP_ROUTE`],
//! without a session: it sets the pre-session `__Host-` cookie (`ensure_pre_session`), puts that
//! cookie's CSRF token in the form (SEC-042) and seals the form's shown channels for that token
//! with `ASKHAVA_ID_SECRET` (`sealed_signup_page`), so a later post can prove the wording it saw;
//! the E-SIGN box is never ticked on the served form (CON-019). The category list is the stored
//! switched-on categories, labelled by the shipped class map (`stored_category_list`, BIZ-021);
//! the fee box comes from the Confirm window setting (BIZ-030). The answer carries
//! `Cache-Control: no-store`. The later steps join this part in order (D-3676): T459 the form
//! `POST` (BIZ-001, BIZ-016, SEC-014, SEC-015, SEC-034, SEC-048), T460 the identity steps
//! (BIZ-002), T461 the service area and the email code (BIZ-019, BIZ-040, BIZ-058), T462 the inbox
//! check (BIZ-042); this file names their IDs now because it exists before they run.

use std::io;
use std::sync::Arc;

use askhava_businesses::api::{stored_category_list, stored_enabled};
use askhava_core::api::IdSecret;
use askhava_licensing::api::ClassMap;
use askhava_picks::api::{ConfirmSettings, WindowRules};
use askhava_portals::api::{
    CookieJar, PageLanguage, Parts, SIGNUP_ROUTE, Screen, csrf_token, ensure_pre_session,
    fee_numbers, sealed_signup_page,
};
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, HeaderValue};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use secrecy::SecretString;
use sqlx::PgPool;

/// TXT-035: business texting is off, so the form offers email notices alone and the E-SIGN box
/// names email only (BIZ-036, CON-019).
const TEXTING_ON: bool = false;

/// What the form page reads: the pool, the sealing key and the fee box's settings.
struct Form {
    pool: PgPool,
    secret: IdSecret,
    confirm: ConfirmSettings,
}

/// The served sign-up routes (T302: the form page only).
pub struct ServedSignUp(Router);

impl ServedSignUp {
    /// The form page over `pool`, sealing its shown channels with `secret` (CON-019, D-3602) and
    /// showing the fee box with `confirm`'s window (BIZ-030).
    #[must_use]
    pub fn new(pool: PgPool, secret: IdSecret, confirm: ConfirmSettings) -> Self {
        let state = Arc::new(Form {
            pool,
            secret,
            confirm,
        });
        let routes = Router::new()
            .route(SIGNUP_ROUTE, get(form_page))
            .with_state(state);
        Self(routes)
    }

    /// The part `main` passes to `build_router`: `ASKHAVA_ID_SECRET` and the Confirm window from
    /// `pairs` (D-3055).
    ///
    /// # Errors
    ///
    /// A missing or weak `ASKHAVA_ID_SECRET` or a bad Confirm window setting. Never a value.
    pub fn from_settings<I>(pool: PgPool, pairs: I) -> io::Result<Self>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let pairs: Vec<(String, String)> = pairs.into_iter().collect();
        let secret = pairs
            .iter()
            .find(|(k, v)| k == "ASKHAVA_ID_SECRET" && !v.trim().is_empty())
            .map(|(_, v)| SecretString::from(v.trim().to_owned()))
            .ok_or_else(|| io::Error::other("missing ASKHAVA_ID_SECRET"))?;
        let secret = IdSecret::new(1, secret).map_err(io::Error::other)?;
        let confirm = ConfirmSettings::from_pairs(pairs).map_err(io::Error::other)?;
        Ok(Self::new(pool, secret, confirm))
    }

    /// The routes, state applied, ready to merge.
    pub fn into_router(self) -> Router {
        self.0
    }
}

/// BIZ-021, CON-019, SEC-042: `GET` [`SIGNUP_ROUTE`], the form with the box unticked, the
/// pre-session cookie and its CSRF token, and the shown channels sealed for that token. Changes
/// nothing stored; any failure is a bare 500 logged once without values.
async fn form_page(State(form): State<Arc<Form>>, jar: CookieJar) -> Response {
    let Ok(jar) = ensure_pre_session(jar) else {
        return failure("pre-session cookie");
    };
    let csrf = csrf_token(&jar).unwrap_or_default();
    let Some(categories) = categories(&form.pool).await else {
        return failure("category list");
    };
    let parts = Parts {
        texting_on: TEXTING_ON,
        language: PageLanguage::En,
        csrf: &csrf,
        fees: fee_numbers(form.confirm, WindowRules::default()),
    };
    match sealed_signup_page(Screen::Form(&categories), &parts, &form.secret) {
        Ok(html) => {
            let no_store = [(CACHE_CONTROL, HeaderValue::from_static("no-store"))];
            (jar, no_store, Html(html)).into_response()
        }
        Err(_) => failure("sign-up template"),
    }
}

/// BIZ-021, LIC-003: the stored switched-on categories in code order, each "Open now" or
/// "Coming later" by the shipped class map; `None` when the database or the map fails.
async fn categories(pool: &PgPool) -> Option<Vec<(String, &'static str)>> {
    let mut conn = pool.acquire().await.ok()?;
    let enabled = stored_enabled(&mut conn).await.ok()?;
    let map = ClassMap::shipped().ok()?;
    let codes: Vec<&str> = enabled.iter().map(String::as_str).collect();
    Some(stored_category_list(map, &enabled, &codes))
}

fn failure(what: &'static str) -> Response {
    tracing::warn!(what, "business sign-up page failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}
