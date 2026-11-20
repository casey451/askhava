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
const MIGRATION_DATABASE_URL: &str = "ASKHAVA_MIGRATION_DATABASE_URL";
const DB_POOL_MAX: &str = "ASKHAVA_DB_POOL_MAX";
const LOG_LEVEL: &str = "ASKHAVA_LOG_LEVEL";
const SENTRY_DSN: &str = "ASKHAVA_SENTRY_DSN";
const STRIPE_SECRET_KEY: &str = "ASKHAVA_STRIPE_SECRET_KEY";
const ALLOW_LIST_TEST: &str = "ASKHAVA_ALLOW_LIST_TEST";
const ALLOW_LIST_LIVE: &str = "ASKHAVA_ALLOW_LIST_LIVE";
const PUBLIC_BASE_URL: &str = "ASKHAVA_PUBLIC_BASE_URL";
const REQUEST_TIMEOUT_SECS: &str = "ASKHAVA_REQUEST_TIMEOUT_SECS";
const POSTAL_ADDRESS: &str = "ASKHAVA_POSTAL_ADDRESS";
const STATIC_DIR: &str = "ASKHAVA_STATIC_DIR";
const LICENSE_BASE_URL: &str = "ASKHAVA_LICENSE_BASE_URL";
const SECURITY_EVENTS_PER_MINUTE: &str = "ASKHAVA_SECURITY_EVENTS_PER_MINUTE";
const MAIN_NUMBER: &str = "ASKHAVA_MAIN_NUMBER";
const STRIPE_PUBLISHABLE_KEY: &str = "ASKHAVA_STRIPE_PUBLISHABLE_KEY";
const LAUNCH_DATE: &str = "ASKHAVA_LAUNCH_DATE";
const SET_ASIDE: &str = "ASKHAVA_SALES_TAX_SET_ASIDE_BASIS_POINTS";
const HOME_NUMBERS_MIN_JOBS: &str = "ASKHAVA_HOME_NUMBERS_MIN_JOBS";
const SPOT_CHECK_EVERY_DAYS: &str = "ASKHAVA_SPOT_CHECK_EVERY_DAYS";
const GOLDEN_TARGET: &str = "ASKHAVA_GOLDEN_TARGET_PERCENT";
const LLC_RECORD_URL: &str = "ASKHAVA_LLC_RECORD_URL";
const CITY_LICENSE: &str = "ASKHAVA_CITY_BUSINESS_LICENSE_NUMBER";
const CITY_LICENSE_COST: &str = "ASKHAVA_CITY_LICENSE_COST";
const CITY_LICENSE_TIME: &str = "ASKHAVA_CITY_LICENSE_TIME";
const CITY_LICENSE_URL: &str = "ASKHAVA_CITY_LICENSE_URL";
const DB_VOLUME_BYTES: &str = "ASKHAVA_DB_VOLUME_BYTES";
const OWNER_PHONE: &str = "ASKHAVA_OWNER_PHONE";
const EXPORT_RECIPIENT: &str = "ASKHAVA_EXPORT_RECIPIENT";
const BACKUP_DATABASE_URL: &str = "ASKHAVA_BACKUP_DATABASE_URL";
const RESTORE_ADMIN_URL: &str = "ASKHAVA_RESTORE_ADMIN_URL";

