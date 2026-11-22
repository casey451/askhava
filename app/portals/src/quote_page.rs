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
//!
//! T388 (PICK-042, PICK-054, D-3078): while the job's pick is `paid` or `free`
//! (`picks::api::accepted_pick`) and the token works, the page shows that quote above the list as
//! the quote she accepted (number, type, amount, earliest visit, the business's own words) with
//! the "Add Ask Hava to your contacts" card; no name, phone or address.
//!
//! T440 (PICK-053, D-2332, D-3626): a tiers quote's Pick form offers one radio per package, read
//! from `quotes::api::quoted_prices` and carried on [`ShownPage`] (not on [`PageView`]), each
//! posting its place, price and name for the pick flow to check; `?choose=<n>` (the pick flow's
//! answer to no package or a stale one) adds the error to card `n`'s choices.
//!
//! T393 (PICK-041, IC-71, D-3642 to D-3645): each shown quote with stored facts carries its card
//! from `quote_cards` (name, license lines, review links, the "check it yourself" line, PICK-040
//! facts), rendered as its lines with plain links; `?lang=es` gives the Spanish card lines
//! (D-3645). A quote with no card keeps today's lines.
//!
//! T308 (JOB-018, JOB-019, SEC-042, SEC-048, D-2128, D-3877): while the job is not final the page
//! shows its close button, a `ButtonForm` posting its CSRF token to [`CLOSE_ROUTE`] (not the page's
//! own path, whose `POST` is the pick flow's review step). That `POST` passes the cross-site
//! refusal and the CSRF check (each refusal a `csrf_rejected` event), then closes the job through
//! `intake::api::close_from_page`, sending no text, and answers 200 with the page saying it is
//! closed; a second press or a job already final changes nothing. An unknown token is a bare 404.

use std::sync::Arc;

use askama::Template;
use askhava_core::api::{Clock, IdSecret, LinkToken};
use askhava_intake::api::{close_from_page, paused_by_homeowner};
use askhava_jobs::api::quote_token::resolve;
use askhava_jobs::api::{JobStatus, current, job_number};
use askhava_licensing::api::{CardLanguage, CardLine};
use askhava_picks::api::accepted_pick;
use askhava_privacy::api::Vault;
use askhava_quotes::api::{
    AcceptedQuote, PageQuote, Price, QuotesError, accepted_quote, page_quotes, quoted_prices,
    shown_numbers,
};
use axum::Router;
use axum::extract::{FromRef, Path, RawQuery, Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum_extra::extract::cookie::CookieJar;
use jiff::{SignedDuration, Timestamp};
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::confirm_pay::money;
use crate::pick_flow::tier_value;
use crate::quote_cards::{Cards, quote_cards};
use crate::security::{ButtonForm, CsrfForm, WebSecurity, no_store, refuse_cross_site};
use crate::session::{csrf_token, ensure_pre_session};
use crate::update_channel::{
    Said, UPDATES_CONFIRM_ROUTE, UPDATES_ROUTE, UpdateKeys, Updates, confirm_query, post_confirm,
    post_updates, updates_view,
};

/// PICK-011: how long a quote-page token keeps working after the job reaches a final status.
pub const TOKEN_LIFE_AFTER_CLOSE: SignedDuration = SignedDuration::from_hours(30 * 24);

/// The route of the quote page; the token is the one path segment (PICK-010).
pub const QUOTE_PAGE_ROUTE: &str = "/q/{token}";

/// The quote page's close button's route (JOB-019, D-2128): its own path, since the page's own
/// `POST` is the pick flow's review step.
pub const CLOSE_ROUTE: &str = "/q/{token}/close";

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

/// PICK-008, D-2632, D-3878: `quotes` numbered by the shown number each kept since it was first
/// shown (`shown`, `quotes::api::shown_numbers`' `(number, business)`), lowest first, so a
/// withdrawn or unavailable quote keeps its place and number and the page, its packages and its
/// pick forms agree with the LIST and the text picks. A quote with no shown number (none: every
/// confirmed quote gets one, migration 0202) is left off.
#[must_use]
pub fn number_shown(quotes: Vec<PageQuote>, shown: &[(u32, Uuid)]) -> Vec<Numbered> {
    let mut numbered: Vec<Numbered> = quotes
        .into_iter()
        .filter_map(|quote| {
            let (n, _) = shown.iter().find(|(_, b)| *b == quote.business)?;
            let number = usize::try_from(*n).ok()?;
            Some(Numbered { number, quote })
        })
        .collect();
    numbered.sort_by_key(|q| q.number);
    numbered
}

/// PICK-008, D-2632, D-3878: `job`'s page quotes numbered by their shown numbers
/// ([`number_shown`]).
///
/// # Errors
///
/// `page_quotes`' or `shown_numbers`' [`QuotesError`].
pub(crate) async fn shown_page(
    db: &mut sqlx::PgConnection,
    job: Uuid,
) -> Result<Vec<Numbered>, QuotesError> {
    let quotes = page_quotes(db, job).await?;
    let shown = shown_numbers(db, job).await?;
    Ok(number_shown(quotes, &shown))
}

/// PICK-011: whether a token stops working at `now`: only once the job is final and 30 days have
/// passed since it got there (`closed_at`); it still works at 29 days 23:59:59.
#[must_use]
pub fn token_expired(is_final: bool, closed_at: Timestamp, now: Timestamp) -> bool {
    is_final && now.duration_since(closed_at) >= TOKEN_LIFE_AFTER_CLOSE
}

/// The quote she accepted, as the page shows it (PICK-042): English, as the page is (D-1236).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Accepted {
    /// Its number on the page.
    pub(crate) number: usize,
    /// Its type ("Firm price").
    pub(crate) kind: &'static str,
    /// The amount as stored.
    pub(crate) amount: String,
    /// The earliest visit day, if given.
    pub(crate) visit: Option<String>,
    /// The business's own words.
    pub(crate) words: String,
}

