//! The owner's approval of a phone a member other than the owner linked or renumbered (BIZ-033,
//! CALL-012; D-311, D-3081, T396).
//!
//! A phone's standing is `businesses.staff_phone.approved_at IS NOT NULL` alone. `team` stores it
//! NULL for a member's link or number change and holds the phone's digests out of `messaging`'s
//! sender directory (`messaging::api::sender_hold`). Here the owner, and only the owner, approves
//! ([`approve_phone`]: `approved_at` set and the digests released) or declines ([`reject_phone`]:
//! the row and its held digests deleted, so it never works) a waiting phone of the owner's own
//! business, naming the `number_set_at` the owner was shown: a number a member set since is
//! [`TeamError::NumberChanged`] and nothing changes. Each runs in one transaction with one audit
//! row of IDs only. [`waiting_phones`] lists what the owner may approve (T397's page);
//! [`phone_standing`] answers whether a phone works (T398). No number is stored here beyond the
//! last 4 digits `team` keeps, and nothing is logged (PRIV-002).

use askhava_messaging::api::{sender_drop_held, sender_release, sender_remove};
use askhava_ops::api::Change;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::team::{Acting, PHONE, TeamError, acting_owner, db, held, log};

/// A phone waiting for the owner's approval (BIZ-033): what the owner is shown to approve it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingPhone {
    /// The staff phone's line ID.
    pub line: Uuid,
    /// The member who set the current number.
    pub set_by: Uuid,
    /// The current number's last 4 digits (D-078), when known.
    pub number_end: Option<String>,
    /// When the current number was set; the approval names it back.
    pub number_set_at: Timestamp,
}

/// Checks `by` is the owner and locks waiting phone `line` of the owner's business whose number
/// was set at `set_at` (BIZ-033, D-3081).
async fn waiting_line(
    tx: &mut PgConnection,
    by: Acting,
    (line, set_at): (Uuid, Timestamp),
) -> Result<(), TeamError> {
    acting_owner(tx, by).await?;
    let row = db(sqlx::query!(
        "SELECT approved_at IS NOT NULL AS \"approved!\", \
         number_set_at IS NOT DISTINCT FROM $3 AS \"same!\" \
         FROM businesses.staff_phone WHERE id = $1 AND business_id = $2 FOR UPDATE",
        line,
        by.business,
        set_at.to_sqlx() as _
    )
    .fetch_optional(tx)
    .await)?
    .ok_or(TeamError::UnknownPhone)?;
    if row.approved {
        Err(TeamError::NotWaiting)
    } else if row.same {
        Ok(())
    } else {
        Err(TeamError::NumberChanged)
    }
}

/// The owner `by` approves waiting phone `line` whose number was set at `number_set_at`, as the
/// owner was shown it: `approved_at` is set to `now` and its digests return to the sender directory,
/// with one `businesses.phone_approved` audit row (BIZ-033, CALL-012, D-3081).
///
/// # Errors
///
/// [`TeamError::NotAllowed`] for any member but the owner; `UnknownPhone` for an unknown, declined
/// or another business's line; `NotWaiting`, `NumberChanged`; `PhoneTaken` when another phone now
/// has the number (nothing changes); `Database` or `Audit`.
pub async fn approve_phone(
    conn: &mut PgConnection,
    by: Acting,
    (line, number_set_at): (Uuid, Timestamp),
    now: Timestamp,
) -> Result<(), TeamError> {
    let mut tx = db(conn.begin().await)?;
    waiting_line(&mut tx, by, (line, number_set_at)).await?;
    db(sqlx::query!(
        "UPDATE businesses.staff_phone SET approved_at = $3 WHERE id = $1 AND business_id = $2",
        line,
        by.business,
        now.to_sqlx() as _
    )
    .execute(&mut *tx)
    .await)?;
    sender_release(&mut tx, line).await.map_err(held)?;
    let action = "businesses.phone_approved";
    log(&mut tx, by.business, action, (PHONE, line), Change::Action).await?;
    db(tx.commit().await)
}

/// The owner `by` declines waiting phone `line` whose number was set at `number_set_at`: the phone
/// row and its held digests are deleted, so it never works, with one `businesses.phone_rejected`
/// audit row (BIZ-033, CALL-012, D-3081). The same number linked again is a new waiting phone.
/// The time is taken so both calls read alike; a decline stores none.
///
/// # Errors
///
/// As [`approve_phone`], without `PhoneTaken`.
pub async fn reject_phone(
    conn: &mut PgConnection,
    by: Acting,
    (line, number_set_at): (Uuid, Timestamp),
    _now: Timestamp,
) -> Result<(), TeamError> {
    let mut tx = db(conn.begin().await)?;
    waiting_line(&mut tx, by, (line, number_set_at)).await?;
    db(sqlx::query!(
        "DELETE FROM businesses.staff_phone WHERE id = $1 AND business_id = $2",
        line,
        by.business
    )
    .execute(&mut *tx)
    .await)?;
    let database = |_| TeamError::Database;
    sender_drop_held(&mut tx, line).await.map_err(database)?;
    sender_remove(&mut tx, line).await.map_err(database)?;
    let action = "businesses.phone_rejected";
    log(&mut tx, by.business, action, (PHONE, line), Change::Action).await?;
    db(tx.commit().await)
}

/// Whether phone `line` works (`approved_at` set), or `None` for no such phone (BIZ-033, D-3081).
///
/// # Errors
///
/// [`TeamError::Database`].
pub async fn phone_standing(
    conn: &mut PgConnection,
    line: Uuid,
) -> Result<Option<bool>, TeamError> {
    db(sqlx::query_scalar!(
        "SELECT approved_at IS NOT NULL AS \"approved!\" FROM businesses.staff_phone \
         WHERE id = $1",
        line
    )
    .fetch_optional(conn)
    .await)
}

/// The phones of `business` waiting for the owner's approval, oldest number first (BIZ-033).
///
/// # Errors
///
/// [`TeamError::Database`].
pub async fn waiting_phones(
    conn: &mut PgConnection,
    business: Uuid,
) -> Result<Vec<WaitingPhone>, TeamError> {
    let rows = db(sqlx::query!(
        "SELECT id, number_set_by AS \"set_by!\", number_end, \
         number_set_at AS \"set_at!\" FROM businesses.staff_phone \
         WHERE business_id = $1 AND approved_at IS NULL ORDER BY number_set_at, id",
        business
    )
    .fetch_all(conn)
    .await)?;
    Ok(rows
        .into_iter()
        .map(|r| WaitingPhone {
            line: r.id,
            set_by: r.set_by,
            number_end: r.number_end,
            number_set_at: r.set_at.to_jiff(),
        })
        .collect())
}
