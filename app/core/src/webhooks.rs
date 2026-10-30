//! The one webhook dedupe helper every module uses (SEC-017, SEC-039, D-491).
//!
//! `core` cannot write into another module's table, so the helper is a pattern plus two types.
//! Each module that takes a webhook owns `<module>.webhook_event` with the columns
//! `source text, event_key text, received_at timestamptz, PRIMARY KEY (source, event_key)`, kept
//! at least as long as the consent log (SEC-039), and in the transaction that applies the event
//! writes its own literal macro:
//!
//! ```sql
//! INSERT INTO <module>.webhook_event (source, event_key, received_at) VALUES ($1, $2, $3)
//! ON CONFLICT (source, event_key) DO NOTHING RETURNING event_key
//! ```
//!
//! fetched with `fetch_optional` and read with [`Dedupe::from_returning`]: on
//! [`Dedupe::Repeat`] it commits nothing more and answers the vendor 200, so the same event
//! arriving 100 times changes nothing after the first. The key is built by [`EventKey`]: the
//! vendor's event ID, or for Twilio (MessageSid, MessageStatus) and (CallSid, step) (SEC-039).

use thiserror::Error;

/// The longest part of an event key accepted, in characters.
const PART_MAX: usize = 128;

/// Why a webhook's event key was refused. It carries no value (PRIV-016).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WebhookError {
    /// A part is empty, longer than 128 characters, or not ASCII letters, digits, `_`, `-`, `.`.
    #[error("a webhook event key part has the wrong shape")]
    BadKey,
}

/// A parsed webhook event key: one or two vendor identifiers joined by `:`, each checked once at
/// the edge, so no free text or contact value can become a key (SEC-017, PRIV-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventKey(String);

impl EventKey {
    /// The key for a vendor event ID (a Stripe `evt_…`, a Svix `msg_…`, a Lob event ID).
    ///
    /// # Errors
    ///
    /// [`WebhookError::BadKey`].
    pub fn new(id: &str) -> Result<Self, WebhookError> {
        Ok(Self(checked(id)?.to_owned()))
    }

    /// The key for a pair, such as Twilio's (MessageSid, MessageStatus) for a status callback or
    /// (CallSid, step) for a voice request (SEC-039). `:` never occurs in a part, so two pairs
    /// never share a key.
    ///
    /// # Errors
    ///
    /// [`WebhookError::BadKey`].
    pub fn pair(first: &str, second: &str) -> Result<Self, WebhookError> {
        Ok(Self(format!("{}:{}", checked(first)?, checked(second)?)))
    }

    /// The key to bind as `event_key`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `part` if it is 1 to 128 ASCII letters, digits, `_`, `-` or `.`.
fn checked(part: &str) -> Result<&str, WebhookError> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.');
    if !part.is_empty() && part.len() <= PART_MAX && part.chars().all(allowed) {
        Ok(part)
    } else {
        Err(WebhookError::BadKey)
    }
}

/// Whether this delivery is the event's first (SEC-017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "a Repeat means the event was already applied and the caller must change nothing"]
pub enum Dedupe {
    /// The event was recorded now: apply its effect in the same transaction.
    First,
    /// The event was recorded before: change nothing and answer 200.
    Repeat,
}

impl Dedupe {
    /// Reads the module's `INSERT … ON CONFLICT DO NOTHING RETURNING …` fetched with
    /// `fetch_optional`: a row means first, none means repeat (SEC-017).
    pub fn from_returning<T>(row: Option<T>) -> Self {
        row.map_or(Self::Repeat, |_| Self::First)
    }

    /// True on the event's first delivery.
    #[must_use]
    pub fn is_first(self) -> bool {
        self == Self::First
    }
}
