//! The public face of `askhava-core`: everything other crates may call (D-481).
//!
//! Settings (SEC-001, SEC-002, SEC-003, SEC-036), the clock and business clock
//! (JOB-007), the database pool, migrations and the guarded-update outcome
//! (JOB-004, D-092), and logging with `Redacted` contact values and the Sentry
//! scrubber (PRIV-016, PRIV-017, PRIV-028), the numbers-only database metrics (D-3292), the task queue and its `unfinished` read (JOB-006, PRIV-007), its dead-letter reads and one-row revive (OPS-004, JOB-006, T327, T366) and the rate-limit counters with
//! the client IP they are keyed on (SEC-014, SEC-040), and the test-control router and clock,
//! whose routes exist only under the `test-control` feature (SEC-035), derived IDs and link tokens
//! (CARD-017, PICK-028, SEC-037) and the webhook dedupe outcome and event key (SEC-017, SEC-039), and
//! the brand as data: `brand()`, `install_brand` and `Brand::fill` (WORD-001, WORD-008, D-1932).

pub use crate::brand::{Brand, BrandError, BrandEs, brand, install as install_brand};
pub use crate::clock::{
    BUSINESS_CLOSE_HOUR, BUSINESS_OPEN_HOUR, Clock, ClockError, ManualClock, SystemClock,
    add_business_minutes, arizona, arizona_time, is_business_hours, local_time, zone,
};
pub use crate::db::{DbError, Guarded, MIGRATOR, app_pool, migrate};
pub use crate::db_metrics::{
    DbMetrics, DbSnapshot, MetricsLayer, PoolSnapshot, WAIT_BUCKETS_US, WaitBucket,
    global as db_metrics,
};
pub use crate::ids::{
    IdError, IdKind, IdSecret, LinkKind, LinkSalt, LinkToken, card_id, derive_id, link_token,
    pick_id, quote_id,
};
pub use crate::logging::{
    LogGuard, LoggingError, REDACTED, Redacted, init as init_logging, json_filter, json_layer,
    metrics_filter, request_span, scrub_breadcrumb, scrub_event, scrub_text, sentry_options,
    subscriber as log_subscriber,
};
pub use crate::rate_limit::{
    CF_CONNECTING_IP, CLOUDFLARE_RANGES, ClientIpPolicy, DEFAULT_CLIENT_IP_HEADER, RateDecision,
    RateLimitError, hit as rate_limit_hit, hit_on as rate_limit_hit_on, ip_bucket,
    prune as prune_rate_limits,
};
pub use crate::settings::{Environment, Settings, SettingsError, Vendor, check_base_url};
pub use crate::task_queries::unfinished;
pub use crate::tasks::{
    Claimed, DEFAULT_MAX_ATTEMPTS, HandlerError, HandlerFuture, LEASE_EXPIRED, Lease, NO_HANDLER,
    NewTask, Outcome, Registry, TIMED_OUT, TaskError, WORKER_CONCURRENCY, Worker, WorkerId,
    backoff, claim, enqueue, fail, finish, reap_expired, release_leases, renew,
};
pub use crate::test_control::{TEST_CONTROL_COMPILED, TestControl, TestControlError};
pub use crate::webhooks::{Dedupe, EventKey, WebhookError};

/// OPS-004, JOB-006, D-2285: each task kind with a dead-lettered `core.task` whose last error is
/// [`NO_HANDLER`], with how many, in kind order. Kinds and counts only, never a payload; the
/// server's hourly no-handler check reads it (`app/server/src/no_handler_alert.rs`, T327).
///
/// # Errors
/// [`TaskError::Db`] when the read fails.
pub async fn dead_without_handler(
    db: &mut sqlx::PgConnection,
) -> Result<Vec<(String, i64)>, TaskError> {
    let rows = sqlx::query!(
        r#"SELECT kind, count(*) AS "count!" FROM core.task
           WHERE dead_at IS NOT NULL AND last_error = $1 GROUP BY kind ORDER BY kind"#,
        NO_HANDLER,
    )
    .fetch_all(db)
    .await
    .map_err(|_| TaskError::Db)?;
    Ok(rows.into_iter().map(|r| (r.kind, r.count)).collect())
}

/// OPS-004, JOB-006, D-2640: each task kind with tasks dead-lettered at or after `since` whose
/// last error is not [`NO_HANDLER`] (that check is [`dead_without_handler`]'s), with the UTC day
/// number (`dead_at` seconds / 86,400) and how many died that day, in kind and day order. Kinds,
/// days and counts only, never a payload; the server's hourly check reads it (T366).
///
/// # Errors
/// [`TaskError::Db`] when the read fails.
pub async fn dead_by_kind_since(
    db: &mut sqlx::PgConnection,
    since: jiff::Timestamp,
) -> Result<Vec<(String, i64, i64)>, TaskError> {
    use jiff_sqlx::ToSqlx;
    let rows = sqlx::query!(
        r#"SELECT kind, floor(extract(epoch FROM dead_at) / 86400)::bigint AS "day!",
                  count(*) AS "count!"
           FROM core.task WHERE dead_at >= $1 AND (last_error IS NULL OR last_error <> $2)
           GROUP BY 1, 2 ORDER BY 1, 2"#,
        since.to_sqlx() as _,
        NO_HANDLER,
    )
    .fetch_all(db)
    .await
    .map_err(|_| TaskError::Db)?;
    Ok(rows.into_iter().map(|r| (r.kind, r.day, r.count)).collect())
}

/// What [`revive_dead`] found for a dedupe key (JOB-006, D-2640).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Revival {
    /// The dead task was revived; how many times it has been revived now.
    Revived(i64),
    /// The task is dead and has been revived the most times allowed already; it stays dead.
    Exhausted,
    /// The task is waiting, running or done; nothing changed.
    Alive,
    /// No task holds the key.
    Missing,
}

/// JOB-006, D-2640: revives the dead-lettered task holding `dedupe_key` in one statement on its
/// row: `dead_at` and `last_error` cleared, attempts back to 0, due at `now`, and the payload's
/// `revived` count raised by one, unless it has already been revived `most` times. Answers what
/// it found; the jobs timer backstop (T366) calls it in its audit row's transaction.
///
/// # Errors
/// [`TaskError::Db`] when a statement fails.
pub async fn revive_dead(
    db: &mut sqlx::PgConnection,
    dedupe_key: &str,
    now: jiff::Timestamp,
    most: i64,
) -> Result<Revival, TaskError> {
    use jiff_sqlx::ToSqlx;
    let revived = sqlx::query_scalar!(
        r#"UPDATE core.task SET dead_at = NULL, last_error = NULL, attempts = 0, run_at = $2,
             payload = jsonb_set(payload, '{revived}',
               to_jsonb(COALESCE((payload->>'revived')::bigint, 0) + 1))
           WHERE dedupe_key = $1 AND dead_at IS NOT NULL
             AND COALESCE((payload->>'revived')::bigint, 0) < $3
           RETURNING (payload->>'revived')::bigint AS "revived!""#,
        dedupe_key,
        now.to_sqlx() as _,
        most,
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(|_| TaskError::Db)?;
    if let Some(times) = revived {
        return Ok(Revival::Revived(times));
    }
    let dead = sqlx::query_scalar!(
        r#"SELECT dead_at IS NOT NULL AS "dead!" FROM core.task WHERE dedupe_key = $1"#,
        dedupe_key,
    )
    .fetch_optional(db)
    .await
    .map_err(|_| TaskError::Db)?;
    Ok(match dead {
        Some(true) => Revival::Exhausted,
        Some(false) => Revival::Alive,
        None => Revival::Missing,
    })
}