impl From<AcceptedQuote> for Accepted {
    fn from(q: AcceptedQuote) -> Self {
        Self {
            number: q.number,
            kind: q.kind(false),
            amount: q.amount(false),
            visit: q.visit(false),
            words: q.original,
        }
    }
}

/// The quote page's state: the pool, the clock the 30-day rule reads, the close form's CSRF
/// refusal's event log (SEC-048) and the update address's keys, if any (TXT-018, T729).
#[derive(Clone)]
pub(crate) struct QuotePage {
    pub(crate) pool: PgPool,
    pub(crate) clock: Arc<dyn Clock>,
    security: WebSecurity,
    pub(crate) keys: Option<Arc<UpdateKeys>>,
}

impl FromRef<QuotePage> for WebSecurity {
    fn from_ref(page: &QuotePage) -> Self {
        page.security.clone()
    }
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

impl PageView {
    /// The page alone shows no accepted quote; [`ShownPage`] adds it (PICK-042, D-3078).
    pub(crate) const fn accepted(&self) -> Option<&Accepted> {
        None
    }

    /// The page alone offers no package; [`ShownPage`] adds them (PICK-053, D-3626).
    pub(crate) const fn packages(&self, _number: &usize) -> Vec<Package> {
        Vec::new()
    }

    /// The page alone shows no package error; [`ShownPage`] adds it (PICK-053).
    pub(crate) const fn choose_again(&self, _number: &usize) -> bool {
        false
    }

    /// The page alone has no failed Pick, so no error title or summary (A11Y-030, D-3629).
    pub(crate) const fn choose_error(&self) -> Option<usize> {
        None
    }

    /// The page alone carries no card; [`ShownPage`] adds them (PICK-041, T393).
    pub(crate) const fn card(&self, _number: &usize) -> Option<&[CardLine]> {
        None
    }

    /// The page alone shows its cards in English (D-3645).
    pub(crate) const fn spanish(&self) -> bool {
        false
    }

    /// The page alone has no close button; [`ShownPage`] adds it (JOB-019, T308).
    pub(crate) const fn close_action(&self) -> Option<&str> {
        None
    }

    /// The page alone says nothing about a close (JOB-019, T308).
    pub(crate) const fn just_closed(&self) -> bool {
        false
    }

    /// The page alone has no update section; [`ShownPage`] adds it (TXT-018, T729).
    pub(crate) const fn updates(&self) -> Option<&Updates> {
        None
    }

    /// The page alone has no address error (A11Y-030, T729).
    pub(crate) const fn update_error(&self) -> bool {
        false
    }
}

/// One package radio on a tiers quote's card (PICK-053, D-2332): the value the pick flow reads
/// back and the label, "<name>, $<price>".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Package {
    /// The radio's value ([`tier_value`]).
    pub(crate) value: String,
    /// The radio's label.
    pub(crate) label: String,
}

