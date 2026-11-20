//! The email action links, served (BIZ-038, SEC-025, SEC-026, SEC-034, SEC-042, SEC-048, PICK-040,
//! PICK-007, BIZ-049, SEC-014; T301, D-1955, D-2128, D-2206, D-2710, D-3134, D-3189, D-3675).
//!
//! `GET` and `POST` on `portals::api::ACTION_ROUTE` (`/b/a/{kind}/{object}/{token}`) for every
//! `ActionKind`. A `GET` only reads: `open_action_link` checks the token, then the page shows
//! (`action_page`, or for a Quote link the priced quote form with the job's rules, its sent card's
//! counts and the business's last facts). Where the page needs the link's person (a Quote link,
//! or a session kind opened while a business is signed in), `use_action_link` names them inside
//! a transaction this module always rolls back, so opening a link never uses it up and changes
//! nothing however often it is opened (SEC-026, D-3669). A session kind opened by another signed-in
//! person is the one not-found answer (SEC-025, D-3614).
//!
//! The Quote form's `POST` acts by the link alone (D-3675): after `refuse_cross_site` (a layer) and the CSRF
//! check (`CsrfForm`, a refusal is a `csrf_rejected` security event), it uses the link in one
//! transaction, calls `admit_quote` first (D-2206), reads the rules (`quote_rules`), the sent
//! card (`sent_card`, D-3189) and the job page, parses the price, facts and note, warns once
//! above the business's largest job (`largest_job_warning`, D-2710), checks the quote with
//! `parse_quote` and stores it with `submit_quote_with_facts`; any problem rolls back and shows
//! the form again, so the link stays unused; the form shown again with a parsed price carries
//! its introduction fee by job size lines (PICK-039, D-3672). A used link's repeat `POST` is the
//! 404, an open gap (D-3673). Pause, Resume and Ask about a charge act once with a business
//! session (T455, D-3839): signed out is a 401 linking to sign-in before any token read; then
//! `use_action_link` in the action's transaction, the link's person must be the signed-in one and
//! `session_may` must allow the kind; Pause and Resume (object: the business) call `pause_self`
//! and `resume_self` (BIZ-028, BIZ-010), Ask about a charge (object: the charged pick) reads
//! `receipt_for`, enqueues `ask_about_charge` and answers with `charge_answer` and the
//! `/b/charges/{pick}` link (BIZ-032, D-225). A refused pause or resume rolls back, so the link
//! stays unused, and says why (409). Approve (object: the phone line; T399, D-3081, D-3744)
//! acts only with the link's own owner's session and a step-up in the last 5 minutes, checked
//! before its transaction (else the sign-in or step-up answer, link unused), and approves the
//! phone with `approve_phone` only when the link was issued at or after the phone's current
//! `number_set_at`; an older link or a refused approval rolls back (409, link unused), another
//! business's or a declined phone is not found (BIZ-033, SEC-038). Hold (object: the waiting
//! pick; T456, PICK-043, D-3890) acts by the link alone: the link is checked open, the pick's
//! facts read (`picks::api::hold_ask`; a pick that does not wait is the one not-found answer, so a
//! wrong-kind link stays unused), `use_action_link` gets them, and `hold_pick` adds the HOLD's
//! business hours once in the same transaction; the page names the new end in Arizona time. A
//! refused Hold (emergency, held before, Away-delayed, window not running) rolls back, so the link
//! stays unused, and says why (409), but only after the link's person was proved by a second,
//! rolled-back `use_action_link`. Away (object: the business; T543, BIZ-034, D-285, D-3890) shows
//! one hours field (`away_page`) and acts with the link's own person's business session, like
//! Pause: the hours, 1 to the setting's limit, are bounded with `picks::api::away_for` and
//! stored with `set_away`; a refusal rolls back (link unused) and shows the page again with its
//! reason; the answer is "Away until <time>" in Arizona time. Every other kind's `POST` gets the one not-found
//! answer before any token read (D-3675). A wrong kind, object, person, an ended, used or
//! unknown token, a removed member's token and a job with no sent card or no rules all get the
//! same bare 404. Every response is `no-store`. After a stored quote the job page carries
//! "Not a fit for us" (`job_details_page_choice`, PICK-057), posting to `NOT_A_FIT_ROUTE`, which
//! `business_portal` serves behind the business session (T306, D-4551).

use std::net::IpAddr;
use std::sync::Arc;

use askhava_businesses::api::{
    Acting, BusinessesError, Roles, TeamError, approve_phone, capabilities, pause_self,
    resume_self, set_away, waiting_phones,
};
use askhava_core::api::{Clock, SystemClock, arizona_time};
use askhava_intake::api::Admission;
use askhava_jobs::api::{job_facts, job_number};
use askhava_payments::api::{JobCodes, JobFeeFacts, receipt_for, resolve_fee_facts};
use askhava_picks::api::{
    HoldAsk, PicksError, TimeRefusal, WindowRules, away_for, hold_allowed, hold_ask, hold_pick,
};
use askhava_portals::api::{
    ACTION_ROUTE, ActionFacts, ActionGrant, ActionKind, ActionLinkError, AwayForm,
    BUSINESS_STEP_UP_ROUTE, ChargeAsked, CookieJar, CsrfField, CsrfForm, FactsForm, LargestWarning,
    NotAFitChoice, PriceForm, QuoteFormFacts, SIGNIN_ROUTE, UseBy, WHY_CHARGED_ROUTE, WebSecurity,
    action_page, admit_quote, ask_about_charge, away_hours, away_page, business_principal,
    charge_answer, charge_reply_line, csrf_token, ensure_pre_session, fee_lines,
    job_details_page_choice, largest_job_warning, not_a_fit_action, open_action_link, parse_facts,
    prefill, quote_form_page_priced, refuse_cross_site, require_step_up, security_event_client,
    session_may, use_action_link,
};
use askhava_portals::api::{JobPageLinks, no_store};
use askhava_portals::api::{SensitiveChange, StepUpError};
use askhava_quotes::api::{
    Bounce, NoteProblem, PageParts, Price, PriceFields, PriceProblem, QuoteForm, QuoteRules,
    Submitted, check_note, job_page, last_facts, parse_price, parse_quote, quote_rules, sent_card,
    submit_quote_with_facts,
};
use axum::extract::{FromRef, Path, Request, State};
use axum::http::StatusCode;
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The served action routes' state: the pool and the clock.
#[derive(Clone)]
struct Links {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
}

