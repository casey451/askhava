//! Free first wins and repeat customers: the $0 introductions (FEE-010 to FEE-013, D-019, D-069,
//! D-151, D-152, D-195).
//!
//! `payments` cannot read `licensing`, `businesses` or `catalog`, so the callers hand it what it
//! judges by (D-491): T081's sign-up (and the card-save step, for the card fingerprint) records
//! a business's [`EntityKey`]s with [`record_entity_keys`]; T224's Confirm page calls
//! [`waive_at_confirm`] before any Stripe charge, with the pick's business, its [`JoinDates`] and
//! the other jobs of the same customer still on record. That call decides, in this order:
//!
//! 1. a fee-free job type is not waived here ($0 already, written by `record_fee_at_confirm`);
//! 2. repeat work: the business already had a confirmed introduction on one of those jobs, so a
//!    $0 `repeat-customer` entry, and the free first win is not touched (FEE-013);
//! 3. a free first win: the business joined inside the 90-day window ([`in_free_win_window`]) and
//!    neither it nor any business sharing one of its entity keys has used one, checked now, at
//!    use (FEE-011); a $0 `free-first-win` entry plus a `payments.free_win` row holding the
//!    waived amount (FEE-012), and every key of the business claimed;
//! 4. otherwise [`Waiver::NotWaived`]: the caller charges the fee.
//!
//! Everything runs on the caller's connection, inside its Confirm transaction, never across a
//! Stripe call (R11). Ledger keys come from the job and business IDs only (SEC-018, JOB-006).

use askhava_adapters::api::IdempotencyKey;
use jiff::{SignedDuration, Timestamp};
use secrecy::SecretString;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::ledger::{Appended, EntryKind, LedgerEntry, LedgerError, NewEntry, ledger_append};
use crate::models::{BusinessId, EntityKey, FreeWinError, JoinDates};
use crate::rules::PickedFee;

/// The free-first-win window: 90 days after launch or after a category is switched on (FEE-010).
pub const FREE_WIN_WINDOW_DAYS: i64 = 90;
/// The idempotency action of a free-first-win ledger entry (SEC-018, JOB-006).
pub const FREE_WIN_ACTION: &str = "payments.free_first_win";
/// The idempotency action of a repeat-customer ledger entry (SEC-018, JOB-006).
pub const REPEAT_ACTION: &str = "payments.repeat_customer";

/// FEE-010, D-152, D-195: true when the sign-up form was submitted strictly before 90 days after
/// launch, or strictly before 90 days after any of the business's categories was switched on
/// (a business that joined earlier, waitlisted, counts). Exactly 90 days is outside.
#[must_use]
pub fn in_free_win_window(dates: &JoinDates) -> bool {
    let window = SignedDuration::from_hours(FREE_WIN_WINDOW_DAYS * 24);
    std::iter::once(dates.launch)
        .chain(dates.trades_opened.iter().copied())
        .any(|start| {
            start
                .checked_add(window)
                .is_ok_and(|end: Timestamp| dates.joined < end)
        })
}

/// FEE-011, D-151: records the business's entity keys (digests only); a key already recorded is
/// kept as it is. Keys are never removed, so a later change of ROC number or card still counts
/// the old one.
///
/// # Errors
/// [`FreeWinError::Database`] when a statement fails.
pub async fn record_entity_keys(
    conn: &mut PgConnection,
    business: BusinessId,
    keys: &[EntityKey],
) -> Result<(), FreeWinError> {
    for key in keys {
        sqlx::query!(
            "INSERT INTO payments.entity_key (business_id, kind, digest) VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
            business.0,
            key.kind().code(),
            key.digest(),
        )
        .execute(&mut *conn)
        .await
        .map_err(|_| FreeWinError::Database)?;
    }
    Ok(())
}

/// What [`waive_at_confirm`] needs to know about the pick being confirmed.
#[derive(Clone, Copy, Debug)]
pub struct WinCheck<'a> {
    /// The business confirming the pick.
    pub business: BusinessId,
    /// The job picked.
    pub job: Uuid,
    /// The fee fixed at the pick (FEE-005).
    pub picked: &'a PickedFee,
    /// The dates FEE-010 judges the business by.
    pub dates: &'a JoinDates,
    /// The customer's other jobs still on record (none after the 180-day contact purge, PRIV-007).
    pub same_customer_jobs: &'a [Uuid],
}

/// What [`waive_at_confirm`] decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waiver {
    /// Nothing waived: the caller charges the fee (or records a fee-free job type).
    NotWaived,
    /// Repeat work from the same customer: a $0 entry, the free first win untouched (FEE-013).
    Repeat(Appended),
    /// The free first win: a $0 entry and the fee it waived, in cents (FEE-012).
    FreeFirstWin {
        /// The ledger entry.
        entry: Appended,
        /// The introduction fee by job size the win waived.
        waived_cents: i64,
    },
}

