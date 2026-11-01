//! Closing at the homeowner's word in the database (TD-JOB-05: JOB-018 to JOB-023). Fake IDs and
//! fake text only.
//!
//! "CLOSE <number>", a cancel-intent message and the quote page's close button move a job to
//! Cancelled; with two open jobs a cancel-intent message gets "Close which?" and closes nothing
//! until answered; "I had to cancel the plumber" gets the JOB-023 line; an opt-out changes no job.

use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    CLOSE_MENTION_TEMPLATE, CLOSE_WHICH_TEMPLATE, Closing, JOB_CLOSED_TEMPLATE,
    JOBS_CLOSED_TEMPLATE, JobStart, Opened, TextIn, answer_close, begin_job, close_from_page,
    conversation, receive_text,
};
use askhava_jobs::api::{JobStatus, current, job_number, move_to};
use askhava_ops::api::Actor;
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
        inbox_id: Uuid::from_u128(0x7_0000 + contact * 100 + minute.unsigned_abs() as u128),
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

/// A fake request with one job per trade in `trades`; (request, jobs with their numbers).
async fn request(db: &mut Db, contact: u128, trades: &[&'static str]) -> (Uuid, Vec<(Uuid, i64)>) {
    let msg = text(contact, "my kitchen sink is leaking under the cabinet", 0);
    let handled = receive_text(db, &secret(), &msg, Settings::default())
        .await
        .expect("open");
    let id = handled.conversation.expect("request");
    let mut jobs = Vec::new();
    for (minute, trade) in (1..).zip(trades) {
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
    (id, jobs)
}

async fn status(db: &mut Db, job: Uuid) -> &'static str {
    current(db, job).await.expect("job").status.code()
}

async fn body(db: &mut Db, outbox: Uuid) -> String {
    sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE id = $1")
        .bind(outbox)
        .fetch_one(&mut **db)
        .await
        .expect("body")
}

/// TD-JOB-05 (JOB-019, JOB-022): "CLOSE <number>" and a cancel-intent message close the one open
/// job and say so; the request closes with it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_close_number_and_cancel_intent_close(mut db: Db) {
    for (contact, words) in [
        (0x9001, "CLOSE"),
        (0x9002, "please cancel my request"),
        (0x9003, "never mind"),
        (0x9004, "ya no lo necesito"),
    ] {
        let (id, jobs) = request(&mut db, contact, &["plumbing"]).await;
        let (job, number) = jobs[0];
        let (done, sent) = answer_close(&mut db, &secret(), &text(contact, words, 10))
            .await
            .expect("close");
        assert_eq!(done, Closing::Closed(vec![number]), "{words}");
        assert_eq!(status(&mut db, job).await, "cancelled", "{words}");
        let sent = sent.expect("says so");
        assert_eq!(sent.template, JOB_CLOSED_TEMPLATE);
        assert!(
            body(&mut db, sent.outbox_id)
                .await
                .contains(&number.to_string())
        );
        let row = conversation(&mut db, id).await.expect("read").expect("row");
        assert!(row.closed_at.is_some(), "{words}");
    }
    let (_, jobs) = request(&mut db, 0x9005, &["plumbing"]).await;
    let words = format!("CLOSE {}", jobs[0].1);
    let (done, _) = answer_close(&mut db, &secret(), &text(0x9005, &words, 10))
        .await
        .expect("close");
    assert_eq!(done, Closing::Closed(vec![jobs[0].1]));
    assert_eq!(status(&mut db, jobs[0].0).await, "cancelled");
}

/// TD-JOB-05 (JOB-020, JOB-023, D-005): "cancel" as an opt-out changes no job and sends nothing;
/// "I had to cancel the plumber" gets the JOB-023 line once and closes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_cancel_alone_or_mentioned_closes_nothing(mut db: Db) {
    let (_, jobs) = request(&mut db, 0x9101, &["plumbing"]).await;
    let (job, number) = jobs[0];
    let mut stop = text(0x9101, "cancel", 10);
    stop.inbound.opt_out = true;
    let done = answer_close(&mut db, &secret(), &stop).await.expect("stop");
    assert_eq!(done, (Closing::Nothing, None));
    assert_eq!(status(&mut db, job).await, "intake");
    let mention = text(0x9101, "I had to cancel the plumber", 11);
    let (done, sent) = answer_close(&mut db, &secret(), &mention)
        .await
        .expect("mention");
    assert_eq!(done, Closing::Explained(number));
    let sent = sent.expect("the how-to line");
    assert_eq!(sent.template, CLOSE_MENTION_TEMPLATE);
    assert_eq!(
        body(&mut db, sent.outbox_id).await,
        format!("Ask Hava: To close request {number}, reply CLOSE {number}.")
    );
    assert_eq!(status(&mut db, job).await, "intake");
}

