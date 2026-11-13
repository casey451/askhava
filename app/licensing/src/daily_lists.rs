//! The daily ROC and city license lists, run by the worker (T371, D-2704; LIC-006, LIC-007,
//! LIC-008, LIC-010, LIC-039, CARD-015, D-230, D-942).
//!
//! [`DAILY_LISTS_KIND`] is one task chain per list ([`DailyList`]), seeded by
//! [`schedule_daily_lists`] only when the list has no task due or running. Each run fetches its
//! list through `LicenseSource` and records it with `import_roc_list` or `import_city_list` dated
//! the list day (the Arizona day of the latest run hour at or before the run, so a list fetched
//! between midnight and the run hour is the previous day's), so a second run for the same list day
//! records nothing new. A fetch or import failure is a failing list: its failure start is the run
//! hour on the day after its latest recorded list (`licensing.list_import` keeps successful
//! imports only), or, for a list never recorded, the start carried in the task's payload. A
//! failing run first commits, in one transaction, the next run (every `retry_every`, with the
//! failure start, this run's time and the lookup cursor already moved past this run's batch) and,
//! when that slot is new, `alert_list_failure`'s alerts due since the last run (no new table,
//! D-3106); only then, and only when the slot was new, it runs `lookup_while_list_fails` (ROC only)
//! for that batch within `sweep_budget`, so a slow agency site or a handler timeout can never lose
//! the chain, an alert or the cursor, and a retried run repeats no lookups (D-3416). The pairs come
//! from the caller's [`ActiveLicenses`] (`businesses`, wired by `server`). A good list ends the
//! failure and queues the next run hour. Every slot lies on one grid anchored at the run hour and
//! its dedupe key names the list and the instant, so a retried run lands on its own slot
//! (JOB-006, R11, D-3412).

use std::future::Future;
use std::ops::RangeInclusive;

use askhava_adapters::api::LicenseSource;
use askhava_core::api::{Claimed, HandlerError, NewTask, SettingsError, due_or_running, enqueue};
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
    /// The longest one run's lookups may take, well under the worker's two-minute handler timeout
    /// (`Lease::WORKER`); it also caps the batch at this over the throttle (D-3416). Not a key.
    pub sweep_budget: SignedDuration,
}

