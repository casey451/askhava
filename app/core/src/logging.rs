//! Logging that cannot show personal data (PRIV-016, PRIV-017, PRIV-028; D-492).
//!
//! Every event is one line of JSON with `message` and `level` at the top level
//! (`flatten_event(true)`), the shape Railway's log viewer parses (PRIV-016).
//! Request spans carry the request ID and the matched route template, never
//! the URI, so tokens in paths never reach a log (PRIV-028, rust-production
//! R15). Contact values live in [`Redacted`], whose `Debug` and `Display` print
//! a marker (R6). Sentry runs with `send_default_pii` off and every event and
//! breadcrumb passes [`scrub_event`] first (PRIV-017).

use std::fmt;
use std::sync::Arc;

use axum::extract::MatchedPath;
use axum::http::Request;
use sentry::protocol::{Breadcrumb, Event, Stacktrace};
use serde::{Deserialize, Deserializer};
use thiserror::Error;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

use crate::settings::{Environment, Settings, expose};

/// What a [`Redacted`] value and a scrubbed piece of text print instead.
pub const REDACTED: &str = "[redacted]";
const MAX_REQUEST_ID: usize = 64;
const MIN_PHONE_DIGITS: usize = 7;
/// Sentry contexts that describe the machine, not a person; any other context
/// (such as the tracing fields of an error event) is dropped (PRIV-017).
const SYSTEM_CONTEXTS: &[&str] = &["os", "rust", "device", "runtime", "trace"];

/// A contact value (phone, email, name, address) that cannot print (PRIV-016, PRIV-028).
///
/// Contact fields deserialize straight into it; it has no `Serialize`, so it
/// cannot land in a JSON log either. Read it with [`Redacted::expose`] only
/// where the value is really needed (the vault, a confirmed and paid pick).
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Redacted<T>(T);

impl<T> Redacted<T> {
    /// Wraps a contact value.
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// The value itself; never pass it to a log, span, error or Sentry.
    pub fn expose(&self) -> &T {
        &self.0
    }
}

impl<T> fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl<T> fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Redacted<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self)
    }
}

/// Why logging could not start.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum LoggingError {
    /// `ASKHAVA_SENTRY_DSN` does not parse as a DSN (its value is never shown).
    #[error("setting ASKHAVA_SENTRY_DSN has an invalid value")]
    InvalidSentryDsn,
    /// Something installed a global subscriber before `init`.
    #[error("a global log subscriber is already installed")]
    AlreadyInstalled,
}

/// Keeps Sentry flushing until the process ends; hold it for the life of `main`.
#[must_use = "dropping the guard stops Sentry"]
pub struct LogGuard {
    _sentry: Option<sentry::ClientInitGuard>,
}

/// Installs the JSON log on stdout and, with a DSN, Sentry (PRIV-016, PRIV-017).
///
/// # Errors
///
/// An invalid DSN, or a subscriber already installed.
pub fn init(settings: &Settings) -> Result<LogGuard, LoggingError> {
    let sentry = match settings.sentry_dsn() {
        Some(dsn) => {
            let dsn = expose(dsn)
                .parse()
                .map_err(|_| LoggingError::InvalidSentryDsn)?;
            Some(sentry::init(sentry_options(dsn, settings.environment())))
        }
        None => None,
    };
    let sentry_layer = sentry
        .as_ref()
        .map(|_| sentry::integrations::tracing::ErrorLayer::new().with_filter(LevelFilter::ERROR));
    tracing_subscriber::registry()
        .with(LevelFilter::from_level(settings.log_level()))
        .with(json_layer(std::io::stdout))
        .with(sentry_layer)
        .try_init()
        .map_err(|_| LoggingError::AlreadyInstalled)?;
    Ok(LogGuard { _sentry: sentry })
}

/// The JSON layer: one line per event, `message` and `level` at the top level,
/// the enclosing spans (request ID, route template) under `spans` (PRIV-016, D-492).
pub fn json_layer<S, W>(writer: W) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    tracing_subscriber::fmt::layer()
        .json()
        .flatten_event(true)
        .with_current_span(false)
        .with_span_list(true)
        .with_writer(writer)
}

/// Sentry's options: default PII off, no request bodies, and the scrubber on
/// every event and breadcrumb (PRIV-017).
pub fn sentry_options(dsn: sentry::types::Dsn, environment: Environment) -> sentry::ClientOptions {
    let mut options = sentry::ClientOptions::default();
    options.dsn = Some(dsn);
    options.environment = Some(environment.as_str().into());
    options.send_default_pii = false;
    options.max_request_body_size = sentry::MaxRequestBodySize::None;
    options.before_send = Some(Arc::new(|event| Some(scrub_event(event))));
    options.before_breadcrumb = Some(Arc::new(|crumb| Some(scrub_breadcrumb(crumb))));
    options
}

