//! The outbox, sender-directory, inbox, delivery-event, opt-out event and suppression rows and
//! their queries, all in the `messaging` schema (TXT-011, TXT-012, TXT-013, D-492, CON-014,
//! CON-035). No query holds a phone number, only opaque IDs, keys, Twilio message SIDs and
//! blind-index digests (PRIV-002). The outbox row is the message log entry (TXT-019) and the
//! opt-out event keeps its inbound message (CON-036).

use askhava_core::api::Dedupe;
use jiff::Timestamp;
use jiff_sqlx::ToSqlx;
use sqlx::PgConnection;
use sqlx::types::Uuid;
use thiserror::Error;

use crate::retention;
use crate::rules::Listing;
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
    /// The carrier refused the send outright; the row is `queued` again for a retry (TXT-014).
    #[error("the carrier did not confirm the send")]
    Carrier,
    /// The carrier may have taken the send (a timeout or a lost answer); the row stays `sending`
    /// and is never resent blind: the sender moves it to `unknown` (TXT-011, D-558, D-2628).
    #[error("the carrier's answer to the send is unknown")]
    OutcomeUnknown,
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
/// taken; returns the row's ID either way and whether it is new (TXT-011, D-849). The row is the
/// message log entry: the vault reference, never the number, and the body's language, read from
/// the body against the template (`retention::language_of`, TXT-019, D-1851). `exempt` is the
/// QH-004 exemption stored as `quiet_exempt_until` (D-2628).
pub(crate) async fn insert_text(
    db: &mut PgConnection,
    (key, kind): (&str, &str),
    (template_id, recipient, subject): (&str, Uuid, &[Uuid]),
    body: &str,
    exempt: QuietExempt,
) -> Result<(Uuid, bool), OutboxError> {
    let language = retention::language_of(template_id, body);
    let (forever, until) = match exempt {
        QuietExempt::None => (false, None),
        QuietExempt::Always => (true, None),
        QuietExempt::Until(at) => (false, Some(at.to_sqlx())),
    };
    let new = sqlx::query_scalar!(
        "INSERT INTO messaging.outbox \
         (idempotency_key, template_id, recipient_id, recipient_kind, subject_ids, body, language, \
         quiet_exempt_until) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, CASE WHEN $8 THEN 'infinity'::timestamptz ELSE $9 END) \
         ON CONFLICT (idempotency_key) DO NOTHING RETURNING id",
        key,
        template_id,
        recipient,
        kind,
        subject,
        body,
        language,
        forever,
        until as _
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

/// The QH-004 exemption a row is queued with (D-2628): none, at any hour, or until an instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuietExempt {
    /// Held by quiet hours like any text.
    None,
    /// Sent at any hour (`infinity`).
    Always,
    /// Sent at any hour before this instant (a direct reply's inbound time plus 15 minutes).
    Until(Timestamp),
}

/// Claims queued row `id` for one attempt unless it was queued for another recipient `kind`:
/// moves it to `sending` with `sending_since` (`at`, else the database's now) and counts the
/// attempt in one statement, so two senders can never both hold it, and returns the key's IDs and
/// the body; `None` when it is not `queued` (sent, being sent, blocked, unknown, expired), so a
/// row is never handed to the carrier twice (TXT-011, CON-014, D-558, D-2628). It does not read
/// `not_before`. A kind mismatch is [`OutboxError::RecipientKind`] and changes nothing (D-849).
pub(crate) async fn claim(
    db: &mut PgConnection,
    id: Uuid,
    kind: &str,
    at: Option<Timestamp>,
) -> Result<Option<Claim>, OutboxError> {
    let row = sqlx::query!(
        "UPDATE messaging.outbox SET status = 'sending', attempts = attempts + 1, \
         sending_since = COALESCE($3, now()) \
         WHERE id = $1 AND status = 'queued' AND recipient_kind = $2 \
         RETURNING template_id, recipient_id, subject_ids, body",
        id,
        kind,
        at.map(|t| t.to_sqlx()) as _
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

/// Moves row `id` from `sending` back to `queued`: the carrier refused it outright or the number
/// could not be read, so nothing reached a phone and a retry is safe (TXT-014, D-2628).
pub(crate) async fn release(db: &mut PgConnection, id: Uuid) -> Result<(), OutboxError> {
    sqlx::query!(
        "UPDATE messaging.outbox SET status = 'queued', sending_since = NULL \
         WHERE id = $1 AND status = 'sending'",
        id
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| OutboxError::Database)
}

/// Moves row `id` from `sending` to `unknown` and drops its body in the same statement, returning
/// what it was and its `language`; with `stale_before`, only a row claimed before then (a crash).
/// `None` when the row is no longer `sending` (TXT-011, SEC-037, D-558, D-2628).
pub(crate) async fn mark_unknown(
    db: &mut PgConnection,
    id: Uuid,
    stale_before: Option<Timestamp>,
) -> Result<Option<(FailedText, Option<String>)>, OutboxError> {
    let row = sqlx::query!(
        "WITH old AS (SELECT id, body FROM messaging.outbox WHERE id = $1 AND status = 'sending' \
         AND ($2::timestamptz IS NULL OR sending_since IS NULL OR sending_since < $2) FOR UPDATE) \
         UPDATE messaging.outbox o SET status = 'unknown', body = NULL FROM old \
         WHERE o.id = old.id RETURNING o.recipient_id, o.recipient_kind, o.template_id, \
         o.subject_ids, o.language, old.body AS \"body!\"",
        id,
        stale_before.map(|t| t.to_sqlx()) as _
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(row.map(|r| {
        let text = FailedText {
            recipient: r.recipient_id,
            kind: r.recipient_kind,
            template_id: r.template_id,
            subject: r.subject_ids,
            body: r.body,
        };
        (text, r.language)
    }))
}

/// The rows still `sending` that were claimed before `before` (or with no claim time), at most
/// `limit`, oldest first: a sender died or lost its answer (D-2628).
pub(crate) async fn stale_sending(
    db: &mut PgConnection,
    before: Timestamp,
    limit: i64,
) -> Result<Vec<Uuid>, OutboxError> {
    sqlx::query_scalar!(
        "SELECT id FROM messaging.outbox WHERE status = 'sending' \
         AND (sending_since IS NULL OR sending_since < $1) ORDER BY sending_since LIMIT $2",
        before.to_sqlx() as _,
        limit
    )
    .fetch_all(db)
    .await
    .map_err(|_| OutboxError::Database)
}

/// One homeowner row the sender may try now: its ID, template, queue time and language, and
/// whether its QH-004 exemption still holds at the instant asked about.
pub(crate) struct Due {
    pub(crate) id: Uuid,
    pub(crate) template_id: String,
    pub(crate) recipient: Uuid,
    pub(crate) queued_at: Timestamp,
    pub(crate) exempt: bool,
    pub(crate) language: Option<String>,
}

/// The queued homeowner rows whose `not_before` has passed at `now` (or is unset), at most
/// `limit`, oldest first (QH-006, D-2628). Business rows wait: their phone is not resolved here.
pub(crate) async fn due(
    db: &mut PgConnection,
    now: Timestamp,
    limit: i64,
) -> Result<Vec<Due>, OutboxError> {
    let rows = sqlx::query!(
        "SELECT id, template_id, recipient_id, created_at, language, \
         COALESCE(quiet_exempt_until > $1, false) AS \"exempt!\" FROM messaging.outbox \
         WHERE status = 'queued' AND recipient_kind = 'homeowner' \
         AND (not_before IS NULL OR not_before <= $1) ORDER BY created_at LIMIT $2",
        now.to_sqlx() as _,
        limit
    )
    .fetch_all(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(rows
        .into_iter()
        .map(|r| Due {
            id: r.id,
            template_id: r.template_id,
            recipient: r.recipient_id,
            queued_at: r.created_at.to_jiff(),
            exempt: r.exempt,
            language: r.language,
        })
        .collect())
}

/// Sets queued row `id`'s `not_before` to `until`: held by quiet hours or waiting for a retry
/// (QH-006, QH-008, TXT-014).
pub(crate) async fn hold(
    db: &mut PgConnection,
    id: Uuid,
    until: Timestamp,
) -> Result<(), OutboxError> {
    sqlx::query!(
        "UPDATE messaging.outbox SET not_before = $2 WHERE id = $1 AND status = 'queued'",
        id,
        until.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| OutboxError::Database)
}

/// Marks queued row `id` `expired` and drops its body; true when it moved (D-2188, CON-030).
pub(crate) async fn expire(db: &mut PgConnection, id: Uuid) -> Result<bool, OutboxError> {
    sqlx::query!(
        "UPDATE messaging.outbox SET status = 'expired', body = NULL \
         WHERE id = $1 AND status = 'queued'",
        id
    )
    .execute(db)
    .await
    .map(|r| r.rows_affected() > 0)
    .map_err(|_| OutboxError::Database)
}

/// Whether the homeowner line was paused (switched off, suspended or restored but not yet on) at
/// `at`, by its latest `messaging.mode_state` row then; none means on (TXT-021, D-2188).
pub(crate) async fn homeowner_paused_at(
    db: &mut PgConnection,
    at: Timestamp,
) -> Result<bool, OutboxError> {
    let open = sqlx::query_scalar!(
        "SELECT texting_on AND line_state = 'working' FROM messaging.mode_state \
         WHERE audience = 'homeowner' AND changed_at <= $1 ORDER BY changed_at DESC, seq DESC LIMIT 1",
        at.to_sqlx() as _
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(open.flatten() == Some(false))
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
/// blind-index digest, `inbox` the inbound text that carried a texted opt-out (kept with the
/// event after its body is cut, CON-036, D-1853) and `at` the instant suppression took effect.
pub(crate) struct OptOutEvent<'a> {
    pub(crate) action: &'static str,
    pub(crate) channel: &'static str,
    pub(crate) detected: &'a str,
    pub(crate) inbox: Option<Uuid>,
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
         (action, channel, detected, index_version, digest, effective_at, inbox_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        event.action,
        event.channel,
        event.detected,
        i16::try_from(version).map_err(|_| OutboxError::Database)?,
        &digest[..],
        event.at.to_sqlx() as _,
        event.inbox
    )
    .fetch_one(db)
    .await
    .map_err(|_| OutboxError::Database)
}

/// The number with `digests`'s current listing on the suppression list, if it is listed: the
/// earliest row's time and the event that listed it (CON-035).
pub(crate) async fn listing(
    db: &mut PgConnection,
    digests: &[(u16, [u8; 32])],
) -> Result<Option<Listing>, OutboxError> {
    let (versions, hashes) = split_digests(digests);
    let row = sqlx::query!(
        "SELECT s.since, s.event_id FROM messaging.suppression s \
         JOIN UNNEST($1::smallint[], $2::bytea[]) AS k (v, h) \
         ON s.index_version = k.v AND s.digest = k.h \
         ORDER BY s.since, s.event_id LIMIT 1",
        &versions,
        &hashes
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(row.map(|r| Listing {
        since: r.since.to_jiff(),
        event: r.event_id,
    }))
}

/// Puts the number with `digests` on the suppression list since `at`, by opt-out event `event`
/// (CON-035). A version already listed keeps its earlier row. Returns how many rows it added: a
/// concurrent opt-out that listed the number first leaves this one adding none, as the insert
/// waits for that transaction's rows (D-870).
pub(crate) async fn put_suppression(
    db: &mut PgConnection,
    digests: &[(u16, [u8; 32])],
    event: Uuid,
    at: Timestamp,
) -> Result<u64, OutboxError> {
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
    .map(|done| done.rows_affected())
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

/// Outbox row `id`'s recipient ID, template ID and subject IDs, if the row exists (CON-035).
pub(crate) async fn outbox_target(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<Option<(Uuid, String, Vec<Uuid>)>, OutboxError> {
    let row = sqlx::query!(
        "SELECT recipient_id, template_id, subject_ids FROM messaging.outbox WHERE id = $1",
        id
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(row.map(|r| (r.recipient_id, r.template_id, r.subject_ids)))
}

/// Marks row `id` blocked and clears its body: the carrier refused it with 21610, so it is never
/// claimed again (CON-014, SEC-037).
pub(crate) async fn block(db: &mut PgConnection, id: Uuid) -> Result<(), OutboxError> {
    sqlx::query!(
        "UPDATE messaging.outbox SET status = 'blocked', body = NULL \
         WHERE id = $1 AND status IN ('queued', 'sending')",
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

/// Why an email call failed. No variant carries an address or a body (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum EmailError {
    /// The LLC postal address is empty or not a P.O. box or private mailbox (SEC-028).
    #[error("the postal address is not a P.O. box or private mailbox")]
    PostalAddress,
    /// The daily limit is zero or its share is outside 1 to 100 (OPS-038).
    #[error("the email limit setting is out of range")]
    Limit,
    /// The email layout did not render.
    #[error("the email could not be rendered")]
    Template,
    /// No address is on file for the recipient, or the vault refused the read.
    #[error("no email address on file")]
    NoAddress,
    /// The row was queued for the other kind of recipient; it is left as it was.
    #[error("the email was queued for another kind of recipient")]
    RecipientKind,
    /// The provider refused the send or its outcome is unknown; a resend carries the same key.
    #[error("the email provider did not confirm the send")]
    Provider,
    /// The email outbox has no row with this ID.
    #[error("no such email")]
    NotFound,
    /// The webhook is not the email provider's, has no message ID, or its body does not parse;
    /// nothing was stored.
    #[error("the webhook is not a well-formed email-provider event")]
    BadWebhook,
    /// The database refused or dropped the statement.
    #[error("the messaging database statement failed")]
    Database,
}

/// One email's notice-log state (BIZ-045, OPS-038).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailLog {
    /// Whether OPS-038 may hold it.
    pub bulk: bool,
    /// When it was queued.
    pub queued_at: Timestamp,
    /// It is not sent before this time.
    pub not_before: Timestamp,
    /// What it waited for last, if anything: `daily_limit` or `provider_limit`.
    pub waiting_for: Option<String>,
    /// When the provider took it; a window counted from the send starts here (D-381).
    pub sent_at: Option<Timestamp>,
}

/// A claimed email: its key's IDs and its rendered parts.
pub(crate) struct ClaimedEmail {
    pub(crate) template_id: String,
    pub(crate) recipient: Uuid,
    pub(crate) subject: Vec<Uuid>,
    pub(crate) rendered: crate::email::Rendered,
}

/// Inserts one email under `key` unless it is already queued; returns its ID and whether it is new.
pub(crate) async fn insert_email(
    db: &mut PgConnection,
    (key, (template, to, subject), kind, bulk): (&str, (&str, Uuid, &[Uuid]), &str, bool),
    r: &crate::email::Rendered,
    now: Timestamp,
) -> Result<(Uuid, bool), EmailError> {
    let row = sqlx::query!(
        "WITH new AS (INSERT INTO messaging.email_outbox (idempotency_key, template_id, \
         recipient_id, recipient_kind, subject_ids, class, subject, html, plain, not_before, \
         queued_at) VALUES ($1, $2, $3, $4, $5, CASE WHEN $6 THEN 'bulk' ELSE 'notice' END, $7, \
         $8, $9, $10, $10) ON CONFLICT (idempotency_key) DO NOTHING RETURNING id) \
         SELECT id AS \"id!\", true AS \"new!\" FROM new UNION ALL SELECT id, false \
         FROM messaging.email_outbox WHERE idempotency_key = $1 AND NOT EXISTS (SELECT 1 FROM new)",
        key,
        template,
        to,
        kind,
        subject,
        bulk,
        r.subject,
        r.html,
        r.plain,
        now.to_sqlx() as _
    )
    .fetch_one(db)
    .await
    .map_err(|_| EmailError::Database)?;
    Ok((row.id, row.new))
}

/// How many emails were sent from `since` to `until`, both included (the day's count, OPS-038;
/// the sends a webhook should have followed, SEC-051).
pub(crate) async fn emails_sent_between(
    db: &mut PgConnection,
    (since, until): (Timestamp, Timestamp),
) -> Result<i64, EmailError> {
    sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM messaging.email_outbox \
         WHERE status = 'sent' AND sent_at >= $1 AND sent_at <= $2",
        since.to_sqlx() as _,
        until.to_sqlx() as _
    )
    .fetch_one(db)
    .await
    .map_err(|_| EmailError::Database)
}

/// Claims unsent, due email `id` for one attempt if it was queued for `kind` and is not a bulk
/// email held while `hold_bulk` (OPS-038).
pub(crate) async fn claim_email(
    db: &mut PgConnection,
    id: Uuid,
    (kind, hold_bulk): (&str, bool),
    now: Timestamp,
) -> Result<Option<ClaimedEmail>, EmailError> {
    let row = sqlx::query!(
        "UPDATE messaging.email_outbox SET status = 'sending', attempts = attempts + 1 \
         WHERE id = $1 AND status IN ('queued', 'sending') AND recipient_kind = $2 \
         AND not_before <= $3 AND NOT (class = 'bulk' AND $4) \
         RETURNING template_id, recipient_id, subject_ids, subject, html, plain",
        id,
        kind,
        now.to_sqlx() as _,
        hold_bulk
    )
    .fetch_optional(db)
    .await
    .map_err(|_| EmailError::Database)?;
    row.map(|r| {
        let rendered = crate::email::Rendered {
            subject: r.subject.ok_or(EmailError::Database)?,
            html: r.html.ok_or(EmailError::Database)?,
            plain: r.plain.ok_or(EmailError::Database)?,
        };
        Ok(ClaimedEmail {
            template_id: r.template_id,
            recipient: r.recipient_id,
            subject: r.subject_ids,
            rendered,
        })
    })
    .transpose()
}

/// Marks email `id` sent at `now` with the provider's ID and clears its parts (SEC-037).
pub(crate) async fn mark_email_sent(
    db: &mut PgConnection,
    id: Uuid,
    provider_id: &str,
    now: Timestamp,
) -> Result<(), EmailError> {
    sqlx::query!(
        "UPDATE messaging.email_outbox SET status = 'sent', provider_id = $2, sent_at = $3, \
         subject = NULL, html = NULL, plain = NULL WHERE id = $1 AND status = 'sending'",
        id,
        provider_id,
        now.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| EmailError::Database)
}

/// Puts unsent email `id` back in the queue until `until`, recording what it waits for (OPS-038).
pub(crate) async fn hold_email(
    db: &mut PgConnection,
    id: Uuid,
    until: Timestamp,
    reason: &str,
) -> Result<(), EmailError> {
    sqlx::query!(
        "UPDATE messaging.email_outbox SET status = 'queued', not_before = $2, waiting_for = $3 \
         WHERE id = $1 AND status <> 'sent'",
        id,
        until.to_sqlx() as _,
        reason
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| EmailError::Database)
}

/// Email `id`'s notice-log state, if the row exists.
pub(crate) async fn email_row(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<Option<EmailLog>, EmailError> {
    let row = sqlx::query!(
        "SELECT class, queued_at, not_before, waiting_for, sent_at \
         FROM messaging.email_outbox WHERE id = $1",
        id
    )
    .fetch_optional(db)
    .await
    .map_err(|_| EmailError::Database)?;
    Ok(row.map(|r| EmailLog {
        bulk: r.class == "bulk",
        queued_at: r.queued_at.to_jiff(),
        not_before: r.not_before.to_jiff(),
        waiting_for: r.waiting_for,
        sent_at: r.sent_at.map(|t| t.to_jiff()),
    }))
}

/// Homeowner `contact`'s stored update channel, if she chose one (TXT-018).
pub(crate) async fn update_channel(
    db: &mut PgConnection,
    contact: Uuid,
) -> Result<Option<String>, EmailError> {
    sqlx::query_scalar!(
        "SELECT channel FROM messaging.update_channel WHERE recipient_id = $1",
        contact
    )
    .fetch_optional(db)
    .await
    .map_err(|_| EmailError::Database)
}

/// Stores homeowner `contact`'s update channel at `now` (TXT-018).
pub(crate) async fn put_update_channel(
    db: &mut PgConnection,
    contact: Uuid,
    channel: &str,
    now: Timestamp,
) -> Result<(), EmailError> {
    sqlx::query!(
        "INSERT INTO messaging.update_channel (recipient_id, channel, updated_at) \
         VALUES ($1, $2, $3) ON CONFLICT (recipient_id) \
         DO UPDATE SET channel = EXCLUDED.channel, updated_at = EXCLUDED.updated_at",
        contact,
        channel,
        now.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| EmailError::Database)
}

/// One text given up after its retries: who it was for and what it said (TXT-014).
pub(crate) struct FailedText {
    pub(crate) recipient: Uuid,
    pub(crate) kind: String,
    pub(crate) template_id: String,
    pub(crate) subject: Vec<Uuid>,
    pub(crate) body: String,
}

/// Counts outbox text `id`'s failed send at `now` (its claims so far, so a repeat call counts
/// nothing twice) unless its retries already ended; returns the failures and the first failure.
pub(crate) async fn record_failure(
    db: &mut PgConnection,
    id: Uuid,
    now: Timestamp,
) -> Result<Option<(i32, Timestamp)>, OutboxError> {
    let row = sqlx::query!(
        "INSERT INTO messaging.text_retry (outbox_id, failures, first_failed_at) \
         SELECT id, GREATEST(attempts, 1), $2 FROM messaging.outbox \
         WHERE id = $1 AND status IN ('queued', 'sending') \
         ON CONFLICT (outbox_id) DO UPDATE SET failures = GREATEST(EXCLUDED.failures, \
         messaging.text_retry.failures) WHERE messaging.text_retry.outcome IS NULL \
         RETURNING failures, first_failed_at",
        id,
        now.to_sqlx() as _
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(row.map(|r| (r.failures, r.first_failed_at.to_jiff())))
}

/// Records when text `id`'s next retry is due (TXT-015).
pub(crate) async fn set_next_retry(
    db: &mut PgConnection,
    id: Uuid,
    at: Timestamp,
) -> Result<(), OutboxError> {
    sqlx::query!(
        "UPDATE messaging.text_retry SET next_at = $2 WHERE outbox_id = $1",
        id,
        at.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| OutboxError::Database)
}

/// Marks unsent text `id` failed and clears its body, returning what it was (TXT-014, SEC-037);
/// `None` when it was sent, blocked or already failed.
pub(crate) async fn fail_text(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<Option<FailedText>, OutboxError> {
    let row = sqlx::query!(
        "WITH old AS (SELECT id, body FROM messaging.outbox \
         WHERE id = $1 AND status IN ('queued', 'sending') FOR UPDATE) \
         UPDATE messaging.outbox o SET status = 'failed', body = NULL FROM old WHERE o.id = old.id \
         RETURNING o.recipient_id, o.recipient_kind, o.template_id, o.subject_ids, \
         old.body AS \"body!\"",
        id
    )
    .fetch_optional(db)
    .await
    .map_err(|_| OutboxError::Database)?;
    Ok(row.map(|r| FailedText {
        recipient: r.recipient_id,
        kind: r.recipient_kind,
        template_id: r.template_id,
        subject: r.subject_ids,
        body: r.body,
    }))
}

/// Lists failed important text `id` for the recipient's portal at `now` (TXT-014).
pub(crate) async fn insert_portal_notice(
    db: &mut PgConnection,
    id: Uuid,
    text: &FailedText,
    now: Timestamp,
) -> Result<(), OutboxError> {
    sqlx::query!(
        "INSERT INTO messaging.portal_notice (outbox_id, recipient_id, template_id, subject_ids, \
         created_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (outbox_id) DO NOTHING",
        id,
        text.recipient,
        text.template_id,
        &text.subject,
        now.to_sqlx() as _
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| OutboxError::Database)
}

/// Records how text `id`'s retries ended: `emailed`, `call_back` (due at `due`) or `failed`.
pub(crate) async fn end_retry(
    db: &mut PgConnection,
    id: Uuid,
    (outcome, due): (&str, Option<Timestamp>),
) -> Result<(), OutboxError> {
    sqlx::query!(
        "UPDATE messaging.text_retry SET outcome = $2, call_back_due = $3, next_at = NULL \
         WHERE outbox_id = $1",
        id,
        outcome,
        due.map(|t| t.to_sqlx()) as _
    )
    .execute(db)
    .await
    .map(|_| ())
    .map_err(|_| OutboxError::Database)
}

/// Stores one provider event `kind` of email `provider_id`, at `event_at` by the provider's clock
/// and received at `now` (SEC-051); returns the outbox row it is about, if one is known.
pub(crate) async fn insert_email_event(
    db: &mut PgConnection,
    (provider_id, kind): (&str, &str),
    (event_at, now): (Timestamp, Timestamp),
) -> Result<Option<Uuid>, EmailError> {
    sqlx::query_scalar!(
        "WITH e AS (INSERT INTO messaging.email_event (provider_id, kind, event_at, received_at) \
         VALUES ($1, $2, $3, $4) RETURNING provider_id) \
         SELECT o.id AS \"id?\" FROM e LEFT JOIN messaging.email_outbox o ON o.provider_id = e.provider_id",
        provider_id,
        kind,
        event_at.to_sqlx() as _,
        now.to_sqlx() as _
    )
    .fetch_one(db)
    .await
    .map_err(|_| EmailError::Database)
}

/// The latest event kind of email `provider_id` by event time (SEC-051).
pub(crate) async fn latest_email_event(
    db: &mut PgConnection,
    provider_id: &str,
) -> Result<Option<String>, EmailError> {
    sqlx::query_scalar!(
        "SELECT kind FROM messaging.email_event WHERE provider_id = $1 \
         ORDER BY event_at DESC, id DESC LIMIT 1",
        provider_id
    )
    .fetch_optional(db)
    .await
    .map_err(|_| EmailError::Database)
}

/// How many provider webhooks arrived since `since` (SEC-051).
pub(crate) async fn email_events_since(
    db: &mut PgConnection,
    since: Timestamp,
) -> Result<i64, EmailError> {
    sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM messaging.email_event WHERE received_at >= $1",
        since.to_sqlx() as _
    )
    .fetch_one(db)
    .await
    .map_err(|_| EmailError::Database)
}
