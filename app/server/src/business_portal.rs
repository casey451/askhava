//! The served business portal (SEC-004, SEC-009, SEC-024, SEC-034, SEC-042, SEC-048, BIZ-013,
//! BIZ-015, BIZ-025, BIZ-036, PICK-024, CARD-022; T300, D-1728, D-1739, D-1848, D-2117, D-3618).
//!
//! [`routes`] serves `GET` [`BUSINESS_HOME_ROUTE`], `POST` [`OPEN_CARDS_ROUTE`], `GET`
//! [`JOB_RELEASE_ROUTE`], `GET` [`BILLING_ROUTE`], `GET` [`STATEMENT_ROUTE`] and `GET` and `POST`
//! [`NOTICE_SETTINGS_ROUTE`] over `portals`' page functions; `build_router` always merges it.
//! Every route first reads the person with `portals::api::business_principal`: no business person
//! (signed out, a homeowner or owner session, a removed member) gets [`signed_out`], a 401 linking
//! to [`SIGNIN_ROUTE`] with no record detail; a refusal by a signed-in person (another business's
//! pick, a missing role, an unknown ID, a non-owner's channel change) is the one [`not_found`]
//! answer and an `authz_denied` security event (SEC-048). Every response is `no-store` (SEC-034);
//! both `POST`s pass `refuse_cross_site` and the CSRF form check (SEC-009, SEC-042).
//!
//! The facts come through each module's `api`: the home's open-cards times and inbox switch from
//! `businesses` and `portals`, its "Your numbers" fee total from `payments::api::business_statement`
//! for this Arizona month with every other count `YourNumbers::default()`'s (D-3618); billing from
//! the same statement with no saved cards and no fee limit shown (no `api` reads them yet,
//! D-3618, D-3664); the released job's page through `picks::api::released_view` with the session
//! proof from [`crate::session_minter`] on every load (D-1739); the notice addresses opened from the
//! vault here (D-1848, D-3664). Business texting is off (TXT-035), so the channel is email.

use std::io;
use std::net::IpAddr;
use std::sync::Arc;

use askhava_adapters::api::PoolNumber;
use askhava_businesses::api::{
    NoticeChannel, NoticePurpose, may_get_notices, notice_addresses, notice_channel,
    open_cards_times, returned_mailboxes, sign_in_lines,
};
use askhava_core::api::{Clock, Settings, SystemClock, arizona_time};
use askhava_payments::api::{Statement, business_statement};
use askhava_portals::api::{
    BILLING_ROUTE, BUSINESS_HOME_ROUTE, BusinessPrincipal, ChannelChangeError, ChannelFacts,
    CookieJar, CsrfField, CsrfForm, InboxSwitch, JOB_RELEASE_ROUTE, JobFacts,
    NOTICE_SETTINGS_ROUTE, Need, NoticeAddress, OPEN_CARDS_ROUTE, SESSION_COOKIE, SIGNIN_ROUTE,
    STATEMENT_ROUTE, SecurityEvent, SecurityEventKind, WebSecurity, YourNumbers,
    admit_security_event, billing_page, business_home_page, business_principal, change_channel,
    csrf_token, inbox_shared, job_release_page, no_store, notice_settings_page,
    record_security_event, refuse_cross_site, save_open_cards_times, security_event_client,
    statement_csv,
};
use askhava_privacy::api::{BusinessContactError, BusinessPurpose, Vault, read_business_mailbox};
use axum::Router;
use axum::extract::{Extension, FromRef, MatchedPath, Path, Request, State};
use axum::http::StatusCode;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, HeaderValue, LOCATION};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use jiff::Timestamp;
use secrecy::ExposeSecret;
use sqlx::PgPool;
use sqlx::types::Uuid;

use crate::inbound_text::Webhooks;
use crate::inbound_voice::VoiceHooks;

/// TXT-035: business texting is off, so the notice channel is email alone (BIZ-036).
const TEXTING_ON: bool = false;

/// Where the vault the released job's page and the notice page read comes from: `main`'s own
/// (`RouterParts::with_vault`) or the Twilio webhooks' (the same keys), else none.
#[derive(Clone)]
pub enum PortalVault {
    /// No vault keys: the two vault pages answer 500 to a signed-in person.
    None,
    /// The vault `main` built from `VAULT_KEY` and `VAULT_INDEX_KEY`.
    Own(Arc<Vault>),
    /// The webhooks' vault, built from the same keys.
    Hooks(Arc<Webhooks>),
}

