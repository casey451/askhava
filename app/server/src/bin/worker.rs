//! The `worker` process: starts the claim loop over `core.task` (`docs/architecture.md` sections 2
//! and 5; JOB-006, SEC-029, D-491, D-710).
//!
//! Same TLS provider, settings checks and JSON log as the web process (SEC-002, PRIV-016) and one
//! Tokio runtime. It builds an `askhava_core::api::Worker` from the registry `register_handlers()`
//! returns and runs it until SIGINT or SIGTERM; the loop itself (claims with `FOR UPDATE SKIP
//! LOCKED`, the drain, `release_leases`) lives in `app/core/src/tasks.rs`, so the integration
//! tests drive the same code (rust brain R14). It also builds the off-database stream writer from
//! `ASKHAVA_STORAGE_BASE_URL` and `ASKHAVA_ID_SECRET` and queues the nightly chain check (OPS-011,
//! PRIV-029, D-734).

use std::io;
use std::sync::Arc;
use std::time::Duration;

use askhava_core::api::{Clock, SystemClock, Worker, WorkerId, app_pool};
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
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let stream = OffDbStream::from_settings(&settings, std::env::vars(), Arc::clone(&clock))
        .map_err(io::Error::other)?;
    let mut conn = pool.acquire().await.map_err(io::Error::other)?;
    schedule_verify(&mut conn, clock.now())
        .await
        .map_err(io::Error::other)?;
    drop(conn);
    let worker = Worker {
        pool,
        registry: Arc::new(askhava_server::register_handlers(Arc::new(stream))),
        clock,
        owner,
    };
    worker
        .run(shutdown, DRAIN)
        .await
        .map_err(io::Error::other)?;
    Ok(())
}
