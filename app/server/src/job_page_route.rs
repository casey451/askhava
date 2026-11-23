//! The served business job page, `GET /b/jobs/{j}` (CARD-023, CARD-019, CARD-020, BIZ-038,
//! PICK-024, PICK-051, SEC-019, SEC-034, PRIV-006; T666, D-4349, D-5526, D-5618 to D-5620).
//!
//! [`routes`] serves [`JOB_PAGE_ROUTE`], the address every job card email links to, the notice
//! link's sign-in lands on and the sign-up done screen links each open job by. Each request first
//! reads the person with `portals::api::business_principal`: no business person gets
//! `portals::api::signed_out` (the 4xx linking to the sign-in). `{j}` is a job number or a job
//! UUID; anything else is malformed. Before anything else is read the card is checked through
//! `quotes::api::carded_job` (by number) or `carded_job_id` (by UUID): a malformed `{j}`, an
//! unknown number or UUID and a job whose card never went to this business are the one not-found
//! answer with an `authz_denied` event (SEC-019, SEC-048). A carded UUID answers 303 to
//! `/b/jobs/<number>`. When the job's accepted pick (`picks::api::accepted_pick`) is this
//! business's and the person may "Confirm and billing", the answer is 303 to the release view
//! `/b/picks/{pick}` (PICK-024). Everyone else gets the page: the sent card as `PageParts`
//! (answers and later details empty until T745 and T746, D-4748), the current number sent and
//! quotes so far, the card photos and concept image through fresh short-lived `/photos/<token>`
//! links when the photo keys are configured (PRIV-014, PRIV-028), and the business's own quote
//! controls from `quotes::api::quote_standing` (closed once the job has an accepted pick). The
//! page never holds a homeowner contact detail (PRIV-006). Every response is `no-store` (SEC-034).

use std::net::IpAddr;
use std::sync::Arc;

use askhava_core::api::{Clock, IdSecret, SystemClock};
use askhava_jobs::api::job_number;
use askhava_picks::api::accepted_pick;
use askhava_portals::api::{
    BUSINESS_HOME_ROUTE, BusinessPrincipal, CookieJar, JOB_RELEASE_ROUTE, JobPageLinks, Need,
    OwnQuote, QuoteControls, SESSION_COOKIE, SecurityEvent, SecurityEventKind, WebSecurity,
    admit_security_event, business_principal, csrf_token, job_details_page_with_quote, no_store,
    record_security_event, refuse_cross_site, security_event_client, signed_out,
};
use askhava_privacy::api::{issue_link, photo_path};
use askhava_quotes::api::{
    JobPage, PageParts, QuoteStanding, carded_job, carded_job_id, job_page, quote_standing,
    sent_card,
};
use axum::Router;
use axum::extract::{Extension, FromRef, Path, Request, State};
use axum::http::StatusCode;
use axum::http::header::LOCATION;
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// CARD-023, BIZ-038: the business job page of one job, by its number (or its UUID, D-5526).
pub const JOB_PAGE_ROUTE: &str = "/b/jobs/{j}";

/// The route's state: the photo link secret only when the photo keys are configured.
#[derive(Clone)]
struct Served {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    photo_links: Option<Arc<IdSecret>>,
}

impl FromRef<Served> for WebSecurity {
    fn from_ref(served: &Served) -> Self {
        served.security.clone()
    }
}

/// The request ID and client address of a request, for its security events; the address only
/// keys the cap and is never stored (SEC-048, SEC-040).
#[derive(Clone, Copy)]
struct Site {
    request_id: Option<Uuid>,
    client: IpAddr,
}

/// What `{j}` names, parsed once at the edge (R1): a job number or a job ID (D-5526).
#[derive(Clone, Copy)]
enum Asked {
    Number(i64),
    Id(Uuid),
}

impl Asked {
    /// A number is ASCII digits only; else a UUID; else `None` (malformed).
    fn parse(j: &str) -> Option<Self> {
        if !j.is_empty() && j.bytes().all(|b| b.is_ascii_digit()) {
            return j.parse().ok().map(Self::Number);
        }
        Uuid::try_parse(j).ok().map(Self::Id)
    }
}

