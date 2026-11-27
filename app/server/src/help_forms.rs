//! The served help forms (BIZ-047, OPS-036, PRIV-031, SEC-004, SEC-014, SEC-034, SEC-042,
//! SEC-048; T309, D-2128, D-3890, D-4293).
//!
//! [`routes`] serves T200's page functions (`portals::help`): `GET` and `POST` `/b/help`, the
//! business Get help form, behind `portals::api::business_principal` (none: `signed_out`, a 401
//! linking `/b/sign-in`), whose post opens or joins one help request for that business with
//! `send_get_help` and queues its answer email with the parts' [`HelpMail`] (the drain sends it);
//! and `POST /help`, the site help form, with no session: the pre-session cookie's CSRF token,
//! then `admit_help_form` for the client IP `security_event_client` reads, before anything else
//! (a refusal is the 429 "Too many messages" screen and nothing else), then `send_help_form`
//! with the vault for an opt-out's number. A delete or copy request through it opens a help
//! request, acts on nothing and answers with the `/h/sign-in` link (D-4293). Every response is
//! `no-store`; every `POST` passes `refuse_cross_site` and then the CSRF form check
//! (`CsrfForm`), each refusal a 403 and a `csrf_rejected` security event.
//!
//! [`guard`] is the layer on `site`'s `GET /help` (D-3890): `site` may not call `portals`, so it
//! runs `ensure_pre_session` and hands the token to T201's page as `site::api::HelpFormCsrf`, the
//! hidden `csrf` field; that answer is `no-store` too. No message, number, email or token is
//! logged (PRIV-016, R15).

use std::io;
use std::net::IpAddr;
use std::sync::Arc;

use askhava_core::api::{Clock, Settings, SystemClock};
use askhava_intake::api::Admission;
use askhava_messaging::api::MailboxKind;
use askhava_portals::api::{
    BusinessPrincipal, CookieJar, CsrfField, CsrfForm, GET_HELP_ROUTE, GetHelpForm, GetHelpInput,
    GetHelpScreen, HELP_FORM_ROUTE, HelpFormScreen, WebSecurity, admit_help_form,
    business_principal, csrf_token, ensure_pre_session, get_help_page, help_form_page, no_store,
    refuse_cross_site, security_event_client, send_get_help, send_help_form, signed_out,
};
use askhava_privacy::api::Vault;
use askhava_site::api::HelpFormCsrf;
use axum::Router;
use axum::extract::{Extension, FromRef, Request, State};
use axum::http::header::{CACHE_CONTROL, LOCATION};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use secrecy::SecretString;
use sqlx::PgPool;

use crate::inbound_text::Webhooks;

/// What the Get help answer email is queued with (SEC-018, SEC-028): the email key
/// (`ASKHAVA_ID_SECRET`) and the footer's postal line. No `Debug`: it holds a secret (R6).
pub struct HelpMail {
    secret: SecretString,
    postal: (String, MailboxKind),
}

impl HelpMail {
    /// The mail parts over the email `secret` and the footer `postal` line and its kind.
    #[must_use]
    pub const fn new(secret: SecretString, postal: (String, MailboxKind)) -> Self {
        Self { secret, postal }
    }
}

/// BIZ-047, SEC-028: the email provider's key and postal line from `settings` and `pairs`; `None`
/// without a provider (dev and CI only), when a valid Get help post answers 500.
///
/// # Errors
///
/// A bad email provider setting, named, never its value.
pub fn help_mail<I>(settings: &Settings, pairs: I) -> io::Result<Option<HelpMail>>
where
    I: IntoIterator<Item = (String, String)>,
{
    let provider = crate::email_provider::email_provider(settings, pairs)?;
    Ok(provider.map(|p| {
        let (line, kind) = p.postal();
        HelpMail::new(p.secret().clone(), (line.to_owned(), kind))
    }))
}

