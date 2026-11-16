//! OPS-046's first-six-weeks email figures and the daily deferral check (D-490, D-3883, T531).
//!
//! The window starts on the Arizona day (UTC-7, no daylight time) of the first email the provider
//! took (`messaging.email_outbox.sent_at`, the sending subdomain's first send) and lasts
//! [`WINDOW_DAYS`] days. For each day so far, [`email_days`] gives totals only: sends (outbox rows
//! sent that day), deferrals, bounces and complaints (distinct provider IDs with an
//! `email.delivery_delayed`, `email.bounced` or `email.complained` event timed that day, so a
//! provider retrying one email counts once). No address, recipient or template leaves here.
//! The daily [`DEFERRAL_CHECK_KIND`] task queues its next run first, then checks the Arizona day
//! before its own: deferrals above [`DEFERRAL_LIMIT_PERCENT`] of that day's sends raise one High
//! [`DEFERRAL_ALERT`] through `ops::api::raise_alert_once_a_day`, so a second run that day raises
//! nothing. A day outside the window, or before any send, raises nothing.

use std::sync::Arc;

use askhava_core::api::{Clock, HandlerError, NewTask, Registry, TaskError, enqueue};
use askhava_ops::api::{Severity, raise_alert_once_a_day};
use jiff::civil::Date;
use jiff::tz::{TimeZone, offset};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use serde_json::json;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::OutboxError;

/// How many days after the first send OPS-046's figures and alert cover (6 weeks).
pub const WINDOW_DAYS: i32 = 42;

/// Deferrals above this share of a day's sends, in percent, raise [`DEFERRAL_ALERT`] (OPS-046).
pub const DEFERRAL_LIMIT_PERCENT: i64 = 5;

/// The daily `core.task` kind that checks the day before's deferrals (OPS-046).
pub const DEFERRAL_CHECK_KIND: &str = "messaging.deferral_check";

/// The ops alert kind raised, at most once an Arizona day, for too many deferrals (OPS-046).
pub const DEFERRAL_ALERT: &str = "messaging.email_deferrals_high";

const DAY: SignedDuration = SignedDuration::from_hours(24);
const DB_FAILED: HandlerError = HandlerError("email deferral check database call failed");

/// One Arizona day's email totals in OPS-046's first six weeks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmailDay {
    /// The Arizona calendar day.
    pub day: Date,
    /// Emails the provider took that day.
    pub sends: i64,
    /// Emails with a delivery-delayed event that day.
    pub deferrals: i64,
    /// Emails with a bounce event that day.
    pub bounces: i64,
    /// Emails with a spam-complaint event that day.
    pub complaints: i64,
}

impl EmailDay {
    /// OPS-046: whether deferrals exceed [`DEFERRAL_LIMIT_PERCENT`] of the day's sends (exactly
    /// 5% does not).
    #[must_use]
    pub fn deferrals_too_high(&self) -> bool {
        self.deferrals * 100 > self.sends * DEFERRAL_LIMIT_PERCENT
    }
}

fn arizona() -> TimeZone {
    TimeZone::fixed(offset(-7))
}

/// OPS-046, D-490: the totals of every Arizona day from the first send's day up to `now`'s day,
/// at most [`WINDOW_DAYS`] days, oldest first, a quiet day with zeros; empty before any send.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn email_days(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<Vec<EmailDay>, OutboxError> {
    let rows = sqlx::query!(
        r#"WITH first AS (
             SELECT ((min(sent_at) AT TIME ZONE 'UTC') - interval '7 hours')::date AS day
             FROM messaging.email_outbox WHERE status = 'sent'),
           days AS (
             SELECT ((f.day + n)::timestamp + interval '7 hours') AT TIME ZONE 'UTC' AS start
             FROM first f, generate_series(0, LEAST($2::int - 1,
               ((($1::timestamptz AT TIME ZONE 'UTC') - interval '7 hours')::date - f.day))) AS n
             WHERE f.day IS NOT NULL)
           SELECT d.start AS "start!",
             (SELECT count(*) FROM messaging.email_outbox o WHERE o.status = 'sent'
                AND o.sent_at >= d.start AND o.sent_at < d.start + interval '1 day') AS "sends!",
             (SELECT count(DISTINCT e.provider_id) FROM messaging.email_event e
                WHERE e.kind = 'email.delivery_delayed'
                AND e.event_at >= d.start AND e.event_at < d.start + interval '1 day') AS "deferrals!",
             (SELECT count(DISTINCT e.provider_id) FROM messaging.email_event e
                WHERE e.kind = 'email.bounced'
                AND e.event_at >= d.start AND e.event_at < d.start + interval '1 day') AS "bounces!",
             (SELECT count(DISTINCT e.provider_id) FROM messaging.email_event e
                WHERE e.kind = 'email.complained'
                AND e.event_at >= d.start AND e.event_at < d.start + interval '1 day') AS "complaints!"
           FROM days d ORDER BY d.start"#,
        now.to_sqlx() as _,
        WINDOW_DAYS,
    )
    .fetch_all(&mut *db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| EmailDay {
            day: r.start.to_jiff().to_zoned(arizona()).date(),
            sends: r.sends,
            deferrals: r.deferrals,
            bounces: r.bounces,
            complaints: r.complaints,
        })
        .collect())
}

/// OPS-046, D-490: checks the Arizona day before `now`'s and, when its deferrals exceed
/// [`DEFERRAL_LIMIT_PERCENT`] of its sends, raises one High [`DEFERRAL_ALERT`] unless one was
/// already raised today; returns the new alert's ID. Outside the window it raises nothing.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn check_deferrals(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<Option<Uuid>, OutboxError> {
    let day = (now - DAY).to_zoned(arizona()).date();
    let days = email_days(db, now).await?;
    let high = days.iter().any(|d| d.day == day && d.deferrals_too_high());
    if !high {
        return Ok(None);
    }
    raise_alert_once_a_day(db, DEFERRAL_ALERT, Severity::High)
        .await
        .map_err(|_| OutboxError::Database)
}

/// OPS-046: queues the next daily [`DEFERRAL_CHECK_KIND`] task, one per day.
///
/// # Errors
///
/// The [`TaskError`] of the insert.
pub async fn schedule_deferral_check(
    db: &mut PgConnection,
    now: Timestamp,
) -> Result<(), TaskError> {
    let run_at = now + DAY;
    let mut task = NewTask::new(DEFERRAL_CHECK_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!(
        "{DEFERRAL_CHECK_KIND}:{}",
        run_at.as_second() / 86_400
    ));
    enqueue(db, &task).await.map(|_| ())
}

/// OPS-046, D-3883: registers [`DEFERRAL_CHECK_KIND`] at `clock`'s now: the next day's check
/// first, so a run that fails for good still leaves it, then [`check_deferrals`].
pub fn register_deferral_check(registry: &mut Registry, clock: Arc<dyn Clock>) {
    registry.register(DEFERRAL_CHECK_KIND, move |pool, _task| {
        let now = clock.now();
        async move {
            let mut conn = pool.acquire().await.map_err(|_| DB_FAILED)?;
            schedule_deferral_check(&mut conn, now)
                .await
                .map_err(|_| DB_FAILED)?;
            check_deferrals(&mut conn, now)
                .await
                .map(|_| ())
                .map_err(|_| DB_FAILED)
        }
    });
}
