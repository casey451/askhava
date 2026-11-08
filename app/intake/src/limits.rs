//! Rate limits on new requests, and the shared limit check every module's limits run through
//! (SEC-014, SEC-015, SEC-040, D-491, T089).
//!
//! A [`Limit`] is a code-name scope, a ceiling and a [`Window`]; [`admit`] counts one request in
//! each `(Limit, Key)` pair, narrowest first, through `core`'s one atomic upsert
//! (`askhava_core::api::rate_limit_hit`), so the count is exact across web processes (SEC-040),
//! and stops at the first pair over its ceiling: a wider bucket is never spent by a refused
//! request. The first refusal of a scope in a window raises one High `ops` alert named by the
//! scope (SEC-015); later refusals in that window raise none. A bucket is a scope plus an IP or a
//! record ID, never a phone or email (PRIV-002). Intake's own limits are [`admit_new_request`]'s:
//! 5 new requests per phone per hour (SEC-014), per IP, and all numbers together per hour, so a
//! burst of 100 fake requests is blocked and alerted (SEC-015). `portals` keeps the sign-in,
//! code, sign-up and quote limits and runs them through [`admit`] (D-491).

use std::net::IpAddr;

use askhava_core::api::{RateDecision, ip_bucket, rate_limit_hit};
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use jiff::{SignedDuration, Timestamp};
use sqlx::PgPool;
use sqlx::types::Uuid;
use thiserror::Error;

/// New requests one phone (her vault contact) may open per hour (SEC-014).
pub const REQUESTS_PER_PHONE: Limit = Limit::hourly("intake.requests_per_phone", 5);
/// New requests one client IP may open per hour (web requests, SEC-014, SEC-040).
pub const REQUESTS_PER_IP: Limit = Limit::hourly("intake.requests_per_ip", 10);
/// New requests all numbers together may open per hour; past it a burst is refused and
/// alerted (SEC-015, D-1950).
pub const REQUESTS_ALL: Limit = Limit::hourly("intake.requests_all", 40);

/// Why a limit could not be checked; no variant carries a value (SEC-001).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum LimitError {
    /// The counter upsert failed.
    #[error("the rate-limit counter could not be reached")]
    Counter,
    /// The over-limit alert could not be written.
    #[error("the rate-limit alert could not be raised")]
    Alert,
}

/// How long a limit's window is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    /// A clock hour.
    Hour,
    /// An Arizona calendar day (UTC-7 all year).
    ArizonaDay,
}

/// One rate limit: its code-name scope (also the alert kind), ceiling and window (SEC-014).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    /// The bucket prefix and the `ops` alert kind, such as `intake.requests_per_phone`.
    pub scope: &'static str,
    /// Requests allowed per window.
    pub max: i32,
    /// The window.
    pub window: Window,
}

impl Limit {
    /// A limit of `max` per clock hour.
    #[must_use]
    pub const fn hourly(scope: &'static str, max: i32) -> Self {
        Self {
            scope,
            max,
            window: Window::Hour,
        }
    }

    /// A limit of `max` per Arizona day.
    #[must_use]
    pub const fn daily(scope: &'static str, max: i32) -> Self {
        Self {
            scope,
            max,
            window: Window::ArizonaDay,
        }
    }
}

/// What one limit counts by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// The client IP as `core::api::ClientIpPolicy::client_ip` reads it (SEC-040).
    Ip(IpAddr),
    /// A record: a vault contact, a business or the owner (never a phone or email, PRIV-002).
    Record(Uuid),
    /// Everyone together.
    All,
}

/// Whether a request may go ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// Every limit had room; each counted this request.
    Admitted,
    /// This limit is full for its window.
    Refused(Limit),
}

fn bucket(limit: Limit, key: Key) -> String {
    match key {
        Key::Ip(ip) => ip_bucket(limit.scope, ip),
        Key::Record(id) => format!("{}:id:{id}", limit.scope),
        Key::All => format!("{}:all", limit.scope),
    }
}

/// The counter call for `window`: an Arizona day is the UTC day of `now` moved back 7 hours.
async fn count(
    pool: &PgPool,
    bucket: &str,
    (now, window): (Timestamp, Window),
    max: i32,
) -> Result<RateDecision, LimitError> {
    let (at, size) = match window {
        Window::Hour => (now, SignedDuration::from_hours(1)),
        Window::ArizonaDay => (
            now - SignedDuration::from_hours(7),
            SignedDuration::from_hours(24),
        ),
    };
    rate_limit_hit(pool, bucket, at, size, max)
        .await
        .map_err(|_| LimitError::Counter)
}

/// Counts one request against each `(Limit, Key)` in order and stops at the first that is full;
/// that scope's first refusal in its window raises one High alert (SEC-014, SEC-015, SEC-040).
///
/// # Errors
///
/// [`LimitError::Counter`] or [`LimitError::Alert`]; the caller refuses the request on either.
pub async fn admit(
    pool: &PgPool,
    checks: &[(Limit, Key)],
    now: Timestamp,
) -> Result<Admission, LimitError> {
    for &(limit, key) in checks {
        let decision = count(pool, &bucket(limit, key), (now, limit.window), limit.max).await?;
        if decision == RateDecision::Limited {
            alert_once(pool, limit, now).await?;
            return Ok(Admission::Refused(limit));
        }
    }
    Ok(Admission::Admitted)
}

/// One High alert per scope and window: the `<scope>:alerted` bucket admits only its first hit.
async fn alert_once(pool: &PgPool, limit: Limit, now: Timestamp) -> Result<(), LimitError> {
    let flag = format!("{}:alerted", limit.scope);
    if count(pool, &flag, (now, limit.window), 1).await? == RateDecision::Limited {
        return Ok(());
    }
    let alert = NewAlert {
        kind: limit.scope,
        severity: Severity::High,
        subject: None,
    };
    let mut conn = pool.acquire().await.map_err(|_| LimitError::Alert)?;
    raise_alert(&mut conn, &alert)
        .await
        .map(|_| ())
        .map_err(|_| LimitError::Alert)
}

/// SEC-014, SEC-015, SEC-040: admits a new request from `contact` (her vault contact), from
/// `ip` when it came by the web: 5 per phone per hour, [`REQUESTS_PER_IP`], then
/// [`REQUESTS_ALL`]. A refused request opens nothing and gets no reply.
///
/// # Errors
///
/// As [`admit`].
pub async fn admit_new_request(
    pool: &PgPool,
    (contact, ip): (Uuid, Option<IpAddr>),
    now: Timestamp,
) -> Result<Admission, LimitError> {
    let mut checks = vec![(REQUESTS_PER_PHONE, Key::Record(contact))];
    checks.extend(ip.map(|ip| (REQUESTS_PER_IP, Key::Ip(ip))));
    checks.push((REQUESTS_ALL, Key::All));
    admit(pool, &checks, now).await
}
