//! The quote page's pick review step and the pick result (PICK-016, PICK-017, PICK-045, PICK-054,
//! A11Y-014, A11Y-031; T055, D-1248 to D-1251).
//!
//! A quote card's Pick form posts to the quote page's own address (`POST /q/{token}`); the answer
//! is the review step (`templates/pick_review.html`): what happens next, "or reply <n> YES", a
//! "Go back" link to the card, and the final Pick form posting to [`PICK_ROUTE`] with only "By
//! picking, you agree to the Terms" directly beside its button (PICK-016) and no Confirm end time
//! (TD-PICK-28). The review lists the Verify code step (PICK-013, T187) the final Pick starts;
//! until it and the stored page pick exist, the final POST re-checks the link and the quote and
//! shows the review again under "Nothing was picked", so no page claims a pick that was not made
//! (D-1248, D-1251). A quote can be picked from the first quote (PICK-012) until the business
//! confirms: in Awaiting confirm a new page pick replaces the waiting one (PICK-032, PICK-045,
//! D-1251). [`result_page`] renders the page shown once a pick is stored: its heading takes focus
//! on load (A11Y-014), it shows the acceptance end time when the caller has it (PICK-045), the Ask
//! Hava contact card and the after-Done line (PICK-054). Both routes are POST only,
//! behind the cross-site refusal, read through [`CsrfForm`], `no-store` and `noindex`; neither
//! changes anything, so a double submit is harmless (A11Y-031). No name, phone or address is read.
//!
//! T187 adds the code-and-contact step (`templates/pick_code.html`, `POST` [`CODE_ROUTE`]): how the
//! business reaches her, a call through the masked number or an email she shares (PICK-018; the
//! accepted-quote copy goes to that email, PICK-042), and
//! one `autocomplete="one-time-code"` code field that takes paste (PICK-014, A11Y-013, A11Y-032).
//! Labels and hints come before each input (A11Y-010); a failed submit gets an "Error: " title, a
//! focused summary linking each field and kept values (A11Y-011, A11Y-012); the code's 10-minute
//! limit is said up front in plain words, with "Send a new code" in the same form keeping her
//! choices (A11Y-015, A11Y-016). Without [`PickCodeKeys`] (dev, `Ci` and the end-to-end runs) the
//! page says no code was sent and nothing is picked, word for word as before (D-1248, D-1264).
//!
//! T246 (D-1516) builds the page pick with the keys: the final Pick is the code step. It
//! re-checks the link and [`JobStatus::takes_pick`]; picking the quote that already waits shows
//! the result and sends no code; a job paused by the homeowner (CON-029) or a number
//! `messaging::api::send_window` says can't be texted gets no code and a line saying nothing was
//! picked; otherwise `picks::api::send_page_code` texts the code to her vaulted phone, which
//! `portals` never sees (PICK-013, PRIV-003). "Finish my pick" checks the fields, then the code
//! (`picks::api::check_page_code`; a malformed code counts no try), and on a passed code runs one
//! transaction: `picks::api::page_pick` and the terms acceptance with channel `quote_page`
//! (PICK-016, TERMS-016), then the result page (PICK-045). From Payment pending on the page shows
//! the confirmed line and stores nothing. The contact choice and email are checked and passed on
//! unstored (PICK-018, D-1264).

use std::sync::Arc;

