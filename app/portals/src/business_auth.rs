//! Business sign-in: the single-use 15-minute emailed link, the 30-day business session, the
//! signed-in business principal every business route is scoped by, and ending a removed member's
//! sessions (SEC-004, SEC-006, SEC-019, SEC-024, SEC-026, A11Y-030; T084, D-1724 to D-1726).
//!
//! `GET` [`SIGNIN_ROUTE`] asks for the sign-in email; its `POST` hands the address to the
//! composition's [`SignInMail`], which names the team members it is on file for, and emails each
//! a link to [`SIGNIN_LINK_ROUTE`] that works once, for [`LINK_LIFE`]. The reply is the same
//! whether or not the address is on file, so the form tells no one who has an account; at most
//! [`LINKS_PER_LIFE`] links go to one member in any 15 minutes. Opening the link (a `GET`) only
//! shows a "Sign in" button (SEC-026: mail scanners open links); the button's `POST` uses the link,
//! checks the member is still on the team and starts a [`Realm::Business`] session for
//! [`BUSINESS_SESSION_LIFE`]. A used, expired or unknown link signs no one in and is logged as an
//! `auth_failed` security event. The new-device code is T194's (TXT-038).
//!
//! [`business_principal`] is the only way a business route learns who is signed in: a business
//! session whose member is still a current member of its business; owner and homeowner sessions
//! are never one, so the owner realm (its own 12-hour sign-in, SEC-005, T086) and the homeowner
//! realm (phone and code, SEC-008) stay separate (SEC-006). [`BusinessPrincipal::may`] scopes
//! each route to that business and role (SEC-019, D-492).

use std::future::Future;
use std::sync::Arc;

use askama::Template;
use askhava_businesses::api::members;
use askhava_core::api::{Claimed, Clock, HandlerError};
use axum::Router;
use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::middleware::from_fn_with_state;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum_extra::extract::cookie::CookieJar;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::models;
use crate::security::{
    ButtonForm, CsrfField, CsrfForm, SecurityEvent, SecurityEventKind, WebSecurity,
    record_security_event, refuse_cross_site,
};
use crate::session::{
    PortalsError, Realm, csrf_token, current_session, ensure_pre_session, start_session,
};

/// The sign-in form (SEC-004).
pub const SIGNIN_ROUTE: &str = "/b/sign-in";
/// The emailed link; the token is the one path segment (SEC-026).
pub const SIGNIN_LINK_ROUTE: &str = "/b/sign-in/{token}";
/// SEC-004: a sign-in link works for 15 minutes.
pub const LINK_LIFE: SignedDuration = SignedDuration::from_mins(15);
/// SEC-004, D-014: a business session lasts 30 days.
pub const BUSINESS_SESSION_LIFE: SignedDuration = SignedDuration::from_hours(30 * 24);
/// At most this many links go to one member in any [`LINK_LIFE`] (D-1725).
pub const LINKS_PER_LIFE: i64 = 5;
/// The site the emailed link points at (SEC-025: askhava.com only).
const SITE: &str = "https://askhava.com";

/// One team member's sign-in address on file: the business and the address's opaque mailbox ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignInAddress {
    /// The business account.
    pub business: Uuid,
    /// The opaque ID of the member's sign-in email (PRIV-001).
    pub mailbox: Uuid,
}

/// The composition's mail side of sign-in: which members an address is on file for, and the
/// email that carries the link (SEC-004, SEC-025). `portals` never stores or logs an address.
pub trait SignInMail: Send + Sync + 'static {
    /// The sign-in addresses `address` is on file as; none when it is on file for no one.
    fn addresses(
        &self,
        db: &mut PgConnection,
        address: &str,
    ) -> impl Future<Output = Result<Vec<SignInAddress>, PortalsError>> + Send;
    /// Emails `link` to `to`; the link is a credential, so it is never logged.
    fn send_link(
        &self,
        db: &mut PgConnection,
        to: SignInAddress,
        link: &SecretString,
    ) -> impl Future<Output = Result<(), PortalsError>> + Send;
}

/// What a business route needs of the signed-in person (BIZ-023, D-492).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Any current member of the business.
    Member,
    /// The "Cards and quotes" role (or the owner).
    CardsAndQuotes,
    /// The "Confirm and billing" role (or the owner).
    ConfirmAndBilling,
}

/// The signed-in business person: a current member of one business (SEC-004, SEC-019). Made only
/// by [`business_principal`].
#[derive(Debug)]
pub struct BusinessPrincipal {
    pub(crate) member: Uuid,
    pub(crate) business: Uuid,
    pub(crate) owner: bool,
    pub(crate) roles: (bool, bool),
}

impl BusinessPrincipal {
    /// The team member signed in.
    #[must_use]
    pub fn member(&self) -> Uuid {
        self.member
    }

    /// The business signed in to.
    #[must_use]
    pub fn business(&self) -> Uuid {
        self.business
    }

