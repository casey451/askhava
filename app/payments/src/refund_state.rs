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

/// Where a pick's one refund stands (FEE-026, D-1836).
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

impl PickRefundState {
    /// FEE-026: the state of a stored refund `status` (`None`: no row) on a charge that is or is
    /// not `disputed`, judged as `RefundStatus::next` judges a new send: a dispute blocks every
    /// status that would still send (none, `sending`, `failed`, `canceled`); `pending` stays in
    /// flight, since Stripe holds that refund and may still finish it.
    #[must_use]
    pub const fn of(status: Option<RefundStatus>, disputed: bool) -> Self {
        match status {
            Some(RefundStatus::Succeeded) => Self::Succeeded,
            Some(RefundStatus::Blocked) => Self::Blocked,
            Some(RefundStatus::Pending) => Self::InFlight,
            None | Some(RefundStatus::Sending | RefundStatus::Failed | RefundStatus::Canceled)
                if disputed =>
            {
                Self::Blocked
            }
            None => Self::NoRefund,
            Some(RefundStatus::Sending) => Self::InFlight,
            Some(RefundStatus::Failed | RefundStatus::Canceled) => Self::Failed,
        }
    }
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
        || PickRefundState::of(None, row.disputed),
        |known| {
            known.map_or(PickRefundState::Failed, |s| {
                PickRefundState::of(Some(s), row.disputed)
            })
        },
    ))
}
