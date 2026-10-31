//! Every waiting status's timer on a fresh database, with the homeowner texts the timeouts and the
//! every-window-ended call queue (TD-JOB-03, TD-PICK-33: JOB-007 to JOB-015, JOB-025, JOB-026,
//! TXT-042, D-871, D-872).
//!
//! Timers are claimed and run as a fresh worker would at a chosen instant; a homeowner text is a
//! `jobs.homeowner_text` event (IDs and numbers only) whose sender is `intake`'s, so these cases
//! check the event, render its template as the sender will, and judge quiet hours with
//! `messaging`'s own rule. JOB-008's checks before Open are `intake`'s; here a job walks the
//! arrows `intake` will drive. All data is fake (the phone is a 555 number).

use askhava_core::api::{Claimed, Guarded, WorkerId, claim, finish};
use askhava_jobs::api::rules::{CheckIn, Contact, Reply, Windows, WindowsEnd};
use askhava_jobs::api::{
    JobStatus, TEXT_KIND, TIMER_KIND, all_windows_ended, connected_reply, create, current, move_to,
    run_timer, text_due,
};
use askhava_messaging::api::{
    AreaCodes, Channel, Decision, Kind, Language, Outgoing, decide, render,
};
use askhava_ops::api::Actor;
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// 2026-11-02 10:00 Arizona time (17:00 UTC).
fn start() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("instant")
}

fn hours(h: i64) -> Timestamp {
    start() + SignedDuration::from_hours(h)
}

/// The arrows from Intake to Connected, in order.
const PATH: [JobStatus; 6] = [
    JobStatus::Intake,
    JobStatus::Open,
    JobStatus::QuotesReady,
    JobStatus::AwaitingConfirm,
    JobStatus::PaymentPending,
    JobStatus::Connected,
];

/// Moves job `id` along one arrow at `at` and checks the move applied.
async fn go(db: &mut PgConnection, id: Uuid, step: (JobStatus, JobStatus), at: Timestamp) {
    let moved = move_to(db, id, step, at, Actor::System).await;
    assert!(moved.expect("move").applied().is_some());
}

/// Creates a job at the start and walks it `steps` arrows along [`PATH`], all at the start.
async fn job_at(db: &mut PgConnection, steps: usize) -> Uuid {
    let id = create(db, start(), false, Actor::System)
        .await
        .expect("job");
    for w in PATH[..=steps].windows(2) {
        go(db, id, (w[0], w[1]), start()).await;
    }
    id
}

async fn status(db: &mut PgConnection, id: Uuid) -> &'static str {
    current(db, id).await.expect("job").status.code()
}

/// Claims every task due at `now` as a fresh worker, runs the job timers and returns the
/// homeowner-text events' payloads, including those the timers queue; each claimed task is
/// finished.
async fn fire(pool: &PgPool, now: Timestamp) -> Vec<Value> {
    let owner = WorkerId::new(pool).await.expect("worker");
    let lease = SignedDuration::from_mins(5);
    let mut texts = Vec::new();
    loop {
        let claimed: Vec<Claimed> = claim(pool, owner, now, lease, 100).await.expect("claim");
        if claimed.is_empty() {
            return texts;
        }
        for task in claimed {
            finish(pool, owner, &task, now).await.expect("finish");
            if task.kind == TIMER_KIND {
                run_timer(pool.clone(), task).await.expect("timer");
            } else if task.kind == TEXT_KIND {
                texts.push(task.payload);
            }
        }
    }
}

/// The (template, values) of each text event, in order.
fn kinds(texts: &[Value]) -> Vec<(String, Value)> {
    let pair = |t: &Value| {
        (
            t["template"].as_str().unwrap_or("").to_owned(),
            t["values"].clone(),
        )
    };
    texts.iter().map(pair).collect()
}

/// TD-JOB-03: Open becomes No quotes yet at 6 p.m. the next day with the JOB-010 text, then
/// closes 5 days later with the JOB-011 note (JOB-010, JOB-011, D-030).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_03_open_to_no_quotes_yet_to_closed_with_texts(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = job_at(&mut db, 1).await;
    let before_six: Timestamp = "2026-11-04T00:59:00Z".parse().expect("5:59 p.m.");
    assert!(fire(&pool, before_six).await.is_empty());
    assert_eq!(status(&mut db, id).await, "open");
    let six: Timestamp = "2026-11-04T01:00:00Z".parse().expect("6 p.m.");
    let texts = fire(&pool, six).await;
    assert_eq!(status(&mut db, id).await, "no_quotes_yet");
    assert_eq!(
        kinds(&texts),
        [("no_quotes_yet".to_owned(), json!({"job": 1001}))]
    );
    let closed = fire(&pool, six + SignedDuration::from_hours(5 * 24)).await;
    assert_eq!(status(&mut db, id).await, "closed");
    assert_eq!(
        kinds(&closed),
        [("closed_no_quotes".to_owned(), json!({"job": 1001}))]
    );
}

