//! TD-WEB-10 (WEB-021, WEB-022; D-409, D-3744, D-3890): the "Who runs Laftly" blocks show the
//! LLC's public-record link and the city business license number only while their settings,
//! `ASKHAVA_LLC_RECORD_URL` and `ASKHAVA_CITY_BUSINESS_LICENSE_NUMBER`, are set, and never a
//! personal name.
//!
//! The end-to-end suite starts one binary with a fixed environment, so it checks only the unset
//! case (`tests/e2e/who_runs_links.spec.ts`); this test sets and unsets both settings through
//! `Settings::from_pairs` and `SitePages::from_settings` and renders every page holding the block
//! (the home page, the business page and their Spanish pages) through the page router, so each
//! block is rendered by the page function that reads the settings, never a hand-built template
//! struct. Fake values only.

use askhava_core::api::{Settings, brand};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

use crate::pages::{SitePages, page_routes};

/// A fake record link and license number; the license is found by its value alone.
const RECORD_URL: &str = "https://fake-record.example.test/llc-0000";
const LICENSE: &str = "FAKE-LIC-0000";

/// The pages that hold a "Who runs" block, each with the heading that opens it.
const PAGES: [(&str, &str); 4] = [
    ("/", "Who runs "),
    ("/business", "Who runs "),
    ("/es", "Quién opera "),
    ("/es/business", "Quién opera "),
];

/// The first words of every paragraph a "Who runs" block may hold (WEB-021, WEB-022): the run-by
/// line, each optional fact, the help email, the day-job line and the 911 note. Anything else (a
/// personal name, a photo caption) fails.
const LINE_STARTS: [&str; 14] = [
    "{brand} is run by ",
    "{brand} lo opera ",
    "Mailing address: ",
    "Dirección postal: ",
    "<a href=\"{record}\">",
    "Lake Havasu City business license: ",
    "Licencia comercial de Lake Havasu City: ",
    "Help email: ",
    "Correo de ayuda: ",
    "The owner has a full-time day job.",
    "El dueño tiene un trabajo de tiempo completo.",
    "{brand} isn't an emergency service",
    "{brand} no es un servicio de emergencias",
    "<a href=\"/\">This page in English</a>",
];

/// The site settings with the record link and license number `set` or left out.
fn pages(set: bool) -> SitePages {
    let mut pairs = vec![
        ("ASKHAVA_APP_ENV", "dev"),
        ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
        ("ASKHAVA_STORAGE_BASE_URL", "file:///tmp/fake-t161"),
        (
            "ASKHAVA_ID_SECRET",
            "test-only-id-secret-for-t161-not-a-real-key",
        ),
    ];
    if set {
        pairs.push(("ASKHAVA_LLC_RECORD_URL", RECORD_URL));
        pairs.push(("ASKHAVA_CITY_BUSINESS_LICENSE_NUMBER", LICENSE));
    }
    let pairs = pairs.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned()));
    SitePages::from_settings(&Settings::from_pairs(pairs).expect("settings"))
}

/// The page at `path` as the router serves it with `pages`.
async fn page(pool: &PgPool, pages: SitePages, path: &str) -> String {
    let request = Request::get(path).body(Body::empty()).expect("request");
    let response = page_routes(pool.clone(), pages)
        .oneshot(request)
        .await
        .expect("answer");
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    let body = to_bytes(response.into_body(), 1 << 21).await.expect("body");
    String::from_utf8(body.to_vec()).expect("utf-8")
}

/// The "Who runs" block of `html`: from its heading to the next heading or the footer.
fn block<'a>(html: &'a str, heading: &str) -> &'a str {
    let start = html
        .find(&format!("<h2>{heading}"))
        .expect("who runs heading");
    let rest = &html[start + 4..];
    let end = ["<h2>", "<footer"]
        .iter()
        .filter_map(|marker| rest.find(marker))
        .min()
        .unwrap_or(rest.len());
    &rest[..end]
}

/// WEB-021, WEB-022, D-409: every paragraph of `block` opens with a known line, so no personal
/// name or photo is in it.
fn assert_no_personal_line(block: &str, path: &str) {
    let name = brand().display_name();
    let starts = LINE_STARTS.map(|s| s.replace("{brand}", name).replace("{record}", RECORD_URL));
    assert!(!block.contains("<img"), "{path}: a photo in the block");
    for paragraph in block.split("<p").skip(1) {
        let text = paragraph[paragraph.find('>').expect("tag end") + 1..].trim_start();
        assert!(
            starts.iter().any(|s| text.starts_with(s.as_str())),
            "{path}: an unexpected line in the block: {text}"
        );
    }
}

/// TD-WEB-10 (WEB-021, WEB-022, D-409): with both settings set, every "Who runs" block links the
/// record and shows the license number; with both unset, neither appears, nor the record line's
/// wording; no block holds a personal name or photo either way.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_web_10_record_link_and_license_follow_their_settings(pool: PgPool) {
    for set in [true, false] {
        for (path, heading) in PAGES {
            let html = page(&pool, pages(set), path).await;
            let block = block(&html, heading);
            let link = format!("<a href=\"{RECORD_URL}\">");
            assert_eq!(block.contains(&link), set, "{path} record link, set {set}");
            assert_eq!(block.contains(LICENSE), set, "{path} license, set {set}");
            assert_eq!(
                block.contains("Arizona Corporation Commission"),
                set,
                "{path} record line, set {set}"
            );
            assert_eq!(html.matches(LICENSE).count(), usize::from(set), "{path}");
            assert_no_personal_line(block, path);
        }
    }
}
