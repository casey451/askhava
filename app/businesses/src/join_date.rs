//! The free first win's join facts from the sign-up row (FEE-010; D-152, D-195, D-3779; T486).
//!
//! [`join_facts`] reads `businesses.signup` only: the business joins on the date its sign-up form
//! was submitted, never its activation (D-152), and its trades are the category codes it chose at
//! sign-up (FEE-010's "a category they signed up for"); a category added later through the job-type
//! checklist does not count (D-3779). The caller pairs these with `catalog::api::trades_opened`
//! and the launch-date setting to build `payments::api::JoinDates`. Read-only.

use jiff::Timestamp;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::{BusinessesError, db};

/// What FEE-010 needs from a business's sign-up: its submit time and the trades chosen then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinFacts {
    joined: Timestamp,
    trades: Vec<String>,
}

impl JoinFacts {
    /// FEE-010, D-152: when the sign-up form was submitted (`businesses.signup.submitted_at`).
    #[must_use]
    pub const fn joined(&self) -> Timestamp {
        self.joined
    }

    /// FEE-010, D-3779: the category codes chosen at sign-up (`businesses.signup.trades`, as
    /// stored: `sign_up` stores them sorted and once each); never a category added later to the
    /// checklist.
    #[must_use]
    pub fn trades(&self) -> &[String] {
        &self.trades
    }
}

/// FEE-010, D-152, D-195, D-3779: the business's join facts, or `None` when it has no sign-up
/// row. Read-only.
///
/// # Errors
/// [`BusinessesError::Database`] when the read fails.
pub async fn join_facts(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<Option<JoinFacts>, BusinessesError> {
    let row = db(sqlx::query!(
        "SELECT submitted_at, trades FROM businesses.signup WHERE business_id = $1",
        business
    )
    .fetch_optional(conn)
    .await)?;
    Ok(row.map(|r| JoinFacts {
        joined: r.submitted_at.to_jiff(),
        trades: r.trades,
    }))
}
