//! TD-CON-33 (CON-001, CON-015, CON-016, CON-028, CON-029, CON-030; T357, D-2629, D-3118):
//! through the served inbound text route (`app/server/src/inbound_text.rs`, compiled in with
//! `#[path]`, deliveries signed with a fake Twilio token): START into a still-open request gets
//! the consent text again under the START inbox row and is never heard as an answer; an opt-out
//! while the homeowner line is suspended queues one follow-up task and, once
//! `messaging::api::run_optout_followup` runs with a fixture postal line, one confirmation email
//! and no text; a sole CANCEL with a pick waiting for Confirm pauses the job and queues one
//! `intake.opted_out_live_job`; a listed homeowner who writes again is emailed (email on file) or
//! listed by one `messaging.optout_wrote_again` audit row, and an emergency in it still raises
//! the CON-017 alert. Every name, phone, email and word is fake.

use std::sync::Arc;

use askhava_businesses::api::{add_owner, create};
use askhava_core::api::{Claimed, Environment, Guarded, ManualClock};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{
    JobStart, OPTED_OUT_LIVE_KIND, Opened, PublishedTerms, TextIn, answer_text_pick, begin_job,
};
use askhava_jobs::api::{JobStatus, move_to, record_job_facts};
use askhava_messaging::api::{
    Audience, MailboxKind, OPTOUT_CONFIRMATION_EMAIL, OPTOUT_FOLLOWUP_KIND,
    OPTOUT_WROTE_AGAIN_EMAIL, OptOutEmailSender, RevokeChannel, WROTE_AGAIN_ACTION,
    record_suspension, run_optout_followup, suppress,
};
use askhava_ops::api::Actor;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::{INBOUND_PATH, Webhooks, routes};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
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
const ID_SECRET: &str = "test-only-id-secret-for-t357-not-a-real-key";
const HOME: &str = "+19285550157";
const ASK_HAVA: &str = "+19285550100";
const LEAK: &str = "my kitchen sink is leaking under the cabinet";
const TERMS: PublishedTerms<'static> = PublishedTerms {
    version: "2026-11-01",
    text: "Sample homeowner terms for tests only.",
};

fn at() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x57), &key(0x67), &url).expect("vault")
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

/// Texts `words` from `HOME` as MessageSid number `sid` with Twilio's `OptOutType` when given,
/// signed, at [`at`]; answers the inbox row's ID.
async fn text(pool: &PgPool, sid: u32, words: &str, opt_out_type: Option<&str>) -> Uuid {
    text_at(pool, (sid, words, opt_out_type), at()).await
}

/// [`text`] at `now`.
async fn text_at(
    pool: &PgPool,
    (sid, words, opt_out_type): (u32, &str, Option<&str>),
    now: Timestamp,
) -> Uuid {
    let sid = format!("SM{sid:032}");
    let mut fields = vec![
        ("MessageSid", sid.as_str()),
        ("From", HOME),
        ("To", ASK_HAVA),
        ("Body", words),
    ];
    if let Some(code) = opt_out_type {
        fields.push(("OptOutType", code));
    }
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
    let answer = routes(hooks(pool, now))
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

/// Her vault contact at `HOME`, with a fake email when `email`.
async fn her(pool: &PgPool, email: bool) -> Uuid {
    let new = NewContact {
        name: Some(SecretString::from("Dana Testperson")),
        phone: Some(SecretString::from(HOME)),
        email: email.then(|| SecretString::from("dana.testperson@example.invalid")),
        address: None,
        onsite_contact: None,
    };
    let mut db = pool.acquire().await.expect("conn");
    store_contact(&mut db, &vault(), &new)
        .await
        .expect("contact")
}

async fn count(pool: &PgPool, sql: &'static str, id: Uuid) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn count_kind(pool: &PgPool, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(kind)
        .fetch_one(pool)
        .await
        .expect("tasks")
}

/// Runs every queued follow-up task once with a fixture postal line.
async fn run_followups(pool: &PgPool) {
    let secret = SecretString::from("fake-email-key-secret-0000000357".to_owned());
    let postal = (
        "PO Box 0000, Lake Havasu City, AZ 86403",
        MailboxKind::PoBox,
    );
    let sender = Arc::new(OptOutEmailSender::new(secret, postal, vault()).expect("sender"));
    let rows: Vec<(i64, serde_json::Value)> =
        sqlx::query_as("SELECT id, payload FROM core.task WHERE kind = $1 ORDER BY id")
            .bind(OPTOUT_FOLLOWUP_KIND)
            .fetch_all(pool)
            .await
            .expect("tasks");
    for (id, payload) in rows {
        let task = Claimed {
            id,
            kind: OPTOUT_FOLLOWUP_KIND.to_owned(),
            payload,
            attempts: 1,
        };
        let run = run_optout_followup(pool.clone(), Some(Arc::clone(&sender)), task).await;
        run.expect("follow-up");
    }
}

/// Emails of `template` stored for `contact`.
async fn emails(pool: &PgPool, template: &str, contact: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM messaging.email_outbox WHERE template_id = $1 AND recipient_id = $2",
    )
    .bind(template)
    .bind(contact)
    .fetch_one(pool)
    .await
    .expect("emails")
}

