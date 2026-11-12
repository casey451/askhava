//! The text outbox and the sender directory (TXT-009, TXT-011, D-492; rust brain R11, R13).
//!
//! [`enqueue`] checks TXT-009, derives the text's idempotency key from the template ID, the
//! recipient ID and the record IDs it is about, and stores it once with those IDs and its QH-004
//! exemption (an any-hour template, or [`enqueue_exempt`]'s emergency job or direct reply): a
//! second enqueue of the same text finds the first row. [`send`] claims a `queued` row in one
//! short statement (`sending`, `sending_since`), calls the carrier outside any transaction with
//! the key re-derived from the stored IDs (D-847), then marks it sent and clears its body (D-850).
//! Twilio does not dedupe by key (D-558), so a row is never resent blind: a definite refusal puts
//! it back to `queued` for the TXT-014 retries, an unknown outcome leaves it `sending`, and
//! `outbox_sender` moves it, and any row a dead worker left `sending`, to `unknown` (D-2628).
//! A homeowner's number is read from the vault at send time for the row's recipient ID, never
//! taken from the caller (D-849); each row records whether it was queued for a homeowner or a
//! business, and a send naming the other kind is refused, so a homeowner's row never skips the
//! vault read or the TXT-009 check. A send refused before the claim (TXT-009, wrong kind) leaves
//! the row `queued`; a failed vault read puts it back to `queued`. [`delivery_status`] gives a
//! text's status from its row and the status callbacks recorded for its provider SID (TXT-013).

use askhava_adapters::api::{AdapterError, IdempotencyKey, SentText, SmsAdapter};
use askhava_privacy::api::{
    Field, IndexPurpose, Purpose, Reader, Vault, normalize_phone, read_contact,
};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

use crate::models::{self, OutboxError, QuietExempt};
use crate::status::DeliveryStatus;

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
/// its `relation` implies (D-849). A template in [`ANY_HOUR_TEMPLATES`] is stored exempt from
/// quiet hours at any hour (QH-004); every other row waits for the window at send time unless it
/// came through [`enqueue_exempt`] (D-2628, D-3106). Returns the row ID and whether this call
/// created it (`false`: the same text was already queued).
///
/// # Errors
///
/// [`OutboxError::PastCustomer`] or [`OutboxError::Database`].
pub async fn enqueue(
    db: &mut PgConnection,
    secret: &SecretString,
    text: &NewText<'_>,
) -> Result<(Uuid, bool), OutboxError> {
    let exempt = if ANY_HOUR_TEMPLATES.contains(&text.template_id) {
        QuietExempt::Always
    } else {
        QuietExempt::None
    };
    insert(db, secret, text, exempt).await
}

/// The text kinds QH-004 lets go at any hour, by template ID: the INT-004 911 text, the STOP,
/// HELP and START replies and the INT-033 after-hours notice (D-2628). [`enqueue`] stores them
/// exempt, so no producer passes anything.
pub const ANY_HOUR_TEMPLATES: [&str; 7] = [
    "emergency_911",
    "help_reply",
    "opt_out_confirmation",
    "start_reply",
    "night_count_many",
    "night_count_one",
    "night_count_none",
];

/// The QH-004 facts a template ID cannot show (D-2628, D-3106).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exemption {
    /// An emergency job's quote, quote-count, question or pick-step text: any hour.
    EmergencyJob,
    /// A direct reply to the person's own text that arrived at `inbound_at`: any hour for 15
    /// minutes from then, after that only in the window (`rules::retry`).
    DirectReply {
        /// When her own text arrived.
        inbound_at: Timestamp,
    },
}

/// Queues `text` as [`enqueue`] does, with the QH-004 exemption `exemption` stored on the row, so
/// the sender lets it go outside the window (QH-004, D-2628). `NewText` is unchanged.
///
/// # Errors
///
/// As [`enqueue`].
pub async fn enqueue_exempt(
    db: &mut PgConnection,
    secret: &SecretString,
    text: &NewText<'_>,
    exemption: Exemption,
) -> Result<(Uuid, bool), OutboxError> {
    let exempt = match exemption {
        Exemption::EmergencyJob => QuietExempt::Always,
        Exemption::DirectReply { inbound_at } => {
            QuietExempt::Until(inbound_at + SignedDuration::from_mins(REPLY_MINUTES))
        }
    };
    insert(db, secret, text, exempt).await
}

/// How long a direct reply may go outside the window after the person's own text (QH-004).
const REPLY_MINUTES: i64 = 15;

