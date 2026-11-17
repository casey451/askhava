//! Two pilot scorecard inputs `ops` keeps for T158's page (OPS-039, PICK-057; D-3753, D-3883):
//! the businesses' "Not a fit for us" answers and Casey's weekly minutes entry.
//!
//! This module is the only writer and reader of `ops.not_a_fit_answer` and `ops.minutes_entry`
//! (migration `0200_ops_scorecard_inputs.sql`, T479). A not-a-fit answer is one row per business
//! and job: a second answer for the same pair adds nothing. The answer changes no routing or fee:
//! nothing outside this module reads it, and only [`not_a_fit_counts`] reports it, as the count per
//! trade and reason in an Arizona calendar month. A minutes entry is append-only; the latest entry
//! for an Arizona week (Monday to Sunday) is that week's minutes, split into recruiting and
//! everything else. Codes, counts, IDs and times only; no personal data (PRIV-028).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{OpsError, is_code_name};

/// PICK-057: why a job is not a fit for a business, the four reasons the job card page offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NotAFitReason {
    /// The job is not the business's trade.
    NotMyTrade,
    /// The job is not in a town the business serves.
    NotMyTown,
    /// The job is too big or too small for the business.
    TooBigOrSmall,
    /// Any other reason.
    Other,
}

impl NotAFitReason {
    /// PICK-057: every reason, in the order the page offers them.
    pub const ALL: [Self; 4] = [
        Self::NotMyTrade,
        Self::NotMyTown,
        Self::TooBigOrSmall,
        Self::Other,
    ];

    /// The reason's stored code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotMyTrade => "not_my_trade",
            Self::NotMyTown => "not_my_town",
            Self::TooBigOrSmall => "too_big_or_small",
            Self::Other => "other",
        }
    }

    /// PICK-057: the reason a code names, or `None` for any other text.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.code() == code)
    }
}

/// PICK-057: one business's not-a-fit answer on one job, with the job's trade code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotAFit<'a> {
    /// The job.
    pub job: Uuid,
    /// The business answering.
    pub business: Uuid,
    /// The job's trade (its catalog category code).
    pub trade: &'a str,
    /// The reason chosen.
    pub reason: NotAFitReason,
}

/// PICK-057, OPS-039: stores `answer` at `at`; true when stored, false (nothing written) when the
/// business already answered for this job. Writes nothing else, so no routing or fee changes.
///
/// # Errors
///
/// [`OpsError::NotACodeName`] for a trade that is not a code name, [`OpsError::Database`].
pub async fn record_not_a_fit(
    db: &mut PgConnection,
    answer: &NotAFit<'_>,
    at: Timestamp,
) -> Result<bool, OpsError> {
    if !is_code_name(answer.trade) {
        return Err(OpsError::NotACodeName("trade"));
    }
    let stored = sqlx::query!(
        "INSERT INTO ops.not_a_fit_answer (job_id, business_id, trade, reason, answered_at) \
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (business_id, job_id) DO NOTHING",
        answer.job,
        answer.business,
        answer.trade,
        answer.reason.code(),
        at.to_sqlx() as _,
    )
    .execute(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(stored.rows_affected() == 1)
}

/// The number of not-a-fit answers for one trade and reason in a month (OPS-039).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotAFitCount {
    /// The trade code.
    pub trade: String,
    /// The reason.
    pub reason: NotAFitReason,
    /// Answers in the month.
    pub count: i64,
}

/// PICK-057, OPS-039: the answers in `at`'s Arizona calendar month, counted per trade and reason,
/// by trade then reason code; a trade and reason with no answer is left out.
///
/// # Errors
///
/// [`OpsError::Database`], also for a stored reason this build does not know.
pub async fn not_a_fit_counts(
    db: &mut PgConnection,
    at: Timestamp,
) -> Result<Vec<NotAFitCount>, OpsError> {
    let rows = sqlx::query!(
        r#"SELECT trade, reason, count(*) AS "count!" FROM ops.not_a_fit_answer
           WHERE date_trunc('month', answered_at AT TIME ZONE 'America/Phoenix')
               = date_trunc('month', $1::timestamptz AT TIME ZONE 'America/Phoenix')
           GROUP BY trade, reason ORDER BY trade, reason"#,
        at.to_sqlx() as _,
    )
    .fetch_all(db)
    .await
    .map_err(|_| OpsError::Database)?;
    rows.into_iter()
        .map(|r| {
            Ok(NotAFitCount {
                reason: NotAFitReason::from_code(&r.reason).ok_or(OpsError::Database)?,
                trade: r.trade,
                count: r.count,
            })
        })
        .collect()
}

/// The most minutes one part of a week's entry can hold: the minutes in a week.
pub const WEEK_MINUTES_MAX: u32 = 7 * 24 * 60;

/// OPS-039: one week's minutes, split into recruiting and everything else; each at most
/// [`WEEK_MINUTES_MAX`], checked by [`WeekMinutes::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeekMinutes {
    recruiting: u32,
    other: u32,
}

impl WeekMinutes {
    /// OPS-039: the entry, or `None` when either part is over [`WEEK_MINUTES_MAX`].
    #[must_use]
    pub const fn new(recruiting: u32, other: u32) -> Option<Self> {
        if recruiting > WEEK_MINUTES_MAX || other > WEEK_MINUTES_MAX {
            None
        } else {
            Some(Self { recruiting, other })
        }
    }

    /// Minutes spent recruiting businesses.
    #[must_use]
    pub const fn recruiting(self) -> u32 {
        self.recruiting
    }

    /// Minutes spent on everything else.
    #[must_use]
    pub const fn other(self) -> u32 {
        self.other
    }

    /// Both parts together.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.recruiting + self.other
    }
}

/// Minutes as the table's integer; [`WeekMinutes::new`] keeps each part far below `i32::MAX`.
fn as_int(minutes: u32) -> i32 {
    i32::try_from(minutes).unwrap_or(i32::MAX)
}

/// OPS-039: Casey (`owner`) enters `minutes` for the Arizona week holding `at`; a later entry for
/// the same week replaces this one when read, and no entry is ever changed.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn record_minutes(
    db: &mut PgConnection,
    owner: Uuid,
    minutes: WeekMinutes,
    at: Timestamp,
) -> Result<(), OpsError> {
    sqlx::query!(
        "INSERT INTO ops.minutes_entry (week, recruiting, other, entered_by, entered_at) \
         VALUES (date_trunc('week', $1::timestamptz AT TIME ZONE 'America/Phoenix')::date, \
         $2, $3, $4, $1)",
        at.to_sqlx() as _,
        as_int(minutes.recruiting),
        as_int(minutes.other),
        owner,
    )
    .execute(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(())
}

/// OPS-039: the minutes of the Arizona week holding `at` (its latest entry), or `None` when the
/// week has no entry.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn minutes_for_week(
    db: &mut PgConnection,
    at: Timestamp,
) -> Result<Option<WeekMinutes>, OpsError> {
    let row = sqlx::query!(
        "SELECT recruiting, other FROM ops.minutes_entry \
         WHERE week = date_trunc('week', $1::timestamptz AT TIME ZONE 'America/Phoenix')::date \
         ORDER BY id DESC LIMIT 1",
        at.to_sqlx() as _,
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OpsError::Database)?;
    Ok(row.and_then(|r| {
        let part = |n: i32| u32::try_from(n).ok();
        WeekMinutes::new(part(r.recruiting)?, part(r.other)?)
    }))
}
