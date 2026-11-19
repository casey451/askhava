//! The served business portal (SEC-004, SEC-009, SEC-024, SEC-034, SEC-042, SEC-048, BIZ-013,
//! BIZ-015, BIZ-025, BIZ-036, PICK-024, CARD-022; T300, D-1728, D-1739, D-1848, D-2117, D-3618).
//!
//! [`routes`] serves `GET` [`BUSINESS_HOME_ROUTE`], `POST` [`OPEN_CARDS_ROUTE`], `GET`
//! [`JOB_RELEASE_ROUTE`], `GET` [`BILLING_ROUTE`], `GET` [`STATEMENT_ROUTE`] and `GET` and `POST`
//! [`NOTICE_SETTINGS_ROUTE`] over `portals`' page functions; `build_router` always merges it.
//! Every route first reads the person with `portals::api::business_principal`: no business person
//! (signed out, a homeowner or owner session, a removed member) gets `portals::api::signed_out`,
//! a 401 linking to `portals::api::SIGNIN_ROUTE` with no record detail; a refusal by a signed-in
//! person (another business's pick, a missing role, an unknown ID, a non-owner's channel change)
//! is the one [`not_found`] answer and an `authz_denied` security event (SEC-048). Every response
//! is `no-store` (SEC-034); both `POST`s pass `refuse_cross_site` and the CSRF form check
//! (SEC-009, SEC-042).
//!
//! The facts come through each module's `api`: the home's open-cards times and inbox switch from
//! `businesses` and `portals`, its "Your numbers" fee total from `payments::api::business_statement`
//! for this Arizona month and every other count from `quotes::api::quote_numbers`,
//! `picks::api::pick_numbers` and `jobs::api::finished_counts` (T451); billing from the same
//! statement and `payments::api::saved_cards` (brand and last four only, FEE-014), with no fee
//! limit shown (no `api` reads it yet, D-3618, D-3664); the released job's page through `picks::api::released_view` with the session
//! proof from [`crate::session_minter`] on every load (D-1739); the notice addresses opened from the
//! vault here (D-1848, D-3664). Business texting is off (TXT-035), so the channel is email.
//!
//! T483 (OPS-026, BIZ-052; D-207, D-3753, D-3890): the home also shows the owner's away date
//! (`ops::api::away_until`) on its review and dispute lines and the saved Google and Yelp links
//! (`businesses::api::review_links`); `POST` [`REVIEW_LINKS_ROUTE`] stores them through
//! `portals::api::save_home_links` with `GuardedOpener` (owner only; a link off its site's domain
//! or that does not open is a 422 with the reason, nothing stored).
//!
//! T306 (BIZ-007, BIZ-029, BIZ-045, PICK-057; D-2128, D-2711, D-3112, D-4551): `GET`
//! [`COMPLAINTS_ROUTE`] and `POST` [`COMPLAINT_REPLY_ROUTE`] (owner or "Confirm and billing"; a
//! complaint not on the business's own list is the not-found answer; a reply after 5 days is a
//! 422 that stores nothing), `GET` and `POST` [`EMERGENCY_SWITCH_ROUTE`] (the caller's own person
//! only, `Acting` from the session; the after-hours step through `post_step`), `GET`
//! [`NOTICE_LOG_ROUTE`] (the pick's job from `payments::api::pick_fee`, checked before any vault
//! read) and `POST` [`NOT_A_FIT_ROUTE`] (the session proves the business, a card for the job
//! proves the job; the path's token is never read, since the job page that offers the choice is
//! shown after its Quote link was used, D-4551).
//!
//! T562 (FEE-031, SEC-019; D-3112, D-3890, D-3967, D-4603): `GET` [`WHY_CHARGED_ROUTE`] shows
//! "Why was I charged?" for the caller's own charged pick (owner or "Confirm and billing"): the
//! receipt from `payments::api::receipt_for`, the job number from `jobs::api::job_number`, the
//! quote from `quotes::api::accepted_quote` and the fee basis derived from that quote's price
//! through `payments::api::period_fee_for` on the job's fee facts, as `picks` fixed it. A derived
//! tier other than the fee fixed at the pick (`payments::api::pick_fee`) shows no basis line; a
//! quote that no longer reads back is the not-found answer. The page reads nothing from the vault.

