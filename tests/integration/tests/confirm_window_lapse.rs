//! The Confirm window's end, every way it comes, runs the lapse with its backup move or PICK-046
//! text, through the production worker registry (T491; PICK-019, PICK-044, PICK-046, PICK-047,
//! CON-029, QH-001; D-3750, D-3805, D-3882).
//!
//! The registry is `askhava_server::worker_registry` from dev stand-in settings with the job-text
//! keys on a manual clock, the composition root compiled in place as `pick_moved_text.rs` does
//! (D-2853's exception, D-3569). A homeowner's plumbing job has three quotes from named businesses;
//! quote 1 is picked on the page with quote 2 as her bound backup (or none), the pick notice's send
//! is told to `notice_sent`, and the window's end runs as the worker runs it. Every number
//! (555-01xx), name, ID and word is fake.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_core::api::{
    Claimed, Clock, IdSecret, Lease, ManualClock, Registry, Settings, WorkerId, brand, claim,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{JobStart, JobTextKeys, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::{JobStatus, TEXT_KIND, current, move_to, record_job_facts};
use askhava_messaging::api::{RevokeChannel, suppress};
use askhava_ops::api::Actor;
use askhava_picks::api::{
    Browser, CONFIRM_ENDS_KIND, ConfirmSettings, PagePick, PicksError, bind_backup, choose_backup,
    notice_sent, page_pick, run_confirm_ends_at,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(dead_code, reason = "only the worker registry is called here")]
mod server;
use server::*;

const ID_SECRET: &str = "test-only-id-secret-for-t491-not-a-real-key";
const PHONE: &str = "+19285550173";
/// 10:00 a.m. Arizona on a Monday.
const DAY: &str = "2026-11-02T17:00:00Z";
const NAMES: [&str; 3] = ["Sample Pipe Co A", "Sample Pipe Co B", "Sample Pipe Co C"];
const BROWSER: Browser = [9; 32];

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn business(n: usize) -> Uuid {
    Uuid::from_u128(0x491_0000 + u128::try_from(n).expect("small"))
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x59), &key(0x69), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from(ID_SECRET.to_owned())
}

