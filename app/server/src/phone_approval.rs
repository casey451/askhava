//! The served owner's waiting-phones page (IC-95; BIZ-033, BIZ-038, SEC-038, SEC-004, SEC-009,
//! SEC-034, SEC-042, SEC-048; T397, D-3081, D-3744, D-3889, D-4726).
//!
//! [`routes`] serves `GET` [`PHONES_ROUTE`] and the two `POST`s [`APPROVE_PHONE_ROUTE`] and
//! [`REJECT_PHONE_ROUTE`] over `portals`' `phone_approval_page` and `businesses`' `waiting_phones`,
//! `approve_phone` and `reject_phone`. Every response is `no-store`. A `POST` passes, in this order,
//! `refuse_cross_site`, the CSRF form check (each refusal a security event), `business_principal`
//! (none: `signed_out`), the owner check (anyone else, another business's line or an unknown line:
//! the one not-found answer and an `authz_denied` event, nothing changed), then `require_step_up`
//! with `SensitiveChange::ApprovePhone` on its own connection before the change's transaction (no
//! fresh code: a 303 to the step-up page and back to the list). The line and the `number_set_at`
//! the owner was shown travel in the form body; a number a member changed since is the page again
//! with the new digits and the changed-number line, nothing changed (D-3081).
//!
//! The member's sign-in address is opened from the vault by the member's `mailbox_id` with
//! `privacy::api::read_business_mailbox` under `BusinessPurpose::PhoneApproval` on a bare
//! connection (D-1848, D-3262): the vault is `main`'s own (`RouterParts::with_vault`), else the
//! Twilio webhooks', as `owner_portal` takes it (D-4726). Without either, or for a removed member
//! (`sign_in_lines` answers `None`) or an address the vault no longer holds, the page shows one
//! fixed line in its place. No address, number or token is logged (PRIV-016, R15).

use std::net::IpAddr;
use std::sync::Arc;

use askhava_businesses::api::{
    Acting, TeamError, WaitingPhone, approve_phone, reject_phone, sign_in_lines, waiting_phones,
};
use askhava_core::api::{Clock, SystemClock};
use askhava_portals::api::{
    APPROVE_PHONE_ROUTE, BUSINESS_HOME_ROUTE, BUSINESS_STEP_UP_ROUTE, BusinessPrincipal, CookieJar,
    CsrfField, CsrfForm, PHONES_ROUTE, PhonesNote, REJECT_PHONE_ROUTE, SecurityEvent,
    SecurityEventKind, SensitiveChange, ShownPhone, StepUpError, WebSecurity, admit_security_event,
    business_principal, csrf_token, no_store, phone_approval_page, posted_phone,
    record_security_event, refuse_cross_site, require_step_up, security_event_client, signed_out,
};
use askhava_privacy::api::{BusinessContactError, BusinessPurpose, Vault, read_business_mailbox};
use axum::Router;
use axum::extract::{Extension, FromRef, Request, State};
use axum::http::header::LOCATION;
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use secrecy::ExposeSecret;
use sqlx::PgPool;

use crate::inbound_text::Webhooks;

/// Where the members' addresses are read: `main`'s vault, the webhooks' (the same keys), or none.
#[derive(Clone)]
enum PhoneVault {
    None,
    Own(Arc<Vault>),
    Hooks(Arc<Webhooks>),
}

impl PhoneVault {
    fn get(&self) -> Option<&Vault> {
        match self {
            Self::None => None,
            Self::Own(vault) => Some(vault),
            Self::Hooks(hooks) => Some(&hooks.vault),
        }
    }
}

/// The routes' state.
#[derive(Clone)]
struct Phones {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    vault: PhoneVault,
}

impl FromRef<Phones> for WebSecurity {
    fn from_ref(state: &Phones) -> Self {
        state.security.clone()
    }
}

/// The request's client address, for the SEC-048 event cap; never stored (SEC-040).
#[derive(Clone, Copy)]
struct ClientIp(IpAddr);

