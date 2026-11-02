//! Stripe customers and saved cards: the steps that touch Stripe and `payments`' tables (FEE-009,
//! FEE-014, FEE-015, FEE-036, D-089, D-365).
//!
//! [`ensure_customer`] runs at business sign-up: it creates the business's Stripe customer with no
//! card, once, and stores its ID (FEE-036). [`save_card`] records a card the business saved through
//! Stripe Elements, with the FEE-015 authorization text, version and time, in one transaction; it
//! charges nothing. Neither takes a homeowner, and nothing in this file creates a charge (FEE-009,
//! FEE-014): charges are the Confirm-and-pay task's (T062). No transaction, and no pooled
//! connection, is held across the Stripe call (R11).
//!
//! T060 (FEE-005 to FEE-007, JOB-016, D-071, D-072): [`switch_fee_mode`] logs the owner's
//! fee-mode switch and returns the new [`FeeTerms`]; [`accept_authorization`] records a business's
//! acceptance of the new authorization version; [`may_confirm`] refuses a Confirm until it has;
//! [`record_fee_at_confirm`] writes the fee fixed at the pick to the ledger when the business
//! confirms, after the charge. No function here takes a final bill or a check-in reply, so a
//! PROBLEM reply about the bill can never change a fee (FEE-006, JOB-016, D-071).

use askhava_adapters::api::{IdempotencyKey, PaymentsAdapter};
use askhava_ops::api::{Actor, AuditEntry, Change, audit};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::ledger::{Appended, EntryKind, LedgerEntry, LedgerError, NewEntry, ledger_append};
use crate::models::{
    AUTHORIZATION_TEXT, AUTHORIZATION_VERSION, BusinessId, CardError, CustomerId, SavedCard,
};
use crate::rules::{Fee, FeeMode, FeeTerms, PickedFee};

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
    // The pooled connection goes back before the Stripe call and is taken again after it (R11).
    drop(conn);
    let id_text = business.0.to_string();
    let key = IdempotencyKey::derive(id_secret, CUSTOMER_ACTION, &[&id_text]);
    let created = stripe
        .create_customer(&key, &id_text)
        .await
        .map_err(|_| CardError::Stripe)?;
    let customer = CustomerId::parse(&created)?;
    let mut conn = pool.acquire().await.map_err(|_| CardError::Database)?;
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

/// The audit action of an owner's fee-mode switch (FEE-007).
pub const FEE_MODE_ACTION: &str = "payments.fee_mode_switched";
/// The audit subject kind of the fee-mode setting (FEE-007).
pub const FEE_MODE_SUBJECT_KIND: &str = "payments.fee_mode";
/// The one fee-mode setting's fixed record ID, the audit rows' subject (FEE-007, PRIV-028).
pub const FEE_MODE_ID: Uuid = Uuid::from_u128(0x0fee_0de0_0000_4000_8000_0000_0000_0007);
/// The idempotency action of the fee entry written at Confirm (SEC-018, JOB-006).
pub const CONFIRM_FEE_ACTION: &str = "payments.confirm_fee";

