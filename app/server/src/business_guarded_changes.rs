//! The served step-up-guarded business changes (SEC-038, SEC-004, SEC-019, SEC-042, SEC-048;
//! T565, D-4045, D-3890).
//!
//! [`routes`] serves `POST` [`TEAM_ROLE_ROUTE`] (a member's roles), `POST` [`TEAM_REMOVE_ROUTE`]
//! (a member's removal) and `POST` [`FEE_LIMIT_ROUTE`] (the monthly limit) over `portals`'
//! `change_team_role`, `remove_team_member` and `change_monthly_limit`, which check the role, then
//! `require_step_up`, then make the change. Every route reads `business_principal` first (no
//! business person: `signed_out`), is `no-store`, passes `refuse_cross_site` and the CSRF form
//! check, and parses its form once: a missing, bad or unknown field is a 422 that changes nothing
//! (SEC-010). The member a change acts on travels in the form body, never the path. A change
//! with no step-up in the last 5 minutes is a 303 to the step-up page with a `return` to the page
//! the change belongs to; a member without the role, or an unknown member, gets the one not-found
//! page and an `authz_denied` event (SEC-019, SEC-048); a done change is a 303 to that page.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::net::IpAddr;
use std::sync::Arc;

use askhava_businesses::api::Roles;
use askhava_core::api::{Clock, SystemClock};
use askhava_portals::api::{
    BILLING_ROUTE, BUSINESS_HOME_ROUTE, BUSINESS_STEP_UP_ROUTE, BusinessPrincipal, CookieJar,
    CsrfField, CsrfForm, FEE_LIMIT_ROUTE, GuardedChangeError, SecurityEvent, SecurityEventKind,
    TEAM_REMOVE_ROUTE, TEAM_ROLE_ROUTE, WebSecurity, admit_security_event, business_principal,
    change_monthly_limit, change_team_role, no_store, record_security_event, refuse_cross_site,
    remove_team_member, security_event_client, signed_out,
};
use axum::Router;
use axum::extract::{Extension, FromRef, Request, State};
use axum::http::header::LOCATION;
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::post;
use sqlx::PgPool;
use sqlx::types::Uuid;

/// The routes' state.
#[derive(Clone)]
struct Guarded {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
}

impl FromRef<Guarded> for WebSecurity {
    fn from_ref(state: &Guarded) -> Self {
        state.security.clone()
    }
}

/// The request's client address, for the SEC-048 event cap; never stored (SEC-040).
#[derive(Clone, Copy)]
struct ClientIp(IpAddr);

/// The three guarded change routes over `pool` (SEC-038).
pub fn routes(pool: PgPool) -> Router {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Guarded {
        pool,
        clock,
        security: security.clone(),
    };
    Router::new()
        .route(TEAM_ROLE_ROUTE, post(role))
        .route(TEAM_REMOVE_ROUTE, post(remove))
        .route(FEE_LIMIT_ROUTE, post(limit))
        .route_layer(from_fn(note_client))
        .with_state(state)
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// Notes the client address (SEC-040) for the event cap.
async fn note_client(mut request: Request, next: Next) -> Response {
    let ip = security_event_client(&request);
    request.extensions_mut().insert(ClientIp(ip));
    next.run(request).await
}

/// A failed read or change: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "business guarded change failed");
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
async fn signed_in(state: &Guarded, jar: &CookieJar) -> Result<BusinessPrincipal, Box<Response>> {
    let Ok(mut db) = state.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    match business_principal(&mut db, jar, state.clock.now()).await {
        Ok(Some(who)) => Ok(who),
        Ok(None) => Err(Box::new(signed_out())),
        Err(_) => Err(Box::new(failed("business principal"))),
    }
}

/// The answer to a change on `route` whose page is `back` (SEC-019, SEC-038, SEC-048).
async fn answer(
    state: &Guarded,
    (ip, route, back): (IpAddr, &str, &'static str),
    who: &BusinessPrincipal,
    done: Result<(), GuardedChangeError>,
) -> Response {
    match done {
        Ok(()) => see_other(back),
        Err(GuardedChangeError::NeedsCode) => {
            let encoded = back.replace('/', "%2F");
            see_other(&format!("{BUSINESS_STEP_UP_ROUTE}?return={encoded}"))
        }
        Err(GuardedChangeError::NotFound) => {
            denied(state, (ip, route), who).await;
            not_found()
        }
        Err(GuardedChangeError::OwnerKeepsBoth) => not_found(),
        Err(GuardedChangeError::Database) => failed("guarded change"),
    }
}

/// SEC-048: one `authz_denied` event for `who` on `route`, under the per-client cap; a failure to
/// record is logged once and never changes the answer.
async fn denied(state: &Guarded, (ip, route): (IpAddr, &str), who: &BusinessPrincipal) {
    let now = state.clock.now();
    if !admit_security_event(SecurityEventKind::AuthzDenied, ip, route, now) {
        return;
    }
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

/// The field names one form may carry, besides `csrf`.
trait Fields {
    /// The allowed names.
    const NAMES: &'static [&'static str];
}

/// The role change form: the member and the two role boxes.
struct RoleFields;
impl Fields for RoleFields {
    const NAMES: &'static [&'static str] = &["member", "cards_and_quotes", "confirm_and_billing"];
}

/// The removal form: the member.
struct RemoveFields;
impl Fields for RemoveFields {
    const NAMES: &'static [&'static str] = &["member"];
}

/// The monthly-limit form: the limit in whole dollars, empty to remove it.
struct LimitFields;
impl Fields for LimitFields {
    const NAMES: &'static [&'static str] = &["limit"];
}

