//! The served "Can't take it" page and button (PICK-050, SEC-019, SEC-026, SEC-009, SEC-034,
//! SEC-042, SEC-048, A11Y-007; T472, D-3750, D-3883).
//!
//! [`routes`] serves `GET` and `POST` [`CANT_TAKE_ROUTE`], the address the Confirm page and the
//! pick notice link to (`picks::api::cant_take_link`); `build_router` merges it beside the
//! business portal under the same business session. Each request first reads the person with
//! `portals::api::business_principal`: no business person gets `portals::api::signed_out` (a 401
//! linking to the sign-in). The `GET` renders `portals::api::cant_take_page` with the job's number
//! and the session's CSRF token for the owner or a "Confirm and billing" member of the picked
//! business while the pick waits for its Confirm, in a transaction it rolls back, so it changes
//! nothing (SEC-026). The `POST` (CSRF form check and the cross-site refusal, SEC-009, SEC-042)
//! runs `portals::api::cant_take_pick` in one transaction and answers 303 to the portal home.
//! Every other case (another business's pick, a cards-only member, a pick no longer waiting, an
//! unknown or malformed ID) is the one not-found answer with an `authz_denied` event, and its
//! `POST` changes nothing (SEC-019, SEC-048). Every response is `no-store` (SEC-034). The page
//! links `/static/portals/forms.css` (T301, D-3700) for its contrast (A11Y-007).

use std::net::IpAddr;
use std::sync::Arc;

use askhava_core::api::{Clock, SystemClock};
use askhava_jobs::api::job_number;
use askhava_picks::api::cant_take_job;
use askhava_portals::api::{
    BUSINESS_HOME_ROUTE, BusinessPrincipal, ButtonForm, CookieJar, CsrfForm, Need, SESSION_COOKIE,
    SecurityEvent, SecurityEventKind, WebSecurity, admit_security_event, business_principal,
    cant_take_page, cant_take_pick, csrf_token, no_store, record_security_event, refuse_cross_site,
    security_event_client, signed_out,
};
use axum::Router;
use axum::extract::{Extension, FromRef, Path, Request, State};
use axum::http::StatusCode;
use axum::http::header::{HeaderValue, LOCATION};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// PICK-050, D-3024: the "Can't take it" page and button of one pick.
pub const CANT_TAKE_ROUTE: &str = "/b/picks/{pick}/cant-take";

/// The route's state.
#[derive(Clone)]
struct Served {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
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

/// PICK-050, SEC-026, SEC-034, SEC-042: `GET` and `POST` [`CANT_TAKE_ROUTE`] over `pool`, behind
/// `no-store` and the cross-site refusal.
pub fn routes(pool: PgPool) -> Router {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Served {
        pool,
        clock,
        security: security.clone(),
    };
    Router::new()
        .route(CANT_TAKE_ROUTE, get(page).post(press))
        .route_layer(from_fn(note_site))
        .with_state(state)
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// Notes the request ID and the client address for the handlers.
async fn note_site(mut request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|id| Uuid::try_parse_ascii(id.as_bytes()).ok());
    let client = security_event_client(&request);
    request.extensions_mut().insert(Site { request_id, client });
    next.run(request).await
}

/// SEC-019: the one not-found answer, the same for every refusal and unknown ID, as the business
/// portal's; it never says which, and links to the portal home.
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
    tracing::warn!(what, "can't take page failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// SEC-048: records an `authz_denied` event for `site` with `actor`, under the per-client cap; a
/// failure to record is logged once and never changes the answer.
async fn denied_event(served: &Served, site: Site, actor: Option<Uuid>) {
    let now = served.clock.now();
    let kind = SecurityEventKind::AuthzDenied;
    if !admit_security_event(kind, site.client, CANT_TAKE_ROUTE, now) {
        return;
    }
    let event = SecurityEvent {
        kind,
        route: CANT_TAKE_ROUTE,
        request_id: site.request_id,
        actor_ref: actor,
    };
    let recorded = match served.pool.acquire().await {
        Ok(mut db) => record_security_event(&mut db, &event, now).await.is_ok(),
        Err(_) => false,
    };
    if !recorded {
        tracing::error!(route = CANT_TAKE_ROUTE, "security event not recorded");
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

/// PICK-050, SEC-019: the job number of `pick` while it waits for the Confirm of `who`'s
/// business and `who` is its owner or a "Confirm and billing" member, else `None`; read in a
/// transaction that is rolled back, so nothing changes (SEC-026).
async fn waiting_number(
    served: &Served,
    who: &BusinessPrincipal,
    pick: Uuid,
) -> Result<Option<i64>, &'static str> {
    if !who.may(who.business(), Need::ConfirmAndBilling) {
        return Ok(None);
    }
    let mut tx = served.pool.begin().await.map_err(|_| "transaction")?;
    let job = cant_take_job(&mut tx, pick, who.business()).await;
    let Some(job) = job.map_err(|_| "waiting pick")? else {
        return Ok(None);
    };
    let number = job_number(&mut tx, job).await.map_err(|_| "job number")?;
    tx.rollback().await.map_err(|_| "rollback")?;
    Ok(Some(number))
}

/// `GET` [`CANT_TAKE_ROUTE`] (PICK-050, SEC-019, SEC-026): the page with one CSRF button for a
/// pick `who` may end; it changes nothing. Anything else is the one not-found answer.
async fn page(
    State(served): State<Served>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
    Path(pick): Path<String>,
) -> Response {
    let who = match signed_in(&served, site, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Ok(pick) = Uuid::try_parse(&pick) else {
        return denied(&served, site, &who).await;
    };
    let number = match waiting_number(&served, &who, pick).await {
        Ok(Some(number)) => number,
        Ok(None) => return denied(&served, site, &who).await,
        Err(what) => return failed(what),
    };
    let Some(csrf) = csrf_token(&jar) else {
        return failed("csrf token");
    };
    match cant_take_page(&number.to_string(), &csrf) {
        Ok(html) => Html(html).into_response(),
        Err(_) => failed("render"),
    }
}

/// `POST` [`CANT_TAKE_ROUTE`] (PICK-050, PICK-046, PICK-047, SEC-019, SEC-026, SEC-042): ends
/// the waiting pick through `cant_take_pick` in one transaction and answers 303 to the portal
/// home; a pick `who` may not end, or one no longer waiting (a second press), is the one
/// not-found answer and changes nothing.
async fn press(
    State(served): State<Served>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
    Path(pick): Path<String>,
    CsrfForm(_): CsrfForm<ButtonForm>,
) -> Response {
    let who = match signed_in(&served, site, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Ok(pick) = Uuid::try_parse(&pick) else {
        return denied(&served, site, &who).await;
    };
    let Ok(mut tx) = served.pool.begin().await else {
        return failed("transaction");
    };
    match cant_take_pick(&mut tx, &who, pick, served.clock.now()).await {
        Ok(Some(_)) => match tx.commit().await {
            Ok(()) => (
                StatusCode::SEE_OTHER,
                [(LOCATION, HeaderValue::from_static(BUSINESS_HOME_ROUTE))],
            )
                .into_response(),
            Err(_) => failed("commit"),
        },
        Ok(None) => {
            drop(tx);
            denied(&served, site, &who).await
        }
        Err(_) => failed("can't take"),
    }
}