/// The packages, in order, of each tiers quote (by its number) not withdrawn among `quotes` of
/// `job`, read from `quotes::api::quoted_prices` (PICK-053, D-2332).
///
/// # Errors
///
/// `quoted_prices`' [`QuotesError`].
pub(crate) async fn quote_packages(
    db: &mut sqlx::PgConnection,
    job: sqlx::types::Uuid,
    quotes: &[Numbered],
) -> Result<Vec<(usize, Vec<(String, u64)>)>, QuotesError> {
    let prices = quoted_prices(db, job).await?;
    Ok(quotes
        .iter()
        .filter_map(|q| {
            let price = prices
                .iter()
                .find(|p| p.business == q.quote.business && !p.withdrawn)?;
            match &price.price {
                Some(Price::Tiers(tiers)) => Some((q.number, tiers.clone())),
                _ => None,
            }
        })
        .collect())
}

/// The served page: [`PageView`] (its fields read through `Deref`) and the quote she accepted,
/// while the job's pick is `paid` or `free` (PICK-042, D-3078).
#[derive(Template)]
#[template(path = "quote_page.html")]
pub(crate) struct ShownPage {
    page: PageView,
    accepted_quote: Option<Accepted>,
    /// Each tiers quote's packages by its number, read only while the job takes a pick.
    tiers: Vec<(usize, Vec<(String, u64)>)>,
    /// The quote whose card shows the "choose a package" error (`?choose=<n>`).
    again: Option<usize>,
    /// Each carded quote's number and card lines (PICK-041, T393).
    cards: Cards,
    /// The card lines are Spanish (`?lang=es`, D-3645).
    in_spanish: bool,
    /// The close form's action, `/q/<token>/close`, while the job is not final (JOB-019, T308).
    close_to: Option<String>,
    /// The page answers the close button's `POST` for a job now final (JOB-019, T308).
    closed: bool,
    /// The update choice while the job is live and has a homeowner (TXT-018, T729).
    update_section: Option<Updates>,
}

impl ShownPage {
    /// The accepted quote the template shows above the list, if any (PICK-042).
    const fn accepted(&self) -> Option<&Accepted> {
        self.accepted_quote.as_ref()
    }

    /// Quote `number`'s package radios, none for a quote without packages (PICK-053).
    fn packages(&self, number: &usize) -> Vec<Package> {
        let tiers = self.tiers.iter().find(|(n, _)| n == number);
        tiers.map_or_else(Vec::new, |(_, tiers)| {
            let radio = |(i, (name, cents)): (usize, &(String, u64))| Package {
                value: tier_value(i, name, *cents),
                label: format!("{name}, {}", money(*cents)),
            };
            tiers.iter().enumerate().map(radio).collect()
        })
    }

    /// Whether quote `number`'s card shows the "choose a package" error (PICK-053).
    fn choose_again(&self, number: &usize) -> bool {
        self.again == Some(*number)
    }

    /// The quote whose failed Pick puts "Error: " in the title and the focused error summary,
    /// linking to its first package, first in `<main>`; only a card with packages (A11Y-030,
    /// PICK-053, D-3629).
    fn choose_error(&self) -> Option<usize> {
        self.again.filter(|n| !self.packages(n).is_empty())
    }

    /// Quote `number`'s card lines, none for a quote with no card (PICK-041, D-3134).
    fn card(&self, number: &usize) -> Option<&[CardLine]> {
        let card = self.cards.iter().find(|(n, _)| n == number);
        card.map(|(_, lines)| lines.as_slice())
    }

    /// Whether the card lines are Spanish, so each card's line list carries `lang="es"` and its
    /// English lines `lang="en"` (A11Y-004, D-3645, D-3646).
    const fn spanish(&self) -> bool {
        self.in_spanish
    }

    /// The close form's action while the job is not final (JOB-018, JOB-019).
    fn close_action(&self) -> Option<&str> {
        self.close_to.as_deref()
    }

    /// Whether the page says the request is closed: only the answer to the close button (JOB-019).
    const fn just_closed(&self) -> bool {
        self.closed
    }

    /// The update section, while the job is live (TXT-018, T729).
    const fn updates(&self) -> Option<&Updates> {
        self.update_section.as_ref()
    }

    /// A malformed address puts "Error: " in the title and the focused summary first in `<main>`
    /// (A11Y-030, T729).
    fn update_error(&self) -> bool {
        self.update_section.as_ref().is_some_and(Updates::error)
    }
}

/// What the request asks beyond the token: the CSRF token, `choose=<n>` (PICK-053), the card
/// language (`lang=es`, D-3645) and what the update section says (`?confirm=`, T729).
pub(crate) struct Asked {
    csrf: String,
    again: Option<usize>,
    language: CardLanguage,
    pub(crate) said: Said,
}

impl Asked {
    /// The page with `csrf` and nothing else asked.
    pub(crate) fn plain(csrf: String) -> Self {
        Self {
            csrf,
            again: None,
            language: CardLanguage::English,
            said: Said::default(),
        }
    }
}

