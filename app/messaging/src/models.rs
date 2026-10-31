//! The outbox, sender-directory, inbox, delivery-event, opt-out event and suppression rows and
//! their queries, all in the `messaging` schema (TXT-011, TXT-012, TXT-013, D-492, CON-014,
//! CON-035). No query holds a phone number, only opaque IDs, keys, Twilio message SIDs and
//! blind-index digests (PRIV-002).

use askhava_core::api::Dedupe;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;
use thiserror::Error;

use crate::status::DeliveryStatus;

/// Why a `messaging` outbox or directory call failed. No variant carries a value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum OutboxError {
    /// A text to a past customer who has not written since the request closed (TXT-009).
    #[error("no text to a past customer who has not written first")]
    PastCustomer,
    /// The phone is not a US number the blind index accepts.
    #[error("the phone number is not a valid US number")]
    BadPhone,
    /// The phone is already linked to another business (D-492).
    #[error("the phone is linked to another business")]
    SenderTaken,
    /// The carrier refused the send or its outcome is unknown; the row stays unsent and a resend
    /// carries the same key (TXT-011).
    #[error("the carrier did not confirm the send")]
    Carrier,
    /// The homeowner's number could not be read from the vault for the row's recipient (D-849).
    #[error("the recipient's number could not be read")]
    Recipient,
    /// The row was queued for the other kind of recipient (homeowner or business) than the send
    /// names; the row is left as it was (D-849).
    #[error("the text was queued for another kind of recipient")]
    RecipientKind,
    /// The outbox has no row with this ID.
    #[error("no such outbox row")]
    NotFound,
    /// The number is on the suppression list and the text is not one that may still go to it
    /// (CON-013, CON-035); the row is left as it was.
    #[error("the number opted out of texts")]
    Suppressed,
    /// The carrier refused the send with 21610 (recipient opted out); the row is blocked and the
    /// number listed, and it is never retried (CON-014).
    #[error("the carrier refused the text: the recipient opted out")]
    Blocked,
    /// The compiled-in template files are broken.
    #[error("the template could not be rendered")]
    Template,
    /// The database refused or dropped the statement.
    #[error("the messaging database statement failed")]
    Database,
}

/// Why a Twilio inbound or status webhook was not recorded. No variant carries a value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum InboxError {
    /// The delivery is not Twilio's, or a field (MessageSid, ErrorCode) is missing or ill-shaped.
    #[error("the webhook is not a well-formed Twilio delivery")]
    BadWebhook,
    /// The database refused or dropped the statement.
    #[error("the messaging database statement failed")]
    Database,
}

/// Records `event_key` under `source` in `messaging.webhook_event`, received at `now` from the
/// injected clock; true on its first delivery (SEC-017, TXT-012, D-863).
pub(crate) async fn first_event(
    db: &mut PgConnection,
    (source, event_key): (&str, &str),
    now: Timestamp,
) -> Result<bool, InboxError> {
    let row = sqlx::query_scalar!(
        "INSERT INTO messaging.webhook_event (source, event_key, received_at) VALUES ($1, $2, $3) \
         ON CONFLICT (source, event_key) DO NOTHING RETURNING event_key",
        source,
        event_key,
        now.to_sqlx() as _
    )
    .fetch_optional(db)
    .await
    .map_err(|_| InboxError::Database)?;
    Ok(Dedupe::from_returning(row).is_first())
}

/// Inserts the inbox row of inbound text `sid`, received at `now`, and returns its ID (TXT-012,
/// D-863).
pub(crate) async fn insert_inbox(
    db: &mut PgConnection,
    sid: &str,
    now: Timestamp,
) -> Result<Uuid, InboxError> {
    sqlx::query_scalar!(
        "INSERT INTO messaging.inbox (message_sid, received_at) VALUES ($1, $2) RETURNING id",
        sid,
        now.to_sqlx() as _
    )
    .fetch_one(db)
    .await
    .map_err(|_| InboxError::Database)
}

/// Appends one delivery event for provider SID `sid`, received at `now` (TXT-013, D-863).
pub(crate) async fn insert_delivery(
    db: &mut PgConnection,
    sid: &str,
    (status, error_code): (DeliveryStatus, Option<i32>),
    now: Timestamp,
) -> Result<(), InboxError> {
    sqlx::query!(
        "INSERT INTO messaging.delivery_event (provider_sid, status, error_code, received_at) \
         VALUES ($1, $2, $3, $4)",
        sid,
        status.as_str(),
        error_code,
        now.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| InboxError::Database)
}

