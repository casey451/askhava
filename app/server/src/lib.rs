//! The composition root both binaries share (`docs/architecture.md` section 2; SEC-029, D-481).
//!
//! `server` (the `web` process) and `worker` call only what is here: the TLS
//! provider every outside connection uses, the settings and logging start-up,
//! the router, and the shutdown signal. Later tasks add each module's routes to [`build_router`] and its task
//! handlers to a `register_handlers()` beside it; nothing else in the app
//! knows about HTTP status codes or process signals.

use std::io;
use std::sync::Arc;

use askhava_adapters::api::{StorageAdapter, TwilioConfig};
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

pub mod inbound_text;
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
/// `webhooks` ([`webhooks`]) adds Twilio's signature-checked inbound-text and status routes
/// (`inbound_text`, SEC-016, SEC-039, T241); `None` adds none.
pub fn build_router(
    pool: PgPool,
    test_control: Router,
    webhooks: Option<Arc<inbound_text::Webhooks>>,
) -> Router {
    let quote_page = quote_page_routes(pool.clone(), Arc::new(SystemClock));
    let pick_flow = pick_flow_routes(pool.clone(), Arc::new(SystemClock));
    let twilio = webhooks.map_or_else(Router::new, inbound_text::routes);
    let (log, _writer) = offdb_stream::RequestLog::spawn(pool);
    Router::new()
        .merge(test_control)
        .merge(quote_page)
        .merge(pick_flow)
        .merge(twilio)
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

/// The keys of `intake`'s homeowner job-text sender (D-1302, D-1306): the `ID_SECRET` as the
/// quote-page link key (version 1) and the outbox key, and the vault built from `VAULT_KEY` and
/// `VAULT_INDEX_KEY` in `pairs` (a blank value counts as unset). With neither vault key it is
/// `None` in dev and CI (no sender is registered) and an error in the test and live projects;
/// one key without the other is an error everywhere, never a quiet "no sender".
///
/// # Errors
///
/// A weak `ID_SECRET`, a bad vault key, a half-set pair, or no vault keys in the test or live
/// project; the message names the missing key, never a value.
pub fn job_text_keys<I>(
    settings: &Settings,
    id_secret: &SecretString,
    pairs: I,
) -> io::Result<Option<JobTextKeys>>
where
    I: IntoIterator<Item = (String, String)>,
{
    let (seal, index) = match vault_keys(pairs) {
        (Some(seal), Some(index)) => (seal, index),
        (None, None) => {
            return match settings.environment() {
                Environment::Dev | Environment::Ci => Ok(None),
                Environment::Test | Environment::Live => {
                    Err(io::Error::other("missing VAULT_KEY and VAULT_INDEX_KEY"))
                }
            };
        }
        (Some(_), None) => return Err(io::Error::other("VAULT_KEY set without VAULT_INDEX_KEY")),
        (None, Some(_)) => return Err(io::Error::other("VAULT_INDEX_KEY set without VAULT_KEY")),
    };
    let vault = Vault::new(&seal, &index, settings.database_url()).map_err(io::Error::other)?;
    let link = IdSecret::new(1, id_secret.clone()).map_err(io::Error::other)?;
    Ok(Some(JobTextKeys::new(link, id_secret.clone(), vault)))
}

/// The `ID_SECRET` setting, read here for the webhook routes' outbox keys (SEC-018).
const ID_SECRET: &str = "ASKHAVA_ID_SECRET";

/// What Twilio's webhook routes need (T241, SEC-016, D-1216), from `settings` and the same
/// `pairs`: with `ASKHAVA_PUBLIC_BASE_URL` set, the Twilio auth token ([`TwilioConfig`]), the
/// `ID_SECRET` (checked as `job_text_keys` checks it, `IdSecret::new`) and both vault keys are
/// required, a half-set pair naming the missing key (D-1306); unset, `None` in dev and CI (no
/// routes) and an error in the test and live projects, which Twilio must reach.
///
/// # Errors
///
/// A missing or bad key, named, never its value.
pub fn webhooks<I>(
    settings: &Settings,
    pool: PgPool,
    pairs: I,
) -> io::Result<Option<Arc<inbound_text::Webhooks>>>
where
    I: IntoIterator<Item = (String, String)>,
{
    let Some(base) = settings.public_base_url() else {
        return match settings.environment() {
            Environment::Dev | Environment::Ci => Ok(None),
            Environment::Test | Environment::Live => {
                Err(io::Error::other("missing ASKHAVA_PUBLIC_BASE_URL"))
            }
        };
    };
    let pairs: Vec<(String, String)> = pairs.into_iter().collect();
    let twilio = TwilioConfig::from_settings(settings, pairs.clone()).map_err(io::Error::other)?;
    let id_secret = pairs
        .iter()
        .find(|(k, v)| k == ID_SECRET && !v.trim().is_empty())
        .map(|(_, v)| SecretString::from(v.trim().to_owned()))
        .ok_or_else(|| io::Error::other("missing ASKHAVA_ID_SECRET"))?;
    IdSecret::new(1, id_secret.clone()).map_err(io::Error::other)?;
    let (seal, index) = match vault_keys(pairs) {
        (Some(seal), Some(index)) => (seal, index),
        (None, None) => return Err(io::Error::other("missing VAULT_KEY and VAULT_INDEX_KEY")),
        (Some(_), None) => return Err(io::Error::other("VAULT_KEY set without VAULT_INDEX_KEY")),
        (None, Some(_)) => return Err(io::Error::other("VAULT_INDEX_KEY set without VAULT_KEY")),
    };
    let vault = Vault::new(&seal, &index, settings.database_url()).map_err(io::Error::other)?;
    Ok(Some(Arc::new(inbound_text::Webhooks {
        pool,
        clock: Arc::new(SystemClock),
        public_base: base.to_owned(),
        auth_token: twilio.auth_token().clone(),
        id_secret,
        vault,
        names: Vec::new(),
        intake: askhava_intake::api::rules::Settings::default(),
    })))
}

/// `VAULT_KEY` and `VAULT_INDEX_KEY` from `pairs`, each `None` when absent or blank.
fn vault_keys<I>(pairs: I) -> (Option<SecretString>, Option<SecretString>)
where
    I: IntoIterator<Item = (String, String)>,
{
    let (mut seal, mut index) = (None, None);
    for (key, value) in pairs.into_iter().filter(|(_, v)| !v.trim().is_empty()) {
        match key.as_str() {
            VAULT_KEY => seal = Some(SecretString::from(value)),
            VAULT_INDEX_KEY => index = Some(SecretString::from(value)),
            _ => {}
        }
    }
    (seal, index)
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

/// The job-text wiring (D-1302, D-1306): `job_text_keys` per environment and per half-set pair,
/// and `register_handlers` registering the sender under `jobs::api::TEXT_KIND` only with keys;
/// the webhook wiring (T241, D-1311): `webhooks` per environment and per missing key. Fake keys
/// are built at run time; the pool is lazy, so nothing here touches a database.
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use askhava_core::api::{Settings, SystemClock};
    use askhava_jobs::api::TEXT_KIND;
    use secrecy::SecretString;

    use crate::offdb_stream::OffDbStream;
    use crate::{job_text_keys, register_handlers, webhooks};

    const TEST_LIST: &str =
        "twilio:ACtestfake01,stripe:acct_testfake,resend:test.example.com,bucket:askhava-test";
    const LIVE_LIST: &str =
        "twilio:AClivefake01,stripe:acct_livefake,resend:mail.example.com,bucket:askhava-live";

    fn pairs(env: &str, extra: &[(&str, &str)]) -> Vec<(String, String)> {
        let base = [
            ("ASKHAVA_APP_ENV", env),
            ("ASKHAVA_DATABASE_URL", "postgres://fake@localhost/fake"),
            (
                "ASKHAVA_STORAGE_BASE_URL",
                "file:///nonexistent/askhava-fake",
            ),
        ];
        base.iter()
            .chain(extra)
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn settings(env: &str) -> Settings {
        let extra: &[(&str, &str)] = match env {
            "test" => &[("ASKHAVA_ALLOW_LIST_TEST", TEST_LIST)],
            "live" => &[
                ("ASKHAVA_ALLOW_LIST_TEST", TEST_LIST),
                ("ASKHAVA_ALLOW_LIST_LIVE", LIVE_LIST),
                ("ASKHAVA_TWILIO_ACCOUNT_SID", "AClivefake01"),
                ("ASKHAVA_STRIPE_ACCOUNT_ID", "acct_livefake"),
                ("ASKHAVA_RESEND_DOMAIN", "mail.example.com"),
                ("ASKHAVA_STORAGE_BUCKET", "askhava-live"),
            ],
            _ => &[],
        };
        Settings::from_pairs(pairs(env, extra)).expect("settings")
    }

    fn id_secret() -> SecretString {
        SecretString::from("test-only-id-secret-not-a-real-key-000".to_owned())
    }

    /// Two distinct fake 32-byte keys, version 1: all 0x00 and all 0x04.
    fn vault_pairs(seal: bool, index: bool) -> Vec<(String, String)> {
        let keys = [
            ("VAULT_KEY", format!("1:{}=", "A".repeat(43)), seal),
            (
                "VAULT_INDEX_KEY",
                format!("1:{}BAQ=", "BAQE".repeat(10)),
                index,
            ),
        ];
        keys.into_iter()
            .filter(|(_, _, on)| *on)
            .map(|(k, v, _)| (k.to_owned(), v))
            .collect()
    }

    fn keys(env: &str, seal: bool, index: bool) -> std::io::Result<bool> {
        job_text_keys(&settings(env), &id_secret(), vault_pairs(seal, index)).map(|k| k.is_some())
    }

    fn error(env: &str, seal: bool, index: bool) -> String {
        keys(env, seal, index).expect_err("refused").to_string()
    }

    #[test]
    fn no_vault_keys_is_no_sender_in_dev_and_ci_and_an_error_in_test_and_live() {
        for env in ["dev", "ci"] {
            assert!(!keys(env, false, false).expect("no sender"), "{env}");
        }
        for env in ["test", "live"] {
            assert_eq!(
                error(env, false, false),
                "missing VAULT_KEY and VAULT_INDEX_KEY"
            );
        }
    }

    #[test]
    fn a_half_set_pair_names_the_missing_key_in_every_environment() {
        for env in ["dev", "ci", "test", "live"] {
            assert_eq!(
                error(env, true, false),
                "VAULT_KEY set without VAULT_INDEX_KEY"
            );
            assert_eq!(
                error(env, false, true),
                "VAULT_INDEX_KEY set without VAULT_KEY"
            );
        }
        let blank = [("VAULT_KEY".to_owned(), " ".to_owned())];
        let index = vault_pairs(false, true).into_iter().chain(blank);
        let err = job_text_keys(&settings("dev"), &id_secret(), index).map(|k| k.is_some());
        assert_eq!(
            err.expect_err("refused").to_string(),
            "VAULT_INDEX_KEY set without VAULT_KEY"
        );
    }

    #[test]
    fn both_vault_keys_build_the_sender_keys() {
        for env in ["dev", "ci", "test", "live"] {
            assert!(keys(env, true, true).expect("keys"), "{env}");
        }
        let weak = SecretString::from("short".to_owned());
        assert!(job_text_keys(&settings("dev"), &weak, vault_pairs(true, true)).is_err());
    }

    #[test]
    fn the_sender_is_registered_under_text_kind_only_with_keys() {
        let dev = settings("dev");
        let stream = || {
            let clock = Arc::new(SystemClock);
            let all = pairs(
                "dev",
                &[(
                    "ASKHAVA_ID_SECRET",
                    "test-only-id-secret-not-a-real-key-000",
                )],
            );
            Arc::new(OffDbStream::from_settings(&dev, all, clock).expect("stream"))
        };
        let texts = job_text_keys(&dev, &id_secret(), vault_pairs(true, true))
            .expect("keys")
            .map(Arc::new);
        let with = format!("{:?}", register_handlers(stream(), texts));
        let without = format!("{:?}", register_handlers(stream(), None));
        assert!(with.contains(&format!("{TEXT_KIND:?}")), "{with}");
        assert!(!without.contains(&format!("{TEXT_KIND:?}")), "{without}");
    }

    const BASE: (&str, &str) = ("ASKHAVA_PUBLIC_BASE_URL", "https://hooks.example.test");
    const ID: (&str, &str) = (
        "ASKHAVA_ID_SECRET",
        "test-only-id-secret-not-a-real-key-000",
    );
    const TOKEN: (&str, &str) = ("ASKHAVA_TWILIO_AUTH_TOKEN", "fake_twilio_auth_token");
    /// Ten distinct fake numbers in the 555-01xx fiction range (CALL-017's pool size).
    const NUMBERS: &str = "+19285550101,+19285550102,+19285550103,+19285550104,+19285550105,\
        +19285550106,+19285550107,+19285550108,+19285550109,+19285550110";
    /// The Twilio keys `TwilioConfig::from_settings` needs in dev, but the auth token.
    const TWILIO: [(&str, &str); 3] = [
        ("ASKHAVA_TWILIO_ACCOUNT_SID", "ACtestfake01"),
        (
            "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
            "VA00000000000000000000000000000001",
        ),
        ("ASKHAVA_MASKED_NUMBERS", NUMBERS),
    ];

    /// `webhooks` over `env`'s fake pairs plus `extra` and the vault keys chosen: `Some` or not.
    fn hooks(env: &str, extra: &[(&str, &str)], vault: (bool, bool)) -> std::io::Result<bool> {
        let lists = settings_pairs(env);
        let all: Vec<(String, String)> = pairs(env, &[lists.as_slice(), extra].concat())
            .into_iter()
            .chain(vault_pairs(vault.0, vault.1))
            .collect();
        let settings = Settings::from_pairs(all.clone()).expect("settings");
        let pool = sqlx::PgPool::connect_lazy("postgres://fake@localhost/fake").expect("pool");
        webhooks(&settings, pool, all).map(|h| h.is_some())
    }

    /// The allow lists and accounts `settings(env)` adds, as pairs.
    fn settings_pairs(env: &str) -> Vec<(&'static str, &'static str)> {
        match env {
            "test" => vec![("ASKHAVA_ALLOW_LIST_TEST", TEST_LIST)],
            "live" => vec![
                ("ASKHAVA_ALLOW_LIST_TEST", TEST_LIST),
                ("ASKHAVA_ALLOW_LIST_LIVE", LIVE_LIST),
                ("ASKHAVA_TWILIO_ACCOUNT_SID", "AClivefake01"),
                ("ASKHAVA_STRIPE_ACCOUNT_ID", "acct_livefake"),
                ("ASKHAVA_RESEND_DOMAIN", "mail.example.com"),
                ("ASKHAVA_STORAGE_BUCKET", "askhava-live"),
            ],
            _ => Vec::new(),
        }
    }

    fn hooks_error(env: &str, extra: &[(&str, &str)], vault: (bool, bool)) -> String {
        hooks(env, extra, vault).expect_err("refused").to_string()
    }

    /// T241 (D-1216): with no public base URL, dev and CI build no webhook routes, whatever else
    /// is set; the test and live projects, which Twilio must reach, refuse to start.
    #[tokio::test]
    async fn no_public_base_url_is_no_routes_in_dev_and_ci_and_an_error_in_test_and_live() {
        for env in ["dev", "ci"] {
            assert!(!hooks(env, &[], (true, true)).expect("no routes"), "{env}");
        }
        for env in ["test", "live"] {
            assert_eq!(
                hooks_error(env, &[], (true, true)),
                "missing ASKHAVA_PUBLIC_BASE_URL"
            );
        }
    }

    /// T241 (SEC-016, SEC-018, D-1306, D-1311): with the base URL set the Twilio auth token, a
    /// strong `ID_SECRET` and both vault keys are required, each error naming the key.
    #[tokio::test]
    async fn with_a_public_base_url_every_key_is_required_and_named() {
        let twilio = [TWILIO.as_slice(), &[TOKEN]].concat();
        let full = [&[BASE, ID], twilio.as_slice()].concat();
        assert!(hooks("dev", &full, (true, true)).expect("routes"));
        let no_token = hooks_error(
            "dev",
            &[&[BASE, ID], TWILIO.as_slice()].concat(),
            (true, true),
        );
        assert!(no_token.contains("ASKHAVA_TWILIO_AUTH_TOKEN"), "{no_token}");
        let no_id = [&[BASE], twilio.as_slice()].concat();
        assert_eq!(
            hooks_error("dev", &no_id, (true, true)),
            "missing ASKHAVA_ID_SECRET"
        );
        let weak = [&[BASE, ("ASKHAVA_ID_SECRET", "short")], twilio.as_slice()].concat();
        assert_eq!(
            hooks_error("dev", &weak, (true, true)),
            "the ID secret is too short or has no version"
        );
        let cases = [
            ((false, false), "missing VAULT_KEY and VAULT_INDEX_KEY"),
            ((true, false), "VAULT_KEY set without VAULT_INDEX_KEY"),
            ((false, true), "VAULT_INDEX_KEY set without VAULT_KEY"),
        ];
        for (vault, message) in cases {
            assert_eq!(hooks_error("dev", &full, vault), message);
        }
    }
}
