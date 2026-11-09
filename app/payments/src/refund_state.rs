//! T270: where a pick's refund stands, read from the authoritative `payments.refund` row and the
//! pick's disputes (FEE-022, FEE-026, FEE-052, D-1819, D-1836, D-2145).
//!
//! `request_refund` answers `RefundOutcome::Unchanged` both when a pick is refunded already and
//! when a try is still waiting for Stripe, so a caller that closes a job or tells a business "your
//! fee is refunded" reads [`refund_state`] instead and acts only on
//! [`PickRefundState::Succeeded`]. The type is named `PickRefundState` because `api` already
//! exports T258's `RefundState` (one Stripe Refund object's status, D-2145).

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::MoneyEventError;
use crate::status::RefundStatus;

/// Where a pick's one refund stands (FEE-026, D-1836); `RefundStatus::pick_state` maps a stored
/// status to it (every match on a status lives in `status.rs`, SEC-031).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickRefundState {
    /// No refund was ever asked for this pick, and its charge is not disputed.
    NoRefund,
    /// A try went to Stripe with no answer yet (`sending`), or Stripe holds it `pending`.
    InFlight,
    /// Stripe said `succeeded`: the fee is back with the business.
    Succeeded,
    /// Stripe refused, failed or canceled it (`failed`, `canceled`): the hourly retry sends again.
    Failed,
    /// The charge is disputed, so no refund goes outside the dispute (FEE-026).
    Blocked,
}

/// FEE-022, FEE-026, FEE-052, D-1836: reads where `pick`'s refund stands from its
/// `payments.refund` row and whether its charge has a `payments.dispute` row. Only
/// [`PickRefundState::Succeeded`] means refunded; a stored status this build cannot read (the
/// table's check allows none) is `Failed`, never a success.
///
/// # Errors
/// [`MoneyEventError::Database`].
pub async fn refund_state(
    conn: &mut PgConnection,
    pick: Uuid,
) -> Result<PickRefundState, MoneyEventError> {
    let row = sqlx::query!(
        r#"SELECT (SELECT status FROM payments.refund WHERE pick_id = $1) AS status,
                  EXISTS (SELECT 1 FROM payments.dispute WHERE pick_id = $1) AS "disputed!""#,
        pick,
    )
    .fetch_one(conn)
    .await
    .map_err(|_| MoneyEventError::Database)?;
    let stored = row.status.as_deref().map(RefundStatus::from_stored);
    Ok(stored.map_or_else(
        || RefundStatus::pick_state(None, row.disputed),
        |known| {
            known.map_or(PickRefundState::Failed, |s| {
                RefundStatus::pick_state(Some(s), row.disputed)
            })
        },
    ))
}
