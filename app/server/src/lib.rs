//! The composition root both binaries share (`docs/architecture.md` section 2; SEC-029, D-481).
//!
//! `server` (the `web` process) and `worker` call only what is here: the TLS
//! provider every outside connection uses, the settings and logging start-up,
//! the router, and the shutdown signal. Later tasks add each module's routes to [`build_router`] and its task
//! handlers to a `register_handlers()` beside it; nothing else in the app
//! knows about HTTP status codes or process signals.

use std::io;
use std::sync::{Arc, OnceLock};

use askhava_adapters::api::{
    LicenseLists, LocalStorage, StorageAdapter, TwilioConfig, TwilioMessageReader, TwilioSms,
    TwilioVerify, twilio_clients,
};
use askhava_core::api::{
    Brand, Clock, Environment, IdSecret, LogGuard, Registry, Settings, SystemClock, init_logging,
    install_brand, request_span,
};
use askhava_intake::api::JobTextKeys;
use askhava_payments::api::{Minter, stripe_webhook_routes};
use askhava_portals::api::{
    PickCodeKeys, feedback_routes, pick_flow_routes, quote_page_routes, security_headers,
};
use askhava_privacy::api::{SessionMint, Vault};
use axum::Router;
use axum::body::Body;
use secrecy::SecretString;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tokio::signal::unix::{SignalKind, signal};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

pub mod business_contacts;
pub mod business_signin;
pub mod call_status;
pub mod catalog_start;
pub mod checkin;
pub mod inbound_retry;
pub mod inbound_text;
pub mod inbound_voice;
pub mod no_handler_alert;
pub mod offdb_stream;
pub mod payment_jobs;
pub mod release_sink;
pub mod request_timeout;
pub mod static_files;
pub mod voice_status;
pub mod webhook_refusals;

use request_timeout::RequestTimeout;
use static_files::StaticRoot;

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

/// Checks and installs the compiled-in brand file before anything renders (WORD-001, D-1932),
/// then reads the settings from the process environment and starts the JSON log
/// and Sentry, so a bad configuration stops the process before it serves or
/// claims anything (SEC-002, SEC-036, PRIV-016, PRIV-017). Hold the guard
/// for the life of `main`.
///
/// # Errors
///
/// A bad brand field (the error names the field), or any settings or logging error; its message
/// names a key, never a value.
pub fn start_up() -> io::Result<(Settings, LogGuard)> {
    let brand = Brand::embedded().map_err(io::Error::other)?;
    install_brand(brand).map_err(io::Error::other)?;
    let settings = Settings::from_pairs(std::env::vars()).map_err(io::Error::other)?;
    let guard = init_logging(&settings).map_err(io::Error::other)?;
    Ok((settings, guard))
}

/// `build_router`'s optional parts: Twilio's text and voice webhooks, the page pick's keys and the
/// Stripe webhook.
pub type Hooks = (
    Option<Arc<inbound_text::Webhooks>>,
    Option<Arc<inbound_voice::VoiceHooks>>,
    Option<PickCodeKeys<TwilioVerify>>,
    Option<release_sink::StripeHooks>,
);

/// `build_router`'s third argument: the optional [`Hooks`], the whole-request time limit
/// (D-1870) and the built files' folder (D-2633). `main` passes `(hooks,
/// RequestTimeout::from_settings(&settings), StaticRoot::from_settings(&settings))`; bare
/// [`Hooks`] take [`RequestTimeout::DEFAULT`], and both shorter forms the image's folder.
pub struct RouterParts {
    hooks: Hooks,
    limit: RequestTimeout,
    statics: StaticRoot,
    signin: Option<business_signin::ServedSignIn>,
}

impl RouterParts {
    /// Adds the served business sign-in pages (T280, D-3055); without it none are served.
    #[must_use]
    pub fn with_sign_in(mut self, signin: business_signin::ServedSignIn) -> Self {
        self.signin = Some(signin);
        self
    }
}

impl From<Hooks> for RouterParts {
    fn from(hooks: Hooks) -> Self {
        Self {
            hooks,
            limit: RequestTimeout::DEFAULT,
            statics: StaticRoot::default(),
            signin: None,
        }
    }
}

impl From<(Hooks, RequestTimeout)> for RouterParts {
    fn from((hooks, limit): (Hooks, RequestTimeout)) -> Self {
        let statics = StaticRoot::default();
        Self {
            hooks,
            limit,
            statics,
            signin: None,
        }
    }
}

impl From<(Hooks, RequestTimeout, StaticRoot)> for RouterParts {
    fn from((hooks, limit, statics): (Hooks, RequestTimeout, StaticRoot)) -> Self {
        Self {
            hooks,
            limit,
            statics,
            signin: None,
        }
    }
}

/// The app's router: every module's routes, merged once here (D-481), under
/// the request-ID and tracing layers, so every request log line carries the
/// request ID and the matched route template, never the URI (PRIV-016,
/// PRIV-028), and every request hands its request-log line to a background writer that queues it
/// for the off-database stream over `pool`, so no response waits on the database (OPS-011, D-734,
/// D-737). Call it inside the runtime (it spawns that writer). Merge routes before the layers so
/// they apply to them. `test_control` is `askhava_core::api::TestControl::router()`, which has no
/// routes in the release build (SEC-035, D-739). The outermost layer sets the SEC-012 security
/// headers on every response, a 404 included (T051). The parts' served business sign-in
/// ([`business_signin::ServedSignIn`], T280) adds the two sign-in routes, always with the
/// new-device code. The homeowner's quote page is `portals`'
/// (`/q/{token}`, T052); its pick review step posts to the same path (`pick_flow`, T055); the
/// private feedback page after Done is `/f/{token}` (`feedback`, T230); the check-in answer page
/// the emailed check-in's buttons open is `/c/{token}/{answer}` ([`checkin`], T231). The built
/// browser files are served at `/static/<crate>/` from the parts' folder ([`static_files`], T362).
/// `webhooks` ([`webhooks`]) adds Twilio's signature-checked inbound-text and status routes
/// (`inbound_text`, SEC-016, SEC-039, T241) and the placed calls' status and safety-key routes
/// (`voice_status`, CALL-022, OPS-028, T378); `None` adds none. `voice` ([`voice_hooks`]) adds the
/// pool numbers' signature-checked voice webhook (`inbound_voice`, SEC-016, SEC-017, T262) and
/// the dialed leg's status callbacks (`call_status`, CALL-008, T271), which applies the refusal
/// layer itself (D-3525).
/// `pick_keys` ([`pick_code_keys`])
/// let the page pick send its code and store the pick (T246, D-1516); `None` keeps the page's
/// "Nothing was picked" lines. `stripe` ([`release_sink::stripe_webhook`]) adds `payments`' Stripe
/// webhook route, whose cleared picks [`release_sink::PickReleaser`] settles, mints and releases
/// (PICK-022, SEC-039, D-1480, D-1739); serve with connect info, so the socket peer is known
/// (SEC-040). `None` adds none. Inside the request-ID and security-headers layers, every route
/// runs under the whole-request time limit (`request_timeout`, 503 with `Retry-After`, D-1870)
/// and, innermost, the panic layer (500 naming no panic message, D-2208).
pub fn build_router(pool: PgPool, test_control: Router, parts: impl Into<RouterParts>) -> Router {
    let RouterParts {
        hooks,
        limit,
        statics,
        signin,
    } = parts.into();
    let (webhooks, voice, pick_keys, stripe) = hooks;
    let quote_page = quote_page_routes(pool.clone(), Arc::new(SystemClock));
    let pick_flow = pick_flow_routes(pool.clone(), Arc::new(SystemClock), pick_keys);
    let feedback = feedback_routes(pool.clone(), Arc::new(SystemClock));
    let checkin = checkin::routes(pool.clone());
    // Each 403 of these four is a source or signature refusal, recorded as a capped
    // `webhook_signature_failed` security event (SEC-048, SEC-016, T319).
    let refusals = webhook_refusals::RefusalLog::new(pool.clone(), Arc::new(SystemClock));
    let refused = |router| webhook_refusals::layer(router, refusals.clone());
    let calls = webhooks
        .clone()
        .map_or_else(Router::new, |h| refused(voice_status::routes(h)));
    let twilio = webhooks.map_or_else(Router::new, |h| refused(inbound_text::routes(h)));
    let dialed = voice.as_ref().map_or_else(Router::new, |v| {
        call_status::routes(Arc::clone(&v.hooks), refusals.clone())
    });
    let voice = voice.map_or_else(Router::new, |v| refused(inbound_voice::routes(v)));
    let stripe = stripe.map_or_else(Router::new, |s| refused(stripe_webhook_routes(s)));
    let (log, _writer) = offdb_stream::RequestLog::spawn(pool);
    Router::new()
        .merge(test_control)
        .merge(quote_page)
        .merge(pick_flow)
        .merge(feedback)
        .merge(checkin)
        .merge(twilio)
        .merge(voice)
        .merge(dialed)
        .merge(calls)
        .merge(stripe)
        .merge(signin.map_or_else(Router::new, business_signin::ServedSignIn::into_router))
        .merge(static_files::router(&statics))
        .layer(request_timeout::catch_panic())
        .layer(axum::middleware::from_fn_with_state(
            limit,
            request_timeout::limit_request,
        ))
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
    let Some(vault) = vault_from(settings, pairs)? else {
        return Ok(None);
    };
    let link = IdSecret::new(1, id_secret.clone()).map_err(io::Error::other)?;
    Ok(Some(JobTextKeys::new(link, id_secret.clone(), vault)))
}