/// TD-JOB-03: the first quote moves Open to Quotes ready and its 6 p.m. timer then does nothing
/// (JOB-009); every return to Quotes ready restarts its 3 days, and a failed pick with quotes
/// still to come leaves the job waiting; the 3-day close sends the JOB-012 note (JOB-009,
/// JOB-012, JOB-013, JOB-025, D-045, D-140, D-461).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_03_quotes_ready_restarts_its_3_days(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = job_at(&mut db, 2).await;
    let pick = (JobStatus::QuotesReady, JobStatus::AwaitingConfirm);
    go(&mut db, id, pick, hours(24)).await;
    assert!(fire(&pool, hours(30)).await.is_empty(), "no confirm in 4 h");
    assert_eq!(status(&mut db, id).await, "quotes_ready");
    assert!(fire(&pool, hours(72)).await.is_empty(), "the first 3 days");
    assert_eq!(status(&mut db, id).await, "quotes_ready");
    let texts = fire(&pool, hours(24 + 4 + 72)).await;
    assert_eq!(status(&mut db, id).await, "closed");
    assert_eq!(
        kinds(&texts),
        [("closed_no_pick".to_owned(), json!({"job": 1001}))]
    );
}

/// TD-JOB-03: when every window has ended with quotes and no pick, exactly one all-quotes-in
/// text, held to 8 a.m. in quiet hours, and none due after a pick (TXT-042, QH-001, QH-006).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_03_all_quotes_in_once_and_never_after_a_pick(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = job_at(&mut db, 2).await;
    let w = Windows {
        quoted: 2,
        sent: 4,
        choosable: 2,
    };
    let night = hours(11); // 9 p.m. Arizona time
    for _ in 0..2 {
        let ended = all_windows_ended(&mut db, id, w, night).await;
        assert_eq!(ended, Ok(Guarded::Applied(WindowsEnd::AllQuotesIn)));
    }
    let texts = fire(&pool, night).await;
    let values = json!({"job": 1001, "quoted": 2, "count": 4});
    assert_eq!(kinds(&texts), [("all_quotes_in".to_owned(), values)]);
    assert_eq!(status(&mut db, id).await, "quotes_ready");
    let version = i32::try_from(texts[0]["version"].as_i64().expect("v")).expect("v");
    assert_eq!(text_due(&mut db, id, version).await, Ok(true));
    let table = AreaCodes::embedded().expect("table");
    let held = decide(&table, &update("+19285550123"), night).expect("decide");
    let eight: Timestamp = "2026-11-03T15:00:00Z".parse().expect("8 a.m.");
    assert_eq!(held, Decision::Hold { until: eight });
    let pick = (JobStatus::QuotesReady, JobStatus::AwaitingConfirm);
    go(&mut db, id, pick, hours(12)).await;
    assert_eq!(text_due(&mut db, id, version).await, Ok(false));
    let after = all_windows_ended(&mut db, id, w, hours(13)).await;
    assert_eq!(after, Ok(Guarded::MovedOn));
}

/// A non-urgent update text to the fake phone.
fn update(phone: &str) -> Outgoing<'_> {
    Outgoing {
        phone,
        kind: Kind::Update,
        channel: Channel::Text,
        emergency_job: false,
        after_hours_business: false,
        emergency_emails: false,
    }
}

/// TD-JOB-03: with no choosable quote left once every window has ended, the job closes and she
/// is told, and the closing note stays due (JOB-025, D-045, D-140, QH-007).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_03_no_choosable_quote_left_closes(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = job_at(&mut db, 2).await;
    let w = Windows {
        quoted: 1,
        sent: 3,
        choosable: 0,
    };
    let ended = all_windows_ended(&mut db, id, w, hours(30)).await;
    assert_eq!(ended, Ok(Guarded::Applied(WindowsEnd::Close)));
    assert_eq!(status(&mut db, id).await, "closed");
    let texts = fire(&pool, hours(30)).await;
    let note = ("closed_no_quote_left".to_owned(), json!({"job": 1001}));
    assert_eq!(kinds(&texts), [note]);
    let version = i32::try_from(texts[0]["version"].as_i64().expect("v")).expect("v");
    assert_eq!(text_due(&mut db, id, version).await, Ok(true));
}

