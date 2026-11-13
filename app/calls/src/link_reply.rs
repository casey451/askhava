//! The business's "LINK <PIN>" reply that links a blocked phone (CALL-011, CALL-003, CALL-012,
//! CALL-025; D-311, D-1696, D-1755, D-2247, D-3079, T391).
//!
//! T390's `link_after_dial` stores one pending link per line after a blocked call connected it
//! (`calls.link_request`, the caller's keyed `sender-dir` digests only). [`link_reply`] is called
//! by the inbound text route, in the delivery's transaction, for a text from a linked business
//! phone that [`link_pin`] reads as `LINK` and 4 digits. It links only when, in order: the
//! replying phone has fewer than 3 refused LINK replies this Arizona day (counted by its keyed
//! digest in `calls.link_reply_refusal`, never its number; past 3 the PIN is never compared, so
//! the reply is no PIN oracle), it is the business's main phone (CALL-012), and a live pending link
//! (`now < expires_at`, never used) on an open line of that same business has a stored PIN equal
//! to the digits. Then, in a savepoint: `businesses::api::link_by_reply` adds a staff phone linked
//! as by the owner (BIZ-033) with its audit row, `messaging::api::sender_put_pending` files the
//! pending digests under it (a digest another phone holds refuses), the pending link is marked
//! used and its digests deleted. Anything else is [`LinkReply::Refused`] with no change but one
//! more refusal counted. Pending links no longer live and used ones of the business are deleted
//! when met. Nothing here logs or stores a number or a PIN (PRIV-002, PRIV-016).

use askhava_businesses::api::{BusinessesError, is_main_phone, link_by_reply};
use askhava_messaging::api::{OutboxError, sender_line, sender_put_pending};
use askhava_privacy::api::{IndexPurpose, Vault, normalize_phone};
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::CallsError;
use crate::pins::{arizona_day, stored_pin};

/// Refused LINK replies a phone may send per Arizona day before every LINK from it is refused
/// unread (CALL-025's per-call PIN count, D-3079).
pub const LINK_REFUSALS_PER_DAY: i16 = 3;

/// How a LINK reply ended; both answer the sender the same way: nothing (D-3079).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkReply {
    /// The blocked phone is now a staff phone of the replying business.
    Linked,
    /// Nothing changed but the phone's refusal count.
    Refused,
}

/// The 4 PIN digits of a text that is exactly `LINK` and 4 digits, any case, any spaces around
/// or between (CALL-011, D-3079); any other text is `None` and keeps its usual path.
#[must_use]
pub fn link_pin(text: &str) -> Option<&str> {
    let mut words = text.split_whitespace();
    let (word, pin) = (words.next()?, words.next()?);
    let pin_shaped = pin.len() == 4 && pin.bytes().all(|b| b.is_ascii_digit());
    (words.next().is_none() && word.eq_ignore_ascii_case("link") && pin_shaped).then_some(pin)
}

/// Handles business `business`'s LINK reply from linked phone `from` with `pin` at `now`, on the
/// caller's transaction (CALL-011, CALL-003, CALL-012, CALL-025, D-3079): links the pending
/// blocked phone as a staff phone, or refuses with no change but the daily refusal count.
///
/// # Errors
///
/// [`CallsError::Database`], `Ops` (an audit row) or `Messaging`.
pub async fn link_reply(
    conn: &mut PgConnection,
    vault: &Vault,
    (business, from): (Uuid, &SecretString),
    pin: &str,
    now: Timestamp,
) -> Result<LinkReply, CallsError> {
    let Ok(phone) = normalize_phone(from.expose_secret()) else {
        return Ok(LinkReply::Refused);
    };
    let keys = vault.blind_index(IndexPurpose::SenderDir, &phone);
    if refusals_today(conn, &keys, now).await? >= LINK_REFUSALS_PER_DAY {
        return Ok(LinkReply::Refused);
    }
    forget_spent(conn, business, now).await?;
    let linked = match matching_request(conn, vault, (business, from), pin, now).await? {
        Some(request) => link_request(conn, business, request, now).await?,
        None => false,
    };
    if linked {
        return Ok(LinkReply::Linked);
    }
    count_refusal(conn, &keys, now).await?;
    Ok(LinkReply::Refused)
}

/// The most refusals any of the phone's key versions holds today (CALL-025).
async fn refusals_today(
    conn: &mut PgConnection,
    keys: &[(u16, [u8; 32])],
    now: Timestamp,
) -> Result<i16, CallsError> {
    let (versions, digests) = split(keys);
    let refused = sqlx::query_scalar!(
        "SELECT max(r.refused) FROM calls.link_reply_refusal r \
         JOIN unnest($1::int4[], $2::bytea[]) AS k(v, d) ON r.key_version = k.v AND r.digest = k.d \
         WHERE r.az_day = $3",
        &versions,
        &digests,
        arizona_day(now).to_sqlx() as _
    )
    .fetch_one(conn)
    .await
    .map_err(|_| CallsError::Database)?;
    Ok(refused.unwrap_or(0))
}

