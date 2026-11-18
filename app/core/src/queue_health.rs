//! The worker's health as the task queue shows it, for `/healthz` (OPS-007, TD-OPS-02, D-3144,
//! D-3881).
//!
//! No table records the worker's heartbeat: the queue itself shows whether work is being done.
//! [`queue_health`] reads, from `core.task` alone, the oldest task that is due and waiting (no
//! `done_at`, no `dead_at`, no live lease) and the newest sign of a worker (the newest finish or
//! lease). `server`'s probe judges it; `server` may name no `core.` table itself.

use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::PgPool;

use crate::tasks::{Lease, TaskError};

/// What the queue shows about the worker at one moment (OPS-007, D-3144). Holds times only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueHealth {
    /// When the oldest task that is due with no `done_at`, `dead_at` or live lease fell due.
    pub oldest_waiting: Option<Timestamp>,
    /// The newest finish or lease start; `None` when no worker has served this database yet.
    pub newest_activity: Option<Timestamp>,
}

/// OPS-007, D-3144: reads [`QueueHealth`] at `now`. A lease's start is its end less
/// [`Lease::WORKER`]'s length, the only lease the claim loop takes.
///
/// # Errors
/// [`TaskError::Db`] when the read fails.
pub async fn queue_health(pool: &PgPool, now: Timestamp) -> Result<QueueHealth, TaskError> {
    let row = sqlx::query!(
        r#"SELECT
             (SELECT min(run_at) FROM core.task WHERE done_at IS NULL AND dead_at IS NULL
                AND run_at <= $1 AND (lease_until IS NULL OR lease_until < $1)) AS oldest,
             (SELECT max(done_at) FROM core.task) AS finished,
             (SELECT max(lease_until) FROM core.task) AS leased"#,
        now.to_sqlx() as _,
    )
    .fetch_one(pool)
    .await
    .map_err(|_| TaskError::Db)?;
    let stamp = |t: Option<jiff_sqlx::Timestamp>| t.map(jiff_sqlx::Timestamp::to_jiff);
    let leased = stamp(row.leased).map(|end| start_of(end, Lease::WORKER.length));
    Ok(QueueHealth {
        oldest_waiting: stamp(row.oldest),
        newest_activity: stamp(row.finished).max(leased),
    })
}

fn start_of(end: Timestamp, length: SignedDuration) -> Timestamp {
    end.checked_sub(length).unwrap_or(end)
}