impl FromRef<Links> for WebSecurity {
    fn from_ref(links: &Links) -> Self {
        links.security.clone()
    }
}

/// The request's client address for the quote limit (SEC-014, SEC-040).
#[derive(Clone, Copy)]
struct Client(IpAddr);

/// What a handler answers when it cannot go on.
enum Stop {
    /// The one not-found answer: a bare 404 (SEC-025, SEC-037).
    NotFound,
    /// A store failed: a bare 500, logged once here (rust-production R5).
    Failed,
}

impl IntoResponse for Stop {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND.into_response(),
            Self::Failed => {
                tracing::warn!("action link page failed");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

/// Any store error is [`Stop::Failed`].
fn failed<E>(_: E) -> Stop {
    Stop::Failed
}

/// A link error: an ended or refused link is not found, a store error failed.
fn link_stop(error: ActionLinkError) -> Stop {
    match error {
        ActionLinkError::Store(_) => Stop::Failed,
        ActionLinkError::NotAllowed | ActionLinkError::NoHold | ActionLinkError::Ended => {
            Stop::NotFound
        }
    }
}

/// BIZ-038, SEC-026, SEC-042: the action routes over `pool`, timed by the system clock.
pub fn routes(pool: PgPool) -> Router {
    routes_at(pool, Arc::new(SystemClock))
}

/// [`routes`] timed by `clock`: every `POST` past `refuse_cross_site`, every response `no-store`,
/// a refusal's 403 included (SEC-034).
pub fn routes_at(pool: PgPool, clock: Arc<dyn Clock>) -> Router {
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    Router::new()
        .route(ACTION_ROUTE, get(show).post(act))
        .with_state(Links {
            pool,
            clock,
            security: security.clone(),
        })
        .layer(from_fn(note_client))
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store))
}

/// Notes the client address the security events use (SEC-040), for `admit_quote`.
async fn note_client(mut request: Request, next: Next) -> Response {
    let ip = security_event_client(&request);
    request.extensions_mut().insert(Client(ip));
    next.run(request).await
}

/// The path's kind, object and token; an unknown kind or a bad object is not found.
fn target(
    (kind, object, token): (String, String, String),
) -> Result<(ActionKind, Uuid, String), Stop> {
    let kind = ActionKind::from_slug(&kind).ok_or(Stop::NotFound)?;
    let object = Uuid::parse_str(&object).map_err(|_| Stop::NotFound)?;
    Ok((kind, object, token))
}

/// Renders a page with the pre-session cookie set (SEC-042).
fn page(jar: CookieJar, html: Option<String>) -> Response {
    match html {
        Some(html) => (jar, Html(html)).into_response(),
        None => Stop::Failed.into_response(),
    }
}

/// `GET /b/a/{kind}/{object}/{token}` (SEC-026): shows the link's page and changes nothing.
async fn show(
    State(links): State<Links>,
    Path(raw): Path<(String, String, String)>,
    jar: CookieJar,
) -> Response {
    let Ok(jar) = ensure_pre_session(jar) else {
        return Stop::Failed.into_response();
    };
    let csrf = csrf_token(&jar).unwrap_or_default();
    match read_page(&links, raw, &jar, &csrf).await {
        Ok(html) => page(jar, html),
        Err(stop) => stop.into_response(),
    }
}

/// The page a link opens, read in a transaction that is always rolled back (D-3669).
async fn read_page(
    links: &Links,
    raw: (String, String, String),
    jar: &CookieJar,
    csrf: &str,
) -> Result<Option<String>, Stop> {
    let (kind, object, token) = target(raw)?;
    let now = links.clock.now();
    let mut tx = links.pool.begin().await.map_err(failed)?;
    open_action_link(&mut tx, (kind, object), &token, now)
        .await
        .map_err(link_stop)?;
    let html = if kind == ActionKind::Quote {
        let grant = use_action_link(&mut tx, (kind, object), &token, None, now).await;
        let business = grant.map_err(link_stop)?.business();
        let job = JobLink {
            job: object,
            business,
        };
        let form = read_form(&mut tx, job, false).await?;
        let fields = PriceFields::default();
        quote_form(&form, csrf, None, &price_form(form.rules, &fields, ""))
    } else {
        let signed_in = session_check(&mut tx, (kind, object), &token, jar, now).await?;
        plain_page(kind, signed_in, csrf)
    };
    tx.rollback().await.map_err(failed)?;
    Ok(html)
}

/// A non-Quote kind's page at first render: the Away page with its hours field (BIZ-034), any
/// other kind's [`action_page`].
fn plain_page(kind: ActionKind, signed_in: bool, csrf: &str) -> Option<String> {
    let facts = ActionFacts {
        kind,
        job_number: "",
        ends_at: None,
        signed_in,
        days: None,
        csrf,
    };
    match kind {
        ActionKind::Away => away_page(&facts, away_form(None, None)).ok(),
        _ => action_page(&facts).ok(),
    }
}

