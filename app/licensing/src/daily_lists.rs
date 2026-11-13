//! The daily ROC and city license lists, run by the worker (T371, D-2704; LIC-006, LIC-007,
//! LIC-008, LIC-010, LIC-039, CARD-015, D-230, D-942).
//!
//! [`DAILY_LISTS_KIND`] is one task per list ([`DailyList`]), seeded by [`schedule_daily_lists`]
//! at `run_hour` Arizona time (a setting). Each run fetches its list through `LicenseSource` and
//! records it with `import_roc_list` or `import_city_list` dated the Arizona day, so a second run
//! for the same list date records nothing new. A fetch or import failure is a failing list: its
//! failure start is the run hour on the day after its latest recorded list (licensing's own import
//! records; `licensing.list_import` keeps successful imports only), or, for a list never recorded,
//! the start carried in the task's payload; `alert_list_failure` raises the alerts whose limits
//! fell since the last run, also carried in the payload (no new table, D-3106). While the ROC list
//! fails, `lookup_while_list_fails` runs for its active businesses at the throttle, at most `batch`
//! a run, once per business per Arizona day, the cursor in the payload; the pairs come from the
//! caller's [`ActiveLicenses`] (`businesses`, wired by `server`, since licensing may not call it).
//! A failing list re-queues itself every `retry_every` (an hour) instead of daily, so the 12-hour
//! alert fires within the hour it falls due; a good list ends the failure. Every run's slot lies on
//! one grid anchored at the run hour, and each slot's dedupe key names the list and the instant, so
//! two chains (a re-seed at start-up) merge into one; the alerts and the next run commit in one
//! transaction, and a run whose next slot was already queued raises nothing (JOB-006, R11).

use std::future::Future;
use std::ops::RangeInclusive;

use askhava_adapters::api::LicenseSource;
use askhava_core::api::{Claimed, HandlerError, NewTask, SettingsError, enqueue};
use jiff::civil::Date;
use jiff::tz::{TimeZone, offset};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::handlers::{
    FailureLimits, ListFailureAlert, alert_list_failure, arizona_day, lookup_while_list_fails,
};
use crate::imports::ImportError;
use crate::service::{ImportSummary, import_city_list, import_roc_list};

/// The `core.task` kind of one list's daily run (LIC-007, LIC-039).
pub const DAILY_LISTS_KIND: &str = "licensing.daily_lists";
/// The Arizona hour the daily run is due, 0 to 23 (default 6, before `businesses`' 7 a.m. re-apply).
pub const LIST_HOUR_KEY: &str = "ASKHAVA_LICENSE_LIST_HOUR";
/// Minutes between runs while a list fails, 5 to 720 (default 60, LIC-039).
pub const LIST_RETRY_KEY: &str = "ASKHAVA_LICENSE_LIST_RETRY_MINUTES";
/// The most live lookups one failing run makes, 1 to 1,000 (default 50, LIC-039).
pub const LOOKUP_BATCH_KEY: &str = "ASKHAVA_LICENSE_LOOKUP_BATCH";
/// The handler's failure sentences (D-491).
const FAILED: HandlerError = HandlerError("the daily license list database call failed");
const BAD_PAYLOAD: HandlerError = HandlerError("the daily license list task names no list");
const DAY: SignedDuration = SignedDuration::from_hours(24);

/// The two daily lists, named as the license source and `licensing.list_import` name them (D-2954).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DailyList {
    /// The ROC's daily list (LIC-007).
    Roc,
    /// The city business-license list (LIC-010).
    City,
}

impl DailyList {
    /// Both lists, in seeding order.
    pub const ALL: [Self; 2] = [Self::Roc, Self::City];

    /// The list's name: `roc` or `city`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Roc => "roc",
            Self::City => "city",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|list| list.name() == name)
    }
}

/// The daily run's settings (LIC-039, D-942); `limits` are the LIC-008 and LIC-039 clocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyListSettings {
    /// The Arizona hour the daily run is due.
    pub run_hour: i8,
    /// The time between runs while a list fails.
    pub retry_every: SignedDuration,
    /// The most live lookups one run makes; the rest wait for the next run.
    pub batch: u32,
    /// The alert clocks and the lookup throttle.
    pub limits: FailureLimits,
}