/// The span for one HTTP request, for tower-http's `TraceLayer::make_span_with`
/// (PRIV-016, PRIV-028): the method, the matched route template and the
/// request ID, never the URI or query string. A request ID that is not short
/// letters, digits and dashes is logged as `invalid`.
pub fn request_span<B>(request: &Request<B>) -> tracing::Span {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", MatchedPath::as_str);
    let request_id = request.headers().get("x-request-id").map_or("none", |id| {
        id.to_str()
            .ok()
            .filter(|id| is_safe_request_id(id))
            .unwrap_or("invalid")
    });
    tracing::info_span!("request", method = %request.method(), route, request_id)
}

fn is_safe_request_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_REQUEST_ID
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// Removes what may hold personal data from a Sentry event (PRIV-017): the
/// user, the request (URL, query, headers, cookies, body), the transaction
/// name (it may hold a raw path with a token), the server name, extra data,
/// every context but the system ones, local variables in stack frames and log parameters; and
/// scrubs every remaining text with [`scrub_text`].
#[must_use]
pub fn scrub_event(mut event: Event<'static>) -> Event<'static> {
    event.user = None;
    event.request = None;
    event.server_name = None;
    event.transaction = None;
    event.extra.clear();
    event
        .contexts
        .retain(|name, _| SYSTEM_CONTEXTS.contains(&name.as_str()));
    scrub_option(&mut event.message);
    scrub_option(&mut event.culprit);
    if let Some(entry) = event.logentry.as_mut() {
        entry.message = scrub_text(&entry.message);
        entry.params.clear();
    }
    event
        .tags
        .values_mut()
        .for_each(|value| *value = scrub_text(value));
    for exception in &mut event.exception.values {
        scrub_option(&mut exception.value);
        exception.stacktrace.iter_mut().for_each(clear_frame_vars);
    }
    event.stacktrace.iter_mut().for_each(clear_frame_vars);
    for thread in &mut event.threads.values {
        thread.stacktrace.iter_mut().for_each(clear_frame_vars);
    }
    event.breadcrumbs.values = event
        .breadcrumbs
        .values
        .drain(..)
        .map(scrub_breadcrumb)
        .collect();
    event
}

/// Scrubs a breadcrumb's message and drops its data (PRIV-017).
#[must_use]
pub fn scrub_breadcrumb(mut crumb: Breadcrumb) -> Breadcrumb {
    scrub_option(&mut crumb.message);
    crumb.data.clear();
    crumb
}

/// Replaces each word with an `@` (an email) and each run of seven or more
/// digits, spaces and phone punctuation (a phone number) with [`REDACTED`]
/// (PRIV-017). It over-redacts long numbers such as dates on purpose; names
/// and streets cannot be told apart from text, which is why they travel as
/// [`Redacted`].
#[must_use]
pub fn scrub_text(text: &str) -> String {
    let emails_gone: String = text
        .split_inclusive(char::is_whitespace)
        .map(|word| {
            if word.contains('@') {
                redact_word(word)
            } else {
                word.to_owned()
            }
        })
        .collect();
    let mut out = String::with_capacity(emails_gone.len());
    let mut run = String::new();
    for c in emails_gone.chars() {
        if c.is_ascii_digit() || "+-(). ".contains(c) {
            run.push(c);
            continue;
        }
        flush_run(&mut out, &mut run);
        out.push(c);
    }
    flush_run(&mut out, &mut run);
    out
}

fn redact_word(word: &str) -> String {
    let trimmed = word.trim_end();
    format!("{REDACTED}{}", &word[trimmed.len()..])
}

fn flush_run(out: &mut String, run: &mut String) {
    if run.chars().filter(char::is_ascii_digit).count() >= MIN_PHONE_DIGITS {
        let start = run.len() - run.trim_start().len();
        let end = run.trim_end().len();
        out.push_str(&run[..start]);
        out.push_str(REDACTED);
        out.push_str(&run[end..]);
    } else {
        out.push_str(run);
    }
    run.clear();
}

fn scrub_option(text: &mut Option<String>) {
    if let Some(value) = text.as_mut() {
        *value = scrub_text(value);
    }
}

fn clear_frame_vars(stacktrace: &mut Stacktrace) {
    stacktrace
        .frames
        .iter_mut()
        .for_each(|frame| frame.vars.clear());
}
