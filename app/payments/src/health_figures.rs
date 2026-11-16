//! The payment-error figures of the owner health page (OPS-008; T475, D-3753, D-3877).
//!
//! [`payment_errors`] counts, in one query, for a window `[from, to)`: the charge attempts started
//! in it that failed (`declined`, `needs_action` or `unreachable`, decided before `to`), those
//! still undecided at `to` (no `decided_at`, or one at or after `to`), and the refunds requested
//! in it that failed or Stripe canceled, from `payments.charge_attempt`
//! (`migrations/0096_payments_charge.sql`) and `payments.refund`
//! (`migrations/0102_payments_refunds_disputes.sql`). Counts only, never an amount, a Stripe ID or
//! a business (PRIV-005).
//!
//! Exact only for a window that ends now, which is how T476 calls it. A charge is judged by its
//! latest decision: `status.rs` `may_become` lets a final status be replaced (`declined` by
//! `succeeded`, for one), rewriting `decided_at`, so an attempt decided again after `to` counts in
//! that window as undecided, no longer as failed. A refund is judged by its current status, so one
//! that fails after `to` joins an earlier window and one the hourly retry later turns into a
//! success leaves it.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;

/// Why a health-figure read failed. No variant carries a value (R5, R6).
#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
pub enum HealthFigureError {
    /// The database refused or dropped the statement.
    #[error("the payments health-figure read failed")]
    Database,
}

/// The payment errors of one window (OPS-008, D-3877).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaymentErrors {
    /// Charge attempts started in the window and, before its end, declined, needing action or
    /// unreachable.
    pub failed_charges: i64,
    /// Charge attempts started in the window and not decided by its end (`decided_at` unset or at
    /// or after `to`); for a window ending now, one sent seconds ago counts too.
    pub undecided_charges: i64,
    /// Refunds requested in the window that are `failed` or `canceled` (both the failure path,
    /// `migrations/0124_payments_refunds_and_events.sql`).
    pub failed_refunds: i64,
}

/// OPS-008, D-3877: the [`PaymentErrors`] of `[from, to)`.
///
/// # Errors
///
/// [`HealthFigureError::Database`] when the counts cannot be read.
pub async fn payment_errors(
    db: &mut PgConnection,
    from: Timestamp,
    to: Timestamp,
) -> Result<PaymentErrors, HealthFigureError> {
    let r = sqlx::query!(
        r#"SELECT
             (SELECT count(*) FROM payments.charge_attempt
               WHERE started_at >= $1 AND started_at < $2
                 AND status IN ('declined', 'needs_action', 'unreachable')
                 AND decided_at < $2) AS "failed!",
             (SELECT count(*) FROM payments.charge_attempt
               WHERE started_at >= $1 AND started_at < $2
                 AND (decided_at IS NULL OR decided_at >= $2)) AS "undecided!",
             (SELECT count(*) FROM payments.refund
               WHERE requested_at >= $1 AND requested_at < $2
                 AND status IN ('failed', 'canceled')) AS "refunds!""#,
        from.to_sqlx() as _,
        to.to_sqlx() as _,
    )
    .fetch_one(db)
    .await
    .map_err(|_| HealthFigureError::Database)?;
    Ok(PaymentErrors {
        failed_charges: r.failed,
        undecided_charges: r.undecided,
        failed_refunds: r.refunds,
    })
}