/// The page and its two forms over `pool`, reading addresses from `vault`, else the webhooks'
/// vault, else none (BIZ-033, D-4726).
pub fn routes(pool: PgPool, vault: Option<Arc<Vault>>, webhooks: Option<&Arc<Webhooks>>) -> Router {
    let vault = match (vault, webhooks) {
        (Some(vault), _) => PhoneVault::Own(vault),
        (None, Some(hooks)) => PhoneVault::Hooks(Arc::clone(hooks)),
        (None, None) => PhoneVault::None,
    };
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Phones {
        pool,
        clock,
        security: security.clone(),
        vault,
    };
    Router::new()
        .route(PHONES_ROUTE, get(page))
        .route(APPROVE_PHONE_ROUTE, post(approve))
        .route(REJECT_PHONE_ROUTE, post(reject))
        .route_layer(from_fn(note_client))
        .with_state(state)
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store))
}

/// Notes the client address (SEC-040) for the event cap.
async fn note_client(mut request: Request, next: Next) -> Response {
    let ip = security_event_client(&request);
    request.extensions_mut().insert(ClientIp(ip));
    next.run(request).await
}

/// A failed read, render or change: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "phone approval failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// SEC-019: the business portal's one not-found answer; it never says why.
fn not_found() -> Response {
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><title>Page not found</title></head><body><main>\
         <h1>We can't find that page</h1><p>The link may be old, or it may be for a different \
         business account. <a href=\"{BUSINESS_HOME_ROUTE}\">Go to your business home</a>.</p>\
         </main></body></html>"
    );
    (StatusCode::NOT_FOUND, Html(page)).into_response()
}

/// A 303 to `path`.
fn see_other(path: &str) -> Response {
    match HeaderValue::from_str(path) {
        Ok(to) => (StatusCode::SEE_OTHER, [(LOCATION, to)]).into_response(),
        Err(_) => failed("redirect"),
    }
}

/// SEC-004: the signed-in business person, else the signed-out answer.
async fn signed_in(state: &Phones, jar: &CookieJar) -> Result<BusinessPrincipal, Box<Response>> {
    let Ok(mut db) = state.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    match business_principal(&mut db, jar, state.clock.now()).await {
        Ok(Some(who)) => Ok(who),
        Ok(None) => Err(Box::new(signed_out())),
        Err(_) => Err(Box::new(failed("business principal"))),
    }
}

/// SEC-019, SEC-048: one `authz_denied` event for `who` on `route`, under the per-client cap, and
/// the one not-found answer; a failure to record is logged once and never changes the answer.
async fn denied(state: &Phones, (ip, route): (IpAddr, &str), who: &BusinessPrincipal) -> Response {
    let now = state.clock.now();
    if admit_security_event(SecurityEventKind::AuthzDenied, ip, route, now) {
        let event = SecurityEvent {
            kind: SecurityEventKind::AuthzDenied,
            route,
            request_id: None,
            actor_ref: Some(who.member()),
        };
        let recorded = match state.pool.acquire().await {
            Ok(mut db) => record_security_event(&mut db, &event, now).await.is_ok(),
            Err(_) => false,
        };
        if !recorded {
            tracing::error!(route, "security event not recorded");
        }
    }
    not_found()
}

/// BIZ-033, D-3744, D-3889: the sign-in address of the member who set `phone`'s number, read on
/// a bare connection under `PhoneApproval`; `None` with no vault, a removed member or an address
/// the vault no longer holds.
async fn set_by(state: &Phones, phone: &WaitingPhone) -> Result<Option<String>, ()> {
    let Some(vault) = state.vault.get() else {
        return Ok(None);
    };
    let mut db = state.pool.acquire().await.map_err(|_| ())?;
    let Some(lines) = sign_in_lines(&mut db, phone.set_by).await.map_err(|_| ())? else {
        return Ok(None);
    };
    let purpose = BusinessPurpose::PhoneApproval;
    match read_business_mailbox(&mut db, vault, purpose, lines.mailbox).await {
        Ok(address) => Ok(Some(address.expose_secret().to_owned())),
        Err(BusinessContactError::NotFound) => Ok(None),
        Err(_) => Err(()),
    }
}

