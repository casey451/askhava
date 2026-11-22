//! The served notice and backup address changes (SEC-038, SEC-014, SEC-019, SEC-042, SEC-048,
//! BIZ-040, BIZ-043; T542, D-3890, D-3892, D-5081).
//!
//! [`routes`] serves `GET` [`ADDRESS_CHANGE_ROUTE`] (the page) and `POST`
//! [`ADDRESS_REQUEST_ROUTE`], [`ADDRESS_CONFIRM_ROUTE`] and [`BACKUP_REMOVE_ROUTE`] over
//! `portals`' `request_address_change`, `confirm_address_change` and `remove_backup_address`.
//! Every route is `no-store` and passes `refuse_cross_site`; every `POST` passes the CSRF form
//! check before the session is read, then parses its form once (an unknown or repeated field, a
//! posted mailbox ID among them, is a 422 that changes nothing, SEC-010). A code send passes
//! `CODE_SENDS_PER_IP` and [`ADDRESS_SENDS_PER_MEMBER`], a code try `CODE_TRIES_PER_IP` and
//! [`ADDRESS_TRIES_PER_MEMBER`], before anything is minted or checked (SEC-014). A change with
//! no step-up in the last 5 minutes is a 303 to the step-up page; a member the person may not
//! change gets the one not-found page and an `authz_denied` event (SEC-019, SEC-048). The code
//! goes to the typed address only, through `MailboxCodeSender`; without [`AddressParts`] (dev and
//! CI with no vault) the page is served and a change answers 500, and without the email provider
//! nothing is emailed. The code and the address are never logged.

use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::marker::PhantomData;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;

use askhava_adapters::api::EmailAdapter;
use askhava_core::api::{Clock, Settings, SystemClock};
use askhava_intake::api::{Admission, Key, Limit, admit};
use askhava_messaging::api::MailboxCodeSender;
use askhava_portals::api::{
    ADDRESS_CHANGE_ROUTE, ADDRESS_CONFIRM_ROUTE, ADDRESS_REQUEST_ROUTE, AddressChangeError,
    AddressKind, AddressScreen, BACKUP_REMOVE_ROUTE, BUSINESS_HOME_ROUTE, BUSINESS_STEP_UP_ROUTE,
    BusinessPrincipal, CODE_SENDS_PER_IP, CODE_TRIES_PER_IP, CookieJar, CsrfField, CsrfForm,
    SecurityEvent, SecurityEventKind, WebSecurity, address_change_page, admit_security_event,
    business_principal, confirm_address_change, csrf_token, no_store, record_security_event,
    refuse_cross_site, remove_backup_address, request_address_change, security_event_client,
    signed_out,
};
use askhava_privacy::api::Vault;
use axum::Router;
use axum::extract::{Extension, FromRef, Request, State};
use axum::http::header::LOCATION;
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// SEC-014: address code sends per member per Arizona day, counted after `CODE_SENDS_PER_IP`.
pub const ADDRESS_SENDS_PER_MEMBER: Limit = Limit::daily("server.address_codes_per_member", 5);
/// SEC-014, SEC-020: address code tries per member per hour, counted after `CODE_TRIES_PER_IP`.
pub const ADDRESS_TRIES_PER_MEMBER: Limit = Limit::hourly("server.address_tries_per_member", 10);

/// A boxed code email's answer: whether it went.
type Mailed<'a> = Pin<Box<dyn Future<Output = bool> + Send + 'a>>;

/// The code mailer behind `dyn`.
trait CodeMail: Send + Sync {
    fn send<'a>(
        &'a self,
        db: &'a mut PgConnection,
        to: (Uuid, &'a SecretString),
        code: &'a SecretString,
        now: Timestamp,
    ) -> Mailed<'a>;
}

impl<E: EmailAdapter + Send + Sync + 'static> CodeMail for MailboxCodeSender<E> {
    fn send<'a>(
        &'a self,
        db: &'a mut PgConnection,
        to: (Uuid, &'a SecretString),
        code: &'a SecretString,
        now: Timestamp,
    ) -> Mailed<'a> {
        Box::pin(async move {
            MailboxCodeSender::send(self, db, to, code, now)
                .await
                .is_ok()
        })
    }
}