/// The published homeowner Terms a page pick accepts: the exact text of
/// `docs/legal/homeowner-terms.md`, compiled in, whose second line names its version (TERMS-016).
const HOMEOWNER_TERMS: &str = include_str!("../../../docs/legal/homeowner-terms.md");

/// The version `HOMEOWNER_TERMS` names on its "Version <v>" line, or `unversioned`. `pub(crate)`
/// so this file's own tests still find it when `tests/integration/tests/task_registry.rs` compiles
/// the file in place (T327).
pub(crate) fn terms_version() -> String {
    HOMEOWNER_TERMS
        .lines()
        .find_map(|l| l.strip_prefix("Version "))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or("unversioned")
        .to_owned()
}

/// The page pick's keys (T246, D-1516), built from `settings` and `pairs` as [`job_text_keys`]
/// builds the sender's (D-1302): with both vault keys, the Verify client from
/// `adapters::api::twilio_clients`, the vault, a strong `ASKHAVA_ID_SECRET` and the compiled-in
/// homeowner Terms; with neither, `None` in dev and CI (the page sends nothing and says so) and an
/// error in the test and live projects; one key without the other is an error everywhere.
///
/// # Errors
///
/// A missing or bad key, named, never its value.
pub fn pick_code_keys<I>(
    settings: &Settings,
    pairs: I,
) -> io::Result<Option<PickCodeKeys<TwilioVerify>>>
where
    I: IntoIterator<Item = (String, String)>,
{
    let pairs: Vec<(String, String)> = pairs.into_iter().collect();
    let Some(vault) = vault_from(settings, pairs.clone())? else {
        return Ok(None);
    };
    let id_secret = pairs
        .iter()
        .find(|(k, v)| k == ID_SECRET && !v.trim().is_empty())
        .map(|(_, v)| SecretString::from(v.trim().to_owned()))
        .ok_or_else(|| io::Error::other("missing ASKHAVA_ID_SECRET"))?;
    IdSecret::new(1, id_secret.clone()).map_err(io::Error::other)?;
    let twilio = TwilioConfig::from_settings(settings, pairs).map_err(io::Error::other)?;
    let (_, _, verify) = twilio_clients(&twilio).map_err(io::Error::other)?;
    let terms = (terms_version(), HOMEOWNER_TERMS);
    Ok(Some(PickCodeKeys::new(verify, vault, id_secret, terms)))
}

/// The vault from `VAULT_KEY` and `VAULT_INDEX_KEY` in `pairs` (D-1302, D-1306): with neither,
/// `None` in dev and CI and an error in the test and live projects; one without the other is an
/// error everywhere.
pub(crate) fn vault_from<I>(settings: &Settings, pairs: I) -> io::Result<Option<Vault>>
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
    Vault::new(&seal, &index, settings.database_url())
        .map(Some)
        .map_err(io::Error::other)
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
        environment: settings.environment(),
        intake: askhava_intake::api::rules::Settings::default(),
    })))
}

