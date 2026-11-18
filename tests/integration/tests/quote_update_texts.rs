//! The quote-count update text through the production worker registry (T583; TXT-017, QH-007,
//! TXT-042, CON-028; D-012, D-1190, D-4052, D-4280, D-4419).
//!
//! The registry is `askhava_server::worker_registry` from dev stand-in settings (the stand-in
//! Twilio), with the job-text keys, on a manual clock; the composition root is compiled in place as
//! `pick_moved_text.rs` does (D-2853's exception). A homeowner opens a request and a plumbing job;
//! the first quote moves it to Quotes ready and the worker queues the first-quote text, whose
//! queue time the fixture sets to 10:00 Arizona (the delivery time TXT-017 measures from). Each
//! later quote is a confirmed row and a `quotes.count_update` event, as `quotes` queues them; every
//! due task then runs through `Registry::run` and the outbox is read. Every number (555-01xx),
//! name, ID and word is fake.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use askhava_core::api::{
    Clock, IdSecret, Lease, ManualClock, NewTask, Registry, Settings, WorkerId, brand, claim,
    enqueue,
};
use askhava_intake::api::rules::{Inbound, Settings as IntakeSettings};
use askhava_intake::api::{JobStart, JobTextKeys, Opened, TextIn, begin_job, receive_text};
use askhava_jobs::api::{JobStatus, first_quote, move_to};
use askhava_messaging::api::{RevokeChannel, suppress};
use askhava_ops::api::Actor;
use askhava_picks::api::ConfirmSettings;
use askhava_privacy::api::{NewContact, Vault, store_contact};
use askhava_quotes::api::COUNT_UPDATE_KIND;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(dead_code, reason = "only the worker registry is called here")]
mod server;
use server::*;

const ID_SECRET: &str = "test-only-id-secret-for-t583-not-a-real-key";
const PHONE: &str = "+19285550183";
/// 10:00 a.m. Arizona.
const DAY: &str = "2026-11-02T17:00:00Z";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

/// `DAY` plus `secs` seconds.
fn after(secs: i64) -> Timestamp {
    ts(DAY) + SignedDuration::from_secs(secs)
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
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t583-{}-{n}", std::process::id()));
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

/// Business `n`'s confirmed quote on `job`, received at `at`.
async fn quote(db: &mut PgConnection, job: Uuid, n: u128, at: Timestamp) {
    let business = Uuid::from_u128(0x583_0000 + n);
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
         confirm_on_site, notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 19000, false, '', 'Sample wording', false, true, $3)",
    )
    .bind(job)
    .bind(business)
    .bind(at.to_sqlx())
    .execute(&mut *db)
    .await
    .expect("quote");
}

/// Business `n`'s quote arrives at `at` and its count-update event is queued, due at `due`.
async fn arrive(db: &mut PgConnection, job: Uuid, n: u128, (at, due): (Timestamp, Timestamp)) {
    quote(db, job, n, at).await;
    let mut task = NewTask::new(COUNT_UPDATE_KIND, json!({ "job": job }), due);
    task.dedupe_key = Some(format!("{COUNT_UPDATE_KIND}.{job}.{n}"));
    enqueue(db, &task).await.expect("event");
}

/// She texts in at 10:00 and gets the consent text; returns her request.
async fn request(db: &mut PgConnection) -> Uuid {
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
        received_at: ts(DAY),
    };
    let handled = receive_text(db, &secret(), &msg, IntakeSettings::default()).await;
    handled.expect("open").conversation.expect("request")
}

/// Her request and Quotes ready job with one quote and the first-quote text queued at 10:00
/// through the worker; returns the job.
async fn first_quote_sent(pool: &PgPool, registry: &Registry, clock: &ManualClock) -> Uuid {
    let mut db = pool.acquire().await.expect("connection");
    let start = JobStart {
        conversation: request(&mut db).await,
        words: "my kitchen sink is leaking under the cabinet",
        emergency: false,
        trade: "plumbing",
        at: ts(DAY),
    };
    let Ok(Opened::Created(job)) = begin_job(&mut db, &start).await else {
        panic!("a new job");
    };
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(&mut db, job, open, ts(DAY), Actor::System).await;
    assert!(moved.expect("open").applied().is_some());
    quote(&mut db, job, 1, ts(DAY)).await;
    let moved = first_quote(&mut db, job, ts(DAY)).await;
    assert!(moved.expect("first quote").applied().is_some());
    run_due(pool, registry, clock).await;
    sqlx::query("UPDATE messaging.outbox SET created_at = $1 WHERE template_id = 'first_quote'")
        .bind(ts(DAY).to_sqlx())
        .execute(&mut *db)
        .await
        .expect("fixture: the first-quote text's queue time");
    assert_eq!(texts(pool, "first_quote").await.len(), 1);
    job
}