/// TD-CON-33 (CON-001, CON-015): open request, STOP, then START: one consent text under the START
/// inbox row, and "START" is no intake answer; a resend of the START adds nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_33_start_into_an_open_request_gets_the_consent_text_again(pool: PgPool) {
    text(&pool, 1, LEAK, None).await;
    text(&pool, 2, "STOP", Some("STOP")).await;
    let start = text(&pool, 3, "START", Some("START")).await;
    text(&pool, 3, "START", Some("START")).await;
    let consent = "SELECT count(*) FROM messaging.outbox \
                   WHERE template_id = 'consent' AND $1 = ANY(subject_ids)";
    assert_eq!(count(&pool, consent, start).await, 1, "one consent text");
    let answers = "SELECT count(*) FROM intake.intake_answer WHERE inbox_id = $1";
    assert_eq!(count(&pool, answers, start).await, 0, "START is no answer");
    let record = "SELECT count(*) FROM intake.consent_record WHERE inbox_id = $1";
    assert_eq!(count(&pool, record, start).await, 1, "recorded as sent");
}

/// TD-CON-33 (CON-001, CON-015, D-3480): a START delivery retried after its first try lifted the
/// listing (the number is no longer listed when the retry runs) still restarts the open request
/// with the consent text and is no intake answer.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_33_a_retried_start_still_gets_the_consent_text(pool: PgPool) {
    text(&pool, 4, LEAK, None).await;
    text(&pool, 5, "STOP", Some("STOP")).await;
    sqlx::query("DELETE FROM messaging.suppression")
        .execute(&pool)
        .await
        .expect("lift committed by the first try");
    let start = text(&pool, 6, "START", Some("START")).await;
    let consent = "SELECT count(*) FROM messaging.outbox \
                   WHERE template_id = 'consent' AND $1 = ANY(subject_ids)";
    assert_eq!(count(&pool, consent, start).await, 1, "one consent text");
    let answers = "SELECT count(*) FROM intake.intake_answer WHERE inbox_id = $1";
    assert_eq!(count(&pool, answers, start).await, 0, "START is no answer");
}

/// TD-CON-33 (CON-030, D-3479): a provider-word STOP while the homeowner line is suspended, email
/// on file: one confirmation email, and no text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_33_a_provider_stop_on_a_suspended_line_is_confirmed_by_email(pool: PgPool) {
    let contact = her(&pool, true).await;
    let mut db = pool.acquire().await.expect("conn");
    let owner = (Uuid::from_u128(0x357), at());
    record_suspension(&mut db, Audience::Homeowner, at(), owner)
        .await
        .expect("suspended");
    drop(db);
    text(&pool, 12, "STOP", Some("STOP")).await;
    assert_eq!(count_kind(&pool, OPTOUT_FOLLOWUP_KIND).await, 1);
    run_followups(&pool).await;
    let confirmation = emails(&pool, OPTOUT_CONFIRMATION_EMAIL, contact).await;
    assert_eq!(confirmation, 1, "one confirmation email");
    let texts = "SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1";
    assert_eq!(count(&pool, texts, contact).await, 0, "no text");
}

/// TD-CON-33 (CON-030): a phrase opt-out while the homeowner line is suspended, email on file:
/// one follow-up task, then one confirmation email, and no text at all.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_33_an_opt_out_on_a_suspended_line_is_confirmed_by_email(pool: PgPool) {
    let contact = her(&pool, true).await;
    let mut db = pool.acquire().await.expect("conn");
    let owner = (Uuid::from_u128(0x357), at());
    record_suspension(&mut db, Audience::Homeowner, at(), owner)
        .await
        .expect("suspended");
    drop(db);
    text(&pool, 11, "please stop texting me", None).await;
    assert_eq!(count_kind(&pool, OPTOUT_FOLLOWUP_KIND).await, 1);
    run_followups(&pool).await;
    run_followups(&pool).await;
    let confirmation = emails(&pool, OPTOUT_CONFIRMATION_EMAIL, contact).await;
    assert_eq!(confirmation, 1, "one confirmation email");
    let texts = "SELECT count(*) FROM messaging.outbox WHERE recipient_id = $1";
    assert_eq!(count(&pool, texts, contact).await, 0, "no text");
}

