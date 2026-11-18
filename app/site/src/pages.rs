//! The home and how-it-works pages, in English and the how-it-works page in Spanish (T102; WEB-001,
//! WEB-003 to WEB-005, WEB-009, WEB-012, WEB-021, WEB-022, WEB-026, WEB-028, WORD-002).
//!
//! [`page_routes`] serves `/`, `/how-it-works` and `/es/how-it-works`, and answers the old
//! directory site's own paths, `/categories` and `/events`, with the home page (200, WEB-012,
//! D-3144); there is no catch-all, so any other unknown path stays a 404. Every page extends
//! `templates/site_layout.html` (its footer, WEB-003, WEB-004, WORD-002) and sets `banner_shown`
//! first (WEB-007). The home page's numbers come only from `intake::api::home_numbers` (the card
//! and quote records, WEB-028, D-466) and show nothing below [`SitePages`]' threshold. The served
//! `/` keeps the numbers (never the HTML) for [`HOME_NUMBERS_WINDOW`] in a [`HomeNumbersCache`]
//! held by the router, and reads the banner and, on a miss, the numbers on one pooled connection
//! (D-4098); [`home_html`] reads them uncached. The fee
//! amounts come from `payments::api::Tier` (WEB-026). GET only; nothing is written.
//!
//! [`help_routes`] (T201) serves `/services` (one `#<category code>` section per switched-on
//! trade, WEB-015), `/coverage` (WEB-023), both read from `catalog::api::category_list`
//! (D-3879), `/help` (WEB-008, WEB-025, WORD-009, AI-018; the AI switch read through
//! `intake::api::ai_on`, D-3144) and `/help/roc-complaint` (WEB-011). The help form posts to
//! `POST /help`, which T309 serves (D-2128); its CSRF token comes from [`HelpFormCsrf`] (D-3890).
//!
//! [`page_routes`] also serves the Spanish home `/es` (T202, WEB-002, WORD-013), the business page
//! `/business` and its Spanish twin `/es/business` (WEB-018 to WEB-021, WEB-024, WEB-025, WORD-017,
//! BIZ-030, REC-009) and the business help page `/business/help` (WEB-020's safety tip). The
//! business page shows one fake sample card per switched-on trade from `catalog::api::category_list`,
//! the city-license guide ([`CityGuide`]) on no-license cards, and each switched-off trade's
//! waitlist count from `businesses::api::trade_count` with the nil business ID (D-4044). Its
//! WEB-024 line follows `texting_on`, passed as `false` as the served sign-up does until T526 wires
//! the stored switch (D-3880, D-3881).
//!
//! [`page_routes`] also serves the accessibility statement at `/accessibility` (T115, A11Y-020,
//! D-3155): `docs/legal/accessibility-statement.md`, rendered once by
//! `legal_pages::accessibility_statement` when the router is built, never a third copy.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use askama::Template;
use askhava_businesses::api::trade_count;
use askhava_catalog::api::{CategoryListing, category_list};
use askhava_core::api::{Environment, Settings, arizona_time, brand};
use askhava_payments::api::Tier;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use jiff::SignedDuration;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::legal_pages::{LegalDoc, accessibility_statement};
use crate::routes::{banner_shown, banner_shown_on};

/// The home page (WEB-001).
pub const HOME_PATH: &str = "/";
/// The how-it-works page (WEB-026, D-3878).
pub const HOW_IT_WORKS_PATH: &str = "/how-it-works";
/// The Spanish how-it-works page (WEB-002, WEB-026, D-3878).
pub const ES_HOW_IT_WORKS_PATH: &str = "/es/how-it-works";
/// The old directory site's categories path, answered with the home page (WEB-012, D-3144).
pub const OLD_CATEGORIES_PATH: &str = "/categories";
/// The old directory site's events path, answered with the home page (WEB-012, D-3144).
pub const OLD_EVENTS_PATH: &str = "/events";
/// The services page, one section per switched-on trade (WEB-015, D-3878, D-3879).
pub const SERVICES_PATH: &str = "/services";
/// The "What we cover now" page (WEB-023, D-3879).
pub const COVERAGE_PATH: &str = "/coverage";
/// The help page (WEB-008, D-3878).
pub const HELP_PATH: &str = "/help";
/// The page that links on to the state's contractor complaint form (WEB-011).
pub const ROC_COMPLAINT_PATH: &str = "/help/roc-complaint";
/// The Spanish home page (WEB-002, D-3880).
pub const ES_HOME_PATH: &str = "/es";
/// The business page (WEB-018, WEB-020, WEB-021, D-3880).
pub const BUSINESS_PATH: &str = "/business";
/// The Spanish business page (WEB-019, D-3880).
pub const ES_BUSINESS_PATH: &str = "/es/business";
/// The business help page with the safety tip for people working alone (WEB-020, D-3880).
pub const BUSINESS_HELP_PATH: &str = "/business/help";
/// The accessibility statement (A11Y-020, D-3155).
pub const ACCESSIBILITY_PATH: &str = "/accessibility";

/// D-3890: the help form's CSRF token, which `server`'s layer on `GET /help` puts in the request's
/// extensions (T309); the page renders it as the hidden `csrf` field, empty when absent. No
/// `Debug`, so the token cannot reach a log (R6).
#[derive(Clone)]
pub struct HelpFormCsrf(pub String);

/// WEB-021, WEB-022, D-3890: the "Who runs" block's optional facts, each left out while unset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WhoRuns {
    /// The LLC's mailing address (`ASKHAVA_POSTAL_ADDRESS`).
    pub mailing_address: Option<String>,
    /// The LLC's public record on the Arizona Corporation Commission site.
    pub record_url: Option<String>,
    /// The Lake Havasu City business license number.
    pub city_license: Option<String>,
}

impl WhoRuns {
    /// WEB-021, WEB-022, D-3890: the block's facts read from `settings`.
    #[must_use]
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            mailing_address: set(settings.postal_address().ok()),
            record_url: set(settings.llc_record_url()),
            city_license: set(settings.city_business_license_number()),
        }
    }
}

