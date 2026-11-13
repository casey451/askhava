//! TD-SEC-54 (SEC-004, D-2207) and TD-SEC-58 (SEC-014, SEC-040, D-2306), T436: the daily
//! `portals.session_sweep` deletes sessions expired more than a day earlier, and the daily
//! `core.rate_limit_prune` deletes rate-limit windows older than the longest window plus a day,
//! in batches; each is queued once per day and a repeat deletes nothing more.
//!
//! Both run through `askhava_server::worker_registry`, built from dev stand-in settings over a
//! `ManualClock`, so each case runs the production worker's own registration. The layer table lets
//! a test crate depend on modules but not on `askhava-server` (D-736), so the composition root is
//! compiled in place, as `purge_sweeps.rs` does (D-2853's exception, logged for this file in
//! D-3481). Every session and bucket is fake: random actor IDs and documentation addresses.

use std::path::Path;
use std::sync::Arc;

use askhava_core::api::{
    Clock, Lease, ManualClock, NewTask, RATE_LIMIT_PRUNE_BATCH, RATE_LIMIT_PRUNE_KIND, Registry,
    Settings, WorkerId, claim, enqueue, prune_rate_limit_batch, rate_limit_prune_cutoff,
    schedule_rate_limit_prune,
};
use askhava_picks::api::ConfirmSettings;
use askhava_portals::api::{SESSION_SWEEP_KIND, schedule_session_sweep, session_sweep_batch};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sqlx::PgPool;

/// The composition root, `app/server/src/lib.rs`, with its modules, compiled into this test crate.
#[path = r"../../../app/server/src/lib.rs"]
#[allow(dead_code, reason = "only the worker registry is called here")]
mod server;
use server::*;

const DAY: SignedDuration = SignedDuration::from_hours(24);
const HOUR: SignedDuration = SignedDuration::from_hours(1);
/// Midnight UTC; the sweeps run one day later.
const START: i64 = 1_790_035_200;

/// The registry the `worker` binary runs, from dev stand-in settings, on `clock`.
fn registry(clock: Arc<dyn Clock>, name: &str) -> Registry {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("t436-{name}"));
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
        (
            "ASKHAVA_ID_SECRET",
            "test-only-id-secret-for-t436-not-a-real-key".into(),
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    let settings = Settings::from_pairs(pairs.clone()).expect("settings");
    let stream =
        offdb_stream::OffDbStream::from_settings(&settings, pairs.clone(), clock).expect("stream");
    worker_registry(&settings, stream, (None, ConfirmSettings::default()), pairs).expect("registry")
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
            let done = registry.run(pool, clock, owner, task, Lease::WORKER).await;
            done.expect("run");
        }
    }
}

/// Seeds `kind` at `START` twice through `seed`, advances a day and runs the worker: one task a
/// day, and the run queued the next day's (the "once per day" half of both entries).
async fn seed_and_run(
    pool: &PgPool,
    kind: &'static str,
    name: &str,
) -> (Registry, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(
        Timestamp::from_second(START).expect("start"),
    ));
    for _ in 0..2 {
        let mut conn = pool.acquire().await.expect("conn");
        match kind {
            SESSION_SWEEP_KIND => schedule_session_sweep(&mut conn, clock.now()).await,
            _ => schedule_rate_limit_prune(&mut conn, clock.now()).await,
        }
        .expect("seed");
    }
    assert_eq!(pending(pool, kind).await, 1, "seeded once per day");
    clock.advance(DAY);
    let registry = registry(clock.clone(), name);
    run_due(pool, &registry, &clock).await;
    assert_eq!(pending(pool, kind).await, 1, "tomorrow's run queued once");
    (registry, clock)
}

/// Runs `kind` again at the same instant (a repeat), as a fresh task with no dedupe key.
async fn repeat(pool: &PgPool, registry: &Registry, clock: &ManualClock, kind: &'static str) {
    let mut conn = pool.acquire().await.expect("conn");
    enqueue(&mut conn, &NewTask::new(kind, json!({}), clock.now()))
        .await
        .expect("repeat");
    drop(conn);
    run_due(pool, registry, clock).await;
    assert_eq!(
        pending(pool, kind).await,
        1,
        "a repeat queues no second run"
    );
}