impl Default for DailyListSettings {
    fn default() -> Self {
        Self {
            run_hour: 6,
            retry_every: SignedDuration::from_hours(1),
            batch: 50,
            limits: FailureLimits::default(),
        }
    }
}

fn parse_in(
    key: &'static str,
    value: &str,
    range: RangeInclusive<u32>,
) -> Result<u32, SettingsError> {
    let n = value.parse::<u32>().ok().filter(|n| range.contains(n));
    n.ok_or(SettingsError::Invalid(key))
}

impl DailyListSettings {
    /// The settings from `pairs` ([`LIST_HOUR_KEY`], [`LIST_RETRY_KEY`], [`LOOKUP_BATCH_KEY`]); an
    /// unset or empty key keeps its default.
    ///
    /// # Errors
    ///
    /// [`SettingsError::Invalid`] naming the key of a value out of range.
    pub fn from_pairs<I>(pairs: I) -> Result<Self, SettingsError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut settings = Self::default();
        for (name, value) in pairs {
            let value = value.trim();
            match name.as_str() {
                _ if value.is_empty() => {}
                LIST_HOUR_KEY => {
                    let hour = parse_in(LIST_HOUR_KEY, value, 0..=23)?;
                    settings.run_hour = i8::try_from(hour).unwrap_or_default();
                }
                LIST_RETRY_KEY => {
                    let minutes = parse_in(LIST_RETRY_KEY, value, 5..=720)?;
                    settings.retry_every = SignedDuration::from_mins(i64::from(minutes));
                }
                LOOKUP_BATCH_KEY => settings.batch = parse_in(LOOKUP_BATCH_KEY, value, 1..=1000)?,
                _ => {}
            }
        }
        Ok(settings)
    }
}

/// The active businesses holding a license on the ROC list, with the number their latest daily
/// check is under, given by the composition (licensing may not call `businesses`).
pub trait ActiveLicenses {
    /// Up to `limit` active businesses with an ID after `after`, by ID, with their numbers.
    fn active_after(
        &self,
        conn: &mut PgConnection,
        after: Option<Uuid>,
        limit: u32,
    ) -> impl Future<Output = Result<Vec<(Uuid, String)>, ImportError>> + Send;
}

/// What one run did (LIC-007, LIC-039).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRun {
    /// The list run.
    pub list: DailyList,
    /// The import's summary, or `None` when the list failed.
    pub recorded: Option<ImportSummary>,
    /// The alerts this run raised.
    pub alerts: Vec<ListFailureAlert>,
    /// The live lookups this run made.
    pub lookups: usize,
    /// When the next run is due.
    pub next_run: Timestamp,
}

/// The state the task's payload carries (D-3106): times, a date and a business ID, nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ListState {
    list: DailyList,
    failing_since: Option<Timestamp>,
    last_run: Option<Timestamp>,
    after: Option<Uuid>,
    swept_on: Option<Date>,
}

impl ListState {
    const fn fresh(list: DailyList, last_run: Option<Timestamp>) -> Self {
        Self {
            list,
            failing_since: None,
            last_run,
            after: None,
            swept_on: None,
        }
    }

    fn from_payload(payload: &Value) -> Option<Self> {
        let text = |key: &str| payload.get(key).and_then(Value::as_str);
        Some(Self {
            list: DailyList::from_name(text("list")?)?,
            failing_since: text("failing_since").and_then(|t| t.parse().ok()),
            last_run: text("last_run").and_then(|t| t.parse().ok()),
            after: text("after").and_then(|t| Uuid::parse_str(t).ok()),
            swept_on: text("swept_on").and_then(|t| t.parse().ok()),
        })
    }

    fn payload(&self) -> Value {
        json!({
            "list": self.list.name(),
            "failing_since": self.failing_since.map(|t| t.to_string()),
            "last_run": self.last_run.map(|t| t.to_string()),
            "after": self.after.map(|id| id.to_string()),
            "swept_on": self.swept_on.map(|d| d.to_string()),
        })
    }
}

