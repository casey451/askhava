//! TD-INT-51 (CON-031, CON-001, CON-015, INT-043, INT-005; T691, D-4360, D-4441): through the
//! served inbound text route (`app/server/src/inbound_text.rs`, compiled in with `#[path]`,
//! deliveries signed with a fake Twilio token), STOP then START into a still-open request gets
//! the consent text and then the request's open question once more, both under the START inbox
//! row: the ZIP question, "How soon?" or the shown card; a request waiting for SAFE gets only
//! the consent text. A resent START delivery queues nothing more, START writes no
//! `intake_answer` row with its inbox ID, and the re-ask is not counted as an ask, so a later
//! unusable ZIP reply still gets its one re-ask. Every first text names one trade and one job
//! type of it (D-4441); every number, name and word is fake.

use std::collections::BTreeSet;
use std::sync::Arc;

use askhava_businesses::api::{
    Checks, Event, JobTypeChoice, add_owner, apply, create, request_welcome, save_business_name,
    set_job_types, set_towns, welcome_sent,
};
use askhava_catalog::api::load_embedded;
use askhava_core::api::{Environment, ManualClock};
use askhava_intake::api::rules::{CONSENT_TEMPLATE, Settings as IntakeSettings};
use askhava_intake::api::{
    CARD_SHOWN_TEMPLATE, HOW_SOON_TEMPLATE, ZIP_AGAIN_TEMPLATE, ZIP_QUESTION_TEMPLATE,
};
use askhava_licensing::api::import_city_list;
use askhava_ops::api::Actor;
use askhava_privacy::api::Vault;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::{INBOUND_PATH, Webhooks, routes};
use jiff::civil::date;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sha1::Sha1;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tower::ServiceExt;

/// The route under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_text.rs"]
mod inbound_text;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const ID_SECRET: &str = "test-only-id-secret-for-t691-not-a-real-key";
const HOME: &str = "+19285550169";
const LINE: &str = "+19285550100";
/// A first text naming one trade (`handyman`) and one job type of it, door repair (D-4441).
const DOOR: &str = "my front door is sticking badly";
const TOWN: &str = "lake_havasu_city";
/// The fake city list the handyman business is matched on.
const CITY: &str = "license_number,business_name,trade_name,owner_name,issued_on\n\
CL-7691,Sample Fixit One LLC,,,2019-03-04\n";
/// START's queued texts: (template, subject holds the START inbox row).
const START_TEXTS: &str = "SELECT template_id FROM messaging.outbox \
                           WHERE $1 = ANY(subject_ids) ORDER BY template_id";
const START_ROWS: &str =
    "SELECT count(*) FROM intake.intake_answer WHERE $1::uuid IS NULL OR inbox_id = $1";

/// `minute` minutes after 11:00 Arizona on a fixed day.
fn at(minute: i64) -> Timestamp {
    let start: Timestamp = "2026-11-02T11:00:00-07:00".parse().expect("time");
    start + SignedDuration::from_mins(minute)
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x69), &key(0x96), &url).expect("vault")
}

fn hooks(pool: &PgPool, now: Timestamp) -> Arc<Webhooks> {
    Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(now)),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: SecretString::from(ID_SECRET.to_owned()),
        vault: vault(),
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    })
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Texts `words` from `HOME` as MessageSid number `minute` (a resend reuses it) with Twilio's
/// `OptOutType` when given, signed, at [`at`]`(minute)`; answers the inbox row's ID.
async fn text(pool: &PgPool, minute: i64, words: &str, opt_out_type: Option<&str>) -> Uuid {
    let sid = format!("SM{minute:032}");
    let mut fields = vec![
        ("MessageSid", sid.as_str()),
        ("From", HOME),
        ("To", LINE),
        ("Body", words),
    ];
    fields.extend(opt_out_type.map(|code| ("OptOutType", code)));
    let body: Vec<String> = fields
        .iter()
        .map(|(n, v)| format!("{n}={}", encode(v)))
        .collect();
    fields.sort_unstable();
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(TOKEN.as_bytes()).expect("key");
    mac.update(format!("{BASE}{INBOUND_PATH}").as_bytes());
    for (name, value) in fields {
        mac.update(name.as_bytes());
        mac.update(value.as_bytes());
    }
    let request = Request::post(INBOUND_PATH)
        .header(
            "X-Twilio-Signature",
            STANDARD.encode(mac.finalize().into_bytes()),
        )
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(body.join("&")))
        .expect("request");
    let answer = routes(hooks(pool, at(minute)))
        .oneshot(request)
        .await
        .expect("answer");
    assert_eq!(answer.status(), StatusCode::OK);
    sqlx::query_scalar("SELECT id FROM messaging.inbox WHERE message_sid = $1")
        .bind(&sid)
        .fetch_one(pool)
        .await
        .expect("inbox row")
}

/// The templates queued under inbox row `inbox`, sorted by name.
async fn queued(pool: &PgPool, inbox: Uuid) -> Vec<String> {
    sqlx::query_scalar(START_TEXTS)
        .bind(inbox)
        .fetch_all(pool)
        .await
        .expect("texts")
}

/// The `intake_answer` rows carrying inbox row `inbox`, or every row with `None`.
async fn rows(pool: &PgPool, inbox: Option<Uuid>) -> i64 {
    sqlx::query_scalar(START_ROWS)
        .bind(inbox)
        .fetch_one(pool)
        .await
        .expect("rows")
}