/// What the changes need: the vault the typed address's mailbox is minted in, `ASKHAVA_ID_SECRET`
/// (the key the codes derive from) and the code mailer, if any (BIZ-040, D-3262).
pub struct AddressParts {
    vault: Vault,
    secret: SecretString,
    mail: Option<Box<dyn CodeMail>>,
}

impl AddressParts {
    /// The parts over `vault` and `secret`; codes are emailed by `mail` when given.
    #[must_use]
    pub fn new<E: EmailAdapter + Send + Sync + 'static>(
        vault: Vault,
        secret: SecretString,
        mail: Option<MailboxCodeSender<E>>,
    ) -> Self {
        let mail = mail.map(|m| Box::new(m) as Box<dyn CodeMail>);
        Self {
            vault,
            secret,
            mail,
        }
    }
}

/// BIZ-040, SEC-025: the parts from settings: the vault, the ID secret and the shared email
/// provider's code mailer; `None` with no vault (dev and CI).
///
/// # Errors
/// A bad setting, or a vault without `ASKHAVA_ID_SECRET`; never a value.
pub fn address_parts<I>(settings: &Settings, pairs: I) -> io::Result<Option<AddressParts>>
where
    I: IntoIterator<Item = (String, String)>,
{
    let pairs: Vec<(String, String)> = pairs.into_iter().collect();
    let Some(vault) = super::vault_from(settings, pairs.clone())? else {
        return Ok(None);
    };
    let secret = (pairs.iter())
        .find(|(k, v)| k == super::ID_SECRET && !v.trim().is_empty())
        .map(|(_, v)| SecretString::from(v.trim().to_owned()))
        .ok_or_else(|| io::Error::other("missing ASKHAVA_ID_SECRET"))?;
    let provider = crate::email_provider::email_provider(settings, pairs)?;
    let mail = provider.map(|p| {
        let keys = (p.secret().clone(), p.client().clone());
        MailboxCodeSender::new(keys.0, keys.1, p.postal(), p.daily_limit())
            .map_err(|_| io::Error::other("setting ASKHAVA_POSTAL_ADDRESS has an invalid value"))
    });
    Ok(Some(AddressParts::new(vault, secret, mail.transpose()?)))
}

/// The routes' state.
#[derive(Clone)]
struct Served {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    parts: Option<Arc<AddressParts>>,
}

impl FromRef<Served> for WebSecurity {
    fn from_ref(state: &Served) -> Self {
        state.security.clone()
    }
}

/// The request's client address, for the limits and the event cap; never stored (SEC-040).
#[derive(Clone, Copy)]
struct ClientIp(IpAddr);