/// The production registry from dev stand-in settings with the job-text keys, on `clock`.
fn registry(clock: Arc<dyn Clock>) -> Registry {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t491-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("storage dir");
    let pairs: Vec<(String, String)> = [
        ("ASKHAVA_APP_ENV", "dev".to_owned()),
        (
            "ASKHAVA_DATABASE_URL",
            "postgres://fake@localhost/fake".into(),
        ),
        (
            "ASKHAVA_STORAGE_BASE_URL",
            format!("file://{}", dir.display()),
        ),
        ("ASKHAVA_ID_SECRET", ID_SECRET.into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let stream =
        offdb_stream::OffDbStream::from_settings(&settings, pairs.clone(), clock).expect("stream");
    let link = IdSecret::new(1, secret()).expect("link key");
    let keys = Arc::new(JobTextKeys::new(link, secret(), vault()));
    let parts = (Some(keys), ConfirmSettings::default());
    worker_registry(&settings, stream, parts, pairs).expect("registry")
}

/// Runs every task due at the clock's now through `registry`, as the worker would, until none is.
async fn run_due(pool: &PgPool, registry: &Registry, clock: &ManualClock) {
    let owner = WorkerId::new(pool).await.expect("worker");
    loop {
        let tasks = claim(pool, owner, clock.now(), SignedDuration::from_secs(60), 50)
            .await
            .expect("claim");
        if tasks.is_empty() {
            return;
        }
        for task in tasks {
            let run = registry.run(pool, clock, owner, task, Lease::WORKER).await;
            run.expect("run");
        }
    }
}

/// Three named businesses, each with a confirmed $190 quote on `job`, in arrival order.
async fn quoted(db: &mut PgConnection, job: Uuid, at: Timestamp) {
    for (i, name) in NAMES.iter().enumerate() {
        let id = business(i + 1);
        sqlx::query(
            "INSERT INTO businesses.business (id, status, created_at, status_changed_at) \
             VALUES ($1, 'active', now(), now())",
        )
        .bind(id)
        .execute(&mut *db)
        .await
        .expect("business");
        sqlx::query(
            "INSERT INTO businesses.business_name (business_id, business_name, changed_at) \
             VALUES ($1, $2, now())",
        )
        .bind(id)
        .bind(*name)
        .execute(&mut *db)
        .await
        .expect("name");
        let received = at + SignedDuration::from_mins(i64::try_from(i).expect("small"));
        sqlx::query(
            "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
             confirm_on_site, notes, original, ai_read, confirmed, received_at) \
             VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
        )
        .bind(job)
        .bind(id)
        .bind(received.to_sqlx())
        .execute(&mut *db)
        .await
        .expect("quote");
    }
}

/// A homeowner texts in and her job (an emergency when `emergency`) opens; returns the job.
async fn her_job(db: &mut PgConnection, emergency: bool, at: Timestamp) -> Uuid {
    askhava_catalog::api::load_embedded(db)
        .await
        .expect("catalog");
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(PHONE.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let words = "water is dripping from the bathroom ceiling";
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
        received_at: at,
    };
    let handled = receive_text(db, &secret(), &msg, IntakeSettings::default()).await;
    let conversation = handled.expect("open").conversation.expect("request");
    let start = JobStart {
        conversation,
        words,
        emergency,
        trade: "plumbing",
        at,
    };
    let Ok(Opened::Created(job)) = begin_job(db, &start).await else {
        panic!("a new job");
    };
    job
}

/// Her job walks to Quotes ready with three quotes, and quote 1 is picked on the page at `at` with
/// quote 2 as her bound backup when `backup`; the queue is then emptied. Returns the job and pick.
async fn picked(
    db: &mut PgConnection,
    (emergency, backup): (bool, bool),
    at: Timestamp,
) -> (Uuid, Uuid) {
    let job = her_job(db, emergency, at).await;
    let types = ["leak repair".to_owned()];
    record_job_facts(db, job, "plumbing_repair", &types)
        .await
        .expect("facts");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, at, Actor::System).await;
        assert!(moved.expect("move").applied().is_some(), "{step:?}");
    }
    quoted(db, job, at).await;
    let Ok(PagePick::Picked(pick)) = page_pick(db, (job, false), 1, Actor::System, at).await else {
        panic!("a page pick");
    };
    if backup {
        choose_backup(db, (job, 1), (Some(2), &BROWSER), at)
            .await
            .expect("asked");
        let bound = bind_backup(db, pick, &BROWSER, at).await.expect("bound");
        assert_eq!(bound, Some(2), "her backup is bound");
    }
    sqlx::query("DELETE FROM core.task")
        .execute(&mut *db)
        .await
        .expect("an empty queue");
    (job, pick)
}

/// The queued window ends, oldest first, with their due times.
async fn window_ends(db: &mut PgConnection) -> Vec<(Value, Timestamp)> {
    let rows: Vec<(Value, i64)> = sqlx::query_as(
        "SELECT payload, (payload->>'due')::bigint FROM core.task WHERE kind = $1 ORDER BY id",
    )
    .bind(CONFIRM_ENDS_KIND)
    .fetch_all(db)
    .await
    .expect("tasks");
    let at = |s| Timestamp::from_second(s).expect("due");
    rows.into_iter().map(|(p, s)| (p, at(s))).collect()
}

fn claimed(payload: Value) -> Claimed {
    Claimed {
        id: 1,
        kind: String::new(),
        payload,
        attempts: 1,
    }
}

/// The job versions of the `template` events queued for ended pick `pick`.
async fn events(db: &mut PgConnection, pick: Uuid, template: &str) -> Vec<i64> {
    sqlx::query_scalar(
        "SELECT (payload->>'version')::bigint FROM core.task WHERE kind = $1 AND dedupe_key = $2",
    )
    .bind(TEXT_KIND)
    .bind(format!("{TEXT_KIND}.{pick}.{template}"))
    .fetch_all(db)
    .await
    .expect("queue")
}