/// The catalog, the fake city list and one active, welcomed handyman business serving the town
/// with door repair ticked (LIC-046's one, D-4421).
async fn business(pool: &PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    load_embedded(&mut db).await.expect("catalog");
    let listed = import_city_list(&mut db, date(2026, 11, 1), CITY).await;
    listed.expect("city list");
    let b = create(&mut db, false, at(-60)).await.expect("business");
    sqlx::query(
        "INSERT INTO businesses.signup (business_id, mailbox_id, submitted_at, trades) \
         VALUES ($1, $2, now(), ARRAY['handyman'])",
    )
    .bind(b)
    .bind(Uuid::new_v4())
    .execute(&mut *db)
    .await
    .expect("signed up as a handyman");
    let owner = add_owner(&mut db, b, Uuid::new_v4(), at(-60)).await;
    owner.expect("owner");
    let ok = Checks {
        code_done: true,
        match_passed: true,
        flagged: false,
    };
    let checked = apply(&mut db, b, Event::Checked(ok), Actor::System, at(-60)).await;
    checked.expect("active");
    request_welcome(&mut db, b, at(-60)).await.expect("welcome");
    welcome_sent(&mut db, b, at(-60)).await.expect("sent");
    let named = save_business_name(&mut db, b, ("Sample Fixit One LLC", None), None).await;
    named.expect("name");
    let actor = Actor::Business(b);
    set_towns(&mut db, b, &[TOWN], actor).await.expect("towns");
    let choice = JobTypeChoice::new("handyman", "door repair");
    let allowed: BTreeSet<_> = [choice.clone()].into();
    let ticked = set_job_types(&mut db, b, &[choice], &allowed, actor).await;
    ticked.expect("job types");
}

/// Sends `words` from minute 0 on, then STOP and START (and START once more as a resend of the
/// same delivery); answers the START inbox row and the `intake_answer` count before STOP.
async fn stop_start(pool: &PgPool, words: &[&str]) -> (Uuid, i64) {
    for (minute, words) in (0..).zip(words) {
        text(pool, minute, words, None).await;
    }
    let before = rows(pool, None).await;
    text(pool, 50, "STOP", Some("STOP")).await;
    let start = text(pool, 51, "START", Some("START")).await;
    text(pool, 51, "START", Some("START")).await;
    (start, before)
}

/// TD-INT-51 (CON-031, CON-015, INT-043): STOP then START while her request waits on its ZIP
/// gets the consent text then the ZIP question once more; a resent START queues nothing more;
/// START writes no row, so a later unusable ZIP reply still gets its one re-ask.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_51_start_at_the_zip_step_resends_consent_then_the_zip_question(pool: PgPool) {
    let (start, before) = stop_start(&pool, &[DOOR]).await;
    let mut want = vec![CONSENT_TEMPLATE, ZIP_QUESTION_TEMPLATE];
    want.sort_unstable();
    assert_eq!(
        queued(&pool, start).await,
        want,
        "consent, then the ZIP once"
    );
    assert_eq!(rows(&pool, Some(start)).await, 0, "START is no answer");
    assert_eq!(rows(&pool, None).await, before, "the re-ask writes no row");
    let unusable = text(&pool, 52, "not sure yet", None).await;
    assert_eq!(
        queued(&pool, unusable).await,
        [ZIP_AGAIN_TEMPLATE],
        "one re-ask kept"
    );
}

/// TD-INT-51 (CON-031, INT-043, INT-030): at the "How soon?" step, START gets the consent text
/// then "How soon?" again, with no row written.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_51_start_at_how_soon_asks_it_again(pool: PgPool) {
    business(&pool).await;
    let (start, before) = stop_start(&pool, &[DOOR, "86403"]).await;
    let mut want = vec![CONSENT_TEMPLATE, HOW_SOON_TEMPLATE];
    want.sort_unstable();
    assert_eq!(queued(&pool, start).await, want);
    assert_eq!(rows(&pool, Some(start)).await, 0, "START is no answer");
    assert_eq!(rows(&pool, None).await, before, "the re-ask writes no row");
}

/// TD-INT-51 (INT-005, CON-001): a request waiting for SAFE gets only the consent text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_51_start_while_waiting_for_safe_gets_only_the_consent_text(pool: PgPool) {
    let (start, _) = stop_start(&pool, &["I smell gas by my water heater"]).await;
    assert_eq!(queued(&pool, start).await, [CONSENT_TEMPLATE]);
    assert_eq!(rows(&pool, Some(start)).await, 0, "START is no answer");
}

/// TD-INT-51 (INT-041, INT-043, CON-031): a request whose card is shown gets the consent text
/// (under the START inbox row) then the card text again (under the card's own subject); the
/// resent START shows nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_51_start_with_a_shown_card_shows_it_again(pool: PgPool) {
    business(&pool).await;
    let words = [DOOR, "86403", "2", "door repair", "1", "not sure"];
    let last = ["it sticks at the top", "Testa 12 Sample Ave"];
    let all: Vec<&str> = words.into_iter().chain(last).collect();
    let (start, _) = stop_start(&pool, &all).await;
    assert_eq!(queued(&pool, start).await, [CONSENT_TEMPLATE]);
    let shown = "SELECT count(*) FROM messaging.outbox WHERE template_id = $1";
    let shown: i64 = sqlx::query_scalar(shown)
        .bind(CARD_SHOWN_TEMPLATE)
        .fetch_one(&pool)
        .await
        .expect("cards");
    assert_eq!(
        shown, 2,
        "shown once, then once again after START, not for the resend"
    );
    assert_eq!(rows(&pool, Some(start)).await, 0, "START is no answer");
}
