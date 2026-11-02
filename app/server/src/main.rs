//! The `web` process: the `server` binary (`docs/architecture.md` section 2; SEC-029).
//!
//! Installs the TLS provider, refuses to start on bad settings (SEC-002),
//! starts the JSON log, builds the router over the app pool (with the test-control routes only in a
//! `test-control` build, SEC-035) (each request queues its request-log
//! line, OPS-011; Twilio's inbound-text and status webhooks once `ASKHAVA_PUBLIC_BASE_URL` is
//! set, SEC-016, T241) and serves it on
//! `0.0.0.0:$PORT` (the host sets `PORT`) until SIGINT or SIGTERM, then
//! finishes the requests in flight and exits 0. `server migrate` instead
//! applies the pending migrations as the migration role and exits: the
//! release step of the `migrate` service (D-092, SEC-052).

use std::io;
use std::net::Ipv4Addr;

use askhava_core::api::TestControl;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> io::Result<()> {
    askhava_server::install_crypto_provider()?;
    let (settings, _log_guard) = askhava_server::start_up()?;
    if std::env::args().nth(1).as_deref() == Some("migrate") {
        return run_migrations(&settings).await;
    }
    let port = port_from_env()?;
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await?;
    let pool = askhava_core::api::app_pool(&settings).map_err(io::Error::other)?;
    let control =
        TestControl::from_settings(&settings, std::env::vars()).map_err(io::Error::other)?;
    let webhooks = askhava_server::webhooks(&settings, pool.clone(), std::env::vars())?;
    axum::serve(
        listener,
        askhava_server::build_router(pool, control.router(), webhooks),
    )
    .with_graceful_shutdown(askhava_server::shutdown_signal()?)
    .await
}

/// Applies every pending migration over `ASKHAVA_MIGRATION_DATABASE_URL`, never the app
/// role's URL, so the app role owns nothing (D-092, SEC-052). Errors name no value.
async fn run_migrations(settings: &askhava_core::api::Settings) -> io::Result<()> {
    let url = settings
        .migration_database_url()
        .map_err(io::Error::other)?;
    askhava_core::api::migrate(url)
        .await
        .map_err(io::Error::other)
}

/// Reads the listening port from `PORT`. The error never echoes the value.
fn port_from_env() -> io::Result<u16> {
    let raw = std::env::var("PORT").map_err(|_| io::Error::other("PORT is not set"))?;
    raw.parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "PORT is not a port number"))
}
