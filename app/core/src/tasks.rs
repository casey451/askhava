//! The database task queue: every timer and event a worker runs (JOB-006, D-090, D-491).
//!
//! A task is a `core.task` row: a kind, an IDs-only payload, when it is due and a unique dedupe
//! key. A module enqueues it with [`enqueue`] in the same transaction as the state change that
//! needs it (rust brain R11), so a timer survives a restart. A worker [`claim`]s due tasks with
//! `FOR UPDATE SKIP LOCKED`, which raises `attempts` and sets a lease; the claim commits at once
//! and the handler runs outside it (R12). [`finish`], [`fail`] and [`renew`] are fenced by the
//! lease owner and the claim's attempt number, so a worker whose lease expired and was reclaimed
//! changes nothing. A failed task backs off ([`backoff`]) and is dead-lettered after
//! `max_attempts`; a lease that expires on the last attempt is dead-lettered by [`reap_expired`].
//! Every "now" comes from the injected clock and each lease end is computed here in Rust (D-491).
//! [`Registry`] maps a kind to its handler; `app/server/src/lib.rs` builds it in
//! `register_handlers()`; [`Worker::run`] is the claim loop, which `app/server/src/bin/worker.rs`
//! starts with that registry and the shutdown signal (D-710). Every handler
//! first re-checks the current status and does nothing if the job has moved on (JOB-006), so a
//! second run is harmless.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::types::{JsonValue, Uuid};
use sqlx::{PgConnection, PgPool};
use thiserror::Error;
use tokio::task::JoinSet;

use crate::clock::Clock;

/// `last_error` of a task whose kind has no registered handler (D-491).
pub const NO_HANDLER: &str = "no handler registered for this kind";
/// `last_error` of a task whose handler ran past [`Lease::timeout`].
pub const TIMED_OUT: &str = "the handler ran past its timeout";
/// `last_error` of a task whose lease ran out on its last attempt ([`reap_expired`]).
pub const LEASE_EXPIRED: &str = "the lease expired on the last attempt";
/// The handler error when this worker lost the lease while the handler ran.
const LEASE_LOST: &str = "the lease was lost while the handler ran";
/// Attempts a task gets unless [`NewTask::max_attempts`] says otherwise.
pub const DEFAULT_MAX_ATTEMPTS: i32 = 5;
const BACKOFF_FIRST: SignedDuration = SignedDuration::from_secs(30);
const BACKOFF_MAX: SignedDuration = SignedDuration::from_hours(1);

/// Why a queue call failed. No variant carries the database's message, which can quote a value.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TaskError {
    /// The database refused or dropped the statement.
    #[error("the task queue could not be reached")]
    Db,
    /// This worker no longer holds the task's lease: it expired and another claim took it.
    #[error("this worker no longer holds the task's lease")]
    LeaseLost,
}

fn db_error(_: sqlx::Error) -> TaskError {
    TaskError::Db
}

/// A handler's failure: a fixed sentence, so no personal detail can reach `last_error` (D-491).
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
#[error("{0}")]
pub struct HandlerError(pub &'static str);

/// A task to enqueue. `payload` holds IDs only, never a contact value (PRIV-002).
#[derive(Debug, Clone)]
pub struct NewTask {
    /// The handler kind, as registered in `register_handlers()`.
    pub kind: &'static str,
    /// IDs the handler needs, as JSON.
    pub payload: JsonValue,
    /// When the task is due.
    pub run_at: Timestamp,
    /// Claims allowed before the task is dead-lettered.
    pub max_attempts: i32,
    /// A key derived from what the task is for; a second enqueue with it is refused.
    pub dedupe_key: Option<String>,
}

impl NewTask {
    /// A task due at `run_at` with [`DEFAULT_MAX_ATTEMPTS`] and no dedupe key.
    #[must_use]
    pub fn new(kind: &'static str, payload: JsonValue, run_at: Timestamp) -> Self {
        let max_attempts = DEFAULT_MAX_ATTEMPTS;
        let dedupe_key = None;
        Self {
            kind,
            payload,
            run_at,
            max_attempts,
            dedupe_key,
        }
    }
}

/// A claimed task, as the handler receives it. `attempts` counts this claim and fences it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claimed {
    /// The task's row ID.
    pub id: i64,
    /// The handler kind.
    pub kind: String,
    /// The IDs-only payload.
    pub payload: JsonValue,
    /// Claims so far, this one included.
    pub attempts: i32,
}