/// BIZ-033: the owner's page with `note` as `status`: the business's waiting phones, each with
/// its member's address, and the jar's CSRF token.
async fn shown(
    state: &Phones,
    (jar, who): (&CookieJar, &BusinessPrincipal),
    (note, status): (PhonesNote, StatusCode),
) -> Response {
    let Some(csrf) = csrf_token(jar) else {
        return failed("csrf token");
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let Ok(waiting) = waiting_phones(&mut db, who.business()).await else {
        return failed("waiting phones");
    };
    drop(db);
    let mut phones = Vec::with_capacity(waiting.len());
    for phone in waiting {
        let Ok(address) = set_by(state, &phone).await else {
            return failed("member address");
        };
        phones.push(ShownPhone {
            phone,
            set_by: address,
        });
    }
    match phone_approval_page(&phones, note, &csrf) {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => failed("phones page"),
    }
}

/// `GET` [`PHONES_ROUTE`] (BIZ-033, BIZ-038, SEC-004): the owner's list; anyone else the
/// not-found answer. Changes nothing.
async fn page(
    State(state): State<Phones>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
) -> Response {
    let who = match signed_in(&state, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    if !who.is_owner() {
        return denied(&state, (ip, PHONES_ROUTE), &who).await;
    }
    shown(&state, (&jar, &who), (PhonesNote::None, StatusCode::OK)).await
}

/// A posted phone form: the CSRF token, the line and the `number_set_at` shown; an unknown field
/// fails the parse, a 422 that changes nothing (SEC-010).
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PhoneForm {
    csrf: String,
    #[serde(default)]
    line: String,
    #[serde(default)]
    number_set_at: String,
}

impl CsrfField for PhoneForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// Which press.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Press {
    Approve,
    Reject,
}

/// `POST` [`APPROVE_PHONE_ROUTE`] (BIZ-033, SEC-038): the owner approves a waiting phone.
async fn approve(
    State(state): State<Phones>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<PhoneForm>,
) -> Response {
    pressed(&state, (ip, &jar), &form, Press::Approve).await
}

/// `POST` [`REJECT_PHONE_ROUTE`] (BIZ-033, SEC-038): the owner declines a waiting phone.
async fn reject(
    State(state): State<Phones>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<PhoneForm>,
) -> Response {
    pressed(&state, (ip, &jar), &form, Press::Reject).await
}

/// BIZ-033, SEC-004, SEC-038, SEC-048, D-3889: after the cross-site and CSRF checks, the
/// principal, then the owner check, then a step-up in the last 5 minutes, then the change.
async fn pressed(
    state: &Phones,
    (ip, jar): (IpAddr, &CookieJar),
    form: &PhoneForm,
    press: Press,
) -> Response {
    let route = match press {
        Press::Approve => APPROVE_PHONE_ROUTE,
        Press::Reject => REJECT_PHONE_ROUTE,
    };
    let who = match signed_in(state, jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    if !who.is_owner() {
        return denied(state, (ip, route), &who).await;
    }
    let Some(phone) = posted_phone(&form.line, &form.number_set_at) else {
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    };
    let now = state.clock.now();
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let change = (SensitiveChange::ApprovePhone, route);
    match require_step_up(&mut db, &who, change, now).await {
        Ok(()) => {}
        Err(StepUpError::NeedsCode) => {
            let back = PHONES_ROUTE.replace('/', "%2F");
            return see_other(&format!("{BUSINESS_STEP_UP_ROUTE}?return={back}"));
        }
        Err(StepUpError::Database) => return failed("step-up"),
    }
    let by = Acting {
        business: who.business(),
        member: who.member(),
    };
    let done = match press {
        Press::Approve => approve_phone(&mut db, by, phone, now).await,
        Press::Reject => reject_phone(&mut db, by, phone, now).await,
    };
    drop(db);
    let page = |note| shown(state, (jar, &who), (note, StatusCode::CONFLICT));
    match done {
        Ok(()) => see_other(PHONES_ROUTE),
        Err(TeamError::NumberChanged) => page(PhonesNote::NumberChanged).await,
        Err(TeamError::PhoneTaken) => page(PhonesNote::NumberTaken).await,
        Err(TeamError::UnknownPhone | TeamError::NotWaiting | TeamError::NotAllowed) => {
            denied(state, (ip, route), &who).await
        }
        Err(_) => failed("phone approval"),
    }
}