/// TD-CON-33 (CON-029): a sole CANCEL while a pick waits for Confirm pauses the job and queues one
/// `intake.opted_out_live_job`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_33_cancel_with_a_waiting_pick_pauses_the_job(pool: PgPool) {
    let contact = her(&pool, false).await;
    text(&pool, 21, LEAK, None).await;
    let job = quoted_job(&pool, contact).await;
    let mut waiting = None;
    for minute in [10, 11] {
        waiting = pick(&pool, contact, minute).await.or(waiting);
    }
    let waiting = waiting.expect("a pick waits for Confirm");
    let status = "SELECT count(*) FROM picks.pick WHERE id = $1 AND status = 'awaiting_confirm'";
    assert_eq!(
        count(&pool, status, waiting).await,
        1,
        "waiting before CANCEL"
    );
    let later = at() + jiff::SignedDuration::from_mins(12);
    text_at(&pool, (22, "CANCEL", Some("STOP")), later).await;
    let pauses = "SELECT count(*) FROM intake.homeowner_pause WHERE job_id = $1";
    assert_eq!(count(&pool, pauses, job).await, 1, "paused");
    assert_eq!(count_kind(&pool, OPTED_OUT_LIVE_KIND).await, 1);
}

/// TD-CON-33 (CON-028, D-3480): a repeat STOP from a listed number with a live job queues no
/// second `intake.opted_out_live_job`, so no second "still open" email.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_33_a_repeat_stop_queues_no_second_still_open_notice(pool: PgPool) {
    let contact = her(&pool, true).await;
    text(&pool, 31, LEAK, None).await;
    quoted_job(&pool, contact).await;
    text(&pool, 32, "STOP", Some("STOP")).await;
    assert_eq!(
        count_kind(&pool, OPTED_OUT_LIVE_KIND).await,
        1,
        "first STOP"
    );
    text(&pool, 33, "STOP", Some("STOP")).await;
    assert_eq!(
        count_kind(&pool, OPTED_OUT_LIVE_KIND).await,
        1,
        "repeat adds none"
    );
}

/// TD-CON-33 (CON-029, D-3480): a sole CANCEL after an earlier STOP still pauses her job and
/// withdraws the pick waiting for Confirm, and queues no second `intake.opted_out_live_job`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_33_cancel_after_stop_still_pauses_and_withdraws_the_pick(pool: PgPool) {
    let contact = her(&pool, true).await;
    text(&pool, 41, LEAK, None).await;
    let job = quoted_job(&pool, contact).await;
    let mut waiting = None;
    for minute in [10, 11] {
        waiting = pick(&pool, contact, minute).await.or(waiting);
    }
    let waiting = waiting.expect("a pick waits for Confirm");
    text_at(
        &pool,
        (42, "STOP", Some("STOP")),
        at() + jiff::SignedDuration::from_mins(12),
    )
    .await;
    let status = "SELECT count(*) FROM picks.pick WHERE id = $1 AND status = 'awaiting_confirm'";
    assert_eq!(
        count(&pool, status, waiting).await,
        1,
        "STOP pauses nothing"
    );
    text_at(
        &pool,
        (43, "CANCEL", Some("STOP")),
        at() + jiff::SignedDuration::from_mins(13),
    )
    .await;
    let pauses = "SELECT count(*) FROM intake.homeowner_pause WHERE job_id = $1";
    assert_eq!(count(&pool, pauses, job).await, 1, "CANCEL pauses");
    assert_eq!(
        count(&pool, status, waiting).await,
        0,
        "the pick is withdrawn"
    );
    assert_eq!(
        count_kind(&pool, OPTED_OUT_LIVE_KIND).await,
        1,
        "no second notice"
    );
}

/// TD-CON-33 (CON-029, CON-035, D-3480): after she turned texts off in the portal (no inbox row
/// lists her), a sole CANCEL still pauses her job and withdraws the pick waiting for Confirm.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_33_cancel_after_a_portal_opt_out_still_pauses(pool: PgPool) {
    let contact = her(&pool, false).await;
    text(&pool, 51, LEAK, None).await;
    let job = quoted_job(&pool, contact).await;
    let mut waiting = None;
    for minute in [10, 11] {
        waiting = pick(&pool, contact, minute).await.or(waiting);
    }
    let waiting = waiting.expect("a pick waits for Confirm");
    let mut db = pool.acquire().await.expect("conn");
    let how = (RevokeChannel::Portal, "portal:switch");
    let off = at() + jiff::SignedDuration::from_mins(12);
    assert!(
        suppress(&mut db, &vault(), HOME, how, off)
            .await
            .expect("off")
    );
    drop(db);
    text_at(
        &pool,
        (52, "CANCEL", Some("STOP")),
        at() + jiff::SignedDuration::from_mins(13),
    )
    .await;
    let pauses = "SELECT count(*) FROM intake.homeowner_pause WHERE job_id = $1";
    assert_eq!(count(&pool, pauses, job).await, 1, "CANCEL pauses");
    let status = "SELECT count(*) FROM picks.pick WHERE id = $1 AND status = 'awaiting_confirm'";
    assert_eq!(
        count(&pool, status, waiting).await,
        0,
        "the pick is withdrawn"
    );
}