/// A setting's value, or `None` while it is unset or empty (WEB-020, WEB-021, D-3890).
fn set(value: Option<&str>) -> Option<String> {
    value.filter(|v| !v.is_empty()).map(str::to_owned)
}

/// WEB-020, D-3880: the city business-license guide's parts, each left out while its setting is
/// empty; the whole guide is hidden while all three are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CityGuide {
    /// What the license costs (`ASKHAVA_CITY_LICENSE_COST`).
    pub cost: Option<String>,
    /// How long it takes to get (`ASKHAVA_CITY_LICENSE_TIME`).
    pub time: Option<String>,
    /// The city's page for it (`ASKHAVA_CITY_LICENSE_URL`).
    pub url: Option<String>,
}

impl CityGuide {
    /// WEB-020, D-3880: the guide's parts read from `settings`.
    #[must_use]
    pub fn from_settings(settings: &Settings) -> Self {
        let [cost, time, url] = settings.city_license_guide().map(set);
        Self { cost, time, url }
    }

    /// WEB-020: whether any part is set, so the guide shows.
    fn shown(&self) -> bool {
        self.cost.is_some() || self.time.is_some() || self.url.is_some()
    }
}

/// The site pages' settings: WEB-028's threshold and the "Who runs" facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitePages {
    min_jobs: u32,
    who: WhoRuns,
    guide: CityGuide,
}

impl Default for SitePages {
    /// WEB-028's default threshold of 50 (D-468) and no optional "Who runs" fact.
    fn default() -> Self {
        Self {
            min_jobs: 50,
            who: WhoRuns::default(),
            guide: CityGuide::default(),
        }
    }
}

impl SitePages {
    /// WEB-020, WEB-021, WEB-028: the pages' settings from `settings` (D-3878, D-3880, D-3890).
    #[must_use]
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            min_jobs: settings.home_numbers_min_jobs(),
            who: WhoRuns::from_settings(settings),
            guide: CityGuide::from_settings(settings),
        }
    }
}

/// WEB-028: the numbers as the home page shows them.
#[derive(Debug, Clone)]
struct Shown {
    requests: u32,
    wait: Option<String>,
    per_request: String,
    range: String,
    trades: String,
}

/// WEB-028, D-4098: how long the served home page keeps its numbers. Not a spec value (so not a
/// setting): it only bounds the database work an anonymous `GET /` can cause, since site pages
/// have no rate limit and the pool is small; the figures are public aggregates that move slowly,
/// so a minute-old figure is still a computed one.
pub const HOME_NUMBERS_WINDOW: Duration = Duration::from_secs(60);

/// WEB-028, D-4098: the home page's last shown numbers and when they were read, held by one page
/// router (never a process-wide static, so separate routers never share figures). It keeps the
/// numbers only, never rendered HTML; `None` (below the threshold) is kept like a value, and a
/// failed read is never kept.
#[derive(Debug)]
pub struct HomeNumbersCache {
    window: Duration,
    kept: Mutex<Option<(Instant, Option<Shown>)>>,
}

impl Default for HomeNumbersCache {
    /// An empty cache keeping numbers for [`HOME_NUMBERS_WINDOW`].
    fn default() -> Self {
        Self::new(HOME_NUMBERS_WINDOW)
    }
}

impl HomeNumbersCache {
    /// An empty cache keeping numbers for `window` (a test shortens it).
    #[must_use]
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            kept: Mutex::new(None),
        }
    }

    /// The kept numbers while still inside the window, else `None` (a miss).
    fn fresh(&self) -> Option<Option<Shown>> {
        let kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        kept.as_ref()
            .filter(|(at, _)| at.elapsed() < self.window)
            .map(|(_, shown)| shown.clone())
    }

    /// Keeps `shown` from now on.
    fn keep(&self, shown: Option<Shown>) {
        *self.kept.lock().unwrap_or_else(PoisonError::into_inner) = Some((Instant::now(), shown));
    }
}

/// The fee amounts in whole dollars, from the fee tiers (WEB-026).
struct Fees {
    small: u64,
    medium: u64,
    large: u64,
}

impl Fees {
    fn from_tiers() -> Self {
        Self {
            small: Tier::Small.cents() / 100,
            medium: Tier::Medium.cents() / 100,
            large: Tier::Large.cents() / 100,
        }
    }
}

#[derive(Template)]
#[template(path = "home.html")]
struct HomePage {
    banner_shown: bool,
    numbers: Option<Shown>,
    who: WhoRuns,
    fees: Fees,
}

#[derive(Template)]
#[template(path = "how_it_works.html")]
struct HowItWorksPage {
    banner_shown: bool,
    fees: Fees,
}

#[derive(Template)]
#[template(path = "es_how_it_works.html")]
struct EsHowItWorksPage {
    banner_shown: bool,
    fees: Fees,
}

/// WEB-015: the services page's switched-on trades, licensed or not by `license_kind`.
#[derive(Template)]
#[template(path = "services.html")]
struct ServicesPage {
    banner_shown: bool,
    trades: Vec<CategoryListing>,
}

/// WEB-023: the coverage page's open and "not yet" names.
#[derive(Template)]
#[template(path = "coverage.html")]
struct CoveragePage {
    banner_shown: bool,
    open: Vec<String>,
    not_yet: Vec<String>,
}

/// WEB-008, WEB-025, AI-018: the help page; `old_name` is the brand file's `forbidden_alone` word.
#[derive(Template)]
#[template(path = "help.html")]
struct HelpPage {
    banner_shown: bool,
    ai_on: bool,
    old_name: String,
    csrf: String,
}

/// WEB-011: the ROC complaint page.
#[derive(Template)]
#[template(path = "roc_complaint.html")]
struct RocComplaintPage {
    banner_shown: bool,
}

/// WEB-002: the Spanish home page.
#[derive(Template)]
#[template(path = "es_home.html")]
struct EsHomePage {
    banner_shown: bool,
    who: WhoRuns,
    fees: Fees,
}