/// D-3645: the page's query, `choose=<n>` and `lang=es` in any order; anything else is ignored.
fn page_query(query: Option<&str>) -> (Option<usize>, CardLanguage) {
    let pairs = query.unwrap_or_default().split('&');
    let mut asked = (None, CardLanguage::English);
    for pair in pairs {
        if let Some(n) = pair.strip_prefix("choose=") {
            asked.0 = n.parse().ok();
        } else if pair == "lang=es" {
            asked.1 = CardLanguage::Spanish;
        }
    }
    asked
}

impl std::ops::Deref for ShownPage {
    type Target = PageView;

    fn deref(&self) -> &PageView {
        &self.page
    }
}

/// Why the page could not be built; logged once, with no value (rust-production R5).
#[derive(Debug, thiserror::Error)]
pub(crate) enum PageError {
    #[error("a quote-page read failed")]
    Read,
    #[error("the quote page's close failed")]
    Close,
    #[error("the quote page's update choice failed")]
    Update,
}

/// The quote page's router over `pool`, timed by `clock`, with no update keys:
/// [`quote_page_routes_with`]`(pool, clock, None)`.
pub fn quote_page_routes(pool: PgPool, clock: Arc<dyn Clock>) -> Router {
    quote_page_routes_with(pool, clock, None)
}