async fn pending(pool: &PgPool, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM core.task WHERE kind = $1 AND done_at IS NULL")
        .bind(kind)
        .fetch_one(pool)
        .await
        .expect("pending")
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count")
}

async fn session(pool: &PgPool, expires: Timestamp) {
    sqlx::query(
        "INSERT INTO portals.session (token_sha256, realm, actor_id, created_at, expires_at) \
         VALUES (sha256(gen_random_uuid()::text::bytea), 'business', gen_random_uuid(), $1 - interval '1 hour', $1)",
    )
    .bind(jiff_sqlx::Timestamp::from(expires))
    .execute(pool)
    .await
    .expect("session");
}

async fn window(pool: &PgPool, bucket: &str, start: Timestamp) {
    sqlx::query("INSERT INTO core.rate_limit (bucket, window_start, hits) VALUES ($1, $2, 1)")
        .bind(bucket)
        .bind(jiff_sqlx::Timestamp::from(start))
        .execute(pool)
        .await
        .expect("window");
}

/// TD-SEC-54 (SEC-004, D-2207).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_54_the_daily_sweep_deletes_sessions_expired_over_a_day(pool: PgPool) {
    let run = Timestamp::from_second(START).expect("start") + DAY;
    session(&pool, run + HOUR).await;
    session(&pool, run - HOUR * 12).await;
    for _ in 0..3 {
        session(&pool, run - DAY * 2).await;
    }
    let before = run - DAY;
    assert_eq!(
        session_sweep_batch(&pool, before, 2).await,
        Ok(2),
        "a batch"
    );
    let (registry, clock) = seed_and_run(&pool, SESSION_SWEEP_KIND, "sec54").await;
    let left = "SELECT count(*) FROM portals.session";
    assert_eq!(
        count(&pool, left).await,
        2,
        "live and recently expired kept"
    );
    repeat(&pool, &registry, &clock, SESSION_SWEEP_KIND).await;
    assert_eq!(count(&pool, left).await, 2, "a repeat deletes nothing more");
}

/// TD-SEC-58 (SEC-014, SEC-040, D-2306).
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn td_sec_58_the_daily_prune_deletes_windows_past_the_longest_plus_a_day(pool: PgPool) {
    let run = Timestamp::from_second(START).expect("start") + DAY;
    let cutoff = rate_limit_prune_cutoff(run);
    assert_eq!(
        cutoff,
        run - DAY * 2,
        "the longest window (a day) plus a day"
    );
    assert_eq!(RATE_LIMIT_PRUNE_BATCH, 5_000);
    for n in 1..=5 {
        window(&pool, &format!("test:ip:192.0.2.{n}"), cutoff - HOUR).await;
    }
    window(&pool, "test:ip:198.51.100.1", cutoff + HOUR).await;
    window(&pool, "test:all", run - HOUR).await;
    assert_eq!(
        prune_rate_limit_batch(&pool, cutoff, 2).await,
        Ok(2),
        "a batch"
    );
    let (registry, clock) = seed_and_run(&pool, RATE_LIMIT_PRUNE_KIND, "sec58").await;
    let left = "SELECT count(*) FROM core.rate_limit";
    assert_eq!(count(&pool, left).await, 2, "newer windows kept");
    repeat(&pool, &registry, &clock, RATE_LIMIT_PRUNE_KIND).await;
    assert_eq!(count(&pool, left).await, 2, "a repeat deletes nothing more");
    let worker = include_str!("../../../app/server/src/bin/worker.rs");
    assert!(
        worker.contains("schedule_rate_limit_prune(&mut conn, now)"),
        "seeded at start"
    );
    assert!(
        worker.contains("schedule_session_sweep(&mut conn, now)"),
        "seeded at start"
    );
}
