//! The no-reply reminder and close and the LIST reply in the database (TD-INT-08, TD-INT-14:
//! INT-019, INT-021, INT-023, TXT-031). Fake IDs, fake text and fake business names only.
//!
//! A job left in Intake gets one reminder at 24:00:00 (held to 8 a.m. at night) and closes at 72
//! hours with the INT-019 text, also when `jobs`' own Intake timer closed it first; a reply
//! restarts the wait. LIST and LISTA get `quotes`' LIST lines; two quoted jobs get the which text.

use askhava_core::api::Claimed;
use askhava_intake::api::rules::{Inbound, Settings};
use askhava_intake::api::{
    CLOSED_TEMPLATE, JobStart, NO_REPLY_KIND, NoReply, Opened, REMINDER_TEMPLATE, TextIn, Wait,
    answer_list, begin_job, conversation, hear, no_reply_step, receive_text,
};
use askhava_jobs::api::{JobStatus, TIMER_KIND, current, move_to, run_timer};
use askhava_licensing::api::LicenseKind;
use askhava_ops::api::Actor;
use askhava_quotes::api::{
    DayPart, Format, Price, QuoteForm, QuoteRules, parse_quote, submit_quote,
};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;

type Db = sqlx::pool::PoolConnection<sqlx::Postgres>;

fn secret() -> SecretString {
    SecretString::from("test-only-id-secret-not-a-real-key".to_owned())
}

fn az(text: &str) -> Timestamp {
    format!("2026-11-{text}-07:00").parse().expect("time")
}