/// One worker process's lease owner ID, unique per process start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerId(Uuid);

impl WorkerId {
    /// A fresh owner ID from the database's `gen_random_uuid()`.
    ///
    /// # Errors
    ///
    /// [`TaskError::Db`] when the database cannot be reached.
    pub async fn new(pool: &PgPool) -> Result<Self, TaskError> {
        let id = sqlx::query_scalar!(r#"SELECT gen_random_uuid() AS "id!""#)
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
        Ok(Self(id))
    }
}

/// How long a claim holds a task and how long its handler may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lease {
    /// The lease length; it is renewed at half-life while the handler runs.
    pub length: SignedDuration,
    /// The longest a handler may run before it is stopped and the task failed.
    pub timeout: Duration,
}

impl Lease {
    /// The worker's lease: 60 seconds, renewed every 30, and a 2-minute handler timeout.
    pub const WORKER: Self = Self {
        length: SignedDuration::from_secs(60),
        timeout: Duration::from_secs(120),
    };
}

/// What became of a task after one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The handler succeeded and the task is done.
    Done,
    /// The handler failed; the task runs again at this instant.
    Retry(Timestamp),
    /// The handler failed on the last attempt; the task is dead-lettered.
    Dead,
}

fn later(at: Timestamp, by: SignedDuration) -> Timestamp {
    at.checked_add(by).unwrap_or(Timestamp::MAX)
}

/// The wait before attempt `attempts + 1`: 30 seconds doubled per failed attempt, at most an hour.
#[must_use]
pub fn backoff(attempts: i32) -> SignedDuration {
    (BACKOFF_FIRST * (1 << (attempts.clamp(1, 8) - 1))).min(BACKOFF_MAX)
}

/// Inserts a task over `db`, the caller's transaction (`&mut tx`, R11) or a pooled connection
/// (JOB-006). Returns its ID,
/// or `None` when a task with the same dedupe key exists, so a timer is never set twice.
///
/// # Errors
///
/// [`TaskError::Db`] when the insert fails.
pub async fn enqueue(db: &mut PgConnection, task: &NewTask) -> Result<Option<i64>, TaskError> {
    sqlx::query_scalar!(
        "INSERT INTO core.task (kind, payload, run_at, max_attempts, dedupe_key) \
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (dedupe_key) DO NOTHING RETURNING id",
        task.kind,
        task.payload,
        task.run_at.to_sqlx() as _,
        task.max_attempts,
        task.dedupe_key.as_deref(),
    )
    .fetch_optional(db)
    .await
    .map_err(db_error)
}

/// Claims up to `limit` due tasks for `owner` with `FOR UPDATE SKIP LOCKED`, so two workers never
/// take one task; a task whose lease ran out is claimable again (JOB-006, D-491).
///
/// # Errors
///
/// [`TaskError::Db`] when the claim fails.
pub async fn claim(
    pool: &PgPool,
    owner: WorkerId,
    now: Timestamp,
    lease: SignedDuration,
    limit: i64,
) -> Result<Vec<Claimed>, TaskError> {
    sqlx::query_as!(
        Claimed,
        "UPDATE core.task SET lease_owner = $1, lease_until = $2, attempts = attempts + 1 \
         WHERE id IN (SELECT id FROM core.task WHERE done_at IS NULL AND dead_at IS NULL \
           AND run_at <= $3 AND attempts < max_attempts \
           AND (lease_until IS NULL OR lease_until < $3) \
           ORDER BY run_at, id LIMIT $4 FOR UPDATE SKIP LOCKED) \
         RETURNING id, kind, payload, attempts",
        owner.0,
        later(now, lease).to_sqlx() as _,
        now.to_sqlx() as _,
        limit,
    )
    .fetch_all(pool)
    .await
    .map_err(db_error)
}

