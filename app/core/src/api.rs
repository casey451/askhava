//! The public face of `askhava-core`: everything other crates may call (D-481).
//!
//! Settings (SEC-001, SEC-002, SEC-003, SEC-036), the clock and business clock
//! (JOB-007), the database pool, migrations and the guarded-update outcome
//! (JOB-004, D-092), and logging with `Redacted` contact values and the Sentry
//! scrubber (PRIV-016, PRIV-017, PRIV-028), the task queue and its `unfinished` read (JOB-006, PRIV-007) and the rate-limit counters with
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
pub use crate::ids::{
    IdError, IdKind, IdSecret, LinkKind, LinkSalt, LinkToken, card_id, derive_id, link_token,
    pick_id, quote_id,
};
pub use crate::logging::{
    LogGuard, LoggingError, REDACTED, Redacted, init as init_logging, json_layer, request_span,
    scrub_breadcrumb, scrub_event, scrub_text, sentry_options,
};
pub use crate::rate_limit::{
    CF_CONNECTING_IP, CLOUDFLARE_RANGES, ClientIpPolicy, DEFAULT_CLIENT_IP_HEADER, RateDecision,
    RateLimitError, hit as rate_limit_hit, ip_bucket, prune as prune_rate_limits,
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
