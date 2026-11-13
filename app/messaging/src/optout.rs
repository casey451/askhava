//! The one suppression list and the opt-out flow (CON-008 to CON-011, CON-013 to CON-017, CON-030,
//! CON-035, CON-036).
//!
//! [`on_inbound_text`] runs the rules' detector on an inbound text (its scrubbed words) with the
//! provider's `OptOutType`: an opt-out puts the number on `messaging.suppression` at once and logs
//! the event with its channel, how it was detected, the inbox row that carried it (kept after the
//! body is cut, CON-036) and when (a repeat is logged too); the app
//! queues the one CON-009 confirmation only for words the provider did not answer, only on the
//! first opt-out, and never
//! while texting is paused (the caller emails it instead, CON-030). The provider's START or UNSTOP
//! (`OptOutType=START`) takes the number off (CON-015); anything else from a listed number sends
//! nothing, and an emergency in it, or in the opt-out itself, raises a critical alert (CON-016,
//! CON-017). [`suppress`] is the entry for every other channel
//! (email, email reply, help form, portal switch, owner-recorded). [`send_checked`] is the one way
//! a queued text goes out: it refuses a listed number except the confirmation within 5 minutes and
//! the HELP reply (CON-013), and a 21610 refusal lists the number and blocks the row, which is
//! never retried (CON-014). The number is only ever held as blind-index digests (PRIV-002).
//! CON-032's pre-Gate C docs test maps the FCC order's items to the tests that prove this file:
//! TD-CON-04 (`tests::optout_words`), TD-CON-05, TD-CON-13 and TD-CON-20
//! (`tests/integration/tests/optout_flow.rs`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use askhava_core::api::{Claimed, HandlerError, NewTask, enqueue as enqueue_task};

use askhava_adapters::api::{AdapterError, IdempotencyKey, SentText, SmsAdapter};
use askhava_ops::api::{NewAlert, Severity, Switch, is_paused, raise_alert};
use askhava_privacy::api::{IndexPurpose, Vault, normalize_phone};
use jiff::Timestamp;
use secrecy::{ExposeSecret, SecretString};
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::mode_state::load_lines;
use crate::models::{self, OptOutEvent, OutboxError};
use crate::optout_email::{
    OptOutEmailSender, queue_optout_confirmation_email, queue_optout_wrote_again,
};
use crate::rules::{self, CONFIRMATION_TEMPLATE, Classified, ProviderKeyword};
use crate::service::{self, NewText, Relation, To};
use crate::templates::{Language, Registry};

/// Twilio's error code for a send to a number that opted out (CON-014).
const OPTED_OUT_CODE: i64 = 21610;
/// The alert kind raised for a possible emergency from an opted-out number (CON-017).
const EMERGENCY_ALERT: &str = "messaging.opted_out_emergency";

/// A way a homeowner can revoke texting consent; each feeds the one list (CON-035).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeChannel {
    /// A text (CON-008, CON-010).
    Text,
    /// An email to the help address.
    Email,
    /// A reply to an Ask Hava email.
    EmailReply,
    /// A message on the help page's form.
    HelpForm,
    /// The homeowner portal's texts-off switch (PRIV-021).
    Portal,
    /// A request the owner recorded in the owner portal.
    Owner,
    /// The carrier refused a send with 21610 (CON-014).
    Carrier,
}

impl RevokeChannel {
    /// The stored channel name.
    fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Email => "email",
            Self::EmailReply => "email_reply",
            Self::HelpForm => "help_form",
            Self::Portal => "portal",
            Self::Owner => "owner",
            Self::Carrier => "carrier",
        }
    }
}