    /// Whether this person may act for `business` where a route needs `need`: only their own
    /// business, and only with the role, which the owner always holds (SEC-019, BIZ-023, D-492).
    #[must_use]
    pub fn may(&self, business: Uuid, need: Need) -> bool {
        let role = match need {
            Need::Member => true,
            Need::CardsAndQuotes => self.roles.0,
            Need::ConfirmAndBilling => self.roles.1,
        };
        self.business == business && (self.owner || role)
    }
}

/// The signed-in business person of the request's session cookie, or `None` for no session, a
/// homeowner or owner session, or a member no longer on the team (SEC-004, SEC-006, SEC-024).
///
/// # Errors
///
/// [`PortalsError::Database`].
pub async fn business_principal(
    db: &mut PgConnection,
    jar: &CookieJar,
    now: Timestamp,
) -> Result<Option<BusinessPrincipal>, PortalsError> {
    let Some(session) = current_session(db, jar, now).await? else {
        return Ok(None);
    };
    if session.realm() != Realm::Business {
        return Ok(None);
    }
    let Some((member, business)) = models::session_member(db, session.id()).await? else {
        return Ok(None);
    };
    let team = members(db, business)
        .await
        .map_err(|_| PortalsError::Database)?;
    Ok(team
        .into_iter()
        .find(|m| m.id == member)
        .map(|m| BusinessPrincipal {
            member,
            business,
            owner: m.owner,
            roles: (m.roles.cards_and_quotes(), m.roles.confirm_and_billing()),
        }))
}

/// Ends every session of a removed member and uses up its unused links (SEC-004, SEC-024): the
/// `businesses.member_removed` handler (TD-BIZ-06). Safe to run twice.
///
/// # Errors
///
/// A payload with no member ID, or a database failure.
pub async fn run_member_removed(pool: PgPool, task: Claimed) -> Result<(), HandlerError> {
    let member = task.payload["member"]
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or(HandlerError("portals.member_removed: no member"))?;
    let mut db = pool
        .acquire()
        .await
        .map_err(|_| HandlerError("portals.member_removed: database"))?;
    models::end_member(&mut db, member, Timestamp::now())
        .await
        .map(|_| ())
        .map_err(|_| HandlerError("portals.member_removed: database"))
}

/// The sign-in pages' state.
struct SignIn<M> {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    mail: Arc<M>,
    security: WebSecurity,
}

impl<M> Clone for SignIn<M> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            clock: Arc::clone(&self.clock),
            mail: Arc::clone(&self.mail),
            security: self.security.clone(),
        }
    }
}

impl<M> FromRef<SignIn<M>> for WebSecurity {
    fn from_ref(state: &SignIn<M>) -> Self {
        state.security.clone()
    }
}

/// The business sign-in router over `pool`, timed by `clock`, emailing through `mail` (SEC-004):
/// [`SIGNIN_ROUTE`] and [`SIGNIN_LINK_ROUTE`], each `GET` and `POST`, behind the cross-site
/// refusal. The composition merges it once its [`SignInMail`] exists (D-1724).
pub fn business_signin_routes<M: SignInMail>(
    pool: PgPool,
    clock: Arc<dyn Clock>,
    mail: Arc<M>,
) -> Router {
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = SignIn {
        pool,
        clock,
        mail,
        security: security.clone(),
    };
    Router::new()
        .route(SIGNIN_ROUTE, get(form).post(request::<M>))
        .route(SIGNIN_LINK_ROUTE, get(open::<M>).post(sign_in::<M>))
        .with_state(state)
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// `templates/business_signin.html`: one of the five screens.
#[derive(Template)]
#[template(path = "business_signin.html")]
struct View<'a> {
    screen: &'a str,
    failed: bool,
    email: &'a str,
    token: &'a str,
    csrf: String,
}

fn page(jar: CookieJar, screen: &str, (email, token): (&str, &str), failed: bool) -> Response {
    let Ok(jar) = ensure_pre_session(jar) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let csrf = csrf_token(&jar).unwrap_or_default();
    let view = View {
        screen,
        failed,
        email,
        token,
        csrf,
    };
    match view.render() {
        Ok(html) => (jar, Html(html)).into_response(),
        Err(_) => failure("business sign-in template failed"),
    }
}

