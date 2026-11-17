//! Consent records kept, never exported, and each Arizona week's opt-out rate by audience with its
//! alert (CON-022 to CON-024, TD-CON-09; D-3880, D-3881, D-4082, T107).
//!
//! CON-022, CON-023: the consent records (`intake`'s consent rows, the wording versions),
//! `messaging.optout_event` (append-only for the app role, 0044) and the outbound message log
//! (`messaging.outbox`, whose rows the 30-day body cut keeps, 0128) are never deleted by any sweep,
//! so they outlive [`CONSENT_KEEP_YEARS`] after the last message; nothing outside this crate reads
//! the opt-out tables, and what leaves here is counts only, never a digest, number or ID.
//!
//! CON-024: a week is an Arizona week, Monday 00:00 to the next Monday 00:00 at UTC-7 ([`Week`]).
//! The texted count is the distinct `recipient_id` of outbox rows sent in the week, by
//! `recipient_kind` (D-3881). The opt-out count is the distinct `(index_version, digest)` of
//! `opt_out` events in the week that newly listed the number: no earlier `opt_out` for it unless a
//! `start` came between, so a repeat STOP from a listed number is never counted and STOP, START,
//! STOP counts again (D-4082). An opt-out is a business's when its inbox row has the
//! `businesses.phone_stopped` audit row (read through `ops::api::audited`), else a homeowner's.
//! Known limits (D-4082): a business opt-out by 21610, email or the portal counts as a homeowner's,
//! and a repeat STOP across an index-key rotation counts once more; both can only raise a rate.
//! The weekly [`OPTOUT_RATE_CHECK_KIND`] task, queued for each week's start, queues the next one
//! first, then checks the week just ended: [`HOMEOWNER_ALERT_PERCENT`] of texted homeowners or
//! [`BUSINESS_ALERT_PERCENT`] of texted businesses raises one Normal alert per audience and week
//! through `ops::api::raise_alert_once` (Normal never pushes, OPS-029). T108's dashboard and
//! T524's digest read [`weekly_optout_rates`].

use std::collections::HashSet;
use std::sync::Arc;

use askhava_core::api::{Clock, HandlerError, NewTask, Registry, TaskError, arizona_time, enqueue};
use askhava_ops::api::{Severity, audited, raise_alert_once};
use jiff::civil::{Date, Time};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::OutboxError;
use crate::templates::Audience;

/// Years the consent records, opt-out events and message log are kept after the last message to
/// that number (CON-022).
pub const CONSENT_KEEP_YEARS: i16 = 5;

/// Opt-outs at or above this share of the week's texted homeowners, in percent, alert (CON-024).
pub const HOMEOWNER_ALERT_PERCENT: i64 = 3;

/// Opt-outs at or above this share of the week's texted businesses, in percent, alert (CON-024).
pub const BUSINESS_ALERT_PERCENT: i64 = 5;

/// The weekly `core.task` kind that checks the Arizona week just ended (CON-024).
pub const OPTOUT_RATE_CHECK_KIND: &str = "messaging.optout_rate_check";

/// The ops alert kind for a homeowner week at or above [`HOMEOWNER_ALERT_PERCENT`] (CON-024).
pub const HOMEOWNER_OPTOUT_ALERT: &str = "messaging.optout_rate_homeowners";

/// The ops alert kind for a business week at or above [`BUSINESS_ALERT_PERCENT`] (CON-024).
pub const BUSINESS_OPTOUT_ALERT: &str = "messaging.optout_rate_businesses";

/// The alert subject kind: the week, its ID being its start in Unix seconds (CON-024).
pub const WEEK_SUBJECT: &str = "messaging.optout_week";

const PHONE_STOPPED: &str = "businesses.phone_stopped";
const INBOX_SUBJECT: &str = "messaging.inbox";
const DAY: SignedDuration = SignedDuration::from_hours(24);
const WEEK: SignedDuration = SignedDuration::from_hours(24 * 7);
const DB_FAILED: HandlerError = HandlerError("opt-out rate check database call failed");

/// One Arizona week, Monday 00:00 to the next Monday 00:00 at UTC-7 (CON-024, D-4082).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Week {
    start: Timestamp,
}