/// TXT-009, TXT-011: the shared insert of [`enqueue`] and [`enqueue_exempt`].
async fn insert(
    db: &mut PgConnection,
    secret: &SecretString,
    text: &NewText<'_>,
    exempt: QuietExempt,
) -> Result<(Uuid, bool), OutboxError> {
    if !may_text(text.relation) {
        return Err(OutboxError::PastCustomer);
    }
    let ids = (text.template_id, text.recipient, text.subject);
    let key = text_key(secret, ids);
    let keys = (key.as_str(), text.relation.kind());
    models::insert_text(db, keys, ids, text.body, exempt).await
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
/// row's stored IDs (TXT-011). A homeowner is checked again against TXT-009 before the row is
/// claimed, and her number read from the vault; `to` must name the recipient kind the row was
/// queued for (D-849). Only a `queued` row is claimed, so a row already sent, being sent or in an
/// unknown state is never handed to the carrier again: `None` (D-558, D-2628). No transaction is
/// open during the call (R11).
///
/// # Errors
///
/// [`OutboxError::PastCustomer`] and [`OutboxError::RecipientKind`] (the row is left as it was,
/// normally `queued`), [`OutboxError::NotFound`], [`OutboxError::Recipient`] and
/// [`OutboxError::Carrier`] (a definite refusal: the row is `queued` again for the TXT-014
/// retries), [`OutboxError::OutcomeUnknown`] (the carrier may have taken it: the row stays
/// `sending` and is never resent blind) or [`OutboxError::Database`].
pub async fn send(
    db: &mut PgConnection,
    keys: (&SecretString, &impl SmsAdapter),
    id: Uuid,
    dest: (To<'_>, &str),
) -> Result<Option<SentText>, OutboxError> {
    send_at(db, keys, id, dest, (None, None)).await
}

/// [`send`], recording the claim at `at` (the caller's clock) when given, else the database's now;
/// `phone` is the recipient's number when the caller already read it from the vault for this row
/// (`optout::send_checked`, D-3149), else it is read here after the claim.
pub(crate) async fn send_at(
    db: &mut PgConnection,
    (secret, carrier): (&SecretString, &impl SmsAdapter),
    id: Uuid,
    (to, from): (To<'_>, &str),
    (at, phone): (Option<Timestamp>, Option<&SecretString>),
) -> Result<Option<SentText>, OutboxError> {
    if let To::Homeowner { relation, .. } = to
        && !may_text(relation)
    {
        return Err(OutboxError::PastCustomer);
    }
    let Some(claim) = models::claim(db, id, to.kind(), at).await? else {
        return Ok(None);
    };
    let read = match phone {
        Some(phone) => Ok(phone.clone()),
        None => recipient_phone(db, to, claim.recipient).await,
    };
    let number = match read {
        Ok(number) => number,
        Err(e) => {
            models::release(db, id).await?;
            return Err(e);
        }
    };
    let key = text_key(
        secret,
        (&claim.template_id, claim.recipient, &claim.subject),
    );
    match carrier
        .send_text(&key, number.expose_secret(), from, &claim.body)
        .await
    {
        Ok(sent) => {
            models::mark_sent(db, id, &sent.sid).await?;
            Ok(Some(sent))
        }
        Err(e) if refused(&e) => {
            models::release(db, id).await?;
            Err(OutboxError::Carrier)
        }
        Err(_) => Err(OutboxError::OutcomeUnknown),
    }
}

/// TXT-011, D-558: whether the carrier's error proves nothing was sent: the request never left, or
/// the vendor answered with a 4xx refusal. A timeout, a lost or unreadable answer and a 5xx may
/// hide an accepted send, so they are an unknown outcome.
fn refused(error: &AdapterError) -> bool {
    match error {
        AdapterError::NotSent | AdapterError::InvalidId | AdapterError::BadConfig => true,
        AdapterError::Vendor { status, .. } => (400..500).contains(status),
        _ => false,
    }
}

/// The delivery status of outbox row `id` (TXT-013): `Queued` or `Sending` while the row is, and
/// once sent the furthest status Twilio reported for its SID (`Sent` if none yet). A callback
/// that arrives late with an earlier status never moves it back; between two final ones the later
/// wins.
///
/// # Errors
///
/// [`OutboxError::NotFound`] or [`OutboxError::Database`].
pub async fn delivery_status(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<DeliveryStatus, OutboxError> {
    let (status, sid) = models::outbox_state(db, id)
        .await?
        .ok_or(OutboxError::NotFound)?;
    let sid = match (status.as_str(), sid) {
        ("queued", _) => return Ok(DeliveryStatus::Queued),
        ("sent", Some(sid)) => sid,
        _ => return Ok(DeliveryStatus::Sending),
    };
    let events = models::delivery_events(db, &sid).await?;
    Ok(events
        .iter()
        .filter_map(|e| DeliveryStatus::from_twilio(e))
        .fold(DeliveryStatus::Sent, |now, next| {
            if next.rank() >= now.rank() { next } else { now }
        }))
}

/// The number to text: a homeowner's from the vault for contact `recipient` (PRIV-003, PRIV-004,
/// D-849), a business phone as the caller gave it. `optout::send_checked` reads the number it
/// checks against the suppression list here too, so both see the same number (D-1362).
pub(crate) async fn recipient_phone(
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
