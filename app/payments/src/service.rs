//! Stripe customers and saved cards: the steps that touch Stripe and `payments`' tables (FEE-009,
//! FEE-014, FEE-015, FEE-036, D-089, D-365).
//!
//! [`ensure_customer`] runs at business sign-up: it creates the business's Stripe customer with no
//! card, once, and stores its ID (FEE-036). [`save_card`] records a card the business saved through
//! Stripe Elements, with the FEE-015 authorization text, version and time, in one transaction; it
//! charges nothing. Neither takes a homeowner, and nothing in this file creates a charge (FEE-009,
//! FEE-014): charges are the Confirm-and-pay task's (T062). No transaction stays open across the
//! Stripe call (R11).

use askhava_adapters::api::{IdempotencyKey, PaymentsAdapter};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::{Connection, PgConnection, PgPool};

use crate::models::{
    AUTHORIZATION_TEXT, AUTHORIZATION_VERSION, BusinessId, CardError, CustomerId, SavedCard,
};

/// The idempotency action for a business's Stripe customer (SEC-018): the key is derived from the
/// business ID only, so a repeated sign-up step makes one customer.
pub const CUSTOMER_ACTION: &str = "payments.customer";

/// The business's stored Stripe customer, if it has one.
///
/// # Errors
/// [`CardError::Database`] when the query fails; [`CardError::BadField`] for a stored ID of the
/// wrong form.
pub async fn customer_of(
    conn: &mut PgConnection,
    business: BusinessId,
) -> Result<Option<CustomerId>, CardError> {
    let id = sqlx::query_scalar!(
        "SELECT stripe_customer_id FROM payments.customer WHERE business_id = $1",
        business.0,
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| CardError::Database)?;
    id.as_deref().map(CustomerId::parse).transpose()
}

/// Creates the business's Stripe customer at sign-up, with no card, or returns the one it has
/// (FEE-036, D-089). The customer names only the business record (FEE-009); the key is derived
/// from the business ID, so a retry after a lost answer reaches the same customer (SEC-018).
///
/// # Errors
/// [`CardError::Stripe`] when Stripe fails (nothing stored); [`CardError::BadField`] for an
/// answer that is not `cus_…`; [`CardError::Database`] when a statement fails.
pub async fn ensure_customer(
    pool: &PgPool,
    stripe: &impl PaymentsAdapter,
    id_secret: &SecretString,
    business: BusinessId,
) -> Result<CustomerId, CardError> {
    let mut conn = pool.acquire().await.map_err(|_| CardError::Database)?;
    if let Some(found) = customer_of(&mut conn, business).await? {
        return Ok(found);
    }
    let id_text = business.0.to_string();
    let key = IdempotencyKey::derive(id_secret, CUSTOMER_ACTION, &[&id_text]);
    let created = stripe
        .create_customer(&key, &id_text)
        .await
        .map_err(|_| CardError::Stripe)?;
    let customer = CustomerId::parse(&created)?;
    sqlx::query!(
        "INSERT INTO payments.customer (business_id, stripe_customer_id) VALUES ($1, $2)
         ON CONFLICT (business_id) DO NOTHING",
        business.0,
        customer.as_str(),
    )
    .execute(&mut *conn)
    .await
    .map_err(|_| CardError::Database)?;
    customer_of(&mut conn, business)
        .await?
        .ok_or(CardError::Database)
}

/// What [`save_card`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saved {
    /// The card and a new authorization acceptance were stored.
    New,
    /// The card was already saved for this business (a repeated webhook or page post).
    Already,
}

/// Records a card the business saved through Stripe Elements, with its acceptance of the FEE-015
/// authorization (text, version, `accepted_at`) in the same transaction; charges nothing
/// (FEE-014, FEE-015, FEE-036, FEE-048). The card is stored with `usage` `off_session`, as the
/// SetupIntent made it.
///
/// # Errors
/// [`CardError::NoCustomer`] when the business has no Stripe customer;
/// [`CardError::OtherBusiness`] when the card is another business's; [`CardError::Database`] when
/// a statement fails.
pub async fn save_card(
    conn: &mut PgConnection,
    business: BusinessId,
    card: &SavedCard,
    accepted_at: Timestamp,
) -> Result<Saved, CardError> {
    let mut tx = conn.begin().await.map_err(|_| CardError::Database)?;
    if customer_of(&mut tx, business).await?.is_none() {
        return Err(CardError::NoCustomer);
    }
    let owner = sqlx::query_scalar!(
        "SELECT business_id FROM payments.payment_method WHERE stripe_payment_method_id = $1",
        card.payment_method(),
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| CardError::Database)?;
    match owner {
        Some(id) if id == business.0 => return Ok(Saved::Already),
        Some(_) => return Err(CardError::OtherBusiness),
        None => {}
    }
    insert_card(&mut tx, business, card, accepted_at).await?;
    tx.commit().await.map_err(|_| CardError::Database)?;
    Ok(Saved::New)
}

/// Inserts the authorization acceptance and then the card that rests on it (FEE-015).
async fn insert_card(
    tx: &mut PgConnection,
    business: BusinessId,
    card: &SavedCard,
    accepted_at: Timestamp,
) -> Result<(), CardError> {
    let authorization = sqlx::query_scalar!(
        "INSERT INTO payments.card_authorization (business_id, version, text, accepted_at)
         VALUES ($1, $2, $3, $4) RETURNING id",
        business.0,
        AUTHORIZATION_VERSION,
        AUTHORIZATION_TEXT,
        accepted_at.to_sqlx() as _,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| CardError::Database)?;
    sqlx::query!(
        "INSERT INTO payments.payment_method (stripe_payment_method_id, business_id,
           authorization_id, method_type, usage, brand, last4, saved_at)
         VALUES ($1, $2, $3, 'card', 'off_session', $4, $5, $6)",
        card.payment_method(),
        business.0,
        authorization,
        card.brand(),
        card.last4(),
        accepted_at.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| CardError::Database)?;
    Ok(())
}
