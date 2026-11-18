//! The pilot scorecard's request inputs from `jobs` (OPS-039; D-3753, D-3883; T478).
//!
//! Read-only, counts and IDs only, each over a half-open window (`from` counted, `to` not) on a
//! job's `created_at`. [`requests_per_week`] counts the requests started in each Arizona week
//! (Monday 00:00 in `America/Phoenix`, which keeps no daylight time), per trade and with no trade
//! yet. [`jobs_by_trade`] lists, per trade, the requests that reached their cards: the trade is the
//! job's catalog category in its first `jobs.job_facts` row, written when its cards go out, so a
//! request with a trade is a routable one (a trade with a business that covers it, D-468's rules
//! tab). The job IDs are what the readers of schemas that store no trade take (`intake`, `calls`,
//! `businesses`; D-3883), so the scorecard (T158) puts each input per trade and in total. Nothing
//! here reaches a card or routing (D-457). OPS-046's email readers and its daily deferral alert
//! are `messaging`'s (T531, D-3883), not these.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::JobsError;

/// The requests started in one Arizona week, for one trade or for none yet (OPS-039).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeekRequests {
    /// The week's start: Monday 00:00 Arizona time.
    pub week: Timestamp,
    /// The trade code, `None` for requests that have not reached their cards.
    pub trade: Option<String>,
    /// The requests started that week.
    pub requests: i64,
}

/// One trade's routable requests in the window (OPS-039; D-3883).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeJobs {
    /// The trade code (the job's catalog category).
    pub trade: String,
    /// The jobs of that trade, oldest first; their count is the trade's routable requests.
    pub jobs: Vec<Uuid>,
}

/// OPS-039: the requests created at or after `from` and before `to`, per Arizona week and trade,
/// weeks oldest first, then trades in code order with the no-trade row last.
///
/// # Errors
///
/// [`JobsError::Database`].
pub async fn requests_per_week(
    db: &mut PgConnection,
    (from, to): (Timestamp, Timestamp),
) -> Result<Vec<WeekRequests>, JobsError> {
    let rows = sqlx::query!(
        r#"SELECT date_trunc('week', j.created_at AT TIME ZONE 'America/Phoenix')
                    AT TIME ZONE 'America/Phoenix' AS "week!: jiff_sqlx::Timestamp",
                  f.category AS "trade?", count(*) AS "requests!"
           FROM jobs.job j LEFT JOIN jobs.job_facts f ON f.job_id = j.id AND f.seq = 1
           WHERE j.created_at >= $1 AND j.created_at < $2
           GROUP BY 1, 2 ORDER BY 1, 2 NULLS LAST"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(db)
    .await
    .map_err(|_| JobsError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| WeekRequests {
            week: r.week.to_jiff(),
            trade: r.trade,
            requests: r.requests,
        })
        .collect())
}

/// OPS-039, D-3883: the jobs created at or after `from` and before `to` that reached their cards,
/// per trade, trades in code order; a trade with none is absent.
///
/// # Errors
///
/// [`JobsError::Database`].
pub async fn jobs_by_trade(
    db: &mut PgConnection,
    (from, to): (Timestamp, Timestamp),
) -> Result<Vec<TradeJobs>, JobsError> {
    sqlx::query_as!(
        TradeJobs,
        r#"SELECT f.category AS "trade!",
                  array_agg(j.id ORDER BY j.created_at, j.id) AS "jobs!"
           FROM jobs.job j JOIN jobs.job_facts f ON f.job_id = j.id AND f.seq = 1
           WHERE j.created_at >= $1 AND j.created_at < $2
           GROUP BY f.category ORDER BY f.category"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(db)
    .await
    .map_err(|_| JobsError::Database)
}
