//! Validated settings, parsed once at startup from `ASKHAVA_*` pairs (SEC-001, SEC-002, SEC-003, SEC-036).
//!
//! The binaries pass `std::env::vars()` and tests pass a map, so no test ever
//! calls `std::env::set_var` (D-481, D-491). The parser refuses an unknown
//! `ASKHAVA_*` key and ignores every other variable; no error ever echoes a
//! value, only the key name. Secrets are `SecretString`, whose `Debug` prints a
//! marker (SEC-001). The environment is `ASKHAVA_APP_ENV`, which the spec calls
//! `APP_ENV` (D-688). Each later task adds its own keys to [`KEYS`] and a typed
//! field below (D-493).

use std::collections::{BTreeSet, HashMap};
use std::net::{Ipv4Addr, Ipv6Addr};

use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;

const PREFIX: &str = "ASKHAVA_";
const APP_ENV: &str = "ASKHAVA_APP_ENV";
const DATABASE_URL: &str = "ASKHAVA_DATABASE_URL";
const DB_POOL_MAX: &str = "ASKHAVA_DB_POOL_MAX";
const LOG_LEVEL: &str = "ASKHAVA_LOG_LEVEL";
const SENTRY_DSN: &str = "ASKHAVA_SENTRY_DSN";
const STRIPE_SECRET_KEY: &str = "ASKHAVA_STRIPE_SECRET_KEY";
const ALLOW_LIST_TEST: &str = "ASKHAVA_ALLOW_LIST_TEST";
const ALLOW_LIST_LIVE: &str = "ASKHAVA_ALLOW_LIST_LIVE";

/// Every key the parser accepts; any other `ASKHAVA_*` key stops startup (D-493).
const KEYS: &[&str] = &[
    APP_ENV,
    DATABASE_URL,
    DB_POOL_MAX,
    LOG_LEVEL,
    SENTRY_DSN,
    STRIPE_SECRET_KEY,
    ALLOW_LIST_TEST,
    ALLOW_LIST_LIVE,
    "ASKHAVA_TWILIO_ACCOUNT_SID",
    "ASKHAVA_STRIPE_ACCOUNT_ID",
    "ASKHAVA_RESEND_DOMAIN",
    "ASKHAVA_STORAGE_BUCKET",
];

/// Key prefixes that only a live vendor account issues (SEC-002).
const LIVE_MARKERS: &[&str] = &["sk_live_", "rk_live_", "pk_live_"];
/// Key prefixes that only a vendor's test mode issues (SEC-002).
const TEST_MARKERS: &[&str] = &["sk_test_", "rk_test_", "pk_test_"];
const DEFAULT_POOL_MAX: u32 = 10;