impl PortalVault {
    fn get(&self) -> Option<&Vault> {
        match self {
            Self::None => None,
            Self::Own(vault) => Some(vault),
            Self::Hooks(hooks) => Some(&hooks.vault),
        }
    }
}

/// The portal routes' state.
#[derive(Clone)]
struct Portal {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    security: WebSecurity,
    vault: PortalVault,
    numbers: Arc<[PoolNumber]>,
}

impl FromRef<Portal> for WebSecurity {
    fn from_ref(portal: &Portal) -> Self {
        portal.security.clone()
    }
}

/// The route template, request ID and client address of a request, for its security events; the
/// address only keys the cap and is never stored (SEC-048, SEC-040).
#[derive(Clone)]
struct Site {
    route: String,
    request_id: Option<Uuid>,
    client: IpAddr,
}

/// The portal's routes over `pool`, reading the vault from `vault` and listing the masked
/// `numbers` on the home page's calls card (BIZ-046), behind `no-store` and the cross-site
/// refusal (SEC-034, SEC-042).
pub fn routes(pool: PgPool, vault: PortalVault, numbers: &[PoolNumber]) -> Router {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let security = WebSecurity::new(pool.clone(), Arc::clone(&clock));
    let state = Portal {
        pool,
        clock,
        security: security.clone(),
        vault,
        numbers: numbers.into(),
    };
    Router::new()
        .route(BUSINESS_HOME_ROUTE, get(home))
        .route(OPEN_CARDS_ROUTE, post(save_times))
        .route(JOB_RELEASE_ROUTE, get(job_page))
        .route(BILLING_ROUTE, get(billing))
        .route(STATEMENT_ROUTE, get(statement))
        .route(NOTICE_SETTINGS_ROUTE, get(notices).post(save_channel))
        .route_layer(from_fn(note_site))
        .with_state(state)
        .layer(from_fn(no_store))
        .layer(from_fn_with_state(security, refuse_cross_site))
}

/// The routes `build_router` merges: `main`'s own `vault`, else the Twilio webhooks' vault, and
/// the voice hooks' masked pool numbers, else none (BIZ-046).
pub fn served(
    pool: PgPool,
    vault: Option<Arc<Vault>>,
    (webhooks, voice): (Option<&Arc<Webhooks>>, Option<&Arc<VoiceHooks>>),
) -> Router {
    let vault = match (vault, webhooks) {
        (Some(vault), _) => PortalVault::Own(vault),
        (None, Some(hooks)) => PortalVault::Hooks(Arc::clone(hooks)),
        (None, None) => PortalVault::None,
    };
    let numbers = voice.map(|v| v.pool.numbers().to_vec()).unwrap_or_default();
    routes(pool, vault, &numbers)
}

/// The vault `main` passes to `RouterParts::with_vault`, from `VAULT_KEY` and `VAULT_INDEX_KEY`
/// in `pairs` (PRIV-002): `None` with neither in dev and CI.
///
/// # Errors
///
/// A bad or half-set key pair, or none in the test or live project. Never a value.
pub fn vault_from_settings<I>(settings: &Settings, pairs: I) -> io::Result<Option<Vault>>
where
    I: IntoIterator<Item = (String, String)>,
{
    crate::vault_from(settings, pairs)
}

/// Notes the matched route, the request ID and the client address for the handlers.
async fn note_site(mut request: Request, next: Next) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", MatchedPath::as_str)
        .to_owned();
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|id| Uuid::try_parse_ascii(id.as_bytes()).ok());
    let client = security_event_client(&request);
    request.extensions_mut().insert(Site {
        route,
        request_id,
        client,
    });
    next.run(request).await
}

/// SEC-004: the answer to a request with no business person: 401, a link to sign in, no detail.
fn signed_out() -> Response {
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><title>Sign in</title></head><body><main>\
         <h1>Sign in to your business account</h1><p><a href=\"{SIGNIN_ROUTE}\">Sign in</a> to \
         see this page.</p></main></body></html>"
    );
    (StatusCode::UNAUTHORIZED, Html(page)).into_response()
}

