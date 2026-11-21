//! The pilot scorecard's money inputs (OPS-039, OPS-047, FEE-046; D-458, D-3753, D-3877, D-3920),
//! read-only, from `payments` tables only.
//!
//! No `payments` table stores a trade, so every reader takes the job IDs of one trade (or of all)
//! from `quotes::api::jobs_by_trade` and a window, at or after `from` and before `to`, on the
//! row's own money time (`ledger_entry.recorded_at`, `refund.requested_at`). Counts, sums in cents
//! and shares only; a window with no matching row gives `None`. The one exception is
//! [`charge_outcomes`], the refund and dispute rates as a cohort (D-5081): of the charges recorded
//! in the window, those whose job and business have a refund or dispute entry recorded by now, so
//! no rate passes 100% and a recent window's rate can still rise.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::MoneyEventError;

const DB: MoneyEventError = MoneyEventError::Database;

/// The ledger's money totals for one trade and window (OPS-039).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerTotals {
    /// Paid introductions: `charge` entries.
    pub paid_introductions: i64,
    /// The sum of those introduction fees by job size, in cents.
    pub fees_cents: i64,
    /// Stripe's `processing-fee` entries, in cents.
    pub stripe_fees_cents: i64,
    /// `refund` entries.
    pub refunds: i64,
    /// Refunded cents, less any `refund-reversal`.
    pub refunded_cents: i64,
    /// Bank disputes: `dispute` entries.
    pub disputes: i64,
}

impl LedgerTotals {
    /// OPS-039: the average fee per paid introduction in cents (rounded down); `None` with none.
    #[must_use]
    pub fn average_fee_cents(&self) -> Option<i64> {
        (self.paid_introductions > 0).then(|| self.fees_cents / self.paid_introductions)
    }

    /// OPS-039, FEE-046, D-458: net per paid introduction in cents: the fees less the sales-tax
    /// set-aside at `set_aside_basis_points` (`Settings::sales_tax_set_aside_basis_points`, the
    /// set-aside rounded up to the cent), Stripe's fees and refunds, over the paid introductions
    /// (rounded toward zero); `None` with none.
    #[must_use]
    pub fn net_per_introduction_cents(&self, set_aside_basis_points: u16) -> Option<i64> {
        let set_aside = (self.fees_cents * i64::from(set_aside_basis_points) + 9_999) / 10_000;
        let net = self.fees_cents - set_aside - self.stripe_fees_cents - self.refunded_cents;
        (self.paid_introductions > 0).then(|| net / self.paid_introductions)
    }
}

/// OPS-039, FEE-046: [`LedgerTotals`] of `jobs` in the window; `None` when it has no charge,
/// Stripe fee, refund or dispute entry.
///
/// # Errors
///
/// [`MoneyEventError::Database`].
pub async fn ledger_totals(
    db: &mut PgConnection,
    jobs: &[Uuid],
    from: Timestamp,
    to: Timestamp,
) -> Result<Option<LedgerTotals>, MoneyEventError> {
    let r = sqlx::query!(
        "SELECT count(*) AS \"rows!\", \
           count(*) FILTER (WHERE kind = 'charge') AS \"paid!\", \
           COALESCE(sum(amount_cents) FILTER (WHERE kind = 'charge'), 0)::bigint AS \"fees!\", \
           COALESCE(sum(amount_cents) FILTER (WHERE kind = 'processing-fee'), 0)::bigint \
             AS \"stripe!\", \
           count(*) FILTER (WHERE kind = 'refund') AS \"refunds!\", \
           COALESCE(sum(CASE kind WHEN 'refund' THEN amount_cents \
             WHEN 'refund-reversal' THEN -amount_cents END), 0)::bigint AS \"refunded!\", \
           count(*) FILTER (WHERE kind = 'dispute') AS \"disputes!\" \
         FROM payments.ledger_entry WHERE job_id = ANY($1) AND recorded_at >= $2 \
           AND recorded_at < $3 \
           AND kind IN ('charge', 'processing-fee', 'refund', 'refund-reversal', 'dispute')",
        jobs,
        from.to_sqlx(),
        to.to_sqlx()
    )
    .fetch_one(db)
    .await
    .map_err(|_| DB)?;
    Ok((r.rows > 0).then_some(LedgerTotals {
        paid_introductions: r.paid,
        fees_cents: r.fees,
        stripe_fees_cents: r.stripe,
        refunds: r.refunds,
        refunded_cents: r.refunded,
        disputes: r.disputes,
    }))
}

/// OPS-039, D-5081: what has become of one window's charges by now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChargeOutcomes {
    /// `charge` entries recorded in the window.
    pub charges: i64,
    /// Of those, the ones whose job and business have a `refund` entry recorded at any time.
    pub refunded: i64,
    /// Of those, the ones whose job and business have a `dispute` entry recorded at any time.
    pub disputed: i64,
}