/// Every key the parser accepts; any other `ASKHAVA_*` key stops startup (D-493).
const KEYS: &[&str] = &[
    APP_ENV,
    DATABASE_URL,
    MIGRATION_DATABASE_URL,
    DB_POOL_MAX,
    LOG_LEVEL,
    SENTRY_DSN,
    STRIPE_SECRET_KEY,
    STRIPE_PUBLISHABLE_KEY,
    LAUNCH_DATE,
    SET_ASIDE,
    ALLOW_LIST_TEST,
    ALLOW_LIST_LIVE,
    // The app's own public origin, the base of every signed webhook URL (T241, SEC-016, D-1216).
    PUBLIC_BASE_URL,
    // The web process's whole-request time limit (T286, D-1870).
    REQUEST_TIMEOUT_SECS,
    // The business email footer's postal line, checked by `messaging` (T313, SEC-028, D-3095).
    POSTAL_ADDRESS,
    // The folder of built browser files the web process serves at `/static/` (T362, D-2633).
    STATIC_DIR,
    // The license lists' and records' base URL, optional here; the composition that builds the
    // license source requires it in `test` and `live` (T385, SEC-032, D-2954).
    LICENSE_BASE_URL,
    // The security-event rows per minute per key, raised to `portals`' alert threshold
    // (T319, SEC-048, D-2207).
    SECURITY_EVENTS_PER_MINUTE,
    // The Laftly main number, read as the safety call's callback (T330, CALL-022, D-3134).
    MAIN_NUMBER,
    // The home page's numbers threshold and the "Who runs" block's record link and city license
    // number, each optional (T102, WEB-021, WEB-028, D-3878, D-3890).
    HOME_NUMBERS_MIN_JOBS,
    LLC_RECORD_URL,
    CITY_LICENSE,
    // The business page's city-license guide: cost, time and link, each optional (T202, WEB-020,
    // D-3880).
    CITY_LICENSE_COST,
    CITY_LICENSE_TIME,
    CITY_LICENSE_URL,
    // The database volume's size in bytes for the daily volume check, optional here; `server`'s
    // `watch_checks` registration requires it in `test` and `live` (T525, OPS-052, D-4044, D-4280).
    DB_VOLUME_BYTES,
    // Casey's own phone for the owner calls, optional here; `server`'s owner-call registration
    // requires it in `test` and `live` (T484, OPS-028, OPS-035, D-3890).
    OWNER_PHONE,
    // T106 (OPS-007, OPS-049, D-3144, D-4044): parsed by `server`'s `liveness` and `healthz`: how
    // long a failed liveness check may last before the process exits, and how long due work may
    // wait before `/healthz` reports the worker down.
    "ASKHAVA_LIVENESS_LIMIT_SECS",
    "ASKHAVA_WORKER_DOWN_SECS",
    "ASKHAVA_TWILIO_ACCOUNT_SID",
    "ASKHAVA_STRIPE_ACCOUNT_ID",
    "ASKHAVA_RESEND_DOMAIN",
    "ASKHAVA_STORAGE_BUCKET",
    // Parsed by `askhava_adapters::api::TwilioConfig` (T016, D-703).
    "ASKHAVA_ADAPTER_MODE_TWILIO",
    "ASKHAVA_TWILIO_BASE_URL",
    "ASKHAVA_TWILIO_VERIFY_BASE_URL",
    "ASKHAVA_TWILIO_AUTH_TOKEN",
    "ASKHAVA_TWILIO_VERIFY_SERVICE_SID",
    "ASKHAVA_MASKED_POOL_SIZE",
    "ASKHAVA_MASKED_NUMBERS",
    // Parsed by `askhava_adapters::api::ResendConfig` (T469, SEC-001, SEC-002, SEC-032, D-3750).
    "ASKHAVA_ADAPTER_MODE_RESEND",
    "ASKHAVA_RESEND_BASE_URL",
    "ASKHAVA_RESEND_API_KEY",
    // The email provider's daily and monthly limits and the bulk-hold share, parsed by
    // `askhava_server::email_provider` (T469, SEC-025, OPS-038, D-3750, D-3890).
    "ASKHAVA_EMAIL_DAILY_LIMIT",
    "ASKHAVA_EMAIL_MONTHLY_LIMIT",
    "ASKHAVA_EMAIL_BULK_HOLD_PERCENT",
    // OPS-009's spot-check cadence in days (default 30), read by the serving route (T304) for
    // T476's owner health page (D-4280, D-4625).
    SPOT_CHECK_EVERY_DAYS,
    // OPS-040's golden-set target in whole percent (1 to 100, default 90), read by the serving
    // route for T434's intake accuracy report (D-395, D-3890).
    GOLDEN_TARGET,
    // OPS-035's alert address, read as a `SecretString` by `askhava_server`'s alert email sender
    // and never logged (T204, D-3882, D-4044).
    "ASKHAVA_ALERT_EMAIL",
    // OPS-041's owner-inbox address for the not-covered email (and T524's weekly digest), read
    // as a `SecretString` by `askhava_server` and never logged (T490, D-3882).
    "ASKHAVA_OWNER_INBOX_EMAIL",
    // The homeowner line's one configured number, parsed by `askhava_server` for `messaging`'s
    // outbox sender (T356, TXT-022, D-2628).
    "ASKHAVA_HOMEOWNER_NUMBER",
    // Parsed by `rate_limit::ClientIpPolicy::from_pairs` (T013, SEC-040).
    "ASKHAVA_CLIENT_IP_HEADER",
    "ASKHAVA_PROXIED_HOSTS",
    // Parsed by `askhava_server::offdb_stream::OffDbStream::from_settings` (T017, D-734).
    "ASKHAVA_STORAGE_BASE_URL",
    "ASKHAVA_ID_SECRET",
    // Parsed by `test_control::TestControl::from_settings`, only under `test-control` (T020, SEC-035).
    "ASKHAVA_TEST_CONTROL_SECRET",
    "ASKHAVA_TEST_CONTROL_CLOCK_FILE",
    // Parsed by `askhava_server::release_sink::stripe_webhook` (T254, SEC-039, SEC-049, D-1480).
    "ASKHAVA_STRIPE_WEBHOOK_SECRET",
    "ASKHAVA_ADAPTER_MODE_STRIPE",
    "ASKHAVA_STRIPE_BASE_URL",
    // Parsed by `askhava_ai::api::AiConfig::from_pairs` (T024, AI-021, AI-023, AI-024).
    "ASKHAVA_AI_MODEL",
    "ASKHAVA_AI_TIMEOUT_SECONDS",
    "ASKHAVA_AI_DAILY_CAP",
    // Parsed by `askhava_picks::api::ConfirmSettings::from_pairs` (T063, PICK-019, D-1503).
    "ASKHAVA_PICK_CONFIRM_WINDOW_MINUTES",
    // Parsed by `askhava_calls::api::PinWait::from_pairs` (T080, CALL-013).
    "ASKHAVA_PIN_WAIT_SECONDS",
    // Parsed by `askhava_businesses::api::SignupSettings::from_pairs` (T193, BIZ-026, BIZ-027).
    "ASKHAVA_BIZ_REVIEW_TARGET_DAYS",
    "ASKHAVA_BIZ_LIST_DELAY_DAYS",
    // Parsed by `askhava_licensing::api::DailyListSettings::from_pairs` (T371, LIC-039, D-2704).
    "ASKHAVA_LICENSE_LIST_HOUR",
    "ASKHAVA_LICENSE_LIST_RETRY_MINUTES",
    "ASKHAVA_LICENSE_LOOKUP_BATCH",
    // Read by `server bootstrap-owner` alone, from the host variable, never stored (T303, SEC-044,
    // D-1918, D-3962).
    "ASKHAVA_OWNER_BOOTSTRAP_TOKEN",
    // OPS-039's go and stop lines and D-458's paid-introduction ladder, each optional with its
    // `docs/PREBUILD.md` default in `SCORECARD_DEFAULTS` (T621, D-4280, D-4448).
    "ASKHAVA_SCORECARD_QUOTED_24H_GO",
    "ASKHAVA_SCORECARD_QUOTED_24H_STOP",
    "ASKHAVA_SCORECARD_TWO_QUOTES_24H_GO",
    "ASKHAVA_SCORECARD_TWO_QUOTES_24H_STOP",
    "ASKHAVA_SCORECARD_P75_FIRST_QUOTE_HOURS_GO",
    "ASKHAVA_SCORECARD_P75_FIRST_QUOTE_HOURS_STOP",
    "ASKHAVA_SCORECARD_MEDIAN_FIRST_QUOTE_HOURS_GO",
    "ASKHAVA_SCORECARD_MEDIAN_FIRST_QUOTE_HOURS_STOP",
    "ASKHAVA_SCORECARD_PICKS_PER_QUOTED_GO",
    "ASKHAVA_SCORECARD_PICKS_PER_QUOTED_STOP",
    "ASKHAVA_SCORECARD_PICKS_CONFIRMED_GO",
    "ASKHAVA_SCORECARD_PICKS_CONFIRMED_STOP",
    "ASKHAVA_SCORECARD_AVERAGE_FEE_DOLLARS_GO",
    "ASKHAVA_SCORECARD_AVERAGE_FEE_DOLLARS_STOP",
    "ASKHAVA_SCORECARD_FREE_SHARE_GO",
    "ASKHAVA_SCORECARD_FREE_SHARE_STOP",
    "ASKHAVA_SCORECARD_REQUESTS_PER_WEEK_GO",
    "ASKHAVA_SCORECARD_REQUESTS_PER_WEEK_STOP",
    "ASKHAVA_SCORECARD_PAID_INTROS_MONTH_GO",
    "ASKHAVA_SCORECARD_PAID_INTROS_MONTH_STOP",
    "ASKHAVA_SCORECARD_NET_PER_INTRO_DOLLARS_GO",
    "ASKHAVA_SCORECARD_NET_PER_INTRO_DOLLARS_STOP",
    "ASKHAVA_SCORECARD_INTAKE_CARD_STEP_GO",
    "ASKHAVA_SCORECARD_INTAKE_CARD_STEP_STOP",
    "ASKHAVA_SCORECARD_SIGNUPS_ACTIVE_15MIN_GO",
    "ASKHAVA_SCORECARD_SIGNUPS_ACTIVE_15MIN_STOP",
    "ASKHAVA_SCORECARD_REFUNDS_GO",
    "ASKHAVA_SCORECARD_REFUNDS_STOP",
    "ASKHAVA_SCORECARD_BANK_DISPUTES_GO",
    "ASKHAVA_SCORECARD_BANK_DISPUTES_STOP",
    "ASKHAVA_SCORECARD_PAYING_AGAIN_60D_GO",
    "ASKHAVA_SCORECARD_PAYING_AGAIN_60D_STOP",
    "ASKHAVA_SCORECARD_MINUTES_PER_INTRO_GO",
    "ASKHAVA_SCORECARD_MINUTES_PER_INTRO_STOP",
    "ASKHAVA_SCORECARD_COMPLAINTS_PER_20_GO",
    "ASKHAVA_SCORECARD_COMPLAINTS_PER_20_STOP",
    "ASKHAVA_SCORECARD_LADDER_PILOT",
    "ASKHAVA_SCORECARD_LADDER_20K",
    "ASKHAVA_SCORECARD_LADDER_50K",
    // The nightly export's offline recipient public key and the `askhava_backup` role's URL, read
    // only by the worker's export (T109, OPS-013, PRIV-024, D-4293); never a private key.
    EXPORT_RECIPIENT,
    BACKUP_DATABASE_URL,
    // The throwaway PostgreSQL service's admin URL, read only by the worker's monthly test
    // restore (T648, OPS-013, OPS-051, SEC-052, D-4293).
    RESTORE_ADMIN_URL,
];

