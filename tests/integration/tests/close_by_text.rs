//! Closing a job by text through the route's entry, `intake::api::receive_raw_reply` (TD-JOB-05:
//! JOB-018 to JOB-023; INT-002; D-4051, D-4064). Fake IDs, fake keys and fake text only.
//!
//! "CLOSE <number>", "CLOSE ALL", a cancel-intent message and a lone open number answering
//! "Close which?" close jobs inside the reply; "cancel" alone (an opt-out) and a mere mention close
//! nothing, a mention getting the JOB-023 line on top of the usual handling; "never mind, I smell
//! gas" keeps its 911 text and closes nothing; a CLOSE text never reaches the name-and-address step.

use askhava_intake::api::rules::{Inbound, SAFETY_TEMPLATE, Settings};
use askhava_intake::api::{
    CLOSE_MENTION_TEMPLATE, CLOSE_WHICH_TEMPLATE, Closing, Handled, JOB_CLOSED_TEMPLATE,
    JOBS_CLOSED_TEMPLATE, JobStart, Opened, RawReply, Reply, Screened, TextIn,
    ZIP_QUESTION_TEMPLATE, begin_job, name_address_open, receive_raw_reply, receive_text,
};
use askhava_jobs::api::{current, job_number};
use askhava_privacy::api::Vault;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jiff::Timestamp;
use secrecy::SecretString;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x57), &key(0x58), &url).expect("vault")
}

fn at(minute: i64) -> Timestamp {
    Timestamp::from_second(1_793_000_000 + minute * 60).expect("time")
}

fn text(contact: u128, words: &str, minute: i64) -> TextIn<'_> {
    TextIn {
        inbox_id: Uuid::from_u128(0x57_0000 + contact * 100 + minute.unsigned_abs() as u128),
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

/// The reply to `msg` through the route's entry.
async fn reply(db: &mut Db, msg: &TextIn<'_>) -> Reply {
    let raw = SecretString::from(msg.inbound.words.to_owned());
    let vault = vault();
    let keys = RawReply {
        raw: &raw,
        vault: &vault,
    };
    receive_raw_reply(db, &secret(), (msg, keys), (Settings::default(), &[]))
        .await
        .expect("reply")
}

async fn status(db: &mut Db, job: Uuid) -> &'static str {
    current(db, job).await.expect("job").status.code()
}

async fn audits(db: &mut Db, job: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM ops.audit_log WHERE subject_id = $1")
        .bind(job)
        .fetch_one(&mut **db)
        .await
        .expect("audit rows")
}

async fn body(db: &mut Db, outbox: Uuid) -> String {
    sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE id = $1")
        .bind(outbox)
        .fetch_one(&mut **db)
        .await
        .expect("body")
}

/// The `Handled` of a reply that went on to `receive_text`.
fn handled(reply: Reply) -> Handled {
    match reply {
        Reply::Handled(handled) => handled,
        other => panic!("handled as any reply, got {other:?}"),
    }
}

fn templates(handled: &Handled) -> Vec<&'static str> {
    handled.texts.iter().map(|q| q.template).collect()
}

/// TD-JOB-05 (JOB-018, JOB-019, JOB-022, D-4051): "CLOSE <number>" closes that open job with its
/// audit row and texts `job_closed`; so does "please cancel my request" with one open job.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_close_number_and_intent_close_by_text(mut db: Db) {
    for (contact, words) in [(0xa001, None), (0xa002, Some("please cancel my request"))] {
        let (_, jobs) = request(&mut db, contact, &["plumbing"]).await;
        let (job, number) = jobs[0];
        let before = audits(&mut db, job).await;
        let close = format!("CLOSE {number}");
        let words = words.unwrap_or(&close);
        let Reply::Closed(done, sent) = reply(&mut db, &text(contact, words, 10)).await else {
            panic!("{words} closes");
        };
        assert_eq!(done, Closing::Closed(vec![number]), "{words}");
        assert_eq!(status(&mut db, job).await, "cancelled", "{words}");
        assert_eq!(audits(&mut db, job).await, before + 1, "{words}");
        let sent = sent.expect("says so");
        assert_eq!(sent.template, JOB_CLOSED_TEMPLATE);
        assert!(
            body(&mut db, sent.outbox_id)
                .await
                .contains(&number.to_string())
        );
    }
}

/// TD-JOB-05 (JOB-019, JOB-021): "CLOSE ALL" with two open jobs closes both and texts `jobs_closed`.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_close_all_by_text(mut db: Db) {
    let (_, jobs) = request(&mut db, 0xa101, &["plumbing", "hvac"]).await;
    let Reply::Closed(done, sent) = reply(&mut db, &text(0xa101, "CLOSE ALL", 10)).await else {
        panic!("CLOSE ALL closes");
    };
    assert_eq!(done, Closing::Closed(jobs.iter().map(|j| j.1).collect()));
    assert_eq!(sent.expect("text").template, JOBS_CLOSED_TEMPLATE);
    for (job, _) in &jobs {
        assert_eq!(status(&mut db, *job).await, "cancelled");
    }
}

