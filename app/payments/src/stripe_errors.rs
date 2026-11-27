//! The outcomes of `payments`' recorded Stripe calls in a window, for the Stripe-or-webhook-errors
//! alert (T518; OPS-004, OPS-023, D-3877, D-4280, D-5670).
//!
//! [`stripe_calls`] reads, in one query, `payments.charge_attempt`
//! (`migrations/0096_payments_charge.sql`) and `payments.refund`
//! (`migrations/0102_payments_refunds_disputes.sql`) and returns each recorded call's time and
//! whether it failed, oldest first: times and outcomes only, never an amount, a Stripe ID, a pick
//! or a business (PRIV-005).
//!
//! A failed call is a charge attempt stored `unreachable` (at its `decided_at`, else its start) or
//! still `sent`, Stripe's answer unknown (at its start), or a refund stored `failed` (at its
//! `decided_at`, else its request). A successful call is a charge attempt with `decided_at` set and
//! status `succeeded`, `declined`, `needs_action` or `processing`, or a refund `succeeded` at its
//! `decided_at` (D-5670). Calls that leave no row are not seen: a customer or intent creation, the
//! 4-hour timer's read that settles nothing and the nightly reconcile's reads. Rows are judged by
//! their current status, so an attempt later decided again moves to its new time and outcome.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;

/// Why the Stripe-call read failed. No variant carries a value (R5, R6).
#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
pub enum StripeCallError {
    /// The database refused or dropped the statement.
    #[error("the payments Stripe-call read failed")]
    Database,
}

/// One recorded Stripe call: when it was decided and whether it failed (OPS-004, D-5670).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripeCall {
    /// When the call's outcome was recorded.
    pub at: Timestamp,
    /// True for an unreachable or unanswered charge attempt or a failed refund.
    pub failed: bool,
}

/// OPS-004, D-3877, D-5670: every recorded Stripe call timed at or after `from` and before `to`,
/// oldest first (a failure before a success at the same time).
///
/// # Errors
///
/// [`StripeCallError::Database`] when the rows cannot be read.
pub async fn stripe_calls(
    db: &mut PgConnection,
    from: Timestamp,
    to: Timestamp,
) -> Result<Vec<StripeCall>, StripeCallError> {
    let rows = sqlx::query!(
        r#"SELECT at AS "at!", failed AS "failed!" FROM (
             SELECT coalesce(decided_at, started_at) AS at, status IN ('unreachable', 'sent') AS failed
               FROM payments.charge_attempt
              WHERE status IN ('unreachable', 'sent')
                 OR (decided_at IS NOT NULL
                     AND status IN ('succeeded', 'declined', 'needs_action', 'processing'))
             UNION ALL
             SELECT coalesce(decided_at, requested_at), status = 'failed'
               FROM payments.refund
              WHERE status = 'failed' OR (status = 'succeeded' AND decided_at IS NOT NULL)
           ) calls
           WHERE at >= $1 AND at < $2
           ORDER BY at, failed DESC"#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_all(db)
    .await
    .map_err(|_| StripeCallError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| StripeCall {
            at: r.at.to_jiff(),
            failed: r.failed,
        })
        .collect())
}