/// Why startup refused the settings. Each variant names a key, never its value (SEC-001).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettingsError {
    /// A required key is absent or empty.
    #[error("missing required setting {0}")]
    Missing(&'static str),
    /// A key's value does not parse.
    #[error("setting {0} has an invalid value")]
    Invalid(&'static str),
    /// An `ASKHAVA_*` key no field reads (a typo, or a key from a removed feature).
    #[error("unknown setting {0}")]
    Unknown(String),
    /// A live vendor key outside the live environment (SEC-002).
    #[error("setting {0} holds a live key outside the live environment")]
    LiveKeyOutsideLive(&'static str),
    /// A test vendor key in the live environment (SEC-002).
    #[error("setting {0} holds a test key in the live environment")]
    TestKeyInLive(&'static str),
    /// A vendor account identifier on the other environment's allow-list (SEC-036).
    #[error("setting {0} names an account from another environment's allow-list")]
    ForeignAccount(&'static str),
    /// A vendor account identifier missing from this environment's allow-list (SEC-036).
    #[error("setting {0} names an account missing from this environment's allow-list")]
    NotAllowed(&'static str),
    /// A stand-in or local base URL in the live environment (SEC-032).
    #[error("setting {0} points at a stand-in in the live environment")]
    StandInInLive(&'static str),
}

/// Where the app runs (`BUILD_BRIEF.md:514`; SEC-003): build session, CI, the
/// Railway test project, or the Railway live project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    /// A build session on fake data and stand-ins.
    Dev,
    /// GitHub Actions on fake data and stand-ins.
    Ci,
    /// The Railway test project with test keys.
    Test,
    /// The Railway live project with live keys.
    Live,
}

impl Environment {
    fn parse(raw: &str) -> Result<Self, SettingsError> {
        match raw {
            "dev" => Ok(Self::Dev),
            "ci" => Ok(Self::Ci),
            "test" => Ok(Self::Test),
            "live" => Ok(Self::Live),
            _ => Err(SettingsError::Invalid(APP_ENV)),
        }
    }

    /// The lower-case name, as written in `ASKHAVA_APP_ENV` and sent to Sentry.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Ci => "ci",
            Self::Test => "test",
            Self::Live => "live",
        }
    }
}

/// A vendor whose account identifier is checked against the allow-lists (SEC-036).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Vendor {
    /// Twilio, by Account SID.
    Twilio,
    /// Stripe, by account ID.
    Stripe,
    /// Resend, by sending domain.
    Resend,
    /// Object storage, by bucket name.
    Storage,
}

impl Vendor {
    const ALL: [Self; 4] = [Self::Twilio, Self::Stripe, Self::Resend, Self::Storage];

    fn key(self) -> &'static str {
        match self {
            Self::Twilio => "ASKHAVA_TWILIO_ACCOUNT_SID",
            Self::Stripe => "ASKHAVA_STRIPE_ACCOUNT_ID",
            Self::Resend => "ASKHAVA_RESEND_DOMAIN",
            Self::Storage => "ASKHAVA_STORAGE_BUCKET",
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Self::Twilio => "twilio",
            Self::Stripe => "stripe",
            Self::Resend => "resend",
            Self::Storage => "bucket",
        }
    }
}

/// One environment's account identifiers, from a list such as
/// `twilio:AC1,stripe:acct_1,resend:mail.example.com,bucket:askhava-test` (SEC-036).
/// Entries are held, and looked up, in [`normalise_id`] form, so a stray space
/// or a case change cannot carry an identifier past the other list.
#[derive(Debug, Default)]
struct AllowList(BTreeSet<(Vendor, String)>);

impl AllowList {
    fn parse(key: &'static str, raw: &str) -> Result<Self, SettingsError> {
        let mut entries = BTreeSet::new();
        for item in raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            let (tag, id) = item.split_once(':').ok_or(SettingsError::Invalid(key))?;
            let vendor = Vendor::ALL
                .into_iter()
                .find(|vendor| vendor.tag() == tag.trim())
                .ok_or(SettingsError::Invalid(key))?;
            entries.insert((vendor, normalise_id(id)));
        }
        Ok(Self(entries))
    }

    fn contains(&self, vendor: Vendor, id: &str) -> bool {
        self.0.contains(&(vendor, normalise_id(id)))
    }
}

/// The comparison form of an account identifier (SEC-036): trimmed and
/// lower-cased. Domains and bucket names are case-insensitive; folding Twilio
/// and Stripe IDs too errs toward refusing, never toward letting one through.
fn normalise_id(id: &str) -> String {
    id.trim().to_lowercase()
}

/// The app's configuration, built only by [`Settings::from_pairs`], so every
/// value in it passed the startup checks (SEC-002, SEC-036).
#[derive(Debug)]
pub struct Settings {
    environment: Environment,
    database_url: SecretString,
    db_pool_max: u32,
    log_level: tracing::Level,
    sentry_dsn: Option<SecretString>,
    stripe_secret_key: Option<SecretString>,
    accounts: HashMap<Vendor, String>,
}

