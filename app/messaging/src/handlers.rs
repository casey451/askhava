//! Twilio's inbound-text and status webhooks, de-duplicated by message ID (TXT-012, TXT-013;
//! SEC-017, SEC-039).
//!
//! Each function takes a [`VerifiedWebhook`], so only a delivery whose signature was checked is
//! ever recorded, and parses its fields once here. [`receive_inbound`] records an inbound text's
//! `MessageSid` in `messaging.webhook_event` and, on its first delivery only, makes its
//! `messaging.inbox` row; a repeat changes nothing. [`receive_status`] records a status callback
//! under (MessageSid, MessageStatus) and, on its first delivery, appends a
//! `messaging.delivery_event` that [`crate::service::delivery_status`] reads. Both open a
//! transaction (a savepoint inside the caller's own), so the dedupe row and its effect commit
//! together. Every `received_at` is `now`, which the caller takes from its injected `Clock`
//! (D-863). The contract for the routing task that wires these to Twilio's webhooks (none exists
//! yet, D-861): answer Twilio 200 whatever the outcome, except on a database error.

use askhava_adapters::api::{VerifiedWebhook, WebhookVendor};
use askhava_core::api::EventKey;
use jiff::Timestamp;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, InboxError};
use crate::status::DeliveryStatus;

/// The dedupe source of an inbound text, keyed on its MessageSid (TXT-012).
const INBOUND_SOURCE: &str = "twilio_inbound";
/// The dedupe source of a status callback, keyed on (MessageSid, MessageStatus) (SEC-039).
const STATUS_SOURCE: &str = "twilio_status";

/// What a status callback did (TXT-013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Callback {
    /// Its first delivery: the status was recorded.
    Recorded(DeliveryStatus),
    /// The same (MessageSid, MessageStatus) was recorded before; nothing changed (TXT-012).
    Repeat,
    /// A status that is not an outbound text's (such as `received`); nothing recorded.
    Ignored,
}

/// Records one verified inbound text (TXT-012): `Some(inbox row ID)` on its first delivery,
/// `None` when the same MessageSid came before, which changes nothing. `now` is the instant from
/// the caller's injected clock, stored as each row's `received_at` (D-863).
///
/// # Errors
///
/// [`InboxError::BadWebhook`] (not Twilio, or no well-formed MessageSid) or
/// [`InboxError::Database`].
pub async fn receive_inbound(
    db: &mut PgConnection,
    hook: &VerifiedWebhook,
    now: Timestamp,
) -> Result<Option<Uuid>, InboxError> {
    let sid = message_sid(hook)?;
    let key = EventKey::new(sid).map_err(|_| InboxError::BadWebhook)?;
    let mut tx = db.begin().await.map_err(|_| InboxError::Database)?;
    let id = if models::first_event(&mut tx, (INBOUND_SOURCE, key.as_str()), now).await? {
        Some(models::insert_inbox(&mut tx, sid, now).await?)
    } else {
        None
    };
    tx.commit().await.map_err(|_| InboxError::Database)?;
    Ok(id)
}

/// Records one verified status callback (TXT-012, TXT-013): its first delivery per
/// (MessageSid, MessageStatus) appends the folded status and Twilio's `ErrorCode`, if any, received
/// at `now` from the caller's injected clock (D-863).
///
/// # Errors
///
/// [`InboxError::BadWebhook`] (not Twilio, no well-formed MessageSid or MessageStatus, or an
/// `ErrorCode` that is not a positive number) or [`InboxError::Database`].
pub async fn receive_status(
    db: &mut PgConnection,
    hook: &VerifiedWebhook,
    now: Timestamp,
) -> Result<Callback, InboxError> {
    let sid = message_sid(hook)?;
    let raw = field(hook, "MessageStatus").ok_or(InboxError::BadWebhook)?;
    let key = EventKey::pair(sid, raw).map_err(|_| InboxError::BadWebhook)?;
    let error_code = error_code(hook)?;
    let Some(status) = DeliveryStatus::from_twilio(raw) else {
        return Ok(Callback::Ignored);
    };
    let mut tx = db.begin().await.map_err(|_| InboxError::Database)?;
    let outcome = if models::first_event(&mut tx, (STATUS_SOURCE, key.as_str()), now).await? {
        models::insert_delivery(&mut tx, sid, (status, error_code), now).await?;
        Callback::Recorded(status)
    } else {
        Callback::Repeat
    };
    tx.commit().await.map_err(|_| InboxError::Database)?;
    Ok(outcome)
}

/// The value of form field `name`, if present.
fn field<'a>(hook: &'a VerifiedWebhook, name: &str) -> Option<&'a str> {
    let found = hook.params().iter().find(|(n, _)| n == name);
    found.map(|(_, value)| value.as_str())
}

/// The delivery's MessageSid, refused unless it is Twilio's and shaped `SM`/`MM` plus 32 letters
/// or digits (TXT-012).
fn message_sid(hook: &VerifiedWebhook) -> Result<&str, InboxError> {
    if hook.vendor() != WebhookVendor::Twilio {
        return Err(InboxError::BadWebhook);
    }
    let sid = field(hook, "MessageSid").ok_or(InboxError::BadWebhook)?;
    let well_formed = sid.len() == 34
        && (sid.starts_with("SM") || sid.starts_with("MM"))
        && sid.bytes().all(|b| b.is_ascii_alphanumeric());
    well_formed.then_some(sid).ok_or(InboxError::BadWebhook)
}

/// Twilio's `ErrorCode`, if sent: a positive number, or the delivery is refused.
fn error_code(hook: &VerifiedWebhook) -> Result<Option<i32>, InboxError> {
    match field(hook, "ErrorCode") {
        None | Some("") => Ok(None),
        Some(code) => match code.parse::<i32>() {
            Ok(n) if n > 0 => Ok(Some(n)),
            _ => Err(InboxError::BadWebhook),
        },
    }
}
