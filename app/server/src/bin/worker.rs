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
//! body delete at Twilio (PRIV-027, D-1841, T276), and with the vault keys too `messaging`'s outbox
//! sender, `messaging.send_due`, whose first run `schedule_start` queues (D-2628, T356). It registers `privacy`'s sealed photo object
//! delete and queues its daily orphan sweep over the same store (PRIV-009, D-1841, T276), and
//! `messaging`'s paused-emergency follow-up with no email keys, so each homeowner goes on Casey's
//! call list until a footer postal setting exists (TXT-023, D-2186, T279). All of these
//! registrations live in `askhava_server::worker_registry`, which the task-kind registry test
//! builds too (TD-OPS-36), with the hourly no-handler check (OPS-004, D-2285, T327), whose first
//! run `schedule_start` queues. It exits non-zero once its database round trip or claim-loop
//! tick has failed for `ASKHAVA_LIVENESS_LIMIT_SECS` (`liveness`, OPS-049, T106).

use std::io;
use std::sync::Arc;
use std::time::Duration;

use askhava_core::api::{ClaimTick, TestControl, Worker, WorkerId, app_pool};
use askhava_server::liveness::{LivenessLimit, watch_worker};
use askhava_server::offdb_stream::{OffDbStream, schedule_verify};

/// How long running handlers get to finish after SIGTERM, inside the host's grace period.
const DRAIN: Duration = Duration::from_secs(20);

#[tokio::main]
async fn main() -> io::Result<()> {
    askhava_server::install_crypto_provider()?;
    let (settings, _log_guard) = askhava_server::start_up()?;
    let shutdown = askhava_server::shutdown_signal()?;
    let pool = app_pool(&settings).map_err(io::Error::other)?;
    // No task runs under a key set missing a held index version (D-4418, D-4473).
    let vault = askhava_server::business_portal::vault_from_settings(&settings, std::env::vars())?;
    askhava_server::check_index_keys(&pool, vault.as_ref()).await?;
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
    let lists = askhava_licensing::api::DailyListSettings::from_pairs(std::env::vars())
        .map_err(io::Error::other)?;
    let email = askhava_server::email_drain_ready(&settings, std::env::vars())?;
    schedule_start(&pool, clock.now(), (lists, email)).await?;
    let parts = (texts, confirm);
    let registry = askhava_server::worker_registry(&settings, stream, parts, std::env::vars())?;
    let worker = Worker {
        pool,
        registry: Arc::new(registry),
        clock,
        owner,
    };
    let tick = Arc::new(ClaimTick::default());
    let limit = LivenessLimit::from_pairs(std::env::vars())?;
    let watch = watch_worker(worker.pool.clone(), Arc::clone(&tick), limit);
    tokio::select! {
        ran = worker.run_watched(shutdown, DRAIN, &tick) => ran.map(|_| ()).map_err(io::Error::other),
        failed = watch => Err(failed),
    }
}

/// `messaging`'s daily tasks at start: the body cut, the day-31 body scan, the outbox sender's
/// first run, OPS-046's deferral check (T531, D-3883), the alert push and hourly checks (T204), and the
/// weekly opt-out rate check.
async fn schedule_messaging(conn: &mut sqlx::PgConnection, now: jiff::Timestamp) -> io::Result<()> {
    askhava_messaging::api::schedule_cut(conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_messaging::api::schedule_body_scan(conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_messaging::api::schedule_send_due(conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_messaging::api::schedule_deferral_check(conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_messaging::api::schedule_alert_push(conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_messaging::api::schedule_hourly_checks(conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_messaging::api::schedule_optout_rate_check(conn, now)
        .await
        .map_err(io::Error::other)
}

/// The first nightly Stripe reconcile (T433), then the first runs of `watch_checks`' hourly
/// vault-read check and daily volume check (T525, SEC-047, OPS-052), `intake`'s daily consent
/// sweep (T563, CON-022) and, only when `email` (the provider and vault exist), `messaging`'s
/// queued-email drain (T470, D-3750), each under its own dedupe key.
async fn schedule_checks(
    conn: &mut sqlx::PgConnection,
    now: jiff::Timestamp,
    email: bool,
) -> io::Result<()> {
    if email {
        askhava_messaging::api::schedule_email_due(conn, now)
            .await
            .map_err(io::Error::other)?;
    }
    askhava_server::payment_jobs::schedule_reconcile(conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_server::watch_checks::schedule_read_check(conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_server::watch_checks::schedule_volume_check(conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_intake::api::schedule_consent_sweep(conn, now)
        .await
        .map_err(io::Error::other)
}

/// Queues the daily tasks at start (the chain check, the masked-line expiry sweep (T662), the retention sweep, the body cut, the
/// photo orphan sweep, the day-31 body scan, this hour's dead-letter check, the job timer
/// backstop, the hourly request count (T105), the safe-mode check (T487), the session sweep and rate-limit prune (T436), the license-map re-apply, each daily license list (T371), the outbox sender's
/// first run, the deferral check, the weekly opt-out rate check ([`schedule_messaging`]) and the first nightly Stripe reconcile
/// (T433) with the hourly vault-read and daily volume checks and the daily consent sweep ([`schedule_checks`], T525, T563)) and runs the intake golden set once.
async fn schedule_start(
    pool: &sqlx::PgPool,
    now: jiff::Timestamp,
    (lists, email): (askhava_licensing::api::DailyListSettings, bool),
) -> io::Result<()> {
    let mut conn = pool.acquire().await.map_err(io::Error::other)?;
    schedule_verify(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    schedule_messaging(&mut conn, now).await?;
    askhava_calls::api::schedule_close_expired(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_jobs::api::schedule_sweep(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_privacy::api::schedule_orphan_sweep(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_privacy::api::schedule_words_cut(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_server::no_handler_alert::schedule(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_jobs::api::schedule_backstop(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_jobs::api::schedule_request_counts(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_ops::api::schedule_safe_mode(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_portals::api::schedule_session_sweep(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_core::api::schedule_rate_limit_prune(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_businesses::api::schedule_reapply(&mut conn, now)
        .await
        .map_err(io::Error::other)?;
    askhava_licensing::api::schedule_daily_lists(&mut conn, now, lists)
        .await
        .map_err(io::Error::other)?;
    schedule_checks(&mut conn, now, email).await?;
    askhava_intake::api::run_golden(&mut conn, now)
        .await
        .map(|_| ())
        .map_err(io::Error::other)
}