/// One inbound text, already recorded in the inbox and scrubbed. `Debug` leaves out the phone.
pub struct InboundText<'a> {
    /// Its `messaging.inbox` row, the subject of any alert (CON-017).
    pub inbox_id: Uuid,
    /// The sender's vault contact ID, to whom a confirmation goes.
    pub contact: Uuid,
    /// Who the sender is to Ask Hava (TXT-009).
    pub relation: Relation,
    /// The `From` number.
    pub phone: &'a str,
    /// The scrubbed words (`privacy::api::read_inbound`).
    pub text: &'a str,
    /// What the provider's Advanced Opt-Out matched (`OptOutType`, CON-030).
    pub provider: ProviderKeyword,
    /// Whether the reader's emergency flag is set (CON-017).
    pub emergency: bool,
}

impl std::fmt::Debug for InboundText<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InboundText")
            .field("inbox_id", &self.inbox_id)
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}

/// What the caller does next after [`on_inbound_text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The number is on the list. `confirmation` is the outbox row of the app's one confirmation,
    /// if it queued one; `email` asks the caller to confirm by email instead, texting being paused
    /// (CON-009, CON-030).
    OptedOut {
        /// The queued confirmation, if any.
        confirmation: Option<Uuid>,
        /// Confirm by email if one is on file: texting is paused by the owner, or the homeowner
        /// line is switched off, suspended or restored and waiting (T360's email, D-2629).
        email: bool,
        /// The critical alert raised, if the message carried the emergency flag (CON-017): the
        /// number cannot be texted back once this message lists it (D-870).
        alert: Option<Uuid>,
    },
    /// The provider's START or UNSTOP took the number off: a new request, from the consent text (CON-015).
    Resubscribed,
    /// The number is listed and wrote without START: no text; email her or list her in the digest
    /// (CON-016). `alert` is the critical alert raised for an emergency (CON-017).
    Suppressed {
        /// The alert raised, if the message carried the emergency flag.
        alert: Option<Uuid>,
    },
    /// Not listed and not an opt-out: the conversation carries on.
    Open,
}

/// Applies one inbound text to the suppression list (CON-008 to CON-011, CON-013, CON-015 to
/// CON-017, CON-030, CON-035) in one transaction, `now` from the caller's clock, `secret` the
/// `ID_SECRET` for the confirmation's key.
///
/// # Errors
///
/// [`OutboxError::BadPhone`], [`OutboxError::Template`] or [`OutboxError::Database`].
pub async fn on_inbound_text(
    db: &mut PgConnection,
    (secret, vault): (&SecretString, &Vault),
    msg: &InboundText<'_>,
    now: Timestamp,
) -> Result<Outcome, OutboxError> {
    let digests = digests(vault, msg.phone)?;
    let mut tx = db.begin().await.map_err(|_| OutboxError::Database)?;
    let listed = models::listing(&mut tx, &digests).await?.is_some();
    let outcome = match rules::classify(msg.text, msg.provider) {
        Classified::OptOut {
            detected,
            by_provider,
        } => {
            let how = detected.map_or_else(|| "provider".to_owned(), rules::Detected::label);
            let (event, added) = record(
                &mut tx,
                &digests,
                (RevokeChannel::Text, &how, Some(msg.inbox_id)),
                now,
            )
            .await?;
            let spanish = detected.is_some_and(rules::Detected::spanish);
            let answered = (by_provider, listed || !added);
            let (confirmation, email) =
                confirm(&mut tx, secret, msg, (event, spanish, answered)).await?;
            Outcome::OptedOut {
                confirmation,
                email,
                alert: alert(&mut tx, msg).await?,
            }
        }
        Classified::Resubscribe if listed => {
            let event = OptOutEvent {
                action: "start",
                channel: RevokeChannel::Text.as_str(),
                detected: "start",
                inbox: Some(msg.inbox_id),
                digest: digests[0],
                at: now,
            };
            models::insert_optout_event(&mut tx, &event).await?;
            models::lift_suppression(&mut tx, &digests).await?;
            Outcome::Resubscribed
        }
        Classified::Resubscribe | Classified::Other if !listed => Outcome::Open,
        Classified::Resubscribe | Classified::Other => Outcome::Suppressed {
            alert: alert(&mut tx, msg).await?,
        },
    };
    tx.commit().await.map_err(|_| OutboxError::Database)?;
    Ok(outcome)
}

