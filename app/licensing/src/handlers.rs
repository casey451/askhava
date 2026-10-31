//! The one throttled live lookup and what happens when a lookup or a list fails (LIC-007, LIC-008,
//! LIC-009, LIC-039).
//!
//! [`lookup_at_pick`] makes exactly one live lookup of a business's ROC license, never retried. The
//! answer is read by the ROC list reader (D-282): a page that does not read (site down, CAPTCHA,
//! changed layout, a row for another number, no good row) is [`LookupOutcome::Unknown`] and records
//! nothing, so a misread never changes a verified status (LIC-008); a read answer is recorded as a
//! daily check dated the Arizona day. [`lookup_while_list_fails`] runs that lookup once for each
//! active business, one at a time, while a list fails to load (LIC-039, D-230). The clocks are pure:
//! [`unknown_standing`] (kept, warned at 24 hours, paused at 72), [`end_date_pauses`] (paused from
//! 00:00 Arizona time on a license-end or bond-cancellation date) and [`list_failure_alert`] (a
//! listed alert at 12 hours, the OPS-032 push at 24). The caller (`businesses` and the worker) holds
//! when the failure started and fills [`FailureLimits`] from its settings.

use askhava_adapters::api::LicenseSource;
use askhava_ops::api::{NewAlert, Severity, raise_alert};
use jiff::civil::Date;
use jiff::tz::{TimeZone, offset};
use jiff::{SignedDuration, Timestamp};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::imports::{ImportError, normalise_license_number, parse_roc_list};
use crate::service::record_lookup;

/// The alert kind for a license list that keeps failing to load (LIC-039).
pub const LIST_FAILING_ALERT: &str = "licensing.list_failing";

/// The LIC-008 and LIC-039 clocks; each is a setting the caller passes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailureLimits {
    /// An unknown status is warned after this long (24 hours, LIC-008).
    pub warn_after: SignedDuration,
    /// An unknown status pauses the business after this long (72 hours, LIC-008).
    pub pause_after: SignedDuration,
    /// A failing list raises a listed alert after this long (12 hours, LIC-039).
    pub alert_after: SignedDuration,
    /// A failing list goes in the OPS-032 push after this long (24 hours, LIC-039, D-282).
    pub push_after: SignedDuration,
}

impl Default for FailureLimits {
    fn default() -> Self {
        Self {
            warn_after: SignedDuration::from_hours(24),
            pause_after: SignedDuration::from_hours(72),
            alert_after: SignedDuration::from_hours(12),
            push_after: SignedDuration::from_hours(24),
        }
    }
}

/// What one live lookup found (LIC-007, LIC-008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupOutcome {
    /// The record shows the license Active; recorded as an `active` check.
    Active,
    /// The record shows the license, but not Active; recorded as an `inactive` check.
    Inactive,
    /// The lookup failed or the page did not read: nothing is recorded (LIC-008).
    Unknown,
}

