//! The composition root both binaries share (`docs/architecture.md` section 2; SEC-029, D-481).
//!
//! `server` (the `web` process) and `worker` call only what is here: the TLS
//! provider every outside connection uses, the router, and the shutdown
//! signal. Later tasks add each module's routes to [`build_router`] and its task
//! handlers to a `register_handlers()` beside it; nothing else in the app
//! knows about HTTP status codes or process signals.

use std::io;

use axum::Router;
use tokio::signal::unix::{SignalKind, signal};

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

/// The app's router: every module's routes, merged once here (D-481). Empty
/// until the first module with a web surface adds its routes.
pub fn build_router() -> Router {
    Router::new()
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