impl Default for DailyListSettings {
    fn default() -> Self {
        Self {
            run_hour: 6,
            retry_every: SignedDuration::from_hours(1),
            batch: 50,
            limits: FailureLimits::default(),
            sweep_budget: SignedDuration::from_secs(60),
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

/// The list day of a run at `now`: the Arizona day of the latest run hour at or before `now`, so a
/// run between midnight and the run hour records the previous day's list (D-3416).
fn list_day(now: Timestamp, run_hour: i8) -> Date {
    let back = SignedDuration::from_hours(i64::from(run_hour));
    arizona_day(now.saturating_sub(back).unwrap_or(now))
}

/// The first instant after `now` on the grid of `every` anchored at today's run hour (Arizona).
fn next_slot(now: Timestamp, run_hour: i8, every: SignedDuration) -> Option<Timestamp> {
    let anchor = run_time(arizona_day(now), run_hour)?;
    let step = every.as_secs().max(1);
    let k = now.duration_since(anchor).as_secs().div_euclid(step) + 1;
    let offset = SignedDuration::from_secs(step.checked_mul(k)?);
    anchor.checked_add(offset).ok()
}

/// The dedupe-key prefix of every slot of `list`.
fn key_prefix(list: DailyList) -> String {
    format!("{DAILY_LISTS_KIND}.{}.", list.name())
}

/// Queues `state`'s run at `run_at`; false when that slot of the list was already queued.
async fn queue(
    conn: &mut PgConnection,
    state: &ListState,
    run_at: Timestamp,
) -> Result<bool, HandlerError> {
    let mut task = NewTask::new(DAILY_LISTS_KIND, state.payload(), run_at);
    task.dedupe_key = Some(format!("{}{}", key_prefix(state.list), run_at.as_second()));
    let queued = enqueue(conn, &task).await.map_err(|_| FAILED)?;
    Ok(queued.is_some())
}

/// LIC-007, LIC-039, D-2704, D-3416: queues a run, at the next `retry_every` slot, for each list
/// with no run due or running, so a start-up never merges into or clobbers a live chain (a failing
/// one keeps its state) and a chain lost to a dead letter starts again.
///
/// # Errors
///
/// [`ImportError::Database`].
pub async fn schedule_daily_lists(
    conn: &mut PgConnection,
    now: Timestamp,
    settings: DailyListSettings,
) -> Result<(), ImportError> {
    let run_at = next_slot(now, settings.run_hour, settings.retry_every);
    let run_at = run_at.ok_or(ImportError::Database)?;
    for list in DailyList::ALL {
        let live = due_or_running(conn, DAILY_LISTS_KIND, &key_prefix(list)).await;
        if live.map_err(|_| ImportError::Database)? {
            continue;
        }
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

/// The most lookups one run makes: `batch`, and no more than fit in `sweep_budget` at the
/// throttle (D-3416).
fn batch_cap(settings: DailyListSettings) -> u32 {
    let spacing = settings.limits.lookup_spacing.as_millis();
    let fits = settings.sweep_budget.as_millis().checked_div(spacing);
    let fits = u32::try_from(fits.unwrap_or(i128::MAX).max(1)).unwrap_or(u32::MAX);
    fits.min(settings.batch)
}

/// LIC-039, D-230: picks this run's batch of the ROC list's active businesses (once per business
/// per Arizona day) and returns the state with the cursor past it, before any lookup is made.
async fn plan_sweep<A: ActiveLicenses + Sync>(
    conn: &mut PgConnection,
    active: &A,
    state: ListState,
    (now, settings): (Timestamp, DailyListSettings),
) -> Result<(ListState, Vec<(Uuid, String)>), HandlerError> {
    let today = arizona_day(now);
    if state.list != DailyList::Roc || state.swept_on == Some(today) {
        return Ok((state, Vec::new()));
    }
    let cap = batch_cap(settings);
    let batch = active.active_after(conn, state.after, cap).await;
    let batch = batch.map_err(|_| FAILED)?;
    let done = batch.len() < usize::try_from(cap).unwrap_or(usize::MAX);
    let after = if done {
        None
    } else {
        batch.last().map(|(id, _)| *id)
    };
    let swept_on = if done { Some(today) } else { state.swept_on };
    let next = ListState {
        after,
        swept_on,
        ..state
    };
    Ok((next, batch))
}

/// LIC-039, D-230, D-3416: the planned lookups at the throttle, stopped at `sweep_budget`; returns
/// how many finished (none when stopped: the rest wait for tomorrow's sweep).
async fn look_up<S: LicenseSource + Sync>(
    conn: &mut PgConnection,
    source: &S,
    batch: &[(Uuid, String)],
    (now, settings): (Timestamp, DailyListSettings),
) -> Result<usize, HandlerError> {
    let pairs: Vec<(Uuid, &str)> = batch.iter().map(|(id, n)| (*id, n.as_str())).collect();
    let budget = std::time::Duration::try_from(settings.sweep_budget).unwrap_or_default();
    let looked = lookup_while_list_fails(conn, source, &pairs, (now, settings.limits));
    match tokio::time::timeout(budget, looked).await {
        Ok(looked) => looked.map(|outcomes| outcomes.len()).map_err(|_| FAILED),
        Err(_) => {
            tracing::warn!(
                planned = pairs.len(),
                "daily license lookups stopped at the budget"
            );
            Ok(0)
        }
    }
}

/// The [`DAILY_LISTS_KIND`] handler (LIC-006, LIC-007, LIC-008, LIC-010, LIC-039, CARD-015,
/// D-2704, D-3416): fetches and records the task's list for its list day and queues the next run
/// hour; on a failure, [`run_failing`].
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
    let day = list_day(now, settings.run_hour);
    let recorded = fetch_and_record(&mut conn, parts.0, state.list, day).await?;
    if recorded.is_none() {
        return run_failing(&mut conn, parts, state, (now, settings)).await;
    }
    let next = ListState::fresh(state.list, Some(now));
    let next_run = next_slot(now, settings.run_hour, DAY).ok_or(FAILED)?;
    queue(&mut conn, &next, next_run).await?;
    Ok(ListRun {
        list: state.list,
        recorded,
        alerts: Vec::new(),
        lookups: 0,
        next_run,
    })
}

/// LIC-039, D-942, D-3416: a failing list's run. One transaction queues the next slot (the failure
/// start, this run and the cursor past this batch) and, when that slot is new, raises the alerts
/// due since the last run (a seed's run, with no last run, counts from the failure start); then,
/// only when the slot was new, the batch's lookups within the budget.
async fn run_failing<S, A>(
    conn: &mut PgConnection,
    (source, active): (&S, &A),
    state: ListState,
    (now, settings): (Timestamp, DailyListSettings),
) -> Result<ListRun, HandlerError>
where
    S: LicenseSource + Sync,
    A: ActiveLicenses + Sync,
{
    let since = recorded_failure_start(conn, state.list, settings.run_hour).await?;
    let since = since.or(state.failing_since).unwrap_or(now);
    let last_run = state.last_run.unwrap_or(since);
    let (next, batch) = plan_sweep(conn, active, state, (now, settings)).await?;
    let next = ListState {
        failing_since: Some(since),
        last_run: Some(now),
        ..next
    };
    let next_run = next_slot(now, settings.run_hour, settings.retry_every).ok_or(FAILED)?;
    let mut tx = conn.begin().await.map_err(|_| FAILED)?;
    let fresh = queue(&mut tx, &next, next_run).await?;
    let alerts = match fresh {
        true => alert_list_failure(&mut tx, (since, last_run, now), settings.limits).await,
        false => Ok(Vec::new()),
    };
    let alerts = alerts.map_err(|_| FAILED)?;
    tx.commit().await.map_err(|_| FAILED)?;
    let lookups = match fresh {
        true => look_up(conn, source, &batch, (now, settings)).await?,
        false => 0,
    };
    Ok(ListRun {
        list: state.list,
        recorded: None,
        alerts,
        lookups,
        next_run,
    })
}