/// Key prefixes that only a live vendor account issues (SEC-002).
const LIVE_MARKERS: &[&str] = &["sk_live_", "rk_live_", "pk_live_"];
/// Key prefixes that only a vendor's test mode issues (SEC-002).
const TEST_MARKERS: &[&str] = &["sk_test_", "rk_test_", "pk_test_"];
const DEFAULT_POOL_MAX: u32 = 10;
/// FEE-046's default sales-tax set-aside: 860 basis points, 8.6% (D-3844).
const DEFAULT_SET_ASIDE_BASIS_POINTS: u16 = 860;
/// The default whole-request time limit in seconds (D-1870).
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 60;
/// WEB-028's default threshold: 50 requests with a card sent (D-468).
const DEFAULT_HOME_NUMBERS_MIN_JOBS: u32 = 50;
/// OPS-009's default spot-check cadence in days (`ASKHAVA_SPOT_CHECK_EVERY_DAYS`, D-4280, D-4625).
pub const DEFAULT_SPOT_CHECK_EVERY_DAYS: u32 = 30;
/// OPS-040's default golden-set target: 90% of rows reach the right job type with no human (D-395).
pub const DEFAULT_GOLDEN_TARGET_PERCENT: u32 = 90;
/// OPS-039's go and stop lines and D-458's ladder with their `docs/PREBUILD.md` section 7 defaults,
/// in whole percent, hours, dollars, minutes or counts as each key names; a missing key takes its
/// default (T621, D-4280, D-4448). `portals::api::ScorecardLines::from_pairs` reads them by key.
const SCORECARD_DEFAULTS: [(&str, u32); 39] = [
    ("ASKHAVA_SCORECARD_QUOTED_24H_GO", 70),
    ("ASKHAVA_SCORECARD_QUOTED_24H_STOP", 50),
    ("ASKHAVA_SCORECARD_TWO_QUOTES_24H_GO", 60),
    ("ASKHAVA_SCORECARD_TWO_QUOTES_24H_STOP", 30),
    ("ASKHAVA_SCORECARD_P75_FIRST_QUOTE_HOURS_GO", 12),
    ("ASKHAVA_SCORECARD_P75_FIRST_QUOTE_HOURS_STOP", 24),
    ("ASKHAVA_SCORECARD_MEDIAN_FIRST_QUOTE_HOURS_GO", 4),
    ("ASKHAVA_SCORECARD_MEDIAN_FIRST_QUOTE_HOURS_STOP", 12),
    ("ASKHAVA_SCORECARD_PICKS_PER_QUOTED_GO", 40),
    ("ASKHAVA_SCORECARD_PICKS_PER_QUOTED_STOP", 25),
    ("ASKHAVA_SCORECARD_PICKS_CONFIRMED_GO", 85),
    ("ASKHAVA_SCORECARD_PICKS_CONFIRMED_STOP", 75),
    ("ASKHAVA_SCORECARD_AVERAGE_FEE_DOLLARS_GO", 50),
    ("ASKHAVA_SCORECARD_AVERAGE_FEE_DOLLARS_STOP", 35),
    ("ASKHAVA_SCORECARD_FREE_SHARE_GO", 20),
    ("ASKHAVA_SCORECARD_FREE_SHARE_STOP", 40),
    ("ASKHAVA_SCORECARD_REQUESTS_PER_WEEK_GO", 15),
    ("ASKHAVA_SCORECARD_REQUESTS_PER_WEEK_STOP", 6),
    ("ASKHAVA_SCORECARD_PAID_INTROS_MONTH_GO", 15),
    ("ASKHAVA_SCORECARD_PAID_INTROS_MONTH_STOP", 5),
    ("ASKHAVA_SCORECARD_NET_PER_INTRO_DOLLARS_GO", 40),
    ("ASKHAVA_SCORECARD_NET_PER_INTRO_DOLLARS_STOP", 28),
    ("ASKHAVA_SCORECARD_INTAKE_CARD_STEP_GO", 80),
    ("ASKHAVA_SCORECARD_INTAKE_CARD_STEP_STOP", 60),
    ("ASKHAVA_SCORECARD_SIGNUPS_ACTIVE_15MIN_GO", 70),
    ("ASKHAVA_SCORECARD_SIGNUPS_ACTIVE_15MIN_STOP", 40),
    ("ASKHAVA_SCORECARD_REFUNDS_GO", 10),
    ("ASKHAVA_SCORECARD_REFUNDS_STOP", 20),
    ("ASKHAVA_SCORECARD_BANK_DISPUTES_GO", 1),
    ("ASKHAVA_SCORECARD_BANK_DISPUTES_STOP", 2),
    ("ASKHAVA_SCORECARD_PAYING_AGAIN_60D_GO", 50),
    ("ASKHAVA_SCORECARD_PAYING_AGAIN_60D_STOP", 30),
    ("ASKHAVA_SCORECARD_MINUTES_PER_INTRO_GO", 20),
    ("ASKHAVA_SCORECARD_MINUTES_PER_INTRO_STOP", 45),
    ("ASKHAVA_SCORECARD_COMPLAINTS_PER_20_GO", 1),
    ("ASKHAVA_SCORECARD_COMPLAINTS_PER_20_STOP", 2),
    ("ASKHAVA_SCORECARD_LADDER_PILOT", 15),
    ("ASKHAVA_SCORECARD_LADDER_20K", 50),
    ("ASKHAVA_SCORECARD_LADDER_50K", 115),
];

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
    migration_database_url: Option<SecretString>,
    restore_admin_url: Option<SecretString>,
    db_pool_max: u32,
    log_level: tracing::Level,
    sentry_dsn: Option<SecretString>,
    stripe_secret_key: Option<SecretString>,
    accounts: HashMap<Vendor, String>,
    public_base_url: Option<String>,
    request_timeout_secs: u64,
    postal_address: Option<String>,
    static_dir: Option<String>,
    license_base_url: Option<String>,
    security_events_per_minute: Option<u32>,
    main_number: Option<String>,
    stripe_publishable_key: Option<String>,
    launch_date: Option<jiff::Timestamp>,
    set_aside_basis_points: u16,
    home_numbers_min_jobs: u32,
    /// OPS-009's spot-check cadence in days and OPS-040's golden-set target in percent.
    owner_reports: (u32, u32),
    llc_record_url: Option<String>,
    city_license: Option<String>,
    city_guide: [Option<String>; 3],
    scorecard: [u32; SCORECARD_DEFAULTS.len()],
    db_volume_bytes: Option<u64>,
    owner_phone: Option<SecretString>,
    /// The export recipient and the backup role's URL (T109).
    export: (Option<String>, Option<SecretString>),
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
        let public_base_url = public_base_url(environment, raw.remove(PUBLIC_BASE_URL))?;
        Ok(Self {
            environment,
            database_url: required(&mut raw, DATABASE_URL)?.into(),
            migration_database_url: raw.remove(MIGRATION_DATABASE_URL).map(SecretString::from),
            restore_admin_url: raw.remove(RESTORE_ADMIN_URL).map(SecretString::from),
            db_pool_max: pool_max(raw.remove(DB_POOL_MAX))?,
            log_level: log_level(raw.remove(LOG_LEVEL))?,
            sentry_dsn: raw.remove(SENTRY_DSN).map(SecretString::from),
            stripe_secret_key: stripe_secret_key.map(SecretString::from),
            accounts,
            public_base_url,
            request_timeout_secs: request_timeout_secs(raw.remove(REQUEST_TIMEOUT_SECS))?,
            postal_address: raw.remove(POSTAL_ADDRESS).map(|p| p.trim().to_owned()),
            static_dir: raw.remove(STATIC_DIR).map(|d| d.trim().to_owned()),
            license_base_url: license_base_url(environment, raw.remove(LICENSE_BASE_URL))?,
            security_events_per_minute: per_minute(raw.remove(SECURITY_EVENTS_PER_MINUTE))?,
            main_number: main_number(raw.remove(MAIN_NUMBER))?,
            stripe_publishable_key: publishable_key(raw.remove(STRIPE_PUBLISHABLE_KEY))?,
            launch_date: launch_date(raw.remove(LAUNCH_DATE))?,
            set_aside_basis_points: set_aside(raw.remove(SET_ASIDE))?,
            home_numbers_min_jobs: above_zero(
                &mut raw,
                HOME_NUMBERS_MIN_JOBS,
                DEFAULT_HOME_NUMBERS_MIN_JOBS,
            )?,
            owner_reports: owner_reports(&mut raw)?,
            llc_record_url: raw.remove(LLC_RECORD_URL).map(|u| u.trim().to_owned()),
            city_license: raw.remove(CITY_LICENSE).map(|n| n.trim().to_owned()),
            city_guide: [CITY_LICENSE_COST, CITY_LICENSE_TIME, CITY_LICENSE_URL]
                .map(|key| raw.remove(key).map(|v| v.trim().to_owned())),
            scorecard: scorecard(&mut raw)?,
            db_volume_bytes: volume_bytes(raw.remove(DB_VOLUME_BYTES))?,
            owner_phone: e164(raw.remove(OWNER_PHONE), OWNER_PHONE)?.map(SecretString::from),
            export: export_keys(&mut raw)?,
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

    /// The migration role's database URL (`ASKHAVA_MIGRATION_DATABASE_URL`), a secret set only
    /// on the `migrate` service and read only by `server migrate` (D-092, SEC-052).
    ///
    /// # Errors
    ///
    /// [`SettingsError::Missing`] when the key is absent.
    pub fn migration_database_url(&self) -> Result<&SecretString, SettingsError> {
        self.migration_database_url
            .as_ref()
            .ok_or(SettingsError::Missing(MIGRATION_DATABASE_URL))
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

    /// The app's public origin (`ASKHAVA_PUBLIC_BASE_URL`), without a trailing slash: Twilio
    /// signs the full URL it posted to, so each webhook's URL is this plus the route path and
    /// query (SEC-016, D-1216). `None` while unset; never hard-coded.
    #[must_use]
    pub fn public_base_url(&self) -> Option<&str> {
        self.public_base_url.as_deref()
    }

    /// The security-event rows stored per minute per (kind, client address, route) and per
    /// (kind, route) (`ASKHAVA_SECURITY_EVENTS_PER_MINUTE`, a whole number above 0; unset is
    /// `None`). `portals::api::configure_security_events` raises a value below its alert
    /// threshold to it (T319, SEC-048, D-2207).
    #[must_use]
    pub fn security_events_per_minute(&self) -> Option<u32> {
        self.security_events_per_minute
    }

    /// The web process's whole-request time limit in seconds (`ASKHAVA_REQUEST_TIMEOUT_SECS`, a
    /// whole number from 10 to 300, default 60), longer than a handler's longest chain of bounded
    /// vendor calls (T286, D-1870, FEE-037).
    #[must_use]
    pub fn request_timeout_secs(&self) -> u64 {
        self.request_timeout_secs
    }

    /// The email footer's postal line (`ASKHAVA_POSTAL_ADDRESS`, trimmed): a P.O. box or private
    /// mailbox, checked by `messaging::api::PostalAddress` where a sender is built (SEC-028,
    /// D-3095).
    ///
    /// # Errors
    ///
    /// [`SettingsError::Missing`] when the key is absent or blank.
    pub fn postal_address(&self) -> Result<&str, SettingsError> {
        self.postal_address
            .as_deref()
            .ok_or(SettingsError::Missing(POSTAL_ADDRESS))
    }

    /// The folder of built browser files, one `<crate>/` folder each (`ASKHAVA_STATIC_DIR`,
    /// trimmed), that `server` serves at `/static/<crate>/`; `None` while unset, when `server`
    /// picks the image's folder in test and live and the repository's `app/<crate>/static/` in
    /// dev and CI (PICK-009, A11Y-033, T362, D-2633).
    #[must_use]
    pub fn static_dir(&self) -> Option<&str> {
        self.static_dir.as_deref()
    }

    /// The license lists' and records' base URL (`ASKHAVA_LICENSE_BASE_URL`, checked by
    /// [`check_base_url`]): `file://` fixtures or the public lists (SEC-032, D-2954). Optional
    /// here; the composition that builds the license source requires it in `test` and `live`, and
    /// in `ci` and `dev` its absence makes every sign-up record read unreadable.
    #[must_use]
    pub fn license_base_url(&self) -> Option<&str> {
        self.license_base_url.as_deref()
    }

    /// The Laftly main number (`ASKHAVA_MAIN_NUMBER`, E.164, trimmed), which the safety voice call
    /// reads as its callback number and must not call from (CALL-022, D-3134); `None` while unset.
    #[must_use]
    pub fn main_number(&self) -> Option<&str> {
        self.main_number.as_deref()
    }

    /// Casey's own phone (`ASKHAVA_OWNER_PHONE`, E.164, trimmed, a secret never logged), which the
    /// owner calls dial (OPS-028, OPS-035, D-3890); `None` while unset.
    #[must_use]
    pub fn owner_phone(&self) -> Option<&SecretString> {
        self.owner_phone.as_ref()
    }

    /// The nightly export's recipient public key (`ASKHAVA_EXPORT_RECIPIENT`, an X25519 `age1...`
    /// recipient, trimmed); its private half never sits on the host (PRIV-024, D-4707).
    ///
    /// # Errors
    ///
    /// [`SettingsError::Missing`] when the key is absent.
    pub fn export_recipient(&self) -> Result<&str, SettingsError> {
        self.export
            .0
            .as_deref()
            .ok_or(SettingsError::Missing(EXPORT_RECIPIENT))
    }

    /// The `askhava_backup` role's database URL (`ASKHAVA_BACKUP_DATABASE_URL`), a secret set on
    /// the `worker` service only and read only by the nightly export (OPS-051, SEC-052, D-4293).
    ///
    /// # Errors
    ///
    /// [`SettingsError::Missing`] when the key is absent.
    pub fn backup_database_url(&self) -> Result<&SecretString, SettingsError> {
        self.export
            .1
            .as_ref()
            .ok_or(SettingsError::Missing(BACKUP_DATABASE_URL))
    }

    /// The throwaway PostgreSQL service's admin URL (`ASKHAVA_RESTORE_ADMIN_URL`), a secret set on
    /// the `worker` service only and read only by the monthly test restore; never the live
    /// database (OPS-013, OPS-051, SEC-052, D-4293).
    ///
    /// # Errors
    ///
    /// [`SettingsError::Missing`] when the key is absent.
    pub fn restore_admin_url(&self) -> Result<&SecretString, SettingsError> {
        self.restore_admin_url
            .as_ref()
            .ok_or(SettingsError::Missing(RESTORE_ADMIN_URL))
    }

    /// Stripe's publishable key (`ASKHAVA_STRIPE_PUBLISHABLE_KEY`, trimmed, `pk_test_` or
    /// `pk_live_`), public by design: the Confirm page hands it to the browser's card form. The
    /// marker check refuses a `pk_test_` key in live and a `pk_live_` key outside it (SEC-002).
    /// Optional here; the composition that builds the Confirm page's card form requires it in
    /// test and live (FEE-014, D-3813).
    #[must_use]
    pub fn stripe_publishable_key(&self) -> Option<&str> {
        self.stripe_publishable_key.as_deref()
    }

    /// The Gate C launch instant (`ASKHAVA_LAUNCH_DATE`, RFC 3339) the free first win's 90-day
    /// join window counts from (FEE-012, D-019). Optional here; the composition that builds the
    /// Confirm page requires it in test and live (D-3813).
    #[must_use]
    pub fn launch_date(&self) -> Option<jiff::Timestamp> {
        self.launch_date
    }

    /// FEE-046's sales-tax set-aside in basis points (`ASKHAVA_SALES_TAX_SET_ASIDE_BASIS_POINTS`,
    /// 0 to 10,000, default 860, that is 8.6%), the rate OPS-039's net per paid introduction
    /// subtracts (D-3844).
    #[must_use]
    pub fn sales_tax_set_aside_basis_points(&self) -> u16 {
        self.set_aside_basis_points
    }

    /// WEB-028's threshold (`ASKHAVA_HOME_NUMBERS_MIN_JOBS`, a whole number above 0, default 50):
    /// the home page shows its numbers only once this many requests have had a card sent (D-468).
    #[must_use]
    pub fn home_numbers_min_jobs(&self) -> u32 {
        self.home_numbers_min_jobs
    }

    /// OPS-009's spot-check cadence (`ASKHAVA_SPOT_CHECK_EVERY_DAYS`, a whole number of days above
    /// 0, default [`DEFAULT_SPOT_CHECK_EVERY_DAYS`]) for the owner health page (T476, D-4625).
    #[must_use]
    pub fn spot_check_every_days(&self) -> u32 {
        self.owner_reports.0
    }

    /// OPS-040's golden-set target (`ASKHAVA_GOLDEN_TARGET_PERCENT`, a whole percent from 1 to 100,
    /// default [`DEFAULT_GOLDEN_TARGET_PERCENT`]) for the intake accuracy report (T434, D-3890).
    #[must_use]
    pub fn golden_target_percent(&self) -> u32 {
        self.owner_reports.1
    }

    /// OPS-039's go and stop lines and D-458's ladder as `(key, value)` pairs, every
    /// `ASKHAVA_SCORECARD_*` key once, its `docs/PREBUILD.md` default when unset (T621, D-4448).
    pub fn scorecard_lines(&self) -> impl Iterator<Item = (&'static str, u32)> + '_ {
        SCORECARD_DEFAULTS
            .iter()
            .zip(self.scorecard)
            .map(|((key, _), value)| (*key, value))
    }

    /// WEB-021, WEB-022: the link to the LLC's public record on the Arizona Corporation Commission
    /// site (`ASKHAVA_LLC_RECORD_URL`, trimmed); `None` while unset, and the "Who runs" block
    /// leaves it out (D-3890).
    #[must_use]
    pub fn llc_record_url(&self) -> Option<&str> {
        self.llc_record_url.as_deref()
    }

    /// WEB-021, WEB-022: the Lake Havasu City business license number
    /// (`ASKHAVA_CITY_BUSINESS_LICENSE_NUMBER`, trimmed); `None` while unset, and the "Who runs"
    /// block leaves it out (D-3890).
    #[must_use]
    pub fn city_business_license_number(&self) -> Option<&str> {
        self.city_license.as_deref()
    }

    /// WEB-020, D-3880: the city business-license guide's cost, time and link, in that order
    /// (`ASKHAVA_CITY_LICENSE_COST`, `ASKHAVA_CITY_LICENSE_TIME`, `ASKHAVA_CITY_LICENSE_URL`,
    /// trimmed); each `None` while unset, and the business page leaves that part out.
    #[must_use]
    pub fn city_license_guide(&self) -> [Option<&str>; 3] {
        self.city_guide.each_ref().map(Option::as_deref)
    }

    /// OPS-052, D-4280: the database volume's size in bytes (`ASKHAVA_DB_VOLUME_BYTES`, a whole
    /// number above 0); `None` while unset. Never required here: `server`'s `watch_checks`
    /// registration refuses a missing one in test and live (T525).
    #[must_use]
    pub fn db_volume_bytes(&self) -> Option<u64> {
        self.db_volume_bytes
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

/// The license-lookup base URL, when set, passes [`check_base_url`].
fn license_base_url(
    environment: Environment,
    raw: Option<String>,
) -> Result<Option<String>, SettingsError> {
    if let Some(url) = &raw {
        check_base_url(environment, LICENSE_BASE_URL, url)?;
    }
    Ok(raw)
}

/// `ASKHAVA_PUBLIC_BASE_URL`, if set: an `http` or `https` origin passing [`check_base_url`]
/// (`https` to a non-local host in live), with no path, query or fragment; a trailing slash is
/// dropped. The routes are served at the root and Twilio signs this base plus the route path, so a
/// path here would break every signature check (SEC-016, D-2208).
fn public_base_url(
    environment: Environment,
    raw: Option<String>,
) -> Result<Option<String>, SettingsError> {
    let Some(url) = raw.map(|u| u.trim().trim_end_matches('/').to_owned()) else {
        return Ok(None);
    };
    let host = match url.split_once("://") {
        Some(("https" | "http", rest)) if !rest.contains('/') => url_host(rest),
        _ => String::new(),
    };
    if host.is_empty() || url.contains(['?', '#']) {
        return Err(SettingsError::Invalid(PUBLIC_BASE_URL));
    }
    check_base_url(environment, PUBLIC_BASE_URL, &url)?;
    Ok(Some(url))
}

/// PRIV-024, D-4707: the export recipient and the backup role's URL; the recipient is refused at
/// load unless it is an X25519 `age1` key, so an `ssh-` recipient or a passphrase never reaches
/// the export.
fn export_keys(
    raw: &mut HashMap<&'static str, String>,
) -> Result<(Option<String>, Option<SecretString>), SettingsError> {
    let url = raw.remove(BACKUP_DATABASE_URL).map(SecretString::from);
    let Some(key) = raw.remove(EXPORT_RECIPIENT).map(|k| k.trim().to_owned()) else {
        return Ok((None, url));
    };
    let valid = key.len() == 62
        && key.starts_with("age1")
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    valid
        .then_some((Some(key), url))
        .ok_or(SettingsError::Invalid(EXPORT_RECIPIENT))
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

fn request_timeout_secs(raw: Option<String>) -> Result<u64, SettingsError> {
    let Some(raw) = raw else {
        return Ok(DEFAULT_REQUEST_TIMEOUT_SECS);
    };
    match raw.trim().parse::<u64>() {
        Ok(secs @ 10..=300) => Ok(secs),
        _ => Err(SettingsError::Invalid(REQUEST_TIMEOUT_SECS)),
    }
}

/// OPS-052: the volume size is a whole number of bytes above 0, `None` when unset; a bad value
/// names only the key (T525).
fn volume_bytes(raw: Option<String>) -> Result<Option<u64>, SettingsError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    match raw.trim().parse::<u64>() {
        Ok(n) if n > 0 => Ok(Some(n)),
        _ => Err(SettingsError::Invalid(DB_VOLUME_BYTES)),
    }
}

fn per_minute(raw: Option<String>) -> Result<Option<u32>, SettingsError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    match raw.trim().parse::<u32>() {
        Ok(n) if n > 0 => Ok(Some(n)),
        _ => Err(SettingsError::Invalid(SECURITY_EVENTS_PER_MINUTE)),
    }
}

/// CALL-022: `ASKHAVA_MAIN_NUMBER`, if set, is `+` and 8 to 15 digits, the first not 0 (E.164);
/// a bad value is refused by key only.
fn main_number(raw: Option<String>) -> Result<Option<String>, SettingsError> {
    e164(raw, MAIN_NUMBER)
}

/// The phone setting `key`, if set, as E.164 (`+` and 8 to 15 digits, the first not 0, trimmed);
/// a bad value is refused by key only (CALL-022, OPS-035).
fn e164(raw: Option<String>, key: &'static str) -> Result<Option<String>, SettingsError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let number = raw.trim();
    let digits = number.strip_prefix('+').unwrap_or_default();
    let e164 = (8..=15).contains(&digits.len())
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !digits.starts_with('0');
    if e164 {
        Ok(Some(number.to_owned()))
    } else {
        Err(SettingsError::Invalid(key))
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

/// SEC-002: a publishable key is a `pk_test_` or `pk_live_` value; which one fits the environment
/// is [`check_key_markers`]' rule.
fn publishable_key(raw: Option<String>) -> Result<Option<String>, SettingsError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let key = raw.trim();
    let body = key
        .strip_prefix("pk_test_")
        .or_else(|| key.strip_prefix("pk_live_"))
        .unwrap_or_default();
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(SettingsError::Invalid(STRIPE_PUBLISHABLE_KEY));
    }
    Ok(Some(key.to_owned()))
}

/// D-019: the launch is one RFC 3339 instant with its offset; a bad value names only the key.
fn launch_date(raw: Option<String>) -> Result<Option<jiff::Timestamp>, SettingsError> {
    raw.map(|raw| {
        raw.trim()
            .parse::<jiff::Timestamp>()
            .map_err(|_| SettingsError::Invalid(LAUNCH_DATE))
    })
    .transpose()
}

/// FEE-046, D-3844: the set-aside is a whole number of basis points from 0 to 10,000, 860 when
/// unset; a bad value names only the key.
fn set_aside(raw: Option<String>) -> Result<u16, SettingsError> {
    let Some(raw) = raw else {
        return Ok(DEFAULT_SET_ASIDE_BASIS_POINTS);
    };
    match raw.trim().parse::<u16>() {
        Ok(points @ 0..=10_000) => Ok(points),
        _ => Err(SettingsError::Invalid(SET_ASIDE)),
    }
}

/// OPS-009's spot-check cadence (days above 0) and OPS-040's golden-set target (a whole percent
/// from 1 to 100), each its default when unset (D-4280, D-3890).
fn owner_reports(raw: &mut HashMap<&'static str, String>) -> Result<(u32, u32), SettingsError> {
    let days = above_zero(raw, SPOT_CHECK_EVERY_DAYS, DEFAULT_SPOT_CHECK_EVERY_DAYS)?;
    match above_zero(raw, GOLDEN_TARGET, DEFAULT_GOLDEN_TARGET_PERCENT)? {
        n @ 1..=100 => Ok((days, n)),
        _ => Err(SettingsError::Invalid(GOLDEN_TARGET)),
    }
}

/// WEB-028, D-468 (the home page's threshold) and OPS-009, D-4625 (the spot-check cadence): a
/// whole number above 0, `default` when unset; a bad value names only the key.
fn above_zero(
    raw: &mut HashMap<&'static str, String>,
    key: &'static str,
    default: u32,
) -> Result<u32, SettingsError> {
    let Some(value) = raw.remove(key) else {
        return Ok(default);
    };
    match value.trim().parse::<u32>() {
        Ok(n @ 1..) => Ok(n),
        _ => Err(SettingsError::Invalid(key)),
    }
}

/// OPS-039, D-4448: each scorecard line is a whole number, its default when unset; a bad value
/// names only the key.
fn scorecard(
    raw: &mut HashMap<&'static str, String>,
) -> Result<[u32; SCORECARD_DEFAULTS.len()], SettingsError> {
    let mut lines = [0; SCORECARD_DEFAULTS.len()];
    for (line, (key, default)) in lines.iter_mut().zip(SCORECARD_DEFAULTS) {
        *line = match raw.remove(key) {
            None => default,
            Some(value) => value
                .trim()
                .parse()
                .map_err(|_| SettingsError::Invalid(key))?,
        };
    }
    Ok(lines)
}
