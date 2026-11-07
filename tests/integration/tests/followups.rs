//! A Connected job's follow-ups on a fresh database (T097: TD-JOB-04, TD-JOB-08, TD-JOB-09,
//! TD-TXT-13; JOB-015, JOB-016, JOB-028, TXT-029; D-071, D-222, D-240, D-1812).
//!
//! A job walks the arrows to Connected with its check-ins scheduled, they run as a worker would,
//! and answers go through `answer_check_in`. Homeowner texts are checked as the
//! `jobs.homeowner_text` events `intake` sends; alerts as `ops.alert` rows. All data is fake.

use askhava_businesses::api::has_safety_word;
use askhava_core::api::{Claimed, Guarded, WorkerId, claim, finish};
use askhava_jobs::api::rules::{CheckIn, Contact};
use askhava_jobs::api::{
    Answer, CHECK_IN_DAY3, CHECK_IN_DAY14, CHECK_IN_KIND, FEEDBACK_REQUEST, Inbound, JobStatus,
    Next, PROBLEM_ALERT, PROBLEM_REPLY, PROBLEM_SAFETY_ALERT, TEXT_KIND, answer_check_in,
    check_in_times, create, current, move_to, parse_answer, run_check_in, schedule_check_ins,
};
use askhava_messaging::api::{Language, render};
use askhava_ops::api::Actor;
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// Day `d` after the release, 2026-11-02 10:00 Arizona time.
fn days(d: i64) -> Timestamp {
    let start: Timestamp = "2026-11-02T17:00:00Z".parse().expect("instant");
    start + SignedDuration::from_hours(d * 24)
}

/// The moves from Quotes ready to Connected.
const TO_CONNECTED: [JobStatus; 4] = [
    JobStatus::QuotesReady,
    JobStatus::AwaitingConfirm,
    JobStatus::PaymentPending,
    JobStatus::Connected,
];

/// Walks job `id` along `path` at day `d`.
async fn walk(db: &mut PgConnection, id: Uuid, path: &[JobStatus], d: i64) {
    for w in path.windows(2) {
        let moved = move_to(db, id, (w[0], w[1]), days(d), Actor::System).await;
        assert!(moved.expect("move").applied().is_some());
    }
}

/// A job walked to Connected at day 0, its check-ins scheduled twice (queued once).
async fn connected(db: &mut PgConnection) -> Uuid {
    let id = create(db, days(0), false, Actor::System).await;
    let id = id.expect("job");
    let open = [JobStatus::Intake, JobStatus::Open, JobStatus::QuotesReady];
    walk(db, id, &open, 0).await;
    walk(db, id, &TO_CONNECTED, 0).await;
    for _ in 0..2 {
        let scheduled = schedule_check_ins(db, id, days(0), None).await;
        scheduled.expect("schedule");
    }
    id
}

/// Runs every check-in due at day `d` and returns the templates of the homeowner texts queued.
async fn fire(pool: &PgPool, d: i64) -> Vec<String> {
    let owner = WorkerId::new(pool).await.expect("worker");
    let mut texts = Vec::new();
    loop {
        let lease = SignedDuration::from_mins(5);
        let claimed: Vec<Claimed> = claim(pool, owner, days(d), lease, 100)
            .await
            .expect("claim");
        if claimed.is_empty() {
            return texts;
        }
        for task in claimed {
            finish(pool, owner, &task, days(d)).await.expect("finish");
            if task.kind == CHECK_IN_KIND {
                run_check_in(pool.clone(), task).await.expect("check-in");
            } else if task.kind == TEXT_KIND {
                texts.push(task.payload["template"].as_str().unwrap_or("").to_owned());
            }
        }
    }
}

/// The fake inbound text row every answer here came in.
const INBOX: Uuid = Uuid::from_u128(0x0097);

/// Applies `words` to `check` on email-contact job `id` at day 3.
async fn answer(db: &mut PgConnection, id: Uuid, words: &str, check: CheckIn) -> Guarded<Next> {
    let parsed = parse_answer(words).expect("an answer");
    let how = (parsed, check, Contact::Email);
    let safety = has_safety_word(words);
    let inbound = Inbound {
        inbox: INBOX,
        safety,
    };
    let answered = answer_check_in(db, id, how, inbound, (days(3), Actor::System)).await;
    answered.expect("answer")
}