/// The page and its three forms over `pool` (SEC-038, BIZ-040, BIZ-043).
pub fn routes(pool: PgPool, parts: Option<AddressParts>) -> Router {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Served {
        pool,
        clock,
        security: security.clone(),
        parts: parts.map(Arc::new),
    };
    Router::new()
        .route(ADDRESS_CHANGE_ROUTE, get(page))
        .route(ADDRESS_REQUEST_ROUTE, post(request))
        .route(ADDRESS_CONFIRM_ROUTE, post(confirm))
        .route(BACKUP_REMOVE_ROUTE, post(remove))
        .route_layer(from_fn(note_client))
        .with_state(state)
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// Notes the client address (SEC-040).
async fn note_client(mut request: Request, next: Next) -> Response {
    let ip = security_event_client(&request);
    request.extensions_mut().insert(ClientIp(ip));
    next.run(request).await
}

/// A failed read, change or send: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "address change failed");
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

/// SEC-004: the signed-in business person, else the signed-out answer.
async fn signed_in(state: &Served, jar: &CookieJar) -> Result<BusinessPrincipal, Box<Response>> {
    let Ok(mut db) = state.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    match business_principal(&mut db, jar, state.clock.now()).await {
        Ok(Some(who)) => Ok(who),
        Ok(None) => Err(Box::new(signed_out())),
        Err(_) => Err(Box::new(failed("business principal"))),
    }
}

/// The page for `screen` with the jar's CSRF token, as `status`.
fn shown(jar: &CookieJar, who: &BusinessPrincipal, screen: AddressScreen<'_>) -> Response {
    let Some(csrf) = csrf_token(jar) else {
        return failed("csrf token");
    };
    let status = match screen {
        AddressScreen::Limited => StatusCode::TOO_MANY_REQUESTS,
        _ => StatusCode::OK,
    };
    match address_change_page(screen, who.member(), &csrf) {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => failed("address page"),
    }
}

/// The answer to a refusal on `route` (SEC-019, SEC-038, SEC-048); a field problem shows the
/// form again with the typed value kept (A11Y-030).
async fn refused(
    (state, jar, ip): (&Served, &CookieJar, IpAddr),
    (route, who): (&str, &BusinessPrincipal),
    (kind, typed): (AddressKind, &str),
    error: AddressChangeError,
) -> Response {
    match error {
        AddressChangeError::NeedsCode => {
            let back = ADDRESS_CHANGE_ROUTE.replace('/', "%2F");
            let to = format!("{BUSINESS_STEP_UP_ROUTE}?return={back}");
            match HeaderValue::from_str(&to) {
                Ok(to) => (StatusCode::SEE_OTHER, [(LOCATION, to)]).into_response(),
                Err(_) => failed("redirect"),
            }
        }
        AddressChangeError::NotFound => {
            denied(state, (ip, route), who).await;
            not_found()
        }
        AddressChangeError::Database => failed("address change"),
        problem => shown(jar, who, AddressScreen::Problem(kind, typed, problem)),
    }
}

/// SEC-048: one `authz_denied` event for `who` on `route`, under the per-client cap.
async fn denied(state: &Served, (ip, route): (IpAddr, &str), who: &BusinessPrincipal) {
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

/// SEC-014: whether `checks` admit this request; a counter error refuses.
async fn admitted(state: &Served, checks: &[(Limit, Key)]) -> bool {
    let admission = admit(&state.pool, checks, state.clock.now()).await;
    matches!(admission, Ok(Admission::Admitted))
}

/// The field names one form may carry, besides `csrf`.
trait Fields {
    /// The allowed names.
    const NAMES: &'static [&'static str];
}

/// The new-address form: the member, the kind and the typed address; never a mailbox ID.
struct RequestFields;
impl Fields for RequestFields {
    const NAMES: &'static [&'static str] = &["member", "kind", "address"];
}

/// The code form: the request's fields again and the code (D-3892).
struct ConfirmFields;
impl Fields for ConfirmFields {
    const NAMES: &'static [&'static str] = &["member", "kind", "address", "code"];
}

/// The removal form: the member.
struct RemoveFields;
impl Fields for RemoveFields {
    const NAMES: &'static [&'static str] = &["member"];
}

/// A posted form: the CSRF token and the named values; an unknown or repeated name fails the
/// parse, a 422 that does nothing (SEC-010). No `Debug`: it holds an address and a code (R6).
#[derive(serde::Deserialize)]
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
    /// The member, the kind and the typed address, trimmed (R1).
    fn change(&self) -> Option<(Uuid, AddressKind, &str)> {
        let member = Uuid::try_parse(self.values.get("member")?).ok()?;
        let kind = AddressKind::parse(self.values.get("kind")?)?;
        Some((member, kind, self.values.get("address")?.trim()))
    }

    /// The member alone (R1).
    fn member(&self) -> Option<Uuid> {
        Uuid::try_parse(self.values.get("member")?).ok()
    }
}

/// A form that does not parse into a change: 422, nothing changed (SEC-010).
fn unprocessable() -> Response {
    StatusCode::UNPROCESSABLE_ENTITY.into_response()
}

/// `GET` [`ADDRESS_CHANGE_ROUTE`] (BIZ-040, BIZ-043): the forms. Changes nothing.
async fn page(State(state): State<Served>, jar: CookieJar) -> Response {
    match signed_in(&state, &jar).await {
        Ok(who) => shown(&jar, &who, AddressScreen::Forms),
        Err(answer) => *answer,
    }
}

