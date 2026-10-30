//! The task queue and the rate-limit counters (JOB-006, SEC-040; TD-SEC-19, D-491).
//!
//! Proves on fresh databases that two workers, each its own pool as two processes would have,
//! run every one of 100 and of 1000 tasks exactly once; that an expired lease is reclaimed and the
//! old owner's finish, renew and fail are fenced out; that a second enqueue with one dedupe key is
//! refused; that a failing task backs off, then is dead-lettered with its fixed error; that an
//! unregistered kind, a released lease and a lease lost on the last attempt end as designed; that
//! the worker's own claim loop runs every task once, at most four at a time, and on shutdown
//! aborts what still runs and gives its leases back; and
//! that 20 concurrent hits from two pools on one bucket allow exactly 5, with the client IP taken
//! only from the edge header or, on a proxied hostname, from Cloudflare. All data is fake.

use std::net::IpAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use askhava_core::api::{
    Claimed, ClientIpPolicy, Clock, HandlerError, LEASE_EXPIRED, Lease, ManualClock, NO_HANDLER,
    NewTask, Outcome, RateDecision, Registry, SystemClock, TaskError, Worker, WorkerId, claim,
    enqueue, fail, finish, rate_limit_hit, reap_expired, release_leases, renew,
};
use axum::http::HeaderMap;
use jiff::{SignedDuration, Timestamp};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

const PROBE: &str = "probe.run";
const SLOW: &str = "probe.slow";
const FAILS: &str = "probe.fail";
const HANGS: &str = "probe.hang";
const PROBE_ERROR: &str = "probe asked to fail";
const LEASE: SignedDuration = SignedDuration::from_secs(30);

/// The test registry, built once and shared, as the worker holds `register_handlers()` (D-488).
fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = Registry::new();
        registry.register(PROBE, |pool, task| record(pool, task, Duration::ZERO));
        registry.register(SLOW, |pool, task| {
            record(pool, task, Duration::from_millis(20))
        });
        registry.register(FAILS, |_, _| async { Err(HandlerError(PROBE_ERROR)) });
        registry.register(HANGS, |_, _| async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(())
        });
        registry
    })
}

/// The probe handler: waits, then records that it ran this task.
async fn record(pool: PgPool, task: Claimed, wait: Duration) -> Result<(), HandlerError> {
    tokio::time::sleep(wait).await;
    sqlx::query("INSERT INTO core.probe_run (task_id) VALUES ($1)")
        .bind(task.id)
        .execute(&pool)
        .await
        .map(|_| ())
        .map_err(|_| HandlerError("probe could not record"))
}

async fn probe_table(pool: &PgPool) {
    sqlx::query("CREATE TABLE core.probe_run (task_id bigint NOT NULL)")
        .execute(pool)
        .await
        .expect("create the probe table");
}

async fn scalar(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.expect(sql)
}

async fn enqueue_many(pool: &PgPool, kind: &'static str, count: usize) {
    let now = Timestamp::now();
    for n in 0..count {
        let task = NewTask::new(kind, serde_json::json!({ "probe": n }), now);
        enqueue(&mut pool.acquire().await.expect("connection"), &task)
            .await
            .expect("enqueue")
            .expect("new task");
    }
}

/// One worker: `lanes` loops that claim one task at a time until no task is open.
async fn worker(pool: PgPool, lanes: usize) {
    let owner = WorkerId::new(&pool).await.expect("worker id");
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..lanes {
        set.spawn(lane(pool.clone(), owner));
    }
    set.join_all().await;
}

/// One lane of a worker: claim one task, run it, repeat until no task is open.
async fn lane(pool: PgPool, owner: WorkerId) {
    loop {
        let got = claim(&pool, owner, Timestamp::now(), LEASE, 1).await;
        let Some(task) = got.expect("claim").into_iter().next() else {
            let sql = "SELECT count(*) FROM core.task WHERE done_at IS NULL";
            if scalar(&pool, sql).await == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
            continue;
        };
        let run = registry().run(&pool, &SystemClock, owner, task, Lease::WORKER);
        assert_eq!(run.await, Ok(Outcome::Done));
    }
}