/// SEC-019: the one not-found answer, the same for every refusal and unknown ID; it never says
/// which, and links to the portal home.
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
    tracing::warn!(what, "business portal page failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// A 303 to `path` after a stored change.
fn see_other(path: &'static str) -> Response {
    (
        StatusCode::SEE_OTHER,
        [(LOCATION, HeaderValue::from_static(path))],
    )
        .into_response()
}

/// SEC-048: records a `kind` event for `site` with `actor`, under the per-client cap; a failure to
/// record is logged once and never changes the answer.
async fn event(portal: &Portal, site: &Site, kind: SecurityEventKind, actor: Option<Uuid>) {
    let now = portal.clock.now();
    if !admit_security_event(kind, site.client, &site.route, now) {
        return;
    }
    let event = SecurityEvent {
        kind,
        route: &site.route,
        request_id: site.request_id,
        actor_ref: actor,
    };
    let recorded = match portal.pool.acquire().await {
        Ok(mut db) => record_security_event(&mut db, &event, now).await.is_ok(),
        Err(_) => false,
    };
    if !recorded {
        tracing::error!(
            kind = kind.as_str(),
            route = site.route,
            "security event not recorded"
        );
    }
}

/// SEC-019, SEC-048: [`not_found`] for `who`, recorded as an `authz_denied` event.
async fn denied(portal: &Portal, site: &Site, who: &BusinessPrincipal) -> Response {
    event(
        portal,
        site,
        SecurityEventKind::AuthzDenied,
        Some(who.member()),
    )
    .await;
    not_found()
}

/// SEC-004, SEC-006, SEC-024: the signed-in business person, else the answer to send: a session
/// that is not a current business member's is recorded as an `authz_denied` event.
async fn signed_in(
    portal: &Portal,
    site: &Site,
    jar: &CookieJar,
) -> Result<BusinessPrincipal, Box<Response>> {
    let Ok(mut db) = portal.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    match business_principal(&mut db, jar, portal.clock.now()).await {
        Ok(Some(who)) => Ok(who),
        Ok(None) => {
            drop(db);
            if jar.get(SESSION_COOKIE).is_some() {
                event(portal, site, SecurityEventKind::AuthzDenied, None).await;
            }
            Err(Box::new(signed_out()))
        }
        Err(_) => Err(Box::new(failed("business principal"))),
    }
}

/// This Arizona month so far: from the 1st at midnight to `now` (BIZ-015, BIZ-025).
fn this_month(now: Timestamp) -> (Timestamp, Timestamp) {
    let local = arizona_time(now);
    let first = local
        .date()
        .first_of_month()
        .to_zoned(local.time_zone().clone());
    (first.map_or(now, |z| z.timestamp()), now)
}

/// The business's statement for this month, from the ledger (BIZ-015, FEE-030).
async fn month_statement(portal: &Portal, business: Uuid) -> Option<Statement> {
    let month = this_month(portal.clock.now());
    business_statement(&portal.pool, business, month).await.ok()
}

/// BIZ-025, CARD-022, BIZ-046, SEC-027: the home page for `who`; `too_many` shows the times error.
async fn home_html(
    portal: &Portal,
    who: &BusinessPrincipal,
    too_many: bool,
    csrf: &str,
) -> Option<String> {
    let statement = month_statement(portal, who.business()).await?;
    let mut db = portal.pool.acquire().await.ok()?;
    let times = open_cards_times(&mut db, who.member()).await.ok()?;
    let lines = sign_in_lines(&mut db, who.member()).await.ok()?;
    let inbox = InboxSwitch {
        shared: inbox_shared(&mut db, who.member()).await.ok()?,
        text_phone: lines.is_some_and(|l| !l.texting_lines.is_empty()),
    };
    let numbers = YourNumbers {
        fees_cents: u64::try_from(statement.totals.charges).unwrap_or(0),
        ..YourNumbers::default()
    };
    business_home_page(&numbers, (times, too_many), (&portal.numbers, &inbox), csrf).ok()
}

/// `GET` [`BUSINESS_HOME_ROUTE`]: the portal home (BIZ-025, CARD-022). Changes nothing.
async fn home(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
) -> Response {
    let who = match signed_in(&portal, &site, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Some(csrf) = csrf_token(&jar) else {
        return failed("csrf token");
    };
    match home_html(&portal, &who, false, &csrf).await {
        Some(html) => Html(html).into_response(),
        None => failed("home page"),
    }
}

/// The open-cards times form: its CSRF token and each ticked `time` (CARD-022). Any other field
/// name fails the parse, a 422 that stores nothing (SEC-010).
#[derive(Debug, serde::Deserialize)]
#[serde(try_from = "Vec<(String, String)>")]
struct TimesForm {
    csrf: String,
    times: Vec<String>,
}

impl TryFrom<Vec<(String, String)>> for TimesForm {
    type Error = String;

    /// Splits the posted pairs into the token and the ticked times; refuses an unknown field
    /// name (SEC-010).
    fn try_from(pairs: Vec<(String, String)>) -> Result<Self, String> {
        let mut form = Self {
            csrf: String::new(),
            times: Vec::new(),
        };
        for (name, value) in pairs {
            match name.as_str() {
                "csrf" => form.csrf = value,
                "time" => form.times.push(value),
                _ => return Err(format!("unknown field `{name}`")),
            }
        }
        Ok(form)
    }
}

impl CsrfField for TimesForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// `POST` [`OPEN_CARDS_ROUTE`] (CARD-022, SEC-009, SEC-042): stores `who`'s own times and goes
/// home; more than two times or an unknown value stores nothing and shows the form's error (422).
async fn save_times(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<TimesForm>,
) -> Response {
    let who = match signed_in(&portal, &site, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Ok(mut db) = portal.pool.acquire().await else {
        return failed("connection");
    };
    match save_open_cards_times(&mut db, &who, &form.times, portal.clock.now()).await {
        Ok(Some(_)) => see_other(BUSINESS_HOME_ROUTE),
        Ok(None) => {
            drop(db);
            let csrf = csrf_token(&jar).unwrap_or_default();
            match home_html(&portal, &who, true, &csrf).await {
                Some(html) => (StatusCode::UNPROCESSABLE_ENTITY, Html(html)).into_response(),
                None => failed("home page"),
            }
        }
        Err(_) => failed("open-cards times"),
    }
}

/// `GET` [`JOB_RELEASE_ROUTE`] (PICK-022 to PICK-024, BIZ-013, D-1739): the released job's page,
/// read through `released_view` with `who`'s session proof on every load; another business's
/// pick, a member without "Confirm and billing" and an unknown or malformed ID are the one
/// not-found answer, with nothing read from the vault (`released_view` checks first). No `api`
/// reads a pick's job number or call log yet, so the page shows job 0 and no calls (D-3664).
async fn job_page(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
    Path(pick): Path<String>,
) -> Response {
    let who = match signed_in(&portal, &site, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Ok(pick) = Uuid::try_parse(&pick) else {
        return denied(&portal, &site, &who).await;
    };
    let Some(vault) = portal.vault.get() else {
        return failed("no vault");
    };
    let facts = JobFacts {
        job_number: 0,
        pick,
        calls: &[],
    };
    let keys = (&portal.pool, vault, crate::session_minter());
    match job_release_page(keys, &who, facts).await {
        Ok(Some(html)) => Html(html).into_response(),
        Ok(None) => denied(&portal, &site, &who).await,
        Err(_) => failed("job release page"),
    }
}

/// `who` with the "Confirm and billing" role (or the owner) and this month's statement, else the
/// answer to send (BIZ-015, BIZ-023).
async fn billing_facts(
    portal: &Portal,
    site: &Site,
    jar: &CookieJar,
) -> Result<Statement, Box<Response>> {
    let who = signed_in(portal, site, jar).await?;
    if !who.may(who.business(), Need::ConfirmAndBilling) {
        return Err(Box::new(denied(portal, site, &who).await));
    }
    month_statement(portal, who.business())
        .await
        .ok_or_else(|| Box::new(failed("statement")))
}

/// `GET` [`BILLING_ROUTE`] (BIZ-015): this month's charges, refunds and credits from the ledger,
/// with no saved cards and no fee limit until an `api` reads them (D-3618).
async fn billing(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
) -> Response {
    let statement = match billing_facts(&portal, &site, &jar).await {
        Ok(statement) => statement,
        Err(answer) => return *answer,
    };
    match billing_page(&statement, &[], None) {
        Ok(html) => Html(html).into_response(),
        Err(_) => failed("billing page"),
    }
}

/// `GET` [`STATEMENT_ROUTE`] (BIZ-015, D-491): this month's statement as a CSV download.
async fn statement(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
) -> Response {
    let statement = match billing_facts(&portal, &site, &jar).await {
        Ok(statement) => statement,
        Err(answer) => return *answer,
    };
    let headers = [
        (
            CONTENT_TYPE,
            HeaderValue::from_static("text/csv; charset=utf-8"),
        ),
        (
            CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=\"statement.csv\""),
        ),
    ];
    (headers, statement_csv(&statement)).into_response()
}

/// BIZ-036, BIZ-040, SEC-024: the business's notice addresses (each person's, then any returned
/// one), each opened from the vault under `AccountEmail` with its verified and returned state; an
/// address the vault no longer holds is left out (D-3664).
async fn notice_list(portal: &Portal, vault: &Vault, business: Uuid) -> Option<Vec<NoticeAddress>> {
    let mut db = portal.pool.acquire().await.ok()?;
    let purpose = NoticePurpose::PicksAndRelease;
    let mut ids = notice_addresses(&mut db, business, purpose).await.ok()?;
    let returned = returned_mailboxes(&mut db, business).await.ok()?;
    ids.extend(
        returned
            .iter()
            .filter(|id| !ids.contains(id))
            .copied()
            .collect::<Vec<_>>(),
    );
    let mut list = Vec::with_capacity(ids.len());
    for id in ids {
        let verified = may_get_notices(&mut db, business, id).await.ok()?;
        let shown = read_business_mailbox(&mut db, vault, BusinessPurpose::AccountEmail, id);
        let shown = match shown.await {
            Ok(address) => address.expose_secret().to_owned(),
            Err(BusinessContactError::NotFound) => continue,
            Err(_) => return None,
        };
        list.push(NoticeAddress {
            shown,
            verified,
            returned: returned.contains(&id),
        });
    }
    Some(list)
}

/// `GET` [`NOTICE_SETTINGS_ROUTE`] (BIZ-036, BIZ-040, SEC-024, D-1848): the notice addresses
/// and the channel, the form for the owner alone. Changes nothing.
async fn notices(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
) -> Response {
    let who = match signed_in(&portal, &site, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let (Some(vault), Some(csrf)) = (portal.vault.get(), csrf_token(&jar)) else {
        return failed("no vault or csrf token");
    };
    let Some(addresses) = notice_list(&portal, vault, who.business()).await else {
        return failed("notice addresses");
    };
    let Ok(mut db) = portal.pool.acquire().await else {
        return failed("connection");
    };
    let Ok(channel) = notice_channel(&mut db, who.business()).await else {
        return failed("notice channel");
    };
    let facts = ChannelFacts {
        channel,
        texting_on: TEXTING_ON,
    };
    match notice_settings_page(&who, &addresses, facts, &csrf) {
        Ok(html) => Html(html).into_response(),
        Err(_) => failed("notice settings page"),
    }
}

/// The channel form: its CSRF token and the chosen channel (BIZ-036). An unknown field or
/// channel value fails the parse, a 422 that stores nothing (SEC-010); a missing channel parses,
/// so the door's own answer comes first, and is then refused with 422 too.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    channel: Option<ChannelChoice>,
}

/// The three channel values the form offers; any other value is refused (SEC-010).
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum ChannelChoice {
    Email,
    Text,
    Both,
}

impl CsrfField for ChannelForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// `POST` [`NOTICE_SETTINGS_ROUTE`] (BIZ-036, TXT-035, SEC-024, SEC-042): the owner's channel
/// change, stored with its audit row; anyone else is refused, changes nothing and gets the
/// not-found answer with a security event; text or both while texting is off stores nothing.
async fn save_channel(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<ChannelForm>,
) -> Response {
    let who = match signed_in(&portal, &site, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let channel = match form.channel {
        Some(ChannelChoice::Email) => NoticeChannel::Email,
        Some(ChannelChoice::Text) => NoticeChannel::Text,
        Some(ChannelChoice::Both) => NoticeChannel::Both,
        None => return StatusCode::UNPROCESSABLE_ENTITY.into_response(),
    };
    let facts = ChannelFacts {
        channel,
        texting_on: TEXTING_ON,
    };
    let Ok(mut db) = portal.pool.acquire().await else {
        return failed("connection");
    };
    match change_channel(&mut db, &who, facts, portal.clock.now()).await {
        Ok(()) | Err(ChannelChangeError::TextingOff) => see_other(NOTICE_SETTINGS_ROUTE),
        Err(ChannelChangeError::NotOwner) => {
            drop(db);
            denied(&portal, &site, &who).await
        }
        Err(ChannelChangeError::Database) => failed("notice channel"),
    }
}