/// TD-JOB-05 (JOB-021): with two open jobs a cancel-intent message gets "Close which?" with both
/// numbers and ALL, and closes nothing until the answer; the number answer closes that job only,
/// and ALL closes the rest.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_two_open_jobs_ask_close_which(mut db: Db) {
    let (id, jobs) = request(&mut db, 0x9201, &["plumbing", "hvac"]).await;
    let numbers: Vec<i64> = jobs.iter().map(|j| j.1).collect();
    let ask = text(0x9201, "please cancel my request", 10);
    let (done, sent) = answer_close(&mut db, &secret(), &ask).await.expect("ask");
    assert_eq!(done, Closing::Which(numbers.clone()));
    let sent = sent.expect("Close which?");
    assert_eq!(sent.template, CLOSE_WHICH_TEMPLATE);
    let which = body(&mut db, sent.outbox_id).await;
    assert!(which.starts_with("Ask Hava: Close which?") && which.contains("CLOSE ALL"));
    assert!(numbers.iter().all(|n| which.contains(&n.to_string())));
    for (job, _) in &jobs {
        assert_eq!(status(&mut db, *job).await, "intake", "nothing closes yet");
    }
    let first = numbers[0].to_string();
    let (done, _) = answer_close(&mut db, &secret(), &text(0x9201, &first, 11))
        .await
        .expect("answer");
    assert_eq!(done, Closing::Closed(vec![numbers[0]]));
    assert_eq!(status(&mut db, jobs[0].0).await, "cancelled");
    assert_eq!(status(&mut db, jobs[1].0).await, "intake");
    let row = conversation(&mut db, id).await.expect("read").expect("row");
    assert!(row.closed_at.is_none(), "one job is still open");
    let (done, sent) = answer_close(&mut db, &secret(), &text(0x9201, "CLOSE ALL", 12))
        .await
        .expect("all");
    assert_eq!(done, Closing::Closed(vec![numbers[1]]));
    assert_eq!(sent.expect("text").template, JOB_CLOSED_TEMPLATE);
    assert_eq!(status(&mut db, jobs[1].0).await, "cancelled");
}

/// TD-JOB-05 (JOB-021): "all" in answer to "Close which?" closes every open job with one text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_close_which_all(mut db: Db) {
    let (_, jobs) = request(&mut db, 0x9301, &["plumbing", "hvac"]).await;
    let ask = text(0x9301, "never mind", 10);
    answer_close(&mut db, &secret(), &ask).await.expect("ask");
    let (done, sent) = answer_close(&mut db, &secret(), &text(0x9301, "all", 11))
        .await
        .expect("all");
    assert_eq!(done, Closing::Closed(jobs.iter().map(|j| j.1).collect()));
    let sent = sent.expect("text");
    assert_eq!(sent.template, JOBS_CLOSED_TEMPLATE);
    let both = format!("Requests {} and {} are closed", jobs[0].1, jobs[1].1);
    assert!(body(&mut db, sent.outbox_id).await.contains(&both));
    for (job, _) in &jobs {
        assert_eq!(status(&mut db, *job).await, "cancelled");
    }
}

/// TD-JOB-05 (JOB-018, JOB-019): the quote page's close button cancels a job from any status
/// before Done, once; a pressed-again or final job is left alone.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_the_page_button_closes(mut db: Db) {
    let (id, jobs) = request(&mut db, 0x9401, &["plumbing"]).await;
    let job = jobs[0].0;
    let moved = move_to(
        &mut db,
        job,
        (JobStatus::Intake, JobStatus::Open),
        at(5),
        Actor::System,
    )
    .await
    .expect("open");
    assert!(moved.applied().is_some());
    assert!(close_from_page(&mut db, job, at(10)).await.expect("close"));
    assert_eq!(status(&mut db, job).await, "cancelled");
    assert!(!close_from_page(&mut db, job, at(11)).await.expect("again"));
    let row = conversation(&mut db, id).await.expect("read").expect("row");
    assert!(row.closed_at.is_some());
}

/// TD-JOB-05 (JOB-021, D-1203): a "Close which?" never answered does not take a later lone number
/// that is not an open request (a ZIP code), nor anything once the jobs are answered; an open
/// number still answers it.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_close_which_ignores_numbers_not_open(mut db: Db) {
    let (_, jobs) = request(&mut db, 0x9501, &["plumbing", "hvac"]).await;
    let ask = text(0x9501, "never mind", 10);
    answer_close(&mut db, &secret(), &ask).await.expect("ask");
    for (minute, words) in [(11, "86403"), (12, "2")] {
        let done = answer_close(&mut db, &secret(), &text(0x9501, words, minute))
            .await
            .expect("number");
        assert_eq!(done, (Closing::Nothing, None), "{words}");
    }
    for (job, _) in &jobs {
        assert_eq!(status(&mut db, *job).await, "intake", "nothing closes");
    }
    let second = jobs[1].1.to_string();
    let (done, _) = answer_close(&mut db, &secret(), &text(0x9501, &second, 13))
        .await
        .expect("answer");
    assert_eq!(done, Closing::Closed(vec![jobs[1].1]));
    let done = answer_close(&mut db, &secret(), &text(0x9501, "all", 14))
        .await
        .expect("all");
    assert_eq!(
        done,
        (Closing::Nothing, None),
        "answered: all is not a close"
    );
    assert_eq!(status(&mut db, jobs[0].0).await, "intake");
}

/// TD-JOB-05 (JOB-023, D-1203): "cancel <number>" with several open jobs gets the how-to line for
/// that number, not the newest, and closes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_mention_names_the_number_written(mut db: Db) {
    let (_, jobs) = request(&mut db, 0x9601, &["plumbing", "hvac"]).await;
    let first = jobs[0].1;
    let words = format!("cancel {first}");
    let (done, sent) = answer_close(&mut db, &secret(), &text(0x9601, &words, 10))
        .await
        .expect("mention");
    assert_eq!(done, Closing::Explained(first));
    assert_eq!(
        body(&mut db, sent.expect("how-to").outbox_id).await,
        format!("Ask Hava: To close request {first}, reply CLOSE {first}.")
    );
    for (job, _) in &jobs {
        assert_eq!(status(&mut db, *job).await, "intake");
    }
}