/// BIZ-034, D-285: the Away form under the setting's limit, in whole hours, with a refused
/// submit's kept `hours` and `refused` reason.
fn away_form<'a>(hours: Option<&'a str>, refused: Option<&'a str>) -> AwayForm<'a> {
    AwayForm {
        max_hours: WindowRules::default().away_max_minutes / 60,
        hours,
        refused,
    }
}

/// SEC-004, SEC-025, D-3614: whether a business is signed in; for a session kind opened while one
/// is, the link must be that person's own (named by `use_action_link` in the caller's
/// transaction, which the caller rolls back), else not found.
async fn session_check(
    tx: &mut PgConnection,
    (kind, object): (ActionKind, Uuid),
    token: &str,
    jar: &CookieJar,
    now: Timestamp,
) -> Result<bool, Stop> {
    let principal = business_principal(tx, jar, now).await.map_err(failed)?;
    let Some(principal) = principal else {
        return Ok(false);
    };
    if kind.use_by() == UseBy::Session {
        let grant = use_action_link(tx, (kind, object), token, None, now).await;
        if !grant.map_err(link_stop)?.same_person(&principal) {
            return Err(Stop::NotFound);
        }
    }
    Ok(true)
}

/// One Quote link's job and the business it was emailed to.
#[derive(Clone, Copy)]
struct JobLink {
    job: Uuid,
    business: Uuid,
}

/// What the quote form needs read: its rules and its facts (PICK-040, PICK-053, CARD-023).
struct ReadForm {
    rules: QuoteRules,
    facts: QuoteFormFacts,
    parts: PageParts,
    fees: Option<JobFeeFacts>,
}

/// CARD-023, PICK-053, D-3189: the form's rules, counts and pre-fill for `link`; not found when
/// the job has no rules for the business, no sent card or no page for it.
async fn read_form(
    tx: &mut PgConnection,
    link: JobLink,
    certified: bool,
) -> Result<ReadForm, Stop> {
    let JobLink { job, business } = link;
    let rules = quote_rules(tx, job, business, certified)
        .await
        .map_err(failed)?;
    let rules = rules.ok_or(Stop::NotFound)?;
    let card = sent_card(tx, job)
        .await
        .map_err(failed)?
        .ok_or(Stop::NotFound)?;
    let parts = PageParts {
        card: card.card,
        answers: Vec::new(),
        later: Vec::new(),
    };
    let shown = job_page(tx, (job, business), &parts)
        .await
        .map_err(failed)?;
    let shown = shown.ok_or(Stop::NotFound)?;
    let number = job_number(tx, job).await.map_err(failed)?;
    let (fees, job_type) = fee_facts(tx, job).await?;
    let last = match job_type {
        Some(t) => last_facts(tx, business, &t).await.map_err(failed)?,
        None => None,
    };
    let facts = QuoteFormFacts {
        job_number: number.to_string(),
        sent: shown.sent(),
        quoted: shown.quoted(),
        fee_lines: Vec::new(),
        values: prefill(last.as_ref()),
        same_as_last: last.is_some(),
        read_back: None,
        problems: Vec::new(),
    };
    Ok(ReadForm {
        rules,
        facts,
        parts,
        fees,
    })
}

/// FEE-004, FEE-045, D-2331, D-3672: the job's fee facts from its latest stored codes through
/// `payments::api::resolve_fee_facts` (none when unsettled), and its first job type.
async fn fee_facts(
    tx: &mut PgConnection,
    job: Uuid,
) -> Result<(Option<JobFeeFacts>, Option<String>), Stop> {
    let row = job_facts(tx, job).await.map_err(failed)?;
    let codes = row.as_ref().map(|r| JobCodes {
        category: &r.category,
        job_types: &r.job_types,
    });
    let fees = resolve_fee_facts(tx, codes).await.map_err(failed)?;
    Ok((fees, row.and_then(|r| r.job_types.into_iter().next())))
}

/// PICK-039, FEE-043, D-3672: the introduction fee by job size lines for a parsed `price` on a
/// job with settled fee facts; none when the facts are unsettled or the price can't carry a fee
/// yet. A diagnostic visit (or hourly with no hours) is priced as one; a monthly price has a period.
fn fee_lines_for(fees: Option<JobFeeFacts>, price: &Price) -> Vec<String> {
    let Some(fees) = fees else {
        return Vec::new();
    };
    let diagnostic_visit = matches!(
        price,
        Price::VisitFee(_) | Price::Hourly { hours: None, .. }
    );
    let has_period = matches!(price, Price::Monthly { .. });
    fee_lines(price, fees.period_job(diagnostic_visit, has_period)).unwrap_or_default()
}

/// The price part for `rules`' format with the posted `fields` and `note`.
fn price_form<'a>(rules: QuoteRules, fields: &'a PriceFields, note: &'a str) -> PriceForm<'a> {
    PriceForm {
        format: rules.format,
        fields,
        note,
        problem: None,
        note_problem: None,
    }
}

/// The priced quote form, with the largest-job `warning` when one is due (BIZ-049).
fn quote_form(
    form: &ReadForm,
    csrf: &str,
    warning: Option<&LargestWarning>,
    price: &PriceForm<'_>,
) -> Option<String> {
    quote_form_page_priced(&form.facts, csrf, (warning, None), price).ok()
}

/// The quote form's posted fields (PICK-040, PICK-052, PICK-053, BIZ-049); every field optional,
/// so a missing one is the form's own error, never a 4xx. No `Debug`: the note is her business's
/// own words (rust-production R6).
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct Posted {
    csrf: String,
    trip_fee: String,
    parts: String,
    permit: String,
    may_change: String,
    visit_date: String,
    visit_part: String,
    spanish: String,
    minutes: String,
    amount: String,
    low: String,
    high: String,
    on_site: String,
    per: String,
    schedule: String,
    setup: String,
    tier1_name: String,
    tier1_price: String,
    tier2_name: String,
    tier2_price: String,
    tier3_name: String,
    tier3_price: String,
    notes: String,
    over_largest: String,
    certified: String,
    hours: String,
}

