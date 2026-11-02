//! The composition root both binaries share (`docs/architecture.md` section 2; SEC-029, D-481).
//!
//! `server` (the `web` process) and `worker` call only what is here: the TLS
//! provider every outside connection uses, the settings and logging start-up,
//! the router, and the shutdown signal. Later tasks add each module's routes to [`build_router`] and its task
//! handlers to a `register_handlers()` beside it; nothing else in the app
//! knows about HTTP status codes or process signals.

use std::io;
use std::sync::Arc;

use askhava_adapters::api::StorageAdapter;
use askhava_core::api::{
    Environment, IdSecret, LogGuard, Registry, Settings, SystemClock, init_logging, request_span,
};
use askhava_intake::api::JobTextKeys;
use askhava_portals::api::{pick_flow_routes, quote_page_routes, security_headers};
use askhava_privacy::api::Vault;
use axum::Router;
use axum::body::Body;
use secrecy::SecretString;
use sqlx::PgPool;
use tokio::signal::unix::{SignalKind, signal};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

pub mod offdb_stream;

/// Installs rustls' aws-lc-rs provider as the process default, so sqlx and
/// reqwest share one TLS provider (D-481, D-488). Call it first in `main`.
///
/// # Errors
///
/// Fails if a provider is already installed in this process, which means
/// something ran before `main` set it up.
pub fn install_crypto_provider() -> io::Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .map_err(|_| io::Error::other("a rustls crypto provider is already installed"))
}

/// Reads the settings from the process environment and starts the JSON log
/// and Sentry, so a bad configuration stops the process before it serves or
/// claims anything (SEC-002, SEC-036, PRIV-016, PRIV-017). Hold the guard
/// for the life of `main`.
///
/// # Errors
///
/// Any settings or logging error; its message names a key, never a value.
pub fn start_up() -> io::Result<(Settings, LogGuard)> {
    let settings = Settings::from_pairs(std::env::vars()).map_err(io::Error::other)?;
    let guard = init_logging(&settings).map_err(io::Error::other)?;
    Ok((settings, guard))
}

/// The app's router: every module's routes, merged once here (D-481), under
/// the request-ID and tracing layers, so every request log line carries the
/// request ID and the matched route template, never the URI (PRIV-016,
/// PRIV-028), and every request hands its request-log line to a background writer that queues it
/// for the off-database stream over `pool`, so no response waits on the database (OPS-011, D-734,
/// D-737). Call it inside the runtime (it spawns that writer). Merge routes before the layers so
/// they apply to them. `test_control` is `askhava_core::api::TestControl::router()`, which has no
/// routes in the release build (SEC-035, D-739). The outermost layer sets the SEC-012 security
/// headers on every response, a 404 included (T051). The homeowner's quote page is `portals`'
/// (`/q/{token}`, T052); its pick review step posts to the same path (`pick_flow`, T055).
pub fn build_router(pool: PgPool, test_control: Router) -> Router {
    let quote_page = quote_page_routes(pool.clone(), Arc::new(SystemClock));
    let pick_flow = pick_flow_routes(pool.clone(), Arc::new(SystemClock));
    let (log, _writer) = offdb_stream::RequestLog::spawn(pool);
    Router::new()
        .merge(test_control)
        .merge(quote_page)
        .merge(pick_flow)
        .layer(axum::middleware::from_fn_with_state(
            log,
            offdb_stream::log_request,
        ))
        .layer(TraceLayer::new_for_http().make_span_with(request_span::<Body>))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(axum::middleware::from_fn(security_headers))
}

/// The vault's sealing keys, named as in `docs/architecture.md` (PRIV-002, PRIV-024).
const VAULT_KEY: &str = "VAULT_KEY";
/// The vault's blind-index keys (PRIV-002).
const VAULT_INDEX_KEY: &str = "VAULT_INDEX_KEY";