/// Where an opt-out's number is sealed: `main`'s vault, else the webhooks' (the same keys).
#[derive(Clone)]
enum FormVault {
    None,
    Own(Arc<Vault>),
    Hooks(Arc<Webhooks>),
}

impl FormVault {
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
struct Forms {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    vault: FormVault,
    mail: Option<Arc<HelpMail>>,
}

impl FromRef<Forms> for WebSecurity {
    fn from_ref(state: &Forms) -> Self {
        state.security.clone()
    }
}

/// The request's client address, for the help form's limits (SEC-014, SEC-040); never stored.
#[derive(Clone, Copy)]
struct ClientIp(IpAddr);

/// BIZ-047, OPS-036, SEC-034, SEC-042: the Get help form and its post and the site help form's
/// post over `pool`, sealing an opt-out's number with `vault`, else the webhooks' vault, and
/// queuing the Get help answer with `mail`; all `no-store`, cross-site posts refused outermost.
pub fn routes(
    pool: PgPool,
    vault: Option<Arc<Vault>>,
    (webhooks, mail): (Option<&Arc<Webhooks>>, Option<HelpMail>),
) -> Router {
    let vault = match (vault, webhooks) {
        (Some(vault), _) => FormVault::Own(vault),
        (None, Some(hooks)) => FormVault::Hooks(Arc::clone(hooks)),
        (None, None) => FormVault::None,
    };
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Forms {
        pool,
        clock,
        security: security.clone(),
        vault,
        mail: mail.map(Arc::new),
    };
    Router::new()
        .route(GET_HELP_ROUTE, get(ask).post(send))
        .route(HELP_FORM_ROUTE, post(site_send))
        .route_layer(from_fn(note_client))
        .with_state(state)
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store))
}

/// SEC-042, D-3890: `pages` with a pre-session cookie and its token as `site::api::HelpFormCsrf`
/// on a `GET` of `/help`, whose form posts to [`routes`]' `POST /help`; that answer is
/// `no-store`, since it holds a per-visitor token. Every other path and method passes as it is.
pub fn guard(pages: Router) -> Router {
    pages.layer(from_fn(help_token))
}