/// The bodies of the `template` texts queued.
async fn texts(pool: &PgPool, template: &str) -> Vec<String> {
    sqlx::query_scalar("SELECT body FROM messaging.outbox WHERE template_id = $1")
        .bind(template)
        .fetch_all(pool)
        .await
        .expect("outbox")
}

/// The update text's opening for `quoted` quotes on request 1001, from the brand file.
fn told(quoted: u32) -> String {
    let text = format!("{{brand_short}}: You now have {quoted} quotes for request 1001.");
    brand().fill(&text, false).expect("brand tokens")
}

/// TXT-017, D-012, D-1190 (TD-TXT-06's sender case): a second quote 10 minutes after the
/// first-quote text is held; nothing goes at 2:59:59 after that text and the update telling 2
/// goes at 3:00:00, once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn txt_017_second_quote_is_told_at_three_hours_not_at_2_59_59(pool: PgPool) {
    let clock = Arc::new(ManualClock::new(ts(DAY)));
    let registry = registry(clock.clone());
    let job = first_quote_sent(&pool, &registry, &clock).await;
    let mut db = pool.acquire().await.expect("connection");
    arrive(&mut db, job, 2, (after(600), after(600))).await;
    clock.set(after(600));
    run_due(&pool, &registry, &clock).await;
    clock.set(after(3 * 3600 - 1));
    run_due(&pool, &registry, &clock).await;
    assert!(
        texts(&pool, "quote_update").await.is_empty(),
        "held at 2:59:59"
    );
    clock.set(after(3 * 3600));
    run_due(&pool, &registry, &clock).await;
    let sent = texts(&pool, "quote_update").await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].starts_with(&told(2)), "{}", sent[0]);
}

/// TXT-017: two events for one job released at the same instant by day, run on two connections
/// at once, send one update with the latest count.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn txt_017_two_events_at_one_instant_send_one_text(pool: PgPool) {
    let clock = Arc::new(ManualClock::new(ts(DAY)));
    let registry = registry(clock.clone());
    let job = first_quote_sent(&pool, &registry, &clock).await;
    let mut db = pool.acquire().await.expect("connection");
    let release = after(3 * 3600);
    arrive(&mut db, job, 2, (after(600), release)).await;
    arrive(&mut db, job, 3, (after(900), release)).await;
    clock.set(release - SignedDuration::from_secs(1));
    run_due(&pool, &registry, &clock).await;
    clock.set(release);
    let owner = WorkerId::new(&pool).await.expect("worker");
    let tasks = claim(&pool, owner, release, SignedDuration::from_secs(60), 50)
        .await
        .expect("claim");
    let kinds: Vec<&str> = tasks.iter().map(|t| t.kind.as_str()).collect();
    assert_eq!(kinds, [COUNT_UPDATE_KIND; 2], "only the two events are due");
    let [one, two]: [_; 2] = tasks.try_into().expect("two tasks");
    let (a, b) = tokio::join!(
        registry.run(&pool, &*clock, owner, one, Lease::WORKER),
        registry.run(&pool, &*clock, owner, two, Lease::WORKER),
    );
    a.expect("first run");
    b.expect("second run");
    let sent = texts(&pool, "quote_update").await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].starts_with(&told(3)), "{}", sent[0]);
}

/// A second quote's update is held; then the job moves to `moved` (a close or a pick) or, with
/// `None`, her number opts out, and at 3:00:00 the held event runs and sends nothing.
async fn held_then(pool: PgPool, moved: Option<JobStatus>) {
    let clock = Arc::new(ManualClock::new(ts(DAY)));
    let registry = registry(clock.clone());
    let job = first_quote_sent(&pool, &registry, &clock).await;
    let mut db = pool.acquire().await.expect("connection");
    arrive(&mut db, job, 2, (after(600), after(600))).await;
    clock.set(after(600));
    run_due(&pool, &registry, &clock).await;
    let ready = JobStatus::QuotesReady;
    if let Some(to) = moved {
        let step = move_to(&mut db, job, (ready, to), after(700), Actor::System).await;
        assert!(step.expect("move").applied().is_some(), "{moved:?}");
    } else {
        let channel = (RevokeChannel::Portal, "portal");
        suppress(&mut db, &vault(), PHONE, channel, after(700))
            .await
            .expect("suppress");
    }
    clock.set(after(3 * 3600));
    run_due(&pool, &registry, &clock).await;
    let sent = texts(&pool, "quote_update").await;
    assert!(sent.is_empty(), "{moved:?}: {sent:?}");
}

/// QH-007: a job closed before its held event runs sends nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn qh_007_a_job_closed_while_held_sends_no_update(pool: PgPool) {
    held_then(pool, Some(JobStatus::Closed)).await;
}

/// TXT-042: no update follows a pick.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn txt_042_a_picked_job_sends_no_update(pool: PgPool) {
    held_then(pool, Some(JobStatus::AwaitingConfirm)).await;
}

/// CON-028: an opted-out number gets no update.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn con_028_an_opted_out_number_gets_no_update(pool: PgPool) {
    held_then(pool, None).await;
}
