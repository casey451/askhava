//! The receipt of each charge: the IDs-only receipt event and the facts the "Why was I charged?"
//! page reads (FEE-031, FEE-053, D-225, D-491, D-492).
//!
//! `payments` cannot call `messaging` or `businesses`, so [`queue_receipt`] raises one
//! [`RECEIPT_KIND`] `core.task` per charged pick, carrying the pick, business and job IDs only, in
//! the caller's transaction; `picks` (`money_notices.rs`) resolves the recipients and asks
//! `messaging` for the email. [`receipt_for`] reads one charge, for its business only, from the
//! FEE-042 evidence record: one pick, one charge, one receipt line (FEE-053).

use askhava_core::api::{NewTask, enqueue};
use jiff::Timestamp;
use serde_json::json;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::MoneyEventError;
use crate::rules::Tier;

/// The `core.task` kind of a charge's receipt event (FEE-031, D-492); `picks` handles it.
pub const RECEIPT_KIND: &str = "payments.receipt";

/// One charge as its receipt and its "Why was I charged?" page show it (FEE-031, FEE-053).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChargeReceipt {
    /// The pick the charge is for: one pick, one charge (FEE-053).
    pub pick: Uuid,
    /// The business charged.
    pub business: Uuid,
    /// The job the pick belongs to.
    pub job: Uuid,
    /// The introduction fee by job size charged, in cents, tax included (FEE-008).
    pub fee_cents: u64,
    /// The tier of that fee (FEE-001).
    pub tier: Tier,
    /// When Stripe took the charge.
    pub charged_at: Timestamp,
}

/// The tier whose fee is exactly `cents`; any other amount is no tier (FEE-001).
#[must_use]
pub fn tier_of_fee(cents: u64) -> Option<Tier> {
    [Tier::Small, Tier::Medium, Tier::Large]
        .into_iter()
        .find(|tier| tier.cents() == cents)
}

/// FEE-031, D-492: queues the receipt event of pick `pick` at `at` once, in the caller's
/// transaction, when the pick has a charge evidence record (a succeeded charge, FEE-042); the
/// dedupe key is the pick ID, so a second call queues nothing (JOB-006). Answers whether it queued.
///
/// # Errors
/// [`MoneyEventError::NothingCharged`] for a pick with no charge; [`MoneyEventError::Database`].
pub async fn queue_receipt(
    conn: &mut PgConnection,
    pick: Uuid,
    at: Timestamp,
) -> Result<bool, MoneyEventError> {
    let row = sqlx::query!(
        "SELECT business_id, job_id FROM payments.charge_evidence WHERE pick_id = $1",
        pick
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| MoneyEventError::Database)?
    .ok_or(MoneyEventError::NothingCharged)?;
    let payload = json!({"pick": pick, "business": row.business_id, "job": row.job_id});
    let mut task = NewTask::new(RECEIPT_KIND, payload, at);
    task.dedupe_key = Some(format!("{RECEIPT_KIND}.{pick}"));
    let queued = enqueue(conn, &task)
        .await
        .map_err(|_| MoneyEventError::Database)?;
    Ok(queued.is_some())
}

/// FEE-031, D-225: the charge of pick `pick` for `business`, or `None` when that business was not
/// charged for it (a page asked for another business's pick shows nothing).
///
/// # Errors
/// [`MoneyEventError::Database`]; a stored amount that is no tier is
/// [`MoneyEventError::UnknownCharge`].
pub async fn receipt_for(
    conn: &mut PgConnection,
    pick: Uuid,
    business: Uuid,
) -> Result<Option<ChargeReceipt>, MoneyEventError> {
    let row = sqlx::query!(
        "SELECT job_id, amount_cents, charged_at FROM payments.charge_evidence \
         WHERE pick_id = $1 AND business_id = $2",
        pick,
        business
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| MoneyEventError::Database)?;
    let Some(row) = row else { return Ok(None) };
    let fee_cents = u64::try_from(row.amount_cents).map_err(|_| MoneyEventError::UnknownCharge)?;
    let tier = tier_of_fee(fee_cents).ok_or(MoneyEventError::UnknownCharge)?;
    Ok(Some(ChargeReceipt {
        pick,
        business,
        job: row.job_id,
        fee_cents,
        tier,
        charged_at: row.charged_at.to_jiff(),
    }))
}