/// OPS-039, D-5081: [`ChargeOutcomes`] of `jobs`' charges recorded in the window; a refund or
/// dispute counts against its charge's window, never its own, so neither passes `charges`.
///
/// # Errors
///
/// [`MoneyEventError::Database`].
pub async fn charge_outcomes(
    db: &mut PgConnection,
    jobs: &[Uuid],
    from: Timestamp,
    to: Timestamp,
) -> Result<ChargeOutcomes, MoneyEventError> {
    let r = sqlx::query!(
        "SELECT count(*) AS \"charges!\", \
           count(*) FILTER (WHERE EXISTS (SELECT 1 FROM payments.ledger_entry r \
             WHERE r.kind = 'refund' AND r.job_id = e.job_id \
             AND r.business_id = e.business_id)) AS \"refunded!\", \
           count(*) FILTER (WHERE EXISTS (SELECT 1 FROM payments.ledger_entry d \
             WHERE d.kind = 'dispute' AND d.job_id = e.job_id \
             AND d.business_id = e.business_id)) AS \"disputed!\" \
         FROM payments.ledger_entry e WHERE e.kind = 'charge' AND e.job_id = ANY($1) \
           AND e.recorded_at >= $2 AND e.recorded_at < $3",
        jobs,
        from.to_sqlx(),
        to.to_sqlx()
    )
    .fetch_one(db)
    .await
    .map_err(|_| DB)?;
    Ok(ChargeOutcomes {
        charges: r.charges,
        refunded: r.refunded,
        disputed: r.disputed,
    })
}

/// A part of a whole, both counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Share {
    /// The rows counted.
    pub part: i64,
    /// All rows they were counted among.
    pub whole: i64,
}

/// OPS-039, D-458: the free share of introductions, counted only on picks after each business's
/// first win: of the introductions (`charge`, `free-first-win`, `repeat-customer`,
/// `fee-free-job-type` entries) of `jobs` in the window that are not their business's first
/// (its earliest introduction entry ever), the part that was $0. `None` with none.
///
/// # Errors
///
/// [`MoneyEventError::Database`].
pub async fn free_share(
    db: &mut PgConnection,
    jobs: &[Uuid],
    from: Timestamp,
    to: Timestamp,
) -> Result<Option<Share>, MoneyEventError> {
    let r = sqlx::query!(
        "WITH intro AS (SELECT kind, job_id, recorded_at, row_number() OVER \
           (PARTITION BY business_id ORDER BY recorded_at, seq) AS nth \
           FROM payments.ledger_entry WHERE kind IN \
           ('charge', 'free-first-win', 'repeat-customer', 'fee-free-job-type')) \
         SELECT count(*) FILTER (WHERE kind <> 'charge') AS \"part!\", count(*) AS \"whole!\" \
         FROM intro WHERE nth > 1 AND job_id = ANY($1) AND recorded_at >= $2 \
           AND recorded_at < $3",
        jobs,
        from.to_sqlx(),
        to.to_sqlx()
    )
    .fetch_one(db)
    .await
    .map_err(|_| DB)?;
    Ok((r.whole > 0).then_some(Share {
        part: r.part,
        whole: r.whole,
    }))
}

/// OPS-047, D-490: refund requests of `jobs` asked in the window, counted by reason (the
/// `payments.refund` trigger code name), by reason name; `None` with none.
///
/// # Errors
///
/// [`MoneyEventError::Database`].
pub async fn refund_reasons(
    db: &mut PgConnection,
    jobs: &[Uuid],
    from: Timestamp,
    to: Timestamp,
) -> Result<Option<Vec<(String, i64)>>, MoneyEventError> {
    let rows = sqlx::query!(
        "SELECT r.trigger AS reason, count(*) AS \"n!\" FROM payments.refund r \
         JOIN payments.charge_evidence c USING (pick_id) WHERE c.job_id = ANY($1) \
           AND r.requested_at >= $2 AND r.requested_at < $3 GROUP BY r.trigger ORDER BY r.trigger",
        jobs,
        from.to_sqlx(),
        to.to_sqlx()
    )
    .fetch_all(db)
    .await
    .map_err(|_| DB)?;
    let reasons: Vec<_> = rows.into_iter().map(|r| (r.reason, r.n)).collect();
    Ok((!reasons.is_empty()).then_some(reasons))
}

/// OPS-039: of the businesses with a paid introduction on `jobs` in the window (`whole`), those
/// that had paid for another introduction in the 60 days before it (`part`); `None` with none.
///
/// # Errors
///
/// [`MoneyEventError::Database`].
pub async fn paying_again(
    db: &mut PgConnection,
    jobs: &[Uuid],
    from: Timestamp,
    to: Timestamp,
) -> Result<Option<Share>, MoneyEventError> {
    let r = sqlx::query!(
        "WITH paid AS (SELECT e.business_id, EXISTS (SELECT 1 FROM payments.ledger_entry p \
           WHERE p.kind = 'charge' AND p.business_id = e.business_id AND p.id <> e.id \
             AND p.recorded_at <= e.recorded_at \
             AND p.recorded_at >= e.recorded_at - interval '60 days') AS again \
           FROM payments.ledger_entry e WHERE e.kind = 'charge' AND e.job_id = ANY($1) \
             AND e.recorded_at >= $2 AND e.recorded_at < $3) \
         SELECT count(DISTINCT business_id) FILTER (WHERE again) AS \"part!\", \
           count(DISTINCT business_id) AS \"whole!\" FROM paid",
        jobs,
        from.to_sqlx(),
        to.to_sqlx()
    )
    .fetch_one(db)
    .await
    .map_err(|_| DB)?;
    Ok((r.whole > 0).then_some(Share {
        part: r.part,
        whole: r.whole,
    }))
}