impl CsrfField for Posted {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

impl Posted {
    /// The fact fields as `parse_facts` reads them.
    fn facts(&self) -> FactsForm {
        FactsForm {
            trip_fee: self.trip_fee.clone(),
            parts: self.parts.clone(),
            permit: self.permit.clone(),
            may_change: self.may_change.clone(),
            visit_date: self.visit_date.clone(),
            visit_part: self.visit_part.clone(),
            spanish: self.spanish.clone(),
            minutes: self.minutes.clone(),
        }
    }

    /// The price fields as `parse_price` reads them.
    fn price(&self) -> PriceFields {
        let tier = |name: &String, price: &String| (name.clone(), price.clone());
        PriceFields {
            amount: self.amount.clone(),
            low: self.low.clone(),
            high: self.high.clone(),
            on_site: self.on_site == "yes",
            per: self.per.clone(),
            schedule: self.schedule.clone(),
            setup: self.setup.clone(),
            tiers: [
                tier(&self.tier1_name, &self.tier1_price),
                tier(&self.tier2_name, &self.tier2_price),
                tier(&self.tier3_name, &self.tier3_price),
            ],
        }
    }
}

/// `POST /b/a/{kind}/{object}/{token}`: after the cross-site and CSRF checks, a Quote link acts
/// by the link alone, Pause, Resume and Ask about a charge with a business session (D-3839),
/// Approve with the owner's session and a fresh step-up (T399), Hold by the link alone (T456),
/// Away with a business session (T543); every other kind is the one not-found answer before any token read (D-3675).
async fn act(
    State(links): State<Links>,
    Extension(Client(ip)): Extension<Client>,
    Path(raw): Path<(String, String, String)>,
    jar: CookieJar,
    CsrfForm(posted): CsrfForm<Posted>,
) -> Response {
    let (kind, object, token) = match target(raw) {
        Ok(found) => found,
        Err(stop) => return stop.into_response(),
    };
    match kind {
        ActionKind::Quote => quote_post(&links, (object, &token), ip, &posted, jar).await,
        ActionKind::Pause | ActionKind::Resume | ActionKind::AskCharge => {
            let acted = session_post(&links, (kind, object, &token), &jar).await;
            acted.unwrap_or_else(IntoResponse::into_response)
        }
        ActionKind::Approve => {
            let acted = approve_post(&links, (object, &token), &jar).await;
            acted.unwrap_or_else(IntoResponse::into_response)
        }
        ActionKind::Hold => {
            let acted = hold_post(&links, (object, &token)).await;
            acted.unwrap_or_else(IntoResponse::into_response)
        }
        ActionKind::Away => {
            let acted = away_post(&links, (object, &token), &jar, &posted.hours).await;
            acted.unwrap_or_else(IntoResponse::into_response)
        }
        _ => Stop::NotFound.into_response(),
    }
}

/// The Quote form's `POST` (PICK-040, D-2206): the stored quote's page, the form again or the
/// refused quote's page.
async fn quote_post(
    links: &Links,
    (job, token): (Uuid, &str),
    ip: IpAddr,
    posted: &Posted,
    jar: CookieJar,
) -> Response {
    let csrf = csrf_token(&jar).unwrap_or_default();
    let sent = send_quote(links, (job, token), ip, posted, &csrf).await;
    match sent {
        Ok(Sent::Page(html)) => page(jar, html),
        Ok(Sent::TryLater) => (StatusCode::TOO_MANY_REQUESTS, Html(TRY_LATER)).into_response(),
        Err(stop) => stop.into_response(),
    }
}

/// What a session action came to: done (committed) or refused (rolled back), each with its
/// page's heading and body HTML.
enum Outcome {
    Done(&'static str, String),
    Refused(&'static str, &'static str),
}

/// BIZ-028, BIZ-032, SEC-004, SEC-025, SEC-026, D-3839: a session kind's `POST`. Signed out is
/// the 401 before any token read; the link is used in the action's transaction and must be the
/// signed-in person's own, for a kind `session_may` allows, else the one not-found answer; a
/// refusal rolls back, so the link stays unused and nothing changes.
async fn session_post(
    links: &Links,
    (kind, object, token): (ActionKind, Uuid, &str),
    jar: &CookieJar,
) -> Result<Response, Stop> {
    let now = links.clock.now();
    let mut tx = links.pool.begin().await.map_err(failed)?;
    let principal = business_principal(&mut tx, jar, now)
        .await
        .map_err(failed)?;
    let Some(principal) = principal else {
        return Ok(signed_out());
    };
    let grant = use_action_link(&mut tx, (kind, object), token, None, now).await;
    let grant = grant.map_err(link_stop)?;
    let business = grant.business();
    if !grant.same_person(&principal) || !session_may(&principal, kind, business) {
        return Err(Stop::NotFound);
    }
    let outcome = match kind {
        ActionKind::AskCharge => ask_charge(&mut tx, business, object, now).await?,
        _ if object != business => return Err(Stop::NotFound),
        ActionKind::Pause => paused(pause_self(&mut tx, business, now).await)?,
        _ => resumed(resume_self(&mut tx, business, now).await)?,
    };
    Ok(match outcome {
        Outcome::Done(title, body) => {
            tx.commit().await.map_err(failed)?;
            done(StatusCode::OK, title, &body)
        }
        Outcome::Refused(title, why) => {
            tx.rollback().await.map_err(failed)?;
            done(
                StatusCode::CONFLICT,
                title,
                &format!("<p id=\"why\">{why}</p>"),
            )
        }
    })
}

/// PICK-043, BIZ-038, SEC-025, SEC-026, D-1956, D-3890: the Hold link's `POST`, by the link
/// alone. An ended or unknown link, or a pick that no longer waits, is the one not-found answer
/// before anything is used; then `use_action_link` with the pick's facts, and `hold_pick` in the
/// same transaction, committed only when the window moved. Every refusal rolls back (link unused)
/// and answers [`not_held`]; one found before the link is used is told only to the link's own
/// current person ([`hold_person`]).
async fn hold_post(links: &Links, (pick, token): (Uuid, &str)) -> Result<Response, Stop> {
    let now = links.clock.now();
    let kind_pick = (ActionKind::Hold, pick);
    let mut tx = links.pool.begin().await.map_err(failed)?;
    open_action_link(&mut tx, kind_pick, token, now)
        .await
        .map_err(link_stop)?;
    let found = hold_ask(&mut tx, pick, Roles::BOTH).await.map_err(failed)?;
    let (business, ask) = found.ok_or(Stop::NotFound)?;
    if let Err(refusal) = hold_allowed(ask) {
        let person = hold_person(&mut tx, (pick, token), now).await;
        tx.rollback().await.map_err(failed)?;
        person?;
        return Ok(not_held(refusal));
    }
    let grant = use_action_link(&mut tx, kind_pick, token, Some(ask), now).await;
    if grant.map_err(link_stop)?.business() != business {
        return Err(Stop::NotFound);
    }
    let rules = WindowRules::default();
    let held = match hold_pick(&mut tx, pick, Roles::BOTH, now, rules).await {
        Ok(held) => held,
        Err(PicksError::MovedOn) => Err(TimeRefusal::Ended),
        Err(_) => return Err(Stop::Failed),
    };
    match held {
        Ok(due) => {
            tx.commit().await.map_err(failed)?;
            Ok(held_page(due))
        }
        Err(refusal) => {
            tx.rollback().await.map_err(failed)?;
            Ok(not_held(refusal))
        }
    }
}

/// SEC-025, D-1956: whether the Hold link `token` for `pick` would grant (its member still
/// current with the Hold role), asked with facts that pass `hold_allowed` so the token and role
/// alone decide; else the one not-found answer. The caller rolls back, so nothing is used.
async fn hold_person(
    tx: &mut PgConnection,
    (pick, token): (Uuid, &str),
    now: Timestamp,
) -> Result<(), Stop> {
    let open = HoldAsk {
        by: Roles::BOTH,
        emergency: false,
        held: false,
        away_delayed: false,
    };
    let grant = use_action_link(tx, (ActionKind::Hold, pick), token, Some(open), now).await;
    grant.map(drop).map_err(link_stop)
}

/// PICK-043, PICK-045: the held window's new end, in Arizona time; the homeowner gets no text.
fn held_page(due: Timestamp) -> Response {
    let end = arizona_time(due)
        .strftime("%-I:%M %p on %A, %B %-d")
        .to_string();
    let body = format!(
        "<p id=\"new-end\">You now have until {end}, Arizona time, to confirm this pick.</p>\
         <p>We don't text the homeowner about this.</p>"
    );
    done(StatusCode::OK, "Pick held", &body)
}

/// PICK-043, D-252, D-284, D-344: the window can't be extended, and why (409).
fn not_held(refusal: TimeRefusal) -> Response {
    let why = match refusal {
        TimeRefusal::Emergency => "This job is an emergency, so its window can't be extended.",
        TimeRefusal::AlreadyHeld => {
            "This pick was already held once, so its window can't be extended again."
        }
        TimeRefusal::AwayDelayed => {
            "This pick's window started when your away time ended, so it can't be extended."
        }
        TimeRefusal::Role | TimeRefusal::Ended | TimeRefusal::TooFar => {
            "This pick's window isn't running, so it can't be extended."
        }
    };
    let body = format!("<p id=\"why\">{why}</p>");
    done(StatusCode::CONFLICT, "Window not extended", &body)
}

/// BIZ-034, BIZ-038, SEC-004, SEC-025, SEC-026, D-285, D-3890: the Away link's `POST` (object:
/// the business). Signed out is the 401 before any token read; the link is used in the action's
/// transaction and must be the signed-in person's own, on their own business, for a person
/// `session_may` allows, else the one not-found answer (link unused). The hours are bounded with
/// `picks::api::away_for` (1 to the setting's limit) and stored with `set_away`; a refusal rolls
/// back, so the link stays unused, and shows the page again with its reason.
async fn away_post(
    links: &Links,
    (object, token): (Uuid, &str),
    jar: &CookieJar,
    hours: &str,
) -> Result<Response, Stop> {
    let now = links.clock.now();
    let mut tx = links.pool.begin().await.map_err(failed)?;
    let principal = business_principal(&mut tx, jar, now)
        .await
        .map_err(failed)?;
    let Some(principal) = principal else {
        return Ok(signed_out());
    };
    let grant = use_action_link(&mut tx, (ActionKind::Away, object), token, None, now).await;
    let grant = grant.map_err(link_stop)?;
    let business = grant.business();
    let may = session_may(&principal, ActionKind::Away, business);
    if !grant.same_person(&principal) || !may || object != business {
        return Err(Stop::NotFound);
    }
    let by = Acting {
        business,
        member: principal.member(),
    };
    let set = away_set(&mut tx, by, hours, now).await?;
    let csrf = csrf_token(jar).unwrap_or_default();
    match set {
        Ok(until) => {
            tx.commit().await.map_err(failed)?;
            Ok(away_done(until))
        }
        Err(refused) => {
            tx.rollback().await.map_err(failed)?;
            away_again(&csrf, hours, refused)
        }
    }
}

/// BIZ-034, D-285: bounds `hours` (a whole number from 1 to the setting's limit, read as the AWAY
/// text reads it, `away_for`) and stores the end with `set_away` in `tx`; the inner error is the
/// refusal's reason (`None`: the hours themselves, which the page's field error names). Hours in
/// range always pass `away_for` for a person `session_may` allowed, so its refusal is a failure.
async fn away_set(
    tx: &mut PgConnection,
    by: Acting,
    hours: &str,
    now: Timestamp,
) -> Result<Result<Timestamp, Option<&'static str>>, Stop> {
    let rules = WindowRules::default();
    let Some(hours) = away_hours(hours, away_form(None, None).max_hours) else {
        return Ok(Err(None));
    };
    let until = away_for(Roles::BOTH, now, hours, rules).map_err(failed)?;
    match set_away(tx, by, until, now).await {
        Ok(()) => Ok(Ok(until)),
        Err(BusinessesError::Refused) => Ok(Err(Some(AWAY_REFUSED))),
        Err(_) => Err(Stop::Failed),
    }
}

/// BIZ-034, BIZ-023: why a valid away time was not stored.
const AWAY_REFUSED: &str = "Only the owner or a \"Confirm and billing\" member can set an away time. Nothing changed. Ask one of them to set it.";

/// BIZ-034, D-285: the stored away time's answer, "Away until <time>" in Arizona time.
fn away_done(until: Timestamp) -> Response {
    let end = arizona_time(until)
        .strftime("%-I:%M %p on %A, %B %-d")
        .to_string();
    let body = format!(
        "<p id=\"away-until\">Away until {end}, Arizona time.</p>\
         <p>A new pick's time to confirm starts when you're back. Emergency picks still start right away.</p>"
    );
    done(StatusCode::OK, "Away time set", &body)
}

/// A11Y-030: the Away page again with the posted `hours` kept and the refusal's reason by the
/// field (422 for the hours, 409 for a refused person); nothing changed and the link stays unused.
fn away_again(csrf: &str, hours: &str, refused: Option<&str>) -> Result<Response, Stop> {
    let facts = ActionFacts {
        kind: ActionKind::Away,
        job_number: "",
        ends_at: None,
        signed_in: true,
        days: None,
        csrf,
    };
    let html = away_page(&facts, away_form(Some(hours), refused)).map_err(failed)?;
    let status = match refused {
        Some(_) => StatusCode::CONFLICT,
        None => StatusCode::UNPROCESSABLE_ENTITY,
    };
    Ok((status, Html(html)).into_response())
}

/// BIZ-028, BIZ-010: a self pause: done, or refused because the business is not active.
fn paused(moved: Result<impl Sized, BusinessesError>) -> Result<Outcome, Stop> {
    Ok(match moved {
        Ok(_) => Outcome::Done(
            "Job cards paused",
            "<p>We won't send you new job cards until you tap Resume.</p>".to_owned(),
        ),
        Err(BusinessesError::Refused | BusinessesError::MovedOn) => Outcome::Refused(
            "Job cards not paused",
            "Only an active account can pause its job cards. Nothing changed.",
        ),
        Err(_) => return Err(Stop::Failed),
    })
}

/// BIZ-028, BIZ-009, BIZ-010: a resume: done, or refused because the pause is not the business's
/// own.
fn resumed(moved: Result<impl Sized, BusinessesError>) -> Result<Outcome, Stop> {
    Ok(match moved {
        Ok(_) => Outcome::Done(
            "Job cards resumed",
            "<p>We'll send you new job cards again.</p>".to_owned(),
        ),
        Err(BusinessesError::Refused | BusinessesError::MovedOn) => Outcome::Refused(
            "Job cards not resumed",
            "Only a pause you set yourself can be resumed here. Nothing changed.",
        ),
        Err(_) => return Err(Stop::Failed),
    })
}

/// BIZ-032, FEE-031, OPS-036, D-225, D-348: the charge of `pick` for `business` (another
/// business's or no charge is the one not-found answer), its help request queued, and the
/// answer: its job number, amount and tier, the "Why was I charged?" link and the 2-day line.
async fn ask_charge(
    tx: &mut PgConnection,
    business: Uuid,
    pick: Uuid,
    now: Timestamp,
) -> Result<Outcome, Stop> {
    let receipt = receipt_for(tx, pick, business).await.map_err(failed)?;
    let receipt = receipt.ok_or(Stop::NotFound)?;
    let asked = ChargeAsked {
        business,
        charge: pick,
        job: receipt.job,
    };
    ask_about_charge(tx, asked, now).await.map_err(failed)?;
    let number = job_number(tx, receipt.job)
        .await
        .map_err(failed)?
        .to_string();
    let why = WHY_CHARGED_ROUTE.replace("{pick}", &pick.to_string());
    let answer = match receipt.tier {
        Some(tier) => charge_answer(&number, tier, &why),
        None => format!("Your charge for job {number}. {}", charge_reply_line()),
    };
    let body = format!(
        "<p id=\"charge-answer\">{}</p><p><a href=\"{why}\">Why was I charged?</a></p>",
        escaped(&answer)
    );
    Ok(Outcome::Done("We got your question", body))
}

/// `text` safe inside an HTML element.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A session action's page with `status`, heading `title` and `body` HTML.
fn done(status: StatusCode, title: &str, body: &str) -> Response {
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{title}</title>\
         </head><body><main><h1>{title}</h1>{body}</main></body></html>"
    );
    (status, Html(page)).into_response()
}