/// TD-JOB-05 (JOB-021): with two open jobs a cancel-intent text gets "Close which?" and closes
/// nothing; a following lone open number closes that one only.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_close_which_by_text(mut db: Db) {
    let (_, jobs) = request(&mut db, 0xa201, &["plumbing", "hvac"]).await;
    let numbers: Vec<i64> = jobs.iter().map(|j| j.1).collect();
    let ask = text(0xa201, "please cancel my request", 10);
    let Reply::Closed(done, sent) = reply(&mut db, &ask).await else {
        panic!("asks which");
    };
    assert_eq!(done, Closing::Which(numbers.clone()));
    assert_eq!(sent.expect("Close which?").template, CLOSE_WHICH_TEMPLATE);
    for (job, _) in &jobs {
        assert_eq!(status(&mut db, *job).await, "intake", "nothing closes yet");
    }
    let first = numbers[0].to_string();
    let Reply::Closed(done, _) = reply(&mut db, &text(0xa201, &first, 11)).await else {
        panic!("the lone number answers");
    };
    assert_eq!(done, Closing::Closed(vec![numbers[0]]));
    assert_eq!(status(&mut db, jobs[0].0).await, "cancelled");
    assert_eq!(status(&mut db, jobs[1].0).await, "intake");
}

/// TD-JOB-05 (JOB-020, D-005): "cancel" alone is an opt-out `messaging` answered; it closes no job
/// and no close text goes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_cancel_alone_closes_nothing_by_text(mut db: Db) {
    let (_, jobs) = request(&mut db, 0xa301, &["plumbing"]).await;
    let mut stop = text(0xa301, "cancel", 10);
    stop.inbound.opt_out = true;
    let handled = handled(reply(&mut db, &stop).await);
    assert!(handled.texts.is_empty(), "{:?}", templates(&handled));
    assert_eq!(status(&mut db, jobs[0].0).await, "intake");
}

/// TD-JOB-05 (JOB-023): "how do I cancel" gets the JOB-023 line and closes nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_how_do_i_cancel_gets_the_close_line(mut db: Db) {
    let (_, jobs) = request(&mut db, 0xa401, &["plumbing"]).await;
    let (job, number) = jobs[0];
    let handled = handled(reply(&mut db, &text(0xa401, "how do I cancel", 10)).await);
    let line = handled
        .texts
        .iter()
        .find(|q| q.template == CLOSE_MENTION_TEMPLATE)
        .expect("the JOB-023 line");
    let said = body(&mut db, line.outbox_id).await;
    assert!(said.ends_with(&format!("To close request {number}, reply CLOSE {number}.")));
    assert_eq!(status(&mut db, job).await, "intake");
}

/// TD-JOB-05 (JOB-023, D-4051): a mention inside a longer intake answer gets the JOB-023 line and
/// is still handled by `receive_text` as it would be without the line.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_a_mention_in_an_answer_is_still_taken(mut db: Db) {
    let words = "I had to cancel the plumber and the sink still drips";
    let (_, jobs) = request(&mut db, 0xa501, &["plumbing"]).await;
    let (_, twin) = request(&mut db, 0xa502, &["plumbing"]).await;
    let got = handled(reply(&mut db, &text(0xa501, words, 10)).await);
    let usual = receive_text(
        &mut db,
        &secret(),
        &text(0xa502, words, 10),
        Settings::default(),
    )
    .await
    .expect("usual");
    let mut expected = templates(&usual);
    expected.push(CLOSE_MENTION_TEMPLATE);
    // INT-043, INT-049 (D-4360): the reply goes on, so the request's ZIP question follows.
    expected.push(ZIP_QUESTION_TEMPLATE);
    assert_eq!(templates(&got), expected);
    assert_eq!((got.then, got.screened), (usual.then, usual.screened));
    assert_eq!(status(&mut db, jobs[0].0).await, "intake");
    assert_eq!(status(&mut db, twin[0].0).await, "intake");
}

/// TD-JOB-05 (INT-002, D-4064): "never mind, I smell gas" with one open job reads as intent but
/// is not clear: it goes to `receive_text`, whose 911 text is queued, and no job closes.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_never_mind_i_smell_gas_keeps_the_911_text(mut db: Db) {
    let (_, jobs) = request(&mut db, 0xa601, &["plumbing"]).await;
    let msg = text(0xa601, "never mind, I smell gas", 10);
    let handled = handled(reply(&mut db, &msg).await);
    assert!(matches!(handled.screened, Screened::Matched(_)));
    assert!(templates(&handled).contains(&SAFETY_TEMPLATE));
    assert!(!templates(&handled).contains(&JOB_CLOSED_TEMPLATE));
    assert_eq!(status(&mut db, jobs[0].0).await, "intake");
}

/// TD-JOB-05 (JOB-019, INT-027, D-3703): while the name-and-address question is open, a CLOSE
/// text closes its job and a CLOSE for a number not open gets the JOB-023 line; neither is taken
/// as her name and address.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_05_close_never_reaches_the_name_and_address_step(mut db: Db) {
    for (contact, closes) in [(0xa701_u128, true), (0xa702, false)] {
        let (id, jobs) = request(&mut db, contact, &["plumbing"]).await;
        sqlx::query(
            "INSERT INTO intake.intake_answer (conversation_id, question_id, choice, answered_at) \
             VALUES ($1, 'name_address_asked', 'asked', to_timestamp($2))",
        )
        .bind(id)
        .bind(at(5).as_second() as f64)
        .execute(&mut *db)
        .await
        .expect("ask");
        let contact_id = Uuid::from_u128(contact);
        assert!(name_address_open(&mut db, contact_id).await.expect("open"));
        let number = if closes { jobs[0].1 } else { jobs[0].1 + 7 };
        let words = format!("CLOSE {number}");
        match reply(&mut db, &text(contact, &words, 10)).await {
            Reply::Closed(Closing::Closed(n), _) if closes => assert_eq!(n, vec![number]),
            Reply::Handled(h) if !closes => {
                assert!(templates(&h).contains(&CLOSE_MENTION_TEMPLATE));
                assert!(name_address_open(&mut db, contact_id).await.expect("still"));
            }
            other => panic!("{words}: {other:?}"),
        }
    }
}
