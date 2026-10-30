//! The test-control router: clock advance and delivered-message reads, for e2e runs only (SEC-035, SEC-002).
//!
//! The routes exist only when `askhava-core` is built with the Cargo feature
//! `test-control`, the one feature the architecture test allows (D-491). The
//! production Docker build never turns it on, so the release binary carries no
//! `/__test/*` handler and every such path is 404 whatever `APP_ENV` says. With
//! the feature on, [`TestControl::from_settings`] still refuses the `test` and
//! `live` environments and needs a per-run random bearer secret
//! (`ASKHAVA_TEST_CONTROL_SECRET`, at least 32 characters); a request without
//! it gets the same 404 as an absent route (D-480, D-739).
//!
//! The clock is the system clock plus an offset that only moves forward. When
//! `ASKHAVA_TEST_CONTROL_CLOCK_FILE` is set the offset lives in that file, so
//! the `web` process that serves the router and the `worker` that runs timers
//! read the same "now" (D-740). Delivered messages are read from the Twilio
//! stand-in's own sent log (`GET .../Messages.json`, T167), never from the
//! `messaging` tables, so a test sees what the carrier received (D-491).

use std::sync::Arc;

use axum::Router;
use thiserror::Error;

use crate::clock::Clock;
use crate::settings::{Settings, SettingsError};

/// True when this build carries the test-control routes (SEC-035). The
/// release build and every default `cargo test` see `false`.
pub const TEST_CONTROL_COMPILED: bool = cfg!(feature = "test-control");

/// Why the test-control router refused to start (SEC-035). Names a key or
/// the environment, never a value (SEC-001).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TestControlError {
    /// The feature is on but `APP_ENV` is `test` or `live`.
    #[error("the test-control router runs only in dev and ci")]
    WrongEnvironment,
    /// A test-control setting is missing or invalid.
    #[error(transparent)]
    Settings(#[from] SettingsError),
}

/// The test-control routes and the clock both binaries use (SEC-035).
///
/// Without the feature the router has no routes and the clock is the system
/// clock, so nothing here changes how the release app behaves.
pub struct TestControl {
    router: Router,
    clock: Arc<dyn Clock>,
}

impl TestControl {
    /// Builds the router and clock from the settings and the same pairs
    /// `Settings::from_pairs` read (SEC-002, SEC-035).
    ///
    /// # Errors
    ///
    /// Only with the `test-control` feature: [`TestControlError::WrongEnvironment`]
    /// in `test` or `live`, and a settings error for a missing or short secret
    /// or a bad Twilio base URL. Without the feature it never fails.
    pub fn from_settings<I>(settings: &Settings, pairs: I) -> Result<Self, TestControlError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        build(settings, pairs)
    }

    /// The routes to merge into the app's router; empty in the release build (SEC-035).
    pub fn router(&self) -> Router {
        self.router.clone()
    }

    /// The clock every process must read "now" from, so an advance moves timers too.
    #[must_use]
    pub fn clock(&self) -> Arc<dyn Clock> {
        Arc::clone(&self.clock)
    }
}

/// Release build: no routes, the system clock, nothing read (SEC-035).
#[cfg(not(feature = "test-control"))]
fn build<I>(_settings: &Settings, _pairs: I) -> Result<TestControl, TestControlError>
where
    I: IntoIterator<Item = (String, String)>,
{
    Ok(TestControl {
        router: Router::new(),
        clock: Arc::new(crate::clock::SystemClock),
    })
}

#[cfg(feature = "test-control")]
fn build<I>(settings: &Settings, pairs: I) -> Result<TestControl, TestControlError>
where
    I: IntoIterator<Item = (String, String)>,
{
    enabled::build(settings, pairs)
}