/// BIZ-033, BIZ-038, SEC-004, SEC-025, SEC-038, SEC-048, D-3081: the Approve link's `POST`.
/// Signed out is the sign-in answer; a person who may not approve is the one not-found answer;
/// without a step-up in the last 5 minutes (`require_step_up` on its own connection, so its
/// `authz_denied` event is kept) the step-up answer; then the link is used, must be the signed-in
/// owner's own, and [`approve`] decides in the same transaction, rolled back unless the phone
/// was approved, so every refusal leaves the link unused.
async fn approve_post(
    links: &Links,
    (line, token): (Uuid, &str),
    jar: &CookieJar,
) -> Result<Response, Stop> {
    let now = links.clock.now();
    let mut conn = links.pool.acquire().await.map_err(failed)?;
    let principal = business_principal(&mut conn, jar, now)
        .await
        .map_err(failed)?;
    let Some(principal) = principal else {
        return Ok(signed_out());
    };
    if !session_may(&principal, ActionKind::Approve, principal.business()) {
        return Err(Stop::NotFound);
    }
    let change = (SensitiveChange::ApprovePhone, ACTION_ROUTE);
    match require_step_up(&mut conn, &principal, change, now).await {
        Ok(()) => {}
        Err(StepUpError::NeedsCode) => return Ok(step_up_first()),
        Err(StepUpError::Database) => return Err(Stop::Failed),
    }
    drop(conn);
    let mut tx = links.pool.begin().await.map_err(failed)?;
    let kind_line = (ActionKind::Approve, line);
    let grant = use_action_link(&mut tx, kind_line, token, None, now).await;
    let grant = grant.map_err(link_stop)?;
    if !grant.same_person(&principal) {
        return Err(Stop::NotFound);
    }
    Ok(match approve(&mut tx, &grant, line, now).await? {
        Outcome::Done(title, body) => {
            tx.commit().await.map_err(failed)?;
            done(StatusCode::OK, title, &body)
        }
        Outcome::Refused(title, why) => {
            tx.rollback().await.map_err(failed)?;
            let body = format!("<p id=\"why\">{why}</p>");
            done(StatusCode::CONFLICT, title, &body)
        }
    })
}