/// The voice webhook's state (T262, D-1755): with `webhooks`, the masked pool from the Twilio
/// settings in `pairs` and the PIN wait (`ASKHAVA_PIN_WAIT_SECONDS`); `None` without `webhooks`.
///
/// # Errors
///
/// A bad Twilio setting or PIN wait, named, never its value.
pub fn voice_hooks<I>(
    settings: &Settings,
    webhooks: Option<&Arc<inbound_text::Webhooks>>,
    pairs: I,
) -> io::Result<Option<Arc<inbound_voice::VoiceHooks>>>
where
    I: IntoIterator<Item = (String, String)>,
{
    let Some(hooks) = webhooks else {
        return Ok(None);
    };
    let pairs: Vec<(String, String)> = pairs.into_iter().collect();
    let twilio = TwilioConfig::from_settings(settings, pairs.clone()).map_err(io::Error::other)?;
    let wait = askhava_calls::api::PinWait::from_pairs(pairs).map_err(io::Error::other)?;
    Ok(Some(Arc::new(inbound_voice::VoiceHooks {
        hooks: Arc::clone(hooks),
        pool: twilio.pool().clone(),
        wait,
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

/// `intake`'s no-reply reminder and close (INT-019) and the emergency address asked again on
/// `picks.address_needed` (INT-029, T255), both with the `ID_SECRET` as outbox key.
fn register_intake_asks(registry: &mut Registry, id_secret: &SecretString) {
    let secret = id_secret.clone();
    registry.register(askhava_intake::api::NO_REPLY_KIND, move |pool, task| {
        askhava_intake::api::run_no_reply(pool, secret.clone(), task)
    });
    let asked = id_secret.clone();
    registry.register(
        askhava_picks::api::ADDRESS_NEEDED_KIND,
        move |pool, task| askhava_intake::api::run_address_needed(pool, asked.clone(), task),
    );
}

/// Every module's `core.task` handlers, one `register` call per kind (D-481, D-491). The worker
/// runs what it returns; tests hold it in a `OnceLock`. A kind with no handler is failed with
/// `NO_HANDLER` and dead-lettered after its attempts (JOB-006). `stream` is the off-database
/// stream writer and its nightly check (OPS-011, PRIV-029, D-734); `jobs` runs the job status
/// timers (JOB-003, JOB-006); `quotes` the every-window-ended check (TXT-042, JOB-025); `intake` the
/// no-reply reminder and close (INT-019), the card approval auto-send (INT-041), the emergency
/// address asked again on `picks.address_needed` (INT-029, T255); `picks` the FEE-020
/// card-fix notice on `payments`' event (D-492) and the ended pick's next quote on
/// `payments.pick_ended` (FEE-018, FEE-041, PICK-046, T328, on the stream's clock), and with `confirm` (read once from
/// `ASKHAVA_PICK_CONFIRM_WINDOW_MINUTES`) the pick notice (PICK-037, D-1503) and the Confirm
/// window's end (PICK-019); `intake` the accepted-quote email after a Confirm, still with no sender
/// although the footer postal setting now exists, so it sends nothing until a follow-up builds
/// one (PICK-042, D-3077, D-3098 (2)); `payments` the pause watch that stops and resumes
/// the payment timers with the charging pause (FEE-038, D-1427); and, with
/// `texts` ([`job_text_keys`]), the homeowner job texts `jobs` and `picks` queue (JOB-010 to
/// JOB-012, TXT-042, PICK-046, D-1030 (A)) on the stream's clock. It takes the release mint first, through [`release_minter`], so a second
/// minter stops the start rather than a payment (SEC-030, D-488).
#[must_use]
pub fn register_handlers<S>(
    stream: Arc<offdb_stream::OffDbStream<S>>,
    texts: Option<Arc<JobTextKeys>>,
    confirm: askhava_picks::api::ConfirmSettings,
) -> Registry
where
    S: StorageAdapter + Send + Sync + 'static,
{
    release_minter();
    let clock = Arc::clone(&stream.clock);
    let mut registry = Registry::new();
    register_intake_asks(&mut registry, &stream.id_secret);
    registry.register(
        askhava_intake::api::CARD_TIMER_KIND,
        askhava_intake::api::run_card_timer,
    );
    offdb_stream::register(&mut registry, stream);
    registry.register(askhava_jobs::api::TIMER_KIND, askhava_jobs::api::run_timer);
    registry.register(
        askhava_jobs::api::CHECK_IN_KIND,
        askhava_jobs::api::run_check_in,
    );
    registry.register(
        askhava_quotes::api::WINDOWS_KIND,
        askhava_quotes::api::run_windows_end,
    );
    registry.register(
        askhava_payments::api::CARD_FIX_KIND,
        askhava_picks::api::run_card_fix_notice,
    );
    registry.register(
        askhava_payments::api::RECEIPT_KIND,
        askhava_picks::api::run_receipt_notice,
    );
    let ended_clock = Arc::clone(&clock);
    registry.register(askhava_payments::api::PICK_ENDED_KIND, move |pool, task| {
        let now = ended_clock.now();
        askhava_picks::api::run_pick_ended(pool, task, now)
    });
    register_pick_notices(&mut registry, confirm);
    let pause_clock = Arc::clone(&clock);
    registry.register(askhava_payments::api::PAUSE_KIND, move |pool, _task| {
        let now = pause_clock.now();
        async move { askhava_payments::api::run_pause_watch(&pool, now).await }
    });
    register_business_handlers(&mut registry);
    register_accepted_quote(&mut registry);
    register_purge_handlers(&mut registry, &clock);
    if let Some(keys) = texts {
        let text_clock = Arc::clone(&clock);
        registry.register(askhava_jobs::api::TEXT_KIND, move |pool, task| {
            let now = text_clock.now();
            askhava_intake::api::run_job_text_at(pool, Arc::clone(&keys), task, now)
        });
    }
    registry
}

/// `picks`' notices to the business: the pick notice with the worker's Confirm window and its end
/// (PICK-019, PICK-037, T063) and the withdrawn-pick notice (PICK-032, CON-029, T330).
fn register_pick_notices(registry: &mut Registry, confirm: askhava_picks::api::ConfirmSettings) {
    registry.register(askhava_picks::api::PICK_MADE_KIND, move |pool, task| {
        askhava_picks::api::run_pick_notice(pool, confirm, task)
    });
    registry.register(
        askhava_picks::api::CONFIRM_ENDS_KIND,
        askhava_picks::api::run_confirm_ends,
    );
    registry.register(
        askhava_picks::api::PICK_WITHDRAWN_KIND,
        askhava_picks::api::run_pick_withdrawn,
    );
}

/// Registers `jobs`' daily timer backstop (`jobs.timer_backstop`) on the worker clock: it revives
/// a non-final job's dead or missing timer (JOB-003, JOB-006, OPS-004, PRIV-007, T366, D-2640).
fn register_timer_backstop(registry: &mut Registry, clock: Arc<dyn Clock>) {
    registry.register(askhava_jobs::api::BACKSTOP_KIND, move |pool, _task| {
        let now = clock.now();
        async move { askhava_jobs::api::run_backstop(&pool, now).await }
    });
}

/// Registers `businesses`' daily license-map re-apply (`businesses.license_reapply`, T372,
/// BIZ-050, D-2705) on the worker clock; `worker.rs`'s `schedule_start` seeds it.
fn register_license_reapply(registry: &mut Registry, clock: Arc<dyn Clock>) {
    registry.register(askhava_businesses::api::REAPPLY_KIND, move |pool, _task| {
        let now = clock.now();
        async move { askhava_businesses::api::run_reapply(&pool, now).await }
    });
}

/// `businesses`' active ROC businesses with their checked numbers, for the daily list run's
/// lookups (LIC-039, D-2704; licensing may not call `businesses`).
struct ActiveRoc;

impl askhava_licensing::api::ActiveLicenses for ActiveRoc {
    async fn active_after(
        &self,
        conn: &mut sqlx::PgConnection,
        after: Option<Uuid>,
        limit: u32,
    ) -> Result<Vec<(Uuid, String)>, askhava_licensing::api::ImportError> {
        askhava_businesses::api::active_license_numbers(conn, after, limit)
            .await
            .map_err(|_| askhava_licensing::api::ImportError::Database)
    }
}

/// Registers `licensing`'s daily ROC and city list run (`licensing.daily_lists`, T371, D-2704,
/// LIC-007, LIC-039) on the worker clock over `ASKHAVA_LICENSE_BASE_URL` with the
/// `DailyListSettings` in `pairs`; `worker.rs`'s `schedule_start` seeds it. Without the base URL
/// a run does nothing and queues no next run in dev and CI; in test and live it is a start error
/// (D-2954).
fn register_daily_lists(
    registry: &mut Registry,
    settings: &Settings,
    clock: Arc<dyn Clock>,
    pairs: Vec<(String, String)>,
) -> io::Result<()> {
    let lists =
        askhava_licensing::api::DailyListSettings::from_pairs(pairs).map_err(io::Error::other)?;
    let source = match settings.license_base_url() {
        Some(url) => Some(LicenseLists::new(url).map_err(io::Error::other)?),
        None => match settings.environment() {
            Environment::Dev | Environment::Ci => None,
            Environment::Test | Environment::Live => {
                return Err(io::Error::other(
                    "the daily license lists need ASKHAVA_LICENSE_BASE_URL",
                ));
            }
        },
    };
    let source = Arc::new(source);
    registry.register(
        askhava_licensing::api::DAILY_LISTS_KIND,
        move |pool, task| {
            let (source, now) = (Arc::clone(&source), clock.now());
            async move {
                let Some(source) = source.as_ref() else {
                    return Ok(());
                };
                let parts = (source, &ActiveRoc);
                askhava_licensing::api::run_daily_list(&pool, parts, &task, (now, lists))
                    .await
                    .map(|_| ())
            }
        },
    );
    Ok(())
}

/// The production worker's whole registry (T327, D-2285): [`register_handlers`], then what only
/// the worker adds: `privacy`'s sealed photo delete over the stream's store (PRIV-009, T276),
/// `messaging`'s paused-emergency follow-up on the call list (TXT-023, T279), with the footer
/// postal setting (required in test and live) `messaging`'s business notices and charge receipts
/// ([`register_notice_senders`], T313), the hourly
/// dead-letter check ([`no_handler_alert`], OPS-004), `jobs`' daily timer backstop
/// ([`register_timer_backstop`], T366), `businesses`' daily license-map re-apply
/// ([`register_license_reapply`], T372), `licensing`'s daily ROC and city list run
/// ([`register_daily_lists`], T371) and, with the Twilio settings in `pairs`
/// (required in test and live, D-1688), `messaging`'s day-31 body delete (PRIV-027), `picks`'
/// masked-line retry (CALL-001), the safety voice call ([`register_safety_call`], T330, D-3134)
/// and, with the vault keys too, `messaging`'s outbox sender
/// (`messaging.send_due`, [`outbox_sender`], T356, D-2628); with both vault keys, `intake`'s card
/// sender ([`register_card_sender`], T251); `intake`'s opt-out event
/// ([`register_opted_out_live`], T330); and `messaging`'s opt-out follow-up emails
/// ([`register_optout_followup`], T357); and the seven Stripe `payments` jobs over the same Stripe
/// client ([`payment_jobs`], T433). `worker.rs` runs it and `tests/integration/tests/task_registry.rs`
/// checks every queued kind against it (TD-OPS-36).
///
/// # Errors
///
/// A missing or bad Twilio or postal-address setting in test or live, a postal address that is not
/// a P.O. box or private mailbox, or a Twilio client that cannot be built, named, never its value.
pub fn worker_registry<I>(
    settings: &Settings,
    stream: offdb_stream::OffDbStream<LocalStorage>,
    (texts, confirm): (
        Option<Arc<JobTextKeys>>,
        askhava_picks::api::ConfirmSettings,
    ),
    pairs: I,
) -> io::Result<Registry>
where
    I: IntoIterator<Item = (String, String)>,
{
    let clock = Arc::clone(&stream.clock);
    let (photos, id_secret) = (Arc::new(stream.storage.clone()), stream.id_secret.clone());
    let senders = notice_senders(settings, &id_secret)?;
    let mut registry = register_handlers(Arc::new(stream), texts, confirm);
    if let Some(senders) = senders {
        register_notice_senders(&mut registry, senders, (Arc::clone(&clock), confirm));
    }
    let photo_clock = Arc::clone(&clock);
    askhava_privacy::api::register_photo_deletes(
        &mut registry,
        photos,
        id_secret.clone(),
        photo_clock,
    );
    register_paused_safety(&mut registry);
    no_handler_alert::register(&mut registry, settings.environment(), Arc::clone(&clock));
    register_timer_backstop(&mut registry, Arc::clone(&clock));
    register_license_reapply(&mut registry, Arc::clone(&clock));
    let pairs: Vec<(String, String)> = pairs.into_iter().collect();
    register_daily_lists(&mut registry, settings, Arc::clone(&clock), pairs.clone())?;
    register_card_sender(&mut registry, settings, &id_secret, pairs.clone())?;
    register_opted_out_live(&mut registry, settings, &id_secret, pairs.clone())?;
    register_optout_followup(&mut registry, settings, &id_secret, pairs.clone())?;
    let stripe = stripe_client(settings, &pairs)?;
    let parts = (&id_secret, &clock);
    register_unreachable(
        &mut registry,
        settings,
        parts,
        stripe.clone(),
        pairs.clone(),
    )?;
    payment_jobs::register(&mut registry, stripe, id_secret.clone(), Arc::clone(&clock));
    register_whos_calling(&mut registry, settings, (&id_secret, &clock), pairs.clone())?;
    register_twilio_handlers(&mut registry, settings, (id_secret, clock), pairs)?;
    Ok(registry)
}

/// The handlers that need the Twilio settings: the inbound retry, the safety call, the outbox
/// sender (with both vault keys), the body deletes and the masked-line handlers. With no Twilio
/// settings (dev and CI only) none is registered.
fn register_twilio_handlers(
    registry: &mut Registry,
    settings: &Settings,
    (id_secret, clock): (SecretString, Arc<dyn Clock>),
    pairs: Vec<(String, String)>,
) -> io::Result<()> {
    let Some(twilio) = twilio_config(settings, pairs.clone())? else {
        return Ok(());
    };
    register_inbound_retry(
        registry,
        settings,
        (&twilio, &id_secret, &clock),
        pairs.clone(),
    )?;
    register_safety_call(
        registry,
        settings,
        (&twilio, &id_secret, &clock),
        pairs.clone(),
    )?;
    let (sms, _, _) = twilio_clients(&twilio).map_err(io::Error::other)?;
    let sms = with_status_url(settings, sms);
    if let Some(vault) = vault_from(settings, pairs.clone())? {
        let keys = (id_secret.clone(), vault);
        let sender = outbox_sender(settings, keys, sms.clone(), (&pairs, Arc::clone(&clock)))?;
        askhava_messaging::api::register_send_due(registry, Arc::new(sender));
    }
    let (sms, scan_clock) = (Arc::new(sms), Arc::clone(&clock));
    askhava_messaging::api::register_body_deletes(registry, sms, id_secret, scan_clock);
    let masked = (twilio.pool().clone(), clock);
    register_masked(registry, settings, masked, pairs)?;
    Ok(())
}

/// `messaging`'s paused-emergency follow-up on the call list only (TXT-023, T279, D-2186).
fn register_paused_safety(registry: &mut Registry) {
    let safety = Arc::new(askhava_messaging::api::SafetySender::call_list_only());
    registry.register(
        askhava_messaging::api::PAUSED_SAFETY_KIND,
        move |pool, task| {
            askhava_messaging::api::run_paused_safety(pool, Arc::clone(&safety), task)
        },
    );
}

/// The handlers that open a masked line: `picks.line_retry` (`picks::api::retry_line`, D-1688,
/// T254) and, with both vault keys, `intake.address_in`'s re-release (T255, D-1739).
fn register_masked(
    registry: &mut Registry,
    settings: &Settings,
    (masked, clock): (askhava_adapters::api::MaskedPool, Arc<dyn Clock>),
    pairs: Vec<(String, String)>,
) -> io::Result<()> {
    let at = Arc::clone(&clock);
    release_sink::register_address_in(registry, settings, (masked.clone(), at), pairs)?;
    registry.register(askhava_picks::api::LINE_RETRY_KIND, move |pool, task| {
        askhava_picks::api::retry_line(pool, masked.clone(), task, clock.now())
    });
    Ok(())
}

/// T330 (CON-028, CON-029, D-2288): `intake.opted_out_live_job` ([`askhava_intake::api::run_opted_out_live`]),
/// always registered, so a CANCEL's waiting picks are withdrawn; the CON-028 email's sender only
/// with the footer postal line and both vault keys (required in test and live), else none.
fn register_opted_out_live(
    registry: &mut Registry,
    settings: &Settings,
    id_secret: &SecretString,
    pairs: Vec<(String, String)>,
) -> io::Result<()> {
    use askhava_intake::api::{OPTED_OUT_LIVE_KIND, StillOpenSender, run_opted_out_live};
    let sender = match email_parts(settings, pairs)? {
        Some((postal, vault)) => Some(Arc::new(
            StillOpenSender::new(id_secret.clone(), postal, vault).map_err(|_| bad_postal())?,
        )),
        None => None,
    };
    registry.register(OPTED_OUT_LIVE_KIND, move |pool, task| {
        run_opted_out_live(pool, sender.clone(), task)
    });
    Ok(())
}

/// T357 (CON-016, CON-030, D-2629, D-3118): `messaging.optout_followup`
/// ([`askhava_messaging::api::run_optout_followup`]), always registered (TD-OPS-36); its
/// `OptOutEmailSender` is built from `ASKHAVA_ID_SECRET`, the footer postal line and both vault
/// keys only when all are set, else none and each run sends nothing and is done (dev and CI).
fn register_optout_followup(
    registry: &mut Registry,
    settings: &Settings,
    id_secret: &SecretString,
    pairs: Vec<(String, String)>,
) -> io::Result<()> {
    use askhava_messaging::api::{OPTOUT_FOLLOWUP_KIND, OptOutEmailSender, run_optout_followup};
    let sender = match email_parts(settings, pairs)? {
        Some((postal, vault)) => Some(Arc::new(
            OptOutEmailSender::new(id_secret.clone(), postal, vault).map_err(|_| bad_postal())?,
        )),
        None => None,
    };
    registry.register(OPTOUT_FOLLOWUP_KIND, move |pool, task| {
        run_optout_followup(pool, sender.clone(), task)
    });
    Ok(())
}

/// The footer postal line with its mailbox kind (a P.O. box when it reads as one, else a private
/// mailbox) and the vault, for the opt-out emails' senders (CON-028, CON-030, D-3463); `None`
/// unless `ASKHAVA_POSTAL_ADDRESS` and both vault keys are set.
fn email_parts(
    settings: &Settings,
    pairs: Vec<(String, String)>,
) -> io::Result<Option<((&str, askhava_messaging::api::MailboxKind), Vault)>> {
    use askhava_messaging::api::{MailboxKind, PostalAddress};
    let vault = vault_from(settings, pairs)?;
    Ok(match (settings.postal_address(), vault) {
        (Ok(postal), Some(vault)) => {
            let kind = match PostalAddress::new(postal, MailboxKind::PoBox) {
                Ok(_) => MailboxKind::PoBox,
                Err(_) => MailboxKind::PrivateMailbox,
            };
            Some(((postal, kind), vault))
        }
        _ => None,
    })
}

/// The start error for a postal line a sender refuses, naming the key, never its value.
fn bad_postal() -> io::Error {
    io::Error::other("setting ASKHAVA_POSTAL_ADDRESS has an invalid value")
}

/// T281 (FEE-022, FEE-052, TXT-030, D-1845, D-3490): `picks.unreachable_step`,
/// `picks.fraud_warning` and T314's `picks.dispute_opened` (FEE-050, D-2204), always registered, over `picks::api::StepParts`: the worker's Stripe client
/// ([`stripe_client`]: unset in dev and CI, required in test and live), `ID_SECRET`, the vault and the footer postal line when set; a step
/// that needs a missing part fails and is retried, then dead-lettered with its alert.
fn register_unreachable(
    registry: &mut Registry,
    settings: &Settings,
    (id_secret, clock): (&SecretString, &Arc<dyn Clock>),
    stripe: Option<askhava_adapters::api::StripePayments>,
    pairs: Vec<(String, String)>,
) -> io::Result<()> {
    use askhava_messaging::api::{MailboxKind, PostalAddress};
    use askhava_picks::api::{
        StepParts, UNREACHABLE_STEP_KIND, run_dispute_opened, run_fraud_warning,
    };
    let postal = settings.postal_address().ok().and_then(|line| {
        PostalAddress::new(line, MailboxKind::PoBox)
            .or_else(|_| PostalAddress::new(line, MailboxKind::PrivateMailbox))
            .ok()
    });
    let parts = Arc::new(StepParts {
        stripe,
        secret: id_secret.clone(),
        vault: vault_from(settings, pairs)?,
        postal,
    });
    let (step_parts, step_clock) = (Arc::clone(&parts), Arc::clone(clock));
    registry.register(UNREACHABLE_STEP_KIND, move |pool, task| {
        askhava_picks::api::run_unreachable_step(
            pool,
            Arc::clone(&step_parts),
            task,
            step_clock.now(),
        )
    });
    let (dispute_parts, dispute_clock) = (Arc::clone(&parts), Arc::clone(clock));
    registry.register(
        askhava_payments::api::DISPUTE_OPENED_KIND,
        move |pool, task| {
            run_dispute_opened(pool, Arc::clone(&dispute_parts), task, dispute_clock.now())
        },
    );
    let fraud_clock = Arc::clone(clock);
    registry.register(
        askhava_payments::api::FRAUD_WARNING_KIND,
        move |pool, task| run_fraud_warning(pool, Arc::clone(&parts), task, fraud_clock.now()),
    );
    Ok(())
}

/// The worker's one Stripe client, the Stripe webhook's (`release_sink::stripe_webhook`, read
/// when [`stripe_wanted`]), shared by `picks`' steps and [`payment_jobs`] (D-3118, D-3134, T433):
/// `None` in dev and CI without the webhook secret.
///
/// # Errors
/// A missing or bad Stripe setting in test or live, named, never its value.
fn stripe_client(
    settings: &Settings,
    pairs: &[(String, String)],
) -> io::Result<Option<askhava_adapters::api::StripePayments>> {
    use secrecy::ExposeSecret;
    if !stripe_wanted(settings, pairs) {
        return Ok(None);
    }
    let lazy = PgPool::connect_lazy(settings.database_url().expose_secret())
        .map_err(|_| io::Error::other("setting ASKHAVA_DATABASE_URL has an invalid value"))?;
    let hooks = release_sink::stripe_webhook(settings, lazy, pairs.to_vec())?;
    Ok(hooks.map(|h| h.stripe.clone()))
}

/// Whether the Stripe webhook's settings are read for the worker: in test and live always (they
/// are required there), elsewhere only when its signing secret is set.
fn stripe_wanted(settings: &Settings, pairs: &[(String, String)]) -> bool {
    let set = pairs
        .iter()
        .any(|(k, v)| k == release_sink::WEBHOOK_SECRET && !v.trim().is_empty());
    set || matches!(
        settings.environment(),
        Environment::Test | Environment::Live
    )
}

/// The safety voice call switch, read on every `calls.safety_call` run, so the registration holds
/// no value (CALL-019, D-3134): off, the launch state (D-384), until T407 replaces this body with
/// the stored owner-portal switch and Casey's recorded approval.
fn safety_switch() -> askhava_calls::api::SafetySwitch {
    askhava_calls::api::SafetySwitch::off()
}

/// What the `calls.safety_call` handler dials with (CALL-019, CALL-022, D-3134): the Twilio voice
/// client and masked pool of the Twilio settings, `ASKHAVA_MAIN_NUMBER`, the `ID_SECRET`, the
/// public base URL, the vault (both vault keys) and the worker clock; none of it is read while
/// the switch is off.
struct SafetyParts {
    voice: askhava_adapters::api::TwilioVoice,
    pool: askhava_adapters::api::MaskedPool,
    main: Option<String>,
    secret: SecretString,
    public_base: Option<String>,
    vault: Option<Vault>,
    clock: Arc<dyn Clock>,
}

const SAFETY_FAILED: askhava_core::api::HandlerError =
    askhava_core::api::HandlerError("the safety call step failed");

/// The `calls.safety_call` handler (CALL-019, CALL-022, D-1705, D-3134): reads the switch first;
/// off (the default), no call is placed and the task completes. On, it dials from the first pool
/// number that is not the main number through `calls::api::place_safety_call`, which re-checks
/// the switch, the 30 minutes, the calling number and the suppression list and dials at most once
/// per alert; a missing main number, public base URL or vault, or no pool number other than the
/// main number, fails the task, never dials.
async fn run_safety_call(
    pool: PgPool,
    parts: Arc<SafetyParts>,
    task: askhava_core::api::Claimed,
) -> Result<(), askhava_core::api::HandlerError> {
    use askhava_calls::api::{SafetyCall, SafetyDialer, place_safety_call};
    let call = SafetyCall::from_payload(&task.payload).ok_or(askhava_core::api::HandlerError(
        "safety call payload is malformed",
    ))?;
    let switch = safety_switch();
    if !switch.is_on() {
        return Ok(());
    }
    let p = &*parts;
    let (Some(main), Some(public_base), Some(vault)) = (&p.main, &p.public_base, &p.vault) else {
        return Err(SAFETY_FAILED);
    };
    let numbers = p.pool.numbers();
    let from = numbers
        .iter()
        .find(|n| n.as_str() != main)
        .ok_or(SAFETY_FAILED)?;
    let dialer = SafetyDialer {
        voice: &p.voice,
        from,
        main,
        switch,
        secret: &p.secret,
        public_base,
    };
    let mut db = pool.acquire().await.map_err(|_| SAFETY_FAILED)?;
    place_safety_call(&mut db, vault, &dialer, &call, p.clock.now())
        .await
        .map(|_| ())
        .map_err(|_| SAFETY_FAILED)
}

/// T318 (TXT-027, QH-004, D-1687): with both vault keys, `calls.whos_calling`
/// (`calls::api::run_whos_calling`) with the `ID_SECRET` as outbox key and the vault for her send
/// window; without them (dev and CI) nothing is registered and the tasks wait, as `vault_from`
/// allows.
fn register_whos_calling(
    registry: &mut Registry,
    settings: &Settings,
    (id_secret, clock): (&SecretString, &Arc<dyn Clock>),
    pairs: Vec<(String, String)>,
) -> io::Result<()> {
    let Some(vault) = vault_from(settings, pairs)? else {
        return Ok(());
    };
    let keys = Arc::new((vault, id_secret.clone()));
    let clock = Arc::clone(clock);
    registry.register(askhava_calls::api::WHOS_CALLING_KIND, move |pool, task| {
        let (keys, now) = (Arc::clone(&keys), clock.now());
        async move {
            askhava_calls::api::run_whos_calling(pool, (&keys.0, &keys.1), task, now).await
        }
    });
    Ok(())
}

/// T330 (CALL-019, CALL-022, D-2288, D-3134): registers `calls.safety_call` inside the Twilio
/// settings branch, over the same settings as `picks.line_retry`, with [`SafetyParts`] built once;
/// the switch is not captured ([`safety_switch`] is read on each run).
fn register_safety_call(
    registry: &mut Registry,
    settings: &Settings,
    (twilio, id_secret, clock): (&TwilioConfig, &SecretString, &Arc<dyn Clock>),
    pairs: Vec<(String, String)>,
) -> io::Result<()> {
    let (_, voice, _) = twilio_clients(twilio).map_err(io::Error::other)?;
    let parts = Arc::new(SafetyParts {
        voice,
        pool: twilio.pool().clone(),
        main: settings.main_number().map(str::to_owned),
        secret: id_secret.clone(),
        public_base: settings.public_base_url().map(str::to_owned),
        vault: vault_from(settings, pairs)?,
        clock: Arc::clone(clock),
    });
    registry.register(askhava_calls::api::SAFETY_CALL_KIND, move |pool, task| {
        run_safety_call(pool, Arc::clone(&parts), task)
    });
    Ok(())
}

/// T251 (D-3180; CARD-025, INT-029, TXT-032): with both vault keys, `intake`'s card sender on
/// `intake.cards_approved` and `intake.emergency_released`, with the `ID_SECRET` as outbox key and
/// the vault for the cards-out text's send window; without them (dev and CI) nothing is
/// registered and the events wait, as `vault_from` allows.
fn register_card_sender(
    registry: &mut Registry,
    settings: &Settings,
    id_secret: &SecretString,
    pairs: Vec<(String, String)>,
) -> io::Result<()> {
    let Some(vault) = vault_from(settings, pairs)? else {
        return Ok(());
    };
    let keys = Arc::new(askhava_intake::api::CardsOutKeys::new(
        id_secret.clone(),
        vault,
    ));
    let approved = Arc::clone(&keys);
    registry.register(
        askhava_intake::api::CARDS_APPROVED_KIND,
        move |pool, task| {
            askhava_intake::api::run_cards_approved(pool, Arc::clone(&approved), task)
        },
    );
    registry.register(
        askhava_intake::api::EMERGENCY_RELEASED_KIND,
        move |pool, task| {
            askhava_intake::api::run_emergency_released(pool, Arc::clone(&keys), task)
        },
    );
    Ok(())
}

/// T377 (D-2761; TXT-013, SEC-016): the texting client asking Twilio for status callbacks at
/// `ASKHAVA_PUBLIC_BASE_URL` plus [`inbound_text::STATUS_PATH`], the URL the status route
/// rebuilds to check the signature; without a public base URL (dev and CI without webhooks) it
/// asks for none.
fn with_status_url(settings: &Settings, sms: TwilioSms) -> TwilioSms {
    match settings.public_base_url() {
        Some(base) => sms.with_status_callback(format!("{base}{}", inbound_text::STATUS_PATH)),
        None => sms,
    }
}

/// T336 (D-2307, D-3137, D-3173): registers `messaging.inbound_retry` over the Twilio Messages
/// reader and the vault from both vault keys in `pairs`, with the worker's `ID_SECRET` and clock.
/// Without both vault keys it registers nothing in dev and CI (as before T336) and is an error in
/// test and live.
fn register_inbound_retry(
    registry: &mut Registry,
    settings: &Settings,
    (twilio, id_secret, clock): (&TwilioConfig, &SecretString, &Arc<dyn Clock>),
    pairs: Vec<(String, String)>,
) -> io::Result<()> {
    let (Some(seal), Some(index)) = vault_keys(pairs) else {
        return match settings.environment() {
            Environment::Dev | Environment::Ci => Ok(()),
            Environment::Test | Environment::Live => Err(io::Error::other(
                "the inbound retry needs VAULT_KEY and VAULT_INDEX_KEY",
            )),
        };
    };
    let vault = Vault::new(&seal, &index, settings.database_url()).map_err(io::Error::other)?;
    let reader = TwilioMessageReader::new(twilio).map_err(io::Error::other)?;
    let keys = inbound_retry::RetryKeys {
        id_secret: id_secret.clone(),
        vault,
        intake: askhava_intake::api::rules::Settings::default(),
        clock: Arc::clone(clock),
    };
    inbound_retry::register(registry, Arc::new(reader), Arc::new(keys));
    Ok(())
}

/// The homeowner line's configured number, `ASKHAVA_HOMEOWNER_NUMBER` (TXT-022, D-2628).
const HOMEOWNER_NUMBER: &str = "ASKHAVA_HOMEOWNER_NUMBER";

/// `messaging`'s outbox sender (T356, D-2628): the `ID_SECRET` and vault, the Twilio texting
/// client, the configured homeowner number from `pairs`, the footer's postal line when set (else
/// no email copy) and the worker's clock. The number is required in test and live, as the Twilio
/// and postal settings are (D-1688, D-3095): unset there the worker refuses to start naming the
/// key, since a line with no sender texts nothing and every CON-009 confirmation would expire
/// unsent (D-3149); unset in dev and CI the line has no sender and rows wait (TXT-022).
///
/// # Errors
///
/// A missing number in test or live, a number that is not `+1` and ten digits, or a broken
/// area-code table, named.
fn outbox_sender(
    settings: &Settings,
    keys: (SecretString, Vault),
    sms: askhava_adapters::api::TwilioSms,
    (pairs, clock): (&[(String, String)], Arc<dyn askhava_core::api::Clock>),
) -> io::Result<askhava_messaging::api::OutboxSender<askhava_adapters::api::TwilioSms>> {
    use askhava_messaging::api::{MailboxKind, OutboxSender, PostalAddress};
    let number = homeowner_number(settings, pairs)?;
    let postal = settings.postal_address().ok().and_then(|line| {
        PostalAddress::new(line, MailboxKind::PoBox)
            .or_else(|_| PostalAddress::new(line, MailboxKind::PrivateMailbox))
            .ok()
    });
    OutboxSender::new(keys, sms, (number, postal), clock).map_err(io::Error::other)
}

/// `ASKHAVA_HOMEOWNER_NUMBER` from `pairs` (the last one wins): `None` when unset in dev and CI,
/// an error naming the key when unset in test and live (D-3149) or not `+1` and ten digits.
fn homeowner_number(
    settings: &Settings,
    pairs: &[(String, String)],
) -> io::Result<Option<askhava_messaging::api::SendingNumber>> {
    use askhava_messaging::api::SendingNumber;
    match pairs.iter().rev().find(|(k, _)| k == HOMEOWNER_NUMBER) {
        Some((_, raw)) => SendingNumber::parse(raw.trim())
            .map(Some)
            .map_err(|_| io::Error::other("setting ASKHAVA_HOMEOWNER_NUMBER has an invalid value")),
        None => match settings.environment() {
            Environment::Dev | Environment::Ci => Ok(None),
            Environment::Test | Environment::Live => Err(io::Error::other(
                "missing required setting ASKHAVA_HOMEOWNER_NUMBER",
            )),
        },
    }
}

/// The Twilio settings the masked-line retry (its pool, CALL-001, CALL-017, D-1688) and the day-31
/// body delete (PRIV-027) use; without them `None` in dev and CI (no handler, so those tasks wait)
/// and an error in the test and live projects.
fn twilio_config<I>(settings: &Settings, pairs: I) -> io::Result<Option<TwilioConfig>>
where
    I: IntoIterator<Item = (String, String)>,
{
    match TwilioConfig::from_settings(settings, pairs) {
        Ok(config) => Ok(Some(config)),
        Err(error) => match settings.environment() {
            Environment::Dev | Environment::Ci => Ok(None),
            Environment::Test | Environment::Live => Err(io::Error::other(error)),
        },
    }
}

/// The composition's send-time checks for a business notice (D-1715, D-1760, D-1828): CON-019's
/// `businesses::api::may_text` read at the send, and PICK-019's "Accept by" from
/// `picks::api::notice_sent` with the worker's Confirm window; a pick that moved on, or a failed
/// read, sends no pick notice.
struct NoticeChecks {
    confirm: askhava_picks::api::ConfirmSettings,
}

impl askhava_messaging::api::SendChecks for NoticeChecks {
    async fn may_text(&self, conn: &mut sqlx::PgConnection, business: Uuid, phone: Uuid) -> bool {
        askhava_businesses::api::may_text(conn, business, phone)
            .await
            .unwrap_or(false)
    }

    async fn accept_by(
        &self,
        conn: &mut sqlx::PgConnection,
        pick: Uuid,
        now: jiff::Timestamp,
    ) -> Option<jiff::Timestamp> {
        askhava_picks::api::notice_sent(conn, pick, now, self.confirm)
            .await
            .ok()
            .flatten()
    }
}

/// The business-notice and receipt senders over the footer's postal line (SEC-028, D-1826 (1)).
type NoticeSenders = (
    askhava_messaging::api::NoticeSender,
    askhava_messaging::api::ReceiptSender,
);

/// Both senders from `ASKHAVA_POSTAL_ADDRESS` (a P.O. box or private mailbox, whichever the line
/// names) and `ID_SECRET` (the email key and the version-1 link key, as [`job_text_keys`]); with no
/// postal setting `None` in dev and CI (nothing registered, so those tasks wait) and an error
/// naming the key in test and live, as [`twilio_config`] (T313, D-3095).
fn notice_senders(
    settings: &Settings,
    id_secret: &SecretString,
) -> io::Result<Option<NoticeSenders>> {
    use askhava_messaging::api::{MailboxKind, NoticeSender, PostalAddress, ReceiptSender};
    let postal = match settings.postal_address() {
        Ok(postal) => postal,
        Err(missing) => match settings.environment() {
            Environment::Dev | Environment::Ci => return Ok(None),
            Environment::Test | Environment::Live => return Err(io::Error::other(missing)),
        },
    };
    let kind = match PostalAddress::new(postal, MailboxKind::PoBox) {
        Ok(_) => MailboxKind::PoBox,
        Err(_) => MailboxKind::PrivateMailbox,
    };
    let invalid = |_| io::Error::other("setting ASKHAVA_POSTAL_ADDRESS has an invalid value");
    let links = IdSecret::new(1, id_secret.clone()).map_err(io::Error::other)?;
    let notice = NoticeSender::new((id_secret.clone(), links), postal, kind).map_err(invalid)?;
    let receipt = ReceiptSender::new(id_secret.clone(), postal, kind).map_err(invalid)?;
    Ok(Some((notice, receipt)))
}

/// Registers `messaging.business_notice` (`run_business_notice` with [`NoticeChecks`] and the
/// worker's clock: CARD-020, CARD-021, BIZ-019, QH-005, D-2203) and `messaging.charge_receipt`
/// (`run_receipt_email`, FEE-031, D-2287), the two senders [`notice_senders`] built.
fn register_notice_senders(
    registry: &mut Registry,
    (notice, receipt): NoticeSenders,
    (clock, confirm): (
        Arc<dyn askhava_core::api::Clock>,
        askhava_picks::api::ConfirmSettings,
    ),
) {
    use askhava_messaging::api::{
        BUSINESS_NOTICE_KIND, NoticeWorker, RECEIPT_EMAIL_KIND, run_business_notice,
        run_receipt_email,
    };
    let worker = Arc::new(NoticeWorker {
        sender: notice,
        checks: NoticeChecks { confirm },
        clock,
    });
    registry.register(BUSINESS_NOTICE_KIND, move |pool, task| {
        run_business_notice(pool, Arc::clone(&worker), task)
    });
    let receipt = Arc::new(receipt);
    registry.register(RECEIPT_EMAIL_KIND, move |pool, task| {
        run_receipt_email(pool, Arc::clone(&receipt), task)
    });
}

/// The business-account handlers: the complaint box's close (BIZ-029, T083), a removed
/// member's sessions and links ended (SEC-004, SEC-024, T084), and the welcome and phone-stopped
/// emails (CON-020, BIZ-017, T268), which have no sender yet: no Resend client is built and no
/// business mailbox address or phone number can be read (the sender directory keeps digests
/// only), so each such task fails with `NO_ACCOUNT_SENDER` and is tried again, and a welcome is
/// never marked sent without its email (D-3186). The OPS-041 owner-inbox email of a not-covered
/// request (`intake.not_covered`, T215) has no sender either until a Resend client and the
/// owner-inbox address exist, so it fails and is tried again the same way (D-3334).
fn register_business_handlers(registry: &mut Registry) {
    use askhava_adapters::api::ResendEmail;
    use askhava_businesses::api::{run_coverage_notice, run_phone_stopped, run_welcome};
    registry.register(askhava_intake::api::NOT_COVERED_KIND, |pool, task| {
        run_coverage_notice::<ResendEmail>(pool, None, task)
    });
    registry.register(
        askhava_businesses::api::BOX_CLOSED_KIND,
        askhava_businesses::api::run_box_closed,
    );
    registry.register(
        askhava_businesses::api::MEMBER_REMOVED_KIND,
        askhava_portals::api::run_member_removed,
    );
    registry.register(askhava_businesses::api::WELCOME_KIND, |pool, task| {
        run_welcome::<ResendEmail, NoAccountDirectory>(pool, None, task)
    });
    registry.register(askhava_businesses::api::PHONE_STOPPED_KIND, |pool, task| {
        run_phone_stopped::<ResendEmail, NoAccountDirectory>(pool, None, task)
    });
}

/// The account emails' directory until business addresses and phone numbers are readable
/// (D-3186): nothing is on file.
struct NoAccountDirectory;

impl askhava_businesses::api::AccountDirectory for NoAccountDirectory {
    async fn address(
        &self,
        _: &mut sqlx::PgConnection,
        _: Uuid,
    ) -> Result<Option<SecretString>, askhava_core::api::HandlerError> {
        Ok(None)
    }
    async fn last4(
        &self,
        _: &mut sqlx::PgConnection,
        _: (Uuid, Uuid),
    ) -> Result<Option<String>, askhava_core::api::HandlerError> {
        Ok(None)
    }
}

/// `intake`'s accepted-quote email after a Confirm (PICK-042, T388), with no sender until the
/// footer postal setting exists, so each task ends done with nothing sent (D-3077).
fn register_accepted_quote(registry: &mut Registry) {
    registry.register(askhava_picks::api::ACCEPTED_QUOTE_KIND, |pool, task| {
        askhava_intake::api::run_accepted_quote(pool, None, task)
    });
}

/// The retention purge (PRIV-007 to PRIV-009, D-491), every handler at `clock`'s now (the worker's
/// clock, SEC-035, D-2641): `messaging`'s daily body cut (TXT-020, D-1854); `jobs`' daily sweep and
/// stage tasks, with `intake`'s sweep of requests that never got a job on the same daily task (the
/// sweep and the cut queue the next day's run first); on a stage task or `privacy`'s re-check,
/// `businesses` queues its complaint hold events and `payments` places its dispute holds before
/// `privacy` decides (a re-check that queued new events waits for them, D-1799); `privacy` applies
/// the complaint events and hands the contact and text stages to `intake`, `quotes` and, on the
/// text stage, the feedback words' step ([`feedback_step`], T315). Each purge task's last failed
/// attempt, the daily sweep's and cut's too, raises an alert ([`alert_if_last`]).
fn register_purge_handlers(registry: &mut Registry, clock: &Arc<dyn Clock>) {
    register_daily_purges(registry, clock);
    register_purge_stages(registry, clock);
    register_daily_sweeps(registry, clock);
    let at = Arc::clone(clock);
    registry.register(
        askhava_businesses::api::COMPLAINT_HOLD_KIND,
        move |pool, task| askhava_privacy::api::run_complaint_hold_at(pool, task, at.now()),
    );
    let at = Arc::clone(clock);
    registry.register(askhava_privacy::api::PURGE_KIND, move |pool, task| {
        let now = at.now();
        async move {
            let outcome = purge_step(&pool, &task, now).await;
            alert_if_last(&pool, &task, outcome).await
        }
    });
    register_feedback_purge(registry, clock);
    let at = Arc::clone(clock);
    registry.register(
        askhava_intake::api::REQUEST_PURGE_KIND,
        move |pool, task| {
            let now = at.now();
            async move {
                let outcome =
                    askhava_intake::api::run_request_purge_at(pool.clone(), task.clone(), now);
                alert_if_last(&pool, &task, outcome.await).await
            }
        },
    );
}

/// PRIV-007 to PRIV-009, TXT-020, D-2641, D-1840: the daily body cut, the daily cut of kept
/// problem words and the daily purge sweep (with `intake`'s request sweep, then the business contact
/// sweep, T415) at `clock`'s now; a run's last failed attempt raises one
/// `privacy.purge_failed` alert, and the next day's run is already queued.
fn register_daily_purges(registry: &mut Registry, clock: &Arc<dyn Clock>) {
    let at = Arc::clone(clock);
    registry.register(askhava_messaging::api::CUT_KIND, move |pool, task| {
        let now = at.now();
        async move {
            let outcome = askhava_messaging::api::run_cut_at(pool.clone(), task.clone(), now);
            alert_if_last(&pool, &task, outcome.await).await
        }
    });
    let at = Arc::clone(clock);
    registry.register(askhava_privacy::api::CUT_WORDS_KIND, move |pool, task| {
        let now = at.now();
        async move {
            let outcome = askhava_privacy::api::run_words_cut_at(pool.clone(), task.clone(), now);
            alert_if_last(&pool, &task, outcome.await).await
        }
    });
    let at = Arc::clone(clock);
    registry.register(askhava_jobs::api::SWEEP_KIND, move |pool, task| {
        let now = at.now();
        async move {
            let outcome = async {
                askhava_jobs::api::run_sweep_at(pool.clone(), task.clone(), now).await?;
                askhava_intake::api::run_request_sweep_at(pool.clone(), task.clone(), now).await?;
                business_contacts::sweep_business_contacts(&pool, now)
                    .await
                    .map(|_| ())
            };
            alert_if_last(&pool, &task, outcome.await).await
        }
    });
}

/// SEC-004, SEC-014, SEC-040, D-2207, D-2306 (T436): `portals`' daily sweep of sessions expired
/// more than a day earlier and `core`'s daily prune of rate-limit windows older than the longest
/// window plus a day, both at `clock`'s now; each queues the next day's run first, and
/// `schedule_start` in `worker.rs` seeds both.
fn register_daily_sweeps(registry: &mut Registry, clock: &Arc<dyn Clock>) {
    let at = Arc::clone(clock);
    registry.register(
        askhava_portals::api::SESSION_SWEEP_KIND,
        move |pool, task| askhava_portals::api::run_session_sweep_at(pool, task, at.now()),
    );
    let at = Arc::clone(clock);
    registry.register(
        askhava_core::api::RATE_LIMIT_PRUNE_KIND,
        move |pool, task| askhava_core::api::run_rate_limit_prune_at(pool, task, at.now()),
    );
}

/// PRIV-007 to PRIV-009, D-1799, D-2641: `jobs`' stage tasks and `privacy`'s re-check at `clock`'s
/// now: the complaint and dispute hold steps, then `privacy`'s decision.
fn register_purge_stages(registry: &mut Registry, clock: &Arc<dyn Clock>) {
    for kind in [
        askhava_jobs::api::PURGE_DUE_KIND,
        askhava_privacy::api::RECHECK_KIND,
    ] {
        let at = Arc::clone(clock);
        registry.register(kind, move |pool, task| {
            let now = at.now();
            async move {
                let outcome = stage_step(&pool, &task, now).await;
                alert_if_last(&pool, &task, outcome).await
            }
        });
    }
}

/// PRIV-007 to PRIV-009, D-1799: one stage task or re-check at `now`: `businesses`' complaint hold
/// events, `payments`' dispute holds, then `privacy`'s decision.
async fn stage_step(
    pool: &PgPool,
    task: &askhava_core::api::Claimed,
    now: jiff::Timestamp,
) -> Result<(), askhava_core::api::HandlerError> {
    let queued = askhava_businesses::api::run_complaint_holds(pool.clone(), task.clone()).await?;
    askhava_payments::api::run_dispute_holds_at(pool.clone(), task.clone(), now).await?;
    askhava_privacy::api::run_purge_due_after_at(pool.clone(), task.clone(), (queued, now)).await
}

/// PRIV-007, PRIV-008, D-491, D-1840: one `privacy.purge` task at `now`: `intake`'s rows,
/// `quotes`' text, `privacy`'s kept problem words and, on the text stage, the feedback words' step.
async fn purge_step(
    pool: &PgPool,
    task: &askhava_core::api::Claimed,
    now: jiff::Timestamp,
) -> Result<(), askhava_core::api::HandlerError> {
    askhava_intake::api::run_purge_at(pool.clone(), task.clone(), now).await?;
    askhava_quotes::api::run_text_purge(pool.clone(), task.clone()).await?;
    askhava_privacy::api::run_words_purge(pool.clone(), task.clone()).await?;
    match askhava_privacy::api::purge_target(task)? {
        (job, askhava_privacy::api::PurgeStage::Text) => feedback_step(pool, job, now).await,
        _ => Ok(()),
    }
}

/// PRIV-009, D-3264: the held feedback's daily ask-again task runs [`feedback_step`].
fn register_feedback_purge(registry: &mut Registry, clock: &Arc<dyn Clock>) {
    let at = Arc::clone(clock);
    registry.register(askhava_jobs::api::FEEDBACK_PURGE_KIND, move |pool, task| {
        let now = at.now();
        async move {
            let job = task.payload["job"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or(askhava_core::api::HandlerError(
                    "feedback purge payload is not a job",
                ));
            let outcome = match job {
                Ok(job) => feedback_step(&pool, job, now).await,
                Err(e) => Err(e),
            };
            alert_if_last(&pool, &task, outcome).await
        }
    });
}

/// PRIV-009, PRIV-007, JOB-017 (T315, D-2205, D-3264): the text stage's feedback step: `privacy`
/// says whether job `job` is held (an open dispute or complaint), and `jobs` blanks its feedback
/// words when not, or keeps them and asks again in a day, at `now` (the worker's clock, D-2641).
async fn feedback_step(
    pool: &PgPool,
    job: Uuid,
    now: jiff::Timestamp,
) -> Result<(), askhava_core::api::HandlerError> {
    let failed = askhava_core::api::HandlerError("feedback purge database call failed");
    let mut conn = pool.acquire().await.map_err(|_| failed)?;
    let held = askhava_privacy::api::is_held(&mut conn, job)
        .await
        .map_err(|_| failed)?;
    askhava_jobs::api::feedback_text_stage(&mut conn, (job, held), now)
        .await
        .map(|_| ())
        .map_err(|_| failed)
}

/// PRIV-007, OPS-001, D-1799: a purge task that failed on its last attempt is dead-lettered and
/// never queued again (its dedupe key stays), so it also raises one High `privacy.purge_failed`
/// alert naming the job or request by ID; `outcome` is returned unchanged.
///
/// # Errors
/// `outcome`'s own error, unchanged; a failure to store the alert is logged, never returned.
pub async fn alert_if_last(
    pool: &PgPool,
    task: &askhava_core::api::Claimed,
    outcome: Result<(), askhava_core::api::HandlerError>,
) -> Result<(), askhava_core::api::HandlerError> {
    if outcome.is_ok() || task.attempts < askhava_core::api::DEFAULT_MAX_ATTEMPTS {
        return outcome;
    }
    let id = |key: &str| {
        task.payload[key]
            .as_str()
            .and_then(|s| sqlx::types::Uuid::parse_str(s).ok())
    };
    let subject = id("job")
        .map(|job| ("jobs.job", job))
        .or_else(|| id("conversation").map(|c| ("intake.conversation", c)));
    let alert = askhava_ops::api::NewAlert {
        kind: "privacy.purge_failed",
        severity: askhava_ops::api::Severity::High,
        subject,
    };
    let raised = match pool.acquire().await {
        Ok(mut conn) => askhava_ops::api::raise_alert(&mut conn, &alert)
            .await
            .is_ok(),
        Err(_) => false,
    };
    if !raised {
        tracing::error!("purge failure alert could not be stored");
    }
    outcome
}

/// `payments`' release minter, made from `privacy`'s mint the first time it is asked for, so the
/// mint is taken once per process however often the composition is built; the Stripe webhook route
/// (T224) hands it verified events (SEC-030, D-488, D-1477).
pub fn release_minter() -> &'static Minter {
    static MINTER: OnceLock<Minter> = OnceLock::new();
    MINTER.get_or_init(|| Minter::new(askhava_privacy::api::release_mint()))
}

/// `privacy`'s session mint, taken the first time it is asked for, so it is taken once per process
/// however often the composition is built and every server test file shares it; the business
/// session extractor makes its `BusinessSessionProof` with it for `picks`' released view (SEC-046,
/// D-488, D-1739).
pub fn session_minter() -> &'static SessionMint {
    static MINT: OnceLock<SessionMint> = OnceLock::new();
    MINT.get_or_init(askhava_privacy::api::session_mint)
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
    use askhava_picks::api::ConfirmSettings;
    use secrecy::SecretString;

    use crate::offdb_stream::OffDbStream;
    use crate::{job_text_keys, pick_code_keys, register_handlers, webhooks};

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
        let with = format!(
            "{:?}",
            register_handlers(stream(), texts, ConfirmSettings::default())
        );
        let without = format!(
            "{:?}",
            register_handlers(stream(), None, ConfirmSettings::default())
        );
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

    /// `pick_code_keys` over `env`'s fake pairs plus `extra` and the vault keys chosen.
    fn picks(env: &str, extra: &[(&str, &str)], vault: (bool, bool)) -> std::io::Result<bool> {
        let lists = settings_pairs(env);
        let all: Vec<(String, String)> = pairs(env, &[lists.as_slice(), extra].concat())
            .into_iter()
            .chain(vault_pairs(vault.0, vault.1))
            .collect();
        let settings = Settings::from_pairs(all.clone()).expect("settings");
        pick_code_keys(&settings, all).map(|k| k.is_some())
    }

    /// T246 (D-1516, D-1302): without vault keys dev and CI build no page-pick keys (the page keeps
    /// its "Nothing was picked" lines) and the test and live projects refuse to start; with them
    /// the Twilio keys and a strong `ID_SECRET` are required, each error naming the key; the
    /// compiled-in Terms name their version.
    #[test]
    fn page_pick_keys_need_the_vault_the_id_secret_and_twilio() {
        let twilio = [TWILIO.as_slice(), &[TOKEN]].concat();
        let full = [&[ID], twilio.as_slice()].concat();
        for env in ["dev", "ci"] {
            assert!(
                !picks(env, &full, (false, false)).expect("no keys"),
                "{env}"
            );
        }
        for env in ["test", "live"] {
            let refused = picks(env, &[ID], (false, false)).expect_err("refused");
            assert_eq!(refused.to_string(), "missing VAULT_KEY and VAULT_INDEX_KEY");
        }
        assert!(picks("dev", &full, (true, true)).expect("keys"));
        let no_id = picks("dev", &twilio, (true, true)).expect_err("refused");
        assert_eq!(no_id.to_string(), "missing ASKHAVA_ID_SECRET");
        let no_token = picks("dev", &[&[ID], TWILIO.as_slice()].concat(), (true, true));
        let message = no_token.expect_err("refused").to_string();
        assert!(message.contains("ASKHAVA_TWILIO_AUTH_TOKEN"), "{message}");
        let half = picks("dev", &full, (true, false)).expect_err("refused");
        assert_eq!(half.to_string(), "VAULT_KEY set without VAULT_INDEX_KEY");
        assert_ne!(crate::terms_version(), "unversioned");
    }

    /// `notice_senders` over `env`'s fake pairs plus `ASKHAVA_POSTAL_ADDRESS` when given.
    fn senders(env: &str, postal: Option<&str>) -> std::io::Result<bool> {
        let lists = settings_pairs(env);
        let extra: Vec<(&str, &str)> = postal
            .map(|p| ("ASKHAVA_POSTAL_ADDRESS", p))
            .into_iter()
            .collect();
        let all = pairs(env, &[lists.as_slice(), extra.as_slice()].concat());
        let settings = Settings::from_pairs(all).expect("settings");
        super::notice_senders(&settings, &id_secret()).map(|s| s.is_some())
    }

    /// T356 (D-3149): without `ASKHAVA_HOMEOWNER_NUMBER` dev and CI have no homeowner sender and
    /// the test and live projects refuse to start naming the key; a `+1` ten-digit number is
    /// taken in every project, and any other value is refused naming the key.
    #[test]
    fn the_homeowner_number_is_required_in_test_and_live() {
        let number = |env: &str, value: Option<&str>| {
            let lists = settings_pairs(env);
            let settings = Settings::from_pairs(pairs(env, &lists)).expect("settings");
            let extra: Vec<(String, String)> = value
                .map(|v| (super::HOMEOWNER_NUMBER.to_owned(), v.to_owned()))
                .into_iter()
                .collect();
            super::homeowner_number(&settings, &extra).map(|n| n.is_some())
        };
        for env in ["dev", "ci"] {
            assert!(!number(env, None).expect("no number"), "{env}");
        }
        for env in ["test", "live"] {
            let refused = number(env, None).expect_err("refused").to_string();
            assert_eq!(
                refused, "missing required setting ASKHAVA_HOMEOWNER_NUMBER",
                "{env}"
            );
        }
        for env in ["dev", "ci", "test", "live"] {
            assert!(number(env, Some("+19285550100")).expect("number"), "{env}");
            let bad = number(env, Some("555-0100")).expect_err("bad").to_string();
            assert_eq!(
                bad, "setting ASKHAVA_HOMEOWNER_NUMBER has an invalid value",
                "{env}"
            );
        }
    }

    /// T313 (D-3095, D-1826 (1)): without the postal setting dev and CI build no business-notice
    /// or receipt sender and the test and live projects refuse to start naming the key; a P.O.
    /// box or private mailbox line builds both; any other line is refused naming the key.
    #[test]
    fn the_notice_senders_need_the_postal_setting_in_test_and_live() {
        for env in ["dev", "ci"] {
            assert!(!senders(env, None).expect("no sender"), "{env}");
        }
        for env in ["test", "live"] {
            let refused = senders(env, None).expect_err("refused").to_string();
            assert_eq!(
                refused, "missing required setting ASKHAVA_POSTAL_ADDRESS",
                "{env}"
            );
        }
        assert!(senders("test", Some("PO Box 0000, Faketown, AZ 86400")).expect("box"));
        assert!(senders("ci", Some("100 Fake Rd PMB 7, Faketown, AZ 86400")).expect("pmb"));
        let home = senders("dev", Some("100 Fake Rd, Faketown, AZ 86400"));
        let message = home.expect_err("refused").to_string();
        assert_eq!(
            message,
            "setting ASKHAVA_POSTAL_ADDRESS has an invalid value"
        );
    }

    /// T377 (D-2761; TXT-013, SEC-016): with a public base URL the texting client asks for status
    /// callbacks at it plus the status route's path; without one it asks for none.
    #[test]
    fn the_texting_client_asks_for_status_callbacks_only_with_a_public_base_url() {
        let client = |extra: &[(&str, &str)]| {
            let all = pairs("dev", &[TWILIO.as_slice(), &[TOKEN], extra].concat());
            let settings = Settings::from_pairs(all.clone()).expect("settings");
            let twilio = askhava_adapters::api::TwilioConfig::from_settings(&settings, all);
            let (sms, _, _) = super::twilio_clients(&twilio.expect("config")).expect("clients");
            format!("{:?}", super::with_status_url(&settings, sms))
        };
        let with = client(&[BASE]);
        assert!(
            with.contains("Some(\"https://hooks.example.test/webhooks/twilio/status\")"),
            "{with}"
        );
        assert!(client(&[]).contains("status_callback: None"));
    }
}
