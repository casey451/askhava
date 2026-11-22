//! "Close which?" expires on a worded reply that is not a close answer (JOB-019, JOB-021, D-1203,
//! D-4777, D-5161). D-4777 also lists INT-038 for this audit; no age rule changes here. Fake IDs
//! and fake text only.
//!
//! While "Close which?" waits, a reply that closes nothing and is not a lone number drops it, so a
//! later lone open number closes nothing; a lone number that is not open (a ZIP) leaves it waiting;
//! an opt-out ("cancel" alone) writes nothing and closes nothing.

use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    Closing, JobStart, Opened, TextIn, answer_close, begin_job, receive_text,
};
use askhava_jobs::api::{current, job_number};
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

fn text(contact: u128, words: &str, minute: i64) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(0x8_0000 + contact * 100 + minute.unsigned_abs() as u128),
        contact: Uuid::from_u128(contact),
        inbound: Inbound {
            words,
            opt_out: false,
            help: false,
            emergency: false,
            ai_spanish: None,
        },
        received_at: at(minute),
    }
}

/// A fake request with a plumbing and an HVAC job, asked "Close which?" at minute 10; its jobs
/// with their numbers.
async fn asked(db: &mut Db, contact: u128) -> Vec<(Uuid, i64)> {
    let msg = text(contact, "my kitchen sink is leaking under the cabinet", 0);
    let handled = receive_text(db, &secret(), &msg, Settings::default())
        .await
        .expect("open");
    let id = handled.conversation.expect("request");
    let mut jobs = Vec::new();
    for (minute, trade) in (1..).zip(["plumbing", "hvac"]) {
        let start = JobStart {
            conversation: id,
            words: "my kitchen sink is leaking and the AC is warm",
            emergency: false,
            trade,
            at: at(minute),
        };
        let Opened::Created(job) = begin_job(db, &start).await.expect("job") else {
            panic!("a new job");
        };
        jobs.push((job, job_number(db, job).await.expect("number")));
    }
    let (done, _) = answer_close(db, &secret(), &text(contact, "never mind", 10))
        .await
        .expect("ask");
    assert_eq!(done, Closing::Which(jobs.iter().map(|j| j.1).collect()));
    jobs
}

async fn reply(db: &mut Db, msg: &TextIn<'_>) -> Closing {
    answer_close(db, &secret(), msg).await.expect("reply").0
}

async fn status(db: &mut Db, job: Uuid) -> &'static str {
    current(db, job).await.expect("job").status.code()
}

/// D-5161 (JOB-021): a worded reply that is not a close answer drops "Close which?", so a later
/// open number or ALL closes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn close_which_expires_on_a_worded_reply(mut db: Db) {
    let jobs = asked(&mut db, 0xA001).await;
    let other = reply(&mut db, &text(0xA001, "the sink still drips", 11)).await;
    assert_eq!(other, Closing::Nothing);
    let number = jobs[1].1.to_string();
    for (minute, words) in [(12, number.as_str()), (13, "all")] {
        let done = reply(&mut db, &text(0xA001, words, minute)).await;
        assert_eq!(done, Closing::Nothing, "{words}");
    }
    for (job, _) in &jobs {
        assert_eq!(status(&mut db, *job).await, "intake", "nothing closes");
    }
}

/// D-5161 (JOB-021, TD-JOB-05): a lone number that is not open (a ZIP) leaves the question
/// waiting; an open number then closes that job only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_zip_leaves_close_which_waiting(mut db: Db) {
    let jobs = asked(&mut db, 0xA002).await;
    let zip = reply(&mut db, &text(0xA002, "86403", 11)).await;
    assert_eq!(zip, Closing::Nothing);
    let number = jobs[0].1.to_string();
    let done = reply(&mut db, &text(0xA002, &number, 12)).await;
    assert_eq!(done, Closing::Closed(vec![jobs[0].1]));
    assert_eq!(status(&mut db, jobs[0].0).await, "cancelled");
    assert_eq!(status(&mut db, jobs[1].0).await, "intake");
}

/// D-005, D-5161 (JOB-020): "cancel" alone is an opt-out: it closes nothing and writes nothing,
/// so the question still waits for an open number.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn cancel_alone_still_closes_nothing(mut db: Db) {
    let jobs = asked(&mut db, 0xA003).await;
    let mut stop = text(0xA003, "cancel", 11);
    stop.inbound.opt_out = true;
    let done = answer_close(&mut db, &secret(), &stop)
        .await
        .expect("opt-out");
    assert_eq!(done, (Closing::Nothing, None));
    for (job, _) in &jobs {
        assert_eq!(
            status(&mut db, *job).await,
            "intake",
            "cancel closes nothing"
        );
    }
    let number = jobs[1].1.to_string();
    let done = reply(&mut db, &text(0xA003, &number, 12)).await;
    assert_eq!(done, Closing::Closed(vec![jobs[1].1]));
}
