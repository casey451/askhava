//! The homeowner's quote page at `/q/{token}` (PICK-008 to PICK-012, WORD-005, SEC-037, A11Y-033;
//! TD-PICK-38's page lines, D-491 to D-493).
//!
//! The token is the job's live quote-page link token (`jobs::api::quote_token`, an HMAC over the
//! job ID and a 128-bit OS-random salt, stored only as its SHA256); an unknown, re-issued or
//! revoked token, or one 30 days after the job reached a final status, is a bare 404 (PICK-011).
//! The page lists every shown quote numbered in arrival order with no sort or filter, a number
//! never moving; a quote `picks` marked unavailable keeps its number and shows "no longer
//! available" with no Pick button (D-084). Every other quote has a Pick button from the first one
//! on, with "or reply <n> YES" beside it (PICK-012, PICK-015). The page shows no name, phone or
//! address: none is read here (PICK-010). It is `noindex`, `no-store` and behind the cross-site
//! refusal, so a cross-site POST is refused (TD-PICK-05). `static/quote_page.ts` polls this same
//! page to add new quotes; without JavaScript a reload does the same (D-492).

use std::sync::Arc;

use askama::Template;
use askhava_core::api::{Clock, LinkToken};
use askhava_intake::api::paused_by_homeowner;
use askhava_jobs::api::quote_token::resolve;
use askhava_jobs::api::{current, job_number};
use askhava_quotes::api::{PageQuote, page_quotes};
use axum::Router;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum_extra::extract::cookie::CookieJar;
use jiff::{SignedDuration, Timestamp};
use sqlx::PgPool;

use crate::security::{WebSecurity, no_store, refuse_cross_site};
use crate::session::{csrf_token, ensure_pre_session};

/// PICK-011: how long a quote-page token keeps working after the job reaches a final status.
pub const TOKEN_LIFE_AFTER_CLOSE: SignedDuration = SignedDuration::from_hours(30 * 24);

/// The route of the quote page; the token is the one path segment (PICK-010).
pub const QUOTE_PAGE_ROUTE: &str = "/q/{token}";

/// One numbered quote on the page (PICK-008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Numbered {
    /// Its place in arrival order, from 1; never changes (D-084).
    pub number: usize,
    /// The quote as `quotes` stored it.
    pub quote: PageQuote,
}

/// PICK-008, D-084: `quotes` numbered from 1 in arrival order (time received, then business ID),
/// whatever order they come in; an unavailable quote keeps its number, so a later quote's never
/// moves.
#[must_use]
pub fn number_quotes(mut quotes: Vec<PageQuote>) -> Vec<Numbered> {
    quotes.sort_by_key(|q| (q.received_at, q.business));
    quotes
        .into_iter()
        .enumerate()
        .map(|(i, quote)| Numbered {
            number: i + 1,
            quote,
        })
        .collect()
}

/// PICK-011: whether a token stops working at `now`: only once the job is final and 30 days have
/// passed since it got there (`closed_at`); it still works at 29 days 23:59:59.
#[must_use]
pub fn token_expired(is_final: bool, closed_at: Timestamp, now: Timestamp) -> bool {
    is_final && now.duration_since(closed_at) >= TOKEN_LIFE_AFTER_CLOSE
}

/// The quote page's state: the pool and the clock the 30-day rule reads.
#[derive(Clone)]
struct QuotePage {
    pool: PgPool,
    clock: Arc<dyn Clock>,
}

/// The page's template (`templates/quote_page.html`): the request number, the lines and the cards.
#[derive(Template)]
#[template(path = "quote_page.html")]
pub(crate) struct PageView {
    /// The request (job) number the homeowner was texted.
    pub(crate) request: i64,
    /// A live job paused by the homeowner's sole-word CANCEL: the CON-028 line shows.
    pub(crate) still_open: bool,
    /// The job is not final, so `quote_page.ts` keeps polling.
    pub(crate) live: bool,
    /// The job has had no pick, so available quotes carry a Pick button (PICK-012).
    pub(crate) pick_open: bool,
    /// The Pick forms' CSRF token (SEC-042).
    pub(crate) csrf: String,
    /// The quotes in arrival order (PICK-008).
    pub(crate) quotes: Vec<Numbered>,
}

/// Why the page could not be built; logged once, with no value (rust-production R5).
#[derive(Debug, thiserror::Error)]
enum PageError {
    #[error("a quote-page read failed")]
    Read,
}

/// The quote page's router over `pool`, timed by `clock` (PICK-008 to PICK-012, D-491): `GET
/// /q/{token}` behind the cross-site refusal (TD-PICK-05), `no-store` and `noindex` (PICK-010).
/// `server::build_router` merges it.
pub fn quote_page_routes(pool: PgPool, clock: Arc<dyn Clock>) -> Router {
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    Router::new()
        .route(QUOTE_PAGE_ROUTE, get(show))
        .with_state(QuotePage { pool, clock })
        .layer(from_fn(noindex))
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// PICK-010: `X-Robots-Tag: noindex, nofollow` on every quote-page response.
async fn noindex(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let value = HeaderValue::from_static("noindex, nofollow");
    response
        .headers_mut()
        .insert(HeaderName::from_static("x-robots-tag"), value);
    response
}

/// `GET /q/{token}`: the page, or a bare 404 for a malformed, unknown, revoked or expired token
/// (SEC-037, PICK-011). Changes nothing but the pre-session cookie its Pick forms need (SEC-042).
async fn show(State(page): State<QuotePage>, Path(raw): Path<String>, jar: CookieJar) -> Response {
    let Ok(token) = LinkToken::parse(&raw) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(jar) = ensure_pre_session(jar) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let csrf = csrf_token(&jar).unwrap_or_default();
    let view = match load(&page, &token, csrf).await {
        Ok(Some(view)) => view,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(err) => {
            tracing::warn!(error = %err, "quote page not built");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    match view.render() {
        Ok(html) => (jar, Html(html)).into_response(),
        Err(_) => {
            tracing::warn!("quote page template failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Reads the page for `token`, or `None` when the token opens nothing now (SEC-037, PICK-011).
/// Pick buttons show while the job has had no pick (`takes_quote_edits`: Open, No quotes yet,
/// Quotes ready; D-1234); the CON-028 line while a live job is paused by the homeowner.
async fn load(
    page: &QuotePage,
    token: &LinkToken,
    csrf: String,
) -> Result<Option<PageView>, PageError> {
    let mut db = page.pool.acquire().await.map_err(|_| PageError::Read)?;
    let Some(job) = resolve(&mut db, token).await.map_err(|_| PageError::Read)? else {
        return Ok(None);
    };
    let row = current(&mut db, job).await.map_err(|_| PageError::Read)?;
    let is_final = row.status.is_final();
    if token_expired(is_final, row.status_at, page.clock.now()) {
        return Ok(None);
    }
    let request = job_number(&mut db, job)
        .await
        .map_err(|_| PageError::Read)?;
    let paused = paused_by_homeowner(&mut db, job).await;
    let quotes = page_quotes(&mut db, job)
        .await
        .map_err(|_| PageError::Read)?;
    Ok(Some(PageView {
        request,
        still_open: !is_final && paused.map_err(|_| PageError::Read)?,
        live: !is_final,
        pick_open: row.status.takes_quote_edits(),
        csrf,
        quotes: number_quotes(quotes),
    }))
}