use askama::Template;
use askhava_adapters::api::VerifyAdapter;
use askhava_core::api::{Clock, LinkToken};
use askhava_intake::api::{
    JobHomeowner, TermsAcceptance, TermsChannel, job_homeowner, paused_by_homeowner,
    paused_under_lock, record_terms_acceptance,
};
use askhava_jobs::api::quote_token::resolve;
use askhava_jobs::api::{JobStatus, current, job_number};
use askhava_messaging::api::{Language, SendWindow, send_window};
use askhava_ops::api::Actor;
use askhava_picks::api::rules::{CodeVia, SendAsk, SendRefused, Stage, parse_code, stage};
use askhava_picks::api::{
    CodeCheck, CodeSent, PageCodeKeys, PagePick, check_page_code, page_pick, send_page_code,
    waiting_quote,
};
use askhava_privacy::api::Vault;
use askhava_quotes::api::page_quotes;
use axum::Router;
use axum::extract::{FromRef, Path, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::post;
use axum_extra::extract::cookie::CookieJar;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::quote_page::{QUOTE_PAGE_ROUTE, number_quotes, token_expired};
use crate::security::{CsrfField, CsrfForm, WebSecurity, no_store, refuse_cross_site};
use crate::session::csrf_token;

/// The final Pick's route on the review step (PICK-016, PICK-017).
pub const PICK_ROUTE: &str = "/q/{token}/pick";

/// The code-and-contact step's route (PICK-013, PICK-018, T187).
pub const CODE_ROUTE: &str = "/q/{token}/code";

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
    /// `true` when the final Pick was pressed before the page pick exists: the review shows again
    /// under "Nothing was picked" (D-1251).
    pub(crate) not_open: bool,
    /// The result's acceptance end time, worded by the caller (PICK-045, T063's window).
    pub(crate) accept_by: Option<String>,
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
            not_open: false,
            accept_by: None,
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
/// steps and, when `accept_by` is given, "The business has until <accept_by> to accept." (PICK-045:
/// the portal shows the acceptance end time; T063's Confirm window supplies it, D-1251), the "Add
/// Ask Hava to your contacts" card and the after-Done line (PICK-054). The stored page pick (T187)
/// calls it. Only the review step hides the end time (TD-PICK-28).
///
/// # Errors
///
/// [`askama::Error`] when the template fails to render.
pub fn result_page(
    request: i64,
    quote: usize,
    token: String,
    accept_by: Option<String>,
) -> Result<String, askama::Error> {
    let mut view = ReviewView::review(request, quote, token, String::new());
    view.done = true;
    view.accept_by = accept_by;
    view.render()
}

/// The global daily Verify count at which page codes stop and the High alert goes (PICK-055,
/// D-1244); [`PickCodeKeys::new`] uses it until a setting names another (D-1603).
pub const VERIFY_DAILY_THRESHOLD: i64 = 500;

/// What the page pick needs beyond the pool (D-1516): the Verify client, the vault `picks` reads
/// the phone from, the `ID_SECRET` for the send's idempotency key, the published homeowner Terms
/// a pick accepts (version and exact text, TERMS-016) and the global daily Verify threshold.
/// `server` builds it as it builds the job-text keys (D-1302); no `Debug`, it holds keys.
pub struct PickCodeKeys<V> {
    verify: V,
    vault: Vault,
    secret: SecretString,
    terms: (String, &'static str),
    threshold: i64,
}

impl<V> PickCodeKeys<V> {
    /// The keys over `verify`, `vault` and `secret`, accepting the Terms `terms` (version, exact
    /// published text), with [`VERIFY_DAILY_THRESHOLD`].
    #[must_use]
    pub fn new(
        verify: V,
        vault: Vault,
        secret: SecretString,
        terms: (String, &'static str),
    ) -> Self {
        Self {
            verify,
            vault,
            secret,
            terms,
            threshold: VERIFY_DAILY_THRESHOLD,
        }
    }

    /// The same keys with another global daily Verify threshold (PICK-055).
    #[must_use]
    pub fn with_threshold(mut self, threshold: i64) -> Self {
        self.threshold = threshold;
        self
    }

    fn code_keys(&self) -> PageCodeKeys<'_, V> {
        PageCodeKeys {
            verify: &self.verify,
            vault: &self.vault,
            secret: &self.secret,
        }
    }
}

/// The routes' state: the pool, the clock the 30-day rule reads, the refusals' state and the
/// page pick's keys (`None` in dev, `Ci` and the end-to-end runs).
struct PickFlow<V> {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    keys: Option<Arc<PickCodeKeys<V>>>,
}

impl<V> Clone for PickFlow<V> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            clock: Arc::clone(&self.clock),
            security: self.security.clone(),
            keys: self.keys.clone(),
        }
    }
}

