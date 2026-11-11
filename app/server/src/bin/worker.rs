//! The `worker` process: starts the claim loop over `core.task` (`docs/architecture.md` sections 2
//! and 5; JOB-006, SEC-029, D-491, D-710).
//!
//! Same TLS provider, settings checks and JSON log as the web process (SEC-002, PRIV-016) and one
//! Tokio runtime. It builds an `askhava_core::api::Worker` from the registry `register_handlers()`
//! returns and runs it until SIGINT or SIGTERM, on `TestControl`'s clock (the system clock in the
//! release build, the advanced test clock in a `test-control` build, SEC-035); the loop itself (claims with `FOR UPDATE SKIP
//! LOCKED`, the drain, `release_leases`) lives in `app/core/src/tasks.rs`, so the integration
//! tests drive the same code (rust brain R14). It also builds the off-database stream writer from
//! `ASKHAVA_STORAGE_BASE_URL` and `ASKHAVA_ID_SECRET` and queues the nightly chain check (OPS-011,
//! PRIV-029, D-734), the daily retention purge sweep (PRIV-007, D-491) and the daily message body
//! cut (TXT-020, D-1854). At start-up it runs the intake golden set once for the compiled-in catalog
//! version (OPS-040, D-493). With `VAULT_KEY` and `VAULT_INDEX_KEY` set (required in the test and
//! live projects) it registers `intake`'s homeowner job-text sender (D-1302). With the Twilio
//! settings (required in the test and live projects) it registers `picks`' masked-line retry,
//! `picks.line_retry`, over the masked pool (CALL-001, D-1688, T254), and `messaging`'s day-31
//! body delete at Twilio (PRIV-027, D-1841, T276). It registers `privacy`'s sealed photo object
//! delete and queues its daily orphan sweep over the same store (PRIV-009, D-1841, T276), and
//! `messaging`'s paused-emergency follow-up with no email keys, so each homeowner goes on Casey's
//! call list until a footer postal setting exists (TXT-023, D-2186, T279). All of these
//! registrations live in `askhava_server::worker_registry`, which the task-kind registry test
//! builds too (TD-OPS-36), with the hourly no-handler check (OPS-004, D-2285, T327), whose first
//! run `schedule_start` queues.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use askhava_core::api::{TestControl, Worker, WorkerId, app_pool};
use askhava_server::offdb_stream::{OffDbStream, schedule_verify};

/// How long running handlers get to finish after SIGTERM, inside the host's grace period.
const DRAIN: Duration = Duration::from_secs(20);

#[tokio::main]
async fn main() -> io::Result<()> {
    askhava_server::install_crypto_provider()?;
    let (settings, _log_guard) = askhava_server::start_up()?;
    let shutdown = askhava_server::shutdown_signal()?;
    let pool = app_pool(&settings).map_err(io::Error::other)?;
    let owner = WorkerId::new(&pool).await.map_err(io::Error::other)?;
    let control =
        TestControl::from_settings(&settings, std::env::vars()).map_err(io::Error::other)?;
    let clock = control.clock();
    let stream = OffDbStream::from_settings(&settings, std::env::vars(), Arc::clone(&clock))
        .map_err(io::Error::other)?;
    let texts = askhava_server::job_text_keys(&settings, &stream.id_secret, std::env::vars())?
        .map(Arc::new);
    let confirm = askhava_picks::api::ConfirmSettings::from_pairs(std::env::vars())
        .map_err(io::Error::other)?;
    schedule_start(&pool, clock.now()).await?;
    let parts = (texts, confirm);
    let registry = askhava_server::worker_registry(&settings, stream, parts, std::env::vars())?;
    let worker = Worker {
        pool,
        registry: Arc::new(registry),
        clock,
        owner,
    };
    worker
        .run(shutdown, DRAIN)
        .await
        .map_err(io::Error::other)?;
    Ok(())
}

/// Queues the daily tasks at start (the chain check, the retention sweep, the body cut, the
/// photo orphan sweep, the day-31 body scan and this hour's no-handler check) and runs the intake golden set once.
async fn schedule_start(pool: &sqlx::PgPool, now: jiff::Timestamp) -> io::Result<()> {
    let mut conn = pool.acquire().await.map_err(io::Error::other)?;
    schedule_verify(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_jobs::api::schedule_sweep(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_messaging::api::schedule_cut(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_privacy::api::schedule_orphan_sweep(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_messaging::api::schedule_body_scan(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_server::no_handler_alert::schedule(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_intake::api::run_golden(&mut conn, now)
        .await
        .map(|_| ())
        .map_err(io::Error::other)
}