/// BIZ-033, D-3081, D-3744: approves `line` as the number its owner was emailed about. A link
/// issued before the phone's current `number_set_at` approves nothing (a newer email is queued,
/// T398). A line not waiting for this business has no number to name, so `approve_phone` is
/// asked with the link's issue time and only says why (`NotWaiting` for the business's own
/// approved phone, `UnknownPhone` for a declined or another business's phone); it cannot approve.
async fn approve(
    tx: &mut PgConnection,
    grant: &ActionGrant,
    line: Uuid,
    now: Timestamp,
) -> Result<Outcome, Stop> {
    let by = Acting {
        business: grant.business(),
        member: grant.member(),
    };
    let waiting = waiting_phones(tx, by.business).await.map_err(failed)?;
    let set_at = waiting
        .iter()
        .find(|p| p.line == line)
        .map(|p| p.number_set_at);
    if set_at.is_some_and(|at| grant.issued_at() < at) {
        return Ok(Outcome::Refused(NOT_APPROVED, NEWER_EMAIL));
    }
    let named = set_at.unwrap_or_else(|| grant.issued_at());
    Ok(match approve_phone(tx, by, (line, named), now).await {
        Ok(()) => Outcome::Done(
            "Phone approved",
            "<p>This phone now works for your business.</p>".to_owned(),
        ),
        Err(TeamError::NotWaiting | TeamError::PhoneTaken | TeamError::NumberChanged) => {
            Outcome::Refused(NOT_APPROVED, COULD_NOT)
        }
        Err(TeamError::UnknownPhone | TeamError::NotAllowed) => return Err(Stop::NotFound),
        Err(_) => return Err(Stop::Failed),
    })
}