async fn status(db: &mut PgConnection, id: Uuid) -> &'static str {
    current(db, id).await.expect("job").status.code()
}

/// The `ops.alert` kinds raised, in order.
async fn alerts(pool: &PgPool) -> Vec<String> {
    let sql = "SELECT kind FROM ops.alert ORDER BY raised_at, id";
    sqlx::query_scalar(sql)
        .fetch_all(pool)
        .await
        .expect("alerts")
}

/// A claimed check-in task carrying `payload`.
fn check_in(payload: Value) -> Claimed {
    let kind = CHECK_IN_KIND.to_owned();
    Claimed {
        id: 0,
        kind,
        payload,
        attempts: 1,
    }
}

/// TD-JOB-08 (JOB-028, D-240): every reply word in English and Spanish, LISTO = DONE. TD-JOB-09
/// (JOB-015): an earliest visit 5 days after the release puts day 3 on visit day 3; day 14
/// counts from the release, and a day 3 not before day 14 is dropped.
#[test]
fn td_job_08_td_job_09_reply_words_and_check_in_times() {
    let cases: [(Answer, &[&str]); 6] = [
        (Answer::Done, &["DONE", "listo"]),
        (Answer::No, &["No."]),
        (Answer::Problem, &["PROBLEM he never finished", "problema"]),
        (Answer::Later, &["later", "Más  tarde", "mas tarde!"]),
        (Answer::Ongoing, &["ONGOING", "continuo"]),
        (Answer::Other, &["Other", "OTRO"]),
    ];
    for (want, all) in cases {
        for words in all {
            assert_eq!(parse_answer(words), Some(want), "{words}");
        }
    }
    for words in ["", "done soon?", "not later", "stop by at 3"] {
        assert_eq!(parse_answer(words), None, "{words}");
    }
    let times = |visit: Option<i64>| check_in_times(days(0), visit.map(days));
    assert_eq!(times(None), (Some(days(3)), days(14)));
    assert_eq!(times(Some(5)), (Some(days(8)), days(14)));
    assert_eq!(times(Some(-2)), (Some(days(3)), days(14)));
    assert_eq!(times(Some(11)), (None, days(14)));
}

/// TD-JOB-08 (JOB-028): LATER at day 3 moves nothing and the day-14 check-in asks again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_08_later_at_day_3_asks_again_at_day_14(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = connected(&mut db).await;
    assert_eq!(fire(&pool, 3).await, [CHECK_IN_DAY3]);
    let later = answer(&mut db, id, "LATER", CheckIn::Day3).await;
    assert_eq!(later, Guarded::Applied(Next::AskAtDay14));
    assert_eq!(status(&mut db, id).await, "connected");
    assert_eq!(fire(&pool, 14).await, [CHECK_IN_DAY14]);
    let again = answer(&mut db, id, "más tarde", CheckIn::Day14).await;
    assert_eq!(again, Guarded::Applied(Next::Nothing));
    assert!(alerts(&pool).await.is_empty());
}

/// TD-JOB-08 (JOB-028, JOB-017's request): ONGOING moves to Done with the one feedback request
/// and no more check-ins; DONE does the same. A bad check-in payload fails; one for a job that
/// no longer exists does nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_08_ongoing_is_done_with_no_more_check_ins(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = connected(&mut db).await;
    let ongoing = answer(&mut db, id, "CONTINUO", CheckIn::Day3).await;
    let done = Next::Move(JobStatus::Done, Some(FEEDBACK_REQUEST));
    assert_eq!(ongoing, Guarded::Applied(done));
    assert_eq!(status(&mut db, id).await, "done");
    assert_eq!(fire(&pool, 15).await, [FEEDBACK_REQUEST]);
    let repeat = answer(&mut db, id, "DONE", CheckIn::Day3).await;
    assert_eq!(repeat, Guarded::MovedOn);
    let late = answer(&mut db, id, "LATER", CheckIn::Day3).await;
    assert_eq!(late, Guarded::MovedOn);
    assert!(fire(&pool, 16).await.is_empty());
    let bad = check_in(json!({"job": id, "template": "other"}));
    assert!(run_check_in(pool.clone(), bad).await.is_err());
    let at = days(3).to_string();
    let gone =
        json!({"job": Uuid::from_u128(1), "version": 1, "template": CHECK_IN_DAY3, "at": at});
    assert!(run_check_in(pool.clone(), check_in(gone)).await.is_ok());
    let other = connected(&mut db).await;
    let listo = answer(&mut db, other, "LISTO", CheckIn::Day14).await;
    assert!(listo.applied().is_some());
    assert_eq!(fire(&pool, 16).await, [FEEDBACK_REQUEST]);
}