fn failure(what: &'static str) -> Response {
    tracing::warn!(what, "business sign-in failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// `GET` [`SIGNIN_ROUTE`]: the email form.
async fn form(jar: CookieJar) -> Response {
    page(jar, "form", ("", ""), false)
}

/// The sign-in form's fields.
#[derive(serde::Deserialize)]
struct AddressForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    email: String,
}

impl CsrfField for AddressForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// An address worth looking up: one `@` with text on both sides and no spaces.
fn plausible(email: &str) -> bool {
    email.split_once('@').is_some_and(|(user, host)| {
        !user.is_empty() && host.contains('.') && !host.contains('@') && !email.contains(' ')
    })
}

/// `POST` [`SIGNIN_ROUTE`]: emails a link to every member the address is on file for, then shows
/// the same "check your email" screen either way (SEC-004).
async fn request<M: SignInMail>(
    State(state): State<SignIn<M>>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<AddressForm>,
) -> Response {
    let email = form.email.trim();
    if !plausible(email) {
        return page(jar, "form", (email, ""), true);
    }
    let Ok(mut db) = state.pool.acquire().await else {
        return failure("database");
    };
    let now = state.clock.now();
    let Ok(found) = state.mail.addresses(&mut db, email).await else {
        return failure("address lookup");
    };
    for to in found {
        if send_one(&state, &mut db, to, now).await.is_err() {
            return failure("link not sent");
        }
    }
    page(jar, "sent", ("", ""), false)
}

/// Stores and emails one link to the current member behind `to`, unless that member already got
/// [`LINKS_PER_LIFE`] in the last 15 minutes (D-1725).
async fn send_one<M: SignInMail>(
    state: &SignIn<M>,
    db: &mut PgConnection,
    to: SignInAddress,
    now: Timestamp,
) -> Result<(), PortalsError> {
    let team = members(db, to.business)
        .await
        .map_err(|_| PortalsError::Database)?;
    let Some(member) = team.into_iter().find(|m| m.mailbox == to.mailbox) else {
        return Ok(());
    };
    let since = now
        .checked_sub(LINK_LIFE)
        .map_err(|_| PortalsError::BadLifetime)?;
    if models::links_since(db, member.id, since).await? >= LINKS_PER_LIFE {
        return Ok(());
    }
    let expires = now
        .checked_add(LINK_LIFE)
        .map_err(|_| PortalsError::BadLifetime)?;
    let token = models::new_link_token()?;
    models::insert_link(db, &token, (member.id, to.business), (now, expires)).await?;
    let link = SecretString::from(format!("{SITE}/b/sign-in/{token}"));
    state.mail.send_link(db, to, &link).await
}

/// A link token's shape: 64 lower-case hex characters.
fn well_formed(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `GET` [`SIGNIN_LINK_ROUTE`]: the "Sign in" button for a usable link, or the "ended" screen.
/// Uses nothing up (SEC-026).
async fn open<M: SignInMail>(
    State(state): State<SignIn<M>>,
    Path(token): Path<String>,
    jar: CookieJar,
) -> Response {
    if !well_formed(&token) {
        return page(jar, "ended", ("", ""), false);
    }
    let Ok(mut db) = state.pool.acquire().await else {
        return failure("database");
    };
    match models::link_open(&mut db, &token, state.clock.now()).await {
        Ok(true) => page(jar, "open", ("", &token), false),
        Ok(false) => page(jar, "ended", ("", ""), false),
        Err(_) => failure("link read"),
    }
}

/// `POST` [`SIGNIN_LINK_ROUTE`]: uses the link and signs its member in for 30 days, or shows the
/// "ended" screen and logs an `auth_failed` event (SEC-004, SEC-048).
async fn sign_in<M: SignInMail>(
    State(state): State<SignIn<M>>,
    Path(token): Path<String>,
    jar: CookieJar,
    CsrfForm(_): CsrfForm<ButtonForm>,
) -> Response {
    let Ok(mut db) = state.pool.acquire().await else {
        return failure("database");
    };
    let now = state.clock.now();
    let signed = if well_formed(&token) {
        start(&mut db, jar.clone(), &token, now).await
    } else {
        Ok(None)
    };
    match signed {
        Ok(Some(jar)) => page(jar, "done", ("", ""), false),
        Ok(None) => {
            let event = SecurityEvent {
                kind: SecurityEventKind::AuthFailed,
                route: SIGNIN_LINK_ROUTE,
                request_id: None,
                actor_ref: None,
            };
            if record_security_event(&mut db, &event, now).await.is_err() {
                return failure("security event");
            }
            page(jar, "ended", ("", ""), false)
        }
        Err(_) => failure("sign-in"),
    }
}

/// In one transaction: uses `token`, checks its member is still on the team, starts the 30-day
/// session and names its business. `None` (nothing changed) when any check fails.
async fn start(
    db: &mut PgConnection,
    jar: CookieJar,
    token: &str,
    now: Timestamp,
) -> Result<Option<CookieJar>, PortalsError> {
    let mut tx = sqlx::Connection::begin(&mut *db)
        .await
        .map_err(|_| PortalsError::Database)?;
    let Some((member, business)) = models::use_link(&mut tx, token, now).await? else {
        return Ok(None);
    };
    let team = members(&mut tx, business)
        .await
        .map_err(|_| PortalsError::Database)?;
    if !team.iter().any(|m| m.id == member) {
        return Ok(None);
    }
    let login = (Realm::Business, member);
    let jar = start_session(&mut tx, jar, login, BUSINESS_SESSION_LIFE, now).await?;
    let session = current_session(&mut tx, &jar, now)
        .await?
        .ok_or(PortalsError::Database)?;
    models::insert_business_session(&mut tx, session.id(), business).await?;
    tx.commit().await.map_err(|_| PortalsError::Database)?;
    Ok(Some(jar))
}
