//! The `web` process: the `server` binary (`docs/architecture.md` section 2; SEC-029).
//!
//! Installs the TLS provider, refuses to start on bad settings (SEC-002),
//! starts the JSON log, builds the router over the app pool (with the test-control routes only in a
//! `test-control` build, SEC-035) (each request queues its request-log
//! line, OPS-011; Twilio's inbound-text and status webhooks once `ASKHAVA_PUBLIC_BASE_URL` is
//! set, SEC-016, T241; the page pick's code keys once the vault keys are set, D-1516; Stripe's
//! webhook and the release once `ASKHAVA_STRIPE_WEBHOOK_SECRET` is set, T254; every request under
//! the `ASKHAVA_REQUEST_TIMEOUT_SECS` limit, D-1870; the business sign-in pages, which refuse to
//! start without the new-device code settings in test and live, T280) and serves it, with
//! the socket peer as connect info (SEC-040), on `0.0.0.0:$PORT` (the host sets `PORT`) until SIGINT or SIGTERM, then
//! finishes the requests in flight and exits 0; it exits non-zero once its database round trip
//! has failed for `ASKHAVA_LIVENESS_LIMIT_SECS` (`liveness`, OPS-049, T106). `server migrate` instead
//! applies the pending migrations as the migration role and exits: the
//! release step of the `migrate` service (D-092, SEC-052). `server bootstrap-owner` creates the
//! one owner once, as the migration role, from the token in `ASKHAVA_OWNER_BOOTSTRAP_TOKEN` and
//! a password read from standard input, and prints the first authenticator's secret and the
//! recovery codes once to the operator's own terminal (SEC-044, SEC-007, D-1918). `server replay`
//! restores an operator's decrypted export into the fresh database and replays from the vendors'
//! records (`askhava_server::replay_drill`; OPS-013, OPS-018, OPS-019, T206).

use std::future::IntoFuture;
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
    if std::env::args().nth(1).as_deref() == Some("replay") {
        let args: Vec<String> = std::env::args().skip(2).collect();
        return askhava_server::replay_drill(&settings, std::env::vars().collect(), &args).await;
    }
    // The security-event cap and the SEC-040 client address, once per process (T319, D-3134).
    let client_ip = askhava_core::api::ClientIpPolicy::from_pairs(std::env::vars())
        .map_err(io::Error::other)?;
    askhava_portals::api::configure_security_events(
        settings.security_events_per_minute(),
        client_ip,
    );
    let port = port_from_env()?;
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await?;
    let pool = askhava_core::api::app_pool(&settings).map_err(io::Error::other)?;
    // Nothing is served under a key set missing a held index version (D-4418, D-4473).
    let vault = askhava_server::business_portal::vault_from_settings(&settings, std::env::vars())?;
    askhava_server::check_index_keys(&pool, vault.as_ref()).await?;
    askhava_server::catalog_start::store_catalog(&pool).await?;
    let control =
        TestControl::from_settings(&settings, std::env::vars()).map_err(io::Error::other)?;
    let webhooks = askhava_server::webhooks(&settings, pool.clone(), std::env::vars())?;
    let voice = askhava_server::voice_hooks(&settings, webhooks.as_ref(), std::env::vars())?;
    let pick_keys = askhava_server::pick_code_keys(&settings, std::env::vars())?;
    let stripe =
        askhava_server::release_sink::stripe_webhook(&settings, pool.clone(), std::env::vars())?;
    let limit = askhava_server::request_timeout::RequestTimeout::from_settings(&settings);
    let hooks = (webhooks, voice, pick_keys, stripe);
    let statics = askhava_server::static_files::StaticRoot::from_settings(&settings);
    let parts = business_parts(&settings, &pool, vault, (hooks, limit, statics).into())?;
    let watch = askhava_server::liveness::watch_web(pool.clone(), liveness_limit()?);
    let app = askhava_server::build_router(pool, control.router(), parts);
    let serve = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(askhava_server::shutdown_signal()?);
    tokio::select! {
        served = serve.into_future() => served,
        failed = watch => Err(failed),
    }
}

