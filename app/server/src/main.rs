//! The `web` process: the `server` binary (`docs/architecture.md` section 2; SEC-029).
//!
//! Installs the TLS provider, refuses to start on bad settings (SEC-002),
//! starts the JSON log, builds the router over the app pool (with the test-control routes only in a
//! `test-control` build, SEC-035) (each request queues its request-log
//! line, OPS-011; Twilio's inbound-text and status webhooks once `ASKHAVA_PUBLIC_BASE_URL` is
//! set, SEC-016, T241; the page pick's code keys once the vault keys are set, D-1516) and serves it on
//! `0.0.0.0:$PORT` (the host sets `PORT`) until SIGINT or SIGTERM, then
//! finishes the requests in flight and exits 0. `server migrate` instead
//! applies the pending migrations as the migration role and exits: the
//! release step of the `migrate` service (D-092, SEC-052). `server bootstrap-owner` creates the
//! one owner once, as the migration role, from the token in `ASKHAVA_OWNER_BOOTSTRAP_TOKEN` and
//! a password read from standard input, and prints the first authenticator's secret and the
//! recovery codes once to the operator's own terminal (SEC-044, SEC-007, D-1918).

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
    if std::env::args().nth(1).as_deref() == Some("bootstrap-owner") {
        return bootstrap_owner(&settings).await;
    }
    let port = port_from_env()?;
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await?;
    let pool = askhava_core::api::app_pool(&settings).map_err(io::Error::other)?;
    let control =
        TestControl::from_settings(&settings, std::env::vars()).map_err(io::Error::other)?;
    let webhooks = askhava_server::webhooks(&settings, pool.clone(), std::env::vars())?;
    let pick_keys = askhava_server::pick_code_keys(&settings, std::env::vars())?;
    axum::serve(
        listener,
        askhava_server::build_router(pool, control.router(), (webhooks, pick_keys)),
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

/// Creates the one owner (SEC-044, D-1918): the token from the host variable, the password as one
/// line on standard input, over the migration role's URL (the app role cannot insert an owner).
/// A second run fails. The enrolment secrets go to standard output once, for Casey's own
/// terminal; errors name no value.
async fn bootstrap_owner(settings: &askhava_core::api::Settings) -> io::Result<()> {
    use askhava_portals::api::{BOOTSTRAP_TOKEN_VAR, bootstrap_owner};
    use secrecy::ExposeSecret;
    use sqlx::Connection;
    let token = std::env::var(BOOTSTRAP_TOKEN_VAR).ok();
    let mut password = String::new();
    io::stdin().read_line(&mut password)?;
    let url = settings
        .migration_database_url()
        .map_err(io::Error::other)?;
    let mut db = sqlx::PgConnection::connect(url.expose_secret())
        .await
        .map_err(|_| io::Error::other("the migration database is unreachable"))?;
    let password = password.trim_end_matches(['\r', '\n']);
    let now = jiff::Timestamp::now();
    let enrolment = bootstrap_owner(&mut db, token.as_deref(), password, now)
        .await
        .map_err(io::Error::other)?;
    let mut out = format!(
        "Owner created. Authenticator secret: {}\nRecovery codes (write them on paper):\n",
        enrolment.totp_secret.expose_secret()
    );
    for code in &enrolment.recovery_codes {
        out.push_str(code.expose_secret());
        out.push('\n');
    }
    io::Write::write_all(&mut io::stdout(), out.as_bytes())
}

/// Reads the listening port from `PORT`. The error never echoes the value.
fn port_from_env() -> io::Result<u16> {
    let raw = std::env::var("PORT").map_err(|_| io::Error::other("PORT is not set"))?;
    raw.parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "PORT is not a port number"))
}
