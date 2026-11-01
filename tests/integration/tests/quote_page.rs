//! The homeowner's quote page over the database (TD-PICK-38, TD-PICK-04, TD-A11Y-02; WORD-005,
//! PICK-003, PICK-004, PICK-008, PICK-009, PICK-012, PICK-015, TXT-016, CON-028, A11Y-033, D-491 to
//! D-493). Fake IDs and wording only.
//!
//! Stored quotes show in arrival order, numbered; a quote `picks` marked unavailable keeps its
//! number and has no Pick button; a quote arriving later takes the next number and moves none;
//! the page carries the no-ranking top line, both WORD-005 lines, the LIST line, no rating, review
//! or hire count and no summary element (summaries are off by default); the status region is
//! present and empty at first render; and after a sole-word CANCEL the CON-028 "still open" line
//! shows.

use std::sync::Arc;

use askhava_core::api::{IdSecret, LinkToken, ManualClock};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, create, job_number, move_to};
use askhava_licensing::api::LicenseKind;
use askhava_ops::api::Actor;
use askhava_portals::api::quote_page_routes;
use askhava_quotes::api::{
    DayPart, Format, Price, QuoteForm, QuoteRules, Submitted, mark_unavailable, parse_quote,
    submit_quote,
};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use jiff::Timestamp;
use jiff::civil::date;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

