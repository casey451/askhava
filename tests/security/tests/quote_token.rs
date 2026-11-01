//! The quote page's token and its refusals (TD-PICK-05, TD-SEC-16; PICK-010, PICK-011, SEC-037,
//! SEC-042, D-480, D-491). Fake IDs and a fake secret only.
//!
//! The token is 64 hex digits over a 16-byte salt from the OS random source, stored only as its
//! hash; the page is `noindex` and `no-store` and shows no name, phone or address; the token works
//! 29 days 23:59:59 after the job closes and is a 404 at 30 days; a re-issued link's old token,
//! a revoked token and a malformed one are 404; a cross-site POST to the page is refused. The
//! quote-page terms acceptance of TD-PICK-05 is the pick step's (T055); the environment and replay
//! cases of TD-SEC-16 are `jobs`' and `core`'s (`webhook_dedupe.rs`, `prop_derived_ids.rs`).

use std::sync::Arc;

use askhava_core::api::{IdSecret, LinkToken, ManualClock};
use askhava_jobs::api::quote_token::{issue, revoke};
use askhava_jobs::api::{JobStatus, create, move_to};
use askhava_ops::api::Actor;
use askhava_portals::api::quote_page_routes;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

const HOST: &str = "askhava.test";

fn start() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("time")
}

fn secret() -> IdSecret {
    let key = SecretString::from("fake-id-secret-for-tests-00000001".to_owned());
    IdSecret::new(1, key).expect("secret")
}

async fn open_job(pool: &PgPool) -> Uuid {
    let mut db = pool.acquire().await.expect("db");
    let job = create(&mut db, start(), false, Actor::System)
        .await
        .expect("job");
    let open = (JobStatus::Intake, JobStatus::Open);
    let _ = move_to(&mut db, job, open, start(), Actor::System)
        .await
        .expect("open");
    job
}

async fn token_for(pool: &PgPool, job: Uuid) -> LinkToken {
    let mut db = pool.acquire().await.expect("db");
    issue(&mut db, &secret(), job, start())
        .await
        .expect("token")
}

fn page(pool: &PgPool, clock: &Arc<ManualClock>) -> Router {
    quote_page_routes(pool.clone(), Arc::clone(clock) as _)
}

async fn get(app: Router, token: &str) -> (StatusCode, axum::http::HeaderMap, String) {
    let request = Request::get(format!("/q/{token}"))
        .header(header::HOST, HOST)
        .body(Body::empty())
        .expect("request");
    let response = app.oneshot(request).await.expect("response");
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 1 << 20).await.expect("body");
    let text = String::from_utf8(bytes.to_vec()).expect("utf8");
    (parts.status, parts.headers, text)
}

/// TD-PICK-05 (PICK-010, SEC-037): 64 hex digits from a fresh 16-byte OS-random salt, only its
/// hash stored, never the same twice; the page is `noindex` and `no-store` and holds no phone,
/// email or street address.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_05_token_random_page_noindex_no_contact(pool: PgPool) {
    let job = open_job(&pool).await;
    let first = token_for(&pool, job).await;
    let second = token_for(&pool, job).await;
    assert_ne!(first, second);
    assert_eq!(second.as_str().len(), 64);
    let row: (i32, Vec<u8>) = sqlx::query_as(
        "SELECT octet_length(salt), token_hash FROM jobs.quote_token WHERE revoked_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("row");
    assert_eq!(row, (16, second.hash().to_vec()));
    let clock = Arc::new(ManualClock::new(start()));
    let (status, headers, body) = get(page(&pool, &clock), second.as_str()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["x-robots-tag"], "noindex, nofollow");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(!body.contains('@') && !body.contains("tel:"), "{body}");
    let words = body.split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '.');
    let words: Vec<&str> = words.filter(|w| w.len() != 64).collect();
    for word in words {
        let digits = word.chars().filter(char::is_ascii_digit).count();
        assert!(digits < 7, "no phone-length number: {word}");
    }
    for street in [" St.", " Street", " Ave", " Dr."] {
        assert!(!body.contains(street), "{street}");
    }
}

/// TD-PICK-05 (PICK-011): after the job closes the token works for 29 days 23:59:59 and is a 404
/// from 30 days on.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_05_token_stops_30_days_after_close(pool: PgPool) {
    let job = open_job(&pool).await;
    let token = token_for(&pool, job).await;
    let closed_at = start() + SignedDuration::from_hours(2);
    let mut db = pool.acquire().await.expect("db");
    let close = (JobStatus::Open, JobStatus::Cancelled);
    let _ = move_to(&mut db, job, close, closed_at, Actor::System)
        .await
        .expect("close");
    let thirty = SignedDuration::from_hours(30 * 24);
    let clock = Arc::new(ManualClock::new(
        closed_at + thirty - SignedDuration::from_secs(1),
    ));
    let (status, _, body) = get(page(&pool, &clock), token.as_str()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("Pick #"), "a closed job takes no pick");
    clock.set(closed_at + thirty);
    let (status, _, _) = get(page(&pool, &clock), token.as_str()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// TD-SEC-16 (SEC-037): a re-issued link's old token, a revoked token and a malformed one are 404.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_16_reissued_and_revoked_tokens_are_404(pool: PgPool) {
    let job = open_job(&pool).await;
    let old = token_for(&pool, job).await;
    let new = token_for(&pool, job).await;
    let clock = Arc::new(ManualClock::new(start()));
    assert_eq!(
        get(page(&pool, &clock), old.as_str()).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(page(&pool, &clock), new.as_str()).await.0,
        StatusCode::OK
    );
    let mut db = pool.acquire().await.expect("db");
    assert_eq!(revoke(&mut db, job, start()).await, Ok(true));
    assert_eq!(
        get(page(&pool, &clock), new.as_str()).await.0,
        StatusCode::NOT_FOUND
    );
    for bad in ["sample-token", &"A".repeat(64), &"0".repeat(64)] {
        assert_eq!(get(page(&pool, &clock), bad).await.0, StatusCode::NOT_FOUND);
    }
}

/// TD-PICK-05 (SEC-042, D-491): a cross-site POST to the quote page is refused before any handler.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_pick_05_cross_site_post_is_refused(pool: PgPool) {
    let job = open_job(&pool).await;
    let token = token_for(&pool, job).await;
    let clock = Arc::new(ManualClock::new(start()));
    let request = Request::post(format!("/q/{}", token.as_str()))
        .header(header::HOST, HOST)
        .header("sec-fetch-site", "cross-site")
        .header(header::ORIGIN, "https://elsewhere.test")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("quote=1"))
        .expect("request");
    let response = page(&pool, &clock)
        .oneshot(request)
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