/// CARD-023, SEC-034, PRIV-014: `GET` [`JOB_PAGE_ROUTE`] over `pool`, behind `no-store` and the
/// cross-site refusal; `id_secret` (`ASKHAVA_ID_SECRET`, given only with the photo keys, whose
/// link secret it keys as version 1) issues the photo links, and without it the page shows none.
pub fn routes(pool: PgPool, id_secret: Option<&SecretString>) -> Router {
    let photo_links = id_secret.and_then(|s| IdSecret::new(1, s.clone()).ok());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Served {
        pool,
        clock,
        security: security.clone(),
        photo_links: photo_links.map(Arc::new),
    };
    Router::new()
        .route(JOB_PAGE_ROUTE, get(page))
        .route_layer(from_fn(note_site))
        .with_state(state)
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// Notes the request ID and the client address for the handler.
async fn note_site(mut request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|id| Uuid::try_parse_ascii(id.as_bytes()).ok());
    let client = security_event_client(&request);
    request.extensions_mut().insert(Site { request_id, client });
    next.run(request).await
}

/// SEC-019: the one not-found answer, the same for every refusal, unknown or malformed `{j}`, as
/// the business portal's; it never says which, and links to the portal home.
fn not_found() -> Response {
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><title>Page not found</title></head><body><main>\
         <h1>We can't find that page</h1><p>The link may be old, or it may be for a different \
         business account. <a href=\"{BUSINESS_HOME_ROUTE}\">Go to your business home</a>.</p>\
         </main></body></html>"
    );
    (StatusCode::NOT_FOUND, Html(page)).into_response()
}

/// A failed read or render: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "business job page failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// A 303 to `to`.
fn see_other(to: &str) -> Response {
    (StatusCode::SEE_OTHER, [(LOCATION, to.to_owned())]).into_response()
}

/// SEC-048: records an `authz_denied` event for `site` with `actor`, under the per-client cap; a
/// failure to record is logged once and never changes the answer.
async fn denied_event(served: &Served, site: Site, actor: Option<Uuid>) {
    let now = served.clock.now();
    let kind = SecurityEventKind::AuthzDenied;
    if !admit_security_event(kind, site.client, JOB_PAGE_ROUTE, now) {
        return;
    }
    let event = SecurityEvent {
        kind,
        route: JOB_PAGE_ROUTE,
        request_id: site.request_id,
        actor_ref: actor,
    };
    let recorded = match served.pool.acquire().await {
        Ok(mut db) => record_security_event(&mut db, &event, now).await.is_ok(),
        Err(_) => false,
    };
    if !recorded {
        tracing::error!(route = JOB_PAGE_ROUTE, "security event not recorded");
    }
}

/// SEC-019, SEC-048: the one not-found answer for `who`, recorded as an `authz_denied` event.
async fn denied(served: &Served, site: Site, who: &BusinessPrincipal) -> Response {
    denied_event(served, site, Some(who.member())).await;
    not_found()
}

/// SEC-004, SEC-024: the signed-in business person, else the answer to send: a session that is
/// not a current business member's is recorded as an `authz_denied` event.
async fn signed_in(
    served: &Served,
    site: Site,
    jar: &CookieJar,
) -> Result<BusinessPrincipal, Box<Response>> {
    let Ok(mut db) = served.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    match business_principal(&mut db, jar, served.clock.now()).await {
        Ok(Some(who)) => Ok(who),
        Ok(None) => {
            drop(db);
            if jar.get(SESSION_COOKIE).is_some() {
                denied_event(served, site, None).await;
            }
            Err(Box::new(signed_out()))
        }
        Err(_) => Err(Box::new(failed("business principal"))),
    }
}

/// What the card check found for `who`: the job and its number, a 303 to the number's page (a
/// UUID asked), or nothing (the one not-found answer).
enum Found {
    Job(Uuid, i64),
    Moved(i64),
    Nothing,
}

/// SEC-019, D-4349, D-5526: the card check before anything else is read.
async fn find(
    db: &mut PgConnection,
    asked: Asked,
    who: &BusinessPrincipal,
) -> Result<Found, &'static str> {
    let business = who.business();
    let (job, moved) = match asked {
        Asked::Number(n) => (carded_job(db, (n, business)).await, false),
        Asked::Id(id) => (carded_job_id(db, (id, business)).await, true),
    };
    let Some(job) = job.map_err(|_| "card check")? else {
        return Ok(Found::Nothing);
    };
    let number = job_number(db, job).await.map_err(|_| "job number")?;
    Ok(if moved {
        Found::Moved(number)
    } else {
        Found::Job(job, number)
    })
}