/// The run hour, Arizona time, on `day`.
fn run_time(day: Date, run_hour: i8) -> Option<Timestamp> {
    let at = day
        .at(run_hour, 0, 0, 0)
        .to_zoned(TimeZone::fixed(offset(-7)));
    at.ok().map(|at| at.timestamp())
}

/// The first instant after `now` on the grid of `every` anchored at today's run hour (Arizona).
fn next_slot(now: Timestamp, run_hour: i8, every: SignedDuration) -> Option<Timestamp> {
    let anchor = run_time(arizona_day(now), run_hour)?;
    let step = every.as_secs().max(1);
    let k = now.duration_since(anchor).as_secs().div_euclid(step) + 1;
    let offset = SignedDuration::from_secs(step.checked_mul(k)?);
    anchor.checked_add(offset).ok()
}

/// Queues `state`'s run at `run_at`; false when that slot of the list was already queued.
async fn queue(
    conn: &mut PgConnection,
    state: &ListState,
    run_at: Timestamp,
) -> Result<bool, HandlerError> {
    let mut task = NewTask::new(DAILY_LISTS_KIND, state.payload(), run_at);
    let slot = run_at.as_second();
    task.dedupe_key = Some(format!("{DAILY_LISTS_KIND}.{}.{slot}", state.list.name()));
    let queued = enqueue(conn, &task).await.map_err(|_| FAILED)?;
    Ok(queued.is_some())
}

/// LIC-007, D-2704: queues each list's run at the first run hour after `now` (deduped by slot, so
/// seeding at every start-up queues each once).
///
/// # Errors
///
/// [`ImportError::Database`].
pub async fn schedule_daily_lists(
    conn: &mut PgConnection,
    now: Timestamp,
    settings: DailyListSettings,
) -> Result<(), ImportError> {
    let run_at = next_slot(now, settings.run_hour, DAY).ok_or(ImportError::Database)?;
    for list in DailyList::ALL {
        let state = ListState::fresh(list, None);
        queue(conn, &state, run_at)
            .await
            .map_err(|_| ImportError::Database)?;
    }
    Ok(())
}

/// Fetches and records `list` for `day`: `Some` when recorded now or before, `None` when the list
/// failed to fetch, read or pass the import's checks (logged without a value).
async fn fetch_and_record<S: LicenseSource + Sync>(
    conn: &mut PgConnection,
    source: &S,
    list: DailyList,
    day: Date,
) -> Result<Option<ImportSummary>, HandlerError> {
    let bytes = match source.fetch_daily_list(list.name()).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(%error, list = list.name(), "daily license list not fetched");
            return Ok(None);
        }
    };
    let text = String::from_utf8_lossy(&bytes);
    let recorded = match list {
        DailyList::Roc => import_roc_list(conn, day, &text).await,
        DailyList::City => import_city_list(conn, day, &text).await,
    };
    match recorded {
        Ok(summary) => Ok(Some(summary)),
        Err(ImportError::Database) => Err(FAILED),
        Err(error) => {
            tracing::warn!(%error, list = list.name(), "daily license list refused");
            Ok(None)
        }
    }
}

