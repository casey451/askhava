//! The `worker` process: the claim loop over `core.task` (`docs/architecture.md` sections 2 and 5;
//! JOB-006, SEC-029, D-491).
//!
//! Same TLS provider, settings checks and JSON log as the web process (SEC-002, PRIV-016) and one
//! Tokio runtime. The loop claims due tasks with `FOR UPDATE SKIP LOCKED` up to a fixed
//! concurrency, runs each in a `JoinSet` through the registry from `register_handlers()` (the
//! lease renewed at half-life, a per-handler timeout, the fenced finish or fail), polls with a
//! backoff while the queue is empty and then dead-letters leases that ran out on a last attempt.
//! On SIGINT or SIGTERM it stops claiming, waits up to [`DRAIN`] for running handlers, aborts the
//! rest and gives back its leases, so the next worker takes those tasks at once (rust brain R14).

use std::io;
use std::sync::Arc;
use std::time::Duration;

use askhava_core::api::{
    Clock, Lease, Registry, SystemClock, WorkerId, app_pool, claim, reap_expired, release_leases,
};
use sqlx::PgPool;
use tokio::task::JoinSet;

/// Tasks one worker runs at once.
const CONCURRENCY: usize = 4;
/// The shortest and longest wait between claims while the queue is empty.
const POLL_MIN: Duration = Duration::from_millis(200);
const POLL_MAX: Duration = Duration::from_secs(5);
/// How long running handlers get to finish after SIGTERM, inside the host's grace period.
const DRAIN: Duration = Duration::from_secs(20);

/// What every spawned run shares.
#[derive(Clone)]
struct Worker {
    pool: PgPool,
    registry: Arc<Registry>,
    clock: Arc<dyn Clock>,
    owner: WorkerId,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    askhava_server::install_crypto_provider()?;
    let (settings, _log_guard) = askhava_server::start_up()?;
    let shutdown = askhava_server::shutdown_signal()?;
    let pool = app_pool(&settings).map_err(io::Error::other)?;
    let owner = WorkerId::new(&pool).await.map_err(io::Error::other)?;
    let worker = Worker {
        pool,
        registry: Arc::new(askhava_server::register_handlers()),
        clock: Arc::new(SystemClock),
        owner,
    };
    let mut running = claim_loop(&worker, shutdown).await;
    if tokio::time::timeout(DRAIN, drain(&mut running))
        .await
        .is_err()
    {
        running.abort_all();
        drain(&mut running).await;
    }
    release_leases(&worker.pool, worker.owner)
        .await
        .map_err(io::Error::other)?;
    Ok(())
}

/// Claims and spawns tasks until `shutdown` finishes; returns the runs still going.
async fn claim_loop(worker: &Worker, shutdown: impl Future<Output = ()>) -> JoinSet<()> {
    let mut shutdown = std::pin::pin!(shutdown);
    let mut running = JoinSet::new();
    let mut idle = POLL_MIN;
    loop {
        let free = CONCURRENCY - running.len();
        let started = if free == 0 {
            0
        } else {
            spawn_due(worker, &mut running, free).await
        };
        let wait = if started > 0 { Duration::ZERO } else { idle };
        idle = if started > 0 {
            POLL_MIN
        } else {
            (idle * 2).min(POLL_MAX)
        };
        let full = running.len() >= CONCURRENCY;
        tokio::select! {
            () = &mut shutdown => return running,
            Some(_) = running.join_next(), if full => {}
            () = tokio::time::sleep(wait), if !full => {}
        }
    }
}

/// Claims up to `free` due tasks and spawns a run for each; on an empty queue it reaps leases that
/// expired on a last attempt. Returns how many runs started. A database error counts as empty.
async fn spawn_due(worker: &Worker, running: &mut JoinSet<()>, free: usize) -> usize {
    let now = worker.clock.now();
    let limit = i64::try_from(free).unwrap_or(1);
    let tasks = claim(&worker.pool, worker.owner, now, Lease::WORKER.length, limit)
        .await
        .unwrap_or_default();
    if tasks.is_empty() {
        let _reaped = reap_expired(&worker.pool, now).await;
    }
    let started = tasks.len();
    for task in tasks {
        let worker = worker.clone();
        running.spawn(async move {
            let clock = worker.clock.as_ref();
            let run = worker
                .registry
                .run(&worker.pool, clock, worker.owner, task, Lease::WORKER);
            let _outcome = run.await;
        });
    }
    started
}

/// Waits for every run in `running` to end.
async fn drain(running: &mut JoinSet<()>) {
    while running.join_next().await.is_some() {}
}