/// `GET` [`JOB_PAGE_ROUTE`] (CARD-023, PICK-024, SEC-019, SEC-026): the job page, or the 303 to
/// the number's page or to the release view; it changes nothing but issuing photo links.
async fn page(
    State(served): State<Served>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
    Path(j): Path<String>,
) -> Response {
    let who = match signed_in(&served, site, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Some(asked) = Asked::parse(&j) else {
        return denied(&served, site, &who).await;
    };
    let Ok(mut db) = served.pool.acquire().await else {
        return failed("connection");
    };
    let (job, number) = match find(&mut db, asked, &who).await {
        Ok(Found::Job(job, number)) => (job, number),
        Ok(Found::Moved(number)) => return see_other(&format!("/b/jobs/{number}")),
        Ok(Found::Nothing) => {
            drop(db);
            return denied(&served, site, &who).await;
        }
        Err(what) => return failed(what),
    };
    let Some(csrf) = csrf_token(&jar) else {
        return failed("csrf token");
    };
    match shown(&served, &mut db, (&who, job, number), &csrf).await {
        Ok(Some(answer)) => answer,
        Ok(None) => {
            drop(db);
            denied(&served, site, &who).await
        }
        Err(what) => failed(what),
    }
}

/// PICK-024, CARD-023, PICK-051: the 303 to the release view for a "Confirm and billing" person
/// of the business holding the job's accepted pick, else the rendered page; `None` when the job
/// has no sent card or no page for the business.
async fn shown(
    served: &Served,
    db: &mut PgConnection,
    (who, job, number): (&BusinessPrincipal, Uuid, i64),
    csrf: &str,
) -> Result<Option<Response>, &'static str> {
    let business = who.business();
    let accepted = accepted_pick(db, job).await.map_err(|_| "accepted pick")?;
    if let Some(a) = accepted.filter(|a| a.business == business)
        && who.may(business, Need::ConfirmAndBilling)
    {
        let to = JOB_RELEASE_ROUTE.replace("{pick}", &a.pick.to_string());
        return Ok(Some(see_other(&to)));
    }
    let Some(card) = sent_card(db, job).await.map_err(|_| "sent card")? else {
        return Ok(None);
    };
    let parts = PageParts {
        card: card.card,
        answers: Vec::new(),
        later: Vec::new(),
    };
    let page = job_page(db, (job, business), &parts).await;
    let Some(page) = page.map_err(|_| "job page")? else {
        return Ok(None);
    };
    let standing = quote_standing(db, (job, business)).await;
    let standing = standing.map_err(|_| "quote standing")?;
    let state = if accepted.is_some() {
        OwnQuote::Closed
    } else {
        own_quote(standing)
    };
    let links = photo_links(served, db, &page).await;
    Ok(Some(render(&page, number, &links, (state, csrf))?))
}

/// PICK-051: the portal's controls state for a [`QuoteStanding`].
fn own_quote(standing: QuoteStanding) -> OwnQuote {
    match standing {
        QuoteStanding::NotSent => OwnQuote::NotSent,
        QuoteStanding::Standing => OwnQuote::Standing,
        QuoteStanding::Withdrawn => OwnQuote::Withdrawn,
        QuoteStanding::Closed => OwnQuote::Closed,
    }
}

/// The page's photo links and concept link, each a fresh short-lived `/photos/<token>` path
/// (PRIV-014, PRIV-028); none without the photo keys. A photo whose link fails is left out, so
/// the page then shows no photo at all rather than a mismatched one (D-2874).
async fn photo_links(
    served: &Served,
    db: &mut PgConnection,
    page: &JobPage,
) -> (Vec<String>, Option<String>) {
    let Some(secret) = served.photo_links.as_deref() else {
        return (Vec::new(), None);
    };
    let now: Timestamp = served.clock.now();
    let mut photos = Vec::new();
    for photo in page.photos() {
        if let Ok(token) = issue_link(db, secret, photo, now).await {
            photos.push(photo_path(&token));
        }
    }
    let concept = match page.concept() {
        Some((id, _)) => issue_link(db, secret, id, now).await.ok(),
        None => None,
    };
    (photos, concept.map(|t| photo_path(&t)))
}

/// CARD-023, CARD-019, PICK-051, D-5618: the page with the business's quote controls; the change
/// link is the page itself until a signed-in quote form is served, and the withdraw form posts to
/// `/b/jobs/<number>/withdraw` (T732).
fn render(
    page: &JobPage,
    number: i64,
    (photos, concept): &(Vec<String>, Option<String>),
    (state, csrf): (OwnQuote, &str),
) -> Result<Response, &'static str> {
    let job_number = number.to_string();
    let here = format!("/b/jobs/{number}");
    let withdraw = format!("{here}/withdraw");
    let links = JobPageLinks {
        job_number: &job_number,
        photos,
        concept: concept.as_deref(),
    };
    let controls = QuoteControls {
        state,
        edit_link: &here,
        withdraw_action: &withdraw,
        csrf,
    };
    let html = job_details_page_with_quote(page, &links, controls).map_err(|_| "render")?;
    Ok(Html(html).into_response())
}
