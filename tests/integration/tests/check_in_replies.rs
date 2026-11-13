//! TD-JOB-12 (T277, D-1840; JOB-015, JOB-016, TXT-029, TXT-020, PRIV-006, FEE-035): check-in
//! replies through Twilio's real inbound route (`app/server/src/inbound_text.rs`, compiled in with
//! `#[path]` as in `inbound_text_failures.rs`) on a real database. A day-3 PROBLEM closes the
//! Connected job and its scrubbed words are stored in `privacy.message_body`, bound to the job and
//! the inbox row the alert names, before the route answers 200; a stated name or phone is
//! `[removed]`; a database failure before the store answers 500 and keeps nothing, so Twilio's
//! resend is handled; a safety word gives the safety alert kind; an email-contact day-3 NO queues
//! one `picks.unreachable_step` (`day3_no`) due now and records no no-show itself; a reply with no
//! check-in due goes to intake as before; the daily step deletes words over 30 days old.
//! Every name, phone and word is fictional (555-01xx numbers).

use std::sync::Arc;

use askhava_core::api::{Claimed, Environment, ManualClock};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{JobStart, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::{JobStatus, PROBLEM_ALERT, PROBLEM_SAFETY_ALERT, move_to};
use askhava_ops::api::Actor;
use askhava_picks::api::{ContactChoice, record_contact_choice};
use askhava_privacy::api::{
    CUT_WORDS_KIND, NewContact, REMOVED, Vault, run_words_cut_at, run_words_purge, store_contact,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use inbound_text::{INBOUND_PATH, Webhooks, routes};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sha1::Sha1;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

/// The route under test, compiled into this test crate.
#[path = r"../../../app/server/src/inbound_text.rs"]
mod inbound_text;

const BASE: &str = "https://hooks.example.test";
const TOKEN: &str = "fake_twilio_auth_token";
const HER: &str = "+19285550171";
const LAFTLY: &str = "+19285550100";
/// The route's clock: November 12, 2026, 10:00 Arizona time.
const NOW: &str = "2026-11-12T17:00:00Z";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn days_ago(d: i64) -> Timestamp {
    ts(NOW) - SignedDuration::from_hours(d * 24)
}

fn vault() -> Vault {
    let key = |byte: u8| format!("1:{}", STANDARD.encode([byte; 32]));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x5a).into(), &key(0x6a).into(), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn hooks(pool: &PgPool) -> Arc<Webhooks> {
    Arc::new(Webhooks {
        pool: pool.clone(),
        clock: Arc::new(ManualClock::new(ts(NOW))),
        public_base: BASE.to_owned(),
        auth_token: SecretString::from(TOKEN),
        id_secret: secret(),
        vault: vault(),
        environment: Environment::Test,
        intake: IntakeSettings::default(),
    })
}

/// POSTs inbound text `n` (MessageSid `SM` + `n`) from her, signed by the fake token.
async fn text(h: &Arc<Webhooks>, n: u32, words: &str) -> StatusCode {
    let sid = format!("SM{n:032}");
    let mut fields = [
        ("MessageSid", sid.as_str()),
        ("From", HER),
        ("To", LAFTLY),
        ("Body", words),
    ];
    let encode = |v: &str| -> String {
        v.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect()
    };
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
    let answer = routes(Arc::clone(h))
        .oneshot(request)
        .await
        .expect("answer");
    answer.status()
}

/// Her fake contact texts in a job; the job walks to Connected at `connected`; returns it.
async fn connected_job(db: &mut PgConnection, connected: Timestamp) -> Uuid {
    let new = NewContact {
        name: Some(SecretString::from("Rosa Fakename")),
        phone: Some(SecretString::from(HER.to_owned())),
        email: Some(SecretString::from("rosa@example.test")),
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let words = "my kitchen sink is leaking under the cabinet";
    let inbound = Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    let at = connected - SignedDuration::from_hours(2);
    let msg = TextIn {
        inbox_id: Uuid::new_v4(),
        contact: her,
        inbound,
        received_at: at,
    };
    let handled = receive_text(db, &secret(), &msg, IntakeSettings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words,
        emergency: false,
        trade: "plumbing",
        at,
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    let path = [
        JobStatus::Intake,
        JobStatus::Open,
        JobStatus::QuotesReady,
        JobStatus::AwaitingConfirm,
        JobStatus::PaymentPending,
        JobStatus::Connected,
    ];
    for pair in path.windows(2) {
        let moved = move_to(db, job, (pair[0], pair[1]), connected, Actor::System).await;
        assert!(moved.expect("move").applied().is_some(), "{pair:?}");
    }
    job
}

/// A paid pick of a fake business on `job`, with her contact choice `choice`.
async fn picked(db: &mut PgConnection, job: Uuid, choice: ContactChoice) -> Uuid {
    let at = days_ago(5);
    let business = askhava_businesses::api::create(db, false, at)
        .await
        .expect("business");
    let pick: Uuid = sqlx::query_scalar("INSERT INTO picks.pick (job_id, business_id, quote_number, channel, status, picked_at) VALUES ($1, $2, 1, 'page', 'paid', $3) RETURNING id")
        .bind(job).bind(business).bind(at.to_sqlx()).fetch_one(&mut *db).await.expect("pick");
    record_contact_choice(db, pick, choice, at)
        .await
        .expect("choice");
    pick
}

async fn status(pool: &PgPool, job: Uuid) -> &'static str {
    let mut db = pool.acquire().await.expect("conn");
    askhava_jobs::api::current(&mut db, job)
        .await
        .expect("job")
        .status
        .code()
}

/// The kept words rows of `job`: `(inbox, words, safety)`.
async fn kept(pool: &PgPool, job: Uuid) -> Vec<(Uuid, String, bool)> {
    sqlx::query_as("SELECT inbox_id, words, safety FROM privacy.message_body WHERE job_id = $1")
        .bind(job)
        .fetch_all(pool)
        .await
        .expect("kept")
}

/// The `(kind, subject)` of every open `jobs.problem_report*` alert.
async fn problem_alerts(pool: &PgPool) -> Vec<(String, Uuid)> {
    sqlx::query_as("SELECT kind, subject_id FROM ops.alert WHERE kind = ANY($1) ORDER BY raised_at")
        .bind(vec![PROBLEM_ALERT, PROBLEM_SAFETY_ALERT])
        .fetch_all(pool)
        .await
        .expect("alerts")
}

/// The `messaging.inbox` row of inbound text `n`.
async fn inbox_row(pool: &PgPool, n: u32) -> Option<Uuid> {
    sqlx::query_scalar("SELECT id FROM messaging.inbox WHERE message_sid = $1")
        .bind(format!("SM{n:032}"))
        .fetch_optional(pool)
        .await
        .expect("inbox")
}

/// TD-JOB-12 (JOB-015, JOB-016, TXT-029, PRIV-006): a day-3 PROBLEM closes the job, its words
/// are kept bound to the job and the alert's inbox row before the 200, a stated name and phone are
/// `[removed]`, and a new pool (a restart) still finds them by the alert's inbox row.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_12_a_problem_keeps_its_scrubbed_words(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let job = connected_job(&mut db, days_ago(4)).await;
    drop(db);
    let words = "PROBLEM my name is Rosa Fakename, call 928 555 0167 the faucet still drips";
    assert_eq!(text(&hooks(&pool), 1, words).await, StatusCode::OK);
    assert_eq!(status(&pool, job).await, "closed");
    let inbox = inbox_row(&pool, 1).await.expect("inbox row");
    assert_eq!(
        problem_alerts(&pool).await,
        [(PROBLEM_ALERT.to_owned(), inbox)]
    );
    let restarted = PgPool::connect_with((*pool.connect_options()).clone())
        .await
        .expect("a new pool");
    let rows = kept(&restarted, job).await;
    assert_eq!(rows.len(), 1);
    let (row_inbox, kept_words, safety) = &rows[0];
    assert_eq!((*row_inbox, *safety), (inbox, false));
    assert!(kept_words.contains("faucet still drips"), "{kept_words}");
    assert!(kept_words.contains(REMOVED), "{kept_words}");
    for gone in ["Rosa", "Fakename", "555", "0167"] {
        assert!(!kept_words.contains(gone), "{gone} kept");
    }
    assert_eq!(text(&hooks(&pool), 1, words).await, StatusCode::OK);
    assert_eq!(kept(&pool, job).await.len(), 1, "a resend keeps one row");
}

/// TD-JOB-12 (JOB-016, OPS-005): a PROBLEM holding a safety word raises the safety alert kind and
/// keeps the flag with the words.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_12_a_safety_word_gives_the_safety_alert(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let job = connected_job(&mut db, days_ago(4)).await;
    drop(db);
    let words = "problem the worker made a threat at my door";
    assert_eq!(text(&hooks(&pool), 2, words).await, StatusCode::OK);
    let inbox = inbox_row(&pool, 2).await.expect("inbox row");
    assert_eq!(
        problem_alerts(&pool).await,
        [(PROBLEM_SAFETY_ALERT.to_owned(), inbox)]
    );
    assert!(kept(&pool, job).await[0].2, "the safety flag is kept");
}

/// TD-JOB-12 (JOB-016, TXT-012): a database failure before the store answers 500 and keeps
/// nothing (no row, no close, no alert, no inbox row), so Twilio's resend is handled in full.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_12_a_failed_store_rolls_the_delivery_back(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let job = connected_job(&mut db, days_ago(4)).await;
    drop(db);
    let away = "ALTER TABLE privacy.message_body RENAME TO message_body_away";
    sqlx::query(away).execute(&pool).await.expect("rename");
    let words = "PROBLEM the drain leaks again";
    let failed = text(&hooks(&pool), 3, words).await;
    assert_eq!(failed, StatusCode::INTERNAL_SERVER_ERROR);
    let back = "ALTER TABLE privacy.message_body_away RENAME TO message_body";
    sqlx::query(back).execute(&pool).await.expect("rename back");
    assert_eq!(status(&pool, job).await, "connected");
    assert!(problem_alerts(&pool).await.is_empty());
    assert_eq!(inbox_row(&pool, 3).await, None, "the delivery rolled back");
    assert_eq!(text(&hooks(&pool), 3, words).await, StatusCode::OK);
    assert_eq!(status(&pool, job).await, "closed");
    assert_eq!(kept(&pool, job).await.len(), 1);
}

