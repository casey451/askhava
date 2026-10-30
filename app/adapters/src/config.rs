//! Adapter mode and Twilio settings, parsed once at startup (SEC-002, SEC-032, CALL-017; D-160).
//!
//! `ASKHAVA_ADAPTER_MODE_TWILIO` picks `stand_in`, `sandbox` or `live`; `live`
//! is accepted only with `APP_ENV=live`, and neither `stand_in` nor `sandbox`
//! (test credentials) is accepted there (SEC-002).
//! The mode only picks the default base URLs; a base URL setting overrides
//! them and passes `check_base_url`, so a stand-in URL stops a live startup
//! (TD-SEC-13). The masked-number pool is a fixed list from settings whose
//! length must equal the pool size, 10 unless set (CALL-017, D-160): the app
//! never buys a number. Errors name the key, never the value (SEC-001).

use std::collections::HashMap;

use askhava_core::api::{Environment, Settings, SettingsError, Vendor, check_base_url};
use secrecy::SecretString;

const MODE: &str = "ASKHAVA_ADAPTER_MODE_TWILIO";
const API_BASE: &str = "ASKHAVA_TWILIO_BASE_URL";
const VERIFY_BASE: &str = "ASKHAVA_TWILIO_VERIFY_BASE_URL";
const AUTH_TOKEN: &str = "ASKHAVA_TWILIO_AUTH_TOKEN";
const VERIFY_SERVICE: &str = "ASKHAVA_TWILIO_VERIFY_SERVICE_SID";
const POOL_SIZE: &str = "ASKHAVA_MASKED_POOL_SIZE";
const POOL_NUMBERS: &str = "ASKHAVA_MASKED_NUMBERS";
const ACCOUNT_SID: &str = "ASKHAVA_TWILIO_ACCOUNT_SID";
const KEYS: [&str; 7] = [
    MODE,
    API_BASE,
    VERIFY_BASE,
    AUTH_TOKEN,
    VERIFY_SERVICE,
    POOL_SIZE,
    POOL_NUMBERS,
];

/// The masked-number pool size when `ASKHAVA_MASKED_POOL_SIZE` is unset (CALL-017, D-160).
pub const DEFAULT_POOL_SIZE: usize = 10;
/// Where `python3 -m stand_ins.server` serves the fake Twilio (SEC-032).
const STAND_IN_TWILIO: &str = "http://127.0.0.1:4010/twilio";

/// Which kind of vendor endpoint an adapter talks to (SEC-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterMode {
    /// The local stand-in server; never in live.
    StandIn,
    /// The vendor's test mode, with test credentials.
    Sandbox,
    /// The vendor's live API; only in live.
    Live,
}

/// One masked number from the fixed pool; only [`MaskedPool`] makes one, so the
/// voice adapter can dial out only from a pool number (CALL-017).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolNumber(String);

impl PoolNumber {
    /// The number in E.164 form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The masked numbers Casey bought, fixed by settings (CALL-017, D-160).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskedPool(Vec<PoolNumber>);

impl MaskedPool {
    /// The pool's numbers in settings order (CALL-017).
    #[must_use]
    pub fn numbers(&self) -> &[PoolNumber] {
        &self.0
    }
}

/// The Twilio adapters' settings, already checked against the environment.
#[derive(Debug)]
pub struct TwilioConfig {
    mode: AdapterMode,
    api_base: String,
    verify_base: String,
    account_sid: String,
    auth_token: SecretString,
    verify_service: String,
    pool: MaskedPool,
}

impl TwilioConfig {
    /// Reads the Twilio keys from the same pairs `Settings::from_pairs` took;
    /// the account SID comes from `settings`, already on its allow-list (SEC-036).
    ///
    /// # Errors
    ///
    /// A missing or invalid key; `live` mode outside live, or `stand_in` or
    /// `sandbox` in live (SEC-002); a stand-in or local base URL in live (SEC-032); a pool list
    /// that is not exactly the pool size of distinct E.164 numbers (CALL-017).
    pub fn from_settings<I>(settings: &Settings, pairs: I) -> Result<Self, SettingsError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut raw: HashMap<&'static str, String> = HashMap::new();
        for (name, value) in pairs {
            if let Some(key) = KEYS.iter().find(|key| **key == name)
                && !value.trim().is_empty()
            {
                raw.insert(key, value.trim().to_owned());
            }
        }
        let environment = settings.environment();
        let mode = parse_mode(environment, raw.remove(MODE))?;
        let (api_default, verify_default) = match mode {
            AdapterMode::StandIn => (STAND_IN_TWILIO, STAND_IN_TWILIO),
            AdapterMode::Sandbox | AdapterMode::Live => {
                ("https://api.twilio.com", "https://verify.twilio.com")
            }
        };
        let api_base = base_url(environment, API_BASE, raw.remove(API_BASE), api_default)?;
        let verify_base = base_url(
            environment,
            VERIFY_BASE,
            raw.remove(VERIFY_BASE),
            verify_default,
        )?;
        let account_sid = settings
            .account(Vendor::Twilio)
            .ok_or(SettingsError::Missing(ACCOUNT_SID))?;
        Ok(Self {
            mode,
            api_base,
            verify_base,
            account_sid: account_sid.to_owned(),
            auth_token: raw
                .remove(AUTH_TOKEN)
                .ok_or(SettingsError::Missing(AUTH_TOKEN))?
                .into(),
            verify_service: verify_service(raw.remove(VERIFY_SERVICE))?,
            pool: parse_pool(raw.remove(POOL_SIZE), raw.remove(POOL_NUMBERS))?,
        })
    }