use std::io;
use std::net::IpAddr;
use std::sync::Arc;

use askhava_adapters::api::PoolNumber;
use askhava_businesses::api::{
    Acting, Complaint, ComplaintError, EMERGENCY_YES_LIFE, GuardedOpener, LinkRefusal,
    NoticeChannel, NoticePurpose, SwitchError, after_hours_box, complaints, may_get_notices,
    notice_addresses, notice_channel, open_cards_times, returned_mailboxes, review_links,
    sign_in_lines, switch_off_warns, switch_state,
};
use askhava_core::api::{Clock, Settings, SystemClock, arizona_time};
use askhava_jobs::api::{finished_counts, job_facts, job_number};
use askhava_messaging::api::still_away;
use askhava_ops::api::away_until;
use askhava_payments::api::{
    Fee, FeeQuote, JobCodes, JobFeeFacts, Period as FeePeriod, Statement, StoredPickFee, Tier,
    business_statement, period_fee_for, pick_fee, receipt_for, resolve_fee_facts, saved_cards,
};
use askhava_picks::api::pick_numbers;
use askhava_portals::api::{
    ACTION_ROUTE, ActionKind, AfterHoursBox, BILLING_ROUTE, BUSINESS_HOME_ROUTE, Basis,
    BusinessPrincipal, COMPLAINT_REPLY_ROUTE, COMPLAINTS_ROUTE, ChannelChangeError, ChannelFacts,
    CookieJar, CsrfField, CsrfForm, EMERGENCY_SWITCH_ROUTE, HomeExtras, InboxSwitch,
    JOB_RELEASE_ROUTE, JobFacts, LinksChangeError, NOT_A_FIT_ROUTE, NOTICE_SETTINGS_ROUTE, Need,
    NotAFitChoice, NotAFitOutcome, NotAFitState, NoticeAddress, NoticeLogPage, OPEN_CARDS_ROUTE,
    PaymentMethod, REVIEW_LINKS_ROUTE, RefusedReply, SESSION_COOKIE, STATEMENT_ROUTE,
    SecurityEvent, SecurityEventKind, SwitchStep, WHY_CHARGED_ROUTE, WebSecurity, WhyCharged,
    YourNumbers, admit_security_event, answer_not_a_fit, billing_page, business_home_page_with,
    business_principal, change_channel, complaints_page, csrf_token,
    emergency_switch_page_with_box, inbox_shared, job_release_page, no_store, not_a_fit_action,
    not_a_fit_page, notice_log_page, notice_settings_page, parse_reason, post_step,
    record_security_event, refuse_cross_site, reply_to_complaint, save_home_links,
    save_open_cards_times, security_event_client, signed_out, statement_csv, why_charged_page,
    why_charged_page_without_basis,
};
use askhava_privacy::api::{BusinessContactError, BusinessPurpose, Vault, read_business_mailbox};
use askhava_quotes::api::{Period, Price, accepted_quote, card_holders, quote_numbers};
use axum::Router;
use axum::extract::{Extension, FromRef, MatchedPath, Path, Request, State};
use axum::http::StatusCode;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, HeaderValue, LOCATION};
use axum::middleware::{Next, from_fn, from_fn_with_state};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use jiff::Timestamp;
use secrecy::ExposeSecret;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

use crate::inbound_text::Webhooks;
use crate::inbound_voice::VoiceHooks;

/// BIZ-045, D-3112: the job page's notice log, under the caller's own pick (the pick gives the job
/// and its number, since the door probe fills `{pick}` and no `{job}`).
pub const NOTICE_LOG_ROUTE: &str = "/b/picks/{pick}/notices";

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
/// `numbers` on the home page's calls card (BIZ-046), behind the cross-site refusal and, outside
/// it, `no-store`, so a refusal's 403 is `no-store` too (SEC-034, SEC-042, TD-SEC-49).
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
        .route(REVIEW_LINKS_ROUTE, post(save_links))
        .route(JOB_RELEASE_ROUTE, get(job_page))
        .route(BILLING_ROUTE, get(billing))
        .route(STATEMENT_ROUTE, get(statement))
        .route(NOTICE_SETTINGS_ROUTE, get(notices).post(save_channel))
        .route(NOTICE_LOG_ROUTE, get(notice_log))
        .route(WHY_CHARGED_ROUTE, get(why_charged))
        .route(COMPLAINTS_ROUTE, get(complaints_get))
        .route(COMPLAINT_REPLY_ROUTE, post(complaint_reply))
        .route(EMERGENCY_SWITCH_ROUTE, get(switch_get).post(switch_post))
        .route(NOT_A_FIT_ROUTE, post(not_a_fit))
        .route_layer(from_fn(note_site))
        .with_state(state)
        .layer(from_fn_with_state(security, refuse_cross_site))
        .layer(from_fn(no_store))
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