/// The `picks.unreachable_step` tasks of `pick`: `(step, due)`.
async fn steps(pool: &PgPool, pick: Uuid) -> Vec<(String, i64)> {
    sqlx::query_as("SELECT payload->>'step', (extract(epoch FROM run_at))::bigint FROM core.task WHERE kind = 'picks.unreachable_step' AND payload->>'pick' = $1::text")
        .bind(pick)
        .fetch_all(pool)
        .await
        .expect("steps")
}

/// TD-JOB-12 (JOB-015, FEE-035, D-3488): an email-contact day-3 NO queues one `day3_no` step for
/// the pick, due now; a repeat queues nothing more; `intake` records no no-show and moves nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_12_an_email_day_3_no_queues_the_no_show_step(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let job = connected_job(&mut db, days_ago(4)).await;
    let pick = picked(&mut db, job, ContactChoice::Email).await;
    drop(db);
    assert_eq!(text(&hooks(&pool), 4, "NO").await, StatusCode::OK);
    assert_eq!(text(&hooks(&pool), 5, "no.").await, StatusCode::OK);
    let due = ts(NOW).as_second();
    assert_eq!(steps(&pool, pick).await, [("day3_no".to_owned(), due)]);
    assert_eq!(status(&pool, job).await, "connected");
    let pick_status: String = sqlx::query_scalar("SELECT status FROM picks.pick WHERE id = $1")
        .bind(pick)
        .fetch_one(&pool)
        .await
        .expect("pick");
    assert_eq!(pick_status, "paid", "no no-show recorded here");
    assert!(kept(&pool, job).await.is_empty());
}

