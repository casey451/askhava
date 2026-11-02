//! `intake`'s sender of the homeowner job texts on a fresh database (TD-JOB-03: JOB-010 to
//! JOB-012, TXT-042, TXT-040, SEC-037; TD-QH-01: QH-001, QH-006, QH-007; CON-021, CON-028;
//! D-871, D-1301 to D-1303). Fake numbers (555-01xx), IDs and words only.
//!
//! A request is opened with its consent text and a job, the job walks to Quotes ready and
//! `jobs` queues its `jobs.homeowner_text` events the real way; each case runs
//! `job_text_step` at a chosen instant and reads the outbox and the task queue.

use askhava_core::api::{IdSecret, LinkToken, WorkerId, claim, finish};
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    JobStart, JobText, JobTextKeys, JobTextSkip, JobTextStep, Opened, TextIn, begin_job,
    job_text_step, parse_job_text, receive_text,
};
use askhava_jobs::api::quote_token::resolve;
use askhava_jobs::api::rules::Windows;
use askhava_jobs::api::{JobStatus, TEXT_KIND, TIMER_KIND, all_windows_ended, move_to, run_timer};
use askhava_messaging::api::{RevokeChannel, suppress};
use askhava_ops::api::Actor;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const ARIZONA: &str = "+19285550141";
const PACIFIC: &str = "+12135550142";
const PAGE: &str = "https://askhava.com/q/";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

/// 10:00 Arizona; 21:00 Arizona; the next 8 a.m. Arizona.
const DAY: &str = "2026-11-02T17:00:00Z";
const NIGHT: &str = "2026-11-03T04:00:00Z";
const MORNING: &str = "2026-11-03T15:00:00Z";

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x54), &key(0x64), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key-000".to_owned())
}

fn keys() -> JobTextKeys {
    JobTextKeys::new(IdSecret::new(1, secret()).expect("id"), secret(), vault())
}

/// A homeowner with `phone` texts in, gets the consent text and a plumbing job, which walks to
/// Quotes ready; returns her contact and the job.
async fn quotes_ready(db: &mut PgConnection, phone: &str) -> (Uuid, Uuid) {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(phone.to_owned())),
        email: None,
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
    let msg = TextIn {
        inbox_id: Uuid::new_v4(),
        contact: her,
        inbound,
        received_at: ts(DAY),
    };
    let handled = receive_text(db, &secret(), &msg, Settings::default()).await;
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
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, ts(DAY), Actor::System).await;
        assert!(moved.expect("move").applied().is_some());
    }
    (her, job)
}

/// The every-window-ended call queues TXT-042's event; returns it parsed (2 of 3 quoted).
async fn all_quotes_in(db: &mut PgConnection, job: Uuid) -> JobText {
    let windows = Windows {
        quoted: 2,
        sent: 3,
        choosable: 2,
    };
    let ended = all_windows_ended(db, job, windows, ts(DAY)).await;
    assert!(ended.expect("windows").applied().is_some());
    event(db, job, "all_quotes_in").await
}

async fn event(db: &mut PgConnection, job: Uuid, template: &str) -> JobText {
    let payload: Value = sqlx::query_scalar(
        "SELECT payload FROM core.task WHERE kind = $1 AND payload->>'job' = $2 \
         AND payload->>'template' = $3 ORDER BY id LIMIT 1",
    )
    .bind(TEXT_KIND)
    .bind(job.to_string())
    .bind(template)
    .fetch_one(db)
    .await
    .expect("event");
    assert!(!payload.to_string().contains(PAGE), "no link in a payload");
    parse_job_text(&payload).expect("parsed")
}

/// The bodies queued to `her`.
async fn bodies(db: &mut PgConnection, her: Uuid) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT template_id, body FROM messaging.outbox WHERE recipient_id = $1 \
         AND template_id <> 'consent' ORDER BY created_at",
    )
    .bind(her)
    .fetch_all(db)
    .await
    .expect("outbox")
}

async fn step(db: &mut PgConnection, text: &JobText, now: &str) -> JobTextStep {
    job_text_step(db, &keys(), text, ts(now))
        .await
        .expect("step")
}