/// BIZ-025 (D-193, D-250, D-284, D-331): the panel's numbers for `business` this month, from
/// `quotes`', `picks`' and `jobs`' readers and the month's statement (T451).
async fn your_numbers(
    db: &mut PgConnection,
    business: Uuid,
    month: (Timestamp, Timestamp),
    statement: &Statement,
) -> Option<YourNumbers> {
    let quotes = quote_numbers(db, business, month).await.ok()?;
    let picks = pick_numbers(db, business, month).await.ok()?;
    let finished = finished_counts(db, &picks.connected, month).await.ok()?;
    Some(YourNumbers {
        cards: quotes.cards,
        cards_without_quote: quotes.cards_without_quote,
        quotes: quotes.quotes,
        picks: picks.picks,
        confirms: picks.confirms,
        fees_cents: u64::try_from(statement.totals.charges).unwrap_or(0),
        lapsed_picks: picks.lapsed,
        finished: (finished.done, finished.no, finished.other),
        typical_quotes: quotes.typical_quotes,
    })
}

/// The links just refused, as sent, and why (BIZ-052).
type Refused<'a> = Option<(&'a LinksForm, LinkRefusal)>;

/// BIZ-025, CARD-022, BIZ-046, SEC-027, OPS-026, BIZ-052: the home page for `who`; `too_many`
/// shows the times error and `refused` the links error with the links as sent.
async fn home_html(
    portal: &Portal,
    who: &BusinessPrincipal,
    (too_many, refused): (bool, Refused<'_>),
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
    let month = this_month(portal.clock.now());
    let numbers = your_numbers(&mut db, who.business(), month, &statement).await?;
    let saved = review_links(&mut db, who.business()).await.ok()?;
    let extras = HomeExtras {
        away_until: still_away(away_until(&mut db).await.ok()?, portal.clock.now()),
        google: refused.map_or(saved.google(), |(f, _)| Some(f.google.as_str())),
        yelp: refused.map_or(saved.yelp(), |(f, _)| Some(f.yelp.as_str())),
        refused: refused.map(|(_, r)| r),
    };
    let pool = &portal.numbers;
    business_home_page_with(&numbers, (times, too_many), (pool, &inbox), &extras, csrf).ok()
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
    match home_html(&portal, &who, (false, None), &csrf).await {
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
            match home_html(&portal, &who, (true, None), &csrf).await {
                Some(html) => (StatusCode::UNPROCESSABLE_ENTITY, Html(html)).into_response(),
                None => failed("home page"),
            }
        }
        Err(_) => failed("open-cards times"),
    }
}

/// The Google and Yelp links form (BIZ-052): its CSRF token and both links; any other field is
/// refused (SEC-010).
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LinksForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    google: String,
    #[serde(default)]
    yelp: String,
}