/// The keys of `intake`'s homeowner job-text sender (D-1302): the `ID_SECRET` (version 1) as the
/// quote-page link key and the outbox key, and the vault built from `VAULT_KEY` and
/// `VAULT_INDEX_KEY` in `pairs`. Without both vault keys it is `None` in dev and CI (no sender
/// is registered) and an error in the test and live projects.
///
/// # Errors
///
/// A weak `ID_SECRET`, a bad vault key, or no vault keys in the test or live project; the
/// message names a key, never a value.
pub fn job_text_keys<I>(
    settings: &Settings,
    id_secret: &SecretString,
    pairs: I,
) -> io::Result<Option<JobTextKeys>>
where
    I: IntoIterator<Item = (String, String)>,
{
    let (mut seal, mut index) = (None, None);
    for (key, value) in pairs {
        match key.as_str() {
            VAULT_KEY => seal = Some(SecretString::from(value)),
            VAULT_INDEX_KEY => index = Some(SecretString::from(value)),
            _ => {}
        }
    }
    let (Some(seal), Some(index)) = (seal, index) else {
        return match settings.environment() {
            Environment::Dev | Environment::Ci => Ok(None),
            Environment::Test | Environment::Live => Err(io::Error::other("missing VAULT_KEY")),
        };
    };
    let vault = Vault::new(&seal, &index, settings.database_url()).map_err(io::Error::other)?;
    let link = IdSecret::new(1, id_secret.clone()).map_err(io::Error::other)?;
    Ok(Some(JobTextKeys::new(link, id_secret.clone(), vault)))
}

/// Every module's `core.task` handlers, one `register` call per kind (D-481, D-491). The worker
/// runs what it returns; tests hold it in a `OnceLock`. A kind with no handler is failed with
/// `NO_HANDLER` and dead-lettered after its attempts (JOB-006). `stream` is the off-database
/// stream writer and its nightly check (OPS-011, PRIV-029, D-734); `jobs` runs the job status
/// timers (JOB-003, JOB-006); `quotes` the every-window-ended check (TXT-042, JOB-025); `intake` the
/// no-reply reminder and close (INT-019), the card approval auto-send (INT-041) and, with
/// `texts` ([`job_text_keys`]), the homeowner job texts `jobs` queues (JOB-010 to JOB-012,
/// TXT-042, D-1030 (A)).
#[must_use]
pub fn register_handlers<S>(
    stream: Arc<offdb_stream::OffDbStream<S>>,
    texts: Option<Arc<JobTextKeys>>,
) -> Registry
where
    S: StorageAdapter + Send + Sync + 'static,
{
    let mut registry = Registry::new();
    let secret = stream.id_secret.clone();
    registry.register(askhava_intake::api::NO_REPLY_KIND, move |pool, task| {
        askhava_intake::api::run_no_reply(pool, secret.clone(), task)
    });
    registry.register(
        askhava_intake::api::CARD_TIMER_KIND,
        askhava_intake::api::run_card_timer,
    );
    offdb_stream::register(&mut registry, stream);
    registry.register(askhava_jobs::api::TIMER_KIND, askhava_jobs::api::run_timer);
    registry.register(
        askhava_quotes::api::WINDOWS_KIND,
        askhava_quotes::api::run_windows_end,
    );
    if let Some(keys) = texts {
        registry.register(askhava_jobs::api::TEXT_KIND, move |pool, task| {
            askhava_intake::api::run_job_text(pool, Arc::clone(&keys), task)
        });
    }
    registry
}

/// Registers SIGINT and SIGTERM now and returns a future that finishes on the
/// first of them, so the web server drains and the worker stops claiming
/// before the host's grace period ends (a container stop sends SIGTERM; D-491).
///
/// # Errors
///
/// Fails if the operating system refuses a signal handler. Must be called
/// inside the Tokio runtime.
pub fn shutdown_signal() -> io::Result<impl Future<Output = ()>> {
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    Ok(async move {
        tokio::select! {
            _ = interrupt.recv() => {}
            _ = terminate.recv() => {}
        }
    })
}