/// The heading of a refused approval.
const NOT_APPROVED: &str = "Phone not approved";
/// A refused approval: approved already, its number taken or changed (BIZ-033).
const COULD_NOT: &str = "We could not approve this phone. Nothing changed.";
/// A link from an email about an earlier number (D-3081).
const NEWER_EMAIL: &str = "This phone's number changed after this email was sent, so nothing \
was approved. A newer email about the new number is on its way.";

/// SEC-038: the Approve `POST` with no step-up in the last 5 minutes: 401 and a link to the
/// step-up page; the link is not used.
fn step_up_first() -> Response {
    let body = format!(
        "<p><a href=\"{BUSINESS_STEP_UP_ROUTE}\">Get a code</a>, then open the link again.</p>"
    );
    done(StatusCode::UNAUTHORIZED, "Confirm it's you", &body)
}

/// SEC-004: a session kind's `POST` with no business person: 401, a link to sign in, no detail.
fn signed_out() -> Response {
    let body = format!("<p><a href=\"{SIGNIN_ROUTE}\">Sign in</a>, then open the link again.</p>");
    done(
        StatusCode::UNAUTHORIZED,
        "Sign in to your business account",
        &body,
    )
}

/// SEC-014, D-2206: the page a refused quote gets; nothing was stored.
const TRY_LATER: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<title>Quote not sent</title></head><body><main><h1>Your quote wasn't sent</h1><p>Your quote \
did not go out. We got too many quotes from your connection in a short time. Please try again \
in a little while.</p></main></body></html>";

/// What a Quote `POST` came to.
enum Sent {
    /// The form again with its message, or the job page after the quote was stored.
    Page(Option<String>),
    /// `admit_quote` refused it (SEC-014).
    TryLater,
}