/// The app's part of the one confirmation (CON-009, CON-030): none when the number was listed
/// already or by a concurrent opt-out (`answered.1`, a repeat); while texting is paused or the
/// line is closed, the email for every new opt-out, a provider word included, since CON-030 makes
/// no word-type exception (`(None, true)`, D-3479); else none when the provider answered
/// (`answered.0`), or the CON-009 text queued once under opt-out `event`, in Spanish for a
/// Spanish phrase: `(Some(row), false)`.
async fn confirm(
    db: &mut PgConnection,
    secret: &SecretString,
    msg: &InboundText<'_>,
    (event, spanish, (by_provider, repeat)): (Uuid, bool, (bool, bool)),
) -> Result<(Option<Uuid>, bool), OutboxError> {
    if repeat {
        return Ok((None, false));
    }
    if texting_paused(db).await? || line_closed(db, msg.relation).await? {
        return Ok((None, true));
    }
    if by_provider {
        return Ok((None, false));
    }
    let language = if spanish { Language::Es } else { Language::En };
    let body = Registry::embedded()
        .and_then(|r| r.render(CONFIRMATION_TEMPLATE, language, false, &[]))
        .map_err(|_| OutboxError::Template)?;
    let text = NewText {
        template_id: CONFIRMATION_TEMPLATE,
        subject: &[event],
        recipient: msg.contact,
        relation: msg.relation,
        body: &body.body,
    };
    let (id, _) = service::enqueue(db, secret, &text).await?;
    Ok((Some(id), false))
}

/// CON-030, TXT-021, D-2629: whether a homeowner's line is closed by the facts `send_on_line`
/// reads (`mode_state`'s stored homeowner line: texting switched off, the number suspended, or
/// restored and waiting for Casey), so a confirmation queued now would never go. A business
/// sender keeps the ops pause alone.
async fn line_closed(db: &mut PgConnection, relation: Relation) -> Result<bool, OutboxError> {
    if relation == Relation::Business {
        return Ok(false);
    }
    let line = load_lines(db, (None, None))
        .await
        .map_err(|_| OutboxError::Database)?
        .homeowner;
    Ok(!line.texting_on || line.state.paused())
}

