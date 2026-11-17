//! PICK-047's "your backup has your pick now" text on every path that moves a pick, through the
//! production worker registry (T471; PICK-047, PICK-046, PICK-050, BIZ-051, FEE-041, QH-001,
//! QH-004, CON-028; D-400, D-446, D-3579, D-3722, D-3750).
//!
//! The registry is `askhava_server::worker_registry` from dev stand-in settings, with the job-text
//! keys, on a manual clock; the composition root is compiled in place as `pick_ended.rs` does
//! (D-2853's exception, D-3569). A homeowner opens a request and a plumbing job with three quotes
//! from named businesses; quote 1 is picked on the page with quote 2 as her bound backup (or none),
//! and the pick ends by "Can't take it" (`cant_take`), the business leaving (`end_for_leaving`) or a
//! `payments.pick_ended` event. Every due task then runs through `Registry::run` and the outbox and
//! the queue are read. Every number (555-01xx), name, ID and word is fake.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_core::api::{
    Clock, IdSecret, Lease, ManualClock, NewTask, Registry, Settings, WorkerId, brand, claim,
    enqueue,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{JobStart, JobTextKeys, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::{JobStatus, TEXT_KIND, move_to, record_job_facts};
use askhava_messaging::api::{RevokeChannel, suppress};
use askhava_ops::api::Actor;
use askhava_payments::api::PICK_ENDED_KIND;
use askhava_picks::api::{
    Browser, ConfirmSettings, PagePick, bind_backup, cant_take, choose_backup, end_for_leaving,
    page_pick,
};
use askhava_privacy::api::{NewContact, Vault, store_contact};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(dead_code, reason = "only the worker registry is called here")]
mod server;
use server::*;

const ID_SECRET: &str = "test-only-id-secret-for-t471-not-a-real-key";
const PHONE: &str = "+19285550172";
/// 10:00 a.m. Arizona; 10:40 p.m. Arizona; 8:00 a.m. Arizona the next morning.
const DAY: &str = "2026-11-02T17:00:00Z";
const NIGHT: &str = "2026-11-03T05:40:00Z";
const EIGHT: &str = "2026-11-03T15:00:00Z";
const NAMES: [&str; 3] = [
    "Sample Drain Co A",
    "Sample Drain Co B",
    "Sample Drain Co C",
];
const BROWSER: Browser = [4; 32];

/// How the pick of quote 1 ends.
#[derive(Clone, Copy, Debug)]
enum End {
    /// PICK-050: its business pressed "Can't take it".
    CantTake,
    /// BIZ-051: its business left Laftly.
    Leaving,
    /// FEE-041: `payments` ended it (`payments.pick_ended`).
    Payments,
}

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn business(n: usize) -> Uuid {
    Uuid::from_u128(0x471_0000 + u128::try_from(n).expect("small"))
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x58), &key(0x68), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from(ID_SECRET.to_owned())
}

/// The production registry from dev stand-in settings with the job-text keys, on `clock`.
fn registry(clock: Arc<dyn Clock>) -> Registry {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t471-{}-{n}", std::process::id()));
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
            registry
                .run(pool, clock, owner, task, Lease::WORKER)
                .await
                .expect("run");
        }
    }
}