fn text(contact: u128, words: &'static str, at: Timestamp) -> TextIn<'static> {
    TextIn {
        inbox_id: Uuid::from_u128(0x5_0000 + contact + u128::from(at.as_second().unsigned_abs())),
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

/// A fake request opened by one text at `at`, with one plumbing job; (request, job).
async fn request(db: &mut Db, contact: u128, at: Timestamp) -> (Uuid, Uuid) {
    let msg = text(contact, "my kitchen sink is leaking under the cabinet", at);
    let handled = receive_text(db, &secret(), &msg, Settings::default())
        .await
        .expect("open");
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

/// The queued no-reply steps of `job`: (step, due).
async fn steps(db: &mut Db, job: Uuid) -> Vec<(String, Timestamp)> {
    let rows: Vec<(serde_json::Value, jiff_sqlx::Timestamp)> = sqlx::query_as(
        "SELECT payload, run_at FROM core.task WHERE kind = $1 AND payload->>'job' = $2 ORDER BY run_at, id",
    )
    .bind(NO_REPLY_KIND)
    .bind(job.to_string())
    .fetch_all(&mut **db)
    .await
    .expect("tasks");
    rows.into_iter()
        .map(|(p, at)| (p["step"].as_str().expect("step").to_owned(), at.to_jiff()))
        .collect()
}

async fn body(db: &mut Db, outbox: Uuid) -> String {
    sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE id = $1")
        .bind(outbox)
        .fetch_one(&mut **db)
        .await
        .expect("body")
}

fn wait(request: Uuid, job: Uuid, step: NoReply, from: Timestamp) -> Wait {
    Wait {
        job,
        conversation: request,
        step,
        from,
    }
}

/// TD-INT-08 (INT-019, D-095, D-419): nothing before 24:00:00, one reminder at it, closed at 72
/// hours with "Text us any time and we'll start again.", and the request closes with its job.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_08_reminder_at_24_hours_and_close_at_72(mut db: Db) {
    let from = az("02T10:00:00");
    let (id, job) = request(&mut db, 0x8001, from).await;
    let due = vec![
        ("remind".to_owned(), az("03T10:00:00")),
        ("close".to_owned(), az("05T10:00:00")),
    ];
    assert_eq!(steps(&mut db, job).await, due, "nothing is due at 23:59:59");
    let remind = wait(id, job, NoReply::Remind, from);
    let sent = no_reply_step(&mut db, &secret(), &remind)
        .await
        .expect("remind");
    let sent = sent.expect("the reminder");
    assert_eq!(sent.template, REMINDER_TEMPLATE);
    let again = no_reply_step(&mut db, &secret(), &remind)
        .await
        .expect("again");
    assert_eq!(
        again.map(|q| q.outbox_id),
        Some(sent.outbox_id),
        "one reminder"
    );
    let close = wait(id, job, NoReply::Close, from);
    let told = no_reply_step(&mut db, &secret(), &close)
        .await
        .expect("close");
    let told = told.expect("the closing text");
    assert_eq!(told.template, CLOSED_TEMPLATE);
    assert!(
        body(&mut db, told.outbox_id)
            .await
            .ends_with("Text us any time and we'll start again.")
    );
    let row = current(&mut db, job).await.expect("job");
    assert_eq!(row.status.code(), "closed_early");
    assert_eq!(row.status_at, az("05T10:00:00"));
    let open = conversation(&mut db, id).await.expect("read").expect("row");
    assert!(open.closed_at.is_some(), "the next text starts again");
}

/// TD-INT-08 (INT-019, QH-001, moved from TD-CON-11): a reminder or close at night waits for 8 a.m.
/// Arizona, and the close's text follows at 8 a.m.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_08_night_reminder_and_closing_text_held_to_8_am(mut db: Db) {
    let from = az("02T21:30:00");
    let (id, job) = request(&mut db, 0x8002, from).await;
    let due = vec![
        ("remind".to_owned(), az("04T08:00:00")),
        ("close".to_owned(), az("05T21:30:00")),
    ];
    assert_eq!(steps(&mut db, job).await, due);
    let close = wait(id, job, NoReply::Close, from);
    let now = no_reply_step(&mut db, &secret(), &close)
        .await
        .expect("close");
    assert!(now.is_none(), "no closing text at night");
    let held = steps(&mut db, job).await;
    assert_eq!(held.last(), Some(&("tell".to_owned(), az("06T08:00:00"))));
    let tell = wait(id, job, NoReply::Tell, from);
    let told = no_reply_step(&mut db, &secret(), &tell)
        .await
        .expect("tell");
    assert_eq!(told.map(|q| q.template), Some(CLOSED_TEMPLATE));
}

/// TD-INT-08 (INT-019, D-807): a reply restarts the wait, so the old reminder and close do nothing
/// and a new pair counts from the reply.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_08_a_reply_restarts_the_wait(mut db: Db) {
    let from = az("02T10:00:00");
    let (id, job) = request(&mut db, 0x8003, from).await;
    let reply = text(0x8003, "under the sink", az("02T15:00:00"));
    hear(&mut db, id, &reply).await.expect("hear");
    for step in [NoReply::Remind, NoReply::Close] {
        let old = wait(id, job, step, from);
        let done = no_reply_step(&mut db, &secret(), &old).await.expect("old");
        assert!(done.is_none());
    }
    assert_eq!(
        current(&mut db, job).await.expect("job").status.code(),
        "intake"
    );
    let due = steps(&mut db, job).await;
    assert!(due.contains(&("remind".to_owned(), az("03T15:00:00"))));
    assert!(due.contains(&("close".to_owned(), az("05T15:00:00"))));
}

/// TD-INT-08 (INT-019, JOB-006): when `jobs`' own Intake timer closes the job first, the close
/// step still sends the INT-019 text once, and a job closed another way gets none.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_08_the_jobs_timer_closing_first_still_texts(pool: sqlx::PgPool) {
    let mut db = pool.acquire().await.expect("connection");
    let from = az("02T10:00:00");
    let (id, job) = request(&mut db, 0x8004, from).await;
    let row = current(&mut db, job).await.expect("job");
    let task = Claimed {
        id: 0,
        kind: TIMER_KIND.to_owned(),
        payload: json!({"job": job, "status": "intake", "version": row.version, "timer": row.timer_version}),
        attempts: 1,
    };
    run_timer(pool, task).await.expect("timer");
    let close = wait(id, job, NoReply::Close, from);
    let told = no_reply_step(&mut db, &secret(), &close)
        .await
        .expect("close");
    assert_eq!(told.map(|q| q.template), Some(CLOSED_TEMPLATE));
    let (other, cancelled) = request(&mut db, 0x8005, from).await;
    let to = (JobStatus::Intake, JobStatus::Cancelled);
    let moved = move_to(&mut db, cancelled, to, az("03T09:00:00"), Actor::System).await;
    assert!(moved.expect("cancel").applied().is_some());
    let close = wait(other, cancelled, NoReply::Close, from);
    assert!(
        no_reply_step(&mut db, &secret(), &close)
            .await
            .expect("close")
            .is_none()
    );
}