impl Settings {
    /// Parses and checks the settings (SEC-001, SEC-002, SEC-003, SEC-036).
    ///
    /// # Errors
    ///
    /// Refuses an unknown `ASKHAVA_*` key, a missing or invalid value, a live
    /// key outside live or a test key in live, and a vendor account identifier
    /// off this environment's allow-list or on the other's.
    pub fn from_pairs<I>(pairs: I) -> Result<Self, SettingsError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut raw = collect(pairs)?;
        let environment = Environment::parse(&required(&mut raw, APP_ENV)?)?;
        check_key_markers(environment, &raw)?;
        let stripe_secret_key = raw.remove(STRIPE_SECRET_KEY);
        if environment == Environment::Live
            && let Some(key) = &stripe_secret_key
            && !(key.starts_with("sk_live_") || key.starts_with("rk_live_"))
        {
            return Err(SettingsError::TestKeyInLive(STRIPE_SECRET_KEY));
        }
        let accounts = check_accounts(environment, &mut raw)?;
        Ok(Self {
            environment,
            database_url: required(&mut raw, DATABASE_URL)?.into(),
            db_pool_max: pool_max(raw.remove(DB_POOL_MAX))?,
            log_level: log_level(raw.remove(LOG_LEVEL))?,
            sentry_dsn: raw.remove(SENTRY_DSN).map(SecretString::from),
            stripe_secret_key: stripe_secret_key.map(SecretString::from),
            accounts,
        })
    }

    /// Which environment this process runs in (SEC-003).
    #[must_use]
    pub fn environment(&self) -> Environment {
        self.environment
    }

    /// The app role's database URL (`ASKHAVA_DATABASE_URL`), a secret.
    #[must_use]
    pub fn database_url(&self) -> &SecretString {
        &self.database_url
    }

    /// The pool's `max_connections` per process (`ASKHAVA_DB_POOL_MAX`, 1 to 100, default 10).
    #[must_use]
    pub fn db_pool_max(&self) -> u32 {
        self.db_pool_max
    }

    /// The lowest log level written (`ASKHAVA_LOG_LEVEL`, default `info`).
    #[must_use]
    pub fn log_level(&self) -> tracing::Level {
        self.log_level
    }

    /// The Sentry DSN (`ASKHAVA_SENTRY_DSN`); Sentry is off without one (PRIV-017).
    #[must_use]
    pub fn sentry_dsn(&self) -> Option<&SecretString> {
        self.sentry_dsn.as_ref()
    }

    /// The Stripe secret key, already matched to the environment (SEC-002).
    #[must_use]
    pub fn stripe_secret_key(&self) -> Option<&SecretString> {
        self.stripe_secret_key.as_ref()
    }

    /// A vendor's account identifier, already checked against the allow-lists (SEC-036).
    #[must_use]
    pub fn account(&self, vendor: Vendor) -> Option<&str> {
        self.accounts.get(&vendor).map(String::as_str)
    }
}

/// Checks a vendor base URL for its environment (SEC-002, SEC-032; TD-SEC-13):
/// live accepts only `https://` to a non-local host; elsewhere `http://`,
/// `https://` and a `file://` stand-in directory are allowed (D-493). The host
/// is read after any `user:pass@`, lower-cased, and an IPv4 or bracketed IPv6
/// loopback or unspecified address counts as local (D-690).
///
/// # Errors
///
/// [`SettingsError::StandInInLive`] for a stand-in or local URL in live, and
/// [`SettingsError::Invalid`] for any other scheme.
pub fn check_base_url(
    environment: Environment,
    key: &'static str,
    url: &str,
) -> Result<(), SettingsError> {
    let (scheme, rest) = url.split_once("://").ok_or(SettingsError::Invalid(key))?;
    if !matches!(scheme, "https" | "http" | "file") {
        return Err(SettingsError::Invalid(key));
    }
    if environment == Environment::Live && (scheme != "https" || is_local_host(&url_host(rest))) {
        return Err(SettingsError::StandInInLive(key));
    }
    Ok(())
}

/// The lower-cased host of a URL's remainder after `://`, with any userinfo,
/// port, path, query and fragment removed; an IPv6 host keeps no brackets.
fn url_host(rest: &str) -> String {
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = match host_port.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or_default(),
        None => host_port.split(':').next().unwrap_or_default(),
    };
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// Whether a host is where a stand-in runs (SEC-032): `localhost` or a
/// subdomain of it, any `127.` address, or a loopback, unspecified or
/// IPv4-mapped loopback address.
fn is_local_host(host: &str) -> bool {
    if host == "localhost" || host.ends_with(".localhost") || host.starts_with("127.") {
        return true;
    }
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return ip.is_loopback() || ip.is_unspecified();
    }
    host.parse::<Ipv6Addr>().is_ok_and(|ip| {
        ip.is_loopback()
            || ip.is_unspecified()
            || ip
                .to_ipv4_mapped()
                .is_some_and(|v4| v4.is_loopback() || v4.is_unspecified())
    })
}

