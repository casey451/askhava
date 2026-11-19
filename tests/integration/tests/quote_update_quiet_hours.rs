//! The quote-count update text and quiet hours, through the production worker registry (T708;
//! TXT-025, QH-001, QH-004, QH-007, TXT-017; D-012, D-158, D-1190, D-4419).
//!
//! The registry is `askhava_server::worker_registry` from dev stand-in settings (the stand-in
//! Twilio), with the job-text keys, on a manual clock, as `quote_update_texts.rs` builds it. A
//! homeowner with a 928 (Arizona) number opens a request and a plumbing job; the first quote moves
//! it to Quotes ready and the worker queues or holds the first-quote text, whose queue time the
//! fixture sets to the instant it was queued (the time TXT-017 measures from while no delivery is
//! recorded). Each later quote is a confirmed row and a `quotes.count_update` event, as `quotes`
//! queues them. Every number (555-01xx), name, ID and word is fake.

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

const ID_SECRET: &str = "test-only-id-secret-for-t708-not-a-real-key";
const PHONE: &str = "+19285550171";
// Arizona times (UTC-7): Nov 2 at 10:00 a.m., 9:30, 10:00, 10:10 and 11:00 p.m.; Nov 3 at 8:00
// a.m., the window's opening.
const DAY: &str = "2026-11-02T17:00:00Z";
const NINE_THIRTY: &str = "2026-11-03T04:30:00Z";
const NIGHT: &str = "2026-11-03T05:00:00Z";
const TEN_TEN: &str = "2026-11-03T05:10:00Z";
const ELEVEN: &str = "2026-11-03T06:00:00Z";
const OPEN: &str = "2026-11-03T15:00:00Z";

fn ts(s: &str) -> Timestamp {
    s.parse().expect("instant")
}

fn secs(n: i64) -> SignedDuration {
    SignedDuration::from_secs(n)
}

fn vault() -> Vault {
    let key = |b: u8| SecretString::from(format!("1:{}", STANDARD.encode([b; 32])));
    let url = SecretString::from("postgres://askhava_app@db.invalid/askhava".to_owned());
    Vault::new(&key(0x5a), &key(0x6a), &url).expect("vault")
}

fn secret() -> SecretString {
    SecretString::from(ID_SECRET.to_owned())
}

