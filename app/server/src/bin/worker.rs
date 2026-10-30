//! The `worker` process (`docs/architecture.md` sections 2 and 5; SEC-029).
//!
//! Same TLS provider, settings checks and JSON log as the web process
//! (SEC-002, PRIV-016) and one Tokio runtime. Until T013 adds
//! the claim loop over `core.task`, it idles until SIGINT or SIGTERM and exits 0,
//! so the host sees a healthy process rather than a restart loop.

use std::io;

#[tokio::main]
async fn main() -> io::Result<()> {
    askhava_server::install_crypto_provider()?;
    let (_settings, _log_guard) = askhava_server::start_up()?;
    askhava_server::shutdown_signal()?.await;
    Ok(())
}
