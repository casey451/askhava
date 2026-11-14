//! One business's saved cards for the portal's billing page (BIZ-015, FEE-014; D-1912; T451).
//!
//! [`saved_cards`] lists the business's saved cards not removed, oldest first, as brand and last
//! four only. The Stripe PaymentMethod ID (`pm_…`), the customer ID and every other card fact stay
//! in `payments`: [`CardOnFile`] has no field that could carry them (FEE-014).

use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::MoneyEventError;

/// A saved card as the business sees it: brand and last four only (FEE-014).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardOnFile {
    /// The card brand, such as `visa`.
    pub brand: String,
    /// The card's last four digits.
    pub last4: String,
}

/// BIZ-015, FEE-014: `business`'s saved cards not removed, oldest first, brand and last four only.
///
/// # Errors
///
/// [`MoneyEventError::Database`].
pub async fn saved_cards(
    db: &mut PgConnection,
    business: Uuid,
) -> Result<Vec<CardOnFile>, MoneyEventError> {
    let rows = sqlx::query!(
        "SELECT brand, last4 FROM payments.payment_method
          WHERE business_id = $1 AND removed_at IS NULL ORDER BY saved_at, last4",
        business,
    )
    .fetch_all(db)
    .await
    .map_err(|_| MoneyEventError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| CardOnFile {
            brand: r.brand,
            last4: r.last4,
        })
        .collect())
}
