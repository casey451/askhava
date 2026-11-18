//! T405: whether each charged pick's payment was 3-D Secure authenticated, read from Stripe once
//! and kept in `payments.charge_three_ds` (FEE-050, FEE-027; D-2204, D-3118).
//!
//! FEE-050 auto-submits a dispute's evidence only with a recorded release and a call attempt or a
//! 3DS-authenticated payment. [`record_three_ds`] asks a [`ThreeDsStripe`] (the adapter's
//! `StripePayments::fetch_three_ds`, or a test fake) about the pick's PaymentIntent and stores the
//! answer once; [`three_ds_authenticated`] reads it, and anything but a stored `true` is `false`.
//! T314's dispute handler records it through [`record_three_ds_pool`], which holds no pooled
//! connection across the Stripe read (D-4052), then reads it. Only the pick ID and the boolean are stored:
//! no card data, cardholder name or homeowner field, and nothing here logs.

use std::future::Future;

use askhava_adapters::api::{AdapterError, StripePayments, ThreeDs};
use sqlx::types::Uuid;
use sqlx::{PgConnection, PgPool};

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
/// row, asks `source` for its 3-D Secure result and stores it once in `payments.charge_three_ds`,
/// all on `conn`. A pick already recorded is not asked again and keeps its row. `conn` is held
/// across the Stripe read (a network call, rust brain R11), so use it only on a connection that
/// holds no other work and outside a transaction; a handler with a pool calls
/// [`record_three_ds_pool`], which holds none across it (D-4052).
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
    let Some(intent) = intent_to_ask(conn, pick).await? else {
        return Ok(());
    };
    let read = ask(source, &intent).await?;
    store_three_ds(conn, pick, read).await
}

/// FEE-050, FEE-027, D-3118, D-4052: [`record_three_ds`] with no connection held across Stripe
/// (rust brain R11): it reads the PaymentIntent on one connection from `pool` and releases it,
/// asks `source`, then stores the answer on a fresh one (`ON CONFLICT DO NOTHING`, so a racing
/// run keeps the first row). A pick already recorded is not asked again.
///
/// # Errors
/// As [`record_three_ds`]; a connection the pool cannot give is [`MoneyEventError::Database`].
pub async fn record_three_ds_pool(
    pool: &PgPool,
    source: &impl ThreeDsStripe,
    pick: Uuid,
) -> Result<(), MoneyEventError> {
    let intent = {
        let mut conn = pool
            .acquire()
            .await
            .map_err(|_| MoneyEventError::Database)?;
        intent_to_ask(&mut conn, pick).await?
    };
    let Some(intent) = intent else {
        return Ok(());
    };
    let read = ask(source, &intent).await?;
    let mut conn = pool
        .acquire()
        .await
        .map_err(|_| MoneyEventError::Database)?;
    store_three_ds(&mut conn, pick, read).await
}

/// FEE-050: `pick`'s PaymentIntent when its 3-D Secure result is not stored yet; `None` when it is.
async fn intent_to_ask(
    conn: &mut PgConnection,
    pick: Uuid,
) -> Result<Option<String>, MoneyEventError> {
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
    Ok((!row.recorded).then_some(row.stripe_intent_id))
}

/// FEE-050: the 3-D Secure result of PaymentIntent `intent`; a failed read is `Stripe`.
async fn ask(source: &impl ThreeDsStripe, intent: &str) -> Result<ThreeDs, MoneyEventError> {
    source
        .three_ds(intent)
        .await
        .map_err(|_| MoneyEventError::Stripe)
}

/// FEE-050: stores `read` for `pick` once; an existing row is kept.
async fn store_three_ds(
    conn: &mut PgConnection,
    pick: Uuid,
    read: ThreeDs,
) -> Result<(), MoneyEventError> {
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
