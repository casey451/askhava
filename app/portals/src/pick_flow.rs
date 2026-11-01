//! The quote page's pick review step and the pick result (PICK-016, PICK-017, PICK-045, PICK-054,
//! A11Y-014, A11Y-031; T055, D-1248 to D-1250).
//!
//! A quote card's Pick form posts to the quote page's own address (`POST /q/{token}`); the answer
//! is the review step (`templates/pick_review.html`): what happens next, "or reply <n> YES", a
//! "Go back" link to the card, and the final Pick form posting to [`PICK_ROUTE`] with only "By
//! picking, you agree to the Terms" directly beside its button (PICK-016) and no Confirm end time
//! (TD-PICK-28). The final Pick is where the Verify code step (PICK-013, T187) starts; until it and
//! the stored page pick exist, the final POST re-checks the link and the quote and sends the
//! homeowner back to her card, so no page claims a pick that was not made (D-1248). [`result_page`]
//! renders the page shown once a pick is stored: its heading takes focus on load (A11Y-014), it
//! offers the Ask Hava contact card and the after-Done line (PICK-054). Both routes are POST only,
//! behind the cross-site refusal, read through [`CsrfForm`], `no-store` and `noindex`; neither
//! changes anything, so a double submit is harmless (A11Y-031). No name, phone or address is read.

use std::sync::Arc;

use askama::Template;
use askhava_core::api::{Clock, LinkToken};
use askhava_jobs::api::quote_token::resolve;
use askhava_jobs::api::{current, job_number};
use askhava_quotes::api::page_quotes;
use axum::Router;
use axum::extract::{FromRef, Path, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::post;
use axum_extra::extract::cookie::CookieJar;
use sqlx::PgPool;

use crate::quote_page::{QUOTE_PAGE_ROUTE, number_quotes, token_expired};
use crate::security::{CsrfField, CsrfForm, WebSecurity, no_store, refuse_cross_site};
use crate::session::csrf_token;

/// The final Pick's route on the review step (PICK-016, PICK-017).
pub const PICK_ROUTE: &str = "/q/{token}/pick";

/// The language of the review step's Terms line (PICK-016: English and Spanish).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageLanguage {
    /// English.
    En,
    /// Spanish.
    Es,
}

impl PageLanguage {
    /// The `lang` code, the words before the link and the link's word (PICK-016).
    const fn terms(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::En => ("en", "By picking, you agree to the", "Terms"),
            Self::Es => ("es", "Al elegir, usted acepta los", "Términos"),
        }
    }
}

/// The posted Pick form: the CSRF token and the quote's number on the page (PICK-008).
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PickForm {
    #[serde(default)]
    csrf: String,
    quote: usize,
}

impl CsrfField for PickForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// The review step or the result (`templates/pick_review.html`).
#[derive(Template)]
#[template(path = "pick_review.html")]
pub(crate) struct ReviewView {
    /// The request (job) number.
    pub(crate) request: i64,
    /// The quote's number on the page.
    pub(crate) quote: usize,
    /// The quote page's token, for "Go back" and the final form's address.
    pub(crate) token: String,
    /// The final form's CSRF token (SEC-042).
    pub(crate) csrf: String,
    /// `lang`, lead words and link word of the Terms line.
    pub(crate) terms: (&'static str, &'static str, &'static str),
    /// `true` once the pick is stored: the result shows instead of the review.
    pub(crate) done: bool,
}

impl ReviewView {
    /// The review step for quote `quote` of request `request` (PICK-017).
    pub(crate) fn review(request: i64, quote: usize, token: String, csrf: String) -> Self {
        let terms = PageLanguage::En.terms();
        Self {
            request,
            quote,
            token,
            csrf,
            terms,
            done: false,
        }
    }