/// Why a fee-mode switch or a Confirm was refused. No variant carries a value (PRIV-016).
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfirmError {
    /// The business has not accepted the authorization version the fee mode needs (FEE-007).
    #[error("the business must accept the new card authorization first")]
    NewAuthorizationNeeded,
    /// The ledger refused the entry.
    #[error("the ledger refused the fee entry")]
    Ledger(#[from] LedgerError),
    /// The database, or the audit write, failed.
    #[error("the payments database statement failed")]
    Database,
}

/// FEE-007, D-072: switches the fee mode, logging the change with the owner as actor in the
/// caller's transaction (JOB-005), and returns the new terms (the next authorization version) for
/// the caller to store as the owner-portal setting in that same transaction. Choosing the mode in
/// force logs nothing and returns none.
///
/// # Errors
///
/// [`ConfirmError::Database`] when the audit row cannot be written.
pub async fn switch_fee_mode(
    conn: &mut PgConnection,
    owner: Uuid,
    current: FeeTerms,
    to: FeeMode,
) -> Result<Option<FeeTerms>, ConfirmError> {
    let Some(next) = current.switched(to) else {
        return Ok(None);
    };
    let entry = AuditEntry {
        actor: Actor::Owner(owner),
        action: FEE_MODE_ACTION,
        subject_kind: FEE_MODE_SUBJECT_KIND,
        subject_id: FEE_MODE_ID,
        change: Change::Status {
            from: current.mode().code(),
            to: to.code(),
        },
    };
    audit(conn, &entry)
        .await
        .map_err(|_| ConfirmError::Database)?;
    Ok(Some(next))
}

/// FEE-007, FEE-015, D-072: records the business's acceptance of `terms`' authorization version,
/// with the exact text it was shown and the time; the table is append-only.
///
/// # Errors
///
/// [`CardError::NoCustomer`] when the business has no Stripe customer; [`CardError::Database`].
pub async fn accept_authorization(
    conn: &mut PgConnection,
    business: BusinessId,
    terms: FeeTerms,
    text: &str,
    accepted_at: Timestamp,
) -> Result<(), CardError> {
    if customer_of(conn, business).await?.is_none() {
        return Err(CardError::NoCustomer);
    }
    sqlx::query!(
        "INSERT INTO payments.card_authorization (business_id, version, text, accepted_at)
         VALUES ($1, $2, $3, $4)",
        business.0,
        terms.authorization_version(),
        text,
        accepted_at.to_sqlx() as _,
    )
    .execute(conn)
    .await
    .map_err(|_| CardError::Database)?;
    Ok(())
}

/// FEE-007, D-072: a business may confirm, and be charged, only once it has accepted the
/// authorization version `terms` needs; after any fee-mode change it can't confirm until then.
///
/// # Errors
///
/// [`ConfirmError::NewAuthorizationNeeded`]; [`ConfirmError::Database`].
pub async fn may_confirm(
    conn: &mut PgConnection,
    business: BusinessId,
    terms: FeeTerms,
) -> Result<(), ConfirmError> {
    let accepted = sqlx::query_scalar!(
        "SELECT max(version) FROM payments.card_authorization WHERE business_id = $1",
        business.0,
    )
    .fetch_one(conn)
    .await
    .map_err(|_| ConfirmError::Database)?;
    if accepted.is_some_and(|version| version >= terms.authorization_version()) {
        Ok(())
    } else {
        Err(ConfirmError::NewAuthorizationNeeded)
    }
}

/// The business's Confirm of a pick: who, which job and the succeeded charge's `ch_…` ID (none
/// for a $0 fee-free job type).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Confirmed<'a> {
    /// The confirming business.
    pub business: BusinessId,
    /// The job picked.
    pub job: Uuid,
    /// The Stripe charge of the fee, none for $0.
    pub charge: Option<&'a str>,
}

/// FEE-005, FEE-006, FEE-007, FEE-045, JOB-016: writes the fee fixed at the pick to the ledger
/// when the business confirms, in the caller's transaction: a charge of exactly `picked`'s
/// amount, or a $0 fee-free entry. The key is derived from the job and business IDs only, so a
/// repeat appends once (SEC-018, JOB-006). The amount is never re-read from a quote, a final
/// bill or a reply.
///
/// # Errors
///
/// [`may_confirm`]'s errors; [`ConfirmError::Ledger`] when the entry does not fit (for example a
/// charge with no Stripe ID).
pub async fn record_fee_at_confirm(
    conn: &mut PgConnection,
    id_secret: &SecretString,
    (picked, terms): (&PickedFee, FeeTerms),
    confirmed: Confirmed<'_>,
) -> Result<Appended, ConfirmError> {
    may_confirm(conn, confirmed.business, terms).await?;
    let kind = match picked.fee() {
        Fee::Charged { .. } => EntryKind::Charge,
        Fee::FeeFree { .. } => EntryKind::FeeFreeJobType,
    };
    let (job, business) = (confirmed.job.to_string(), confirmed.business.0.to_string());
    let key = IdempotencyKey::derive(id_secret, CONFIRM_FEE_ACTION, &[&job, &business]);
    let entry = LedgerEntry::new(NewEntry {
        kind,
        business: Some(confirmed.business.0),
        job: Some(confirmed.job),
        amount_cents: i64::try_from(picked.cents()).map_err(|_| LedgerError::Amount)?,
        stripe_id: confirmed.charge,
        idempotency_key: key.as_str(),
    })?;
    Ok(ledger_append(conn, &entry).await?)
}
