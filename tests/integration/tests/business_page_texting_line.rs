//! The business page's WEB-024 line follows the stored business-texting switch (T769; TD-WEB-05,
//! WEB-024, TXT-035, BIZ-036, D-4931).
//!
//! The English and Spanish business pages, served by `site::api::page_routes` with the business
//! number set through `SitePages::with_business_number` as `server`'s `main.rs` sets it, show the
//! switch-on line only while the stored switch is on and a number is set; the switch off, no
//! number or a failed read shows the switch-off line. The switch goes on as T526's tests turn it:
//! a fake allowed provider answer is filed first. Every ID, answer and number is fake.

use askhava_catalog::api::load_embedded;
use askhava_core::api::brand;
use askhava_messaging::api::{
    Audience, BusinessEligibility, SendingNumber, file_business_eligibility, switch_on,
};
use askhava_site::api::{SitePages, page_routes};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

/// A fake business sending number (a 555-01xx line).
const NUMBER: &str = "+19285550127";
/// The English switch-off line (WEB-024).
const OFF_EN: &str = "Job requests and picks come by email. We don't text or call businesses.";
/// The Spanish switch-off line (WEB-019, WEB-024).
const OFF_ES: &str = "Las solicitudes de trabajo y los avisos de que lo eligieron llegan por correo electrónico. No enviamos mensajes de texto ni llamamos a los negocios.";
/// The Spanish switch-on line, before the brand name.
const ON_ES: &str = "Elija cómo recibe las solicitudes de trabajo y los avisos de que lo eligieron: correo electrónico, mensaje de texto o ambos.";

/// The English switch-on line, the brand name read from the brand file.
fn on_en() -> String {
    let name = brand().display_name();
    format!(
        "Choose how you get job requests and picks: email, text or both. {name} never calls you with a recording."
    )
}

/// The site pages with the fake business number when `set`.
fn pages(set: bool) -> SitePages {
    let number = set.then(|| SendingNumber::parse(NUMBER).expect("number"));
    SitePages::default().with_business_number(number)
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

/// Asserts both business pages show the switch-on line when `on`, else the switch-off line.
async fn assert_line(pool: &PgPool, set: bool, on: bool, case: &str) {
    let en = page(pool, pages(set), "/business").await;
    let es = page(pool, pages(set), "/es/business").await;
    assert_eq!(en.contains(&on_en()), on, "{case}: English switch-on line");
    assert_eq!(en.contains(OFF_EN), !on, "{case}: English switch-off line");
    assert_eq!(es.contains(ON_ES), on, "{case}: Spanish switch-on line");
    assert_eq!(es.contains(OFF_ES), !on, "{case}: Spanish switch-off line");
}

/// Files a fake allowed provider answer naming a campaign (T527's gate), then turns the stored
/// business switch on.
async fn switched_on(db: &mut PgConnection) {
    let (owner, now) = (
        Uuid::from_u128(0x769),
        "2026-11-10T19:00:00Z".parse().expect("time"),
    );
    let allowed = BusinessEligibility {
        answered_on: "2026-11-04".parse().expect("date"),
        reference: "FAKE-T769-CASE".to_owned(),
        answer: "FAKE-T769 answer text as received.".to_owned(),
        allowed: true,
        campaign_reference: "FAKE-T769-CAMPAIGN".to_owned(),
    };
    file_business_eligibility(db, owner, &allowed, now)
        .await
        .expect("filed");
    let switched = switch_on(db, Audience::Business, (owner, now)).await;
    assert!(switched.expect("switched"), "the stored switch is on");
}

/// TD-WEB-05 (WEB-024, TXT-035): off by default, still off with no number, on with both.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_web_05_business_page_line_follows_the_switch_and_the_number(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
    assert_line(&pool, true, false, "switch off").await;
    switched_on(&mut db).await;
    assert_line(&pool, false, false, "switch on, no number").await;
    assert_line(&pool, true, true, "switch on with a number").await;
}

/// TD-WEB-05, D-4931: a failed switch read shows the switch-off line, and the page still serves.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_web_05_a_failed_switch_read_shows_the_switch_off_line(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    load_embedded(&mut db).await.expect("catalog");
    switched_on(&mut db).await;
    assert_line(&pool, true, true, "readable").await;
    sqlx::query("ALTER TABLE messaging.mode_state RENAME TO mode_state_gone")
        .execute(&mut *db)
        .await
        .expect("rename");
    assert_line(&pool, true, false, "read fails").await;
}