/// FEE-010 to FEE-013, D-069, D-151: decides at Confirm, before any charge, whether the pick is
/// a repeat customer or the business's free first win, and writes the $0 entry (see the module
/// docs for the order). A repeated call for the same job returns the same answer and writes
/// nothing new.
///
/// # Errors
/// [`FreeWinError::Race`] when a shared key or this business's win was used by another Confirm
/// at the same moment (nothing written; run the Confirm again); [`FreeWinError::Ledger`] or
/// [`FreeWinError::Database`] when a write fails.
pub async fn waive_at_confirm(
    conn: &mut PgConnection,
    secret: &SecretString,
    check: WinCheck<'_>,
) -> Result<Waiver, FreeWinError> {
    if check.picked.cents() == 0 {
        return Ok(Waiver::NotWaived);
    }
    if is_repeat(conn, &check).await? {
        let entry = append_zero(conn, secret, EntryKind::RepeatCustomer, &check).await?;
        return Ok(Waiver::Repeat(entry));
    }
    if let Some((job, entry, waived_cents)) = used_win(conn, check.business).await? {
        return Ok(if job == check.job {
            Waiver::FreeFirstWin {
                entry: Appended::Already(entry),
                waived_cents,
            }
        } else {
            Waiver::NotWaived
        });
    }
    if !in_free_win_window(check.dates) || key_taken(conn, check.business).await? {
        return Ok(Waiver::NotWaived);
    }
    use_win(conn, secret, &check).await
}

/// FEE-013, D-069: the business already had a confirmed introduction (charged, free first win,
/// repeat or fee-free) on another of the customer's jobs.
async fn is_repeat(conn: &mut PgConnection, check: &WinCheck<'_>) -> Result<bool, FreeWinError> {
    if check.same_customer_jobs.is_empty() {
        return Ok(false);
    }
    let found = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM payments.ledger_entry
           WHERE business_id = $1 AND job_id = ANY($2) AND job_id <> $3
             AND kind IN ('charge', 'free-first-win', 'repeat-customer', 'fee-free-job-type'))",
        check.business.0,
        check.same_customer_jobs,
        check.job,
    )
    .fetch_one(conn)
    .await
    .map_err(|_| FreeWinError::Database)?;
    Ok(found.unwrap_or(false))
}

/// The business's used free first win: its job, ledger entry and waived cents.
async fn used_win(
    conn: &mut PgConnection,
    business: BusinessId,
) -> Result<Option<(Uuid, Uuid, i64)>, FreeWinError> {
    let row = sqlx::query!(
        "SELECT job_id, ledger_entry_id, waived_cents FROM payments.free_win
           WHERE business_id = $1",
        business.0,
    )
    .fetch_optional(conn)
    .await
    .map_err(|_| FreeWinError::Database)?;
    Ok(row.map(|r| (r.job_id, r.ledger_entry_id, r.waived_cents)))
}

/// FEE-011: one of the business's entity keys was already claimed by a used free first win.
async fn key_taken(conn: &mut PgConnection, business: BusinessId) -> Result<bool, FreeWinError> {
    let taken = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM payments.entity_key k
           JOIN payments.free_win_claim c ON c.kind = k.kind AND c.digest = k.digest
           WHERE k.business_id = $1)",
        business.0,
    )
    .fetch_one(conn)
    .await
    .map_err(|_| FreeWinError::Database)?;
    Ok(taken.unwrap_or(false))
}

/// FEE-011, FEE-012: in a savepoint, the $0 entry, the `free_win` row with the waived amount and
/// a claim on every key; a claim or win taken meanwhile rolls it all back as [`FreeWinError::Race`].
async fn use_win(
    conn: &mut PgConnection,
    secret: &SecretString,
    check: &WinCheck<'_>,
) -> Result<Waiver, FreeWinError> {
    let waived_cents = i64::try_from(check.picked.cents()).map_err(|_| LedgerError::Amount)?;
    let mut tx = conn.begin().await.map_err(|_| FreeWinError::Database)?;
    let entry = append_zero(&mut tx, secret, EntryKind::FreeFirstWin, check).await?;
    let (Appended::New(entry_id) | Appended::Already(entry_id)) = entry;
    let won = sqlx::query!(
        "INSERT INTO payments.free_win (business_id, job_id, ledger_entry_id, waived_cents)
         VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
        check.business.0,
        check.job,
        entry_id,
        waived_cents,
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| FreeWinError::Database)?;
    let claims = sqlx::query!(
        "WITH mine AS (SELECT kind, digest FROM payments.entity_key WHERE business_id = $1),
              added AS (INSERT INTO payments.free_win_claim (kind, digest, business_id)
                        SELECT kind, digest, $1 FROM mine ON CONFLICT DO NOTHING RETURNING 1)
         SELECT (SELECT count(*) FROM mine) AS \"keys!\", (SELECT count(*) FROM added) AS \"added!\"",
        check.business.0,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| FreeWinError::Database)?;
    if won.rows_affected() != 1 || claims.keys != claims.added {
        tx.rollback().await.map_err(|_| FreeWinError::Database)?;
        return Err(FreeWinError::Race);
    }
    tx.commit().await.map_err(|_| FreeWinError::Database)?;
    Ok(Waiver::FreeFirstWin {
        entry,
        waived_cents,
    })
}

/// Appends the $0 entry of `kind` for the check's job and business (FEE-012, FEE-013, FEE-028).
async fn append_zero(
    conn: &mut PgConnection,
    secret: &SecretString,
    kind: EntryKind,
    check: &WinCheck<'_>,
) -> Result<Appended, FreeWinError> {
    let action = if kind == EntryKind::FreeFirstWin {
        FREE_WIN_ACTION
    } else {
        REPEAT_ACTION
    };
    let (job, business) = (check.job.to_string(), check.business.0.to_string());
    let key = IdempotencyKey::derive(secret, action, &[&job, &business]);
    let entry = LedgerEntry::new(NewEntry {
        kind,
        business: Some(check.business.0),
        job: Some(check.job),
        amount_cents: 0,
        stripe_id: None,
        idempotency_key: key.as_str(),
    })?;
    Ok(ledger_append(conn, &entry).await?)
}
