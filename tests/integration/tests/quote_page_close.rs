//! The quote page's close button through `portals`' quote page routes (TD-JOB-16: JOB-018,
//! JOB-019, SEC-042, SEC-048; T308, D-2128, D-3877). Fake IDs, words and tokens only.
//!
//! The served page of an open job shows a close form carrying its CSRF token and posting to
//! `CLOSE_ROUTE`, not the page's own path; that `POST` cancels the job through
//! `intake::api::close_from_page`, sends no text and answers with the page saying it is closed; a
//! second press changes nothing; a job already Done shows no button and a forged `POST` leaves it
//! Done; a cross-site `POST` or a wrong CSRF token is a 403 that changes nothing and is logged as a
//! `csrf_rejected` security event; an unknown token is a bare 404, but only after the CSRF check.

use std::sync::Arc;

use askhava_core::api::{IdSecret, LinkToken, ManualClock};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, current, job_number, move_to};
use askhava_ops::api::Actor;
use askhava_portals::api::{CLOSE_ROUTE, quote_page_routes};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

const HOST: &str = "askhava.test";

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

fn app(pool: &PgPool) -> Router {
    quote_page_routes(pool.clone(), Arc::new(ManualClock::new(at(30))))
}

/// A fake request from contact `n` with one open plumbing job; the job, its number and its token.
async fn open_job(pool: &PgPool, n: u128) -> (Uuid, i64, LinkToken) {
    let mut db = pool.acquire().await.expect("db");
    let id_secret = SecretString::from("test-only-id-secret-not-a-real-key".to_owned());
    let msg = TextIn {
        inbox_id: Uuid::from_u128(0x8_0000 + n),
        contact: Uuid::from_u128(n),
        inbound: Inbound {
            words: "my kitchen sink is leaking under the cabinet",
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(0),
    };
    let handled = receive_text(&mut db, &id_secret, &msg, Settings::default()).await;
    let start = JobStart {
        conversation: handled.expect("open").conversation.expect("request"),
        words: "my kitchen sink is leaking",
        emergency: false,
        trade: "plumbing",
        at: at(1),
    };
    let Opened::Created(job) = begin_job(&mut db, &start).await.expect("job") else {
        panic!("a new job");
    };
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(&mut db, job, open, at(2), Actor::System).await;
    assert!(moved.expect("open").applied().is_some());
    let key = SecretString::from("fake-id-secret-for-tests-00000001".to_owned());
    let secret = IdSecret::new(1, key).expect("secret");
    let token = issue(&mut db, &secret, job, at(2)).await.expect("token");
    (job, job_number(&mut db, job).await.expect("number"), token)
}

/// A browser that opened the page of `token`'s open job: its pre-session cookie and the close
/// form's CSRF token, as the page served them.
async fn browser(pool: &PgPool, token: &LinkToken) -> (String, String) {
    let request = Request::get(format!("/q/{}", token.as_str()))
        .header(header::HOST, HOST)
        .body(Body::empty())
        .expect("request");
    let response = app(pool).oneshot(request).await.expect("response");
    let set = response.headers().get(header::SET_COOKIE).expect("cookie");
    let set = set.to_str().expect("ascii");
    let cookie = set.split(';').next().expect("pair").to_owned();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    let body = String::from_utf8(bytes.to_vec()).expect("utf8");
    let (_, rest) = body
        .split_once("name=\"csrf\" value=\"")
        .expect("csrf field");
    let (csrf, _) = rest.split_once('"').expect("csrf value");
    (cookie, csrf.to_owned())
}

/// How the `POST` arrives: same-origin with the browser's token, a wrong token, or cross-site.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Site {
    Own,
    WrongToken,
    Cross,
}

/// `method path` from `browser`; the status and the body.
async fn send(
    pool: &PgPool,
    (method, path): (&str, &str),
    (cookie, csrf): &(String, String),
    site: Site,
) -> (u16, String) {
    let csrf = if site == Site::WrongToken {
        "0".repeat(64)
    } else {
        csrf.clone()
    };
    let fetch = if site == Site::Cross {
        "cross-site"
    } else {
        "same-origin"
    };
    let origin = if site == Site::Cross {
        "https://elsewhere.test".to_owned()
    } else {
        format!("https://{HOST}")
    };
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, origin)
        .header("sec-fetch-site", fetch)
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("csrf={csrf}")))
        .expect("request");
    let response = app(pool).oneshot(request).await.expect("response");
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf8"))
}

fn close_path(token: &LinkToken) -> String {
    CLOSE_ROUTE.replace("{token}", token.as_str())
}