/// The production registry from dev stand-in settings with the job-text keys, on `clock`.
fn registry(clock: Arc<dyn Clock>) -> Registry {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t708-{}-{n}", std::process::id()));
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

/// Sets the clock to `at` and runs every task due then through `registry`, until none is.
async fn run_at(pool: &PgPool, registry: &Registry, clock: &ManualClock, at: Timestamp) {
    clock.set(at);
    let owner = WorkerId::new(pool).await.expect("worker");
    loop {
        let tasks = claim(pool, owner, at, secs(60), 50).await.expect("claim");
        if tasks.is_empty() {
            return;
        }
        for task in tasks {
            let run = registry.run(pool, clock, owner, task, Lease::WORKER);
            run.await.expect("run");
        }
    }
}

/// Business `n`'s confirmed quote on `job`, received at `at`.
async fn quote(db: &mut PgConnection, job: Uuid, n: u128, at: Timestamp) {
    sqlx::query(
        "INSERT INTO quotes.quote (job_id, business_id, format, amount_cents, \
         confirm_on_site, notes, original, ai_read, confirmed, received_at) \
         VALUES ($1, $2, 'firm', 21000, false, '', 'Sample wording', false, true, $3)",
    )
    .bind(job)
    .bind(Uuid::from_u128(0x708_0000 + n))
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

/// She texts in at `at` and gets the consent text; returns her request.
async fn request(db: &mut PgConnection, at: Timestamp) -> Uuid {
    let new = NewContact {
        name: None,
        phone: Some(SecretString::from(PHONE.to_owned())),
        email: None,
        address: None,
        onsite_contact: None,
    };
    let her = store_contact(db, &vault(), &new).await.expect("contact");
    let inbound = Inbound {
        words: "my kitchen sink is leaking under the cabinet",
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
    handled.expect("open").conversation.expect("request")
}

/// A manual clock at `at`, the registry on it, and her request and job (`emergency` as she chose)
/// opened then, moved to Quotes ready by one quote, and the worker run then (the first-quote text
/// queued by day, held at night).
async fn quotes_ready(
    pool: &PgPool,
    at: &str,
    emergency: bool,
) -> (Arc<ManualClock>, Registry, Uuid) {
    let at = ts(at);
    let clock = Arc::new(ManualClock::new(at));
    let registry = registry(clock.clone());
    let mut db = pool.acquire().await.expect("connection");
    let start = JobStart {
        conversation: request(&mut db, at).await,
        words: "my kitchen sink is leaking under the cabinet",
        emergency,
        trade: "plumbing",
        at,
    };
    let Ok(Opened::Created(job)) = begin_job(&mut db, &start).await else {
        panic!("a new job");
    };
    let open = (JobStatus::Intake, JobStatus::Open);
    let moved = move_to(&mut db, job, open, at, Actor::System);
    assert!(moved.await.expect("open").applied().is_some());
    quote(&mut db, job, 1, at).await;
    assert!(
        first_quote(&mut db, job, at)
            .await
            .expect("first quote")
            .applied()
            .is_some()
    );
    run_at(pool, &registry, &clock, at).await;
    queued_at(pool, at).await;
    (clock, registry, job)
}

/// Fixture: the first-quote text's queue time is `at`, the instant the worker queued it.
async fn queued_at(pool: &PgPool, at: Timestamp) {
    sqlx::query("UPDATE messaging.outbox SET created_at = $1 WHERE template_id = 'first_quote'")
        .bind(at.to_sqlx())
        .execute(pool)
        .await
        .expect("fixture: the first-quote text's queue time");
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

/// Asserts exactly one update text was queued, telling `quoted`.
async fn one_update(pool: &PgPool, quoted: u32) {
    let sent = texts(pool, "quote_update").await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].starts_with(&told(quoted)), "{}", sent[0]);
}

/// TXT-025, QH-001, D-158 (TD-QH-02's merge case): after a first-quote text by day, two quotes in
/// at 10 p.m. to a 928 number give nothing before 8:00:00 and one update telling 3 at 8:00:00.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn txt_025_two_night_quotes_give_one_update_at_eight_with_the_latest_count(pool: PgPool) {
    let (clock, registry, job) = quotes_ready(&pool, DAY, false).await;
    let mut db = pool.acquire().await.expect("connection");
    arrive(&mut db, job, 2, (ts(NIGHT), ts(NIGHT))).await;
    arrive(&mut db, job, 3, (ts(NIGHT), ts(NIGHT))).await;
    run_at(&pool, &registry, &clock, ts(NIGHT)).await;
    run_at(&pool, &registry, &clock, ts(OPEN) - secs(1)).await;
    assert!(
        texts(&pool, "quote_update").await.is_empty(),
        "held to 8:00"
    );
    run_at(&pool, &registry, &clock, ts(OPEN)).await;
    one_update(&pool, 3).await;
}

/// TXT-025, TXT-017: two held events for one job released at 8:00 and run on two connections at
/// once send one update with the latest count.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn txt_025_two_held_events_run_at_once_at_eight_send_one_text(pool: PgPool) {
    let (clock, registry, job) = quotes_ready(&pool, DAY, false).await;
    let mut db = pool.acquire().await.expect("connection");
    arrive(&mut db, job, 2, (ts(NIGHT), ts(NIGHT))).await;
    run_at(&pool, &registry, &clock, ts(NIGHT)).await;
    arrive(&mut db, job, 3, (ts(ELEVEN), ts(OPEN))).await;
    run_at(&pool, &registry, &clock, ts(OPEN) - secs(1)).await;
    clock.set(ts(OPEN));
    let owner = WorkerId::new(&pool).await.expect("worker");
    let tasks = claim(&pool, owner, ts(OPEN), secs(60), 50)
        .await
        .expect("claim");
    let kinds: Vec<&str> = tasks.iter().map(|t| t.kind.as_str()).collect();
    assert_eq!(kinds, [COUNT_UPDATE_KIND; 2], "the two held events are due");
    let [one, two]: [_; 2] = tasks.try_into().expect("two tasks");
    let (a, b) = tokio::join!(
        registry.run(&pool, &*clock, owner, one, Lease::WORKER),
        registry.run(&pool, &*clock, owner, two, Lease::WORKER),
    );
    a.expect("first run");
    b.expect("second run");
    one_update(&pool, 3).await;
}