/// The quote page's router over `pool`, timed by `clock` (PICK-008 to PICK-012, D-491): `GET
/// /q/{token}`, the close button's `POST` [`CLOSE_ROUTE`] (JOB-019, SEC-042) and the update
/// choice's `POST`s [`UPDATES_ROUTE`] and [`UPDATES_CONFIRM_ROUTE`] (TXT-018, T729) behind the
/// cross-site refusal (TD-PICK-05), `no-store` and `noindex` (PICK-010). `keys` (the vault and
/// the ID secret) let the update choice store an address and confirm it; with `None` it stores
/// nothing (D-5442). `server::build_router` merges it.
pub fn quote_page_routes_with(
    pool: PgPool,
    clock: Arc<dyn Clock>,
    keys: Option<(Vault, IdSecret)>,
) -> Router {
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let keys = keys.map(|(vault, secret)| Arc::new(UpdateKeys { vault, secret }));
    let state = QuotePage {
        pool,
        clock,
        security: security.clone(),
        keys,
    };
    Router::new()
        .route(QUOTE_PAGE_ROUTE, get(show))
        .route(CLOSE_ROUTE, post(close))
        .route(UPDATES_ROUTE, post(post_updates))
        .route(UPDATES_CONFIRM_ROUTE, post(post_confirm))
        .with_state(state)
        .layer(from_fn(noindex))
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// PICK-010: `X-Robots-Tag: noindex, nofollow` on every quote-page response; the feedback and
/// check-in pages (private too) use this one copy (T517).
pub(crate) async fn noindex(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let value = HeaderValue::from_static("noindex, nofollow");
    response
        .headers_mut()
        .insert(HeaderName::from_static("x-robots-tag"), value);
    response
}

/// `GET /q/{token}`: the page, or a bare 404 for a malformed, unknown, revoked or expired token
/// (SEC-037, PICK-011). Changes nothing but the pre-session cookie its Pick forms need (SEC-042).
async fn show(
    State(page): State<QuotePage>,
    Path(raw): Path<String>,
    RawQuery(query): RawQuery,
    jar: CookieJar,
) -> Response {
    let Ok(token) = LinkToken::parse(&raw) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(jar) = ensure_pre_session(jar) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let csrf = csrf_token(&jar).unwrap_or_default();
    let (again, language) = page_query(query.as_deref());
    let said = Said {
        confirm: confirm_query(query.as_deref()),
        ..Said::default()
    };
    let asked = Asked {
        csrf,
        again,
        language,
        said,
    };
    respond(jar, load(&page, &token, asked).await)
}

/// `POST /q/{token}/close` (JOB-018, JOB-019, SEC-042, SEC-048): the CSRF token is checked first
/// (`CsrfForm`, a refusal a 403 and a `csrf_rejected` event), then the token (a bare 404 when it
/// opens nothing); the job closes through `close_from_page`, which sends no text and leaves a job
/// already final alone, and the answer is the page itself, saying the request is closed.
async fn close(
    State(page): State<QuotePage>,
    Path(raw): Path<String>,
    jar: CookieJar,
    CsrfForm(_): CsrfForm<ButtonForm>,
) -> Response {
    let Ok(token) = LinkToken::parse(&raw) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let asked = Asked::plain(csrf_token(&jar).unwrap_or_default());
    let shown = match close_job(&page, &token).await {
        Ok(true) => load(&page, &token, asked).await,
        Ok(false) => Ok(None),
        Err(err) => Err(err),
    };
    let shown = shown.map(|view| {
        view.map(|view| ShownPage {
            closed: !view.live,
            ..view
        })
    });
    respond(jar, shown)
}

/// Closes the job `token` opens now, as its homeowner at the clock's time (JOB-019); `false` when
/// the token opens nothing (SEC-037, PICK-011).
async fn close_job(page: &QuotePage, token: &LinkToken) -> Result<bool, PageError> {
    let mut db = page.pool.acquire().await.map_err(|_| PageError::Read)?;
    let Some((job, _)) = open_job(&mut db, page, token).await? else {
        return Ok(false);
    };
    close_from_page(&mut db, job, page.clock.now())
        .await
        .map_err(|_| PageError::Close)?;
    Ok(true)
}

/// The job `token` opens at the clock's time, with its current status; `None` for an unknown,
/// revoked or expired token (SEC-037, PICK-011).
pub(crate) async fn open_job(
    db: &mut sqlx::PgConnection,
    page: &QuotePage,
    token: &LinkToken,
) -> Result<Option<(sqlx::types::Uuid, JobStatus)>, PageError> {
    let Some(job) = resolve(db, token).await.map_err(|_| PageError::Read)? else {
        return Ok(None);
    };
    let row = current(db, job).await.map_err(|_| PageError::Read)?;
    if token_expired(row.status.is_final(), row.status_at, page.clock.now()) {
        return Ok(None);
    }
    Ok(Some((job, row.status)))
}

/// The built page with the pre-session cookie, a bare 404 for `None`, a logged 500 for an error
/// (rust-production R5).
pub(crate) fn respond(jar: CookieJar, shown: Result<Option<ShownPage>, PageError>) -> Response {
    let view = match shown {
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
/// Pick buttons show while the job takes a pick (`JobStatus::takes_pick`: Quotes ready and
/// Awaiting confirm, where a new pick replaces the waiting one; PICK-012, PICK-032, D-1516); the
/// CON-028 line while a live job is paused by the homeowner; a tiers quote's packages and, for
/// quote `again`, the "choose a package" error with them (PICK-053); each shown quote's card in
/// the asked language (PICK-041, T393).
pub(crate) async fn load(
    page: &QuotePage,
    token: &LinkToken,
    asked: Asked,
) -> Result<Option<ShownPage>, PageError> {
    let mut db = page.pool.acquire().await.map_err(|_| PageError::Read)?;
    let Some((job, status)) = open_job(&mut db, page, token).await? else {
        return Ok(None);
    };
    let is_final = status.is_final();
    let request = job_number(&mut db, job)
        .await
        .map_err(|_| PageError::Read)?;
    let still_open = !is_final
        && paused_by_homeowner(&mut db, job)
            .await
            .map_err(|_| PageError::Read)?;
    let quotes = shown_page(&mut db, job)
        .await
        .map_err(|_| PageError::Read)?;
    let pick_open = status.takes_pick();
    let tiers = if pick_open {
        quote_packages(&mut db, job, &quotes)
            .await
            .map_err(|_| PageError::Read)?
    } else {
        Vec::new()
    };
    let cards = quote_cards(&mut db, job, &quotes, asked.language).await;
    let place = (page.keys.as_deref(), job, token, !is_final);
    let update_section = updates_view(&mut db, place, asked.said).await?;
    let view = PageView {
        request,
        still_open,
        live: !is_final,
        pick_open,
        csrf: asked.csrf,
        quotes,
    };
    let accepted_quote = accepted(&mut db, job).await?;
    Ok(Some(ShownPage {
        page: view,
        accepted_quote,
        tiers,
        again: asked.again,
        cards: cards.map_err(|_| PageError::Read)?,
        in_spanish: asked.language == CardLanguage::Spanish,
        close_to: (!is_final).then(|| format!("/q/{}/close", token.as_str())),
        closed: false,
        update_section,
    }))
}

/// PICK-042, D-3078: the quote of `job`'s `paid` or `free` pick, or none.
async fn accepted(
    db: &mut sqlx::PgConnection,
    job: sqlx::types::Uuid,
) -> Result<Option<Accepted>, PageError> {
    let Some(pick) = accepted_pick(db, job).await.map_err(|_| PageError::Read)? else {
        return Ok(None);
    };
    let quote = accepted_quote(db, (job, pick.business))
        .await
        .map_err(|_| PageError::Read)?;
    Ok(quote.map(Accepted::from))
}
