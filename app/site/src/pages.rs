//! The home and how-it-works pages, in English and the how-it-works page in Spanish (T102; WEB-001,
//! WEB-003 to WEB-005, WEB-009, WEB-012, WEB-021, WEB-022, WEB-026, WEB-028, WORD-002).
//!
//! [`page_routes`] serves `/`, `/how-it-works` and `/es/how-it-works`, and answers the old
//! directory site's own paths, `/categories` and `/events`, with the home page (200, WEB-012,
//! D-3144); there is no catch-all, so any other unknown path stays a 404. Every page extends
//! `templates/site_layout.html` (its footer, WEB-003, WEB-004, WORD-002) and sets `banner_shown`
//! first (WEB-007). The home page's numbers come only from `intake::api::home_numbers` (the card
//! and quote records, WEB-028, D-466) and show nothing below [`SitePages`]' threshold; the fee
//! amounts come from `payments::api::Tier` (WEB-026). GET only; nothing is written.
//!
//! [`help_routes`] (T201) serves `/services` (one `#<category code>` section per switched-on
//! trade, WEB-015), `/coverage` (WEB-023), both read from `catalog::api::category_list`
//! (D-3879), `/help` (WEB-008, WEB-025, WORD-009, AI-018; the AI switch read through
//! `intake::api::ai_on`, D-3144) and `/help/roc-complaint` (WEB-011). The help form posts to
//! `POST /help`, which T309 serves (D-2128); its CSRF token comes from [`HelpFormCsrf`] (D-3890).

use std::sync::Arc;

use askama::Template;
use askhava_catalog::api::{CategoryListing, category_list};
use askhava_core::api::{Environment, Settings, arizona_time, brand};
use askhava_payments::api::Tier;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use jiff::SignedDuration;
use sqlx::PgPool;

use crate::routes::banner_shown;

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
            mailing_address: settings.postal_address().ok().map(str::to_owned),
            record_url: settings.llc_record_url().map(str::to_owned),
            city_license: settings.city_business_license_number().map(str::to_owned),
        }
    }
}

/// The site pages' settings: WEB-028's threshold and the "Who runs" facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitePages {
    min_jobs: u32,
    who: WhoRuns,
}

impl Default for SitePages {
    /// WEB-028's default threshold of 50 (D-468) and no optional "Who runs" fact.
    fn default() -> Self {
        Self {
            min_jobs: 50,
            who: WhoRuns::default(),
        }
    }
}

impl SitePages {
    /// WEB-021, WEB-028: the pages' settings from `settings` (D-3878, D-3890).
    #[must_use]
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            min_jobs: settings.home_numbers_min_jobs(),
            who: WhoRuns::from_settings(settings),
        }
    }
}

/// WEB-028: the numbers as the home page shows them.
struct Shown {
    requests: u32,
    wait: Option<String>,
    per_request: String,
    range: String,
    trades: String,
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
        old_name: brand()
            .forbidden_alone()
            .first()
            .cloned()
            .unwrap_or_default(),
        csrf,
    }
    .render()
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

/// WEB-001, WEB-012: the site pages' router over `pool`, with `pages`' settings.
pub fn page_routes(pool: PgPool, pages: SitePages) -> Router {
    let state = Arc::new((pool, pages));
    let home = {
        let state = Arc::clone(&state);
        move || async move { respond(home_html(&state.0, &state.1).await) }
    };
    let how = {
        let pool = state.0.clone();
        move || async move { respond(how_it_works_html(&pool, false).await) }
    };
    let es = move || async move { respond(how_it_works_html(&state.0, true).await) };
    Router::new()
        .route(HOME_PATH, get(home.clone()))
        .route(OLD_CATEGORIES_PATH, get(home.clone()))
        .route(OLD_EVENTS_PATH, get(home))
        .route(HOW_IT_WORKS_PATH, get(how))
        .route(ES_HOW_IT_WORKS_PATH, get(es))
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
    HomePage {
        banner_shown: banner_shown(pool).await,
        numbers: numbers(pool, pages.min_jobs).await,
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

/// WEB-028, D-466: the computed numbers, or none below `min_jobs` or when the read fails (one
/// warning, no values, R5).
async fn numbers(pool: &PgPool, min_jobs: u32) -> Option<Shown> {
    let read = match pool.acquire().await {
        Ok(mut db) => askhava_intake::api::home_numbers(&mut db, min_jobs).await,
        Err(_) => {
            tracing::warn!("home numbers read failed; the page shows none");
            return None;
        }
    };
    let found = read
        .map_err(|_| tracing::warn!("home numbers read failed; the page shows none"))
        .ok()??;
    let day = |at| arizona_time(at).strftime("%b %-d, %Y").to_string();
    Some(Shown {
        requests: found.requests,
        wait: found.median_first_quote.map(wait_words),
        per_request: format!(
            "{}.{}",
            found.quotes_per_request_tenths / 10,
            found.quotes_per_request_tenths % 10
        ),
        range: format!("{} to {}", day(found.cards_from), day(found.cards_to)),
        trades: plural(found.trades, "trade"),
    })
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
        CoveragePage, EsHowItWorksPage, Fees, HelpPage, HomePage, HowItWorksPage, RocComplaintPage,
        ServicesPage, WhoRuns, wait_words,
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
}