/// JOB-006, D-491: `count` tasks across two workers of four lanes run exactly once each.
async fn exactly_once(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
    kind: &'static str,
    count: usize,
) {
    let first = opts
        .clone()
        .max_connections(5)
        .connect_with(conn.clone())
        .await
        .expect("pool 1");
    let second = opts
        .max_connections(5)
        .connect_with(conn)
        .await
        .expect("pool 2");
    probe_table(&first).await;
    enqueue_many(&first, kind, count).await;
    tokio::join!(worker(first.clone(), 4), worker(second, 4));
    let runs = scalar(&first, "SELECT count(*) FROM core.probe_run").await;
    let distinct = scalar(&first, "SELECT count(DISTINCT task_id) FROM core.probe_run").await;
    let done = scalar(
        &first,
        "SELECT count(*) FROM core.task WHERE done_at IS NOT NULL",
    )
    .await;
    let expected = i64::try_from(count).expect("count fits");
    assert_eq!((runs, distinct, done), (expected, expected, expected));
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn two_workers_run_100_slow_tasks_exactly_once(opts: PgPoolOptions, conn: PgConnectOptions) {
    exactly_once(opts, conn, SLOW, 100).await;
}

#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn two_workers_run_1000_tasks_exactly_once(opts: PgPoolOptions, conn: PgConnectOptions) {
    exactly_once(opts, conn, PROBE, 1000).await;
}

/// Finishes once `sql` returns `want`: the shutdown signal a test hands the worker loop.
async fn until(pool: PgPool, sql: &'static str, want: i64) {
    while scalar(&pool, sql).await != want {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn real_worker(pool: &PgPool) -> Worker {
    Worker {
        pool: pool.clone(),
        registry: Arc::new(registry().clone()),
        clock: Arc::new(SystemClock),
        owner: WorkerId::new(pool).await.expect("worker id"),
    }
}

/// JOB-006, D-710: the worker's claim loop runs each of 30 tasks once, and a clean shutdown after
/// the queue empties has no lease to give back.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_worker_loop_runs_every_task_once(pool: PgPool) {
    probe_table(&pool).await;
    enqueue_many(&pool, SLOW, 30).await;
    let done = until(
        pool.clone(),
        "SELECT count(*) FROM core.task WHERE done_at IS NOT NULL",
        30,
    );
    let worker = real_worker(&pool).await;
    let run = worker.run(done, Duration::from_secs(5));
    let released = tokio::time::timeout(Duration::from_secs(60), run).await;
    assert_eq!(released.expect("the loop finishes"), Ok(0));
    let runs = scalar(&pool, "SELECT count(*) FROM core.probe_run").await;
    let distinct = scalar(&pool, "SELECT count(DISTINCT task_id) FROM core.probe_run").await;
    assert_eq!((runs, distinct), (30, 30));
}

/// JOB-006, R14, D-710: the loop holds at most four tasks; on shutdown it aborts handlers still
/// running after the drain and gives back each lease and its attempt, so they are due at once.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn the_worker_loop_gives_back_its_leases_on_shutdown(pool: PgPool) {
    enqueue_many(&pool, HANGS, 6).await;
    let leased = "SELECT count(*) FROM core.task WHERE lease_owner IS NOT NULL";
    let worker = real_worker(&pool).await;
    let run = worker.run(until(pool.clone(), leased, 4), Duration::from_millis(50));
    let released = tokio::time::timeout(Duration::from_secs(30), run).await;
    assert_eq!(released.expect("the loop stops"), Ok(4));
    let attempts = "SELECT coalesce(sum(attempts), 0)::bigint FROM core.task";
    assert_eq!(
        (scalar(&pool, leased).await, scalar(&pool, attempts).await),
        (0, 0)
    );
}

fn start() -> Timestamp {
    "2026-11-02T17:00:00Z".parse().expect("fixed instant")
}

async fn one_task(pool: &PgPool, task: NewTask) -> i64 {
    enqueue(&mut pool.acquire().await.expect("connection"), &task)
        .await
        .expect("enqueue")
        .expect("new task")
}

/// JOB-006, D-491: an expired lease is claimed again with the next attempt, and the old owner's
/// finish, renew and fail change nothing.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn an_expired_lease_is_reclaimed_and_the_old_owner_is_fenced(pool: PgPool) {
    let (old, new) = (
        WorkerId::new(&pool).await.unwrap(),
        WorkerId::new(&pool).await.unwrap(),
    );
    one_task(&pool, NewTask::new(PROBE, serde_json::json!({}), start())).await;
    let first = claim(&pool, old, start(), LEASE, 10).await.expect("claim");
    assert_eq!(first.len(), 1);
    let early = start() + SignedDuration::from_secs(29);
    assert!(
        claim(&pool, new, early, LEASE, 10)
            .await
            .expect("claim")
            .is_empty()
    );
    let late = start() + SignedDuration::from_secs(31);
    let second = claim(&pool, new, late, LEASE, 10).await.expect("claim");
    assert_eq!(second.iter().map(|t| t.attempts).collect::<Vec<_>>(), [2]);
    let stale = &first[0];
    assert_eq!(finish(&pool, old, stale, late).await, Ok(false));
    assert_eq!(renew(&pool, old, stale, late + LEASE).await, Ok(false));
    assert_eq!(
        fail(&pool, old, stale, late, PROBE_ERROR).await,
        Err(TaskError::LeaseLost)
    );
    assert_eq!(finish(&pool, new, &second[0], late).await, Ok(true));
}