fn collect<I>(pairs: I) -> Result<HashMap<&'static str, String>, SettingsError>
where
    I: IntoIterator<Item = (String, String)>,
{
    let mut raw = HashMap::new();
    for (name, value) in pairs {
        if !name.starts_with(PREFIX) {
            continue;
        }
        let key = KEYS
            .iter()
            .find(|known| **known == name)
            .ok_or(SettingsError::Unknown(name))?;
        if !value.trim().is_empty() {
            raw.insert(*key, value);
        }
    }
    Ok(raw)
}

fn required(
    raw: &mut HashMap<&'static str, String>,
    key: &'static str,
) -> Result<String, SettingsError> {
    raw.remove(key).ok_or(SettingsError::Missing(key))
}

/// SEC-002: every value is scanned, so a key a later task adds is covered too.
fn check_key_markers(
    environment: Environment,
    raw: &HashMap<&'static str, String>,
) -> Result<(), SettingsError> {
    let has = |value: &str, markers: &[&str]| markers.iter().any(|marker| value.contains(marker));
    for (key, value) in raw {
        if environment != Environment::Live && has(value, LIVE_MARKERS) {
            return Err(SettingsError::LiveKeyOutsideLive(key));
        }
        if environment == Environment::Live && has(value, TEST_MARKERS) {
            return Err(SettingsError::TestKeyInLive(key));
        }
    }
    Ok(())
}

/// SEC-036, SEC-003: live needs every identifier and both lists, so the
/// foreign-account check always runs there; test needs its own list; dev and
/// CI only refuse a live identifier.
fn check_accounts(
    environment: Environment,
    raw: &mut HashMap<&'static str, String>,
) -> Result<HashMap<Vendor, String>, SettingsError> {
    let test = optional_list(raw, ALLOW_LIST_TEST)?;
    let live = optional_list(raw, ALLOW_LIST_LIVE)?;
    let (own, other) = match environment {
        Environment::Live => (
            Some(require_list(live, ALLOW_LIST_LIVE)?),
            Some(require_list(test, ALLOW_LIST_TEST)?),
        ),
        Environment::Test => (Some(require_list(test, ALLOW_LIST_TEST)?), live),
        Environment::Dev | Environment::Ci => (None, live),
    };
    let mut accounts = HashMap::new();
    for vendor in Vendor::ALL {
        let Some(id) = raw.remove(vendor.key()) else {
            if environment == Environment::Live {
                return Err(SettingsError::Missing(vendor.key()));
            }
            continue;
        };
        if other
            .as_ref()
            .is_some_and(|list| list.contains(vendor, &id))
        {
            return Err(SettingsError::ForeignAccount(vendor.key()));
        }
        if own.as_ref().is_some_and(|list| !list.contains(vendor, &id)) {
            return Err(SettingsError::NotAllowed(vendor.key()));
        }
        accounts.insert(vendor, id.trim().to_owned());
    }
    Ok(accounts)
}

fn optional_list(
    raw: &mut HashMap<&'static str, String>,
    key: &'static str,
) -> Result<Option<AllowList>, SettingsError> {
    raw.remove(key)
        .map(|list| AllowList::parse(key, &list))
        .transpose()
}

fn require_list(list: Option<AllowList>, key: &'static str) -> Result<AllowList, SettingsError> {
    list.ok_or(SettingsError::Missing(key))
}

fn pool_max(raw: Option<String>) -> Result<u32, SettingsError> {
    let Some(raw) = raw else {
        return Ok(DEFAULT_POOL_MAX);
    };
    match raw.trim().parse::<u32>() {
        Ok(max @ 1..=100) => Ok(max),
        _ => Err(SettingsError::Invalid(DB_POOL_MAX)),
    }
}

fn log_level(raw: Option<String>) -> Result<tracing::Level, SettingsError> {
    raw.map_or(Ok(tracing::Level::INFO), |level| {
        level
            .trim()
            .parse()
            .map_err(|_| SettingsError::Invalid(LOG_LEVEL))
    })
}

/// Reads a secret for the one place that needs its value (SEC-001).
pub(crate) fn expose(secret: &SecretString) -> &str {
    secret.expose_secret()
}