/// Whether the owner paused texting, alone or with everything (OPS-003, CON-030).
async fn texting_paused(db: &mut PgConnection) -> Result<bool, OutboxError> {
    for switch in [Switch::Texting, Switch::All] {
        if is_paused(db, switch)
            .await
            .map_err(|_| OutboxError::Database)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Raises the critical alert for an emergency from a listed number (one listed before or by this
/// message), if the message has the flag (CON-017, D-870); the alert holds the inbox row's ID only
/// (PRIV-006).
async fn alert(db: &mut PgConnection, msg: &InboundText<'_>) -> Result<Option<Uuid>, OutboxError> {
    if !msg.emergency {
        return Ok(None);
    }
    let alert = NewAlert {
        kind: EMERGENCY_ALERT,
        severity: Severity::Critical,
        subject: Some(("messaging.inbox", msg.inbox_id)),
    };
    raise_alert(db, &alert)
        .await
        .map(Some)
        .map_err(|_| OutboxError::Database)
}

/// Puts `phone` on the one suppression list at `now` for a revocation by `channel`, `how` being
/// what was detected (CON-035, CON-036): the entry for help emails, email replies, form messages,
/// the portal switch and owner-recorded requests. Such a revocation is confirmed by email or on
/// screen, never by text. Returns whether the number was newly listed.
///
/// # Errors
///
/// [`OutboxError::BadPhone`] or [`OutboxError::Database`].
pub async fn suppress(
    db: &mut PgConnection,
    vault: &Vault,
    phone: &str,
    (channel, how): (RevokeChannel, &str),
    now: Timestamp,
) -> Result<bool, OutboxError> {
    let digests = digests(vault, phone)?;
    let mut tx = db.begin().await.map_err(|_| OutboxError::Database)?;
    let listed = models::listing(&mut tx, &digests).await?.is_some();
    let (_, added) = record(&mut tx, &digests, (channel, how, None), now).await?;
    tx.commit().await.map_err(|_| OutboxError::Database)?;
    Ok(added && !listed)
}

/// Logs an opt-out by `channel`, detected as `how`, with the inbound text `inbox` that carried it
/// (texts only, CON-036, D-1853), and lists the number from `now` (a listed number keeps its
/// earlier time, the repeat being logged only); returns the event's ID and whether it added a
/// suppression row. The insert waits on a concurrent opt-out's uncommitted rows, so of two at once
/// only the first adds any: that, not an earlier read, decides "newly listed" (CON-009, CON-022,
/// CON-035, D-870).
async fn record(
    db: &mut PgConnection,
    digests: &[(u16, [u8; 32])],
    (channel, how, inbox): (RevokeChannel, &str, Option<Uuid>),
    now: Timestamp,
) -> Result<(Uuid, bool), OutboxError> {
    let event = OptOutEvent {
        action: "opt_out",
        channel: channel.as_str(),
        detected: how,
        inbox,
        digest: *digests.first().ok_or(OutboxError::Database)?,
        at: now,
    };
    let id = models::insert_optout_event(db, &event).await?;
    let added = models::put_suppression(db, digests, id, now).await?;
    Ok((id, added > 0))
}

/// Sends outbox row `id` as `service::send` does, after checking the suppression list at `now`
/// (CON-013, CON-035): a listed number gets only the HELP reply and the confirmation of its current
/// listing within 5 minutes of it (`rules::may_send_to`), whatever the provider has blocked. A
/// carrier refusal with 21610 lists the number and blocks the row, which is never retried
/// (CON-014). The claim is stamped at `now`, so `outbox_sender`'s 5-minute sweep counts from the
/// sender's clock (D-2628). The number read for the list check is handed on to the claim, so the
/// vault is read once here (D-3149). It applies no quiet-hours rule: the sender, its only
/// production caller, re-checks the window first (QH-001, QH-008, D-3055).
///
/// # Errors
///
/// [`OutboxError::Suppressed`], [`OutboxError::Blocked`], [`OutboxError::NotFound`],
/// [`OutboxError::BadPhone`], [`OutboxError::Recipient`] or any error of `service::send`.
pub async fn send_checked(
    db: &mut PgConnection,
    (secret, carrier, vault): (&SecretString, &(impl SmsAdapter + Sync), &Vault),
    id: Uuid,
    (to, from): (To<'_>, &str),
    now: Timestamp,
) -> Result<Option<SentText>, OutboxError> {
    let (recipient, template, subject) = models::outbox_target(db, id)
        .await?
        .ok_or(OutboxError::NotFound)?;
    let number = service::recipient_phone(db, to, recipient).await?;
    let digests = digests(vault, number.expose_secret())?;
    let listing = models::listing(db, &digests).await?;
    if !rules::may_send_to(listing, &template, &subject, now) {
        return Err(OutboxError::Suppressed);
    }
    let watch = Watch {
        carrier,
        opted_out: AtomicBool::new(false),
    };
    let sent = service::send_at(
        db,
        (secret, &watch),
        id,
        (to, from),
        (Some(now), Some(&number)),
    )
    .await;
    if sent != Err(OutboxError::Carrier) || !watch.opted_out.load(Ordering::Relaxed) {
        return sent;
    }
    let mut tx = db.begin().await.map_err(|_| OutboxError::Database)?;
    if listing.is_none() {
        record(
            &mut tx,
            &digests,
            (RevokeChannel::Carrier, "carrier:21610", None),
            now,
        )
        .await?;
    }
    models::block(&mut tx, id).await?;
    tx.commit().await.map_err(|_| OutboxError::Database)?;
    Err(OutboxError::Blocked)
}

/// CON-028, D-3480: whether inbox row `inbox` is the text whose opt-out put `phone` on the list
/// it is on now, so a repeat STOP from a listed number (`Outcome::OptedOut` with nothing added)
/// is told apart from the opt-out that listed it, and its caller sends no second "still open"
/// email. A resend of that same text still answers `true`.
///
/// # Errors
///
/// [`OutboxError::BadPhone`] or [`OutboxError::Database`].
pub async fn listed_by(
    db: &mut PgConnection,
    vault: &Vault,
    phone: &str,
    inbox: Uuid,
) -> Result<bool, OutboxError> {
    let digests = digests(vault, phone)?;
    let Some(listing) = models::listing(db, &digests).await? else {
        return Ok(false);
    };
    sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM messaging.optout_event WHERE id = $1 AND inbox_id = $2) \
         AS \"listed!\"",
        listing.event,
        inbox
    )
    .fetch_one(db)
    .await
    .map_err(|_| OutboxError::Database)
}

/// The digests of `phone` under the homeowner-phone index key, one per key version, active first,
/// so a number matches during a rotation (PRIV-024).
fn digests(vault: &Vault, phone: &str) -> Result<Vec<(u16, [u8; 32])>, OutboxError> {
    let phone = normalize_phone(phone).map_err(|_| OutboxError::BadPhone)?;
    let digests = vault.blind_index(IndexPurpose::HomeownerPhone, &phone);
    if digests.is_empty() {
        return Err(OutboxError::Database);
    }
    Ok(digests)
}

/// The carrier, watched for a 21610 refusal (CON-014).
struct Watch<'a, C> {
    carrier: &'a C,
    opted_out: AtomicBool,
}

impl<C: SmsAdapter + Sync> SmsAdapter for Watch<'_, C> {
    async fn send_text(
        &self,
        key: &IdempotencyKey,
        to: &str,
        from: &str,
        body: &str,
    ) -> Result<SentText, AdapterError> {
        let sent = self.carrier.send_text(key, to, from, body).await;
        if let Err(AdapterError::Vendor {
            code: Some(OPTED_OUT_CODE),
            ..
        }) = sent
        {
            self.opted_out.store(true, Ordering::Relaxed);
        }
        sent
    }
}