/// Marks a claimed task done if `owner` still holds this claim; `false` means the lease was lost
/// and another worker owns the task now (fencing, D-491).
///
/// # Errors
///
/// [`TaskError::Db`] when the update fails.
pub async fn finish(
    pool: &PgPool,
    owner: WorkerId,
    task: &Claimed,
    now: Timestamp,
) -> Result<bool, TaskError> {
    let row = sqlx::query_scalar!(
        "UPDATE core.task SET done_at = $4, lease_owner = NULL, lease_until = NULL \
         WHERE id = $1 AND lease_owner = $2 AND attempts = $3 AND done_at IS NULL RETURNING id",
        task.id,
        owner.0,
        task.attempts,
        now.to_sqlx() as _,
    )
    .fetch_optional(pool)
    .await
    .map_err(db_error)?;
    Ok(row.is_some())
}

/// Extends a held lease to `until`; `false` means the lease was lost (D-491).
///
/// # Errors
///
/// [`TaskError::Db`] when the update fails.
pub async fn renew(
    pool: &PgPool,
    owner: WorkerId,
    task: &Claimed,
    until: Timestamp,
) -> Result<bool, TaskError> {
    let row = sqlx::query_scalar!(
        "UPDATE core.task SET lease_until = $4 \
         WHERE id = $1 AND lease_owner = $2 AND attempts = $3 AND done_at IS NULL RETURNING id",
        task.id,
        owner.0,
        task.attempts,
        until.to_sqlx() as _,
    )
    .fetch_optional(pool)
    .await
    .map_err(db_error)?;
    Ok(row.is_some())
}

/// Records a failed run: the lease is cleared and the task runs again after [`backoff`], or, on
/// its last attempt, is dead-lettered. `error` is a fixed sentence (D-491).
///
/// # Errors
///
/// [`TaskError::LeaseLost`] when `owner` no longer holds this claim; [`TaskError::Db`] otherwise.
pub async fn fail(
    pool: &PgPool,
    owner: WorkerId,
    task: &Claimed,
    now: Timestamp,
    error: &'static str,
) -> Result<Outcome, TaskError> {
    let retry_at = later(now, backoff(task.attempts));
    let dead = sqlx::query_scalar!(
        r#"UPDATE core.task SET lease_owner = NULL, lease_until = NULL, last_error = $4, run_at = $5,
             dead_at = CASE WHEN attempts >= max_attempts THEN $6::timestamptz END
           WHERE id = $1 AND lease_owner = $2 AND attempts = $3 AND done_at IS NULL AND dead_at IS NULL
           RETURNING dead_at IS NOT NULL AS "dead!""#,
        task.id,
        owner.0,
        task.attempts,
        error,
        retry_at.to_sqlx() as _,
        now.to_sqlx() as _,
    )
    .fetch_optional(pool)
    .await
    .map_err(db_error)?
    .ok_or(TaskError::LeaseLost)?;
    Ok(if dead {
        Outcome::Dead
    } else {
        Outcome::Retry(retry_at)
    })
}

/// Gives back every lease `owner` holds, and the attempt each claim took, so a worker stopping on
/// SIGTERM leaves its unfinished tasks due at once for the next worker (R14). Returns the count.
///
/// # Errors
///
/// [`TaskError::Db`] when the update fails.
pub async fn release_leases(pool: &PgPool, owner: WorkerId) -> Result<u64, TaskError> {
    let done = sqlx::query!(
        "UPDATE core.task SET lease_owner = NULL, lease_until = NULL, attempts = attempts - 1 \
         WHERE lease_owner = $1 AND done_at IS NULL AND dead_at IS NULL",
        owner.0,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;
    Ok(done.rows_affected())
}

/// Dead-letters every task whose lease ran out on its last attempt (a worker died mid-run), since
/// [`claim`] will never take it again. Returns the count.
///
/// # Errors
///
/// [`TaskError::Db`] when the update fails.
pub async fn reap_expired(pool: &PgPool, now: Timestamp) -> Result<u64, TaskError> {
    let done = sqlx::query!(
        "UPDATE core.task SET dead_at = $1, lease_owner = NULL, lease_until = NULL, last_error = $2 \
         WHERE done_at IS NULL AND dead_at IS NULL AND attempts >= max_attempts AND lease_until < $1",
        now.to_sqlx() as _,
        LEASE_EXPIRED,
    )
    .execute(pool)
    .await
    .map_err(db_error)?;
    Ok(done.rows_affected())
}

/// The future a handler returns.
pub type HandlerFuture = Pin<Box<dyn Future<Output = Result<(), HandlerError>> + Send>>;
type Handler = Arc<dyn Fn(PgPool, Claimed) -> HandlerFuture + Send + Sync>;

/// Which handler runs each task kind; built once by `register_handlers()` (D-481, D-491).
#[derive(Clone, Default)]
pub struct Registry {
    handlers: HashMap<&'static str, Handler>,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.handlers.keys()).finish()
    }
}

