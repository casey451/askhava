//! Resend settings, parsed once at startup (SEC-001, SEC-002, SEC-025, SEC-032, SEC-036; T469,
//! D-3750).
//!
//! Shaped as [`crate::api::TwilioConfig`]. `ASKHAVA_ADAPTER_MODE_RESEND` picks `stand_in` or
//! `live` only: Resend has no sandbox and any valid key sends real email, so there is no
//! `sandbox` mode; `live` is accepted only with `APP_ENV=live` and `stand_in` never there
//! (SEC-002). `ASKHAVA_RESEND_BASE_URL` defaults to the stand-in in `stand_in` mode and to
//! `https://api.resend.com` in `live`, and passes `check_base_url`, so a stand-in or local URL
//! stops a live start (SEC-032). `ASKHAVA_RESEND_API_KEY` is a `SecretString`, never logged,
//! echoed or stored (SEC-001); the sending domain is `settings.account(Vendor::Resend)`, already on
//! its allow-list (SEC-036). Every error names the key, never the value.

use std::collections::HashMap;

use askhava_core::api::{Environment, Settings, SettingsError, Vendor, check_base_url};
use secrecy::SecretString;

use crate::base::AdapterError;
use crate::config::AdapterMode;
use crate::email::ResendEmail;

const MODE: &str = "ASKHAVA_ADAPTER_MODE_RESEND";
const BASE: &str = "ASKHAVA_RESEND_BASE_URL";
const API_KEY: &str = "ASKHAVA_RESEND_API_KEY";
const DOMAIN: &str = "ASKHAVA_RESEND_DOMAIN";
const KEYS: [&str; 3] = [MODE, BASE, API_KEY];

/// Where `python3 -m stand_ins.server` serves the fake Resend (SEC-032).
const STAND_IN_RESEND: &str = "http://127.0.0.1:4010/resend";
/// Resend's live API.
const LIVE_RESEND: &str = "https://api.resend.com";

/// The Resend client's settings, already checked against the environment (SEC-002, SEC-032).
#[derive(Debug)]
pub struct ResendConfig {
    mode: AdapterMode,
    base: String,
    api_key: SecretString,
    domain: String,
}

impl ResendConfig {
    /// Reads the Resend keys from the same pairs `Settings::from_pairs` took; the sending domain
    /// comes from `settings`, already on its allow-list (SEC-002, SEC-032, SEC-036).
    ///
    /// # Errors
    ///
    /// An unknown mode; `live` mode outside live or `stand_in` in live (SEC-002); a stand-in or
    /// local base URL in live (SEC-032); a missing API key or sending domain. Never a value.
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
        let default = match mode {
            AdapterMode::Live => LIVE_RESEND,
            AdapterMode::StandIn | AdapterMode::Sandbox => STAND_IN_RESEND,
        };
        let base = raw.remove(BASE).unwrap_or_else(|| default.to_owned());
        check_base_url(environment, BASE, &base)?;
        let domain = settings
            .account(Vendor::Resend)
            .ok_or(SettingsError::Missing(DOMAIN))?;
        let api_key = raw.remove(API_KEY).ok_or(SettingsError::Missing(API_KEY))?;
        Ok(Self {
            mode,
            base: base.trim_end_matches('/').to_owned(),
            api_key: api_key.into(),
            domain: domain.to_owned(),
        })
    }

    /// The mode in force: `StandIn` or `Live`, never `Sandbox` (SEC-002).
    #[must_use]
    pub fn mode(&self) -> AdapterMode {
        self.mode
    }

    /// The API base URL, without a trailing slash (SEC-032).
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base
    }

    /// The allow-listed sending domain (SEC-025, SEC-036).
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// The one Resend client over these settings (SEC-032).
    ///
    /// # Errors
    ///
    /// [`AdapterError::BadConfig`] for an unusable base URL.
    pub fn client(&self) -> Result<ResendEmail, AdapterError> {
        ResendEmail::new(&self.base, self.api_key.clone())
    }
}

/// SEC-002: `live` only in live and `stand_in` never there; no `sandbox`, since any valid Resend
/// key sends real email. The default is `live` in live and `stand_in` everywhere else.
fn parse_mode(environment: Environment, raw: Option<String>) -> Result<AdapterMode, SettingsError> {
    let live = environment == Environment::Live;
    let mode = match raw.as_deref() {
        None if live => AdapterMode::Live,
        None | Some("stand_in") => AdapterMode::StandIn,
        Some("live") => AdapterMode::Live,
        Some(_) => return Err(SettingsError::Invalid(MODE)),
    };
    match (live, mode) {
        (false, AdapterMode::Live) => Err(SettingsError::LiveKeyOutsideLive(MODE)),
        (true, AdapterMode::StandIn) => Err(SettingsError::StandInInLive(MODE)),
        _ => Ok(mode),
    }
}