/// The `core.task` kind of an inbound opt-out's email follow-up (CON-016, CON-030, D-2629,
/// D-3118): payload `{"inbox", "contact", "language", "follow_up"}`, IDs and codes only.
pub const OPTOUT_FOLLOWUP_KIND: &str = "messaging.optout_followup";

/// Which email an inbound opt-out text is followed up with (D-2629).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowUp {
    /// CON-030: the opt-out was recorded while texting was off or suspended; confirm by email.
    Confirmation,
    /// CON-016: a listed homeowner wrote again without START.
    WroteAgain,
}

impl FollowUp {
    /// The payload code.
    fn code(self) -> &'static str {
        match self {
            Self::Confirmation => "confirmation",
            Self::WroteAgain => "wrote_again",
        }
    }
}

/// CON-016, CON-030, JOB-005, SEC-018, D-3118: queues one [`OPTOUT_FOLLOWUP_KIND`] task in the
/// caller's (the delivery's) transaction for homeowner `contact`'s inbox row `inbox`, in
/// `language` (the caller's choice: a Spanish opt-out phrase's confirmation, else TXT-039),
/// deduped per inbox row and follow-up; `true` when new. No address, phone or body in it.
///
/// # Errors
///
/// [`OutboxError::Database`].
pub async fn queue_optout_followup(
    db: &mut PgConnection,
    (inbox, contact, language): (Uuid, Uuid, Language),
    follow_up: FollowUp,
    now: Timestamp,
) -> Result<bool, OutboxError> {
    let code = if language == Language::Es { "es" } else { "en" };
    let payload = serde_json::json!({
        "inbox": inbox, "contact": contact, "language": code, "follow_up": follow_up.code(),
    });
    let mut task = NewTask::new(OPTOUT_FOLLOWUP_KIND, payload, now);
    task.dedupe_key = Some(format!(
        "{OPTOUT_FOLLOWUP_KIND}:{inbox}:{}",
        follow_up.code()
    ));
    enqueue_task(db, &task)
        .await
        .map(|id| id.is_some())
        .map_err(|_| OutboxError::Database)
}