/// `POST` [`ADDRESS_REQUEST_ROUTE`] (SEC-038, SEC-014, BIZ-040, BIZ-043): after the limits and a
/// step-up in the last 5 minutes, emails a code to the typed address only.
async fn request(
    State(state): State<Served>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<Posted<RequestFields>>,
) -> Response {
    let who = match signed_in(&state, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Some((member, kind, typed)) = form.change() else {
        return unprocessable();
    };
    let Some(parts) = state.parts.clone() else {
        return failed("address parts");
    };
    let checks = [
        (CODE_SENDS_PER_IP, Key::Ip(ip)),
        (ADDRESS_SENDS_PER_MEMBER, Key::Record(who.member())),
    ];
    if !admitted(&state, &checks).await {
        return shown(&jar, &who, AddressScreen::Limited);
    }
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let (address, now) = (SecretString::from(typed.to_owned()), state.clock.now());
    let keys = (&parts.vault, &parts.secret);
    let asked = request_address_change(&mut db, keys, &who, (kind, member, &address), now).await;
    let code = match asked {
        Ok(code) => code,
        Err(error) => {
            let site = (&state, &jar, ip);
            return refused(site, (ADDRESS_REQUEST_ROUTE, &who), (kind, typed), error).await;
        }
    };
    if let Some(mail) = &parts.mail
        && !mail
            .send(&mut db, (code.mailbox, &address), &code.code, now)
            .await
    {
        return failed("code email");
    }
    shown(&jar, &who, AddressScreen::Code(kind, typed, false))
}

/// `POST` [`ADDRESS_CONFIRM_ROUTE`] (BIZ-040, BIZ-043, SEC-014): after the limits, takes the
/// emailed code; a wrong or old code confirms nothing and shows the code form again.
async fn confirm(
    State(state): State<Served>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<Posted<ConfirmFields>>,
) -> Response {
    let who = match signed_in(&state, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let (Some((member, kind, typed)), Some(code)) = (form.change(), form.values.get("code")) else {
        return unprocessable();
    };
    let Some(parts) = state.parts.clone() else {
        return failed("address parts");
    };
    let checks = [
        (CODE_TRIES_PER_IP, Key::Ip(ip)),
        (ADDRESS_TRIES_PER_MEMBER, Key::Record(who.member())),
    ];
    if !admitted(&state, &checks).await {
        return shown(&jar, &who, AddressScreen::Limited);
    }
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let address = SecretString::from(typed.to_owned());
    let keys = (&parts.vault, &parts.secret);
    let change = (kind, member, &address);
    let try_ = (code.trim(), state.clock.now());
    match confirm_address_change(&mut db, keys, &who, change, try_).await {
        Ok(true) => shown(&jar, &who, AddressScreen::Confirmed(kind)),
        Ok(false) => shown(&jar, &who, AddressScreen::Code(kind, typed, true)),
        Err(error) => {
            let site = (&state, &jar, ip);
            refused(site, (ADDRESS_CONFIRM_ROUTE, &who), (kind, typed), error).await
        }
    }
}

/// `POST` [`BACKUP_REMOVE_ROUTE`] (SEC-038, BIZ-043): after a step-up in the last 5 minutes,
/// removes the member's backup address.
async fn remove(
    State(state): State<Served>,
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
    match remove_backup_address(&mut db, &who, member, state.clock.now()).await {
        Ok(_) => shown(&jar, &who, AddressScreen::Removed),
        Err(error) => {
            let site = (&state, &jar, ip);
            let at = (BACKUP_REMOVE_ROUTE, &who);
            refused(site, at, (AddressKind::Backup, ""), error).await
        }
    }
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
    fn a_mailbox_or_repeated_field_fails_the_parse() {
        let id = Uuid::new_v4().to_string();
        assert!(posted::<RequestFields>(&[("mailbox", &id)]).is_err());
        assert!(posted::<RemoveFields>(&[("member", "a"), ("member", "b")]).is_err());
        let form = posted::<RequestFields>(&[("member", &id), ("kind", "spare"), ("address", "")]);
        assert!(form.expect("parses").change().is_none());
        let form = posted::<RequestFields>(&[
            ("member", &id),
            ("kind", "backup"),
            ("address", " a@x.example "),
        ]);
        let form = form.expect("parses");
        let (_, kind, typed) = form.change().expect("a change");
        assert_eq!((kind, typed), (AddressKind::Backup, "a@x.example"));
    }
}
