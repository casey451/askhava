//! The pending link a blocked call leaves once it connected a line (CALL-011, CALL-003; D-078,
//! D-311, D-1696, D-2247, D-3079, D-3167 to D-3169).
//!
//! A "blocked" call is one from an unlinked phone, made to enter the PIN (D-078). Its business is
//! known only once the PIN connects a line, so [`link_after_dial`] runs only after the voice route
//! answered a turn with the dial: for a [`Party::Unlinked`] caller whose CallSid has a stored call
//! start (`models::call_start`, written by the dial), it calls [`request_link`] for that line and,
//! in the same transaction, when the notice was queued now, stores one pending link in
//! `calls.link_request`: the line, its business, the request time and an expiry 24 hours later,
//! with the caller's keyed `sender-dir` digests in `calls.link_request_digest` (as
//! `messaging.sender_directory` stores a linked phone). Never the number, its last 4 digits or the
//! PIN (PRIV-002). A homeowner, a linked business phone, a call that connected no line and a
//! repeat on the same line (`request_link`'s once-per-line rule) store nothing. T391's
//! `link_reply` reads the pending link while `expires_at` is still in the future.

use askhava_privacy::api::{IndexPurpose, Vault, normalize_phone};
use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::lifecycle::request_link;
use crate::models::{self, CallsError};
use crate::routing::{Party, party_of};

/// How long a pending link stays live after its request: 24 hours (D-3079, D-3168).
pub const LINK_REQUEST_LIFE: SignedDuration = SignedDuration::from_hours(24);

/// After a voice turn answered with the dial for call `call_sid` from `caller` at `now`: when the
/// caller is unlinked and the call's stored start names a line, queues that line's LINK notice
/// ([`request_link`]) and, in the same transaction, when it was queued now, stores one pending
/// link holding only the line, its business, the times and the caller's `sender-dir` digests
/// (CALL-011, CALL-003, PRIV-002, D-1696, D-3079). Returns whether a pending link was stored.
/// Call it outside a transaction; a failure rolls back both the notice and the pending link.
///
/// # Errors
///
/// [`CallsError::Database`] or [`CallsError::Messaging`].
pub async fn link_after_dial(
    db: &mut PgConnection,
    vault: &Vault,
    call_sid: &str,
    caller: &SecretString,
    now: Timestamp,
) -> Result<bool, CallsError> {
    let Ok(phone) = normalize_phone(caller.expose_secret()) else {
        return Ok(false);
    };
    if party_of(db, vault, caller).await? != Party::Unlinked {
        return Ok(false);
    }
    let Some((line, _)) = models::call_start(db, call_sid).await? else {
        return Ok(false);
    };
    let digests = vault.blind_index(IndexPurpose::SenderDir, &phone);
    let mut tx = db.begin().await.map_err(|_| CallsError::Database)?;
    if !request_link(&mut tx, line, caller, now).await? {
        return Ok(false);
    }
    store_pending(&mut tx, line, &digests, now).await?;
    tx.commit().await.map_err(|_| CallsError::Database)?;
    Ok(true)
}

/// Inserts the pending link for `line` (its business read from the line) and its digests.
async fn store_pending(
    tx: &mut PgConnection,
    line: Uuid,
    digests: &[(u16, [u8; 32])],
    now: Timestamp,
) -> Result<(), CallsError> {
    let expires = now
        .checked_add(LINK_REQUEST_LIFE)
        .map_err(|_| CallsError::Database)?;
    let request = sqlx::query_scalar!(
        "INSERT INTO calls.link_request (line_id, business_id, requested_at, expires_at) \
         SELECT id, business_id, $2, $3 FROM calls.number_pool WHERE id = $1 RETURNING id",
        line,
        now.to_sqlx() as _,
        expires.to_sqlx() as _
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| CallsError::Database)?;
    let (versions, digests): (Vec<i32>, Vec<Vec<u8>>) = digests
        .iter()
        .map(|(v, d)| (i32::from(*v), d.to_vec()))
        .unzip();
    sqlx::query!(
        "INSERT INTO calls.link_request_digest (request_id, index_version, digest) \
         SELECT $1, v::smallint, d FROM unnest($2::int4[], $3::bytea[]) AS k(v, d)",
        request,
        &versions,
        &digests
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| CallsError::Database)?;
    Ok(())
}
