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
//! `auth_failed` security event.
//!
//! The new-device code (T194, SEC-004, TXT-038, D-1773 to D-1775): with [`DeviceCodes`] set
//! ([`SignIn::with_device_codes`]), the button on a browser that is not a known device of that
//! member sends one Twilio Verify code, by text to the member's own phone that gets texts, else by
//! email to the member's own sign-in address, never by a call, and shows the code field; only a
//! correct code uses the link and signs in, and marks the browser a known device for
//! [`DEVICE_LIFE`] ([`DEVICE_COOKIE`]). Each person signs in as themself (BIZ-023): the link, the
//! code and the session all name one team member.
//!
//! [`business_principal`] is the only way a business route learns who is signed in: a business
//! session whose member is still a current member of its business; owner and homeowner sessions
//! are never one, so the owner realm (its own 12-hour sign-in, SEC-005, T086) and the homeowner
//! realm (phone and code, SEC-008) stay separate (SEC-006). [`BusinessPrincipal::may`] scopes
//! each route to that business and role (SEC-019, D-492).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use askama::Template;
use askhava_adapters::api::{CodeChannel, IdempotencyKey, VerifyAdapter};
use askhava_businesses::api::members;
use askhava_core::api::{Claimed, Clock, HandlerError};
use axum::Router;
use axum::extract::{FromRef, Path, State};
use axum::http::StatusCode;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::models;
use crate::security::{
    CsrfField, CsrfForm, SecurityEvent, SecurityEventKind, WebSecurity, no_store,
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
/// The cookie that marks a known device: a 256-bit token, stored only as SHA256 (D-1773).
pub const DEVICE_COOKIE: &str = "__Host-askhava-device";
/// How long a device stays known after a correct code (D-1773).
pub const DEVICE_LIFE: SignedDuration = SignedDuration::from_hours(180 * 24);
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

/// A boxed future, so the code traits can sit behind `dyn` in [`SignIn`]'s state.
pub type CodeFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Where one person's new-device code goes; never another person's phone (SEC-004, D-366).
pub enum CodeTo {
    /// That person's own phone that gets texts.
    Phone(SecretString),
    /// That person's own sign-in address, when they have no phone that gets texts.
    Email(SecretString),
}

impl CodeTo {
    /// By text to a phone, by email to an address, never by a call (TXT-038, D-366).
    #[must_use]
    pub fn channel(&self) -> CodeChannel {
        match self {
            Self::Phone(_) => CodeChannel::Sms,
            Self::Email(_) => CodeChannel::Email,
        }
    }

    fn address(&self) -> &str {
        match self {
            Self::Phone(to) | Self::Email(to) => to.expose_secret(),
        }
    }
}

/// The composition's lookup of where `member`'s code goes: the member's own phone that gets
/// texts, else the member's own sign-in address (SEC-004). `portals` never stores or logs it.
pub trait CodeAddress: Send + Sync + 'static {
    /// Where `member`'s new-device code goes.
    fn code_to<'a>(
        &'a self,
        db: &'a mut PgConnection,
        member: Uuid,
    ) -> CodeFuture<'a, Result<CodeTo, PortalsError>>;
}

/// Twilio Verify behind `dyn`: send (`false` when it failed) and check (`None` when it failed).
trait Codes: Send + Sync {
    fn send<'a>(&'a self, key: &'a IdempotencyKey, to: &'a CodeTo) -> CodeFuture<'a, bool>;
    fn check<'a>(&'a self, to: &'a CodeTo, code: &'a str) -> CodeFuture<'a, Option<bool>>;
}

impl<V: VerifyAdapter + Send + Sync + 'static> Codes for V {
    fn send<'a>(&'a self, key: &'a IdempotencyKey, to: &'a CodeTo) -> CodeFuture<'a, bool> {
        Box::pin(async move {
            self.send_code(key, to.address(), to.channel())
                .await
                .is_ok()
        })
    }

    fn check<'a>(&'a self, to: &'a CodeTo, code: &'a str) -> CodeFuture<'a, Option<bool>> {
        Box::pin(async move { self.check_code(to.address(), code).await.ok() })
    }
}