/// JOB-006: a second task with the same dedupe key is refused, so a timer is never set twice.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_duplicate_dedupe_key_is_refused(pool: PgPool) {
    let mut task = NewTask::new(PROBE, serde_json::json!({ "job": 7 }), start());
    task.dedupe_key = Some("timer:job-7:quote-window".to_owned());
    let mut tx = pool.begin().await.expect("begin");
    assert!(enqueue(&mut tx, &task).await.expect("enqueue").is_some());
    tx.commit().await.expect("commit");
    let mut conn = pool.acquire().await.expect("connection");
    assert_eq!(enqueue(&mut conn, &task).await, Ok(None));
    assert_eq!(scalar(&pool, "SELECT count(*) FROM core.task").await, 1);
}

async fn claim_one(pool: &PgPool, owner: WorkerId, at: Timestamp) -> Option<Claimed> {
    claim(pool, owner, at, LEASE, 10)
        .await
        .expect("claim")
        .into_iter()
        .next()
}

/// D-491: a failing task waits 30 s, then 60 s, and on its third attempt is dead-lettered with
/// the handler's fixed error, never claimed again.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn a_failing_task_backs_off_then_is_dead_lettered(pool: PgPool) {
    let owner = WorkerId::new(&pool).await.expect("worker id");
    let clock = ManualClock::new(start());
    let mut task = NewTask::new(FAILS, serde_json::json!({}), start());
    task.max_attempts = 3;
    one_task(&pool, task).await;
    let mut outcomes = Vec::new();
    for wait in [0, 30, 60] {
        clock.advance(SignedDuration::from_secs(wait - 1));
        if wait > 0 {
            assert!(
                claim_one(&pool, owner, clock.now()).await.is_none(),
                "claimed early"
            );
        }
        clock.advance(SignedDuration::from_secs(1));
        let claimed = claim_one(&pool, owner, clock.now()).await.expect("due");
        outcomes.push(
            registry()
                .run(&pool, &clock, owner, claimed, Lease::WORKER)
                .await,
        );
    }
    let retry = |secs| Ok(Outcome::Retry(start() + SignedDuration::from_secs(secs)));
    assert_eq!(outcomes, [retry(30), retry(90), Ok(Outcome::Dead)]);
    let later = start() + SignedDuration::from_hours(48);
    assert!(claim_one(&pool, owner, later).await.is_none());
    let error: String =
        sqlx::query_scalar("SELECT last_error FROM core.task WHERE dead_at IS NOT NULL")
            .fetch_one(&pool)
            .await
            .expect("dead task");
    assert_eq!(error, PROBE_ERROR);
}

/// D-491: a kind with no handler fails with the fixed sentence; a released lease gives the
/// attempt back; a lease that expires on the last attempt is dead-lettered by the reaper.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn unknown_kinds_released_leases_and_expired_last_attempts(pool: PgPool) {
    let owner = WorkerId::new(&pool).await.expect("worker id");
    let clock = ManualClock::new(start());
    one_task(
        &pool,
        NewTask::new("probe.unregistered", serde_json::json!({}), start()),
    )
    .await;
    let claimed = claim_one(&pool, owner, start()).await.expect("due");
    let outcome = registry()
        .run(&pool, &clock, owner, claimed, Lease::WORKER)
        .await;
    assert!(matches!(outcome, Ok(Outcome::Retry(_))));
    let sql =
        "SELECT count(*) FROM core.task WHERE last_error = 'no handler registered for this kind'";
    assert_eq!(
        (scalar(&pool, sql).await, NO_HANDLER),
        (1, "no handler registered for this kind")
    );
    let mut last = NewTask::new(PROBE, serde_json::json!({}), start());
    last.max_attempts = 1;
    one_task(&pool, last).await;
    assert!(claim_one(&pool, owner, start()).await.is_some());
    assert_eq!(release_leases(&pool, owner).await, Ok(1));
    let again = claim_one(&pool, owner, start())
        .await
        .expect("released task is due at once");
    assert_eq!(again.attempts, 1);
    let expired = start() + SignedDuration::from_secs(31);
    assert_eq!(reap_expired(&pool, expired).await, Ok(1));
    let sql = "SELECT last_error FROM core.task WHERE dead_at IS NOT NULL";
    let error: String = sqlx::query_scalar(sql)
        .fetch_one(&pool)
        .await
        .expect("dead task");
    assert_eq!(error, LEASE_EXPIRED);
}