impl CsrfField for LinksForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// `POST` [`REVIEW_LINKS_ROUTE`] (BIZ-052, SEC-041, SEC-009, SEC-042, D-439): the owner's Google
/// and Yelp links, checked and stored, then home; a refused link stores nothing and shows its
/// reason (422); anyone but the owner gets the not-found answer with a security event.
async fn save_links(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<LinksForm>,
) -> Response {
    let who = match signed_in(&portal, &site, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Ok(mut db) = portal.pool.acquire().await else {
        return failed("connection");
    };
    let links = (Some(form.google.as_str()), Some(form.yelp.as_str()));
    let refusal = match save_home_links(&mut db, &GuardedOpener, &who, links).await {
        Ok(_) => return see_other(BUSINESS_HOME_ROUTE),
        Err(LinksChangeError::Role) => return denied(&portal, &site, &who).await,
        Err(LinksChangeError::Database) => return failed("review links"),
        Err(LinksChangeError::Refused(refusal)) => refusal,
    };
    drop(db);
    let csrf = csrf_token(&jar).unwrap_or_default();
    match home_html(&portal, &who, (false, Some((&form, refusal))), &csrf).await {
        Some(html) => (StatusCode::UNPROCESSABLE_ENTITY, Html(html)).into_response(),
        None => failed("home page"),
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
) -> Result<(Statement, Uuid), Box<Response>> {
    let who = signed_in(portal, site, jar).await?;
    if !who.may(who.business(), Need::ConfirmAndBilling) {
        return Err(Box::new(denied(portal, site, &who).await));
    }
    month_statement(portal, who.business())
        .await
        .map(|statement| (statement, who.business()))
        .ok_or_else(|| Box::new(failed("statement")))
}

/// FEE-014, BIZ-015: `business`'s saved cards not removed, brand and last four only (T451).
async fn card_methods(portal: &Portal, business: Uuid) -> Option<Vec<PaymentMethod>> {
    let mut db = portal.pool.acquire().await.ok()?;
    let cards = saved_cards(&mut db, business).await.ok()?;
    let methods = cards.into_iter().map(|c| PaymentMethod {
        brand: c.brand,
        last4: c.last4,
    });
    Some(methods.collect())
}

/// `GET` [`BILLING_ROUTE`] (BIZ-015, FEE-014): this month's charges, refunds and credits from the
/// ledger and the saved cards' brand and last four (T451), with no fee limit until an `api` reads
/// it (D-3618).
async fn billing(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
) -> Response {
    let (statement, business) = match billing_facts(&portal, &site, &jar).await {
        Ok(facts) => facts,
        Err(answer) => return *answer,
    };
    let Some(cards) = card_methods(&portal, business).await else {
        return failed("saved cards");
    };
    match billing_page(&statement, &cards, None) {
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
        Ok((statement, _)) => statement,
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

/// BIZ-029, SEC-019: `who`, with the owner or "Confirm and billing" role, and the business's own
/// complaints, else the answer to send (D-4551).
async fn complaint_facts(
    portal: &Portal,
    site: &Site,
    jar: &CookieJar,
) -> Result<(BusinessPrincipal, Vec<Complaint>), Box<Response>> {
    let who = signed_in(portal, site, jar).await?;
    if !who.may(who.business(), Need::ConfirmAndBilling) {
        return Err(Box::new(denied(portal, site, &who).await));
    }
    let Ok(mut db) = portal.pool.acquire().await else {
        return Err(Box::new(failed("connection")));
    };
    match complaints(&mut db, who.business()).await {
        Ok(list) => Ok((who, list)),
        Err(_) => Err(Box::new(failed("complaints"))),
    }
}

/// `GET` [`COMPLAINTS_ROUTE`] (BIZ-029, D-200): each complaint's kind and date, no homeowner
/// detail, the reply box while it is open. Changes nothing.
async fn complaints_get(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
) -> Response {
    let list = match complaint_facts(&portal, &site, &jar).await {
        Ok((_, list)) => list,
        Err(answer) => return *answer,
    };
    let Some(csrf) = csrf_token(&jar) else {
        return failed("csrf token");
    };
    match complaints_page(&list, (portal.clock.now(), None), &csrf) {
        Ok(html) => Html(html).into_response(),
        Err(_) => failed("complaints page"),
    }
}

/// The reply form: its CSRF token and the reply's words (BIZ-029); any other field is refused
/// (SEC-010). No `Debug`: the words are the business's own (rust-production R6).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplyForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    reply: String,
}

impl CsrfField for ReplyForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// `POST` [`COMPLAINT_REPLY_ROUTE`] (BIZ-029, SEC-009, SEC-042, SEC-019): the business's one
/// reply, stored before the box closes at exactly 5 days; a complaint not on its own list is the
/// not-found answer; an empty, too long or late reply stores nothing and shows why (422).
async fn complaint_reply(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
    Path(complaint): Path<String>,
    CsrfForm(form): CsrfForm<ReplyForm>,
) -> Response {
    let (who, list) = match complaint_facts(&portal, &site, &jar).await {
        Ok(facts) => facts,
        Err(answer) => return *answer,
    };
    let id = Uuid::try_parse(&complaint).ok();
    let Some(id) = id.filter(|id| list.iter().any(|c| c.id == *id)) else {
        return denied(&portal, &site, &who).await;
    };
    let (now, Ok(mut db)) = (portal.clock.now(), portal.pool.acquire().await) else {
        return failed("connection");
    };
    let error = match reply_to_complaint(&mut db, &who, (id, &form.reply), now).await {
        Ok(()) => return see_other(COMPLAINTS_ROUTE),
        Err(ComplaintError::Database | ComplaintError::Audit) => return failed("complaint reply"),
        Err(error) => error,
    };
    let refused = RefusedReply {
        complaint: id,
        error,
        words: &form.reply,
    };
    let csrf = csrf_token(&jar).unwrap_or_default();
    match complaints_page(&list, (now, Some(refused)), &csrf) {
        Ok(html) => (StatusCode::UNPROCESSABLE_ENTITY, Html(html)).into_response(),
        Err(_) => failed("complaints page"),
    }
}

/// The signed-in person as `businesses` names who acts: never a person from the browser.
fn acting(who: &BusinessPrincipal) -> Acting {
    Acting {
        business: who.business(),
        member: who.member(),
    }
}

/// BIZ-007, BIZ-042: the switch page for `by` now, with the after-hours box for the owner or a
/// "Confirm and billing" member.
async fn switch_html(
    db: &mut PgConnection,
    by: Acting,
    csrf: &str,
    now: Timestamp,
) -> Result<String, SwitchError> {
    let state = switch_state(db, by, (now, EMERGENCY_YES_LIFE)).await?;
    let warns = state.on && switch_off_warns(db, by).await?;
    let after_hours = after_hours_box(db, by).await?.map(|t| AfterHoursBox {
        ticked: t.ticked,
        refused: false,
        suspended: t.suspended,
    });
    emergency_switch_page_with_box(state, (warns, false), after_hours, csrf)
        .map_err(|_| SwitchError::Database)
}

/// `GET` [`EMERGENCY_SWITCH_ROUTE`] (BIZ-007, BIZ-037, BIZ-042): the caller's own switch. Changes
/// nothing.
async fn switch_get(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
) -> Response {
    let who = match signed_in(&portal, &site, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let (Some(csrf), Ok(mut db)) = (csrf_token(&jar), portal.pool.acquire().await) else {
        return failed("csrf token or connection");
    };
    match switch_html(&mut db, acting(&who), &csrf, portal.clock.now()).await {
        Ok(html) => Html(html).into_response(),
        Err(_) => failed("emergency switch page"),
    }
}

/// The switch form: its CSRF token and the step (BIZ-007, BIZ-042); any other field is refused
/// (SEC-010).
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StepForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    step: String,
}

impl CsrfField for StepForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// `POST` [`EMERGENCY_SWITCH_ROUTE`] (BIZ-007, BIZ-037, BIZ-042, SEC-042, D-2711): one step for the
/// caller's own person (`Acting` from the session); the after-hours step by anyone but the owner
/// or a "Confirm and billing" member is the not-found answer; an unknown step is a 422; a Yes
/// before a test changes nothing and shows the page again.
async fn switch_post(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
    CsrfForm(form): CsrfForm<StepForm>,
) -> Response {
    let who = match signed_in(&portal, &site, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Some(step) = SwitchStep::from_form(&form.step) else {
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    };
    let Ok(mut db) = portal.pool.acquire().await else {
        return failed("connection");
    };
    let csrf = csrf_token(&jar).unwrap_or_default();
    match post_step(&mut db, acting(&who), (step, &csrf), portal.clock.now()).await {
        Ok(html) => Html(html).into_response(),
        Err(SwitchError::NotAllowed | SwitchError::UnknownMember) => {
            drop(db);
            denied(&portal, &site, &who).await
        }
        Err(SwitchError::NoTest | SwitchError::NoCover) => see_other(EMERGENCY_SWITCH_ROUTE),
        Err(SwitchError::Database) => failed("emergency switch"),
    }
}

/// BIZ-045, SEC-019: `raw`'s job when it is a pick of `who`'s business (`pick_fee`, as the Confirm
/// page reads it), else `None`; read before anything from the vault.
async fn own_pick_job(
    db: &mut PgConnection,
    who: &BusinessPrincipal,
    raw: &str,
) -> Result<Option<Uuid>, ()> {
    let Ok(pick) = Uuid::try_parse(raw) else {
        return Ok(None);
    };
    let fee = pick_fee(db, pick).await.map_err(|_| ())?;
    Ok(fee
        .filter(|f| who.may(f.business.0, Need::Member))
        .map(|f| f.job))
}

/// BIZ-045, SEC-024: every notice address of `business` (each purpose's, then any returned one)
/// by mailbox ID, opened from the vault; an address the vault no longer holds is left out.
async fn log_addresses(
    portal: &Portal,
    vault: &Vault,
    business: Uuid,
) -> Option<Vec<(Uuid, String)>> {
    let mut db = portal.pool.acquire().await.ok()?;
    let mut ids: Vec<Uuid> = returned_mailboxes(&mut db, business).await.ok()?;
    for purpose in [
        NoticePurpose::Cards,
        NoticePurpose::PicksAndRelease,
        NoticePurpose::Account,
        NoticePurpose::Money,
    ] {
        ids.extend(notice_addresses(&mut db, business, purpose).await.ok()?);
    }
    ids.sort_unstable();
    ids.dedup();
    let mut list = Vec::with_capacity(ids.len());
    for id in ids {
        match read_business_mailbox(&mut db, vault, BusinessPurpose::AccountEmail, id).await {
            Ok(address) => list.push((id, address.expose_secret().to_owned())),
            Err(BusinessContactError::NotFound) => {}
            Err(_) => return None,
        }
    }
    Some(list)
}

/// `GET` [`NOTICE_LOG_ROUTE`] (BIZ-045, OPS-037, SEC-019, D-3112): the notice log of the job of
/// the caller's own pick; another business's or an unknown pick is the not-found answer with
/// nothing read from the vault. Changes nothing but the vault's read audit.
async fn notice_log(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
    Path(pick): Path<String>,
) -> Response {
    let who = match signed_in(&portal, &site, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Ok(mut db) = portal.pool.acquire().await else {
        return failed("connection");
    };
    let job = match own_pick_job(&mut db, &who, &pick).await {
        Ok(Some(job)) => job,
        Ok(None) => {
            drop(db);
            return denied(&portal, &site, &who).await;
        }
        Err(()) => return failed("pick fee"),
    };
    let (Some(vault), Ok(number)) = (portal.vault.get(), job_number(&mut db, job).await) else {
        return failed("no vault or job number");
    };
    let Some(addresses) = log_addresses(&portal, vault, who.business()).await else {
        return failed("notice addresses");
    };
    let number = u32::try_from(number).unwrap_or(0);
    let page = notice_log_page(&mut db, who.business(), (job, number), &addresses).await;
    match page {
        Ok(NoticeLogPage::Shown(html)) => Html(html).into_response(),
        Ok(NoticeLogPage::NotFound) => {
            drop(db);
            denied(&portal, &site, &who).await
        }
        Err(_) => failed("notice log page"),
    }
}

/// FEE-031, FEE-044, REC-002, D-3890: the fee inputs of a stored `price`, as `picks` reads them at
/// the pick (a diagnostic visit or an hourly rate with no hours priced as a visit); none for
/// packages (which one was picked is not read here) or a per-visit price, whose visit schedule
/// is not read here either.
fn fee_quote(price: &Price) -> Option<(FeeQuote, Option<FeePeriod>)> {
    Some(match price {
        Price::Firm(cents) => (FeeQuote::Fixed(*cents), None),
        Price::Range { low, high, .. } => (
            FeeQuote::Range {
                low: *low,
                high: *high,
            },
            None,
        ),
        Price::Hourly { rate, hours } => (
            FeeQuote::Hourly {
                rate: *rate,
                hours: *hours,
            },
            None,
        ),
        Price::VisitFee(fee) => (FeeQuote::DiagnosticVisit { visit_fee: *fee }, None),
        Price::Monthly { cents, period, .. } => {
            let period = match period {
                Period::Week => FeePeriod::Weekly,
                Period::Month => FeePeriod::Monthly,
                Period::Quarter => FeePeriod::Quarterly,
                Period::Year => FeePeriod::Yearly,
                Period::Visit => return None,
            };
            (FeeQuote::Fixed(*cents), Some(period))
        }
        Price::Tiers(_) => return None,
    })
}

/// FEE-031, FEE-003, FEE-004, FEE-044, D-3890: the basis line's [`Basis`] and the tier it gives
/// for `price` on a job with fee `facts`: no basis (a set-tier visit) is [`Basis::OnSite`], a
/// priced quote with a period [`Basis::FourMonths`], any other [`Basis::Quote`]; none when the
/// price gives no charged fee.
fn derived_basis(price: &Price, facts: JobFeeFacts) -> Option<(Basis, Tier)> {
    let (quote, period) = fee_quote(price)?;
    let visit = matches!(
        quote,
        FeeQuote::DiagnosticVisit { .. } | FeeQuote::Hourly { hours: None, .. }
    );
    let job = facts.period_job(visit, period.is_some());
    let Ok(Fee::Charged { basis, tier }) = period_fee_for(quote, period, job) else {
        return None;
    };
    let basis = match (basis, period) {
        (None, _) => Basis::OnSite,
        (Some(cents), Some(_)) => Basis::FourMonths(cents),
        (Some(cents), None) => Basis::Quote(cents),
    };
    Some((basis, tier))
}

/// FEE-005, FEE-031, D-2331, D-4603: the basis to show for `price` on `job`, only when its tier's
/// fee is the fee `fixed` at the pick; the job's fee facts as `picks` read them (unsettled facts
/// are the emergency default, the only facts an unsettled pick was fixed on).
async fn shown_basis(
    db: &mut PgConnection,
    (job, price): (Uuid, &Price),
    fixed: &StoredPickFee,
) -> Result<Option<Basis>, &'static str> {
    let row = job_facts(db, job).await.map_err(|_| "job facts")?;
    let codes = row.as_ref().map(|r| JobCodes {
        category: &r.category,
        job_types: &r.job_types,
    });
    let facts = resolve_fee_facts(db, codes)
        .await
        .map_err(|_| "fee facts")?
        .unwrap_or(JobFeeFacts::EMERGENCY_DEFAULT);
    Ok(derived_basis(price, facts)
        .filter(|(_, tier)| fixed.charged && tier.cents() == fixed.cents)
        .map(|(basis, _)| basis))
}

/// FEE-031, SEC-019, D-3890: the page facts of `raw` when it is a charged pick of `who`'s
/// business and `who` is its owner or a "Confirm and billing" member, with whether the basis line
/// shows; `None` for any other pick or a quote that no longer reads back.
async fn charge_facts(
    db: &mut PgConnection,
    who: &BusinessPrincipal,
    raw: &str,
) -> Result<Option<(WhyCharged, bool)>, &'static str> {
    let Ok(pick) = Uuid::try_parse(raw) else {
        return Ok(None);
    };
    let fixed = pick_fee(db, pick).await.map_err(|_| "pick fee")?;
    let Some(fixed) = fixed.filter(|f| who.may(f.business.0, Need::ConfirmAndBilling)) else {
        return Ok(None);
    };
    let receipt = receipt_for(db, pick, who.business()).await;
    let Some(receipt) = receipt.map_err(|_| "receipt")? else {
        return Ok(None);
    };
    let job = receipt.job;
    let job_number = job_number(db, job).await.map_err(|_| "job number")?;
    let accepted = accepted_quote(db, (job, who.business())).await;
    let Some(quote) = accepted.map_err(|_| "accepted quote")? else {
        return Ok(None);
    };
    let basis = shown_basis(db, (job, &quote.price), &fixed).await?;
    let facts = WhyCharged {
        receipt,
        job_number,
        quote: format!("{} ({})", quote.amount(false), quote.kind(false)),
        basis: basis.unwrap_or(Basis::OnSite),
    };
    Ok(Some((facts, basis.is_some())))
}

/// `GET` [`WHY_CHARGED_ROUTE`] (FEE-031, SEC-019, SEC-034, D-3112, D-3890): "Why was I charged?"
/// for the caller's own charged pick; another business's pick, an uncharged or unknown one, a
/// missing role or a quote that no longer reads back is the not-found answer. Changes nothing.
async fn why_charged(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
    Path(pick): Path<String>,
) -> Response {
    let who = match signed_in(&portal, &site, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let Ok(mut db) = portal.pool.acquire().await else {
        return failed("connection");
    };
    let (facts, with_basis) = match charge_facts(&mut db, &who, &pick).await {
        Ok(Some(found)) => found,
        Ok(None) => {
            drop(db);
            return denied(&portal, &site, &who).await;
        }
        Err(what) => return failed(what),
    };
    let page = if with_basis {
        why_charged_page(&facts)
    } else {
        why_charged_page_without_basis(&facts)
    };
    page.map_or_else(
        |_| failed("why charged page"),
        |html| Html(html).into_response(),
    )
}

/// The "Not a fit for us" form: its CSRF token and the tapped reason (PICK-057); any other field
/// is refused (SEC-010); a missing or unknown reason shows the reasons again.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FitForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    reason: Option<String>,
}

impl CsrfField for FitForm {
    fn csrf(&self) -> &str {
        &self.csrf
    }
}

/// PICK-057: the page after a tap on `job` (number `number`), linking back to its Quote link page.
fn fit_page(state: NotAFitState<'_>, number: i64, (job, token): (Uuid, &str)) -> Response {
    let back = (ACTION_ROUTE.replace("{kind}", ActionKind::Quote.slug()))
        .replace("{object}", &job.to_string())
        .replace("{token}", token);
    let status = match state {
        NotAFitState::Refused(_) => StatusCode::UNPROCESSABLE_ENTITY,
        NotAFitState::Saved(_) | NotAFitState::AlreadyAnswered => StatusCode::OK,
    };
    match not_a_fit_page(state, &number.to_string(), &back) {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => failed("not a fit page"),
    }
}

/// `POST` [`NOT_A_FIT_ROUTE`] (PICK-057, OPS-039, SEC-004, SEC-019, SEC-042, D-4551): the signed-in
/// owner or "Cards and quotes" member's answer for a job its business holds a card for, stored
/// through `answer_not_a_fit` (no routing or fee change); any other job is the not-found answer.
async fn not_a_fit(
    State(portal): State<Portal>,
    Extension(site): Extension<Site>,
    jar: CookieJar,
    Path((object, token)): Path<(String, String)>,
    CsrfForm(form): CsrfForm<FitForm>,
) -> Response {
    let who = match signed_in(&portal, &site, &jar).await {
        Ok(who) => who,
        Err(answer) => return *answer,
    };
    let (job, Ok(mut db)) = (Uuid::try_parse(&object).ok(), portal.pool.acquire().await) else {
        return failed("connection");
    };
    let holds = match job {
        Some(job) => card_holders(&mut db, job)
            .await
            .map(|h| h.contains(&who.business())),
        None => Ok(false),
    };
    let (Some(job), Ok(true)) = (
        job.filter(|_| who.may(who.business(), Need::CardsAndQuotes)),
        holds,
    ) else {
        drop(db);
        return denied(&portal, &site, &who).await;
    };
    let Ok(number) = job_number(&mut db, job).await else {
        return failed("job number");
    };
    let Some(reason) = parse_reason(form.reason.as_deref()) else {
        let (action, csrf) = (
            not_a_fit_action(job, &token),
            csrf_token(&jar).unwrap_or_default(),
        );
        let choice = NotAFitChoice {
            action: &action,
            csrf: &csrf,
        };
        return fit_page(NotAFitState::Refused(choice), number, (job, &token));
    };
    let state =
        match answer_not_a_fit(&mut db, (job, who.business()), reason, portal.clock.now()).await {
            Ok(NotAFitOutcome::Saved) => NotAFitState::Saved(reason),
            Ok(NotAFitOutcome::AlreadyAnswered) => NotAFitState::AlreadyAnswered,
            Ok(NotAFitOutcome::NoCard) => return denied(&portal, &site, &who).await,
            Err(_) => return failed("not a fit"),
        };
    fit_page(state, number, (job, &token))
}
