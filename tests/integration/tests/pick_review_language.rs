//! T589 (PICK-016, PICK-049, TXT-039, CON-026; D-4052 item B42): the pick review step is shown in
//! the request's language and names the business. A Spanish request with Spanish conversations
//! on gets the Spanish Terms line and the business named in the share box; with the switch off it
//! is English; an English request is English. Fake numbers (555-01xx), IDs, names and words only.

use std::sync::Arc;

use askhava_core::api::{IdSecret, ManualClock};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::quote_token::issue;
use askhava_jobs::api::{JobStatus, move_to, record_job_facts};
use askhava_messaging::api::{SpanishRecords, spanish_template_versions, turn_spanish_on};
use askhava_ops::api::Actor;
use askhava_portals::api::{PRE_SESSION_COOKIE, pick_flow_routes};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sha2::{Digest, Sha256};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const COOKIE: &str = "fake-pre-session-cookie-value-0589";
const DAY: &str = "2026-11-02T17:00:00Z";
const BUSINESS: Uuid = Uuid::from_u128(0xb589);
const NAME: &str = "Sample Pipe Co 589";
const SPANISH: &str = "Hola, necesito ayuda, mi lavabo gotea";
const ENGLISH: &str = "my kitchen sink is leaking under the cabinet";
const ES_TERMS: &str = "<p id=\"terms-line\" lang=\"es\">Al elegir, usted acepta los <a href=\"/terms\">Términos</a></p>";
const EN_TERMS: &str = "<p id=\"terms-line\" lang=\"en\">By picking, you agree to the <a href=\"/terms\">Terms</a></p>";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x58), &key(0x59), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-589".to_owned())
}

/// A request opened by `words` (Spanish conversations `spanish` while it opens) with one plumbing
/// job in Quotes ready holding [`BUSINESS`]'s confirmed quote; its quote-page link.
async fn ready(db: &mut PgConnection, words: &str, spanish: bool) -> String {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from("+19285550189".to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let inbound = Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let msg = TextIn {
        inbox_id: Uuid::new_v4(),
        contact: her,
        inbound,
        received_at: ts(DAY),
    };
    let settings = Settings {
        spanish_conversations_on: spanish,
        ..Settings::default()
    };
    let handled = receive_text(db, &secret(), &msg, settings).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words,
        emergency: false,
        trade: "plumbing",
        at: ts(DAY),
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    askhava_catalog::api::load_embedded(db)
        .await
        .expect("catalog");
    let types = ["leak repair".to_owned()];
    record_job_facts(db, job, "plumbing_repair", &types)
        .await
        .expect("job facts");
    quoted(db, job).await;
    let link = IdSecret::new(1, secret()).expect("id");
    let token = issue(db, &link, job, ts(DAY)).await.expect("token");
    token.as_str().to_owned()
}

/// Moves `job` to Quotes ready with [`BUSINESS`], named [`NAME`], holding its one confirmed quote.
async fn quoted(db: &mut PgConnection, job: Uuid) {
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, ts(DAY), Actor::System).await;
        assert!(moved.expect("move").applied().is_some());
    }
    for sql in [
        "INSERT INTO businesses.business (id, status, created_at, status_changed_at) \
         VALUES ($1, 'active', now(), now())",
        "INSERT INTO businesses.business_name (business_id, business_name, changed_at) \
         VALUES ($1, $2, now())",
    ] {
        let query = sqlx::query(sql).bind(BUSINESS);
        let query = if sql.contains("$2") {
            query.bind(NAME)
        } else {
            query
        };
        query.execute(&mut *db).await.expect("business");
    }
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
         confirm_on_site, notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
    )
    .bind(job)
    .bind(BUSINESS)
    .bind((ts(DAY) + SignedDuration::from_mins(1)).to_sqlx())
    .execute(&mut *db)
    .await
    .expect("quote");
}

/// Turns Spanish conversations on with a fake review of every Spanish template.
async fn spanish_on(db: &mut PgConnection) {
    let records = SpanishRecords {
        reviewed_on: Some("2026-11-02".parse().expect("date")),
        reviewer: "FAKE-T589-REVIEWER".to_owned(),
        reviewed: spanish_template_versions().expect("versions"),
        note_on: Some("2026-11-03".parse().expect("date")),
        note_reference: "FAKE-T589-CAMPAIGN".to_owned(),
        sample: "FAKE-T589-SAMPLE Hola, recibimos su solicitud.".to_owned(),
    };
    let owner = Uuid::from_u128(0x589);
    let on = turn_spanish_on(db, owner, &records, ts(DAY)).await;
    assert!(on.is_ok(), "switch on");
}

/// The review step of quote #1 behind link `token`, as the browser posts it.
async fn review(pool: &PgPool, token: &str) -> String {
    let clock = Arc::new(ManualClock::new(ts(DAY)));
    let app = pick_flow_routes::<askhava_adapters::api::TwilioVerify>(pool.clone(), clock, None);
    let csrf: String = Sha256::digest(format!("askhava.csrf.pre\0{COOKIE}").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let body = format!("csrf={csrf}&quote=1&business={BUSINESS}");
    let request = Request::post(format!("/q/{token}"))
        .header(header::HOST, "askhava.test")
        .header(header::ORIGIN, "https://askhava.test")
        .header("sec-fetch-site", "same-origin")
        .header(header::COOKIE, format!("{PRE_SESSION_COOKIE}={COOKIE}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .expect("request");
    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), 200, "the review step");
    let bytes = to_bytes(response.into_body(), 1 << 20).await.expect("body");
    String::from_utf8(bytes.to_vec()).expect("utf8")
}

/// PICK-016, PICK-049, TXT-039: a Spanish request with Spanish conversations on gets the Spanish
/// Terms line and the share box naming the business.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_spanish_request_with_the_switch_on_gets_the_spanish_review(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let token = ready(&mut db, SPANISH, true).await;
    spanish_on(&mut db).await;
    let page = review(&pool, &token).await;
    assert!(page.contains(ES_TERMS), "PICK-016's Spanish line: {page}");
    let share =
        format!("Compartir mi nombre de pila y la dirección del trabajo con {NAME} cuando acepte.");
    assert!(page.contains(&share), "PICK-049 names the business");
    assert!(!page.contains(EN_TERMS), "no English Terms line");
}

/// TXT-039, CON-026: the same Spanish request with the switch off is English, naming the business.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_spanish_request_with_the_switch_off_is_english(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let token = ready(&mut db, SPANISH, true).await;
    let page = review(&pool, &token).await;
    assert!(page.contains(EN_TERMS), "English Terms line");
    assert!(!page.contains(ES_TERMS), "no Spanish line");
    let share = format!("Share my first name and the job address with {NAME} once they accept.");
    assert!(page.contains(&share), "PICK-049 names the business");
}

/// CON-026: an English request is English even with Spanish conversations on.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_english_request_is_english(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let token = ready(&mut db, ENGLISH, true).await;
    spanish_on(&mut db).await;
    let page = review(&pool, &token).await;
    assert!(page.contains(EN_TERMS), "English Terms line");
    assert!(!page.contains(ES_TERMS), "no Spanish line");
}