/// TD-SEC-19 (SEC-040): 20 concurrent requests for one phone across two processes' pools give
/// exactly 5 accepted, because each hit is one atomic upsert.
#[sqlx::test(migrator = "askhava_core::api::MIGRATOR")]
async fn twenty_concurrent_hits_from_two_pools_allow_exactly_five(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let first = opts
        .clone()
        .max_connections(10)
        .connect_with(conn.clone())
        .await
        .expect("pool 1");
    let second = opts
        .max_connections(10)
        .connect_with(conn)
        .await
        .expect("pool 2");
    let mut set = tokio::task::JoinSet::new();
    for n in 0..20 {
        let pool = if n % 2 == 0 {
            first.clone()
        } else {
            second.clone()
        };
        set.spawn(async move {
            let bucket = "intake:phone:fake-blind-index-0001";
            rate_limit_hit(&pool, bucket, start(), SignedDuration::from_hours(1), 5).await
        });
    }
    let decisions = set.join_all().await;
    let allowed = decisions
        .iter()
        .filter(|d| matches!(d, Ok(RateDecision::Allowed { .. })))
        .count();
    let limited = decisions
        .iter()
        .filter(|d| **d == Ok(RateDecision::Limited))
        .count();
    assert_eq!((allowed, limited), (5, 15));
}

fn policy(pairs: &[(&str, &str)]) -> ClientIpPolicy {
    ClientIpPolicy::from_pairs(
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
    )
    .expect("policy parses")
}

fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.insert(*name, value.parse().expect("header value"));
    }
    map
}

fn ip(text: &str) -> IpAddr {
    text.parse().expect("ip")
}

/// TD-SEC-19 (SEC-040): the IP comes from the edge header the setting names; a spoofed
/// `X-Forwarded-For` changes nothing, so it cannot reset the IP limit.
#[test]
fn the_client_ip_comes_only_from_the_named_edge_header() {
    let peer = ip("10.0.0.2");
    let railway = policy(&[]);
    let spoofed = headers(&[
        ("x-real-ip", "198.51.100.7"),
        ("x-forwarded-for", "203.0.113.9"),
    ]);
    assert_eq!(railway.client_ip(&spoofed, peer), ip("198.51.100.7"));
    assert_eq!(
        railway.client_ip(&headers(&[("x-forwarded-for", "203.0.113.9")]), peer),
        peer
    );
    let named = policy(&[("ASKHAVA_CLIENT_IP_HEADER", "X-Envoy-External-Address")]);
    let edge = headers(&[
        ("x-envoy-external-address", "192.0.2.44"),
        ("x-real-ip", "203.0.113.9"),
    ]);
    assert_eq!(named.client_ip(&edge, peer), ip("192.0.2.44"));
    let bad = [(
        "ASKHAVA_CLIENT_IP_HEADER".to_owned(),
        "not a header".to_owned(),
    )];
    assert!(ClientIpPolicy::from_pairs(bad).is_err());
}

/// TD-SEC-19 (SEC-040, SEC-053): `CF-Connecting-IP` is ignored on a DNS-only hostname and, on a
/// proxied one, honoured only when the edge saw a Cloudflare address.
#[test]
fn cf_connecting_ip_counts_only_on_a_proxied_host_from_cloudflare() {
    let peer = ip("10.0.0.2");
    let proxied = policy(&[("ASKHAVA_PROXIED_HOSTS", "www.askhava.com")]);
    let request = |host, edge| {
        headers(&[
            ("host", host),
            ("x-real-ip", edge),
            ("cf-connecting-ip", "198.51.100.7"),
        ])
    };
    let dns_only = request("hooks.askhava.com", "172.64.1.1");
    assert_eq!(proxied.client_ip(&dns_only, peer), ip("172.64.1.1"));
    let from_cloudflare = request("WWW.askhava.com:443", "172.64.1.1");
    assert_eq!(
        proxied.client_ip(&from_cloudflare, peer),
        ip("198.51.100.7")
    );
    let from_v6 = request("www.askhava.com", "2606:4700::6810:1");
    assert_eq!(proxied.client_ip(&from_v6, peer), ip("198.51.100.7"));
    let not_cloudflare = request("www.askhava.com", "203.0.113.9");
    assert_eq!(proxied.client_ip(&not_cloudflare, peer), ip("203.0.113.9"));
    assert_eq!(
        policy(&[]).client_ip(&from_cloudflare, peer),
        ip("172.64.1.1")
    );
}