/// The new-device code's parts: Verify, the composition's [`CodeAddress`] and the idempotency
/// key secret (SEC-004, SEC-018, TXT-038: only through Verify, never the outbox).
pub struct DeviceCodes {
    verify: Box<dyn Codes>,
    to: Box<dyn CodeAddress>,
    key: SecretString,
}

impl DeviceCodes {
    /// Codes sent and checked by `verify`, to the place `to` names, keyed with `key`.
    pub fn new<V>(verify: V, to: impl CodeAddress, key: SecretString) -> Self
    where
        V: VerifyAdapter + Send + Sync + 'static,
    {
        Self {
            verify: Box::new(verify),
            to: Box::new(to),
            key,
        }
    }
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

/// The sign-in pages' state: the pool, clock, mail and web security every handler reads.
pub struct SignIn<M> {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    mail: Arc<M>,
    security: WebSecurity,
    codes: Option<Arc<DeviceCodes>>,
}

impl<M> Clone for SignIn<M> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            clock: Arc::clone(&self.clock),
            mail: Arc::clone(&self.mail),
            security: self.security.clone(),
            codes: self.codes.clone(),
        }
    }
}

impl<M> FromRef<SignIn<M>> for WebSecurity {
    fn from_ref(state: &SignIn<M>) -> Self {
        state.security.clone()
    }
}

impl<M: SignInMail> SignIn<M> {
    /// The sign-in pages' state over `pool`, timed by `clock`, emailing through `mail` (SEC-004).
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>, mail: Arc<M>) -> Self {
        let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
        Self {
            pool,
            clock,
            mail,
            security,
            codes: None,
        }
    }

    /// Asks for the new-device code before a link signs a new device in (SEC-004, TXT-038). The
    /// composition sets it when it serves the routes; without it the link alone signs in (T084).
    #[must_use]
    pub fn with_device_codes(mut self, codes: DeviceCodes) -> Self {
        self.codes = Some(Arc::new(codes));
        self
    }

    /// `routes` (the handlers below on [`SIGNIN_ROUTE`] and [`SIGNIN_LINK_ROUTE`]) with this state,
    /// `no-store` (a link page holds its token) and the cross-site refusal (SEC-004, SEC-042). The
    /// composition writes the two `.route` lines and calls this once its [`SignInMail`] exists;
    /// until then no page is served, so the Ally route table lists none (D-1724, D-1728).
    pub fn into_router(self, routes: Router<Self>) -> Router {
        let security = self.security.clone();
        routes
            .with_state(self)
            .layer(from_fn(no_store))
            .layer(from_fn_with_state(security, refuse_cross_site))
    }
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
pub async fn signin_form(jar: CookieJar) -> Response {
    page(jar, "form", ("", ""), false)
}