#[cfg(feature = "test-control")]
mod enabled {
    //! The routes themselves, compiled only under the `test-control` feature (SEC-035).

    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, Ordering};

    use axum::extract::{Query, Request, State};
    use axum::http::{StatusCode, header::AUTHORIZATION};
    use axum::middleware::{self, Next};
    use axum::response::{IntoResponse, Response};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use jiff::{SignedDuration, Timestamp};
    use secrecy::{ExposeSecret, SecretString};
    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json};

    use super::{TestControl, TestControlError};
    use crate::clock::Clock;
    use crate::settings::{Environment, Settings, SettingsError, Vendor, check_base_url};

    const SECRET: &str = "ASKHAVA_TEST_CONTROL_SECRET";
    const CLOCK_FILE: &str = "ASKHAVA_TEST_CONTROL_CLOCK_FILE";
    const TWILIO_BASE: &str = "ASKHAVA_TWILIO_BASE_URL";
    const TWILIO_TOKEN: &str = "ASKHAVA_TWILIO_AUTH_TOKEN";
    const KEYS: [&str; 4] = [SECRET, CLOCK_FILE, TWILIO_BASE, TWILIO_TOKEN];
    /// The stand-in server's Twilio prefix (`stand_ins/server.py`, T167).
    const STAND_IN_TWILIO: &str = "http://127.0.0.1:4010/twilio";
    /// A per-run random secret is long; a short one is a hand-typed one (SEC-035).
    const MIN_SECRET_LEN: usize = 32;
    /// One advance moves at most a year, which covers every timer in the spec.
    const MAX_ADVANCE_SECONDS: u32 = 366 * 24 * 3_600;

    struct Control {
        secret: SecretString,
        clock: Arc<OffsetClock>,
        http: reqwest::Client,
        messages_url: String,
        account_sid: String,
        auth_token: Option<SecretString>,
    }

    /// SEC-035: dev and CI only, a long secret, the stand-in's sent log.
    pub(super) fn build<I>(settings: &Settings, pairs: I) -> Result<TestControl, TestControlError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let environment = settings.environment();
        if !matches!(environment, Environment::Dev | Environment::Ci) {
            return Err(TestControlError::WrongEnvironment);
        }
        let mut raw = read(pairs);
        let secret = raw.remove(SECRET).ok_or(SettingsError::Missing(SECRET))?;
        if secret.len() < MIN_SECRET_LEN {
            return Err(SettingsError::Invalid(SECRET).into());
        }
        let base = raw
            .remove(TWILIO_BASE)
            .unwrap_or_else(|| STAND_IN_TWILIO.to_owned());
        check_base_url(environment, TWILIO_BASE, &base)?;
        let account_sid = settings
            .account(Vendor::Twilio)
            .map_or_else(|| format!("AC{}", "0".repeat(32)), str::to_owned);
        let clock = Arc::new(OffsetClock {
            file: raw.remove(CLOCK_FILE).map(PathBuf::from),
            offset: AtomicI64::new(0),
        });
        let control = Arc::new(Control {
            secret: secret.into(),
            clock: Arc::clone(&clock),
            http: reqwest::Client::new(),
            messages_url: format!(
                "{}/2010-04-01/Accounts/{account_sid}/Messages.json",
                base.trim_end_matches('/')
            ),
            account_sid,
            auth_token: raw.remove(TWILIO_TOKEN).map(SecretString::from),
        });
        Ok(TestControl {
            router: routes(control),
            clock,
        })
    }

    fn read<I>(pairs: I) -> HashMap<&'static str, String>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        pairs
            .into_iter()
            .filter_map(|(name, value)| {
                let key = KEYS.iter().find(|key| **key == name)?;
                let value = value.trim().to_owned();
                (!value.is_empty()).then_some((*key, value))
            })
            .collect()
    }

    /// Every route sits behind the bearer check; an unmatched path is 404 anyway.
    fn routes(control: Arc<Control>) -> Router {
        Router::new()
            .route("/__test/clock", get(now))
            .route("/__test/clock/advance", post(advance))
            .route("/__test/messages", get(messages))
            .route_layer(middleware::from_fn_with_state(Arc::clone(&control), bearer))
            .with_state(control)
    }

    /// SEC-035: only the per-run secret passes; anything else looks like an absent route.
    async fn bearer(State(control): State<Arc<Control>>, request: Request, next: Next) -> Response {
        let given = request
            .headers()
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .unwrap_or_default();
        if same(given.as_bytes(), control.secret.expose_secret().as_bytes()) {
            next.run(request).await
        } else {
            StatusCode::NOT_FOUND.into_response()
        }
    }

    /// Compares without stopping at the first different byte.
    fn same(given: &[u8], secret: &[u8]) -> bool {
        given.len() == secret.len()
            && given
                .iter()
                .zip(secret)
                .fold(0_u8, |diff, (a, b)| diff | (a ^ b))
                == 0
    }

    #[derive(Serialize)]
    struct Now {
        now: String,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Advance {
        seconds: u32,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct SentQuery {
        to: Option<String>,
        key: Option<String>,
    }

    async fn now(State(control): State<Arc<Control>>) -> Json<Now> {
        Json(Now {
            now: control.clock.now().to_string(),
        })
    }

    async fn advance(
        State(control): State<Arc<Control>>,
        Json(body): Json<Advance>,
    ) -> Result<Json<Now>, StatusCode> {
        if body.seconds > MAX_ADVANCE_SECONDS {
            return Err(StatusCode::UNPROCESSABLE_ENTITY);
        }
        control
            .clock
            .advance(i64::from(body.seconds))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(now(State(control)).await)
    }

    /// The stand-in's sent log, filtered by `to` and the app's derived `key` (OPS-017, D-491).
    async fn messages(
        State(control): State<Arc<Control>>,
        Query(query): Query<SentQuery>,
    ) -> Result<Json<Value>, StatusCode> {
        let filters = [("To", query.to), ("MessageKey", query.key)];
        let search: Vec<String> = filters
            .iter()
            .filter_map(|(name, value)| Some(format!("{name}={}", encode(value.as_deref()?))))
            .collect();
        let url = format!("{}?{}", control.messages_url, search.join("&"));
        let token = control.auth_token.as_ref().map(ExposeSecret::expose_secret);
        let reply = control
            .http
            .get(url)
            .basic_auth(&control.account_sid, token)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        let body: Value = reply.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
        let sent = body.get("messages").cloned().unwrap_or_else(|| json!([]));
        Ok(Json(json!({ "messages": sent })))
    }

    /// Percent-encodes a query value (so `+1…` stays a plus).
    fn encode(value: &str) -> String {
        value
            .bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    char::from(b).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect()
    }

    /// System time plus a forward-only offset in seconds, shared through a
    /// file when one is set (D-740). The file read is a few bytes and runs only
    /// in test-control builds.
    struct OffsetClock {
        file: Option<PathBuf>,
        offset: AtomicI64,
    }

    impl OffsetClock {
        fn offset(&self) -> i64 {
            match &self.file {
                Some(path) => std::fs::read_to_string(path)
                    .ok()
                    .and_then(|text| text.trim().parse().ok())
                    .unwrap_or(0),
                None => self.offset.load(Ordering::SeqCst),
            }
        }

        fn advance(&self, seconds: i64) -> std::io::Result<()> {
            let next = self.offset().saturating_add(seconds);
            self.offset.store(next, Ordering::SeqCst);
            let Some(path) = &self.file else {
                return Ok(());
            };
            let staging = path.with_extension("next");
            std::fs::write(&staging, next.to_string())?;
            std::fs::rename(staging, path)
        }
    }

    impl Clock for OffsetClock {
        fn now(&self) -> Timestamp {
            let real = Timestamp::now();
            real.saturating_add(SignedDuration::from_secs(self.offset()))
                .unwrap_or(real)
        }
    }
}