/// TD-JOB-12 (JOB-015): a call-contact day-3 NO closes the job through `answer_check_in` and
/// queues no step.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_12_a_call_contact_no_closes_the_job(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let job = connected_job(&mut db, days_ago(4)).await;
    let pick = picked(&mut db, job, ContactChoice::Call).await;
    drop(db);
    assert_eq!(text(&hooks(&pool), 6, "NO").await, StatusCode::OK);
    assert_eq!(status(&pool, job).await, "closed");
    assert!(steps(&pool, pick).await.is_empty());
}

/// TD-JOB-12 (JOB-015): 14 days after the connection the reply answers the day-14 check-in; a
/// day-14 NO under email contact closes the job and queues no step.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_12_a_day_14_no_closes_even_under_email(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let job = connected_job(&mut db, days_ago(15)).await;
    let pick = picked(&mut db, job, ContactChoice::Email).await;
    drop(db);
    assert_eq!(text(&hooks(&pool), 9, "NO").await, StatusCode::OK);
    assert_eq!(status(&pool, job).await, "closed");
    assert!(steps(&pool, pick).await.is_empty());
}

/// TD-JOB-12 (JOB-015): a check-in word on a Connected job with no check-in due yet goes to
/// intake as before: nothing moves and nothing is kept.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_12_no_check_in_due_goes_to_intake(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let job = connected_job(&mut db, days_ago(1)).await;
    drop(db);
    assert_eq!(text(&hooks(&pool), 7, "PROBLEM").await, StatusCode::OK);
    assert_eq!(status(&pool, job).await, "connected");
    assert!(kept(&pool, job).await.is_empty());
    assert!(problem_alerts(&pool).await.is_empty());
}

