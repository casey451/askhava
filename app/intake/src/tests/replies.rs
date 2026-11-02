//! The reply dispatcher inside an open request (TD-TXT-11, TD-INT-08: TXT-031, INT-019; D-999,
//! D-1199 (4), T241).
//!
//! On a fresh database with fake IDs, fake text and fake business names only: a first text opens
//! the request through `receive_text`; LIST with no quoted job and any other reply restart each
//! Intake job's 72-hour wait at the reply, then go to `receive_text`; an opt-out or HELP text
//! restarts nothing. LIST with a quoted job is the integration test's.

use askhava_jobs::api::current;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;

use crate::api::{JobStart, Opened, Reply, TextIn, begin_job, receive_reply};
use crate::rules::{CONSENT_TEMPLATE, Inbound, Settings};

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn az(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

fn text(contact: u128, words: &str, at: Timestamp) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(0x6_0000 + contact + u128::from(at.as_second().unsigned_abs())),
        contact: Uuid::from_u128(contact),
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at,
    }
}

async fn reply(db: &mut Db, msg: &TextIn<'_>, names: &[(Uuid, String)]) -> Reply {
    receive_reply(db, &secret(), msg, (Settings::default(), names))
        .await
        .expect("reply")
}

/// A fake request opened through the dispatcher at `at`, with one plumbing job: (request, job).
async fn request(db: &mut Db, contact: u128, at: Timestamp) -> (Uuid, Uuid) {
    let msg = text(contact, "my kitchen sink is leaking under the cabinet", at);
    let Reply::Handled(handled) = reply(db, &msg, &[]).await else {
        panic!("a first text is handled");
    };
    assert_eq!(handled.texts[0].template, CONSENT_TEMPLATE);
    let id = handled.conversation.expect("request");
    let start = JobStart {
        conversation: id,
        words: msg.inbound.words,
        emergency: false,
        trade: "plumbing_repair",
        at,
    };
    let Opened::Created(job) = begin_job(db, &start).await.expect("job") else {
        panic!("a new job");
    };
    (id, job)
}

/// The instant `job`'s no-reply wait counts from: the 24-hour reminder and the 72-hour close are
/// queued from it (`jobs::api::restart_timer`, D-807).
async fn timer_from(db: &mut Db, job: Uuid) -> Timestamp {
    current(db, job).await.expect("job").timer_from
}

/// TD-INT-08 (INT-019, D-807): a reply in an open request restarts the wait at the reply (the
/// new reminder and close count from it), and it still reaches `receive_text`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_08_a_reply_through_the_dispatcher_restarts_the_wait(mut db: Db) {
    let (id, job) = request(&mut db, 0x9001, az("02T10:00:00")).await;
    let msg = text(0x9001, "it drips under the sink", az("02T15:00:00"));
    let Reply::Handled(handled) = reply(&mut db, &msg, &[]).await else {
        panic!("a reply is handled");
    };
    assert_eq!(handled.conversation, Some(id));
    assert_eq!(timer_from(&mut db, job).await, az("02T15:00:00"));
}

/// TD-INT-08 (INT-019, CON-008, CON-018): an opt-out or HELP text, already answered by
/// `messaging`, restarts no wait.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_08_an_opt_out_or_help_text_restarts_nothing(mut db: Db) {
    let (_, job) = request(&mut db, 0x9002, az("02T10:00:00")).await;
    let before = timer_from(&mut db, job).await;
    let mut stop = text(0x9002, "STOP", az("02T15:00:00"));
    stop.inbound.opt_out = true;
    let mut help = text(0x9002, "HELP", az("02T16:00:00"));
    help.inbound.help = true;
    for msg in [stop, help] {
        let Reply::Handled(handled) = reply(&mut db, &msg, &[]).await else {
            panic!("handled");
        };
        assert!(handled.texts.is_empty());
    }
    assert_eq!(timer_from(&mut db, job).await, before);
}

/// TD-TXT-11 (TXT-031, D-1197): LIST or LISTA in an open request with no quoted job queues no
/// list and is heard as a reply (the wait restarts); the quoted-job case is
/// `tests/integration/tests/inbound_text_route.rs`'s, which can build a quote.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_txt_11_list_with_no_quoted_job_is_heard_as_a_reply(mut db: Db) {
    let (id, job) = request(&mut db, 0x9003, az("02T10:00:00")).await;
    for (words, hour) in [("LIST", "12"), ("lista 7", "13")] {
        let msg = text(0x9003, words, az(&format!("02T{hour}:00:00")));
        let Reply::Handled(handled) = reply(&mut db, &msg, &[]).await else {
            panic!("{words} with no quote is a reply");
        };
        assert_eq!(handled.conversation, Some(id));
        let heard = az(&format!("02T{hour}:00:00"));
        assert_eq!(timer_from(&mut db, job).await, heard, "{words}");
    }
}
