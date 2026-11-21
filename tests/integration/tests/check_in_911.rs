//! A check-in reply that names a 911 danger gets the 911 screen first (T775, D-5081, D-4293 (5);
//! INT-002, INT-004, JOB-015), through `intake::api::receive_raw_reply` on a real database. With
//! a Connected job's check-in due, a text on the INT-004 list queues one INT-004 text and one
//! `intake.emergency_event` row, is no `Reply::CheckedIn` and leaves the check-in due; a text
//! with no match is answered as before. Every name, phone and word is fictional (555-01xx).

use askhava_intake::api::rules::{Inbound, SAFETY_TEMPLATE, Settings};
use askhava_intake::api::{
    JobStart, Opened, RawReply, Reply, Screened, TextIn, begin_job, receive_raw_reply, receive_text,
};
use askhava_jobs::api::{JobStatus, current, move_to};
use askhava_ops::api::Actor;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

const NOW: &str = "2026-11-12T17:00:00Z";

fn now() -> Timestamp {
    NOW.parse().expect("instant")
}

fn vault() -> Vault {
    let key = |byte: u8| format!("1:{}", STANDARD.encode([byte; 32]));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x5a).into(), &key(0x6a).into(), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn text_in(contact: Uuid, words: &str, received_at: Timestamp) -> TextIn<'_> {
    let inbound = Inbound {
        words,
        opt_out: false,
        help: false,
        emergency: false,
        ai_spanish: None,
    };
    TextIn {
        inbox_id: Uuid::new_v4(),
        contact,
        inbound,
        received_at,
    }
}

async fn connected_job(db: &mut PgConnection, days: i64) -> (Uuid, Uuid) {
    let new = NewContact {
        name: Some(SecretString::from("Rosa Fakename")),
        phone: Some(SecretString::from("+19285550171".to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let connected = now() - SignedDuration::from_hours(days * 24);
    let at = connected - SignedDuration::from_hours(2);
    let words = "my kitchen sink is leaking under the cabinet";
    let first = text_in(her, words, at);
    let handled = receive_text(db, &secret(), &first, Settings::default()).await;
    let start = JobStart {
        conversation: handled.expect("open").conversation.expect("request"),
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
    (her, job)
}

async fn reply(db: &mut PgConnection, her: Uuid, words: &str, minutes: i64) -> Reply {
    let msg = text_in(her, words, now() + SignedDuration::from_mins(minutes));
    let raw = SecretString::from(words.to_owned());
    let vault = vault();
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    receive_raw_reply(db, &secret(), (&msg, keys), (Settings::default(), &[]))
        .await
        .expect("reply")
}

/// The `intake.emergency_event` rows and the kept PROBLEM words rows.
async fn rows(db: &mut PgConnection) -> (i64, i64) {
    let sql = "SELECT (SELECT count(*) FROM intake.emergency_event), \
               (SELECT count(*) FROM privacy.message_body)";
    sqlx::query_as(sql).fetch_one(db).await.expect("rows")
}

/// Asserts `reply` is the 911 path: one INT-004 text, one event row, nothing recorded on `job`.
async fn assert_911(db: &mut PgConnection, reply: Reply, job: Uuid, words: &str) {
    let Reply::Handled(handled) = reply else {
        panic!("{words}: the 911 screen, never a check-in answer: {reply:?}");
    };
    assert!(matches!(handled.screened, Screened::Matched(_)), "{words}");
    let sent = handled
        .texts
        .iter()
        .filter(|q| q.template == SAFETY_TEMPLATE);
    assert_eq!(sent.count(), 1, "{words}: one INT-004 text");
    assert_eq!(rows(db).await, (1, 0), "{words}: one event, no words kept");
    let job = current(db, job).await.expect("job");
    assert_eq!(job.status.code(), "connected", "{words}");
}

/// INT-002, INT-004, D-5081: with the day-3 check-in due, `words` takes the 911 path, and the
/// check-in is still due after it: a later PROBLEM with no match is the check-in answer.
async fn danger_goes_first(pool: &PgPool, words: &str) {
    let mut db = pool.acquire().await.expect("conn");
    let (her, job) = connected_job(&mut db, 4).await;
    let first = reply(&mut db, her, words, 0).await;
    assert_911(&mut db, first, job, words).await;
    let later = reply(&mut db, her, "PROBLEM leak is back", 5).await;
    assert_eq!(
        later,
        Reply::CheckedIn(job),
        "{words}: the check-in stays due"
    );
}
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_problem_with_a_gas_smell_gets_the_911_text(pool: PgPool) {
    danger_goes_first(&pool, "PROBLEM gas smell again").await;
}
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_problema_with_a_gas_smell_in_spanish_gets_the_911_text(pool: PgPool) {
    danger_goes_first(&pool, "PROBLEMA huele a gas otra vez").await;
}
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_no_with_a_sparking_outlet_gets_the_911_text(pool: PgPool) {
    danger_goes_first(&pool, "NO, the outlet is sparking").await;
}
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_problem_with_a_power_line_down_gets_the_911_text(pool: PgPool) {
    danger_goes_first(&pool, "PROBLEM power line down in the yard").await;
}

/// JOB-015, INT-002: with no 911-list match the due check-in is answered as before: a PROBLEM
/// closes the job and keeps its words, a NO is the check-in answer, a YES goes on to intake.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_text_with_no_match_is_answered_as_before(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (her, job) = connected_job(&mut db, 4).await;
    let yes = reply(&mut db, her, "YES", 0).await;
    assert!(matches!(&yes, Reply::Handled(h) if h.screened == Screened::Clear));
    let problem = reply(&mut db, her, "PROBLEM leak is back", 1).await;
    assert_eq!(problem, Reply::CheckedIn(job));
    assert_eq!(rows(&mut db).await, (0, 1));
    let closed = current(&mut db, job).await.expect("job").status;
    assert_eq!(closed.code(), "closed");
}
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_no_with_no_match_is_the_check_in_answer(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (her, job) = connected_job(&mut db, 4).await;
    assert_eq!(reply(&mut db, her, "NO", 0).await, Reply::CheckedIn(job));
    assert_eq!(rows(&mut db).await, (0, 0));
}

/// INT-004: a 911 text with no check-in due is unchanged.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_911_text_with_no_check_in_due_is_unchanged(pool: PgPool) {
    let mut db = pool.acquire().await.expect("conn");
    let (her, job) = connected_job(&mut db, 1).await;
    let first = reply(&mut db, her, "PROBLEM gas smell again", 0).await;
    assert_911(&mut db, first, job, "no check-in due").await;
}
