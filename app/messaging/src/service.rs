//! The text outbox and the sender directory (TXT-009, TXT-011, D-492; rust brain R11, R13).
//!
//! [`enqueue`] checks TXT-009, derives the text's idempotency key from the template ID, the
//! recipient ID and the record IDs it is about, and stores it once with those IDs: a second
//! enqueue of the same text finds the first row. [`send`] claims the row in one short statement,
//! calls the carrier outside any transaction with the key re-derived from the stored IDs (D-847),
//! then marks it sent and clears its body (D-850). A worker killed between the carrier's acceptance
//! and the mark leaves it `sending`, and the resend carries the same key, which the carrier delivers
//! once (TXT-011). A sent row is never handed to the carrier again. A homeowner's number is read
//! from the vault at send time for the row's recipient ID, never taken from the caller (D-849);
//! each row records whether it was queued for a homeowner or a business, and a send naming the
//! other kind is refused, so a homeowner's row never skips the vault read or the TXT-009 check.
//! A send refused before the claim (TXT-009, wrong kind) leaves the row `queued`; a failed vault
//! read or carrier call leaves it `sending`. Neither is ever made terminal here: the retry task
//! (TXT-014, TXT-015) decides when an unsent row is given up.

use askhava_adapters::api::{IdempotencyKey, SentText, SmsAdapter};
use askhava_privacy::api::{
    Field, IndexPurpose, Purpose, Reader, Vault, normalize_phone, read_contact,
};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, OutboxError};

/// The action name every text's idempotency key is derived under (SEC-018).
pub const SEND_ACTION: &str = "sms.send";

/// Who the recipient is to Ask Hava, which decides whether a text may go (TXT-009).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    /// A business main or staff phone, texted about its own jobs.
    Business,
    /// A homeowner whose request is still open.
    OpenRequest,
    /// A homeowner whose last request closed at `closed_at`; `last_inbound` is the time of her
    /// latest text to Ask Hava, if any.
    PastCustomer {
        /// When her last request closed.
        closed_at: Timestamp,
        /// When she last wrote to Ask Hava.
        last_inbound: Option<Timestamp>,
    },
}

/// One text to queue: the template, the records it is about and the rendered body (TXT-011). Its
/// `Debug` leaves the body out, which may hold a link token (PRIV-016, SEC-037).
#[derive(Clone, Copy)]
pub struct NewText<'a> {
    /// The template ID it was rendered from (TXT-004).
    pub template_id: &'a str,
    /// The record IDs the text identifies (job, quote, pick, inbound message); with the template
    /// and the recipient they make the key, so pass every ID that makes the text distinct.
    pub subject: &'a [Uuid],
    /// The opaque recipient ID (a vault contact or a business address), never the number.
    pub recipient: Uuid,
    /// Who the recipient is (TXT-009).
    pub relation: Relation,
    /// The rendered body.
    pub body: &'a str,
}

impl std::fmt::Debug for NewText<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewText")
            .field("template_id", &self.template_id)
            .field("subject", &self.subject)
            .field("recipient", &self.recipient)
            .field("relation", &self.relation)
            .finish_non_exhaustive()
    }
}

impl Relation {
    /// The recipient kind stored on the row: `business` for [`Relation::Business`], else
    /// `homeowner` (D-849).
    fn kind(self) -> &'static str {
        match self {
            Self::Business => KIND_BUSINESS,
            Self::OpenRequest | Self::PastCustomer { .. } => KIND_HOMEOWNER,
        }
    }
}

/// The stored kind of a row queued for a homeowner's vault contact (D-849).
const KIND_HOMEOWNER: &str = "homeowner";
/// The stored kind of a row queued for a business main or staff phone (D-849).
const KIND_BUSINESS: &str = "business";

/// Where a queued text goes, resolved at send time; it must name the kind the row was queued for
/// (D-849).
#[derive(Clone, Copy)]
pub enum To<'a> {
    /// A homeowner: the row's recipient ID is her vault contact, whose phone is read under
    /// `messaging`'s send grant (PRIV-004); `relation` is checked again before the send (TXT-009).
    Homeowner {
        /// The vault the number is read from.
        vault: &'a Vault,
        /// Who she is to Ask Hava now (TXT-009).
        relation: Relation,
    },
    /// A business main or staff phone, not vault data; the caller reads it from `businesses`.
    Business(&'a str),
}

impl To<'_> {
    /// The recipient kind a row must have been queued for to go to this destination (D-849).
    fn kind(self) -> &'static str {
        match self {
            Self::Homeowner { .. } => KIND_HOMEOWNER,
            Self::Business(_) => KIND_BUSINESS,
        }
    }
}

/// Whether a text to someone with `relation` may go: never to a past customer unless she wrote
/// after her request closed (TXT-009).
#[must_use]
pub fn may_text(relation: Relation) -> bool {
    match relation {
        Relation::Business | Relation::OpenRequest => true,
        Relation::PastCustomer {
            closed_at,
            last_inbound,
        } => last_inbound.is_some_and(|at| at > closed_at),
    }
}

/// Queues `text` once (TXT-009, TXT-011): refused for a past customer who has not written first;
/// otherwise stored under its derived key, `secret` being the `ID_SECRET`, with the recipient kind
/// its `relation` implies (D-849). Returns the row ID and whether this call created it (`false`:
/// the same text was already queued).
///
/// # Errors
///
/// [`OutboxError::PastCustomer`] or [`OutboxError::Database`].
pub async fn enqueue(
    db: &mut PgConnection,
    secret: &SecretString,
    text: &NewText<'_>,
) -> Result<(Uuid, bool), OutboxError> {
    if !may_text(text.relation) {
        return Err(OutboxError::PastCustomer);
    }
    let ids = (text.template_id, text.recipient, text.subject);
    let key = text_key(secret, ids);
    models::insert_text(db, key.as_str(), ids, text.relation.kind(), text.body).await
}