/// QH-007: a job closed overnight while its update is held sends nothing at 8:00.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn qh_007_a_job_closed_overnight_sends_no_update(pool: PgPool) {
    let (clock, registry, job) = quotes_ready(&pool, DAY, false).await;
    let mut db = pool.acquire().await.expect("connection");
    arrive(&mut db, job, 2, (ts(NIGHT), ts(NIGHT))).await;
    run_at(&pool, &registry, &clock, ts(NIGHT)).await;
    let closed = (JobStatus::QuotesReady, JobStatus::Closed);
    let step = move_to(&mut db, job, closed, ts(ELEVEN), Actor::System).await;
    assert!(step.expect("close").applied().is_some());
    run_at(&pool, &registry, &clock, ts(OPEN)).await;
    let sent = texts(&pool, "quote_update").await;
    assert!(sent.is_empty(), "{sent:?}");
}

/// QH-004: an emergency job's update at 11 p.m. is queued at once, its outbox row exempt.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn qh_004_an_emergency_jobs_update_at_eleven_goes_at_once(pool: PgPool) {
    let (clock, registry, job) = quotes_ready(&pool, DAY, true).await;
    let mut db = pool.acquire().await.expect("connection");
    arrive(&mut db, job, 2, (ts(ELEVEN), ts(ELEVEN))).await;
    run_at(&pool, &registry, &clock, ts(ELEVEN)).await;
    one_update(&pool, 2).await;
    let exempt: bool = sqlx::query_scalar(
        "SELECT quiet_exempt_until = 'infinity'::timestamptz FROM messaging.outbox \
         WHERE template_id = 'quote_update'",
    )
    .fetch_one(&pool)
    .await
    .expect("exemption");
    assert!(exempt, "the row carries QH-004's exemption");
}

/// D-4419, TXT-017, D-1190: a first quote at 9:30 p.m. and a second at 10:10 p.m. give the
/// first-quote text at 8:00:00 (none before) and one update telling 2 three hours after that
/// text, none at 2:59:59.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn d_4419_a_second_quote_while_the_first_text_is_held_is_told_later(pool: PgPool) {
    let (clock, registry, job) = quotes_ready(&pool, NINE_THIRTY, false).await;
    assert!(
        texts(&pool, "first_quote").await.is_empty(),
        "held at 9:30 p.m."
    );
    let mut db = pool.acquire().await.expect("connection");
    arrive(&mut db, job, 2, (ts(TEN_TEN), ts(TEN_TEN))).await;
    run_at(&pool, &registry, &clock, ts(TEN_TEN)).await;
    run_at(&pool, &registry, &clock, ts(OPEN) - secs(1)).await;
    assert!(texts(&pool, "first_quote").await.is_empty(), "held to 8:00");
    run_at(&pool, &registry, &clock, ts(OPEN)).await;
    assert_eq!(texts(&pool, "first_quote").await.len(), 1);
    queued_at(&pool, ts(OPEN)).await;
    run_at(&pool, &registry, &clock, ts(OPEN) + secs(15 * 60)).await;
    run_at(&pool, &registry, &clock, ts(OPEN) + secs(3 * 3600 - 1)).await;
    assert!(
        texts(&pool, "quote_update").await.is_empty(),
        "held at 2:59:59"
    );
    run_at(&pool, &registry, &clock, ts(OPEN) + secs(3 * 3600)).await;
    one_update(&pool, 2).await;
}