fn at(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

fn business(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

/// An open job and its quote-page token.
async fn open_job(pool: &PgPool) -> (Uuid, LinkToken) {
    let mut db = pool.acquire().await.expect("db");
    let job = create(&mut db, at("02T21:00:00"), false, Actor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let _ = move_to(&mut db, job, open, at("02T21:00:00"), Actor::System)
        .await
        .expect("open");
    let key = SecretString::from("fake-id-secret-for-tests-00000001".to_owned());
    let secret = IdSecret::new(1, key).expect("secret");
    let token = issue(&mut db, &secret, job, at("02T21:00:00"))
        .await
        .expect("token");
    (job, token)
}

/// Business `n` quotes on `job` at 9:`minute` a.m., in its own words.
async fn quote(pool: &PgPool, job: Uuid, (n, minute): (u128, u32)) {
    let mut db = pool.acquire().await.expect("db");
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
        .bind(job)
        .bind(business(n))
        .bind(at("03T08:00:00").to_sqlx())
        .execute(&mut *db)
        .await
        .expect("card");
    let form = QuoteForm {
        price: Price::Firm(19_000),
        notes: String::new(),
        original: format!("Sample wording from business {n}"),
        ai_read: false,
        earliest_visit: Some((date(2026, 11, 9), DayPart::Evening)),
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: false,
        license: LicenseKind::None,
        handyman: false,
        certified: false,
    };
    let checked = parse_quote(form, &rules).expect("quote");
    let when = at(&format!("03T09:{minute:02}:00"));
    let saved = submit_quote(&mut db, (job, business(n)), &checked, when).await;
    assert_eq!(saved, Ok(Submitted::Saved));
}

async fn page(pool: &PgPool, token: &LinkToken) -> String {
    let clock = Arc::new(ManualClock::new(at("03T12:00:00")));
    let request = Request::get(format!("/q/{}", token.as_str()))
        .header(header::HOST, "askhava.test")
        .body(Body::empty())
        .expect("request");
    let app = quote_page_routes(pool.clone(), clock);
    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    String::from_utf8(bytes.to_vec()).expect("utf8")
}

fn position(body: &str, text: &str) -> usize {
    body.find(text).unwrap_or_else(|| panic!("{text} missing"))
}

/// TD-PICK-04 (PICK-008, PICK-009, PICK-012, PICK-015, D-084): arrival order whatever the business
/// IDs; an unavailable quote keeps #2 with no Pick button; a later quote is #4 and moves none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_04_arrival_order_numbers_stay(pool: PgPool) {
    let (job, token) = open_job(&pool).await;
    quote(&pool, job, (3, 0)).await;
    quote(&pool, job, (1, 5)).await;
    quote(&pool, job, (2, 9)).await;
    let mut db = pool.acquire().await.expect("db");
    let marked = mark_unavailable(&mut db, (job, business(1)), at("03T10:00:00")).await;
    assert!(marked.is_ok());
    quote(&pool, job, (4, 30)).await;
    let body = page(&pool, &token).await;
    let first = position(&body, "Sample wording from business 3");
    let third = position(&body, "Sample wording from business 2");
    let fourth = position(&body, "Sample wording from business 4");
    let gone = position(&body, "No longer available");
    assert!(first < gone && gone < third && third < fourth);
    assert!(!body.contains("Sample wording from business 1"));
    for n in [1, 3, 4] {
        assert!(body.contains(&format!("Pick #{n}</button>")), "{n}");
        assert!(body.contains(&format!("or reply {n} YES")), "{n}");
    }
    assert!(!body.contains("Pick #2") && !body.contains("or reply 2 YES"));
}

/// TD-PICK-38 (WORD-005, PICK-003, PICK-004, PICK-010, TXT-016) and TD-A11Y-02 (A11Y-033): the
/// fixed page lines, no rating, review or hire count, no summary element, the LIST line and an
/// empty status region at first render.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_38_page_lines_and_empty_status(pool: PgPool) {
    let (job, token) = open_job(&pool).await;
    quote(&pool, job, (1, 0)).await;
    let body = page(&pool, &token).await;
    let top = "Ask Hava doesn't rank businesses or show ratings. Check each business yourself: \
               its license, and its reviews on Google and Yelp.";
    for line in [
        top,
        "Every eligible quote shown, in the order received.",
        "#1 means first received, not first choice.",
        "Can't open links? Reply LIST to get your quotes by text.",
    ] {
        assert!(body.contains(line), "{line}");
    }
    let lower = body.to_lowercase();
    for word in [
        "★", "stars", "rating:", "rated", "hires", "hired", "reviews)", "summary",
    ] {
        assert!(!lower.contains(word), "{word}");
    }
    let status = position(&body, "role=\"status\"");
    let close = status + body[status..].find('>').expect("tag end");
    assert!(body[close..].starts_with("></p>"), "status region empty");
    assert!(!body.contains("still open"));
}

/// TD-PICK-38 (CON-028, D-372): after a sole-word CANCEL paused the live job, the page shows "Your
/// request <n> is still open. Close it on your quote page."
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_38_still_open_after_cancel(pool: PgPool) {
    let (job, token) = open_job(&pool).await;
    let (homeowner, conversation) = (Uuid::from_u128(71), Uuid::from_u128(72));
    let opened = at("02T20:00:00").to_sqlx();
    sqlx::query("INSERT INTO intake.homeowner (id, contact_id, created_at) VALUES ($1, $2, $3)")
        .bind(homeowner)
        .bind(Uuid::from_u128(73))
        .bind(opened)
        .execute(&pool)
        .await
        .expect("homeowner");
    sqlx::query(
        "INSERT INTO intake.conversation (id, homeowner_id, job_id, language, first_inbox_id, \
         consent_template, consent_sent_at, opened_at) VALUES ($1, $2, $3, 'en', $4, 'consent', $5, $5)",
    )
    .bind(conversation)
    .bind(homeowner)
    .bind(job)
    .bind(Uuid::from_u128(74))
    .bind(opened)
    .execute(&pool)
    .await
    .expect("conversation");
    sqlx::query(
        "INSERT INTO intake.homeowner_pause (conversation_id, job_id, inbox_id, paused_at) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(conversation)
    .bind(job)
    .bind(Uuid::from_u128(75))
    .bind(at("03T11:00:00").to_sqlx())
    .execute(&pool)
    .await
    .expect("pause");
    let mut db = pool.acquire().await.expect("db");
    let number = job_number(&mut db, job).await.expect("number");
    let body = page(&pool, &token).await;
    let line = format!("Your request {number} is still open. Close it on your quote page.");
    assert!(body.contains(&line), "{body}");
}
