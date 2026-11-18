//! The owner portal's needs-review queue (BIZ-027, TD-BIZ-07; D-204, D-3882, T108): each sign-up
//! now in needs review with the time it began waiting, oldest first. IDs and times only; the
//! owner page renders `review_target_line` from each row. The queue blocks nothing (D-204).

use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{BusinessesError, db};

/// One sign-up waiting for Casey's decision: the business and when it began waiting (BIZ-027).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewWaiting {
    /// The business's record ID.
    pub business: Uuid,
    /// When its sign-up went into needs review (`businesses.signup.review_since`).
    pub review_since: Timestamp,
}

/// BIZ-027, D-3882: every business in needs review whose sign-up has a `review_since`, oldest
/// first (ties by business ID).
///
/// # Errors
///
/// [`BusinessesError::Database`].
pub async fn needs_review_signups(
    conn: &mut PgConnection,
) -> Result<Vec<ReviewWaiting>, BusinessesError> {
    let rows = db(sqlx::query!(
        r#"SELECT s.business_id, s.review_since AS "review_since!" FROM businesses.signup s
           JOIN businesses.business b ON b.id = s.business_id
           WHERE b.status = 'needs_review' AND s.review_since IS NOT NULL
           ORDER BY s.review_since, s.business_id"#
    )
    .fetch_all(conn)
    .await)?;
    Ok(rows
        .into_iter()
        .map(|r| ReviewWaiting {
            business: r.business_id,
            review_since: r.review_since.to_jiff(),
        })
        .collect())
}
