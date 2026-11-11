//! Rate limits on new requests, and the shared limit check every module's limits run through
//! (SEC-014, SEC-015, SEC-040, D-491, T089).
//!
//! A [`Limit`] is a code-name scope, a ceiling and a [`Window`]; [`admit`] counts one request in
//! each `(Limit, Key)` pair, narrowest first, through `core`'s one atomic upsert
//! (`askhava_core::api::rate_limit_hit`), so the count is exact across web processes (SEC-040),
//! and stops at the first pair over its ceiling: a wider bucket is never spent by a refused
//! request. The narrower buckets a refused request already counted stay counted: `core`'s counter
//! only adds, so a refusal by a wider limit costs the narrower keys one hit each (D-1953). The
//! first refusal of a scope in a window raises one `ops` alert named by the scope, High unless
//! the limit is [`Limit::quiet`] (SEC-015); later refusals in that window raise none. The
//! refusal that takes the marker tries the alert [`ALERT_TRIES`] times (D-1953). A bucket
//! is a scope plus an IP or a record ID, never a phone or email (PRIV-002). Intake's own limits
//! are [`admit_new_request`]'s: 5 new requests per phone per hour (SEC-014, a Normal alert), per
//! IP, and all numbers together per hour, so a burst of 100 fake requests is blocked and alerted
//! High (SEC-015). It counts on a connection the caller hands it, so the inbound text route counts
//! inside the delivery's transaction (D-2206, D-3083). `portals` keeps the sign-in, code, sign-up and quote limits and runs them
//! through [`admit`] (D-491).

use std::net::IpAddr;

use askhava_core::api::{RateDecision, ip_bucket, rate_limit_hit, rate_limit_hit_on};
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use jiff::{SignedDuration, Timestamp};
use sqlx::types::Uuid;
use sqlx::{Acquire, Connection, PgConnection, PgPool, Postgres};
use thiserror::Error;

/// New requests one phone (her vault contact) may open per hour (SEC-014); its refusal is one
/// homeowner, not a burst, so its alert is Normal (the weekly digest, D-1953).
pub const REQUESTS_PER_PHONE: Limit = Limit::hourly("intake.requests_per_phone", 5).quiet();
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
    /// The severity of the alert its first refusal in a window raises (SEC-015).
    pub alert: Severity,
}

impl Limit {
    /// A limit of `max` per clock hour, alerting High.
    #[must_use]
    pub const fn hourly(scope: &'static str, max: i32) -> Self {
        Self {
            scope,
            max,
            window: Window::Hour,
            alert: Severity::High,
        }
    }

    /// A limit of `max` per Arizona day, alerting High.
    #[must_use]
    pub const fn daily(scope: &'static str, max: i32) -> Self {
        Self {
            scope,
            max,
            window: Window::ArizonaDay,
            alert: Severity::High,
        }
    }