/// The [`OPTOUT_FOLLOWUP_KIND`] handler (CON-016, CON-030, D-2629, D-3118). Without a sender (no
/// footer postal line or vault keys: dev and CI) it sends nothing and is done. Else, at the inbox
/// row's arrival: a confirmation finds the opt-out event the text recorded (`inbox_id`, migration
/// 0128) and calls `queue_optout_confirmation_email`; a wrote-again calls
/// `queue_optout_wrote_again`. Her email is read only through the vault inside those functions,
/// never logged or put in a payload. Safe to run twice: both are keyed.
///
/// # Errors
///
/// A fixed sentence for a malformed payload or a failed step (the queue tries again).
pub async fn run_optout_followup(
    pool: PgPool,
    sender: Option<Arc<OptOutEmailSender>>,
    task: Claimed,
) -> Result<(), HandlerError> {
    const FAILED: HandlerError = HandlerError("opt-out follow-up could not be stored");
    let id = |k: &str| {
        task.payload[k]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
    };
    let follow_up = match task.payload["follow_up"].as_str() {
        Some("confirmation") => FollowUp::Confirmation,
        Some("wrote_again") => FollowUp::WroteAgain,
        _ => return Err(HandlerError("opt-out follow-up is malformed")),
    };
    let (Some(inbox), Some(contact)) = (id("inbox"), id("contact")) else {
        return Err(HandlerError("opt-out follow-up is malformed"));
    };
    let language = match task.payload["language"].as_str() {
        Some("es") => Language::Es,
        _ => Language::En,
    };
    let Some(sender) = sender else {
        return Ok(());
    };
    let mut db = pool.acquire().await.map_err(|_| FAILED)?;
    let Some((event, at)) = optout_event_of(&mut db, inbox).await? else {
        return Ok(());
    };
    let her = (contact, language);
    match (follow_up, event) {
        (FollowUp::Confirmation, Some(event)) => {
            queue_optout_confirmation_email(&mut db, &sender, her, event, at)
                .await
                .map(|_| ())
        }
        (FollowUp::Confirmation, None) => Ok(()),
        (FollowUp::WroteAgain, _) => queue_optout_wrote_again(&mut db, &sender, her, inbox, at)
            .await
            .map(|_| ()),
    }
    .map_err(|_| FAILED)
}

/// Inbox row `inbox`'s arrival time, which every follow-up is dated by, and the opt-out event it
/// recorded (none for a wrote-again text); `None` when the row is unknown (nothing to do).
async fn optout_event_of(
    db: &mut PgConnection,
    inbox: Uuid,
) -> Result<Option<(Option<Uuid>, Timestamp)>, HandlerError> {
    let row = sqlx::query!(
        r#"SELECT i.received_at, e.id AS "event?" FROM messaging.inbox i
           LEFT JOIN messaging.optout_event e ON e.inbox_id = i.id AND e.action = 'opt_out'
           WHERE i.id = $1 ORDER BY e.effective_at LIMIT 1"#,
        inbox
    )
    .fetch_optional(db)
    .await
    .map_err(|_| HandlerError("opt-out follow-up could not be read"))?;
    Ok(row.map(|r| (r.event, jiff_sqlx::Timestamp::to_jiff(r.received_at))))
}