async fn status_of(pool: &PgPool, job: Uuid) -> (JobStatus, Timestamp) {
    let mut db = pool.acquire().await.expect("db");
    let row = current(&mut db, job).await.expect("job");
    (row.status, row.status_at)
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn texts(pool: &PgPool) -> i64 {
    count(pool, "SELECT count(*) FROM messaging.outbox").await
}

async fn refusals(pool: &PgPool) -> i64 {
    let sql = "SELECT count(*) FROM portals.security_event WHERE kind = 'csrf_rejected'";
    count(pool, sql).await
}

/// TD-JOB-16 (JOB-018, JOB-019): the open job's page shows the close form with its CSRF token,
/// posting to `/q/{token}/close`; the press cancels the job, sends no text and says it is closed;
/// a second press changes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_16_the_close_button_closes_once_and_sends_no_text(pool: PgPool) {
    let (job, number, token) = open_job(&pool, 0x9501).await;
    let b = browser(&pool, &token).await;
    let page = format!("/q/{}", token.as_str());
    let (status, body) = send(&pool, ("GET", &page), &b, Site::Own).await;
    assert_eq!(status, 200);
    let form = format!(
        "<form method=\"post\" action=\"{}\" novalidate>",
        close_path(&token)
    );
    assert!(body.contains(&form), "{body}");
    assert!(body.contains("<input type=\"hidden\" name=\"csrf\" value=\""));
    assert!(body.contains(&format!("Close request {number}</button>")));
    assert!(!body.contains("is closed."));
    let sent = texts(&pool).await;
    let (status, body) = send(&pool, ("POST", &close_path(&token)), &b, Site::Own).await;
    assert_eq!(status, 200);
    assert!(
        body.contains(&format!("Request {number} is closed.")),
        "{body}"
    );
    assert!(!body.contains("</button>"), "no button once closed");
    let closed = status_of(&pool, job).await;
    assert_eq!(closed.0.code(), "cancelled");
    assert_eq!(texts(&pool).await, sent, "no text");
    let (status, body) = send(&pool, ("POST", &close_path(&token)), &b, Site::Own).await;
    assert_eq!(status, 200);
    assert!(body.contains(&format!("Request {number} is closed.")));
    assert_eq!(
        status_of(&pool, job).await,
        closed,
        "a second press changes nothing"
    );
    assert_eq!(texts(&pool).await, sent);
}

/// TD-JOB-16 (JOB-018): a job already Done shows no close button, and a forged `POST` for it
/// leaves it Done and sends nothing; an unknown token is a bare 404.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_16_a_final_job_has_no_button_and_a_forged_post_changes_nothing(pool: PgPool) {
    let (job, _, token) = open_job(&pool, 0x9502).await;
    let b = browser(&pool, &token).await;
    sqlx::query("UPDATE jobs.job SET status = 'done' WHERE id = $1")
        .bind(job)
        .execute(&pool)
        .await
        .expect("done");
    let done = status_of(&pool, job).await;
    let page = format!("/q/{}", token.as_str());
    let (status, body) = send(&pool, ("GET", &page), &b, Site::Own).await;
    assert_eq!(status, 200);
    assert!(!body.contains("/close"), "{body}");
    let sent = texts(&pool).await;
    let (status, _) = send(&pool, ("POST", &close_path(&token)), &b, Site::Own).await;
    assert_eq!(status, 200);
    assert_eq!(status_of(&pool, job).await, done);
    assert_eq!(texts(&pool).await, sent);
    let unknown = CLOSE_ROUTE.replace("{token}", &"a".repeat(64));
    let (status, body) = send(&pool, ("POST", &unknown), &b, Site::Own).await;
    assert_eq!((status, body.as_str()), (404, ""));
}

/// TD-JOB-16 (SEC-042, SEC-048): a cross-site `POST` and a wrong CSRF token are each a 403 that
/// leaves the job open and is logged as a `csrf_rejected` security event.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_16_cross_site_or_wrong_token_is_refused_and_logged(pool: PgPool) {
    let (job, _, token) = open_job(&pool, 0x9503).await;
    let b = browser(&pool, &token).await;
    let open = status_of(&pool, job).await;
    for (n, site) in [(1, Site::Cross), (2, Site::WrongToken)] {
        let (status, _) = send(&pool, ("POST", &close_path(&token)), &b, site).await;
        assert_eq!(status, 403);
        assert_eq!(status_of(&pool, job).await, open, "changes nothing");
        assert_eq!(refusals(&pool).await, n, "logged as a security event");
    }
}

/// TD-JOB-16 (SEC-042, SEC-048; D-3877 (2)): the CSRF check comes before the token lookup, so a
/// cross-site `POST` or a wrong CSRF token to a made-up token's close path is a logged 403, never
/// the 404 that would tell a forger which tokens exist.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_16_an_unknown_token_is_refused_by_csrf_before_its_404(pool: PgPool) {
    let (_, _, token) = open_job(&pool, 0x9504).await;
    let b = browser(&pool, &token).await;
    let unknown = CLOSE_ROUTE.replace("{token}", &"b".repeat(64));
    for (n, site) in [(1, Site::Cross), (2, Site::WrongToken)] {
        let (status, _) = send(&pool, ("POST", &unknown), &b, site).await;
        assert_eq!(status, 403, "CSRF refuses before the token is looked up");
        assert_eq!(refusals(&pool).await, n, "logged as a security event");
    }
}