    /// The same limit with a Normal alert: one person's limit, not a burst (D-1953).
    #[must_use]
    pub const fn quiet(self) -> Self {
        Self {
            alert: Severity::Normal,
            ..self
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

/// The counter's `(at, size)` for `window`: an Arizona day is the UTC day of `now` moved back 7
/// hours.
fn span(now: Timestamp, window: Window) -> (Timestamp, SignedDuration) {
    match window {
        Window::Hour => (now, SignedDuration::from_hours(1)),
        Window::ArizonaDay => (
            now - SignedDuration::from_hours(7),
            SignedDuration::from_hours(24),
        ),
    }
}

/// The counter call for `window` on the pool.
async fn count(
    pool: &PgPool,
    bucket: &str,
    (now, window): (Timestamp, Window),
    max: i32,
) -> Result<RateDecision, LimitError> {
    let (at, size) = span(now, window);
    rate_limit_hit(pool, bucket, at, size, max)
        .await
        .map_err(|_| LimitError::Counter)
}

/// The counter call for `window` on the caller's connection (D-2206).
async fn count_on(
    conn: &mut PgConnection,
    bucket: &str,
    (now, window): (Timestamp, Window),
    max: i32,
) -> Result<RateDecision, LimitError> {
    let (at, size) = span(now, window);
    rate_limit_hit_on(conn, bucket, at, size, max)
        .await
        .map_err(|_| LimitError::Counter)
}

/// Counts one request against each `(Limit, Key)` in order and stops at the first that is full;
/// that scope's first refusal in its window raises one alert of the limit's severity (SEC-014,
/// SEC-015, SEC-040). The narrower keys already counted keep their hit (D-1953).
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

/// One alert per scope and window (SEC-015): the `<scope>:alerted` bucket admits only its first
/// hit, and that refusal writes the alert, trying [`ALERT_TRIES`] times, since
/// `core`'s counter cannot take the marker back. Holding the alert in a transaction until the
/// marker answers would need two connections per refusal and starves the pool in a burst
/// (D-1953). Every try failing is [`LimitError::Alert`], which the caller logs.
async fn alert_once(pool: &PgPool, limit: Limit, now: Timestamp) -> Result<(), LimitError> {
    let flag = format!("{}:alerted", limit.scope);
    if count(pool, &flag, (now, limit.window), 1).await? == RateDecision::Limited {
        return Ok(());
    }
    let alert = NewAlert {
        kind: limit.scope,
        severity: limit.alert,
        subject: None,
    };
    let mut tried = write_alert(pool, &alert).await;
    for _ in 1..ALERT_TRIES {
        if tried.is_ok() {
            break;
        }
        tried = write_alert(pool, &alert).await;
    }
    tried
}

/// How many times [`alert_once`] tries to write the alert, each on a fresh pool connection
/// (D-1953).
pub const ALERT_TRIES: usize = 3;

async fn write_alert(pool: &PgPool, alert: &NewAlert) -> Result<(), LimitError> {
    let mut conn = pool.acquire().await.map_err(|_| LimitError::Alert)?;
    raise_alert(&mut conn, alert)
        .await
        .map(|_| ())
        .map_err(|_| LimitError::Alert)
}

/// SEC-014, SEC-015, SEC-040, D-2206, D-3083: admits a new request from `contact` (her vault
/// contact), from `ip` when it came by the web: 5 per phone per hour, [`REQUESTS_PER_IP`], then
/// [`REQUESTS_ALL`] last. Every count, the alert marker and the alert run on the one connection
/// acquired from `db`, inside a transaction (a savepoint when `db` is already in one), so the
/// caller's delivery transaction holds them: a rolled-back delivery counts nothing and raises
/// nothing, and a resent one counts once. The `REQUESTS_ALL` row stays locked until the caller
/// commits, so the caller counts as its last step and waits on no network call before the commit.
/// A refused request opens nothing and gets no reply.
///
/// # Errors
///
/// [`LimitError::Counter`] when the connection or a count fails, [`LimitError::Alert`] when the
/// alert cannot be written; on either nothing of this call is kept.
pub fn admit_new_request<'a, A>(
    db: A,
    (contact, ip): (Uuid, Option<IpAddr>),
    now: Timestamp,
) -> impl Future<Output = Result<Admission, LimitError>> + Send + 'a
where
    A: Acquire<'a, Database = Postgres> + Send + 'a,
{
    let mut checks = vec![(REQUESTS_PER_PHONE, Key::Record(contact))];
    checks.extend(ip.map(|ip| (REQUESTS_PER_IP, Key::Ip(ip))));
    checks.push((REQUESTS_ALL, Key::All));
    async move {
        let mut conn = db.acquire().await.map_err(|_| LimitError::Counter)?;
        let mut tx = Connection::begin(&mut *conn)
            .await
            .map_err(|_| LimitError::Counter)?;
        let admission = admit_on(&mut tx, &checks, now).await?;
        tx.commit().await.map_err(|_| LimitError::Counter)?;
        Ok(admission)
    }
}

/// [`admit`] on one connection: the counts, the `<scope>:alerted` marker and the one alert all
/// commit or roll back together, so the alert needs no retry and a lost alert leaves no marker
/// (SEC-014, SEC-015, D-2206).
async fn admit_on(
    conn: &mut PgConnection,
    checks: &[(Limit, Key)],
    now: Timestamp,
) -> Result<Admission, LimitError> {
    for &(limit, key) in checks {
        let decision = count_on(conn, &bucket(limit, key), (now, limit.window), limit.max).await?;
        if decision == RateDecision::Limited {
            let flag = format!("{}:alerted", limit.scope);
            if count_on(conn, &flag, (now, limit.window), 1).await? != RateDecision::Limited {
                let alert = NewAlert {
                    kind: limit.scope,
                    severity: limit.alert,
                    subject: None,
                };
                raise_alert(conn, &alert)
                    .await
                    .map_err(|_| LimitError::Alert)?;
            }
            return Ok(Admission::Refused(limit));
        }
    }
    Ok(Admission::Admitted)
}