/// TD-JOB-03, TD-QH-01 (TXT-042, QH-001, QH-006, SEC-037, TXT-040): at 9 p.m. the all-quotes-in
/// text is held and queued again for 8 a.m.; then exactly one text goes, with the counts and a
/// quote-page link that opens her job; a second run queues nothing and keeps that link live.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_03_all_quotes_in_held_to_8_am_then_sent_once_with_her_link(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (her, job) = quotes_ready(&mut db, ARIZONA).await;
    let text = all_quotes_in(&mut db, job).await;
    assert_eq!(
        step(&mut db, &text, NIGHT).await,
        JobTextStep::Held(ts(MORNING))
    );
    let key = format!(
        "{TEXT_KIND}.{job}.all_quotes_in.{}",
        ts(MORNING).as_microsecond()
    );
    let held: i64 = sqlx::query_scalar("SELECT count(*) FROM core.task WHERE dedupe_key = $1")
        .bind(key)
        .fetch_one(&mut *db)
        .await
        .expect("held");
    assert_eq!(held, 1, "the held event is queued for 8 a.m.");
    assert!(bodies(&mut db, her).await.is_empty());
    let JobTextStep::Queued(_) = step(&mut db, &text, MORNING).await else {
        panic!("sent at 8 a.m.");
    };
    let sent = bodies(&mut db, her).await;
    assert_eq!(sent.len(), 1);
    let body = &sent[0].1;
    let first = "Ask Hava: All quotes are in for request 1001: 2 of 3 businesses quoted. ";
    assert!(body.starts_with(first), "{body}");
    let token = body[first.len()..].split(' ').next().expect("link");
    let token = LinkToken::parse(token.strip_prefix(PAGE).expect("page link")).expect("token");
    assert_eq!(resolve(&mut db, &token).await.expect("resolve"), Some(job));
    let again = step(&mut db, &text, MORNING).await;
    assert_eq!(again, JobTextStep::Skipped(JobTextSkip::AlreadyQueued));
    assert_eq!(bodies(&mut db, her).await.len(), 1);
    assert_eq!(resolve(&mut db, &token).await.expect("resolve"), Some(job));
}

/// TD-QH-01 (QH-001, QH-006): a Pacific number at 08:30 Arizona in January (07:30 Pacific) is
/// held to 08:00 Pacific, the area-code zone `messaging` reads from the vault.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_qh_01_a_pacific_number_waits_for_8_am_pacific(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (her, job) = quotes_ready(&mut db, PACIFIC).await;
    let text = all_quotes_in(&mut db, job).await;
    let held = step(&mut db, &text, "2027-01-15T15:30:00Z").await;
    assert_eq!(held, JobTextStep::Held(ts("2027-01-15T16:00:00Z")));
    assert!(bodies(&mut db, her).await.is_empty());
}

/// Runs every task due at `now` as a fresh worker, the job timers among them.
async fn fire(pool: &PgPool, now: Timestamp) {
    let owner = WorkerId::new(pool).await.expect("worker");
    let lease = SignedDuration::from_mins(5);
    for task in claim(pool, owner, now, lease, 100).await.expect("claim") {
        finish(pool, owner, &task, now).await.expect("finish");
        if task.kind == TIMER_KIND {
            run_timer(pool.clone(), task).await.expect("timer");
        }
    }
}

/// TD-JOB-03, QH-007 (JOB-012): once the 3-day close has run, the held all-quotes-in text is
/// dropped, while the close text, held at night, still goes at 8 a.m. with the JOB-012 words.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_03_a_held_text_is_dropped_after_the_close_but_the_close_text_goes(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (her, job) = quotes_ready(&mut db, ARIZONA).await;
    let held = all_quotes_in(&mut db, job).await;
    fire(&pool, ts(DAY) + SignedDuration::from_hours(10 * 24)).await;
    let close = event(&mut db, job, "closed_no_pick").await;
    let night = "2026-11-13T04:00:00Z";
    let moved = JobTextStep::Skipped(JobTextSkip::MovedOn);
    assert_eq!(step(&mut db, &held, night).await, moved);
    let morning = ts("2026-11-13T15:00:00Z");
    assert_eq!(
        step(&mut db, &close, night).await,
        JobTextStep::Held(morning)
    );
    let JobTextStep::Queued(_) = step(&mut db, &close, "2026-11-13T15:00:00Z").await else {
        panic!("the close text goes");
    };
    let expected = "Ask Hava: Request 1001 is closed because no quote was picked. Text us any \
                    time to start a new one.";
    let sent = bodies(&mut db, her).await;
    assert_eq!(sent, [("closed_no_pick".to_owned(), expected.to_owned())]);
}

/// CON-021, CON-028: a homeowner with no consent record, or whose number opted out, gets no
/// job text; the opted-out job keeps going by email and the quote page.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn con_021_and_con_028_no_text_without_consent_or_after_an_opt_out(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let (her, job) = quotes_ready(&mut db, ARIZONA).await;
    let text = all_quotes_in(&mut db, job).await;
    let channel = (RevokeChannel::Portal, "portal");
    suppress(&mut db, &vault(), ARIZONA, channel, ts(DAY))
        .await
        .expect("suppress");
    let opted = JobTextStep::Skipped(JobTextSkip::OptedOut);
    assert_eq!(step(&mut db, &text, DAY).await, opted);
    sqlx::query("DELETE FROM intake.consent_record")
        .execute(&mut *db)
        .await
        .expect("fixture: no consent record");
    let none = JobTextStep::Skipped(JobTextSkip::NoConsent);
    assert_eq!(step(&mut db, &text, DAY).await, none);
    assert!(bodies(&mut db, her).await.is_empty());
}