/// Counts one more refused LINK today under each of the phone's key versions, at most 3.
async fn count_refusal(
    conn: &mut PgConnection,
    keys: &[(u16, [u8; 32])],
    now: Timestamp,
) -> Result<(), CallsError> {
    let (versions, digests) = split(keys);
    sqlx::query!(
        "INSERT INTO calls.link_reply_refusal (key_version, digest, az_day, refused) \
         SELECT v::smallint, d, $3, 1 FROM unnest($1::int4[], $2::bytea[]) AS k(v, d) \
         ON CONFLICT (key_version, digest, az_day) \
         DO UPDATE SET refused = LEAST(calls.link_reply_refusal.refused + 1, 3)",
        &versions,
        &digests,
        arizona_day(now).to_sqlx() as _
    )
    .execute(conn)
    .await
    .map_err(|_| CallsError::Database)?;
    Ok(())
}

/// Deletes `business`'s pending links no longer live or already used, with their digests.
async fn forget_spent(
    conn: &mut PgConnection,
    business: Uuid,
    now: Timestamp,
) -> Result<(), CallsError> {
    sqlx::query!(
        "DELETE FROM calls.link_request WHERE business_id = $1 \
         AND (expires_at <= $2 OR used_at IS NOT NULL)",
        business,
        now.to_sqlx() as _
    )
    .execute(conn)
    .await
    .map_err(|_| CallsError::Database)?;
    Ok(())
}

/// The live, unused pending link on an open line of `business` whose stored PIN is `pin`, when
/// `from` is the business's main phone (CALL-012); locked for the link.
async fn matching_request(
    conn: &mut PgConnection,
    vault: &Vault,
    (business, from): (Uuid, &SecretString),
    pin: &str,
    now: Timestamp,
) -> Result<Option<Uuid>, CallsError> {
    let main = match sender_line(conn, vault, from.expose_secret()).await {
        Ok(Some((owner, line))) if owner == business => is_main_phone(conn, business, line)
            .await
            .map_err(|_| CallsError::Database)?,
        Ok(_) | Err(OutboxError::BadPhone) => false,
        Err(_) => return Err(CallsError::Messaging),
    };
    if !main {
        return Ok(None);
    }
    let live = sqlx::query!(
        "SELECT r.id, r.line_id FROM calls.link_request r \
         JOIN calls.number_pool p ON p.id = r.line_id \
         WHERE r.business_id = $1 AND p.business_id = $1 AND p.closed_at IS NULL \
         AND r.used_at IS NULL AND r.expires_at > $2 ORDER BY r.requested_at FOR UPDATE OF r",
        business,
        now.to_sqlx() as _
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(|_| CallsError::Database)?;
    for row in live {
        if stored_pin(conn, row.line_id).await?.map(|p| p.digits()) == Some(pin.to_owned()) {
            return Ok(Some(row.id));
        }
    }
    Ok(None)
}

/// In a savepoint: the staff phone, its sender-directory digests, the pending link marked used
/// and its digests deleted (CALL-011, BIZ-033). A digest another phone holds rolls it all back
/// and answers false.
async fn link_request(
    conn: &mut PgConnection,
    business: Uuid,
    request: Uuid,
    now: Timestamp,
) -> Result<bool, CallsError> {
    let mut tx = conn.begin().await.map_err(|_| CallsError::Database)?;
    let line = link_by_reply(&mut tx, business, now)
        .await
        .map_err(|e| match e {
            BusinessesError::Audit => CallsError::Ops,
            _ => CallsError::Database,
        })?;
    let rows = sqlx::query!(
        "DELETE FROM calls.link_request_digest WHERE request_id = $1 \
         RETURNING index_version, digest",
        request
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| CallsError::Database)?;
    let digests: Vec<(u16, [u8; 32])> = rows
        .into_iter()
        .filter_map(|r| {
            Some((
                u16::try_from(r.index_version).ok()?,
                r.digest.try_into().ok()?,
            ))
        })
        .collect();
    match sender_put_pending(&mut tx, (business, line), &digests).await {
        Ok(()) => {}
        Err(OutboxError::SenderTaken) => return Ok(false),
        Err(_) => return Err(CallsError::Messaging),
    }
    sqlx::query!(
        "UPDATE calls.link_request SET used_at = $2 WHERE id = $1",
        request,
        now.to_sqlx() as _
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| CallsError::Database)?;
    tx.commit().await.map_err(|_| CallsError::Database)?;
    Ok(true)
}

/// The keys as the two arrays the queries unnest.
fn split(keys: &[(u16, [u8; 32])]) -> (Vec<i32>, Vec<Vec<u8>>) {
    keys.iter()
        .map(|(v, d)| (i32::from(*v), d.to_vec()))
        .unzip()
}