    /// The mode in force (SEC-002).
    #[must_use]
    pub fn mode(&self) -> AdapterMode {
        self.mode
    }

    /// The Messages and Calls API base URL, without a trailing slash (SEC-032).
    #[must_use]
    pub fn api_base(&self) -> &str {
        &self.api_base
    }

    /// The Verify API base URL, without a trailing slash (SEC-032).
    #[must_use]
    pub fn verify_base(&self) -> &str {
        &self.verify_base
    }

    /// The Twilio Account SID.
    #[must_use]
    pub fn account_sid(&self) -> &str {
        &self.account_sid
    }

    /// The Twilio auth token, a secret (SEC-001).
    #[must_use]
    pub fn auth_token(&self) -> &SecretString {
        &self.auth_token
    }

    /// The Verify service SID (`VA…`).
    #[must_use]
    pub fn verify_service(&self) -> &str {
        &self.verify_service
    }

    /// The fixed masked-number pool (CALL-017).
    #[must_use]
    pub fn pool(&self) -> &MaskedPool {
        &self.pool
    }
}

/// SEC-002: live mode only in live; neither the stand-in nor the sandbox (a
/// test key) there; the default is `live` in live and `stand_in` everywhere else.
fn parse_mode(environment: Environment, raw: Option<String>) -> Result<AdapterMode, SettingsError> {
    let live = environment == Environment::Live;
    let mode = match raw.as_deref() {
        None if live => AdapterMode::Live,
        None | Some("stand_in") => AdapterMode::StandIn,
        Some("sandbox") => AdapterMode::Sandbox,
        Some("live") => AdapterMode::Live,
        Some(_) => return Err(SettingsError::Invalid(MODE)),
    };
    match (live, mode) {
        (false, AdapterMode::Live) => Err(SettingsError::LiveKeyOutsideLive(MODE)),
        (true, AdapterMode::StandIn) => Err(SettingsError::StandInInLive(MODE)),
        (true, AdapterMode::Sandbox) => Err(SettingsError::TestKeyInLive(MODE)),
        _ => Ok(mode),
    }
}

fn base_url(
    environment: Environment,
    key: &'static str,
    raw: Option<String>,
    default: &str,
) -> Result<String, SettingsError> {
    let url = raw.unwrap_or_else(|| default.to_owned());
    check_base_url(environment, key, &url)?;
    Ok(url.trim_end_matches('/').to_owned())
}

fn verify_service(raw: Option<String>) -> Result<String, SettingsError> {
    let sid = raw.ok_or(SettingsError::Missing(VERIFY_SERVICE))?;
    if !crate::base::is_sid(&sid, "VA") {
        return Err(SettingsError::Invalid(VERIFY_SERVICE));
    }
    Ok(sid)
}

/// CALL-017, D-160: exactly `size` distinct E.164 numbers, size 10 unless set.
fn parse_pool(size: Option<String>, numbers: Option<String>) -> Result<MaskedPool, SettingsError> {
    let size = size.map_or(Ok(DEFAULT_POOL_SIZE), |raw| {
        raw.parse::<usize>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or(SettingsError::Invalid(POOL_SIZE))
    })?;
    let numbers = numbers.ok_or(SettingsError::Missing(POOL_NUMBERS))?;
    let mut pool: Vec<PoolNumber> = Vec::with_capacity(size);
    for number in numbers.split(',').map(str::trim) {
        let digits = number.strip_prefix('+').unwrap_or_default();
        let valid = (8..=15).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit());
        if !valid || pool.iter().any(|known| known.0 == number) {
            return Err(SettingsError::Invalid(POOL_NUMBERS));
        }
        pool.push(PoolNumber(number.to_owned()));
    }
    if pool.len() != size {
        return Err(SettingsError::Invalid(POOL_NUMBERS));
    }
    Ok(MaskedPool(pool))
}