/// [`guard`]'s layer.
async fn help_token(mut request: Request, next: Next) -> Response {
    let shown = matches!(*request.method(), Method::GET | Method::HEAD);
    if !shown || request.uri().path() != HELP_FORM_ROUTE {
        return next.run(request).await;
    }
    let Ok(jar) = ensure_pre_session(CookieJar::from_headers(request.headers())) else {
        return failed("pre-session cookie");
    };
    let token = csrf_token(&jar).unwrap_or_default();
    request.extensions_mut().insert(HelpFormCsrf(token));
    let mut response = (jar, next.run(request).await).into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Notes the client address (SEC-040) for the help form's limits.
async fn note_client(mut request: Request, next: Next) -> Response {
    let ip = security_event_client(&request);
    request.extensions_mut().insert(ClientIp(ip));
    next.run(request).await
}

/// A failed read, render or change: a bare 500, logged once here without values (R5).
fn failed(what: &'static str) -> Response {
    tracing::warn!(what, "help form failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// `page` as HTML with `status`, or the 500 when it did not render.
fn shown<E>(page: Result<String, E>, status: StatusCode) -> Response {
    match page {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => failed("render"),
    }
}

/// SEC-004: the signed-in business person, else the signed-out answer.
async fn signed_in(state: &Forms, jar: &CookieJar) -> Result<BusinessPrincipal, Box<Response>> {
    let Ok(mut db) = state.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    match business_principal(&mut db, jar, state.clock.real_now()).await {
        Ok(Some(who)) => Ok(who),
        Ok(None) => Err(Box::new(signed_out())),
        Err(_) => Err(Box::new(failed("business principal"))),
    }
}

/// `GET /b/help` (BIZ-047, SEC-004): the empty Get help form for a signed-in business person.
async fn ask(State(state): State<Forms>, jar: CookieJar) -> Response {
    if let Err(answer) = signed_in(&state, &jar).await {
        return *answer;
    }
    let Some(csrf) = csrf_token(&jar) else {
        return failed("csrf token");
    };
    shown(get_help_page(&csrf, GetHelpScreen::Ask), StatusCode::OK)
}

/// A Get help post's fields as typed (BIZ-047); missing fields read as empty.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct GetHelpPost {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    topic: String,
    #[serde(default)]
    job: String,
    #[serde(default)]
    message: String,
}

impl CsrfField for GetHelpPost {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// `POST /b/help` (BIZ-047, OPS-036, A11Y-030): after the cross-site and CSRF checks, the
/// principal; a form with problems is the form again (422); a checked one opens or joins the
/// business's help request and queues the answer email in one transaction, then shows the answer.
async fn send(
    State(state): State<Forms>,
    jar: CookieJar,
    CsrfForm(post): CsrfForm<GetHelpPost>,
) -> Response {
    let who = match signed_in(&state, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let input = GetHelpInput {
        topic: post.topic,
        job: post.job,
        message: post.message,
    };
    let form = match GetHelpForm::parse(&input) {
        Ok(form) => form,
        Err(problems) => {
            let screen = GetHelpScreen::Problems(&input, &problems);
            return shown(
                get_help_page(&post.csrf, screen),
                StatusCode::UNPROCESSABLE_ENTITY,
            );
        }
    };
    let Some(mail) = state.mail.as_deref() else {
        return failed("help mail");
    };
    let Ok(mut tx) = state.pool.begin().await else {
        return failed("connection");
    };
    let keys = (&mail.secret, (mail.postal.0.as_str(), mail.postal.1));
    let sent = send_get_help(&mut tx, keys, &who, &form, state.clock.now()).await;
    let Ok(sent) = sent else {
        return failed("get help");
    };
    if tx.commit().await.is_err() {
        return failed("commit");
    }
    shown(
        get_help_page(&post.csrf, GetHelpScreen::Sent(&sent.answer)),
        StatusCode::OK,
    )
}

/// The site help form's fields (OPS-036, CON-035, D-4293); the optional email is taken and never
/// used, since the form keeps no sender address; missing fields read as empty.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SiteHelpPost {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    message: String,
    #[serde(default, rename = "email")]
    _email: String,
    #[serde(default)]
    mobile: String,
}

impl CsrfField for SiteHelpPost {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// `POST /help` (OPS-036, CON-035, PRIV-031, SEC-014, D-4293): after the cross-site and CSRF
/// checks, `admit_help_form` for the client IP before anything else (refused: the 429 screen,
/// nothing opened or suppressed); an empty message goes back to the form (303); otherwise the
/// message opens one help request and the answer links the homeowner sign-in.
async fn site_send(
    State(state): State<Forms>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    CsrfForm(post): CsrfForm<SiteHelpPost>,
) -> Response {
    match admit_help_form(&state.pool, ip, state.clock.real_now()).await {
        Ok(Admission::Admitted) => {}
        Ok(Admission::Refused(_)) => {
            let page = help_form_page(HelpFormScreen::TooMany);
            return shown(page, StatusCode::TOO_MANY_REQUESTS);
        }
        Err(_) => return failed("help form limit"),
    }
    if post.message.trim().is_empty() {
        let to = HeaderValue::from_static(HELP_FORM_ROUTE);
        return (StatusCode::SEE_OTHER, [(LOCATION, to)]).into_response();
    }
    let Some(vault) = state.vault.get() else {
        return failed("vault");
    };
    let Ok(mut db) = state.pool.acquire().await else {
        return failed("connection");
    };
    let said = (post.message.as_str(), Some(post.mobile.as_str()));
    match send_help_form(&mut db, vault, said, state.clock.now()).await {
        Ok(screen) => shown(help_form_page(screen), StatusCode::OK),
        Err(_) => failed("help form"),
    }
}