/// One plumbing job on her open request in Quotes ready, quoted by one business; the job.
async fn quoted_job(pool: &PgPool, contact: Uuid) -> Uuid {
    let mut db = pool.acquire().await.expect("conn");
    let conversation: Uuid = sqlx::query_scalar(
        "SELECT c.id FROM intake.conversation c JOIN intake.homeowner h ON h.id = c.homeowner_id \
         WHERE h.contact_id = $1",
    )
    .bind(contact)
    .fetch_one(&mut *db)
    .await
    .expect("request");
    askhava_catalog::api::load_embedded(&mut db)
        .await
        .expect("catalog");
    let start = JobStart {
        conversation,
        words: LEAK,
        emergency: false,
        trade: "plumbing",
        at: at(),
    };
    let Ok(Opened::Created(job)) = begin_job(&mut db, &start).await else {
        panic!("a new job");
    };
    let types = ["leak repair".to_owned()];
    record_job_facts(&mut db, job, "plumbing_repair", &types)
        .await
        .expect("facts");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(&mut db, job, step, at(), Actor::System).await;
        assert!(matches!(moved, Ok(Guarded::Applied(()))), "move");
    }
    quote(&mut db, job).await;
    job
}

/// One business quotes $190 firm on `job`, confirmed (fake wording).
async fn quote(db: &mut sqlx::PgConnection, job: Uuid) {
    let business = create(&mut *db, false, at()).await.expect("business");
    add_owner(&mut *db, business, Uuid::from_u128(0x357_0001), at())
        .await
        .expect("owner");
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
         confirm_on_site, notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
    )
    .bind(job)
    .bind(business)
    .bind(at().to_sqlx())
    .execute(&mut *db)
    .await
    .expect("quote");
}

/// Her "1 YES" text pick at `minute` past [`at`]; the pick it left waiting for Confirm, if any.
async fn pick(pool: &PgPool, contact: Uuid, minute: i64) -> Option<Uuid> {
    let msg = TextIn {
        inbox_id: Uuid::from_u128(0x357_0100 + u128::from(minute.unsigned_abs())),
        contact,
        inbound: Inbound {
            words: "1 YES",
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at() + jiff::SignedDuration::from_mins(minute),
    };
    let mut db = pool.acquire().await.expect("conn");
    let secret = SecretString::from(ID_SECRET.to_owned());
    answer_text_pick(&mut db, &secret, &msg, (IntakeSettings::default(), &TERMS))
        .await
        .expect("text pick")
        .and_then(|done| done.pick)
}

/// TD-CON-33 (CON-016, CON-017): a listed homeowner who writes again is emailed once with an
/// email on file; with none she is listed by one audit row; an emergency still alerts.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_33_a_listed_homeowner_who_writes_is_emailed(pool: PgPool) {
    let contact = her(&pool, true).await;
    text(&pool, 31, "STOP", Some("STOP")).await;
    text(&pool, 32, "is anyone coming about my sink", None).await;
    run_followups(&pool).await;
    let wrote = emails(&pool, OPTOUT_WROTE_AGAIN_EMAIL, contact).await;
    assert_eq!(wrote, 1, "one wrote-again email");
}

/// TD-CON-33 (CON-016, CON-017): no email on file: one `messaging.optout_wrote_again` audit row
/// on the message; an emergency keyword in it still raises the critical alert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_con_33_a_listed_homeowner_with_no_email_is_listed(pool: PgPool) {
    her(&pool, false).await;
    text(&pool, 41, "STOP", Some("STOP")).await;
    let gas = text(&pool, 42, "I smell gas in the kitchen", None).await;
    run_followups(&pool).await;
    let listed = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM ops.audit_log WHERE action = $1 AND subject_id = $2",
    )
    .bind(WROTE_AGAIN_ACTION)
    .bind(gas)
    .fetch_one(&pool)
    .await
    .expect("audit");
    assert_eq!(listed, 1, "listed once");
    let alert = "SELECT count(*) FROM ops.alert \
                 WHERE kind = 'messaging.opted_out_emergency' AND subject_id = $1";
    assert_eq!(count(&pool, alert, gas).await, 1, "CON-017 alert");
}