/// WEB-020: a switched-on trade's fake sample card; `guide` for a no-license trade.
struct SampleCard {
    code: String,
    name: String,
    guide: bool,
}

/// WEB-020: a switched-off trade and how many waitlisted businesses chose it.
struct Waiting {
    name: String,
    waiting: u32,
}

/// What both business pages show (WEB-018 to WEB-021, WEB-025, BIZ-030, REC-009).
struct BusinessFacts {
    cards: Vec<SampleCard>,
    later: Vec<Waiting>,
    guide: CityGuide,
    who: WhoRuns,
    fees: Fees,
    old_name: String,
}

/// WEB-018, WEB-020, WEB-021, WEB-024: the business page.
#[derive(Template)]
#[template(path = "business.html")]
struct BusinessPage {
    banner_shown: bool,
    texting_on: bool,
    facts: BusinessFacts,
}

/// WEB-019: the Spanish business page.
#[derive(Template)]
#[template(path = "es_business.html")]
struct EsBusinessPage {
    banner_shown: bool,
    texting_on: bool,
    facts: BusinessFacts,
}

/// WEB-020: the business help page.
#[derive(Template)]
#[template(path = "business_help.html")]
struct BusinessHelpPage {
    banner_shown: bool,
}

/// A11Y-020, D-3155: the accessibility statement page, its text the rendered statement document.
#[derive(Template)]
#[template(path = "accessibility.html")]
struct AccessibilityPage<'a> {
    banner_shown: bool,
    doc: &'a LegalDoc,
}

/// WEB-008, WEB-011, WEB-015, WEB-023: the services, coverage, help and ROC complaint pages over
/// `pool`; `env` is the environment the AI switch is read in (AI-018).
pub fn help_routes(pool: PgPool, env: Environment) -> Router {
    let help = {
        let pool = pool.clone();
        move |csrf: Option<Extension<HelpFormCsrf>>| async move {
            let csrf = csrf.map(|Extension(HelpFormCsrf(token))| token);
            respond(help_html(&pool, env, csrf.unwrap_or_default()).await)
        }
    };
    let listed = |coverage: bool| {
        let pool = pool.clone();
        move || async move { listed_page(&pool, coverage).await }
    };
    let (services, coverage) = (listed(false), listed(true));
    let roc = move || async move {
        let banner_shown = banner_shown(&pool).await;
        respond(RocComplaintPage { banner_shown }.render())
    };
    Router::new()
        .route(SERVICES_PATH, get(services))
        .route(COVERAGE_PATH, get(coverage))
        .route(HELP_PATH, get(help))
        .route(ROC_COMPLAINT_PATH, get(roc))
}

/// WEB-008, WEB-025, WORD-009, AI-018: the help page with the AI switch read as it renders; a
/// failed read shows the switched-off wording and logs one warning with no values (R5).
async fn help_html(pool: &PgPool, env: Environment, csrf: String) -> Result<String, askama::Error> {
    let read = match pool.acquire().await {
        Ok(mut db) => askhava_intake::api::ai_on(&mut db, env).await.ok(),
        Err(_) => None,
    };
    let ai_on = read.unwrap_or_else(|| {
        tracing::warn!("AI switch read failed; the help page shows the switched-off wording");
        false
    });
    HelpPage {
        banner_shown: banner_shown(pool).await,
        ai_on,
        old_name: old_name(),
        csrf,
    }
    .render()
}

/// WEB-025, D-4044: the brand file's `forbidden_alone` word, never typed.
fn old_name() -> String {
    brand()
        .forbidden_alone()
        .first()
        .cloned()
        .unwrap_or_default()
}