/// Three named businesses, each with a confirmed $190 quote on `job`, in arrival order.
async fn quoted(db: &mut PgConnection, job: Uuid, at: Timestamp) {
    for (i, name) in NAMES.iter().enumerate() {
        let id = business(i + 1);
        sqlx::query(
            "INSERT INTO businesses.business (id, status, created_at, status_changed_at) \
             VALUES ($1, 'active', now(), now()) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .execute(&mut *db)
        .await
        .expect("business");
        sqlx::query(
            "INSERT INTO businesses.business_name (business_id, business_name, changed_at) \
             VALUES ($1, $2, now()) ON CONFLICT DO NOTHING",
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

/// A homeowner texts in (consent text and record), her job (an emergency when `emergency`) walks
/// to Quotes ready with three quotes; returns the job.
async fn quotes_ready(db: &mut PgConnection, emergency: bool, at: Timestamp) -> Uuid {
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
    walk_to_quotes_ready(db, job, at).await;
    job
}

/// Stores `job`'s facts and walks it from Intake to Quotes ready with its three quotes.
async fn walk_to_quotes_ready(db: &mut PgConnection, job: Uuid, at: Timestamp) {
    let types = ["leak repair".to_owned()];
    record_job_facts(db, job, "plumbing_repair", &types)
        .await
        .expect("job facts");
    for step in [
        (JobStatus::Intake, JobStatus::Open),
        (JobStatus::Open, JobStatus::QuotesReady),
    ] {
        let moved = move_to(db, job, step, at, Actor::System).await;
        assert!(moved.expect("move").applied().is_some(), "{step:?}");
    }
    quoted(db, job, at).await;
}

/// The page pick of quote 1 at `at` with quote 2 as her bound backup when `backup`; the queue is
/// then emptied, so only what the end queues runs.
async fn picked(db: &mut PgConnection, job: Uuid, backup: bool, at: Timestamp) -> Uuid {
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
        .expect("fixture: an empty queue");
    pick
}

/// Ends `pick` of `job` by `end` at `at`, as its caller does; `again` is a second run.
async fn end_pick(
    db: &mut PgConnection,
    end: End,
    (job, pick): (Uuid, Uuid),
    at: Timestamp,
    again: bool,
) {
    let mut tx = db.begin().await.expect("tx");
    match end {
        End::CantTake => {
            let by = (Actor::Business(business(1)), at);
            cant_take(&mut tx, (pick, job), false, by)
                .await
                .expect("cant take");
        }
        End::Leaving => {
            let by = (Actor::Business(business(1)), at);
            end_for_leaving(&mut tx, (pick, job), false, by)
                .await
                .expect("leaving");
        }
        End::Payments => {
            let payload = json!({"pick": pick, "business": business(1), "job": job,
                "reason": "fee_limit"});
            let mut task = NewTask::new(PICK_ENDED_KIND, payload, at);
            task.dedupe_key = Some(format!("{pick}.{again}"));
            enqueue(&mut tx, &task).await.expect("event");
        }
    }
    tx.commit().await.expect("commit");
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

/// The job versions of the `template` events queued for ended pick `pick` (held copies apart).
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

async fn job_version(db: &mut PgConnection, job: Uuid) -> i64 {
    sqlx::query_scalar("SELECT version::bigint FROM jobs.job WHERE id = $1")
        .bind(job)
        .fetch_one(db)
        .await
        .expect("job")
}

/// The businesses whose pick on `job` waits for a Confirm.
async fn waiting(db: &mut PgConnection, job: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar(
        "SELECT business_id FROM picks.pick WHERE job_id = $1 AND status = 'awaiting_confirm'",
    )
    .bind(job)
    .fetch_all(db)
    .await
    .expect("picks")
}

/// PICK-047's English text naming the ended business and her backup.
fn moved_text() -> String {
    let short = brand().fill("{brand_short}", false).expect("brand");
    format!(
        "{short}: {} couldn't take it, so your backup, {}, has your pick now. We'll text you when they accept.",
        NAMES[0], NAMES[1]
    )
}

/// PICK-047 on `end` by day: the pick moves once to her backup, one `pick_moved` event is queued
/// at the job's version after the move, the sender renders the English text naming both
/// businesses, and a rerun queues nothing more.
async fn moves_once(pool: PgPool, end: End) {
    let mut db = pool.acquire().await.expect("db");
    let job = quotes_ready(&mut db, false, ts(DAY)).await;
    let pick = picked(&mut db, job, true, ts(DAY)).await;
    let clock = Arc::new(ManualClock::new(ts(DAY)));
    let registry = registry(clock.clone());
    end_pick(&mut db, end, (job, pick), ts(DAY), false).await;
    run_due(&pool, &registry, &clock).await;
    assert_eq!(waiting(&mut db, job).await, vec![business(2)], "{end:?}");
    let versions = events(&mut db, pick, "pick_moved").await;
    assert_eq!(versions, vec![job_version(&mut db, job).await], "{end:?}");
    assert_eq!(
        sent(&mut db, "pick_moved").await,
        vec![moved_text()],
        "{end:?}"
    );
    assert!(
        events(&mut db, pick, "pick_lapsed").await.is_empty(),
        "{end:?}"
    );
    end_pick(&mut db, end, (job, pick), ts(DAY), true).await;
    run_due(&pool, &registry, &clock).await;
    assert_eq!(
        events(&mut db, pick, "pick_moved").await.len(),
        1,
        "{end:?}: a rerun"
    );
    assert_eq!(
        sent(&mut db, "pick_moved").await.len(),
        1,
        "{end:?}: a rerun"
    );
    assert_eq!(
        waiting(&mut db, job).await,
        vec![business(2)],
        "{end:?}: moved once"
    );
}

/// PICK-046 on `end` with no backup: one `pick_lapsed` event and no `pick_moved` one; a rerun
/// queues nothing more.
async fn lapses_once(pool: PgPool, end: End) {
    let mut db = pool.acquire().await.expect("db");
    let job = quotes_ready(&mut db, false, ts(DAY)).await;
    let pick = picked(&mut db, job, false, ts(DAY)).await;
    let clock = Arc::new(ManualClock::new(ts(DAY)));
    let registry = registry(clock.clone());
    end_pick(&mut db, end, (job, pick), ts(DAY), false).await;
    run_due(&pool, &registry, &clock).await;
    end_pick(&mut db, end, (job, pick), ts(DAY), true).await;
    run_due(&pool, &registry, &clock).await;
    assert_eq!(
        events(&mut db, pick, "pick_lapsed").await.len(),
        1,
        "{end:?}"
    );
    assert_eq!(sent(&mut db, "pick_lapsed").await.len(), 1, "{end:?}");
    assert!(
        events(&mut db, pick, "pick_moved").await.is_empty(),
        "{end:?}"
    );
    assert!(waiting(&mut db, job).await.is_empty(), "{end:?}");
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_047_cant_take_moves_to_the_backup_with_her_text(pool: PgPool) {
    moves_once(pool, End::CantTake).await;
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_047_the_business_leaving_moves_to_the_backup_with_her_text(pool: PgPool) {
    moves_once(pool, End::Leaving).await;
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_047_a_payments_end_moves_to_the_backup_with_her_text(pool: PgPool) {
    moves_once(pool, End::Payments).await;
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_046_cant_take_with_no_backup_queues_pick_lapsed_once(pool: PgPool) {
    lapses_once(pool, End::CantTake).await;
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_046_the_business_leaving_with_no_backup_queues_pick_lapsed_once(pool: PgPool) {
    lapses_once(pool, End::Leaving).await;
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_046_a_payments_end_with_no_backup_queues_pick_lapsed_once(pool: PgPool) {
    lapses_once(pool, End::Payments).await;
}

/// PICK-047 (TXT-039): a Spanish request gets the Spanish text naming both businesses.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_047_a_spanish_request_gets_the_spanish_text(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = quotes_ready(&mut db, false, ts(DAY)).await;
    sqlx::query("UPDATE intake.conversation SET language = 'es'")
        .execute(&mut *db)
        .await
        .expect("fixture: a Spanish request");
    let pick = picked(&mut db, job, true, ts(DAY)).await;
    let clock = Arc::new(ManualClock::new(ts(DAY)));
    let registry = registry(clock.clone());
    end_pick(&mut db, End::CantTake, (job, pick), ts(DAY), false).await;
    run_due(&pool, &registry, &clock).await;
    let short = brand().fill("{brand_short}", true).expect("brand");
    let want = format!(
        "{short}: {} no pudo tomar el trabajo, así que pasamos su elección a su segunda opción, {}. Le avisaremos por texto cuando acepte.",
        NAMES[0], NAMES[1]
    );
    assert_eq!(sent(&mut db, "pick_moved").await, vec![want]);
}

/// PICK-047, QH-001: a move at night on a normal job holds her text to 8 a.m.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_047_a_night_move_waits_for_8_am_on_a_normal_job(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = quotes_ready(&mut db, false, ts(DAY)).await;
    let pick = picked(&mut db, job, true, ts(NIGHT)).await;
    let clock = Arc::new(ManualClock::new(ts(NIGHT)));
    let registry = registry(clock.clone());
    end_pick(&mut db, End::CantTake, (job, pick), ts(NIGHT), false).await;
    run_due(&pool, &registry, &clock).await;
    assert_eq!(waiting(&mut db, job).await, vec![business(2)]);
    assert!(
        sent(&mut db, "pick_moved").await.is_empty(),
        "nothing at night"
    );
    clock.set(ts(EIGHT) - SignedDuration::from_secs(1));
    run_due(&pool, &registry, &clock).await;
    assert!(
        sent(&mut db, "pick_moved").await.is_empty(),
        "nothing at 7:59:59"
    );
    clock.set(ts(EIGHT));
    run_due(&pool, &registry, &clock).await;
    assert_eq!(sent(&mut db, "pick_moved").await, vec![moved_text()]);
}

/// PICK-047, QH-004: on a job she chose as an emergency her text goes at once at night.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_047_a_night_move_texts_at_once_on_an_emergency_job(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = quotes_ready(&mut db, true, ts(DAY)).await;
    let pick = picked(&mut db, job, true, ts(NIGHT)).await;
    let clock = Arc::new(ManualClock::new(ts(NIGHT)));
    let registry = registry(clock.clone());
    end_pick(&mut db, End::Leaving, (job, pick), ts(NIGHT), false).await;
    run_due(&pool, &registry, &clock).await;
    assert_eq!(sent(&mut db, "pick_moved").await, vec![moved_text()]);
}

/// PICK-047, CON-028: an opted-out homeowner gets no text, while the pick still moves.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn pick_047_an_opted_out_homeowner_gets_nothing(pool: PgPool) {
    let mut db = pool.acquire().await.expect("db");
    let job = quotes_ready(&mut db, false, ts(DAY)).await;
    let pick = picked(&mut db, job, true, ts(DAY)).await;
    let channel = (RevokeChannel::Portal, "portal");
    suppress(&mut db, &vault(), PHONE, channel, ts(DAY))
        .await
        .expect("suppress");
    let clock = Arc::new(ManualClock::new(ts(DAY)));
    let registry = registry(clock.clone());
    end_pick(&mut db, End::Payments, (job, pick), ts(DAY), false).await;
    run_due(&pool, &registry, &clock).await;
    assert_eq!(waiting(&mut db, job).await, vec![business(2)]);
    assert!(
        sent(&mut db, "pick_moved").await.is_empty(),
        "no text after an opt-out"
    );
    assert!(
        sent(&mut db, "pick_lapsed").await.is_empty(),
        "and no PICK-046 text"
    );
}