/// A posted form: the CSRF token and the named values; an unknown or repeated name fails the
/// parse, a 422 that does nothing (SEC-010).
#[derive(Debug, serde::Deserialize)]
#[serde(try_from = "Vec<(String, String)>", bound = "F: Fields")]
struct Posted<F> {
    csrf: String,
    values: BTreeMap<String, String>,
    form: PhantomData<F>,
}

impl<F: Fields> TryFrom<Vec<(String, String)>> for Posted<F> {
    type Error = String;

    /// Splits the posted pairs; refuses an unknown or repeated field name (SEC-010).
    fn try_from(pairs: Vec<(String, String)>) -> Result<Self, String> {
        let mut csrf = String::new();
        let mut values = BTreeMap::new();
        for (name, value) in pairs {
            if name == "csrf" {
                csrf = value;
            } else if !F::NAMES.contains(&name.as_str()) {
                return Err(format!("unknown field `{name}`"));
            } else if values.insert(name.clone(), value).is_some() {
                return Err(format!("repeated field `{name}`"));
            }
        }
        Ok(Self {
            csrf,
            values,
            form: PhantomData,
        })
    }
}

impl<F> CsrfField for Posted<F> {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

impl<F> Posted<F> {
    /// The member the change acts on (R1).
    fn member(&self) -> Option<Uuid> {
        Uuid::try_parse(self.values.get("member")?).ok()
    }

    /// The roles ticked; none ticked is no role set (BIZ-023).
    fn roles(&self) -> Option<Roles> {
        let ticked = |name: &str| self.values.contains_key(name);
        Roles::new(ticked("cards_and_quotes"), ticked("confirm_and_billing"))
    }

    /// FEE-041: the limit in cents: `Some(None)` for an empty field (remove it), `Some(Some(_))`
    /// for 1 to 9,999,999 whole dollars, `None` for anything else.
    fn limit_cents(&self) -> Option<Option<u32>> {
        let raw = self.values.get("limit")?.trim();
        if raw.is_empty() {
            return Some(None);
        }
        let digits = raw.len() <= 7 && raw.bytes().all(|b| b.is_ascii_digit());
        let dollars: u32 = raw.parse().ok().filter(|d| digits && *d > 0)?;
        dollars.checked_mul(100).map(Some)
    }
}

/// A form that does not parse into a change: 422, nothing changed (SEC-010).
fn unprocessable() -> Response {
    StatusCode::UNPROCESSABLE_ENTITY.into_response()
}

/// `POST` [`TEAM_ROLE_ROUTE`] (SEC-038, BIZ-023): the owner, within 5 minutes of a step-up, sets
/// a member's roles.
async fn role(
    State(state): State<Guarded>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<Posted<RoleFields>>,
) -> Response {
    let who = match signed_in(&state, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let (Some(member), Some(roles)) = (form.member(), form.roles()) else {
        return unprocessable();
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let done = change_team_role(&mut db, &who, (member, roles), state.clock.now()).await;
    drop(db);
    let site = (ip, TEAM_ROLE_ROUTE, BUSINESS_HOME_ROUTE);
    answer(&state, site, &who, done).await
}

/// `POST` [`TEAM_REMOVE_ROUTE`] (SEC-038, BIZ-023): the owner, within 5 minutes of a step-up,
/// removes a member.
async fn remove(
    State(state): State<Guarded>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<Posted<RemoveFields>>,
) -> Response {
    let who = match signed_in(&state, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Some(member) = form.member() else {
        return unprocessable();
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let done = remove_team_member(&mut db, &who, member, state.clock.now()).await;
    drop(db);
    let site = (ip, TEAM_REMOVE_ROUTE, BUSINESS_HOME_ROUTE);
    answer(&state, site, &who, done).await
}

/// `POST` [`FEE_LIMIT_ROUTE`] (SEC-038, FEE-041): the owner or a "Confirm and billing" member,
/// within 5 minutes of a step-up, sets or removes the monthly limit.
async fn limit(
    State(state): State<Guarded>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<Posted<LimitFields>>,
) -> Response {
    let who = match signed_in(&state, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Some(cents) = form.limit_cents() else {
        return unprocessable();
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let done = change_monthly_limit(&mut db, &who, cents, state.clock.now()).await;
    drop(db);
    answer(&state, (ip, FEE_LIMIT_ROUTE, BILLING_ROUTE), &who, done).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn posted<F: Fields>(pairs: &[(&str, &str)]) -> Result<Posted<F>, String> {
        let pairs = pairs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()));
        Posted::<F>::try_from(pairs.collect::<Vec<_>>())
    }

    #[test]
    fn a_limit_parses_once_as_whole_dollars() {
        let cents = |raw: &str| {
            posted::<LimitFields>(&[("csrf", "t"), ("limit", raw)])
                .expect("parses")
                .limit_cents()
        };
        assert_eq!(cents("300"), Some(Some(30_000)));
        assert_eq!(cents(" "), Some(None));
        for bad in ["0", "-5", "12.50", "12345678", "1e3"] {
            assert_eq!(cents(bad), None, "{bad}");
        }
        let none = posted::<LimitFields>(&[("csrf", "t")]).expect("parses");
        assert_eq!(none.limit_cents(), None);
    }

    #[test]
    fn unknown_or_repeated_fields_fail_the_parse() {
        assert!(posted::<RemoveFields>(&[("limit", "5")]).is_err());
        assert!(posted::<RemoveFields>(&[("member", "a"), ("member", "b")]).is_err());
        let role = posted::<RoleFields>(&[("member", "x"), ("cards_and_quotes", "yes")]);
        let role = role.expect("parses");
        assert!(role.member().is_none());
        assert!(role.roles().is_some_and(Roles::cards_and_quotes));
        let none = posted::<RoleFields>(&[]).expect("parses");
        assert!(none.roles().is_none());
    }
}