/// Business `n` quotes on `job` (fake business, confirmed at submit).
async fn quote(db: &mut Db, job: Uuid, n: u128) {
    sqlx::query("INSERT INTO quotes.job_card (job_id, business_id, handyman, sent_at) VALUES ($1, $2, false, $3)")
        .bind(job)
        .bind(Uuid::from_u128(n))
        .bind(az("03T08:00:00").to_sqlx())
        .execute(&mut **db)
        .await
        .expect("card");
    let form = QuoteForm {
        price: Price::Firm(19_000),
        notes: String::new(),
        original: "Sample wording".to_owned(),
        ai_read: false,
        earliest_visit: Some((jiff::civil::date(2026, 11, 9), DayPart::Evening)),
    };
    let rules = QuoteRules {
        format: Format::Firm,
        construction_type: false,
        license: LicenseKind::None,
        handyman: false,
        certified: false,
    };
    let checked = parse_quote(form, &rules).expect("quote");
    let saved = submit_quote(db, (job, Uuid::from_u128(n)), &checked, az("03T09:00:00")).await;
    assert!(saved.is_ok());
}

/// TD-INT-08 (TXT-031, D-491): LIST gets `quotes`' LIST text for the request's quoted job, LISTA
/// the Spanish one; a normal message gets nothing; two quoted jobs get the which-request text.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_int_08_list_and_lista_route_to_the_list_lines(mut db: Db) {
    let from = az("02T10:00:00");
    let (id, job) = request(&mut db, 0x8006, from).await;
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(&mut db, job, open, from, Actor::System).await;
    assert!(moved.expect("open").applied().is_some());
    quote(&mut db, job, 1).await;
    let names = vec![(Uuid::from_u128(1), "Sample Business 1".to_owned())];
    let ask = |words| text(0x8006, words, az("03T11:00:00"));
    let none = answer_list(&mut db, &secret(), &ask("my sink"), (id, true), &names).await;
    assert_eq!(none.expect("none").len(), 0);
    let en = answer_list(&mut db, &secret(), &ask("LIST"), (id, true), &names).await;
    let en = en.expect("list");
    assert_eq!(en.len(), 1);
    assert!(
        body(&mut db, en[0].outbox_id)
            .await
            .contains("1. Sample Business 1: $190.")
    );
    let es = answer_list(
        &mut db,
        &secret(),
        &text(0x8006, "lista", az("03T12:00:00")),
        (id, true),
        &names,
    )
    .await;
    assert!(
        body(&mut db, es.expect("lista")[0].outbox_id)
            .await
            .contains("2 SÍ")
    );
    let second = JobStart {
        conversation: id,
        words: "the water heater is leaking too",
        emergency: false,
        trade: "water_heater",
        at: from,
    };
    let Opened::Created(job2) = begin_job(&mut db, &second).await.expect("job") else {
        panic!("a second job");
    };
    let moved = move_to(&mut db, job2, open, from, Actor::System).await;
    assert!(moved.expect("open").applied().is_some());
    quote(&mut db, job2, 1).await;
    let which = answer_list(
        &mut db,
        &secret(),
        &text(0x8006, "LIST", az("03T13:00:00")),
        (id, false),
        &names,
    )
    .await;
    let which = which.expect("which");
    assert_eq!(which[0].template, "quote_list_which");
}