/// TD-JOB-03: DONE moves a Connected job to Done; with no reply 7 days after the day-14 check-in
/// it closes with no text and so no review request (JOB-015, JOB-026, D-046).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_03_connected_done_or_silent_close(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let done = job_at(&mut db, 5).await;
    let reply = (Reply::Done, CheckIn::Day3, Contact::Call);
    let at = (hours(72), Actor::System);
    assert_eq!(
        connected_reply(&mut db, done, reply, at).await,
        Ok(Guarded::Applied(()))
    );
    assert_eq!(status(&mut db, done).await, "done");
    assert_eq!(
        connected_reply(&mut db, done, reply, at).await,
        Ok(Guarded::MovedOn)
    );
    let silent = job_at(&mut db, 5).await;
    assert!(fire(&pool, hours(21 * 24 - 1)).await.is_empty());
    assert_eq!(status(&mut db, silent).await, "connected");
    assert!(fire(&pool, hours(21 * 24)).await.is_empty(), "no text");
    assert_eq!(status(&mut db, silent).await, "closed");
}

/// TD-JOB-03: every non-final status leaves a pending timer, and each one firing after the job
/// moved does nothing (JOB-003, JOB-006, moved from TD-JOB-01).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_job_03_every_waiting_status_has_a_timer(pool: PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let id = job_at(&mut db, 5).await;
    let back = (JobStatus::Connected, JobStatus::QuotesReady);
    go(&mut db, id, back, start()).await;
    let step = (JobStatus::QuotesReady, JobStatus::Closed);
    go(&mut db, id, step, start()).await;
    let owner = WorkerId::new(&pool).await.expect("worker");
    let far = hours(100 * 24);
    let lease = SignedDuration::from_mins(5);
    let timers: Vec<Claimed> = claim(&pool, owner, far, lease, 100)
        .await
        .expect("claim")
        .into_iter()
        .filter(|t| t.kind == TIMER_KIND)
        .collect();
    let mut seen: Vec<&str> = timers
        .iter()
        .filter_map(|t| t.payload["status"].as_str())
        .collect();
    seen.sort_unstable();
    seen.dedup();
    let waiting: Vec<&str> = {
        let mut w: Vec<_> = PATH.iter().map(|s| s.code()).collect();
        w.sort_unstable();
        w
    };
    assert_eq!(seen, waiting, "one timer per waiting status entered");
    for task in timers {
        run_timer(pool.clone(), task).await.expect("stale timer");
    }
    let job = current(&mut db, id).await.expect("job");
    assert_eq!((job.status.code(), job.version), ("closed", 7));
}

/// The four timeout and closing templates, rendered for job 1001 in `language`.
fn rendered(language: Language) -> Vec<(String, String, bool)> {
    let ids = [
        "no_quotes_yet",
        "closed_no_quotes",
        "closed_no_pick",
        "closed_no_quote_left",
    ];
    ids.iter()
        .map(|id| {
            let t = render(id, language, true, &[("job", "1001")]).expect("template");
            (t.id, t.body, t.language == Language::Es && t.draft)
        })
        .collect()
}

/// TD-PICK-33, TD-JOB-03: the JOB-010 and JOB-012 texts equal their spec text, JOB-010's replies
/// are 1 add details, 2 keep waiting, 3 close, each has a Spanish draft, and no pressure word is
/// in any of them (JOB-010, JOB-011, JOB-012, WORD-024, D-406, D-461).
#[test]
fn td_pick_33_timeout_texts_equal_the_spec() {
    let en = rendered(Language::En);
    assert_eq!(
        en[0].1,
        "Ask Hava: No quotes yet for request 1001. A few more details or a photo often help a \
         business price it. Reply 1 to add details, 2 to keep waiting, or 3 to close."
    );
    assert!(
        en[1]
            .1
            .contains("closed because no quotes came in. Text us any time")
    );
    assert_eq!(
        en[2].1,
        "Ask Hava: Request 1001 is closed because no quote was picked. Text us any time to start \
         a new one."
    );
    let es = rendered(Language::Es);
    assert!(es.iter().all(|(_, _, spanish_draft)| *spanish_draft));
    let pressure = [
        "hurry",
        "don't miss out",
        "act now",
        "limited time",
        "last chance",
    ];
    let presion = ["apúrese", "no se lo pierda"];
    for (id, body, _) in en.iter().chain(&es) {
        let lower = body.to_lowercase();
        let hit = pressure.iter().chain(&presion).find(|w| lower.contains(*w));
        assert_eq!(hit, None, "{id}");
    }
}