/// SEC-026, PICK-040, BIZ-049, D-2206: uses the Quote link and sends the quote in one
/// transaction, committed only when the quote was stored.
async fn send_quote(
    links: &Links,
    (job, token): (Uuid, &str),
    ip: IpAddr,
    posted: &Posted,
    csrf: &str,
) -> Result<Sent, Stop> {
    let now = links.clock.now();
    let mut tx = links.pool.begin().await.map_err(failed)?;
    let grant = use_action_link(&mut tx, (ActionKind::Quote, job), token, None, now).await;
    let business = grant.map_err(link_stop)?.business();
    let admitted = admit_quote(&links.pool, (business, Some(ip)), now).await;
    if !matches!(admitted, Ok(Admission::Admitted)) {
        return Ok(Sent::TryLater);
    }
    let link = JobLink { job, business };
    let form = read_form(&mut tx, link, posted.certified == "yes").await?;
    match checked(&mut tx, link, &form, posted).await? {
        Checked::Again(problems) => Ok(Sent::Page(again(&form, csrf, posted, &problems))),
        Checked::Ready(quote, facts) => {
            let stored = submit_quote_with_facts(&mut tx, (job, business), &quote, &facts, now);
            if stored.await.map_err(failed)? != Submitted::Saved {
                return Err(Stop::NotFound);
            }
            let action = not_a_fit_action(job, token);
            let choice = NotAFitChoice {
                action: &action,
                csrf,
            };
            let shown = done_page(&mut tx, link, &form, choice).await?;
            tx.commit().await.map_err(failed)?;
            Ok(Sent::Page(shown))
        }
    }
}

/// What the form shows again: the problems found and any largest-job warning.
#[derive(Default)]
struct Problems {
    facts: Vec<askhava_portals::api::FactProblem>,
    price: Option<PriceProblem>,
    note: Option<NoteProblem>,
    warning: Option<LargestWarning>,
}

/// A checked post: ready to store, or the form again.
enum Checked {
    Ready(askhava_quotes::api::Quote, askhava_quotes::api::QuoteFacts),
    Again(Problems),
}

/// PICK-040, PICK-052, PICK-053, PICK-007, BIZ-049, D-2710: the posted price, facts and note
/// checked, then the largest-job warning, then `parse_quote`.
async fn checked(
    tx: &mut PgConnection,
    link: JobLink,
    form: &ReadForm,
    posted: &Posted,
) -> Result<Checked, Stop> {
    let price = parse_price(form.rules.format, &posted.price());
    let facts = parse_facts(&posted.facts());
    let note = check_note(&posted.notes);
    let (Ok((price, _schedule)), Ok((facts, _minutes)), Ok(())) = (&price, &facts, &note) else {
        return Ok(Checked::Again(Problems {
            facts: facts.err().unwrap_or_default(),
            price: price.err(),
            note: note.err(),
            warning: None,
        }));
    };
    let warning = largest_warning(tx, link.business, price, posted).await?;
    if warning.is_some() {
        return Ok(Checked::Again(Problems {
            warning,
            ..Problems::default()
        }));
    }
    let quote = QuoteForm {
        price: price.clone(),
        notes: posted.notes.clone(),
        original: posted.notes.clone(),
        ai_read: false,
        earliest_visit: Some(facts.earliest_visit),
    };
    Ok(match parse_quote(quote, &form.rules) {
        Ok(quote) => Checked::Ready(quote, *facts),
        Err(bounce) => Checked::Again(bounced(bounce)),
    })
}

/// BIZ-049, D-2510: the warning for a price above `business`'s stored largest job, unless the
/// post confirmed this same amount; a business with no capability row is never warned.
async fn largest_warning(
    tx: &mut PgConnection,
    business: Uuid,
    price: &askhava_quotes::api::Price,
    posted: &Posted,
) -> Result<Option<LargestWarning>, Stop> {
    let stored = capabilities(tx, &[business]).await.map_err(failed)?;
    let confirmed = posted.over_largest.trim().parse::<u64>().ok();
    Ok(stored
        .get(&business)
        .and_then(|c| largest_job_warning(price, c.largest, confirmed)))
}

/// PICK-007, CARD-010, PICK-053: a bounce shown by the notes box or the price field.
fn bounced(bounce: Bounce) -> Problems {
    let (price, note) = match bounce {
        Bounce::Contact => (None, Some(NoteProblem::Contact)),
        Bounce::Restricted => (None, Some(NoteProblem::Restricted)),
        _ => (Some(PriceProblem::Amount), None),
    };
    Problems {
        price,
        note,
        ..Problems::default()
    }
}

/// The form again with the posted values and `problems` (A11Y-030); nothing was stored.
fn again(form: &ReadForm, csrf: &str, posted: &Posted, problems: &Problems) -> Option<String> {
    let fields = posted.price();
    let parsed = parse_price(form.rules.format, &fields).ok();
    let fee_lines = parsed.map(|(p, _)| fee_lines_for(form.fees, &p));
    let facts = QuoteFormFacts {
        fee_lines: fee_lines.unwrap_or_default(),
        values: posted.facts(),
        same_as_last: false,
        problems: problems.facts.clone(),
        ..form.facts.clone()
    };
    let price = PriceForm {
        problem: problems.price,
        note_problem: problems.note,
        ..price_form(form.rules, &fields, &posted.notes)
    };
    let price = quote_form_page_priced(&facts, csrf, (problems.warning.as_ref(), None), &price);
    price.ok()
}

/// CARD-023, PICK-057: after a stored quote, the job page with the counts read now, the new quote
/// counted, and the "Not a fit for us" `choice` (served by `business_portal`, T306).
async fn done_page(
    tx: &mut PgConnection,
    link: JobLink,
    form: &ReadForm,
    choice: NotAFitChoice<'_>,
) -> Result<Option<String>, Stop> {
    let shown = job_page(tx, (link.job, link.business), &form.parts).await;
    let shown = shown.map_err(failed)?.ok_or(Stop::NotFound)?;
    let links = JobPageLinks {
        job_number: &form.facts.job_number,
        photos: &[],
        concept: None,
    };
    Ok(job_details_page_choice(&shown, &links, None, choice).ok())
}
