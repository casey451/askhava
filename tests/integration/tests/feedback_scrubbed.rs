//! Her feedback words are scrubbed before they are stored (T819; JOB-017, PRIV-008, PRIV-001;
//! D-5399), on a fresh database through `portals::api::feedback_routes`.
//!
//! A Done job's quote-page link token opens `/f/{token}`; a ticked send holding a 555 phone
//! number, an `example.test` email and a street number with its street is stored with none of
//! them and with `[removed]`, and `jobs::api::feedback::shown` holds none of them either; a plain
//! send is stored as typed (the e2e texts of `tests/e2e/feedback.spec.ts` included). The router's
//! pool runs as `askhava_app`. Every word, number and address is fake.

use std::sync::Arc;

use askhava_core::api::{IdSecret, ManualClock};
use askhava_integration::set_app_role;
use askhava_jobs::api::feedback::shown;
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, create, move_to};
use askhava_ops::api::Actor;
use askhava_portals::api::{PRE_SESSION_COOKIE, feedback_routes};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const COOKIE: &str = "fake-pre-session-cookie-value-0819";
const CLOSED: &str = "2026-11-02T17:00:00Z";
const PHONE: &str = "928-555-0142";
const EMAIL: &str = "pat.sample@example.test";
const STREET: &str = "2150 Mesquite";

/// The arrows from Intake to Done (JOB-002).
const TO_DONE: [JobStatus; 7] = [
    JobStatus::Intake,
    JobStatus::Open,
    JobStatus::QuotesReady,
    JobStatus::AwaitingConfirm,
    JobStatus::PaymentPending,
    JobStatus::Connected,
    JobStatus::Done,
];

fn closed() -> Timestamp {
    CLOSED.parse().expect("instant")
}

async fn app_pool(pool: &PgPool) -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|conn, _| Box::pin(async move { set_app_role(conn).await }))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("app-role pool")
}

/// A Done job closed at [`CLOSED`] and its quote-page link token.
async fn done_job(db: &mut PgConnection) -> (Uuid, String) {
    let job = create(db, closed(), false, Actor::System)
        .await
        .expect("job");
    for pair in TO_DONE.windows(2) {
        let moved = move_to(db, job, (pair[0], pair[1]), closed(), Actor::System).await;
        assert!(moved.expect("move").applied().is_some(), "moved");
    }
    let secret = SecretString::from("test-only-id-secret-not-a-real-key-819".to_owned());
    let link = IdSecret::new(1, secret).expect("id");
    let token = issue(db, &link, job, closed()).await.expect("token");
    (job, token.as_str().to_owned())
}

/// `text` percent-encoded for a form body.
fn encoded(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Posts `body` (ticked when `share`) to `/f/<token>` as the browser would; the status.
async fn send(pool: &PgPool, token: &str, body: &str, share: bool) -> StatusCode {
    let clock = Arc::new(ManualClock::new(closed() + SignedDuration::from_hours(2)));
    let app = feedback_routes(app_pool(pool).await, clock);
    let csrf: String = Sha256::digest(format!("askhava.csrf.pre\0{COOKIE}").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut form = format!("csrf={csrf}&body={}", encoded(body));
    if share {
        form.push_str("&share=on");
    }
    let request = Request::post(format!("/f/{token}"))
        .header(header::HOST, "askhava.test")
        .header(header::ORIGIN, "https://askhava.test")
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, format!("{PRE_SESSION_COOKIE}={COOKIE}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form))
        .expect("request");
    app.oneshot(request).await.expect("response").status()
}

/// The stored feedback body of `job`.
async fn stored(pool: &PgPool, job: Uuid) -> String {
    sqlx::query_scalar("SELECT body FROM jobs.feedback WHERE job_id = $1")
        .bind(job)
        .fetch_one(pool)
        .await
        .expect("feedback row")
}

/// JOB-017, PRIV-008, PRIV-001 (D-5399): ticked feedback with a phone number, an email and a
/// street address is stored and shown with none of them and with `[removed]`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn ticked_feedback_is_stored_and_shown_without_her_contact(pool: PgPool) {
    let (job, token) = done_job(&mut pool.acquire().await.expect("conn")).await;
    let words = format!("Sample: fine work. Call {PHONE}, mail {EMAIL}, we live at {STREET} Dr.");
    assert_eq!(send(&pool, &token, &words, true).await, StatusCode::OK);
    let body = stored(&pool, job).await;
    for detail in [PHONE, "0142", EMAIL, "example.test", STREET, "2150"] {
        assert!(!body.contains(detail), "stored body keeps {detail}");
    }
    assert!(body.contains("[removed]"), "stored body: {body}");
    assert!(
        body.starts_with("Sample: fine work."),
        "stored body: {body}"
    );
    let mut conn = pool.acquire().await.expect("conn");
    let rows = shown(&mut conn).await.expect("shown");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].body, body);
}

/// PRIV-008 (D-5399): feedback with no contact detail, the e2e texts included, is stored as typed
/// (trimmed).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn plain_feedback_is_stored_as_typed(pool: PgPool) {
    for words in [
        "Sample: the visit was late and the work was poor.",
        "Sample: fine overall.",
    ] {
        let (job, token) = done_job(&mut pool.acquire().await.expect("conn")).await;
        let typed = format!("  {words} ");
        assert_eq!(send(&pool, &token, &typed, false).await, StatusCode::OK);
        assert_eq!(stored(&pool, job).await, words);
    }
}