/// The bodies of the queued texts of `template`, oldest first.
async fn sent(db: &mut PgConnection, template: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT body FROM messaging.outbox WHERE template_id = $1 ORDER BY created_at, id",
    )
    .bind(template)
    .fetch_all(db)
    .await
    .expect("outbox")
}

/// The waiting picks on `job` as (business, picked at).
async fn waiting(db: &mut PgConnection, job: Uuid) -> Vec<(Uuid, Timestamp)> {
    let rows: Vec<(Uuid, jiff_sqlx::Timestamp)> = sqlx::query_as(
        "SELECT business_id, picked_at FROM picks.pick WHERE job_id = $1 AND status = 'awaiting_confirm'",
    )
    .bind(job)
    .fetch_all(db)
    .await
    .expect("picks");
    rows.into_iter().map(|(b, t)| (b, t.to_jiff())).collect()
}

async fn pick_status(db: &mut PgConnection, pick: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM picks.pick WHERE id = $1")
        .bind(pick)
        .fetch_one(db)
        .await
        .expect("pick")
}

/// A normal job's pick whose notice went at 10:00 a.m., the registry and the window's end task.
async fn normal_window(pool: &PgPool, backup: bool) -> (Uuid, Uuid, Value, Timestamp) {
    let mut db = pool.acquire().await.expect("db");
    let (job, pick) = picked(&mut db, (false, backup), ts(DAY)).await;
    let due = notice_sent(&mut db, pick, ts(DAY), ConfirmSettings::default()).await;
    let ends = window_ends(&mut db).await;
    assert_eq!(due, Ok(Some(ends[0].1)), "one window end at the deadline");
    (job, pick, ends[0].0.clone(), ends[0].1)
}

/// PICK-047's English text naming the ended business and her backup.
fn moved_text() -> String {
    let short = brand().fill("{brand_short}", false).expect("brand");
    format!(
        "{short}: {} couldn't take it, so your backup, {}, has your pick now. We'll text you when they accept.",
        NAMES[0], NAMES[1]
    )
}

/// PICK-019, PICK-047: at the window's end the pick moves once to her backup, one `pick_moved`
/// event is queued at the job's version after the move and rendered as PICK-047's text; a rerun
/// (a pick no longer waiting) queues nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_047_the_window_end_moves_to_the_backup_once(pool: PgPool) {
    let (job, pick, task, due) = normal_window(&pool, true).await;
    let mut db = pool.acquire().await.expect("db");
    let clock = Arc::new(ManualClock::new(due));
    let registry = registry(clock.clone());
    run_due(&pool, &registry, &clock).await;
    assert_eq!(waiting(&mut db, job).await, vec![(business(2), due)]);
    assert_eq!(pick_status(&mut db, pick).await, "expired");
    let version = i64::from(current(&mut db, job).await.expect("job").version);
    assert_eq!(events(&mut db, pick, "pick_moved").await, vec![version]);
    assert_eq!(sent(&mut db, "pick_moved").await, vec![moved_text()]);
    assert!(events(&mut db, pick, "pick_lapsed").await.is_empty());
    run_confirm_ends_at(pool.clone(), claimed(task), due)
        .await
        .expect("again");
    run_due(&pool, &registry, &clock).await;
    assert_eq!(
        events(&mut db, pick, "pick_moved").await.len(),
        1,
        "a rerun"
    );
    assert_eq!(waiting(&mut db, job).await.len(), 1, "moved once");
}

