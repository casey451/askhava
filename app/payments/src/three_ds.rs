//! T405: whether each charged pick's payment was 3-D Secure authenticated, read from Stripe once
//! and kept in `payments.charge_three_ds` (FEE-050, FEE-027; D-2204, D-3118).
//!
//! FEE-050 auto-submits a dispute's evidence only with a recorded release and a call attempt or a
//! 3DS-authenticated payment. [`record_three_ds`] asks a [`ThreeDsStripe`] (the adapter's
//! `StripePayments::fetch_three_ds`, or a test fake) about the pick's PaymentIntent and stores the
//! answer once; [`three_ds_authenticated`] reads it, and anything but a stored `true` is `false`.
//! T314's dispute handler records it, then reads it. Only the pick ID and the boolean are stored:
//! no card data, cardholder name or homeowner field, and nothing here logs.

use std::future::Future;

use askhava_adapters::api::{AdapterError, StripePayments, ThreeDs};
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::MoneyEventError;

/// FEE-050, D-3118: where a PaymentIntent's 3-D Secure result comes from.
pub trait ThreeDsStripe {
    /// Whether the PaymentIntent `pi_…` was paid with 3-D Secure authentication.
    fn three_ds(&self, intent: &str) -> impl Future<Output = Result<ThreeDs, AdapterError>> + Send;
}

impl ThreeDsStripe for StripePayments {
    async fn three_ds(&self, intent: &str) -> Result<ThreeDs, AdapterError> {
        self.fetch_three_ds(intent).await
    }
}

/// FEE-050, FEE-027, D-3118: reads `pick`'s PaymentIntent from its `payments.charge_evidence`
/// row, asks `source` for its 3-D Secure result and stores it once in `payments.charge_three_ds`.
/// A pick already recorded is not asked again and keeps its row. Call it outside a transaction:
/// the Stripe read is a network call (rust brain R11).
///
/// # Errors
/// [`MoneyEventError::NothingCharged`] for a pick with no charge evidence and
/// [`MoneyEventError::Stripe`] for a failed read, both storing nothing;
/// [`MoneyEventError::Database`].
pub async fn record_three_ds(
    conn: &mut PgConnection,
    source: &impl ThreeDsStripe,
    pick: Uuid,
) -> Result<(), MoneyEventError> {
    let row = sqlx::query!(
        r#"SELECT e.stripe_intent_id,
                  EXISTS (SELECT 1 FROM payments.charge_three_ds t WHERE t.pick_id = e.pick_id)
                    AS "recorded!"
           FROM payments.charge_evidence e WHERE e.pick_id = $1"#,
        pick,
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(|_| MoneyEventError::Database)?
    .ok_or(MoneyEventError::NothingCharged)?;
    if row.recorded {
        return Ok(());
    }
    let read = source
        .three_ds(&row.stripe_intent_id)
        .await
        .map_err(|_| MoneyEventError::Stripe)?;
    sqlx::query!(
        "INSERT INTO payments.charge_three_ds (pick_id, authenticated) VALUES ($1, $2) \
         ON CONFLICT (pick_id) DO NOTHING",
        pick,
        read == ThreeDs::Authenticated,
    )
    .execute(conn)
    .await
    .map_err(|_| MoneyEventError::Database)?;
    Ok(())
}

/// FEE-050, D-3118: `true` only when `pick` has a stored `true` row; no row (unknown) is `false`,
/// never authenticated.
///
/// # Errors
/// [`MoneyEventError::Database`].
pub async fn three_ds_authenticated(
    conn: &mut PgConnection,
    pick: Uuid,
) -> Result<bool, MoneyEventError> {
    let stored = sqlx::query_scalar!(
        "SELECT authenticated FROM payments.charge_three_ds WHERE pick_id = $1",
        pick,
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| MoneyEventError::Database)?;
    Ok(stored == Some(true))
}