/// TD-JOB-08 (JOB-028, JOB-015, D-1812): OTHER closes with no no-show, even under email contact
/// at day 3, where NO returns the job to Quotes ready; neither sends the feedback request. That
/// job connected again gets its own check-ins, and the first connection's day 14 does nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_08_other_closes_and_a_second_connection_is_checked_again(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = connected(&mut db).await;
    let no = connected(&mut db).await;
    assert_eq!(fire(&pool, 3).await, [CHECK_IN_DAY3; 2]);
    let other = answer(&mut db, id, "otro", CheckIn::Day3).await;
    let closed = Next::Move(JobStatus::Closed, None);
    assert_eq!(other, Guarded::Applied(closed));
    assert_eq!(status(&mut db, id).await, "closed");
    let no_show = answer(&mut db, no, "NO", CheckIn::Day3).await;
    assert!(no_show.applied().is_some());
    assert_eq!(status(&mut db, no).await, "quotes_ready");
    walk(&mut db, no, &TO_CONNECTED, 4).await;
    let again = schedule_check_ins(&mut db, no, days(4), None).await;
    again.expect("schedule again");
    assert_eq!(fire(&pool, 7).await, [CHECK_IN_DAY3]);
    assert!(fire(&pool, 14).await.is_empty());
    assert_eq!(fire(&pool, 18).await, [CHECK_IN_DAY14]);
}

/// TD-JOB-04, TD-JOB-08, TD-TXT-13 (JOB-016, TXT-029, D-071, D-222): a day-3 PROBLEM about a
/// final bill above the quote closes the job, queues the TXT-029 reply and writes one digest alert
/// naming its text's inbox row; a repeat writes nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_04_problem_is_listed_for_casey_with_its_text(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = connected(&mut db).await;
    let words = "PROBLEM the final bill was $900 over the quote";
    let first = answer(&mut db, id, words, CheckIn::Day3).await;
    let closed = Next::Move(JobStatus::Closed, Some(PROBLEM_REPLY));
    assert_eq!(first, Guarded::Applied(closed));
    let repeat = answer(&mut db, id, words, CheckIn::Day3).await;
    assert_eq!(repeat, Guarded::MovedOn);
    assert_eq!(alerts(&pool).await, [PROBLEM_ALERT]);
    let sql = "SELECT severity, subject_kind, subject_id FROM ops.alert";
    let row: (String, String, Uuid) = sqlx::query_as(sql).fetch_one(&pool).await.expect("row");
    assert_eq!(row, ("normal".into(), "messaging.inbox".into(), INBOX));
    assert_eq!(fire(&pool, 3).await, [PROBLEM_REPLY]);
}

/// TD-JOB-08, TD-JOB-09 (JOB-016, OPS-005, D-277): "he stole", "unsafe" and "threat" raise the
/// safety alert for the OPS-032 push instead of the digest row, at day 14 too (D-1812).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_09_problem_with_a_safety_word_raises_the_safety_alert(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    for words in ["PROBLEM he stole it", "problem: unsafe", "PROBLEM a threat"] {
        let id = connected(&mut db).await;
        let next = answer(&mut db, id, words, CheckIn::Day14).await;
        assert!(next.applied().is_some(), "{words}");
    }
    assert_eq!(alerts(&pool).await, [PROBLEM_SAFETY_ALERT; 3]);
}

/// TD-TXT-13 (TXT-029, D-222): the PROBLEM reply in English and, for a Spanish homeowner, Spanish.
#[test]
fn td_txt_13_problem_reply_in_both_languages() {
    let body = |language| {
        render(PROBLEM_REPLY, language, true, &[])
            .expect("text")
            .body
    };
    let (en, es) = (body(Language::En), body(Language::Es));
    let want = "reads every problem report, usually within 7 days";
    assert!(en.contains(want), "{en}");
    assert!(es.contains("7 días"), "{es}");
    assert!(es.contains("help/roc-complaint"), "{es}");
}