/// PICK-019, PICK-046: with no backup the window's end queues one `pick_lapsed` event and the job
/// returns to Quotes ready; a rerun queues nothing more.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_046_the_window_end_with_no_backup_queues_pick_lapsed_once(pool: PgPool) {
    let (job, pick, task, due) = normal_window(&pool, false).await;
    let mut db = pool.acquire().await.expect("db");
    for _ in 0..2 {
        run_confirm_ends_at(pool.clone(), claimed(task.clone()), due)
            .await
            .expect("ends");
    }
    assert_eq!(events(&mut db, pick, "pick_lapsed").await.len(), 1);
    assert!(events(&mut db, pick, "pick_moved").await.is_empty());
    let row = current(&mut db, job).await.expect("job");
    assert_eq!(row.status.code(), "quotes_ready");
    assert_eq!(
        events(&mut db, pick, "pick_lapsed").await,
        vec![i64::from(row.version)]
    );
}

/// PICK-044, PICK-046, PICK-047: an emergency pick's 60 minutes lapse it whichever of `jobs`' timer
/// (`jobs_first`) and the window task runs first, on a worker `late` minutes past the due time: a
/// backup move whose pick starts at the worker's now, or one PICK-046 event (D-4187).
async fn emergency_lapses(pool: PgPool, (backup, jobs_first): (bool, bool), late: i64) {
    let mut db = pool.acquire().await.expect("db");
    let (job, pick) = picked(&mut db, (true, backup), ts(DAY)).await;
    let sent_at = ts(DAY) + SignedDuration::from_mins(5);
    let none = notice_sent(&mut db, pick, sent_at, ConfirmSettings::default()).await;
    assert_eq!(none, Ok(None), "no Accept by on an emergency (PICK-044)");
    let ends = window_ends(&mut db).await;
    assert_eq!(ends.len(), 1, "one window end");
    let due = ends[0].1;
    assert_eq!(
        due,
        ts(DAY) + SignedDuration::from_mins(60),
        "jobs' 60 minutes"
    );
    if jobs_first {
        let back = (JobStatus::AwaitingConfirm, JobStatus::QuotesReady);
        let moved = move_to(&mut db, job, back, due, Actor::System).await;
        assert!(
            moved.expect("move").applied().is_some(),
            "jobs' timer fired"
        );
    }
    let now = due + SignedDuration::from_mins(late);
    let clock = Arc::new(ManualClock::new(now));
    run_due(&pool, &registry(clock.clone()), &clock).await;
    assert_eq!(pick_status(&mut db, pick).await, "expired");
    let moved = events(&mut db, pick, "pick_moved").await.len();
    let lapsed = events(&mut db, pick, "pick_lapsed").await.len();
    assert_eq!((moved, lapsed), if backup { (1, 0) } else { (0, 1) });
    let want = if backup {
        vec![(business(2), now)]
    } else {
        vec![]
    };
    assert_eq!(waiting(&mut db, job).await, want);
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_044_an_emergency_lapses_to_the_backup_after_jobs_timer(pool: PgPool) {
    emergency_lapses(pool, (true, true), 0).await;
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_044_an_emergency_lapses_to_the_backup_by_its_window_task(pool: PgPool) {
    emergency_lapses(pool, (true, false), 0).await;
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_046_an_emergency_with_no_backup_lapses_after_jobs_timer(pool: PgPool) {
    emergency_lapses(pool, (false, true), 0).await;
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_046_an_emergency_with_no_backup_lapses_by_its_window_task(pool: PgPool) {
    emergency_lapses(pool, (false, false), 0).await;
}

/// PICK-044, PICK-047, D-4187: on an emergency job whose window task runs before `jobs`' timer on a
/// worker 20 minutes late, the backup's pick and 60 minutes start at the worker's now, never at
/// the past due time.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_044_a_late_window_task_starts_the_emergency_backup_now(pool: PgPool) {
    emergency_lapses(pool, (true, false), 20).await;
}

/// PICK-019, PICK-047: a notice sent after `jobs`' backstop returned the job answers `MovedOn` and
/// lapses the pick: it expires, the backup's pick starts at the send, and one `pick_moved` event
/// (no `pick_lapsed`) is queued at the job's version after the move.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_047_a_notice_after_the_backstop_moves_to_the_backup(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let (job, pick) = picked(&mut db, (false, true), ts(DAY)).await;
    let back = (JobStatus::AwaitingConfirm, JobStatus::QuotesReady);
    let moved = move_to(&mut db, job, back, ts(DAY), Actor::System).await;
    assert!(
        moved.expect("move").applied().is_some(),
        "the backstop fired"
    );
    let late = ts(DAY) + SignedDuration::from_mins(5);
    let sent_late = notice_sent(&mut db, pick, late, ConfirmSettings::default()).await;
    assert_eq!(sent_late, Err(PicksError::MovedOn));
    assert_eq!(pick_status(&mut db, pick).await, "expired");
    assert_eq!(waiting(&mut db, job).await, vec![(business(2), late)]);
    let version = i64::from(current(&mut db, job).await.expect("job").version);
    assert_eq!(events(&mut db, pick, "pick_moved").await, vec![version]);
    assert!(events(&mut db, pick, "pick_lapsed").await.is_empty());
}

/// PICK-019, PICK-047: a normal pick lapses the same when `jobs`' backstop returned the job first,
/// and a late worker starts the backup's window at its own now, never in the past.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_047_a_late_worker_after_the_backstop_starts_the_backup_now(pool: PgPool) {
    let (job, pick, _, due) = normal_window(&pool, true).await;
    let mut db = pool.acquire().await.expect("db");
    let back = (JobStatus::AwaitingConfirm, JobStatus::QuotesReady);
    let moved = move_to(&mut db, job, back, due, Actor::System).await;
    assert!(
        moved.expect("move").applied().is_some(),
        "the backstop fired"
    );
    let late = due + SignedDuration::from_hours(2);
    let clock = Arc::new(ManualClock::new(late));
    run_due(&pool, &registry(clock.clone()), &clock).await;
    assert_eq!(pick_status(&mut db, pick).await, "expired");
    assert_eq!(waiting(&mut db, job).await, vec![(business(2), late)]);
    let row = current(&mut db, job).await.expect("job");
    assert_eq!(row.status.code(), "awaiting_confirm");
    assert_eq!(row.status_at, late, "the backup's window starts now");
    assert_eq!(events(&mut db, pick, "pick_moved").await.len(), 1);
}

/// PICK-047, CON-028: an opted-out homeowner gets no text, while the pick still moves.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_047_an_opted_out_homeowner_gets_nothing_at_the_window_end(pool: PgPool) {
    let (job, _, _, due) = normal_window(&pool, true).await;
    let mut db = pool.acquire().await.expect("db");
    let channel = (RevokeChannel::Portal, "portal");
    suppress(&mut db, &vault(), PHONE, channel, ts(DAY))
        .await
        .expect("suppress");
    let clock = Arc::new(ManualClock::new(due));
    run_due(&pool, &registry(clock.clone()), &clock).await;
    assert_eq!(waiting(&mut db, job).await, vec![(business(2), due)]);
    assert!(
        sent(&mut db, "pick_moved").await.is_empty(),
        "no text after an opt-out"
    );
    assert!(sent(&mut db, "pick_lapsed").await.is_empty());
}

/// PICK-019: the end of a window that restarted later does nothing; the pick keeps waiting.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_019_a_restarted_window_end_does_nothing(pool: PgPool) {
    let (job, pick, first, due) = normal_window(&pool, true).await;
    let mut db = pool.acquire().await.expect("db");
    let later = ts(DAY) + SignedDuration::from_mins(30);
    let again = notice_sent(&mut db, pick, later, ConfirmSettings::default()).await;
    assert!(
        matches!(again, Ok(Some(end)) if end > due),
        "the window restarted"
    );
    run_confirm_ends_at(pool.clone(), claimed(first), due)
        .await
        .expect("stale");
    assert_eq!(pick_status(&mut db, pick).await, "awaiting_confirm");
    assert_eq!(waiting(&mut db, job).await, vec![(business(1), ts(DAY))]);
    assert!(events(&mut db, pick, "pick_moved").await.is_empty());
}