    /// The same view with the Terms line in `language` (PICK-016). Only tests call it until the
    /// page reads the request's language (D-1236 item 5, D-1249).
    #[cfg(test)]
    #[must_use]
    pub(crate) fn in_language(mut self, language: PageLanguage) -> Self {
        self.terms = language.terms();
        self
    }
}

/// The page shown once a page pick of quote `quote` on request `request` is stored, with `token`
/// for the link back to the quote page: its heading takes focus (A11Y-014), then PICK-045's next
/// steps, the "Add Ask Hava to your contacts" card and the after-Done line (PICK-054). The
/// stored page pick (T187) calls it; it never says when the business must accept (TD-PICK-28).
///
/// # Errors
///
/// [`askama::Error`] when the template fails to render.
pub fn result_page(request: i64, quote: usize, token: String) -> Result<String, askama::Error> {
    let mut view = ReviewView::review(request, quote, token, String::new());
    view.done = true;
    view.render()
}

/// The routes' state: the pool, the clock the 30-day rule reads and the refusals' state.
#[derive(Clone)]
struct PickFlow {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
}

impl FromRef<PickFlow> for WebSecurity {
    fn from_ref(flow: &PickFlow) -> Self {
        flow.security.clone()
    }
}

/// The pick flow's router over `pool`, timed by `clock` (PICK-016, PICK-017): `POST /q/{token}`
/// (the review step) and `POST` [`PICK_ROUTE`], behind the cross-site refusal and `no-store`.
/// `server::build_router` merges it beside the quote page's `GET`.
pub fn pick_flow_routes(pool: PgPool, clock: Arc<dyn Clock>) -> Router {
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = PickFlow {
        pool,
        clock,
        security: security.clone(),
    };
    Router::new()
        .route(QUOTE_PAGE_ROUTE, post(review))
        .route(PICK_ROUTE, post(final_pick))
        .with_state(state)
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// `POST /q/{token}`: the review step for a quote that can still be picked (PICK-017); a bad link
/// is a bare 404 (SEC-037), a quote that can't be picked goes back to the quote page.
async fn review(
    State(flow): State<PickFlow>,
    Path(raw): Path<String>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<PickForm>,
) -> Response {
    let request = match pickable(&flow, &raw, form.quote).await {
        Ok(Some(request)) => request,
        Ok(None) => return back(&raw, form.quote),
        Err(status) => return status.into_response(),
    };
    let csrf = csrf_token(&jar).unwrap_or_default();
    match ReviewView::review(request, form.quote, raw, csrf).render() {
        Ok(html) => page(html),
        Err(_) => {
            tracing::warn!("pick review template failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /q/{token}/pick`: the final Pick. It re-checks the link and the quote; the code step and
/// the stored pick are T187's, so for now it returns the homeowner to her card (D-1248).
async fn final_pick(
    State(flow): State<PickFlow>,
    Path(raw): Path<String>,
    CsrfForm(form): CsrfForm<PickForm>,
) -> Response {
    match pickable(&flow, &raw, form.quote).await {
        Ok(_) => back(&raw, form.quote),
        Err(status) => status.into_response(),
    }
}

/// The request number when quote `quote` behind token `raw` can be picked now: the link opens
/// (SEC-037, PICK-011), the job has had no pick (PICK-012) and the quote is on the page and
/// available (PICK-008). `Ok(None)` when it can't; `Err(404)` for a bad link, `Err(500)` on a
/// failed read (logged once, with no value).
async fn pickable(flow: &PickFlow, raw: &str, quote: usize) -> Result<Option<i64>, StatusCode> {
    let token = LinkToken::parse(raw).map_err(|_| StatusCode::NOT_FOUND)?;
    let read = |()| {
        tracing::warn!("pick flow read failed");
        StatusCode::INTERNAL_SERVER_ERROR
    };
    let mut db = flow.pool.acquire().await.map_err(|_| read(()))?;
    let job = resolve(&mut db, &token).await.map_err(|_| read(()))?;
    let job = job.ok_or(StatusCode::NOT_FOUND)?;
    let row = current(&mut db, job).await.map_err(|_| read(()))?;
    if token_expired(row.status.is_final(), row.status_at, flow.clock.now()) {
        return Err(StatusCode::NOT_FOUND);
    }
    if !row.status.takes_quote_edits() {
        return Ok(None);
    }
    let quotes = page_quotes(&mut db, job).await.map_err(|_| read(()))?;
    let open = number_quotes(quotes)
        .iter()
        .any(|q| q.number == quote && !q.quote.unavailable);
    if !open {
        return Ok(None);
    }
    job_number(&mut db, job)
        .await
        .map(Some)
        .map_err(|_| read(()))
}

/// Back to quote `quote`'s card on the quote page (a 303, so a reload never re-posts).
fn back(raw: &str, quote: usize) -> Response {
    Redirect::to(&format!("/q/{raw}#quote-{quote}")).into_response()
}

/// A rendered page with PICK-010's `X-Robots-Tag: noindex, nofollow`.
fn page(html: String) -> Response {
    let robots = HeaderValue::from_static("noindex, nofollow");
    let name = HeaderName::from_static("x-robots-tag");
    ([(name, robots)], Html(html)).into_response()
}