/// The sign-in form's fields.
#[derive(serde::Deserialize)]
pub struct AddressForm {
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
pub async fn signin_request<M: SignInMail>(
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
pub async fn signin_open<M: SignInMail>(
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

/// The link button's fields: the code, empty until the new-device code step (SEC-004).
#[derive(serde::Deserialize)]
pub struct CodeForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    code: String,
}

impl CsrfField for CodeForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// What the link's button does next (SEC-004).
enum Step {
    /// A used, expired or unknown link.
    Refused,
    /// The code field; `phone` when the code went by text, `failed` after a wrong code.
    Code { phone: bool, failed: bool },
    /// Sign in; `remember` marks the browser a known device.
    SignIn { remember: bool },
}

/// `POST` [`SIGNIN_LINK_ROUTE`]: uses the link and signs its member in for 30 days, after the
/// new-device code on a browser that is not a known device of that member when [`DeviceCodes`] is
/// set; a bad link or a wrong code is an `auth_failed` event (SEC-004, SEC-048, TXT-038).
pub async fn signin_post<M: SignInMail>(
    State(state): State<SignIn<M>>,
    Path(token): Path<String>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<CodeForm>,
) -> Response {
    let Ok(mut db) = state.pool.acquire().await else {
        return failure("database");
    };
    let now = state.clock.now();
    let step = match (&state.codes, well_formed(&token)) {
        (_, false) => Ok(Step::Refused),
        (None, true) => Ok(Step::SignIn { remember: false }),
        (Some(codes), true) => device_step(codes, &mut db, (&jar, &token), &form.code, now).await,
    };
    let signed = match step {
        Ok(Step::SignIn { remember }) => start(&mut db, jar.clone(), &token, now, remember).await,
        Ok(Step::Refused) => Ok(None),
        Ok(Step::Code { phone, failed }) => {
            if failed && !auth_failed(&mut db, now).await {
                return failure("security event");
            }
            return code_page(jar, &token, phone, failed);
        }
        Err(what) => return failure(what),
    };
    match signed {
        Ok(Some(jar)) => page(jar, "done", ("", ""), false),
        Ok(None) if auth_failed(&mut db, now).await => page(jar, "ended", ("", ""), false),
        Ok(None) => failure("security event"),
        Err(_) => failure("sign-in"),
    }
}

/// Records one `auth_failed` event on the link route; `false` when that failed (SEC-048).
async fn auth_failed(db: &mut PgConnection, now: Timestamp) -> bool {
    let event = SecurityEvent {
        kind: SecurityEventKind::AuthFailed,
        route: SIGNIN_LINK_ROUTE,
        request_id: None,
        actor_ref: None,
    };
    record_security_event(db, &event, now).await.is_ok()
}

/// The new-device step for an open link: a known device signs in; otherwise an empty `code`
/// sends one Verify code (one per link, its key) and a correct one signs in and is remembered
/// (SEC-004, TXT-038, D-1773, D-1774). The error names what failed.
async fn device_step(
    codes: &DeviceCodes,
    db: &mut PgConnection,
    (jar, token): (&CookieJar, &str),
    code: &str,
    now: Timestamp,
) -> Result<Step, &'static str> {
    let Some((link, member)) = open_link(db, token, now).await.map_err(|_| "link read")? else {
        return Ok(Step::Refused);
    };
    if known_device(db, jar, member, now)
        .await
        .map_err(|_| "device read")?
    {
        return Ok(Step::SignIn { remember: false });
    }
    let to = codes
        .to
        .code_to(db, member)
        .await
        .map_err(|_| "code address")?;
    let phone = to.channel() == CodeChannel::Sms;
    let code = code.trim();
    if code.is_empty() {
        let key = IdempotencyKey::derive(&codes.key, "verify.signin_code", &[&link.to_string()]);
        let sent = codes.verify.send(&key, &to).await;
        return sent
            .then_some(Step::Code {
                phone,
                failed: false,
            })
            .ok_or("code not sent");
    }
    let six = code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit());
    let right = six && codes.verify.check(&to, code).await.ok_or("code check")?;
    Ok(if right {
        Step::SignIn { remember: true }
    } else {
        Step::Code {
            phone,
            failed: true,
        }
    })
}

fn digest(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// The `(link, member)` of an unused, unexpired link; reads only (SEC-026).
async fn open_link(
    db: &mut PgConnection,
    token: &str,
    now: Timestamp,
) -> Result<Option<(Uuid, Uuid)>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT id, member_id FROM portals.login_link \
         WHERE token_sha256 = $1 AND used_at IS NULL AND expires_at > $2",
        digest(token),
        now.to_sqlx(),
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|r| (r.id, r.member_id)))
}

/// Whether the browser's [`DEVICE_COOKIE`] names a live known device of `member` (D-1773): a
/// `login_link` row born used whose life is [`DEVICE_LIFE`], never a 15-minute link.
async fn known_device(
    db: &mut PgConnection,
    jar: &CookieJar,
    member: Uuid,
    now: Timestamp,
) -> Result<bool, sqlx::Error> {
    let Some(cookie) = jar.get(DEVICE_COOKIE) else {
        return Ok(false);
    };
    let known = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM portals.login_link WHERE token_sha256 = $1 \
         AND member_id = $2 AND used_at = created_at AND expires_at > $3 \
         AND expires_at - created_at > interval '1 day')",
        digest(cookie.value()),
        member,
        now.to_sqlx(),
    )
    .fetch_one(db)
    .await?;
    Ok(known.unwrap_or(false))
}

