//! The `web` process: the `server` binary (`docs/architecture.md` section 2; SEC-029).
//!
//! Installs the TLS provider, builds the router and serves it on
//! `0.0.0.0:$PORT` (the host sets `PORT`) until SIGINT or SIGTERM, then
//! finishes the requests in flight and exits 0.

use std::io;
use std::net::Ipv4Addr;

use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> io::Result<()> {
    askhava_server::install_crypto_provider()?;
    let port = port_from_env()?;
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await?;
    axum::serve(listener, askhava_server::build_router())
        .with_graceful_shutdown(askhava_server::shutdown_signal()?)
        .await
}

/// Reads the listening port from `PORT`. The error never echoes the value.
fn port_from_env() -> io::Result<u16> {
    let raw = std::env::var("PORT").map_err(|_| io::Error::other("PORT is not set"))?;
    raw.parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "PORT is not a port number"))
}