impl Registry {
    /// A registry with no handlers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the handler for `kind`, replacing any earlier one. The handler must re-check the
    /// current status first and do nothing if things moved on (JOB-006).
    pub fn register<F, Fut>(&mut self, kind: &'static str, handler: F)
    where
        F: Fn(PgPool, Claimed) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), HandlerError>> + Send + 'static,
    {
        let boxed: Handler = Arc::new(move |pool, task| Box::pin(handler(pool, task)));
        self.handlers.insert(kind, boxed);
    }

    /// Runs one claimed task to the end: its handler under the lease (renewed at half-life) and
    /// the timeout, then the fenced [`finish`] or [`fail`] (JOB-006, D-491).
    ///
    /// # Errors
    ///
    /// [`TaskError::LeaseLost`] when another claim took the task meanwhile; [`TaskError::Db`].
    pub async fn run(
        &self,
        pool: &PgPool,
        clock: &dyn Clock,
        owner: WorkerId,
        task: Claimed,
        lease: Lease,
    ) -> Result<Outcome, TaskError> {
        let result = match self.handlers.get(task.kind.as_str()) {
            None => Err(HandlerError(NO_HANDLER)),
            Some(handler) => {
                let work = handler(pool.clone(), task.clone());
                hold_lease(pool, clock, owner, &task, lease, work).await
            }
        };
        let outcome = match result {
            Ok(()) if finish(pool, owner, &task, clock.now()).await? => Ok(Outcome::Done),
            Ok(()) => Err(TaskError::LeaseLost),
            Err(e) => fail(pool, owner, &task, clock.now(), e.0).await,
        };
        tracing::info!(task = task.id, kind = task.kind, outcome = ?outcome, "task run");
        outcome
    }
}

/// Drives `work` to the end, renewing the lease at half-life; stops it on the timeout or when the
/// lease is lost (rust brain 3.8).
async fn hold_lease(
    pool: &PgPool,
    clock: &dyn Clock,
    owner: WorkerId,
    task: &Claimed,
    lease: Lease,
    work: HandlerFuture,
) -> Result<(), HandlerError> {
    let every = (lease.length / 2).unsigned_abs();
    let mut work = std::pin::pin!(tokio::time::timeout(lease.timeout, work));
    loop {
        tokio::select! {
            done = &mut work => return done.unwrap_or(Err(HandlerError(TIMED_OUT))),
            () = tokio::time::sleep(every) => keep_lease(pool, clock, owner, task, lease).await?,
        }
    }
}

/// Renews the lease once. A lost lease stops the handler; a database error is logged once with a
/// fixed sentence (rust brain R5) and the handler goes on, since the lease may still hold.
async fn keep_lease(
    pool: &PgPool,
    clock: &dyn Clock,
    owner: WorkerId,
    task: &Claimed,
    lease: Lease,
) -> Result<(), HandlerError> {
    let until = later(clock.now(), lease.length);
    match renew(pool, owner, task, until).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(HandlerError(LEASE_LOST)),
        Err(_) => {
            tracing::warn!(task = task.id, "the lease could not be renewed");
            Ok(())
        }
    }
}

/// Runs one worker process starts at once.
pub const WORKER_CONCURRENCY: usize = 4;
/// The shortest and longest wait between claims while the queue is empty.
const POLL_MIN: Duration = Duration::from_millis(200);
const POLL_MAX: Duration = Duration::from_secs(5);

