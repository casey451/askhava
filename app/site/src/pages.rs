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

use std::sync::Arc;

use askama::Template;
use askhava_core::api::{Settings, arizona_time};
use askhava_payments::api::Tier;
use axum::Router;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
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
    use askhava_core::api::brand;
    use askhava_payments::api::Tier;
    use jiff::SignedDuration;

    use super::{EsHowItWorksPage, Fees, HomePage, HowItWorksPage, WhoRuns, wait_words};

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

    /// WEB-028: a wait reads in minutes under two hours, else in rounded hours.
    #[test]
    fn waits_read_in_minutes_then_hours() {
        assert_eq!(wait_words(SignedDuration::from_secs(60)), "1 minute");
        assert_eq!(wait_words(SignedDuration::from_mins(119)), "119 minutes");
        assert_eq!(wait_words(SignedDuration::from_mins(150)), "3 hours");
        assert_eq!(wait_words(SignedDuration::from_mins(-5)), "0 minutes");
    }
}