/// The liveness limit (OPS-049) and the `/healthz` thresholds (OPS-007), read once at start
/// (T106); a bad value stops start-up, naming its key.
fn liveness_limit() -> io::Result<askhava_server::liveness::LivenessLimit> {
    use askhava_server::healthz::{HealthLimits, configure};
    configure(HealthLimits::from_pairs(std::env::vars())?);
    askhava_server::liveness::LivenessLimit::from_pairs(std::env::vars())
}

/// `parts` with the served business sign-in, sign-up, portal `vault` (built and checked at start,
/// D-4418) and Confirm settings from `settings` and the process environment (T280, T302, T300,
/// T261), the step-up page's Verify part (T364), the homeowner sign-in's Verify part (T307), the web chat's code-step keys (T489) and the
/// site pages' settings (T102); each errors as its own builder does, naming a key, never a value.
fn business_parts(
    settings: &askhava_core::api::Settings,
    pool: &sqlx::PgPool,
    vault: Option<askhava_privacy::api::Vault>,
    parts: askhava_server::RouterParts,
) -> io::Result<askhava_server::RouterParts> {
    let signin = askhava_server::business_signin::ServedSignIn::from_settings(
        settings,
        pool.clone(),
        std::env::vars(),
    )?;
    let signup = askhava_server::business_signup::ServedSignUp::from_settings(
        pool.clone(),
        std::env::vars(),
    )?;
    let step_up = askhava_server::business_step_up::step_up_codes(settings, std::env::vars())?;
    let confirm = askhava_server::confirm_settings(settings, std::env::vars())?;
    let chat = askhava_server::chat_codes::chat_keys(settings, std::env::vars())?;
    let homeowner = askhava_server::homeowner_portal::homeowner_verify(settings, std::env::vars())?;
    let link_codes = askhava_server::action_links::confirm_link_codes(settings, std::env::vars())?;
    Ok(parts
        .with_sign_in(signin)
        .with_sign_up(signup)
        .with_step_up(step_up)
        .with_homeowner_verify(homeowner)
        .with_vault(vault)
        .with_business_number(askhava_server::business_number(settings.business_number())?)
        .with_confirm_part(confirm)
        .with_confirm_link_codes(link_codes)
        .with_chat_codes(chat)
        .with_site_pages(askhava_site::api::SitePages::from_settings(settings))
        .with_environment(settings.environment()))
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
/// line on standard input (not echoed on a terminal), over the migration role's URL (the app role
/// cannot insert an owner). A second run fails. The enrolment secrets go to standard output once, for Casey's own
/// terminal; errors name no value.
async fn bootstrap_owner(settings: &askhava_core::api::Settings) -> io::Result<()> {
    use askhava_portals::api::{BOOTSTRAP_TOKEN_VAR, bootstrap_owner};
    use secrecy::ExposeSecret;
    use sqlx::Connection;
    let token = std::env::var(BOOTSTRAP_TOKEN_VAR).ok();
    let password = read_password()?;
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

/// Reads one line of standard input. On a terminal the typing is not echoed: `stty -echo` before
/// the read and `stty echo` after, with std alone (no new crate, D-1918); a pipe is read as is.
fn read_password() -> io::Result<String> {
    use std::io::IsTerminal;
    let terminal = io::stdin().is_terminal();
    if terminal {
        io::Write::write_all(&mut io::stderr(), b"Owner password (not shown): ")?;
        set_echo(false)?;
    }
    let mut password = String::new();
    let read = io::stdin().read_line(&mut password);
    if terminal {
        set_echo(true)?;
        io::Write::write_all(&mut io::stderr(), b"\n")?;
    }
    read.map(|_| password)
}

fn set_echo(on: bool) -> io::Result<()> {
    let status = std::process::Command::new("stty")
        .arg(if on { "echo" } else { "-echo" })
        .stdin(std::process::Stdio::inherit())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("could not turn terminal echo off"))
    }
}

/// Reads the listening port from `PORT`. The error never echoes the value.
fn port_from_env() -> io::Result<u16> {
    let raw = std::env::var("PORT").map_err(|_| io::Error::other("PORT is not set"))?;
    raw.parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "PORT is not a port number"))
}