/// The key of a text: [`SEND_ACTION`] over the template ID, the recipient ID and the subject IDs,
/// so the same text always gets the same key and no personal data is in it (SEC-018, TXT-011). The
/// subject IDs are sorted first, so the order a caller lists them in does not change the key.
fn text_key(
    secret: &SecretString,
    (template_id, recipient, subject): (&str, Uuid, &[Uuid]),
) -> IdempotencyKey {
    let recipient = recipient.to_string();
    let mut sorted = subject.to_vec();
    sorted.sort_unstable();
    let subject: Vec<String> = sorted.iter().map(Uuid::to_string).collect();
    let ids: Vec<&str> = [template_id, recipient.as_str()]
        .into_iter()
        .chain(subject.iter().map(String::as_str))
        .collect();
    IdempotencyKey::derive(secret, SEND_ACTION, &ids)
}

/// Sends outbox row `id` from `from` to `to` through `carrier` under the key re-derived from the
/// row's stored IDs, so every attempt carries the same key (TXT-011). A homeowner is checked again
/// against TXT-009 before the row is claimed, and her number read from the vault; `to` must name
/// the recipient kind the row was queued for (D-849). Returns `None` when the row was already sent:
/// the carrier is not called again. No transaction is open during the call (R11). No error makes
/// the row terminal; the retry task decides when to give it up (TXT-014, TXT-015).
///
/// # Errors
///
/// [`OutboxError::PastCustomer`] and [`OutboxError::RecipientKind`] (the row is left as it was,
/// normally `queued`), [`OutboxError::NotFound`], [`OutboxError::Recipient`] and
/// [`OutboxError::Carrier`] (the row stays `sending`; a resend carries the same key) or
/// [`OutboxError::Database`].
pub async fn send(
    db: &mut PgConnection,
    (secret, carrier): (&SecretString, &impl SmsAdapter),
    id: Uuid,
    (to, from): (To<'_>, &str),
) -> Result<Option<SentText>, OutboxError> {
    if let To::Homeowner { relation, .. } = to
        && !may_text(relation)
    {
        return Err(OutboxError::PastCustomer);
    }
    let Some(claim) = models::claim(db, id, to.kind()).await? else {
        return Ok(None);
    };
    let number = recipient_phone(db, to, claim.recipient).await?;
    let key = text_key(
        secret,
        (&claim.template_id, claim.recipient, &claim.subject),
    );
    let sent = carrier
        .send_text(&key, number.expose_secret(), from, &claim.body)
        .await
        .map_err(|_| OutboxError::Carrier)?;
    models::mark_sent(db, id, &sent.sid).await?;
    Ok(Some(sent))
}

/// The number to text: a homeowner's from the vault for contact `recipient` (PRIV-003, PRIV-004,
/// D-849), a business phone as the caller gave it.
async fn recipient_phone(
    db: &mut PgConnection,
    to: To<'_>,
    recipient: Uuid,
) -> Result<SecretString, OutboxError> {
    match to {
        To::Business(phone) => Ok(SecretString::from(phone.to_owned())),
        To::Homeowner { vault, .. } => read_contact(
            db,
            vault,
            (Reader::Messaging, Purpose::Send),
            recipient,
            &[Field::Phone],
        )
        .await
        .map_err(|_| OutboxError::Recipient)?
        .phone
        .ok_or(OutboxError::Recipient),
    }
}

/// Links phone `phone_ref` (`phone` its number) to `business` in the sender directory, replacing
/// any earlier number of that phone; T220 calls it for every main and staff phone added or changed
/// (D-492). The phone's earlier rows are replaced whatever business they named, so re-adding a
/// `phone_ref` under another business moves it there; the caller owns which business a phone
/// record belongs to. Only blind-index digests are stored, never the number (PRIV-002).
///
/// # Errors
///
/// [`OutboxError::BadPhone`], [`OutboxError::SenderTaken`] or [`OutboxError::Database`].
pub async fn sender_add(
    db: &mut PgConnection,
    vault: &Vault,
    (business, phone_ref): (Uuid, Uuid),
    phone: &str,
) -> Result<(), OutboxError> {
    let digests = sender_digests(vault, phone)?;
    let mut tx = db.begin().await.map_err(|_| OutboxError::Database)?;
    models::put_sender(&mut tx, (business, phone_ref), &digests).await?;
    tx.commit().await.map_err(|_| OutboxError::Database)
}

/// Unlinks phone `phone_ref` from the sender directory; T220 calls it when a phone is removed
/// (D-492). Removing an unknown phone is not an error.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn sender_remove(db: &mut PgConnection, phone_ref: Uuid) -> Result<(), OutboxError> {
    models::delete_sender(db, phone_ref).await
}

/// The business an inbound text from `phone` belongs to, if the number is a linked main or staff
/// phone (D-492).
///
/// # Errors
///
/// [`OutboxError::BadPhone`] or [`OutboxError::Database`].
pub async fn sender_business(
    db: &mut PgConnection,
    vault: &Vault,
    phone: &str,
) -> Result<Option<Uuid>, OutboxError> {
    models::find_sender(db, &sender_digests(vault, phone)?).await
}

/// The digests of `phone` under the per-purpose `sender-dir` index key, one per key version, so a
/// number still matches during an index-key rotation (PRIV-024).
fn sender_digests(vault: &Vault, phone: &str) -> Result<Vec<(u16, [u8; 32])>, OutboxError> {
    let phone = normalize_phone(phone).map_err(|_| OutboxError::BadPhone)?;
    Ok(vault.blind_index(IndexPurpose::SenderDir, &phone))
}