/// Outbox row `id`'s own status and provider SID, if the row exists.
pub(crate) async fn outbox_state(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<Option<(String, Option<String>)>, OutboxError> {
    let row = sqlx::query!(
        "SELECT status, provider_sid FROM messaging.outbox WHERE id = $1",
        id
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(row.map(|r| (r.status, r.provider_sid)))
}

/// Every delivery event of provider SID `sid`, oldest first (TXT-013).
pub(crate) async fn delivery_events(
    db: &mut PgConnection,
    sid: &str,
) -> Result<Vec<String>, OutboxError> {
    sqlx::query_scalar!(
        "SELECT status FROM messaging.delivery_event WHERE provider_sid = $1 ORDER BY id",
        sid
    )
    .fetch_all(db)
    .await
    .map_err(|_| OutboxError::Database)
}

/// A queued row, claimed for one send attempt: the IDs its key is derived from, and its body.
pub(crate) struct Claim {
    pub(crate) template_id: String,
    pub(crate) recipient: Uuid,
    pub(crate) subject: Vec<Uuid>,
    pub(crate) body: String,
}

/// Inserts a queued text for a recipient of `kind` (`homeowner` or `business`) unless its key is
/// taken; returns the row's ID either way and whether it is new (TXT-011, D-849).
pub(crate) async fn insert_text(
    db: &mut PgConnection,
    key: &str,
    (template_id, recipient, subject): (&str, Uuid, &[Uuid]),
    kind: &str,
    body: &str,
) -> Result<(Uuid, bool), OutboxError> {
    let new = sqlx::query_scalar!(
        "INSERT INTO messaging.outbox \
         (idempotency_key, template_id, recipient_id, recipient_kind, subject_ids, body) \
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (idempotency_key) DO NOTHING RETURNING id",
        key,
        template_id,
        recipient,
        kind,
        subject,
        body
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(|_| OutboxError::Database)?;
    if let Some(id) = new {
        return Ok((id, true));
    }
    let id = sqlx::query_scalar!(
        "SELECT id FROM messaging.outbox WHERE idempotency_key = $1",
        key
    )
    .fetch_one(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok((id, false))
}

/// Claims row `id` for one attempt unless it was already sent or blocked, or was queued for
/// another recipient `kind`: counts the attempt and returns the key's IDs and the body; `None` when
/// it is sent or blocked, so a 21610 refusal is never retried (TXT-011, CON-014). A kind mismatch
/// is [`OutboxError::RecipientKind`] and changes nothing (D-849).
pub(crate) async fn claim(
    db: &mut PgConnection,
    id: Uuid,
    kind: &str,
) -> Result<Option<Claim>, OutboxError> {
    let row = sqlx::query!(
        "UPDATE messaging.outbox SET status = 'sending', attempts = attempts + 1 \
         WHERE id = $1 AND status IN ('queued', 'sending') AND recipient_kind = $2 \
         RETURNING template_id, recipient_id, subject_ids, body",
        id,
        kind
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(|_| OutboxError::Database)?;
    if let Some(row) = row {
        return Ok(Some(Claim {
            template_id: row.template_id,
            recipient: row.recipient_id,
            subject: row.subject_ids,
            body: row.body.ok_or(OutboxError::Database)?,
        }));
    }
    let stored = sqlx::query_scalar!(
        "SELECT recipient_kind FROM messaging.outbox WHERE id = $1",
        id
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    match stored {
        None => Err(OutboxError::NotFound),
        Some(stored) if stored != kind => Err(OutboxError::RecipientKind),
        Some(_) => Ok(None),
    }
}

/// Marks row `id` sent with the carrier's message SID and clears its body, only if it is still
/// being sent (TXT-011, D-850).
pub(crate) async fn mark_sent(
    db: &mut PgConnection,
    id: Uuid,
    sid: &str,
) -> Result<(), OutboxError> {
    sqlx::query!(
        "UPDATE messaging.outbox SET status = 'sent', provider_sid = $2, sent_at = now(), body = NULL \
         WHERE id = $1 AND status = 'sending'",
        id,
        sid
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| OutboxError::Database)
}

/// Replaces phone `phone_ref`'s digests with `digests`, linked to `business` (D-492). A digest
/// another phone holds is [`OutboxError::SenderTaken`].
pub(crate) async fn put_sender(
    db: &mut PgConnection,
    (business, phone_ref): (Uuid, Uuid),
    digests: &[(u16, [u8; 32])],
) -> Result<(), OutboxError> {
    delete_sender(&mut *db, phone_ref).await?;
    for (version, digest) in digests {
        sqlx::query!(
            "INSERT INTO messaging.sender_directory (line_id, index_version, digest, business_id) \
             VALUES ($1, $2, $3, $4)",
            phone_ref,
            i16::try_from(*version).map_err(|_| OutboxError::Database)?,
            &digest[..],
            business
        )
        .execute(&mut *db)
        .await
        .map_err(|e| match e.as_database_error() {
            Some(d) if d.is_unique_violation() => OutboxError::SenderTaken,
            _ => OutboxError::Database,
        })?;
    }
    Ok(())
}

/// Deletes every digest of phone `phone_ref` (D-492).
pub(crate) async fn delete_sender(
    db: &mut PgConnection,
    phone_ref: Uuid,
) -> Result<(), OutboxError> {
    sqlx::query!(
        "DELETE FROM messaging.sender_directory WHERE line_id = $1",
        phone_ref
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| OutboxError::Database)
}

/// The business holding any of `digests` (D-492).
pub(crate) async fn find_sender(
    db: &mut PgConnection,
    digests: &[(u16, [u8; 32])],
) -> Result<Option<Uuid>, OutboxError> {
    let (versions, hashes) = split_digests(digests);
    sqlx::query_scalar!(
        "SELECT d.business_id FROM messaging.sender_directory d \
         JOIN UNNEST($1::smallint[], $2::bytea[]) AS k (v, h) \
         ON d.index_version = k.v AND d.digest = k.h LIMIT 1",
        &versions,
        &hashes
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OutboxError::Database)
}

/// One opt-out or START to log (CON-022, CON-035, CON-036): `action` is `opt_out` or `start`,
/// `channel` the stored channel name, `detected` how it was detected, `digest` the number's active
/// blind-index digest and `at` the instant suppression took effect.
pub(crate) struct OptOutEvent<'a> {
    pub(crate) action: &'static str,
    pub(crate) channel: &'static str,
    pub(crate) detected: &'a str,
    pub(crate) digest: (u16, [u8; 32]),
    pub(crate) at: Timestamp,
}

/// Appends `event` to `messaging.optout_event` and returns its ID (CON-022, CON-035).
pub(crate) async fn insert_optout_event(
    db: &mut PgConnection,
    event: &OptOutEvent<'_>,
) -> Result<Uuid, OutboxError> {
    let (version, digest) = event.digest;
    sqlx::query_scalar!(
        "INSERT INTO messaging.optout_event \
         (action, channel, detected, index_version, digest, effective_at) \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
        event.action,
        event.channel,
        event.detected,
        i16::try_from(version).map_err(|_| OutboxError::Database)?,
        &digest[..],
        event.at.to_sqlx() as _
    )
    .fetch_one(db)
    .await
    .map_err(|_| OutboxError::Database)
}

/// Since when the number with `digests` is on the suppression list, if it is (CON-035).
pub(crate) async fn suppressed_since(
    db: &mut PgConnection,
    digests: &[(u16, [u8; 32])],
) -> Result<Option<Timestamp>, OutboxError> {
    let (versions, hashes) = split_digests(digests);
    let since = sqlx::query_scalar!(
        "SELECT min(s.since) FROM messaging.suppression s \
         JOIN UNNEST($1::smallint[], $2::bytea[]) AS k (v, h) \
         ON s.index_version = k.v AND s.digest = k.h",
        &versions,
        &hashes
    )
    .fetch_one(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(since.map(|at| at.to_jiff()))
}

/// Puts the number with `digests` on the suppression list since `at`, by opt-out event `event`
/// (CON-035). A version already listed keeps its earlier row.
pub(crate) async fn put_suppression(
    db: &mut PgConnection,
    digests: &[(u16, [u8; 32])],
    event: Uuid,
    at: Timestamp,
) -> Result<(), OutboxError> {
    let (versions, hashes) = split_digests(digests);
    sqlx::query!(
        "INSERT INTO messaging.suppression (index_version, digest, event_id, since) \
         SELECT v, h, $3, $4 FROM UNNEST($1::smallint[], $2::bytea[]) AS k (v, h) \
         ON CONFLICT (index_version, digest) DO NOTHING",
        &versions,
        &hashes,
        event,
        at.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| OutboxError::Database)
}

/// Takes the number with `digests` off the suppression list (CON-015).
pub(crate) async fn lift_suppression(
    db: &mut PgConnection,
    digests: &[(u16, [u8; 32])],
) -> Result<(), OutboxError> {
    let (versions, hashes) = split_digests(digests);
    sqlx::query!(
        "DELETE FROM messaging.suppression s USING UNNEST($1::smallint[], $2::bytea[]) AS k (v, h) \
         WHERE s.index_version = k.v AND s.digest = k.h",
        &versions,
        &hashes
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| OutboxError::Database)
}

/// Outbox row `id`'s recipient ID and template ID, if the row exists (CON-035).
pub(crate) async fn outbox_target(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<Option<(Uuid, String)>, OutboxError> {
    let row = sqlx::query!(
        "SELECT recipient_id, template_id FROM messaging.outbox WHERE id = $1",
        id
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(row.map(|r| (r.recipient_id, r.template_id)))
}

/// Marks row `id` blocked and clears its body: the carrier refused it with 21610, so it is never
/// claimed again (CON-014, SEC-037).
pub(crate) async fn block(db: &mut PgConnection, id: Uuid) -> Result<(), OutboxError> {
    sqlx::query!(
        "UPDATE messaging.outbox SET status = 'blocked', body = NULL \
         WHERE id = $1 AND status = 'sending'",
        id
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| OutboxError::Database)
}

/// Splits digests into the parallel version and hash arrays the `UNNEST` queries take.
fn split_digests(digests: &[(u16, [u8; 32])]) -> (Vec<i16>, Vec<Vec<u8>>) {
    digests
        .iter()
        .filter_map(|(v, d)| Some((i16::try_from(*v).ok()?, d.to_vec())))
        .unzip()
}