/// One worker process: its pool, the handlers from `register_handlers()`, the clock and its lease
/// owner ID. `app/server/src/bin/worker.rs` builds it and calls [`Worker::run`] (D-491).
#[derive(Clone)]
pub struct Worker {
    /// The app-role pool every claim and handler uses.
    pub pool: PgPool,
    /// Which handler runs each kind.
    pub registry: Arc<Registry>,
    /// Where every "now" comes from.
    pub clock: Arc<dyn Clock>,
    /// This process's lease owner ID.
    pub owner: WorkerId,
}

impl fmt::Debug for Worker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Worker")
            .field("owner", &self.owner)
            .finish()
    }
}

impl Worker {
    /// The claim loop (JOB-006, rust brain R12, R14): claims due tasks with `SKIP LOCKED` up to
    /// [`WORKER_CONCURRENCY`], runs each through [`Registry::run`], polls with a backoff while the
    /// queue is empty and reaps expired last attempts. When `shutdown` finishes it stops claiming,
    /// waits up to `drain` for running handlers, aborts the rest and gives back its leases with
    /// [`release_leases`]. Returns how many leases it gave back.
    ///
    /// # Errors
    ///
    /// [`TaskError::Db`] when the leases cannot be given back; they then expire on their own.
    pub async fn run(
        &self,
        shutdown: impl Future<Output = ()>,
        drain: Duration,
    ) -> Result<u64, TaskError> {
        let mut running = self.claim_loop(shutdown).await;
        if tokio::time::timeout(drain, join_all(&mut running))
            .await
            .is_err()
        {
            running.abort_all();
            join_all(&mut running).await;
        }
        release_leases(&self.pool, self.owner).await
    }

    /// Claims and spawns tasks until `shutdown` finishes; returns the runs still going.
    async fn claim_loop(&self, shutdown: impl Future<Output = ()>) -> JoinSet<()> {
        let mut shutdown = std::pin::pin!(shutdown);
        let mut running = JoinSet::new();
        let mut idle = POLL_MIN;
        loop {
            while running.try_join_next().is_some() {}
            let free = WORKER_CONCURRENCY.saturating_sub(running.len());
            let started = if free == 0 {
                0
            } else {
                self.spawn_due(&mut running, free).await
            };
            let wait = if started > 0 { Duration::ZERO } else { idle };
            idle = if started > 0 {
                POLL_MIN
            } else {
                (idle * 2).min(POLL_MAX)
            };
            let full = running.len() >= WORKER_CONCURRENCY;
            tokio::select! {
                () = &mut shutdown => return running,
                Some(_) = running.join_next(), if full => {}
                () = tokio::time::sleep(wait), if !full => {}
            }
        }
    }

    /// Claims up to `free` due tasks and spawns a run for each; on an empty queue it reaps leases
    /// that expired on a last attempt. Returns how many runs started. A database error is logged
    /// once with a fixed sentence (rust brain R5) and counts as an empty queue.
    async fn spawn_due(&self, running: &mut JoinSet<()>, free: usize) -> usize {
        let now = self.clock.now();
        let limit = i64::try_from(free).unwrap_or(1);
        let tasks = claim(&self.pool, self.owner, now, Lease::WORKER.length, limit)
            .await
            .unwrap_or_else(|_| {
                tracing::warn!("the worker could not claim tasks");
                Vec::new()
            });
        if tasks.is_empty() && reap_expired(&self.pool, now).await.is_err() {
            tracing::warn!("the worker could not reap expired leases");
        }
        let started = tasks.len();
        for task in tasks {
            let worker = self.clone();
            running.spawn(async move {
                let clock = worker.clock.as_ref();
                let run =
                    worker
                        .registry
                        .run(&worker.pool, clock, worker.owner, task, Lease::WORKER);
                let _logged_by_run = run.await;
            });
        }
        started
    }
}

/// Waits for every run in `running` to end.
async fn join_all(running: &mut JoinSet<()>) {
    while running.join_next().await.is_some() {}
}