/// The failure start from licensing's own records: the run hour on the day after the list's latest
/// recorded date; `None` for a list never recorded.
async fn recorded_failure_start(
    conn: &mut PgConnection,
    list: DailyList,
    run_hour: i8,
) -> Result<Option<Timestamp>, HandlerError> {
    let latest = sqlx::query_scalar!(
        r#"SELECT max(list_date)::text AS "day" FROM licensing.list_import WHERE source = $1"#,
        list.name(),
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(|_| FAILED)?;
    let Some(day) = latest else {
        return Ok(None);
    };
    let day: Date = day.parse().map_err(|_| FAILED)?;
    let next = day.tomorrow().map_err(|_| FAILED)?;
    run_time(next, run_hour).map(Some).ok_or(FAILED)
}

/// LIC-039, D-230: one capped batch of throttled lookups for the ROC list's active businesses,
/// once per business per Arizona day; returns the state after it and the lookups made.
async fn sweep<S, A>(
    conn: &mut PgConnection,
    (source, active): (&S, &A),
    state: ListState,
    (now, settings): (Timestamp, DailyListSettings),
) -> Result<(ListState, usize), HandlerError>
where
    S: LicenseSource + Sync,
    A: ActiveLicenses + Sync,
{
    let today = arizona_day(now);
    if state.list != DailyList::Roc || state.swept_on == Some(today) {
        return Ok((state, 0));
    }
    let batch = active.active_after(conn, state.after, settings.batch).await;
    let batch = batch.map_err(|_| FAILED)?;
    let pairs: Vec<(Uuid, &str)> = batch.iter().map(|(id, n)| (*id, n.as_str())).collect();
    let looked = lookup_while_list_fails(conn, source, &pairs, (now, settings.limits)).await;
    looked.map_err(|_| FAILED)?;
    let done = batch.len() < usize::try_from(settings.batch).unwrap_or(usize::MAX);
    let after = if done {
        None
    } else {
        batch.last().map(|(id, _)| *id)
    };
    let swept_on = if done { Some(today) } else { state.swept_on };
    Ok((
        ListState {
            after,
            swept_on,
            ..state
        },
        batch.len(),
    ))
}

/// The [`DAILY_LISTS_KIND`] handler (LIC-006, LIC-007, LIC-008, LIC-010, LIC-039, CARD-015,
/// D-2704): fetches and records the task's list; on a failure runs a batch of lookups (ROC), then
/// in one transaction queues the next run (daily after a good list, `retry_every` while failing)
/// and, when that slot was not queued yet, raises the alerts due since the last run.
///
/// # Errors
///
/// A fixed [`HandlerError`] for a payload naming no list or a database failure; the task retries
/// and a retry raises nothing twice.
pub async fn run_daily_list<S, A>(
    pool: &PgPool,
    parts: (&S, &A),
    task: &Claimed,
    (now, settings): (Timestamp, DailyListSettings),
) -> Result<ListRun, HandlerError>
where
    S: LicenseSource + Sync,
    A: ActiveLicenses + Sync,
{
    let state = ListState::from_payload(&task.payload).ok_or(BAD_PAYLOAD)?;
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    let day = arizona_day(now);
    let recorded = fetch_and_record(&mut conn, parts.0, state.list, day).await?;
    if recorded.is_some() {
        let next = ListState::fresh(state.list, Some(now));
        let next_run = next_slot(now, settings.run_hour, DAY).ok_or(FAILED)?;
        queue(&mut conn, &next, next_run).await?;
        let (alerts, lookups) = (Vec::new(), 0);
        return Ok(ListRun {
            list: state.list,
            recorded,
            alerts,
            lookups,
            next_run,
        });
    }
    let since = recorded_failure_start(&mut conn, state.list, settings.run_hour).await?;
    let since = since.or(state.failing_since).unwrap_or(now);
    let last_run = state
        .last_run
        .unwrap_or_else(|| now.saturating_sub(settings.retry_every).unwrap_or(now));
    let (next, lookups) = sweep(&mut conn, parts, state, (now, settings)).await?;
    let next = ListState {
        failing_since: Some(since),
        last_run: Some(now),
        ..next
    };
    let next_run = next_slot(now, settings.run_hour, settings.retry_every).ok_or(FAILED)?;
    let mut tx = conn.begin().await.map_err(|_| FAILED)?;
    let alerts = match queue(&mut tx, &next, next_run).await? {
        true => alert_list_failure(&mut tx, (since, last_run, now), settings.limits).await,
        false => Ok(Vec::new()),
    };
    let alerts = alerts.map_err(|_| FAILED)?;
    tx.commit().await.map_err(|_| FAILED)?;
    Ok(ListRun {
        list: state.list,
        recorded,
        alerts,
        lookups,
        next_run,
    })
}