/// WEB-015 (services, when not `coverage`) or WEB-023 (coverage), read from the catalog's list
/// (D-3879); a failed read is a bare 500 with one warning and no values (R5), never an empty list.
async fn listed_page(pool: &PgPool, coverage: bool) -> Response {
    let read = match pool.acquire().await {
        Ok(mut db) => category_list(&mut db).await.ok(),
        Err(_) => None,
    };
    let Some(list) = read else {
        tracing::warn!("category list read failed; the page is not shown");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let banner_shown = banner_shown(pool).await;
    let (on, off): (Vec<_>, Vec<_>) = list.categories.into_iter().partition(|c| c.enabled);
    if !coverage {
        return respond(
            ServicesPage {
                banner_shown,
                trades: on,
            }
            .render(),
        );
    }
    let off = off.into_iter().map(|c| c.name);
    let page = CoveragePage {
        banner_shown,
        open: on.into_iter().map(|c| c.name).collect(),
        not_yet: off.chain(list.not_offered).collect(),
    };
    respond(page.render())
}

/// WEB-001, WEB-012: the site pages' router over `pool`, with `pages`' settings and its own
/// home numbers cache of [`HOME_NUMBERS_WINDOW`] (D-4098).
pub fn page_routes(pool: PgPool, pages: SitePages) -> Router {
    page_routes_with(pool, pages, HomeNumbersCache::default())
}

/// WEB-001, WEB-012, WEB-028, D-4098: [`page_routes`] with the home numbers cache `numbers`, which
/// this router alone holds.
pub fn page_routes_with(pool: PgPool, pages: SitePages, numbers: HomeNumbersCache) -> Router {
    let state = Arc::new((pool, pages));
    let home = {
        let (state, numbers) = (Arc::clone(&state), Arc::new(numbers));
        move || async move { respond(home_read(&state.0, &state.1, Some(&numbers)).await) }
    };
    let how = {
        let pool = state.0.clone();
        move || async move { respond(how_it_works_html(&pool, false).await) }
    };
    let es = {
        let pool = state.0.clone();
        move || async move { respond(how_it_works_html(&pool, true).await) }
    };
    let statement = {
        let (pool, doc) = (state.0.clone(), Arc::new(accessibility_statement()));
        move || async move {
            let banner_shown = banner_shown(&pool).await;
            respond(
                AccessibilityPage {
                    banner_shown,
                    doc: &doc,
                }
                .render(),
            )
        }
    };
    Router::new()
        .route(HOME_PATH, get(home.clone()))
        .route(OLD_CATEGORIES_PATH, get(home.clone()))
        .route(OLD_EVENTS_PATH, get(home))
        .route(HOW_IT_WORKS_PATH, get(how))
        .route(ES_HOW_IT_WORKS_PATH, get(es))
        .route(ACCESSIBILITY_PATH, get(statement))
        .merge(business_routes(&state))
}

/// WEB-002, WEB-018 to WEB-021, WEB-024: the Spanish home, both business pages and the business
/// help page over the shared pool and settings.
fn business_routes(state: &Arc<(PgPool, SitePages)>) -> Router {
    let page = |spanish: bool| {
        let state = Arc::clone(state);
        move || async move { business_page(&state.0, &state.1, spanish).await }
    };
    let es_home = {
        let state = Arc::clone(state);
        move || async move {
            let (pool, pages) = (&state.0, &state.1);
            let (who, fees) = (pages.who.clone(), Fees::from_tiers());
            let banner_shown = banner_shown(pool).await;
            respond(
                EsHomePage {
                    banner_shown,
                    who,
                    fees,
                }
                .render(),
            )
        }
    };
    let help = {
        let pool = state.0.clone();
        move || async move {
            let banner_shown = banner_shown(&pool).await;
            respond(BusinessHelpPage { banner_shown }.render())
        }
    };
    Router::new()
        .route(ES_HOME_PATH, get(es_home))
        .route(BUSINESS_PATH, get(page(false)))
        .route(ES_BUSINESS_PATH, get(page(true)))
        .route(BUSINESS_HELP_PATH, get(help))
}

/// WEB-018 to WEB-021, WEB-024: the business page, in Spanish when `spanish` (WEB-019). Business
/// texting shows as off, as the served sign-up does, until T526 wires the stored switch (D-3880,
/// D-3881). A failed read is a bare 500 with one warning and no values (R5), never an empty list.
async fn business_page(pool: &PgPool, pages: &SitePages, spanish: bool) -> Response {
    let Some(facts) = business_facts(pool, pages).await else {
        tracing::warn!("business page reads failed; the page is not shown");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let (banner_shown, texting_on) = (banner_shown(pool).await, false);
    respond(if spanish {
        EsBusinessPage {
            banner_shown,
            texting_on,
            facts,
        }
        .render()
    } else {
        BusinessPage {
            banner_shown,
            texting_on,
            facts,
        }
        .render()
    })
}

/// WEB-020, D-4044: a sample card per switched-on trade (the guide on no-license ones) and, per
/// switched-off trade, the waitlisted businesses that chose it; `None` when a read fails.
async fn business_facts(pool: &PgPool, pages: &SitePages) -> Option<BusinessFacts> {
    let mut db = pool.acquire().await.ok()?;
    let list = category_list(&mut db).await.ok()?;
    let (mut cards, mut later) = (Vec::new(), Vec::new());
    for trade in list.categories {
        if trade.enabled {
            let guide = trade.license_kind == "none";
            let (code, name) = (trade.code, trade.name);
            cards.push(SampleCard { code, name, guide });
        } else {
            let waiting = trade_count(&mut db, Uuid::nil(), &trade.code, true)
                .await
                .ok()?;
            later.push(Waiting {
                name: trade.name,
                waiting,
            });
        }
    }
    Some(BusinessFacts {
        cards,
        later,
        guide: pages.guide.clone(),
        who: pages.who.clone(),
        fees: Fees::from_tiers(),
        old_name: old_name(),
    })
}

/// A rendered page, or a bare 500 that names nothing (R4, R5).
fn respond(page: Result<String, askama::Error>) -> Response {
    match page {
        Ok(html) => Html(html).into_response(),
        Err(_) => {
            tracing::error!("site page failed to render");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// WEB-001, WEB-005, WEB-009, WEB-022, WEB-028: the home page, its numbers read as it renders.
///
/// # Errors
///
/// The template's error.
pub async fn home_html(pool: &PgPool, pages: &SitePages) -> Result<String, askama::Error> {
    home_read(pool, pages, None).await
}

/// WEB-007, WEB-028, D-4098: the home page on one pooled connection: the banner, then the
/// numbers from `cache` while fresh, else read (and kept when the read succeeds). A failed
/// acquire shows neither (one warning, no values, R5).
async fn home_read(
    pool: &PgPool,
    pages: &SitePages,
    cache: Option<&HomeNumbersCache>,
) -> Result<String, askama::Error> {
    let Ok(mut db) = pool.acquire().await else {
        tracing::warn!("home page database read failed; the page shows no banner and no numbers");
        return home_page(pages, false, None);
    };
    let banner_shown = banner_shown_on(&mut db).await;
    let numbers = match cache.and_then(HomeNumbersCache::fresh) {
        Some(kept) => kept,
        None => {
            let read = numbers(&mut db, pages.min_jobs).await;
            if let (Some(cache), Some(found)) = (cache, &read) {
                cache.keep(found.clone());
            }
            read.flatten()
        }
    };
    home_page(pages, banner_shown, numbers)
}

/// The home page with its banner and numbers decided.
fn home_page(
    pages: &SitePages,
    banner_shown: bool,
    numbers: Option<Shown>,
) -> Result<String, askama::Error> {
    HomePage {
        banner_shown,
        numbers,
        who: pages.who.clone(),
        fees: Fees::from_tiers(),
    }
    .render()
}

/// WEB-026: the how-it-works page, in Spanish when `spanish` (WEB-002).
///
/// # Errors
///
/// The template's error.
pub async fn how_it_works_html(pool: &PgPool, spanish: bool) -> Result<String, askama::Error> {
    let (banner_shown, fees) = (banner_shown(pool).await, Fees::from_tiers());
    if spanish {
        EsHowItWorksPage { banner_shown, fees }.render()
    } else {
        HowItWorksPage { banner_shown, fees }.render()
    }
}

/// WEB-028, D-466: the computed numbers (`Some(None)` below `min_jobs`), or `None` when the read
/// fails (one warning, no values, R5).
async fn numbers(db: &mut PgConnection, min_jobs: u32) -> Option<Option<Shown>> {
    let read = askhava_intake::api::home_numbers(db, min_jobs).await;
    let Some(found) = read
        .map_err(|_| tracing::warn!("home numbers read failed; the page shows none"))
        .ok()?
    else {
        return Some(None);
    };
    let day = |at| arizona_time(at).strftime("%b %-d, %Y").to_string();
    Some(Some(Shown {
        requests: found.requests,
        wait: found.median_first_quote.map(wait_words),
        per_request: format!(
            "{}.{}",
            found.quotes_per_request_tenths / 10,
            found.quotes_per_request_tenths % 10
        ),
        range: format!("{} to {}", day(found.cards_from), day(found.cards_to)),
        trades: plural(found.trades, "trade"),
    }))
}

/// WEB-028: a median wait in words: whole minutes under two hours, else the nearest hour.
fn wait_words(wait: SignedDuration) -> String {
    let minutes = u32::try_from(wait.as_secs().max(0) / 60).unwrap_or(u32::MAX);
    if minutes < 120 {
        plural(minutes, "minute")
    } else {
        plural(minutes.saturating_add(30) / 60, "hour")
    }
}

/// `n` and `word`, with an "s" unless `n` is 1.
fn plural(n: u32, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

#[cfg(test)]
mod tests {
    use askama::Template;
    use askhava_catalog::api::CategoryListing;
    use askhava_core::api::brand;
    use askhava_payments::api::Tier;
    use jiff::SignedDuration;

    use super::{
        AccessibilityPage, BusinessFacts, BusinessHelpPage, BusinessPage, CityGuide, CoveragePage,
        EsBusinessPage, EsHomePage, EsHowItWorksPage, Fees, HelpPage, HomePage, HowItWorksPage,
        RocComplaintPage, SampleCard, ServicesPage, Settings, Waiting, WhoRuns, wait_words,
    };

    /// The home page with no numbers and `who`'s facts.
    fn home(who: WhoRuns) -> String {
        let fees = Fees::from_tiers();
        let page = HomePage {
            banner_shown: false,
            numbers: None,
            who,
            fees,
        };
        page.render().expect("renders")
    }

    /// A tier's fee in whole dollars, read from the fee settings.
    fn dollars(tier: Tier) -> u64 {
        tier.cents() / 100
    }

    /// TD-WEB-09 (WEB-026, D-384): both how-it-works pages hold the fee sentence with the amounts
    /// of the fee tiers, so a changed fee setting changes the pages with it.
    #[test]
    fn td_web_09_the_fee_sentence_matches_the_fee_tiers() {
        let (name, (s, m, l)) = (
            brand().display_name(),
            (
                dollars(Tier::Small),
                dollars(Tier::Medium),
                dollars(Tier::Large),
            ),
        );
        let fees = Fees::from_tiers;
        let en = HowItWorksPage {
            banner_shown: false,
            fees: fees(),
        }
        .render()
        .expect("en");
        assert!(en.contains(&format!(
            "The business you choose pays {name} a set introduction fee (${s}, ${m} or ${l}, by quote size). You pay {name} nothing."
        )));
        let es = EsHowItWorksPage {
            banner_shown: false,
            fees: fees(),
        }
        .render()
        .expect("es");
        assert!(es.contains("<html lang=\"es\">"));
        assert!(es.contains(&format!(
            "(${s}, ${m} o ${l}, según el tamaño de la cotización)"
        )));
        assert!(es.contains(&format!("Usted no le paga nada a {name}.")));
    }

    /// TD-WEB-01 (WEB-003, WEB-004, WEB-005, WEB-009, WORD-002): the footer's "Homeowners" column,
    /// tagline and statement, the launch-offer line and the chat offer; no directory, events or
    /// /chat link and no script.
    #[test]
    fn td_web_01_footer_launch_offer_and_chat() {
        let html = home(WhoRuns::default());
        let footer = &html[html.find("<footer").expect("footer")..];
        assert!(footer.contains("<h2>Homeowners</h2>"));
        for link in [
            "/how-it-works\">How it works",
            "/services\">Services",
            "/request\">Get quotes",
            "/help\">Help",
        ] {
            assert!(footer.contains(link), "{link}");
        }
        assert!(footer.contains("Local quotes. Your choice."));
        let name = brand().display_name();
        assert!(footer.contains(&format!("{name} is not a contractor and does not perform, bid on or supervise work. We pass along your request and show you quotes from independent local businesses, as they wrote them. You choose, and you hire the business directly.")));
        assert!(html.contains("Launch offer: your first win is free, on any job size, when you join in the first 90 days or within 90 days after your trade opens."));
        assert!(html.contains("<a href=\"/request\">Get quotes</a>"));
        for gone in [
            "/chat\"",
            "/categories",
            "/events",
            "More than quotes",
            "<script",
        ] {
            assert!(!html.contains(gone), "{gone}");
        }
    }

    /// TD-WEB-07 (WEB-021, WEB-022): the "Who runs" block says "usually", carries the 911 note and
    /// never "Safety texts go out at any hour."; each optional fact shows only when set.
    #[test]
    fn td_web_07_who_runs_block() {
        let name = brand().display_name();
        let bare = home(WhoRuns::default());
        assert!(bare.contains(&format!(
            "{name} is run by {}, a Lake Havasu City business.",
            brand().legal_name()
        )));
        assert!(bare.contains("help and charge questions usually within 2 days"));
        assert!(bare.contains(&format!("Don't rely on {name} in an emergency: call 911.")));
        assert!(!bare.contains("Safety texts go out at any hour."));
        for absent in [
            "Mailing address",
            "Arizona Corporation Commission",
            "business license:",
        ] {
            assert!(!bare.contains(absent), "{absent}");
        }
        let full = home(WhoRuns {
            mailing_address: Some("PO Box 0000, Fake Town, AZ".to_owned()),
            record_url: Some("https://example.test/record".to_owned()),
            city_license: Some("FAKE-0000".to_owned()),
        });
        assert!(full.contains("Mailing address: PO Box 0000, Fake Town, AZ"));
        assert!(full.contains("<a href=\"https://example.test/record\">"));
        assert!(full.contains("Lake Havasu City business license: FAKE-0000"));
    }

    /// A switched-on fake category with `kind` of license.
    fn trade(code: &str, plural: &str, kind: &str) -> CategoryListing {
        let (code, name, plural) = (code.to_owned(), format!("Fake {code}"), plural.to_owned());
        let license_kind = kind.to_owned();
        CategoryListing {
            code,
            name,
            plural,
            enabled: true,
            license_kind,
        }
    }

    /// TD-WEB-01 (WEB-015): each licensed trade's section opens with "Get quotes from licensed
    /// <plural>." and the long statement; a trade with no license never says "licensed".
    #[test]
    fn td_web_01_services_sections_open_with_the_licensed_line() {
        let html = ServicesPage {
            banner_shown: false,
            trades: vec![
                trade("fake_pipes", "pipe fixers", "roc"),
                trade("fake_mops", "moppers", "none"),
            ],
        }
        .render()
        .expect("renders");
        let name = brand().display_name();
        let pipes = &html[html.find("id=\"fake_pipes\"").expect("section")..];
        let opening = format!(
            "<p data-fixed=\"WEB-015\">Get quotes from licensed pipe fixers.</p>\n<p data-fixed=\"WEB-015\">{name} is not a contractor and does not perform, bid on or supervise work. We pass along your request and show you quotes from independent local businesses, as they wrote them. You choose, and you hire the business directly.</p>"
        );
        assert!(pipes.contains(&opening), "{pipes}");
        assert!(html.contains("Get quotes from local moppers."));
        assert!(!html.contains("licensed moppers"));
    }

    /// TD-WEB-01 TD-WORD-02 TD-WORD-03 TD-WEB-07 TD-WEB-06 TD-TERMS-04 (WEB-008, WEB-025, WORD-009,
    /// AI-018): the person answer, the reply-time line, the WEB-025 answer in both languages, the
    /// AI wording that follows the switch, and the CSRF field.
    #[test]
    fn td_web_01_help_page_answers() {
        let page = |ai_on| HelpPage {
            banner_shown: false,
            ai_on,
            old_name: brand().forbidden_alone()[0].clone(),
            csrf: "fake-token".to_owned(),
        };
        let (off, on) = (
            page(false).render().expect("off"),
            page(true).render().expect("on"),
        );
        let (name, old_name) = (brand().display_name(), &brand().forbidden_alone()[0]);
        assert!(off.contains(&format!(
            "{name} is a local service. Our texts are automated."
        )));
        assert!(off.contains(&format!("The {name} owner reads HELP messages and flagged messages, usually within 2 days, or within 24 hours if your job is urgent. Messages are not watched around the clock. In an emergency, call 911.")));
        assert!(!off.contains("a real person reviews anything flagged"));
        assert!(off.contains(&format!(
            "{name} isn't connected to any business with \"{old_name}\" in its name."
        )));
        assert!(off.contains(&format!("<p lang=\"es\">{name} no tiene ninguna relación con negocios que lleven \"{old_name}\" en su nombre.</p>")));
        assert!(off.contains(AI_OFF) && !off.contains(AI_ON));
        assert!(on.contains(AI_ON) && !on.contains(AI_OFF));
        assert!(off.contains("<input type=\"hidden\" name=\"csrf\" value=\"fake-token\">"));
        assert!(off.contains(&format!("Send the {name} owner a message")));
        assert!(!off.contains("AI-powered"));
    }

    /// TD-WEB-01 (WEB-011, WEB-023): the ROC page links on to the ROC's complaint form; the
    /// coverage page lists open and "not yet" names and the Canadian-number line.
    #[test]
    fn td_web_01_roc_and_coverage_pages() {
        let roc = RocComplaintPage {
            banner_shown: false,
        }
        .render()
        .expect("roc");
        assert!(roc.contains("href=\"https://roc.az.gov/file-complaint\""));
        let coverage = CoveragePage {
            banner_shown: false,
            open: vec!["Fake open".to_owned()],
            not_yet: vec!["Fake later".to_owned()],
        }
        .render()
        .expect("coverage");
        let not_yet = coverage.find("<li>Fake later</li>").expect("not yet");
        assert!(coverage.find("<li>Fake open</li>").expect("open") < not_yet);
        assert!(
            coverage.contains("Canadian numbers work. Your phone company may charge roaming fees.")
        );
    }

    /// AI-018's switched-off sentence (`docs/spec/ai.md:22`).
    const AI_OFF: &str = "Our texts are automated. When it's switched on, an AI service helps word our questions and write your job card from your request, after our software removes names, phone numbers, emails and street numbers. You see the card before it goes.";
    /// The start of AI-018's switched-on paragraph.
    const AI_ON: &str = "To sort your request and help word our questions, an AI service from Anthropic reads your job description";

    /// WEB-028: a wait reads in minutes under two hours, else in rounded hours.
    #[test]
    fn waits_read_in_minutes_then_hours() {
        assert_eq!(wait_words(SignedDuration::from_secs(60)), "1 minute");
        assert_eq!(wait_words(SignedDuration::from_mins(119)), "119 minutes");
        assert_eq!(wait_words(SignedDuration::from_mins(150)), "3 hours");
        assert_eq!(wait_words(SignedDuration::from_mins(-5)), "0 minutes");
    }

    /// WEB-020, D-3880: the three city-license settings fill the guide, trimmed, and an empty
    /// one counts as unset (fake dev settings).
    #[test]
    fn web_020_city_guide_reads_its_three_settings() {
        let guide = |cost: &str, url: &str| {
            let pairs = [
                ("ASKHAVA_APP_ENV", "dev"),
                ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
                ("ASKHAVA_STORAGE_BASE_URL", "file:///tmp/fake-t202"),
                (
                    "ASKHAVA_ID_SECRET",
                    "test-only-id-secret-for-t202-not-a-real-key",
                ),
                ("ASKHAVA_CITY_LICENSE_COST", cost),
                ("ASKHAVA_CITY_LICENSE_TIME", ""),
                ("ASKHAVA_CITY_LICENSE_URL", url),
            ];
            let pairs = pairs.map(|(k, v)| (k.to_owned(), v.to_owned()));
            CityGuide::from_settings(&Settings::from_pairs(pairs).expect("settings"))
        };
        let set = guide(" $50 ", "https://fake.example/license");
        assert_eq!(set.cost.as_deref(), Some("$50"));
        assert_eq!(set.time, None);
        assert_eq!(set.url.as_deref(), Some("https://fake.example/license"));
        assert!(set.shown());
        assert!(!guide("", "").shown());
    }

    /// Fake business-page facts: a licensed and a no-license card, one waiting trade, `guide`.
    fn facts(guide: CityGuide) -> BusinessFacts {
        let card = |code: &str, guide| SampleCard {
            code: code.to_owned(),
            name: format!("Fake {code}"),
            guide,
        };
        BusinessFacts {
            cards: vec![card("fake_pipes", false), card("fake_mops", true)],
            later: vec![Waiting {
                name: "Fake later".to_owned(),
                waiting: 4,
            }],
            guide,
            who: WhoRuns::default(),
            fees: Fees::from_tiers(),
            old_name: brand().forbidden_alone()[0].clone(),
        }
    }

    /// The business page in English and Spanish with `texting_on` and `guide`.
    fn business(texting_on: bool, guide: &CityGuide) -> (String, String) {
        let en = BusinessPage {
            banner_shown: false,
            texting_on,
            facts: facts(guide.clone()),
        };
        let es = EsBusinessPage {
            banner_shown: false,
            texting_on,
            facts: facts(guide.clone()),
        };
        (en.render().expect("en"), es.render().expect("es"))
    }

    /// The part of `html` from `start` to the next section's end.
    fn section<'a>(html: &'a str, start: &str) -> &'a str {
        let from = &html[html.find(start).expect("section")..];
        &from[..from.find("</section>").expect("end")]
    }

    /// TD-WEB-03 TD-WEB-07 (WEB-018, WEB-019, WEB-020, WEB-021, WORD-017): both WORD-017 lines, a
    /// fake sample card per open trade, the city guide on the no-license card only, the waiting
    /// line, the "Who runs" block with "usually" and the day-job line, the Spanish sign-up link, and
    /// no prototype wording, in English and Spanish.
    #[test]
    fn td_web_03_business_page_cards_guide_and_lines() {
        let guide = CityGuide {
            cost: Some("Fake cost".to_owned()),
            time: Some("Fake time".to_owned()),
            url: Some("https://example.test/city".to_owned()),
        };
        let (en, es) = business(false, &guide);
        assert!(en.contains("You quote once. No bidding rounds, no paid placement."));
        assert!(en.contains("Every card says how many businesses got it."));
        assert!(section(&en, "id=\"sample-fake_mops\"").contains("Cost: Fake cost"));
        assert!(section(&en, "id=\"sample-fake_mops\"").contains("https://example.test/city"));
        assert!(!section(&en, "id=\"sample-fake_pipes\"").contains("city business license"));
        assert!(en.contains("Opens when there are enough requests; 4 businesses waiting."));
        let name = brand().display_name();
        assert!(en.contains(&format!(
            "{name} is run by {}, a Lake Havasu City business. Its owner lives in Havasu.",
            brand().legal_name()
        )));
        assert!(en.contains("The owner has a full-time day job. Sign-up reviews are usually answered within 4 days, help and charge questions usually within 2 days, and this page shows when the owner is away."));
        assert!(en.contains("href=\"/b/signup\"") && en.contains("four months"));
        for gone in [
            " lead",
            "no bidding wars",
            "Mailing address",
            "<img",
            "<script",
        ] {
            assert!(!en.contains(gone) && !es.contains(gone), "{gone}");
        }
        assert!(es.contains("<html lang=\"es\">") && es.contains("href=\"/b/signup?lang=es\""));
        assert!(es.contains("Sin rondas de ofertas, sin posiciones pagadas."));
        assert!(es.contains("Cada tarjeta dice cuántos negocios la recibieron."));
        assert!(section(&es, "id=\"muestra-fake_mops\"").contains("Costo: Fake cost"));
        assert!(!section(&es, "id=\"muestra-fake_pipes\"").contains("licencia comercial"));
        assert!(es.contains("Abre cuando haya suficientes solicitudes; 4 negocios esperando."));
        assert!(es.contains("normalmente se revisan en 4 días"));
    }

    /// TD-WEB-03 (WEB-020, D-3880): the city guide is hidden while every part is unset, and a set
    /// part shows alone.
    #[test]
    fn td_web_03_city_guide_shows_only_set_parts() {
        let (en, es) = business(false, &CityGuide::default());
        assert!(!en.contains("city business license") && !es.contains("licencia comercial de la"));
        let link_only = CityGuide {
            url: Some("https://example.test/city".to_owned()),
            ..CityGuide::default()
        };
        let (en, _) = business(false, &link_only);
        let mops = section(&en, "id=\"sample-fake_mops\"");
        assert!(mops.contains("https://example.test/city") && !mops.contains("Cost:"));
    }

    /// TD-WEB-05 (WEB-024): the switch-off notice line by default and the switch-on line when
    /// business texting is on, in English and Spanish.
    #[test]
    fn td_web_05_notice_line_follows_the_switch() {
        const OFF: &str = "Job requests and picks come by email. We don't text or call businesses.";
        const ES_OFF: &str = "No enviamos mensajes de texto ni llamamos a los negocios.";
        let name = brand().display_name();
        let on = format!(
            "Choose how you get job requests and picks: email, text or both. {name} never calls you with a recording."
        );
        let (en, es) = business(false, &CityGuide::default());
        assert!(en.contains(OFF) && !en.contains(&on) && es.contains(ES_OFF));
        let (en, es) = business(true, &CityGuide::default());
        assert!(en.contains(&on) && !en.contains(OFF) && !es.contains(ES_OFF));
        assert!(es.contains(&format!("{name} nunca le llama con una grabación.")));
    }

    /// TD-WEB-06 (WEB-025): the business page holds the WEB-025 answer in English and Spanish.
    #[test]
    fn td_web_06_business_page_answers_the_old_name_question() {
        let (en, es) = business(false, &CityGuide::default());
        let (name, old) = (brand().display_name(), &brand().forbidden_alone()[0]);
        assert!(en.contains(&format!(
            "{name} isn't connected to any business with \"{old}\" in its name."
        )));
        assert!(es.contains(&format!(
            "{name} no tiene ninguna relación con negocios que lleven \"{old}\" en su nombre."
        )));
    }

    /// TD-WEB-03 (BIZ-030, REC-009, WORD-002): "How you're charged" and the fee table take their
    /// numbers from the fee settings; the $150-a-month pool example sits in the $75 row; ongoing
    /// services count as four months, capped at the middle fee.
    #[test]
    fn td_web_03_charges_and_pricing_match_the_fee_settings() {
        let (s, m, l) = (
            dollars(Tier::Small),
            dollars(Tier::Medium),
            dollars(Tier::Large),
        );
        let (en, es) = business(false, &CityGuide::default());
        assert!(en.contains(&format!(
            "Pay only when you confirm a pick: ${s} / ${m} / ${l} by job size."
        )));
        assert!(en.contains(&format!(
            "You pay one fixed fee when you confirm a pick: ${s}, ${m} or ${l} by job size."
        )));
        let name = brand().display_name();
        assert!(en.contains(&format!("{name} is not a contractor. It passes along this request and the homeowner's choice. If the homeowner picks you, they hire you directly; {name} doesn't direct, schedule, supervise or pay for the work.")));
        assert_eq!(Tier::for_basis(150 * 100 * 4), Tier::Medium);
        assert_eq!(askhava_payments::api::RECURRING_MONTHS, 4);
        let cap = i32::try_from(m).expect("fits");
        assert_eq!(
            askhava_payments::api::RecurringCap::from_dollars(cap),
            Ok(askhava_payments::api::RecurringCap::DEFAULT)
        );
        assert!(en.contains(&format!("counts as $600.</td><td>${m}</td>")));
        assert!(en.contains(&format!("The fee never goes over ${m}.")));
        assert!(es.contains(&format!("cuenta como $600.</td><td>${m}</td>")));
        assert!(es.contains(&format!("${s} / ${m} / ${l} según el tamaño del trabajo")));
        for (edge, tier) in [
            (49_999, Tier::Small),
            (50_000, Tier::Medium),
            (150_000, Tier::Medium),
            (150_001, Tier::Large),
        ] {
            assert_eq!(Tier::for_basis(edge), tier, "{edge}");
        }
    }

    /// TD-WEB-01 TD-WEB-07 TD-WORD-02 (WEB-002, WEB-022, WORD-002, WORD-007, WORD-013): the Spanish
    /// home page is in Spanish with the fee line, the first-name privacy line, WEB-022's whole
    /// emergency line and the Spanish footer statement; the business help page holds WEB-020's
    /// safety tip.
    #[test]
    fn td_web_01_spanish_home_and_business_help() {
        let es = EsHomePage {
            banner_shown: false,
            who: WhoRuns::default(),
            fees: Fees::from_tiers(),
        }
        .render()
        .expect("es");
        let (name, (s, m, l)) = (
            brand().display_name(),
            (
                dollars(Tier::Small),
                dollars(Tier::Medium),
                dollars(Tier::Large),
            ),
        );
        assert!(es.contains("<html lang=\"es\">"));
        assert!(es.contains(&format!(
            "tarifa de presentación fija según el tamaño del trabajo: ${s}, ${m} o ${l}."
        )));
        assert!(es.contains("Su número nunca se comparte."));
        assert!(
            es.contains("Su nombre de pila y su dirección van solo al negocio que usted elija.")
        );
        assert!(es.contains(&format!(
            "{name} no es un servicio de emergencias y no evalúa qué requiere una respuesta de \
             emergencia. Si su mensaje menciona gas, humo, fuego, una alarma de monóxido de \
             carbono sonando o un cable eléctrico caído, le enviamos un recordatorio automático \
             para que llame al 911. No dependa de {name} en una emergencia: llame al 911."
        )));
        assert!(es.contains(&format!(
            "{name} no es contratista y no realiza, cotiza ni supervisa trabajos."
        )));
        assert!(!es.contains("<h2>Homeowners</h2>"));
        let help = BusinessHelpPage {
            banner_shown: false,
        }
        .render()
        .expect("help");
        assert!(help.contains("<h2>Safety when you work alone</h2>"));
        assert!(help.contains("tell someone where the job is and when you expect to finish"));
        assert!(help.contains("If anyone is in danger, call 911."));
    }

    /// TD-WEB-08 (A11Y-020, D-3155): the statement page holds the statement's title and its
    /// rendered body, the one copy in `docs/legal/accessibility-statement.md`.
    #[test]
    fn td_web_08_statement_page_holds_the_rendered_statement() {
        let doc = crate::legal_pages::accessibility_statement();
        let page = AccessibilityPage {
            banner_shown: false,
            doc: &doc,
        }
        .render()
        .expect("statement");
        assert!(page.contains(&format!("<h1>{}</h1>", doc.title)), "{page}");
        assert!(page.contains(doc.body.as_str()), "{page}");
    }
}