impl<V> FromRef<PickFlow<V>> for WebSecurity {
    fn from_ref(flow: &PickFlow<V>) -> Self {
        flow.security.clone()
    }
}

/// The pick flow's router over `pool`, timed by `clock` (PICK-013, PICK-016, PICK-017, D-1516):
/// `POST /q/{token}` (the review step), `POST` [`PICK_ROUTE`] and `POST` [`CODE_ROUTE`], behind
/// the cross-site refusal and `no-store`. With `keys` the final Pick sends the code and "Finish my
/// pick" stores the pick; without them nothing is sent or stored and the page says so.
/// `server::build_router` merges it beside the quote page's `GET`.
pub fn pick_flow_routes<V>(
    pool: PgPool,
    clock: Arc<dyn Clock>,
    keys: Option<PickCodeKeys<V>>,
) -> Router
where
    V: VerifyAdapter + Send + Sync + 'static,
{
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = PickFlow {
        pool,
        clock,
        security: security.clone(),
        keys: keys.map(Arc::new),
    };
    Router::new()
        .route(QUOTE_PAGE_ROUTE, post(review::<V>))
        .route(PICK_ROUTE, post(final_pick::<V>))
        .route(CODE_ROUTE, post(code_step::<V>))
        .with_state(state)
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// `POST /q/{token}`: the review step for a quote that can still be picked (PICK-017); a bad link
/// is a bare 404 (SEC-037), a quote that can't be picked goes back to the quote page.
async fn review<V>(
    State(flow): State<PickFlow<V>>,
    Path(raw): Path<String>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<PickForm>,
) -> Response {
    let request = match pickable(&flow, &raw, form.quote).await {
        Ok(Pickable::Open { request, .. }) => request,
        Ok(Pickable::Confirmed { .. } | Pickable::No) => return back(&raw, form.quote),
        Err(status) => return status.into_response(),
    };
    let csrf = csrf_token(&jar).unwrap_or_default();
    render(ReviewView::review(request, form.quote, raw, csrf))
}

/// `POST /q/{token}/pick`: the final Pick, the code step (PICK-013, D-1516). It re-checks the link
/// and the quote; without keys it shows the review again under "Nothing was picked" (D-1248,
/// D-1251); with them [`start_code`] decides; from Payment pending on it shows the confirmed line.
async fn final_pick<V>(
    State(flow): State<PickFlow<V>>,
    Path(raw): Path<String>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<PickForm>,
) -> Response
where
    V: VerifyAdapter + Send + Sync,
{
    let csrf = csrf_token(&jar).unwrap_or_default();
    let found = pickable(&flow, &raw, form.quote).await;
    let Some(keys) = flow.keys.clone() else {
        return match found {
            Ok(Pickable::Open { request, .. }) => {
                let mut view = ReviewView::review(request, form.quote, raw, csrf);
                view.not_open = true;
                render(view)
            }
            Ok(Pickable::Confirmed { .. } | Pickable::No) => back(&raw, form.quote),
            Err(status) => status.into_response(),
        };
    };
    let (job, request) = match found {
        Ok(Pickable::Open { job, request }) => (job, request),
        Ok(Pickable::Confirmed { request }) => {
            return render(CodeView::line_only(request, form.quote, raw, CONFIRMED));
        }
        Ok(Pickable::No) => return back(&raw, form.quote),
        Err(status) => return status.into_response(),
    };
    let view = CodeView::arrival(request, form.quote, raw, csrf);
    started(&flow, &keys, (job, form.quote), view).await
}

/// Starts the code for `quote` of `job` and renders what happened in `view`: the result page for
/// the quote already waiting, else the code step with its line.
async fn started<V>(
    flow: &PickFlow<V>,
    keys: &PickCodeKeys<V>,
    (job, quote): (Uuid, usize),
    mut view: CodeView,
) -> Response
where
    V: VerifyAdapter + Send + Sync,
{
    match start_code(flow, keys, job, quote).await {
        Ok(Start::Same) => done(view.request, quote, view.token),
        Ok(Start::Line(line, form)) => {
            view.line = Some(line);
            view.form = form;
            render(view)
        }
        Err(status) => status.into_response(),
    }
}

/// What the code step's start found.
enum Start {
    /// The quote is already the waiting pick: no code (TD-PICK-47).
    Same,
    /// The line to show, and whether the form shows under it.
    Line(PageLine, bool),
}

/// Sends the page code for `quote` of `job`, unless that quote already waits (no code), the
/// homeowner paused the job (CON-029) or her number can't be texted (`send_window`): then a
/// line saying nothing was picked and no form (PICK-013, PICK-055, D-1516). Failures are a bare
/// 500, logged once with no value.
async fn start_code<V>(
    flow: &PickFlow<V>,
    keys: &PickCodeKeys<V>,
    job: Uuid,
    quote: usize,
) -> Result<Start, StatusCode>
where
    V: VerifyAdapter + Send + Sync,
{
    let mut db = flow.pool.acquire().await.map_err(|_| failed("pool"))?;
    if waiting_quote(&mut db, job)
        .await
        .map_err(|_| failed("pick"))?
        == Some(quote)
    {
        return Ok(Start::Same);
    }
    let paused = paused_by_homeowner(&mut db, job)
        .await
        .map_err(|_| failed("pause"))?;
    let owner = job_homeowner(&mut db, job)
        .await
        .map_err(|_| failed("homeowner"))?;
    let Some(owner) = owner.filter(|_| !paused) else {
        return Ok(Start::Line(CANT_TEXT, false));
    };
    let now = flow.clock.now();
    let window = send_window(&mut db, &keys.vault, owner.contact, now)
        .await
        .map_err(|_| failed("window"))?;
    if matches!(window, SendWindow::OptedOut | SendWindow::NotTextable) {
        return Ok(Start::Line(CANT_TEXT, false));
    }
    let ask = SendAsk {
        via: CodeVia::Text,
        own_session: false,
        local_hour: 0,
    };
    let ids = (job, owner.contact);
    let when = (now, keys.threshold);
    let sent = send_page_code(&mut db, &keys.code_keys(), ids, ask, when).await;
    Ok(match sent {
        Ok(CodeSent::Sent) => Start::Line(CODE_SENT, true),
        Ok(CodeSent::Refused(SendRefused::CoolingDown(_))) => Start::Line(COOLING_DOWN, true),
        Ok(CodeSent::Refused(_)) => Start::Line(CAPPED, true),
        Err(_) => {
            tracing::warn!("pick code send failed");
            Start::Line(NOT_SENT, true)
        }
    })
}

/// A bare 500 for a failed `step`, logged once with the step's name only (R5, PRIV-016).
fn failed(step: &'static str) -> StatusCode {
    tracing::warn!(step, "pick flow step failed");
    StatusCode::INTERNAL_SERVER_ERROR
}

/// The result page for quote `quote` of request `request` (PICK-045).
fn done(request: i64, quote: usize, token: String) -> Response {
    match result_page(request, quote, token, None) {
        Ok(html) => page(html),
        Err(_) => failed("result").into_response(),
    }
}

/// `view` as a `noindex` page, or a bare 500 (logged once) when the template fails.
fn render(view: impl Template) -> Response {
    match view.render() {
        Ok(html) => page(html),
        Err(_) => {
            tracing::warn!("pick review template failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// True while the job takes a page pick: [`JobStatus::takes_pick`], Quotes ready and Awaiting
/// confirm, where a new pick replaces the waiting one until the business confirms (PICK-012,
/// PICK-032, PICK-045, D-1516).
pub(crate) const fn takes_pick(status: JobStatus) -> bool {
    status.takes_pick()
}

/// What a quote behind a token is to the pick flow.
enum Pickable {
    /// The job takes a pick and the quote is on the page and available.
    Open {
        /// The job.
        job: Uuid,
        /// Its request number.
        request: i64,
    },
    /// The business already confirmed a pick (Payment pending on, before a final status).
    Confirmed {
        /// The request number.
        request: i64,
    },
    /// Nothing to pick: back to the card.
    No,
}

/// Whether quote `quote` behind token `raw` can be picked now: the link opens (SEC-037,
/// PICK-011), the job takes a pick ([`takes_pick`]: PICK-012, PICK-032) and the quote is on the
/// page and available (PICK-008); a confirmed job is [`Pickable::Confirmed`]. `Err(404)` for a
/// bad link, `Err(500)` on a failed read (logged once, with no value).
async fn pickable<V>(flow: &PickFlow<V>, raw: &str, quote: usize) -> Result<Pickable, StatusCode> {
    let token = LinkToken::parse(raw).map_err(|_| StatusCode::NOT_FOUND)?;
    let mut db = flow.pool.acquire().await.map_err(|_| failed("pool"))?;
    let job = resolve(&mut db, &token)
        .await
        .map_err(|_| failed("token"))?;
    let job = job.ok_or(StatusCode::NOT_FOUND)?;
    let row = current(&mut db, job).await.map_err(|_| failed("job"))?;
    if token_expired(row.status.is_final(), row.status_at, flow.clock.now()) {
        return Err(StatusCode::NOT_FOUND);
    }
    if matches!(stage(row.status), Stage::Accepted) {
        let request = job_number(&mut db, job)
            .await
            .map_err(|_| failed("number"))?;
        return Ok(Pickable::Confirmed { request });
    }
    if !takes_pick(row.status) {
        return Ok(Pickable::No);
    }
    let quotes = page_quotes(&mut db, job)
        .await
        .map_err(|_| failed("quotes"))?;
    let open = number_quotes(quotes)
        .iter()
        .any(|q| q.number == quote && !q.quote.unavailable);
    if !open {
        return Ok(Pickable::No);
    }
    let request = job_number(&mut db, job)
        .await
        .map_err(|_| failed("number"))?;
    Ok(Pickable::Open { job, request })
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

/// The posted code-and-contact form. No `Debug`: it carries her email (PRIV-016).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CodeForm {
    #[serde(default)]
    csrf: String,
    quote: usize,
    #[serde(default)]
    code: String,
    #[serde(default)]
    reach: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    send: String,
}

impl CsrfField for CodeForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// A line at the top of the code step: its element ID and its words (drafts for the copy panel,
/// D-1604).
type PageLine = (&'static str, &'static str);

/// Without keys: today's words, unchanged (D-1264).
const NOT_OPEN: PageLine = (
    "pick-not-open",
    "Nothing was picked, and no code was sent. Picking on this page isn't open yet.",
);
/// The code went (PICK-013).
const CODE_SENT: PageLine = (
    "code-sent",
    "We texted a 6-digit code to your phone. Type it below to finish your pick.",
);
/// A paused job or a number that can't be texted (CON-029, QH-003).
const CANT_TEXT: PageLine = (
    "pick-not-open",
    "We can't text your phone right now, so no code was sent and nothing was picked.",
);
/// The business already accepted (PICK-032).
const CONFIRMED: PageLine = (
    "pick-not-open",
    "You already have a pick, and that business has accepted it. Your pick stays as it is.",
);
/// Five wrong tries: the cooldown (D-015).
const COOLING_DOWN: PageLine = (
    "pick-not-open",
    "That code didn't match 5 times, so new codes are paused for 15 minutes. After that, press \"Send a new code\".",
);
/// A daily cap (PICK-055).
const CAPPED: PageLine = (
    "pick-not-open",
    "We've sent as many codes as we can today. Please try again tomorrow.",
);
/// Verify failed.
const NOT_SENT: PageLine = (
    "pick-not-open",
    "We couldn't send the code. Press \"Send a new code\" to try again.",
);
/// The pick was taken another way while she typed the code.
const TAKEN: PageLine = (
    "pick-not-open",
    "This quote can't be picked right now, so your pick didn't go through. Go back to your quotes to see where things stand.",
);

/// The code-and-contact step (`templates/pick_code.html`): what she entered, kept, and the
/// message for each field that failed (A11Y-011, A11Y-012).
#[derive(Template)]
#[template(path = "pick_code.html")]
struct CodeView {
    /// The request (job) number.
    request: i64,
    /// The quote's number on the page.
    quote: usize,
    /// The quote page's token, for the form's address and "Go back".
    token: String,
    /// The form's CSRF token (SEC-042).
    csrf: String,
    /// The code as typed or pasted, kept after an error.
    code: String,
    /// `call` or `email` when chosen (PICK-018).
    reach: String,
    /// The email she typed, kept after an error.
    email: String,
    /// The code field's message.
    code_error: Option<String>,
    /// The contact choice's message.
    reach_error: Option<&'static str>,
    /// The email field's message.
    email_error: Option<&'static str>,
    /// `true` when any field failed: the title starts "Error: " and the summary shows.
    failed: bool,
    /// The line at the top when no field failed.
    line: Option<PageLine>,
    /// Whether the form shows (not after a line saying no code can come).
    form: bool,
}

impl CodeView {
    /// The step on arrival from the final Pick: no check, no line yet.
    fn arrival(request: i64, quote: usize, token: String, csrf: String) -> Self {
        let form = CodeForm {
            csrf: String::new(),
            quote,
            code: String::new(),
            reach: String::new(),
            email: String::new(),
            send: String::new(),
        };
        Self::checked(request, token, csrf, form)
    }

    /// Only `line`, no form: nothing can be picked here now.
    fn line_only(request: i64, quote: usize, token: String, line: PageLine) -> Self {
        let mut view = Self::arrival(request, quote, token, String::new());
        view.line = Some(line);
        view.form = false;
        view
    }

    /// The step for `form` (PICK-013, PICK-018, A11Y-010 to A11Y-012, A11Y-015): on arrival no
    /// check; "Send a new code" clears only the code; "Finish my pick" checks every field.
    fn checked(request: i64, token: String, csrf: String, form: CodeForm) -> Self {
        let mut view = Self {
            request,
            quote: form.quote,
            token,
            csrf,
            code: form.code,
            reach: form.reach,
            email: form.email,
            code_error: None,
            reach_error: None,
            email_error: None,
            failed: false,
            line: None,
            form: true,
        };
        match form.send.as_str() {
            "new" => view.code.clear(),
            "pick" => {
                view.code_error = code_problem(&view.code).map(str::to_owned);
                view.reach_error = reach_problem(&view.reach);
                view.email_error = (view.reach == "email")
                    .then(|| email_problem(&view.email))
                    .flatten();
                view.failed = view.code_error.is_some()
                    || view.reach_error.is_some()
                    || view.email_error.is_some();
            }
            _ => {}
        }
        view
    }

    /// The code field failed with `message` after Verify's answer (A11Y-011).
    fn code_failed(mut self, message: String) -> Self {
        self.code_error = Some(message);
        self.failed = true;
        self
    }
}

/// The code field's message for a code that is not six digits (PICK-014).
const MALFORMED_CODE: &str = "The code must be 6 numbers, like 123456.";

/// The code field's message, if any (PICK-014: spaces and dashes are allowed, as when pasted).
fn code_problem(code: &str) -> Option<&'static str> {
    if code.trim().is_empty() {
        Some("Enter the 6-digit code from our text.")
    } else if parse_code(code).is_none() {
        Some(MALFORMED_CODE)
    } else {
        None
    }
}

/// The contact choice's message, if any (PICK-018: a masked call or a shared email).
fn reach_problem(reach: &str) -> Option<&'static str> {
    (!matches!(reach, "call" | "email")).then_some("Choose how the business should reach you.")
}

/// The email field's message when email was chosen: one `@`, a dotted domain, no spaces.
fn email_problem(email: &str) -> Option<&'static str> {
    let email = email.trim();
    let shaped = email.len() <= 254
        && !email.contains(char::is_whitespace)
        && email.split_once('@').is_some_and(|(name, domain)| {
            !name.is_empty()
                && !domain.contains('@')
                && domain.split('.').all(|p| !p.is_empty())
                && domain.contains('.')
        });
    (!shaped).then_some("Enter your email address, like name@example.com.")
}

/// The code field's message for Verify's answer (PICK-013, D-015), `None` for a passed code.
fn check_message(check: CodeCheck) -> Option<String> {
    match check {
        CodeCheck::Accepted => None,
        CodeCheck::Wrong { left: 1 } => {
            Some("That code didn't match. You have 1 try left.".to_owned())
        }
        CodeCheck::Wrong { left } => Some(format!(
            "That code didn't match. You have {left} tries left."
        )),
        CodeCheck::Expired => {
            Some("That code has expired. Codes work for 10 minutes. Press \"Send a new code\" to get another one.".to_owned())
        }
        CodeCheck::CoolingDown(_) => Some(COOLING_DOWN.1.to_owned()),
        CodeCheck::Malformed => Some(MALFORMED_CODE.to_owned()),
        CodeCheck::NoCode => {
            Some("We haven't sent a code for this pick yet. Press \"Send a new code\" to get one.".to_owned())
        }
    }
}

/// `POST /q/{token}/code`: the code-and-contact step for a quote that can still be picked
/// (PICK-013, PICK-018); a bad link is a bare 404, a quote that can't be picked goes back to its
/// card. Without keys it sends and stores nothing and says so (D-1264); with them "Send a new
/// code" sends one and "Finish my pick" checks the fields, then the code, then stores the pick.
async fn code_step<V>(
    State(flow): State<PickFlow<V>>,
    Path(raw): Path<String>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<CodeForm>,
) -> Response
where
    V: VerifyAdapter + Send + Sync,
{
    let csrf = csrf_token(&jar).unwrap_or_default();
    let quote = form.quote;
    let (job, request) = match (pickable(&flow, &raw, quote).await, flow.keys.is_some()) {
        (Ok(Pickable::Open { job, request }), _) => (job, request),
        (Ok(Pickable::Confirmed { request }), true) => {
            return render(CodeView::line_only(request, quote, raw, CONFIRMED));
        }
        (Ok(_), _) => return back(&raw, quote),
        (Err(status), _) => return status.into_response(),
    };
    let send = form.send.clone();
    let mut view = CodeView::checked(request, raw, csrf, form);
    let Some(keys) = flow.keys.clone() else {
        if !view.failed {
            view.line = Some(NOT_OPEN);
        }
        return render(view);
    };
    match send.as_str() {
        "new" => started(&flow, &keys, (job, quote), view).await,
        "pick" if !view.failed => finish(&flow, &keys, job, view).await,
        _ => render(view),
    }
}

/// "Finish my pick" with every field shaped: checks the code against her vaulted phone, then
/// stores the pick and the terms acceptance in one transaction (PICK-013, PICK-016, TERMS-016,
/// D-1516). The contact choice and email are not stored (PICK-018, D-1264).
async fn finish<V>(
    flow: &PickFlow<V>,
    keys: &PickCodeKeys<V>,
    job: Uuid,
    view: CodeView,
) -> Response
where
    V: VerifyAdapter + Send + Sync,
{
    let Ok(mut db) = flow.pool.acquire().await else {
        return failed("pool").into_response();
    };
    let owner = match job_homeowner(&mut db, job).await {
        Ok(Some(owner)) => owner,
        Ok(None) => {
            return render(CodeView::line_only(
                view.request,
                view.quote,
                view.token,
                CANT_TEXT,
            ));
        }
        Err(_) => return failed("homeowner").into_response(),
    };
    let now = flow.clock.now();
    let ids = (job, owner.contact);
    let check = check_page_code(&mut db, &keys.code_keys(), ids, &view.code, now).await;
    match check.map(check_message) {
        Ok(None) => {}
        Ok(Some(message)) => return render(view.code_failed(message)),
        Err(_) => return failed("check").into_response(),
    }
    let (request, quote) = (view.request, view.quote);
    match store(flow, keys, (job, quote), owner).await {
        Ok(true) => done(request, quote, view.token),
        Ok(false) => render(CodeView::line_only(request, quote, view.token, TAKEN)),
        Err(status) => status.into_response(),
    }
}

/// One transaction under the homeowner lock (the text pick's, so a CANCEL committing meanwhile is
/// seen: CON-029, D-1602): the pause re-checked, the page pick, then the terms acceptance with
/// channel `quote_page`, the line shown beside the final Pick and the published Terms (PICK-016,
/// TERMS-016, D-1516). `false` when nothing could be picked (rolled back: the job moved, was
/// paused or the quote went); a failed acceptance insert rolls the pick back too.
async fn store<V>(
    flow: &PickFlow<V>,
    keys: &PickCodeKeys<V>,
    (job, quote): (Uuid, usize),
    owner: JobHomeowner,
) -> Result<bool, StatusCode> {
    let now = flow.clock.now();
    let mut tx = flow.pool.begin().await.map_err(|_| failed("begin"))?;
    let paused = paused_under_lock(&mut tx, owner.conversation, job)
        .await
        .map_err(|_| failed("pause"))?;
    let actor = Actor::Homeowner(owner.contact);
    let pick = match page_pick(&mut tx, (job, paused), quote, actor, now).await {
        Ok(PagePick::Picked(pick)) => pick,
        Ok(PagePick::Same(_)) => return Ok(true),
        Ok(PagePick::Taken) | Err(askhava_picks::api::PicksError::MovedOn) => return Ok(false),
        Err(_) => return Err(failed("pick")),
    };
    // The review step is shown in English only until the page reads the request's language
    // (D-1249), so the acceptance stores the English line it showed, not `owner.language`.
    let (_, lead, link) = PageLanguage::En.terms();
    let assent = format!("{lead} {link}");
    let acceptance = TermsAcceptance {
        conversation: owner.conversation,
        pick,
        document: "homeowner_terms",
        version: &keys.terms.0,
        published_text: keys.terms.1,
        language: Language::En,
        channel: TermsChannel::QuotePage,
        assent_line: &assent,
        accepted_at: now,
    };
    record_terms_acceptance(&mut tx, &acceptance)
        .await
        .map_err(|_| failed("terms"))?;
    tx.commit().await.map_err(|_| failed("commit"))?;
    Ok(true)
}

#[cfg(test)]
mod code_tests {
    //! The code step's checks (PICK-014, PICK-018, A11Y-011, A11Y-012). Fake values only.

    use super::{code_problem, email_problem, reach_problem};

    #[test]
    fn pick_014_pasted_code_with_spaces_or_dashes_passes() {
        assert_eq!(code_problem("123 456"), None);
        assert_eq!(code_problem("123-456"), None);
        assert!(code_problem("").is_some());
        assert!(code_problem("12345").is_some());
    }

    #[test]
    fn pick_018_only_call_or_email_and_a_shaped_email() {
        assert_eq!(reach_problem("call"), None);
        assert_eq!(reach_problem("email"), None);
        assert!(reach_problem("text").is_some());
        assert_eq!(email_problem("sample@example.com"), None);
        for bad in [
            "",
            "sample",
            "a@b",
            "a b@example.com",
            "a@@example.com",
            "@example.com",
        ] {
            assert!(email_problem(bad).is_some(), "{bad}");
        }
    }
}