/// Stores a new known device of `member` and sets its `__Host-` cookie (SEC-009, D-1773).
async fn remember_device(
    db: &mut PgConnection,
    jar: CookieJar,
    (member, business): (Uuid, Uuid),
    now: Timestamp,
) -> Result<CookieJar, PortalsError> {
    let token = models::new_link_token()?;
    let expires = now
        .checked_add(DEVICE_LIFE)
        .map_err(|_| PortalsError::BadLifetime)?;
    sqlx::query!(
        "INSERT INTO portals.login_link \
         (token_sha256, member_id, business_id, created_at, expires_at, used_at) \
         VALUES ($1, $2, $3, $4, $5, $4)",
        digest(&token),
        member,
        business,
        now.to_sqlx(),
        expires.to_sqlx(),
    )
    .execute(db)
    .await
    .map_err(|_| PortalsError::Database)?;
    let age = DEVICE_LIFE.as_secs();
    let line =
        format!("{DEVICE_COOKIE}={token}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={age}");
    let cookie = Cookie::parse(line).map_err(|_| PortalsError::NoRandom)?;
    Ok(jar.add(cookie))
}

/// The code step (A11Y-013: one paste-friendly one-time-code field; A11Y-030, A11Y-031).
#[derive(Template)]
#[template(
    ext = "html",
    source = r##"{% extends "base.html" %}
{% block title %}{% if failed %}Error: {% endif %}Enter your sign-in code{% endblock %}
{% block main %}<h1>Enter your sign-in code</h1>
{% if failed %}<div id="error-summary" role="alert" tabindex="-1" autofocus>
<h2>Check the code</h2>
<p><a href="#code">That code didn't work. Enter the 6-digit code we just sent you.</a></p>
</div>
{% endif %}<p>We haven't seen this phone or computer before, so we need a code to check it's you.</p>
<form method="post" action="/b/sign-in/{{ token }}" novalidate>
<input type="hidden" name="csrf" value="{{ csrf }}">
<p><label for="code">Sign-in code</label></p>
<p id="code-hint">{% if phone %}We texted a 6-digit code to your phone.{% else %}We emailed a 6-digit code to the same inbox your sign-in link went to.{% endif %}</p>
{% if failed %}<p id="code-error">Error: That code didn't work. Enter the 6-digit code we just sent you.</p>
{% endif %}<p><input type="text" id="code" name="code" autocomplete="one-time-code" inputmode="numeric" spellcheck="false" aria-describedby="code-hint{% if failed %} code-error{% endif %}"{% if failed %} aria-invalid="true"{% endif %}></p>
<p><button type="submit" id="code-button">Sign in</button></p>
</form>
<p>If the code doesn't come, <a href="/b/sign-in">get a new sign-in link</a>.</p>
{% endblock %}"##
)]
struct CodeView<'a> {
    token: &'a str,
    phone: bool,
    failed: bool,
    csrf: String,
}

fn code_page(jar: CookieJar, token: &str, phone: bool, failed: bool) -> Response {
    let Ok(jar) = ensure_pre_session(jar) else {
        return failure("pre-session");
    };
    let csrf = csrf_token(&jar).unwrap_or_default();
    let view = CodeView {
        token,
        phone,
        failed,
        csrf,
    };
    match view.render() {
        Ok(html) => (jar, Html(html)).into_response(),
        Err(_) => failure("code template failed"),
    }
}

/// In one transaction: uses `token`, checks its member is still on the team, starts the 30-day
/// session, names its business and, when `remember`, stores the known device. `None` (nothing
/// changed) when any check fails.
async fn start(
    db: &mut PgConnection,
    jar: CookieJar,
    token: &str,
    now: Timestamp,
    remember: bool,
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
    let jar = if remember {
        remember_device(&mut tx, jar, (member, business), now).await?
    } else {
        jar
    };
    tx.commit().await.map_err(|_| PortalsError::Database)?;
    Ok(Some(jar))
}