impl LookupOutcome {
    /// Whether the business may Confirm on this answer: never while its status is unknown (LIC-008).
    #[must_use]
    pub const fn allows_confirm(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// Where a business stands while its license status is unknown (LIC-008, D-199).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownStanding {
    /// It stays as it was.
    Kept,
    /// It stays as it was and is warned.
    Warned,
    /// It is paused; the pause clears without review when a list shows it Active.
    Paused,
}

/// LIC-008: kept before `warn_after`, warned from it, paused from `pause_after`, counted from
/// `unknown_since`, the first failed lookup.
#[must_use]
pub fn unknown_standing(
    unknown_since: Timestamp,
    now: Timestamp,
    limits: FailureLimits,
) -> UnknownStanding {
    let unknown_for = now.duration_since(unknown_since);
    match unknown_for {
        d if d >= limits.pause_after => UnknownStanding::Paused,
        d if d >= limits.warn_after => UnknownStanding::Warned,
        _ => UnknownStanding::Kept,
    }
}

/// The Arizona calendar day of `at` (UTC-7 all year, no daylight time).
#[must_use]
pub fn arizona_day(at: Timestamp) -> Date {
    at.to_zoned(TimeZone::fixed(offset(-7))).date()
}

/// LIC-009: a license-end or bond-cancellation date on the state record pauses the business from
/// 00:00 Arizona time on that date.
#[must_use]
pub fn end_date_pauses(
    license_ends_on: Option<Date>,
    bond_cancelled_on: Option<Date>,
    now: Timestamp,
) -> bool {
    let today = arizona_day(now);
    [license_ends_on, bond_cancelled_on]
        .into_iter()
        .flatten()
        .any(|day| day <= today)
}

/// LIC-039, D-282: the alert a list failing since `failing_since` raises on a run at `now` whose
/// previous run was at `last_run`: `Normal` (listed for Casey's next window) when the alert limit
/// falls in `(last_run, now]`, `High` (the OPS-032 push) when the push limit does, else none, so
/// each is raised once.
#[must_use]
pub fn list_failure_alert(
    failing_since: Timestamp,
    last_run: Timestamp,
    now: Timestamp,
    limits: FailureLimits,
) -> Option<Severity> {
    let crossed = |limit: SignedDuration| {
        let at = failing_since
            .saturating_add(limit)
            .unwrap_or(Timestamp::MAX);
        last_run < at && at <= now
    };
    match (crossed(limits.push_after), crossed(limits.alert_after)) {
        (true, _) => Some(Severity::High),
        (false, true) => Some(Severity::Normal),
        (false, false) => None,
    }
}

/// Raises the [`list_failure_alert`] for this run, if any, and returns its severity (LIC-039).
///
/// # Errors
///
/// [`ImportError::Database`] when the alert cannot be written.
pub async fn alert_list_failure(
    conn: &mut PgConnection,
    (failing_since, last_run, now): (Timestamp, Timestamp, Timestamp),
    limits: FailureLimits,
) -> Result<Option<Severity>, ImportError> {
    let Some(severity) = list_failure_alert(failing_since, last_run, now, limits) else {
        return Ok(None);
    };
    let alert = NewAlert {
        kind: LIST_FAILING_ALERT,
        severity,
        subject: None,
    };
    raise_alert(conn, &alert)
        .await
        .map_err(|_| ImportError::Database)?;
    Ok(Some(severity))
}

/// Reads one live lookup answer for `number` with the ROC list reader (D-282); anything that does
/// not read as rows for that number alone is `Unknown` (LIC-008).
fn read_answer(number: &str, answer: &[u8]) -> LookupOutcome {
    let Ok(Ok(list)) = std::str::from_utf8(answer).map(parse_roc_list) else {
        return LookupOutcome::Unknown;
    };
    if list.skipped > 0 || list.rows.iter().any(|r| r.license_number != number) {
        return LookupOutcome::Unknown;
    }
    match list.rows.iter().any(|r| r.status.is_active()) {
        true => LookupOutcome::Active,
        false => LookupOutcome::Inactive,
    }
}

/// Makes exactly one live lookup of `license_number` for `business_id` at a pick, never retried,
/// and records a read answer as a check dated the Arizona day of `now` (LIC-007, LIC-008).
///
/// # Errors
///
/// [`ImportError::Database`] when a read answer cannot be recorded; a failed lookup is not an
/// error but [`LookupOutcome::Unknown`].
pub async fn lookup_at_pick(
    conn: &mut PgConnection,
    source: &impl LicenseSource,
    (business_id, license_number): (Uuid, &str),
    now: Timestamp,
) -> Result<LookupOutcome, ImportError> {
    let number = normalise_license_number(license_number);
    let outcome = match source.lookup_license(&number).await {
        Ok(answer) => read_answer(&number, &answer),
        Err(_) => LookupOutcome::Unknown,
    };
    let active = match outcome {
        LookupOutcome::Unknown => return Ok(outcome),
        other => other == LookupOutcome::Active,
    };
    record_lookup(conn, business_id, &number, active, arizona_day(now)).await?;
    Ok(outcome)
}

/// While a list fails to load, runs [`lookup_at_pick`] once for each active business on it, one at
/// a time, and returns the outcomes in order (LIC-039, D-230).
///
/// # Errors
///
/// [`ImportError::Database`] when an answer cannot be recorded.
pub async fn lookup_while_list_fails(
    conn: &mut PgConnection,
    source: &impl LicenseSource,
    businesses: &[(Uuid, &str)],
    now: Timestamp,
) -> Result<Vec<LookupOutcome>, ImportError> {
    let mut outcomes = Vec::with_capacity(businesses.len());
    for &business in businesses {
        outcomes.push(lookup_at_pick(conn, source, business, now).await?);
    }
    Ok(outcomes)
}
