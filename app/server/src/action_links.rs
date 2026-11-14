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
//! Only the Quote form's `POST` acts (D-3675): after `refuse_cross_site` (a layer) and the CSRF
//! check (`CsrfForm`, a refusal is a `csrf_rejected` security event), it uses the link in one
//! transaction, calls `admit_quote` first (D-2206), reads the rules (`quote_rules`), the sent
//! card (`sent_card`, D-3189) and the job page, parses the price, facts and note, warns once
//! above the business's largest job (`largest_job_warning`, D-2710), checks the quote with
//! `parse_quote` and stores it with `submit_quote_with_facts`; any problem rolls back and shows
//! the form again, so the link stays unused. Every other kind's `POST` gets the one not-found
//! answer before any token read (D-3675). A wrong kind, object, person, an ended, used or
//! unknown token, a removed member's token and a job with no sent card or no rules all get the
//! same bare 404. Every response is `no-store`.

use std::net::IpAddr;
use std::sync::Arc;

use askhava_businesses::api::capabilities;
use askhava_core::api::{Clock, SystemClock};
use askhava_intake::api::Admission;
use askhava_jobs::api::{job_facts, job_number};
use askhava_portals::api::{
    ACTION_ROUTE, ActionFacts, ActionKind, ActionLinkError, CookieJar, CsrfField, CsrfForm,
    FactsForm, LargestWarning, PriceForm, QuoteFormFacts, UseBy, WebSecurity, action_page,
    admit_quote, business_principal, csrf_token, ensure_pre_session, job_details_page,
    largest_job_warning, open_action_link, parse_facts, prefill, quote_form_page_priced,
    refuse_cross_site, security_event_client, use_action_link,
};
use askhava_portals::api::{JobPageLinks, no_store};
use askhava_quotes::api::{
    Bounce, NoteProblem, PageParts, PriceFields, PriceProblem, QuoteForm, QuoteRules, Submitted,
    check_note, job_page, last_facts, parse_price, parse_quote, quote_rules, sent_card,
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
        action_page(&ActionFacts {
            kind,
            job_number: "",
            ends_at: None,
            signed_in,
            days: None,
            csrf,
        })
        .ok()
    };
    tx.rollback().await.map_err(failed)?;
    Ok(html)
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
    let job_type = job_facts(tx, job).await.map_err(failed)?;
    let job_type = job_type.and_then(|f| f.job_types.into_iter().next());
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
    })
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

/// `POST /b/a/{kind}/{object}/{token}`: after the cross-site and CSRF checks, only a Quote link
/// acts; every other kind is the one not-found answer before any token read (D-3675).
async fn act(
    State(links): State<Links>,
    Extension(Client(ip)): Extension<Client>,
    Path(raw): Path<(String, String, String)>,
    jar: CookieJar,
    CsrfForm(posted): CsrfForm<Posted>,
) -> Response {
    let csrf = csrf_token(&jar).unwrap_or_default();
    let quote = target(raw).and_then(|(kind, object, token)| match kind {
        ActionKind::Quote => Ok((object, token)),
        _ => Err(Stop::NotFound),
    });
    let (job, token) = match quote {
        Ok(found) => found,
        Err(stop) => return stop.into_response(),
    };
    let sent = send_quote(&links, (job, &token), ip, &posted, &csrf).await;
    match sent {
        Ok(Sent::Page(html)) => page(jar, html),
        Ok(Sent::TryLater) => (StatusCode::TOO_MANY_REQUESTS, Html(TRY_LATER)).into_response(),
        Err(stop) => stop.into_response(),
    }
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
            let shown = done_page(&mut tx, link, &form).await?;
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
    let facts = QuoteFormFacts {
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

/// CARD-023: after a stored quote, the job page with the counts read now, the new quote counted.
async fn done_page(
    tx: &mut PgConnection,
    link: JobLink,
    form: &ReadForm,
) -> Result<Option<String>, Stop> {
    let shown = job_page(tx, (link.job, link.business), &form.parts).await;
    let shown = shown.map_err(failed)?.ok_or(Stop::NotFound)?;
    let links = JobPageLinks {
        job_number: &form.facts.job_number,
        photos: &[],
        concept: None,
    };
    Ok(job_details_page(&shown, &links).ok())
}
