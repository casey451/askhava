//! The fee terms in force and the introduction fee by job size fixed at each pick (FEE-005,
//! FEE-007, FEE-016, FEE-047; D-072, D-1415, D-1871).
//!
//! [`fee_terms_in_force`] reads the one `payments.fee_terms` row (the fee mode and the
//! authorization version a business must have accepted); only `service::switch_fee_mode` writes
//! it. [`fix_fee_at_pick`] stores the [`PickedFee`] of a pick once, in the pick's own transaction
//! (`picks` calls it through `api`), under a share lock of the terms row, so a fee-mode switch
//! never commits between the terms the fee was computed under and the stored row. [`pick_fee`]
//! reads it back: `open_intent` and `confirm_and_pay` charge only that amount, `may_confirm` holds
//! its mode against the terms in force and `record_fee_at_confirm` writes it to the ledger. The
//! table is insert-only for the app role, so nothing changes a fee after the pick.

use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::BusinessId;
use crate::rules::{Fee, FeeMode, FeeTerms, PickedFee};

/// Why the fee terms or a pick's fee could not be read or fixed. No variant carries a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PickFeeError {
    /// The pick already has a fee fixed with other values; the stored one stands (FEE-005).
    #[error("the pick already has another fee fixed")]
    Conflict,
    /// The fee was computed under another fee mode than the one in force (FEE-007): compute it
    /// again under [`fee_terms_in_force`].
    #[error("the fee terms changed while the pick was made")]
    TermsChanged,
    /// The database refused or dropped the statement, or a stored value does not read back.
    #[error("the payments database statement failed")]
    Database,
}

/// The fee fixed at one pick, as stored (FEE-005, D-1871).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredPickFee {
    /// The picked business, the only one a press may come from.
    pub business: BusinessId,
    /// The pick's job.
    pub job: Uuid,
    /// The fee mode in force at the pick.
    pub mode: FeeMode,
    /// Whether a fee is charged; `false` is a fee-free job type at $0 (FEE-045).
    pub charged: bool,
    /// The fee in cents: above 0 exactly when charged.
    pub cents: u64,
    /// The authorization version of the terms in force at the pick.
    pub terms_version: i32,
}

fn mode_of(code: &str) -> Result<FeeMode, PickFeeError> {
    [FeeMode::Tiers, FeeMode::Flat]
        .into_iter()
        .find(|mode| mode.code() == code)
        .ok_or(PickFeeError::Database)
}

/// FEE-007, D-072, D-1871: the fee terms in force, from the one `payments.fee_terms` row; `picks`,
/// the Confirm page and the owner switch read the terms only here.
///
/// # Errors
///
/// [`PickFeeError::Database`].
pub async fn fee_terms_in_force(conn: &mut PgConnection) -> Result<FeeTerms, PickFeeError> {
    let row = sqlx::query!("SELECT mode, authorization_version FROM payments.fee_terms")
        .fetch_one(conn)
        .await
        .map_err(|_| PickFeeError::Database)?;
    Ok(FeeTerms::new(
        mode_of(&row.mode)?,
        row.authorization_version,
    ))
}

/// The terms in force under a share lock held to the end of the caller's transaction, so a switch
/// (which updates the row) waits for the pick, and the pick for a switch already under way.
async fn terms_locked(conn: &mut PgConnection) -> Result<FeeTerms, PickFeeError> {
    let row = sqlx::query!("SELECT mode, authorization_version FROM payments.fee_terms FOR SHARE")
        .fetch_one(conn)
        .await
        .map_err(|_| PickFeeError::Database)?;
    Ok(FeeTerms::new(
        mode_of(&row.mode)?,
        row.authorization_version,
    ))
}

/// FEE-005, FEE-007, D-1415, D-1871: stores `fee`, fixed by `fee_at_pick` for `pick` of `business`
/// on `job`, in the pick's transaction `tx`, with the authorization version of the terms in force.
/// Returns `true` when it wrote the row; a repeat with the same values writes nothing and returns
/// `false`.
///
/// # Errors
///
/// [`PickFeeError::TermsChanged`] when `fee` was fixed under another mode than the one in force;
/// [`PickFeeError::Conflict`] when the pick already has a fee with other values;
/// [`PickFeeError::Database`].
pub async fn fix_fee_at_pick(
    tx: &mut PgConnection,
    pick: Uuid,
    business: BusinessId,
    job: Uuid,
    fee: &PickedFee,
    at: Timestamp,
) -> Result<bool, PickFeeError> {
    let terms = terms_locked(tx).await?;
    if terms.mode() != fee.mode() {
        return Err(PickFeeError::TermsChanged);
    }
    let charged = matches!(fee.fee(), Fee::Charged { .. });
    let cents = i64::try_from(fee.cents()).map_err(|_| PickFeeError::Database)?;
    let written = sqlx::query!(
        "INSERT INTO payments.pick_fee (pick_id, business_id, job_id, mode, kind, amount_cents,
           terms_version, fixed_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (pick_id) DO NOTHING",
        pick,
        business.0,
        job,
        fee.mode().code(),
        if charged { "charged" } else { "fee_free" },
        cents,
        terms.authorization_version(),
        at.to_sqlx() as _,
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| PickFeeError::Database)?
    .rows_affected();
    if written == 1 {
        return Ok(true);
    }
    let stored = pick_fee(tx, pick).await?.ok_or(PickFeeError::Database)?;
    let same = (
        stored.business,
        stored.job,
        stored.mode,
        stored.charged,
        stored.cents,
    ) == (business, job, fee.mode(), charged, fee.cents());
    if same {
        Ok(false)
    } else {
        Err(PickFeeError::Conflict)
    }
}

/// FEE-005, FEE-016, D-1871: the fee fixed at `pick`, or none when no fee was fixed for it.
///
/// # Errors
///
/// [`PickFeeError::Database`], also for a stored row that does not read back.
pub async fn pick_fee(
    conn: &mut PgConnection,
    pick: Uuid,
) -> Result<Option<StoredPickFee>, PickFeeError> {
    let row = sqlx::query!(
        "SELECT business_id, job_id, mode, kind, amount_cents, terms_version
         FROM payments.pick_fee WHERE pick_id = $1",
        pick
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| PickFeeError::Database)?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(StoredPickFee {
        business: BusinessId(row.business_id),
        job: row.job_id,
        mode: mode_of(&row.mode)?,
        charged: row.kind == "charged",
        cents: u64::try_from(row.amount_cents).map_err(|_| PickFeeError::Database)?,
        terms_version: row.terms_version,
    }))
}
