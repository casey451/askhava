//! The weekly digest's not-covered roll-up by trade (OPS-041, OPS-030, INT-017; D-435, D-4202;
//! T605).
//!
//! [`not_covered_by_trade`] counts the requests closed as not covered (one
//! `intake.recruiting_demand` row per request and trade) in a half-open window, `from` counted and
//! `to` not, each request once per trade. It returns trade codes and counts only, never a town, a
//! request number or a homeowner detail. The per-request email's monthly count is `routing`'s.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;

use crate::models::IntakeError;

/// One trade's not-covered requests in the window (OPS-041).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotCoveredCount {
    /// The trade code.
    pub trade: String,
    /// The distinct requests closed as not covered for that trade.
    pub requests: i64,
}

/// OPS-041, OPS-030: the not-covered requests created at or after `from` and before `to`, per
/// trade, trades in code order; a trade with none is absent.
///
/// # Errors
///
/// [`IntakeError::Database`].
pub async fn not_covered_by_trade(
    db: &mut PgConnection,
    (from, to): (Timestamp, Timestamp),
) -> Result<Vec<NotCoveredCount>, IntakeError> {
    sqlx::query_as!(
        NotCoveredCount,
        r#"SELECT trade, count(DISTINCT conversation_id) AS "requests!"
           FROM intake.recruiting_demand WHERE created_at >= $1 AND created_at < $2
           GROUP BY trade ORDER BY trade"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(db)
    .await
    .map_err(|_| IntakeError::Database)
}
