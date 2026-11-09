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
//! delete and queues its daily orphan sweep over the same store (PRIV-009, D-1841, T276).

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
    let (photos, id_secret) = (Arc::new(stream.storage.clone()), stream.id_secret.clone());
    let mut registry = askhava_server::register_handlers(Arc::new(stream), texts, confirm);
    let photo_clock = Arc::clone(&clock);
    let secret = id_secret.clone();
    askhava_privacy::api::register_photo_deletes(&mut registry, photos, secret, photo_clock);
    if let Some(twilio) = twilio_config(&settings)? {
        let (sms, _, _) =
            askhava_adapters::api::twilio_clients(&twilio).map_err(io::Error::other)?;
        let (sms, scan_clock) = (Arc::new(sms), Arc::clone(&clock));
        askhava_messaging::api::register_body_deletes(&mut registry, sms, id_secret, scan_clock);
        let masked = twilio.pool().clone();
        let line_clock = Arc::clone(&clock);
        registry.register(askhava_picks::api::LINE_RETRY_KIND, move |pool, task| {
            askhava_picks::api::retry_line(pool, masked.clone(), task, line_clock.now())
        });
    }
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
/// photo orphan sweep and the day-31 body scan) and runs the intake golden set once.
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
    askhava_intake::api::run_golden(&mut conn, now)
        .await
        .map(|_| ())
        .map_err(io::Error::other)
}

/// The Twilio settings the masked-line retry (its pool, CALL-001, CALL-017, D-1688) and the day-31
/// body delete (PRIV-027) use; without them `None` in dev and CI (no handler, so those tasks wait)
/// and an error in the test and live projects.
fn twilio_config(
    settings: &askhava_core::api::Settings,
) -> io::Result<Option<askhava_adapters::api::TwilioConfig>> {
    use askhava_core::api::Environment;
    match askhava_adapters::api::TwilioConfig::from_settings(settings, std::env::vars()) {
        Ok(config) => Ok(Some(config)),
        Err(error) => match settings.environment() {
            Environment::Dev | Environment::Ci => Ok(None),
            Environment::Test | Environment::Live => Err(io::Error::other(error)),
        },
    }
}