impl Week {
    /// CON-024, D-4082: the Arizona week holding `at`.
    #[must_use]
    pub fn containing(at: Timestamp) -> Self {
        let local = arizona_time(at);
        let into_day = local.time().duration_since(Time::midnight());
        let back = DAY * i32::from(local.weekday().to_monday_zero_offset());
        Self {
            start: at - into_day - back,
        }
    }

    /// The week before this one.
    #[must_use]
    pub fn previous(self) -> Self {
        Self {
            start: self.start - WEEK,
        }
    }

    /// The week after this one.
    #[must_use]
    pub fn next(self) -> Self {
        Self {
            start: self.start + WEEK,
        }
    }

    /// Monday 00:00 Arizona time, the first instant in the week.
    #[must_use]
    pub fn start(self) -> Timestamp {
        self.start
    }

    /// The next Monday 00:00 Arizona time, the first instant after the week.
    #[must_use]
    pub fn end(self) -> Timestamp {
        self.start + WEEK
    }

    /// The week's Monday as an Arizona calendar date.
    #[must_use]
    pub fn monday(self) -> Date {
        arizona_time(self.start).date()
    }

    /// The alert subject ID: the week's start in Unix seconds (CON-024).
    #[must_use]
    pub fn id(self) -> Uuid {
        Uuid::from_u128(u128::from(self.start.as_second().unsigned_abs()))
    }
}

/// CON-024: `audience`'s alert threshold in percent of the texted count.
#[must_use]
pub fn alert_percent(audience: Audience) -> i64 {
    match audience {
        Audience::Homeowner => HOMEOWNER_ALERT_PERCENT,
        Audience::Business => BUSINESS_ALERT_PERCENT,
    }
}

/// CON-024: the ops alert kind raised for `audience`'s week.
#[must_use]
pub fn alert_kind(audience: Audience) -> &'static str {
    match audience {
        Audience::Homeowner => HOMEOWNER_OPTOUT_ALERT,
        Audience::Business => BUSINESS_OPTOUT_ALERT,
    }
}

/// One audience's week: counts only, never a number or ID (CON-023, CON-024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeekRate {
    /// The audience.
    pub audience: Audience,
    /// Distinct recipients sent a text in the week.
    pub texted: i64,
    /// Distinct numbers newly listed in the week.
    pub opted_out: i64,
}

impl WeekRate {
    /// CON-024, TD-CON-09: whether the week alerts: opt-outs at or above the audience's share of
    /// the texted count (2.99% of homeowners does not, 3.00% does). A week with nobody texted
    /// never alerts, having no rate.
    #[must_use]
    pub fn alerts(&self) -> bool {
        self.texted > 0 && self.opted_out * 100 >= self.texted * alert_percent(self.audience)
    }
}

/// One Arizona week's opt-out rates for both audiences (CON-024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptOutRates {
    /// The week counted.
    pub week: Week,
    /// The homeowners' rate.
    pub homeowners: WeekRate,
    /// The businesses' rate.
    pub businesses: WeekRate,
}

/// CON-024, D-3881, D-4082: `week`'s texted and newly-opted-out counts by audience.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn weekly_optout_rates(
    db: &mut PgConnection,
    week: Week,
) -> Result<OptOutRates, OutboxError> {
    let texted = sqlx::query!(
        r#"SELECT
             count(DISTINCT recipient_id) FILTER (WHERE recipient_kind = 'homeowner') AS "homeowners!",
             count(DISTINCT recipient_id) FILTER (WHERE recipient_kind = 'business') AS "businesses!"
           FROM messaging.outbox
           WHERE status = 'sent' AND sent_at >= $1 AND sent_at < $2"#,
        week.start().to_sqlx() as _,
        week.end().to_sqlx() as _,
    )
    .fetch_one(&mut *db)
    .await
    .map_err(|_| OutboxError::Database)?;
    let (homeowners, businesses) = new_optouts(db, week).await?;
    Ok(OptOutRates {
        week,
        homeowners: WeekRate {
            audience: Audience::Homeowner,
            texted: texted.homeowners,
            opted_out: homeowners,
        },
        businesses: WeekRate {
            audience: Audience::Business,
            texted: texted.businesses,
            opted_out: businesses,
        },
    })
}