fn claimed(kind: &str, payload: serde_json::Value) -> Claimed {
    Claimed {
        id: 0,
        kind: kind.to_owned(),
        payload,
        attempts: 1,
    }
}

/// TD-JOB-12 (TXT-020, PRIV-008): the daily step deletes words over 30 days old and queues the
/// next day's run; the job's text stage deletes the job's words, any other stage none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_12_old_words_go_after_30_days(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let job = connected_job(&mut db, days_ago(4)).await;
    drop(db);
    assert_eq!(
        text(&hooks(&pool), 8, "PROBLEM a leak").await,
        StatusCode::OK
    );
    let aged = "UPDATE privacy.message_body SET created_at = $1";
    sqlx::query(aged)
        .bind(days_ago(29).to_sqlx())
        .execute(&pool)
        .await
        .expect("age");
    let cut = run_words_cut_at(pool.clone(), claimed(CUT_WORDS_KIND, json!({})), ts(NOW)).await;
    assert_eq!(cut, Ok(()));
    assert_eq!(kept(&pool, job).await.len(), 1, "29 days old stays");
    sqlx::query(aged)
        .bind(days_ago(31).to_sqlx())
        .execute(&pool)
        .await
        .expect("age");
    let cut = run_words_cut_at(pool.clone(), claimed(CUT_WORDS_KIND, json!({})), ts(NOW)).await;
    assert_eq!(cut, Ok(()));
    assert!(kept(&pool, job).await.is_empty(), "31 days old goes");
    let next: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1")
        .bind(CUT_WORDS_KIND)
        .fetch_one(&pool)
        .await
        .expect("next");
    assert_eq!(next, 1, "one run queued for the next day");
    let mut db = pool.acquire().await.expect("conn");
    let job2 = Uuid::new_v4();
    sqlx::query("INSERT INTO privacy.message_body (inbox_id, job_id, words, safety, created_at) VALUES ($1, $2, 'a leak', false, now())")
        .bind(Uuid::new_v4()).bind(job2).execute(&mut *db).await.expect("row");
    drop(db);
    let stage = |s: &str| claimed("privacy.purge", json!({"job": job2, "stage": s}));
    assert_eq!(
        run_words_purge(pool.clone(), stage("contact")).await,
        Ok(())
    );
    assert_eq!(
        kept(&pool, job2).await.len(),
        1,
        "another stage keeps the words"
    );
    assert_eq!(run_words_purge(pool.clone(), stage("text")).await, Ok(()));
    assert!(
        kept(&pool, job2).await.is_empty(),
        "the text stage deletes them"
    );
    let bad = claimed("privacy.purge", json!({}));
    assert!(run_words_purge(pool.clone(), bad).await.is_err());
}

/// TD-JOB-12 (PRIV-006, SEC-046): the kept words have one read, `problem_words`, and it takes
/// Casey's owner session proof; no read takes a business or homeowner session, and no crate but
/// `privacy` names the table.
#[test]
fn td_job_12_only_the_owner_session_reads_the_words() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = std::fs::read_to_string(root.join("app/privacy/src/message_body.rs"))
        .expect("message_body.rs");
    let reads: Vec<&str> = source
        .split("pub async fn ")
        .skip(1)
        .filter(|f| f.contains("SELECT"))
        .collect();
    assert_eq!(reads.len(), 1, "one read of the kept words");
    let signature = reads[0].split('{').next().expect("signature");
    assert!(signature.starts_with("problem_words("));
    assert!(signature.contains("&OwnerSessionProof"));
    for other in ["HomeownerSessionProof", "BusinessSessionProof"] {
        assert!(!source.contains(other), "{other} named");
    }
}
