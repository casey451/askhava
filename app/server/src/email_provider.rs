//! The one email provider, built from settings (SEC-001, SEC-002, SEC-025, SEC-032, SEC-036,
//! OPS-038; T469, D-3750, D-3890).
//!
//! [`email_provider`] builds an [`EmailProvider`]: the Resend client from
//! `adapters::api::ResendConfig`, the email key (`ASKHAVA_ID_SECRET`, as `notice_senders` uses
//! it), the footer postal line (`settings.postal_address()`) and one `messaging::api::DailyLimit`
//! from `ASKHAVA_EMAIL_DAILY_LIMIT` (default 100, Resend's free-tier daily quota, SEC-025) and
//! `ASKHAVA_EMAIL_BULK_HOLD_PERCENT` (default 90, OPS-038), plus `ASKHAVA_EMAIL_MONTHLY_LIMIT`
//! (default 3,000, the free-tier monthly quota) for the health page. Every mailer built from it
//! shares that one limit, so every email counts against the one daily quota (`send_email` reads
//! the day's sent count). In dev and CI a missing API key, sending domain, ID secret or postal
//! line answers `None` (nothing is sent); in test and live it is an error naming the key. A wrong
//! value (a `live` mode outside live, `stand_in` or a stand-in URL in live, a bad limit) is an
//! error everywhere. No error carries a value (SEC-001).

use std::io;

use askhava_adapters::api::{ResendConfig, ResendEmail};
use askhava_core::api::{Environment, Settings, SettingsError};
use askhava_messaging::api::{DailyLimit, MailboxKind, PostalAddress, SignInLinkSender};
use secrecy::SecretString;

const ID_SECRET: &str = "ASKHAVA_ID_SECRET";
const DAILY: &str = "ASKHAVA_EMAIL_DAILY_LIMIT";
const MONTHLY: &str = "ASKHAVA_EMAIL_MONTHLY_LIMIT";
const HOLD: &str = "ASKHAVA_EMAIL_BULK_HOLD_PERCENT";
/// Resend's free-tier daily quota (SEC-025).
const DEFAULT_DAILY: u32 = 100;
/// Resend's free-tier monthly quota (SEC-025, D-3890).
const DEFAULT_MONTHLY: u32 = 3_000;
/// The share of the daily limit above which bulk email waits (OPS-038).
const DEFAULT_HOLD: u8 = 90;

/// The Resend client, the email key, the footer's postal line and the shared limits (SEC-025).
pub struct EmailProvider {
    client: ResendEmail,
    secret: SecretString,
    postal: (String, MailboxKind),
    daily: DailyLimit,
    monthly: u32,
}

impl EmailProvider {
    /// The one Resend client (SEC-032).
    #[must_use]
    pub fn client(&self) -> &ResendEmail {
        &self.client
    }

    /// The email key, `ASKHAVA_ID_SECRET` (SEC-018); never logged.
    #[must_use]
    pub fn secret(&self) -> &SecretString {
        &self.secret
    }

    /// The footer's postal line and its mailbox kind (SEC-028).
    #[must_use]
    pub fn postal(&self) -> (&str, MailboxKind) {
        (&self.postal.0, self.postal.1)
    }

    /// The one daily limit every mailer shares (SEC-025, OPS-038).
    #[must_use]
    pub fn daily_limit(&self) -> DailyLimit {
        self.daily
    }

    /// The provider's monthly quota, shown on the health page (SEC-025, D-3890).
    #[must_use]
    pub fn monthly_limit(&self) -> u32 {
        self.monthly
    }

    /// The business sign-in link mailer over this provider (SEC-004, SEC-025, SEC-026).
    ///
    /// # Errors
    ///
    /// A postal line `messaging` refuses, naming the key.
    pub fn sign_in_sender(&self) -> io::Result<SignInLinkSender<ResendEmail>> {
        let (postal, kind) = self.postal();
        SignInLinkSender::new(
            self.secret.clone(),
            self.client.clone(),
            (postal, kind),
            self.daily,
        )
        .map_err(|_| invalid("ASKHAVA_POSTAL_ADDRESS"))
    }
}

/// SEC-002, SEC-025, SEC-032, OPS-038: the provider from `settings` and the same `pairs`; `None`
/// in dev and CI when a key or the postal line is missing.
///
/// # Errors
///
/// A missing key or postal line in test and live; a wrong mode, base URL or limit anywhere.
/// The error names the key, never a value.
pub fn email_provider<I>(settings: &Settings, pairs: I) -> io::Result<Option<EmailProvider>>
where
    I: IntoIterator<Item = (String, String)>,
{
    let pairs: Vec<(String, String)> = pairs.into_iter().collect();
    let config = ResendConfig::from_settings(settings, pairs.clone());
    let secret = value(&pairs, ID_SECRET).ok_or(SettingsError::Missing(ID_SECRET));
    let postal = settings.postal_address();
    let (config, secret, postal) = match (config, secret, postal) {
        (Ok(config), Ok(secret), Ok(postal)) => (config, secret, postal),
        (Err(e), ..) | (_, Err(e), _) | (.., Err(e)) => return missing_or_error(settings, e),
    };
    let client = config
        .client()
        .map_err(|_| invalid("ASKHAVA_RESEND_BASE_URL"))?;
    let kind = match PostalAddress::new(postal, MailboxKind::PoBox) {
        Ok(_) => MailboxKind::PoBox,
        Err(_) => MailboxKind::PrivateMailbox,
    };
    let daily = number(&pairs, DAILY, DEFAULT_DAILY)?;
    let hold = number(&pairs, HOLD, DEFAULT_HOLD)?;
    let daily = DailyLimit::new(daily, hold).map_err(|_| invalid(HOLD))?;
    let monthly = number(&pairs, MONTHLY, DEFAULT_MONTHLY)?;
    Ok(Some(EmailProvider {
        client,
        secret: SecretString::from(secret),
        postal: (postal.to_owned(), kind),
        daily,
        monthly,
    }))
}

/// A missing key is `None` in dev and CI and an error in test and live; any other error stops.
fn missing_or_error(
    settings: &Settings,
    error: SettingsError,
) -> io::Result<Option<EmailProvider>> {
    let dev_or_ci = matches!(settings.environment(), Environment::Dev | Environment::Ci);
    match error {
        SettingsError::Missing(_) if dev_or_ci => Ok(None),
        error => Err(io::Error::other(error)),
    }
}

/// The trimmed, non-empty value of `key` in `pairs`.
fn value(pairs: &[(String, String)], key: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(k, v)| k == key && !v.trim().is_empty())
        .map(|(_, v)| v.trim().to_owned())
}

/// `key` as a whole number above 0, or `default` (logged) when unset.
fn number<N>(pairs: &[(String, String)], key: &'static str, default: N) -> io::Result<N>
where
    N: std::str::FromStr + Default + PartialEq + std::fmt::Display,
{
    let Some(raw) = value(pairs, key) else {
        tracing::info!(setting = key, default = %default, "email limit setting unset; default used");
        return Ok(default);
    };
    match raw.parse::<N>() {
        Ok(n) if n != N::default() => Ok(n),
        _ => Err(invalid(key)),
    }
}

/// The error for `key`'s bad value, naming the key only (SEC-001).
fn invalid(key: &'static str) -> io::Error {
    io::Error::other(SettingsError::Invalid(key))
}