/// D-4082: the distinct numbers `week`'s `opt_out` events newly listed, as (homeowners,
/// businesses); a business's carries the `businesses.phone_stopped` mark on its inbox row.
async fn new_optouts(db: &mut PgConnection, week: Week) -> Result<(i64, i64), OutboxError> {
    let rows = sqlx::query!(
        r#"SELECT e.index_version, e.digest, e.inbox_id
           FROM messaging.optout_event e
           WHERE e.action = 'opt_out' AND e.effective_at >= $1 AND e.effective_at < $2
             AND NOT EXISTS (
               SELECT 1 FROM messaging.optout_event p
               WHERE p.index_version = e.index_version AND p.digest = e.digest
                 AND p.action = 'opt_out'
                 AND (p.effective_at < e.effective_at
                      OR (p.effective_at = e.effective_at AND p.id < e.id))
                 AND NOT EXISTS (
                   SELECT 1 FROM messaging.optout_event s
                   WHERE s.index_version = e.index_version AND s.digest = e.digest
                     AND s.action = 'start'
                     AND s.effective_at > p.effective_at AND s.effective_at < e.effective_at))"#,
        week.start().to_sqlx() as _,
        week.end().to_sqlx() as _,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| OutboxError::Database)?;
    let (mut homeowners, mut businesses) = (HashSet::new(), HashSet::new());
    for row in rows {
        let number = (row.index_version, row.digest);
        if is_business(db, row.inbox_id).await? {
            businesses.insert(number);
        } else {
            homeowners.insert(number);
        }
    }
    Ok((count(&homeowners), count(&businesses)))
}

/// D-4082: an opt-out is a business's when its inbox row has the `phone_stopped` audit row.
async fn is_business(db: &mut PgConnection, inbox: Option<Uuid>) -> Result<bool, OutboxError> {
    let Some(inbox) = inbox else {
        return Ok(false);
    };
    audited(db, PHONE_STOPPED, (INBOX_SUBJECT, inbox))
        .await
        .map_err(|_| OutboxError::Database)
}

fn count<T>(set: &HashSet<T>) -> i64 {
    i64::try_from(set.len()).unwrap_or(i64::MAX)
}

/// CON-024: counts the Arizona week before `now`'s and raises one Normal alert per audience at
/// or above its threshold, unless that week's is already open; returns the counted rates.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn check_optout_rates(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<OptOutRates, OutboxError> {
    let week = Week::containing(now).previous();
    let rates = weekly_optout_rates(db, week).await?;
    for rate in [rates.homeowners, rates.businesses] {
        if rate.alerts() {
            let subject = (WEEK_SUBJECT, week.id());
            raise_alert_once(db, alert_kind(rate.audience), Severity::Normal, subject)
                .await
                .map_err(|_| OutboxError::Database)?;
        }
    }
    Ok(rates)
}

/// CON-024: queues the [`OPTOUT_RATE_CHECK_KIND`] task for the start of the week after `now`'s,
/// one per week by its dedupe key.
///
/// # Errors
///
/// The [`TaskError`] of the insert.
pub async fn schedule_optout_rate_check(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<(), TaskError> {
    let run_at = Week::containing(now).next().start();
    let mut task = NewTask::new(OPTOUT_RATE_CHECK_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!("{OPTOUT_RATE_CHECK_KIND}:{}", run_at.as_second()));
    enqueue(db, &task).await.map(|_| ())
}

/// CON-024, D-3881: registers [`OPTOUT_RATE_CHECK_KIND`] at `clock`'s now: the next week's check
/// first, so a run that fails for good still leaves it, then [`check_optout_rates`].
pub fn register_optout_rate_check(registry: &mut Registry, clock: Arc<dyn Clock>) {
    registry.register(OPTOUT_RATE_CHECK_KIND, move |pool, _task| {
        let now = clock.now();
        async move {
            let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
            schedule_optout_rate_check(&mut conn, now)
                .await
                .map_err(|_| DB_FAILED)?;
            check_optout_rates(&mut conn, now)
                .await
                .map(|_| ())
                .map_err(|_| DB_FAILED)
        }
    });
}
