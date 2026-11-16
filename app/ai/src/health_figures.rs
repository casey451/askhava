//! The AI figures of the owner health page (OPS-008; T475, D-3753).
//!
//! [`ai_figures`] counts, for a window `[from, to)`, the AI calls made and the calls that ended
//! in an error, from `ai.call_stat` (`migrations/0040_ai_adapter.sql`), in one query. A call is
//! any row but a daily-cap refusal (no request went out); an error is any outcome but `ok` and
//! `daily_cap`, the rule `weekly_counts` uses. Counts only, never a subject or a
//! request ID (PRIV-005).

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;

use crate::models::AiError;

/// The AI calls and AI errors of one window (OPS-008).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AiFigures {
    /// Calls made in the window (daily-cap refusals excluded).
    pub calls: i64,
    /// Of those, the calls that ended without a reading (any outcome but `ok`).
    pub errors: i64,
}

/// OPS-008: the [`AiFigures`] of `[from, to)`.
///
/// # Errors
///
/// [`AiError::Database`] when the counts cannot be read.
pub async fn ai_figures(
    db: &mut PgConnection,
    from: Timestamp,
    to: Timestamp,
) -> Result<AiFigures, AiError> {
    let r = sqlx::query!(
        r#"SELECT count(*) FILTER (WHERE outcome <> 'daily_cap') AS "calls!",
                  count(*) FILTER (WHERE outcome NOT IN ('ok', 'daily_cap')) AS "errors!"
             FROM ai.call_stat WHERE at >= $1 AND at < $2"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_one(db)
    .await
    .map_err(|_| AiError::Database)?;
    Ok(AiFigures {
        calls: r.calls,
        errors: r.errors,
    })
}
