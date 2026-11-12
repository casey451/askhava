//! The outbox sender: the `messaging.send_due` worker task that sends queued homeowner texts once
//! (QH-001, QH-006, QH-008, TXT-011, TXT-014, TXT-021, CON-009, CON-030; D-558, D-2188, D-2628).
//!
//! Each run, at the worker clock's now, first sweeps every row still `sending` 5 minutes after its
//! claim ([`STALE_AFTER`], a crash or a lost lease) to `unknown`, then takes the due queued
//! homeowner rows (`not_before` passed or unset) in bounded batches, and last queues the next run.
//! For each row, in order: a row queued while the homeowner line was paused, switched off or
//! suspended (D-2188), and a CON-009 confirmation older than 5 minutes (CON-030), is marked
//! `expired` with one audit row and never sent; then quiet hours are re-checked at this instant
//! (`rules::retry`, QH-001, QH-008) unless the row's stored QH-004 exemption still holds, and a hold
//! sets `not_before` (QH-006) without reaching the carrier; then the row goes through
//! `mode::send_on_line` (the line, the `ops` pauses, the suppression list, the 21610 block). A
//! definite refusal waits for its TXT-014 retry (`retries::record_failure`), and its last one gives
//! the text up (`retries::give_up`). An unknown outcome is never resent blind: the row becomes
//! `unknown` with its body dropped in the same statement, an important text goes to its email copy
//! and portal line, and one Normal ("Low", digest) [`OUTCOME_UNKNOWN_ALERT`] is raised per row, with
//! no body or phone. The Twilio sent-log lookup that could settle an `unknown` row (OPS-017) is not
//! built. Business rows are not sent here: their phone is not resolved in `messaging` (D-2042).

use std::sync::Arc;

use askhava_adapters::api::SmsAdapter;
use askhava_core::api::{Claimed, Clock, HandlerError, NewTask, Registry, TaskError, enqueue};
use askhava_ops::api::{Actor, AuditEntry, Change, NewAlert, Severity, audit, raise_alert};
use askhava_privacy::api::{Field, Purpose, Reader, Vault, VaultError, read_contact};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::email::{
    EmailClass, NewEmail, Notice, PostalAddress, Recipient, homeowner_email, queue_email,
};
use crate::mode::{LineSendError, Lines, SendingNumber, send_on_line_as};
use crate::mode_state::load_lines;
use crate::models::{self, Due, OutboxError};
use crate::retries::{
    self, IMPORTANT_TEMPLATES, NO_RUSH_CALL_BACK, Reach, Step, TEXT_FAILED_EMAIL, Urgency,
};
use crate::rules::{
    self, AreaCodes, CONFIRM_WITHIN, CONFIRMATION_TEMPLATE, Channel, Decision, HELP_TEMPLATE, Kind,
    Outgoing, QuietHoursError,
};
use crate::service::{Relation, To};
use crate::templates::{Audience, Language};

/// The `core.task` kind of the outbox sender; each run queues the next (D-2628).
pub const SEND_DUE_KIND: &str = "messaging.send_due";
/// The Normal alert raised once per row whose send outcome is unknown (TXT-011, D-2628).
pub const OUTCOME_UNKNOWN_ALERT: &str = "messaging.send_outcome_unknown";
/// The `ops.audit_log` action of a row marked `expired` (D-2188, CON-030).
pub const EXPIRED_ACTION: &str = "messaging.text_expired";
/// How long a row may stay `sending` before it is `unknown` (D-2628).
pub const STALE_AFTER: SignedDuration = SignedDuration::from_mins(5);
/// How often the sender runs.
pub const SEND_EVERY: SignedDuration = SignedDuration::from_mins(1);
/// The most rows one run sweeps or sends.
const BATCH: i64 = 50;
/// The homeowner portal, the "Manage email notices" link of a text's email copy.
const MANAGE_URL: &str = "/h";
const FAILED: HandlerError = HandlerError("outbox sender database call failed");

/// What the sender needs: the keys, the carrier, the vault, the configured homeowner number (none:
/// the line has no sender and nothing goes, TXT-022), the footer's postal line for a text's email
/// copy (none in dev and CI: no copy is queued) and the worker's clock.
pub struct OutboxSender<C> {
    secret: SecretString,
    carrier: C,
    vault: Vault,
    number: Option<SendingNumber>,
    postal: Option<PostalAddress>,
    clock: Arc<dyn Clock>,
    areas: AreaCodes,
}

impl<C> OutboxSender<C> {
    /// The sender over `carrier`, with `secret` (the `ID_SECRET`) and `vault`.
    ///
    /// # Errors
    ///
    /// [`QuietHoursError`] when the compiled-in area-code table is broken.
    pub fn new(
        (secret, vault): (SecretString, Vault),
        carrier: C,
        (number, postal): (Option<SendingNumber>, Option<PostalAddress>),
        clock: Arc<dyn Clock>,
    ) -> Result<Self, QuietHoursError> {
        Ok(Self {
            secret,
            carrier,
            vault,
            number,
            postal,
            clock,
            areas: AreaCodes::embedded()?,
        })
    }
}

/// Registers [`SEND_DUE_KIND`] with `sender` (D-2628, T327's `worker_registry`).
pub fn register_send_due<C>(registry: &mut Registry, sender: Arc<OutboxSender<C>>)
where
    C: SmsAdapter + Send + Sync + 'static,
{
    registry.register(SEND_DUE_KIND, move |pool, task| {
        run_send_due(pool, Arc::clone(&sender), task)
    });
}

/// Queues the [`SEND_DUE_KIND`] run due at `run_at`, once per minute.
///
/// # Errors
///
/// The [`TaskError`] of the insert.
pub async fn schedule_send_due(db: &mut PgConnection, run_at: Timestamp) -> Result<(), TaskError> {
    let mut task = NewTask::new(SEND_DUE_KIND, json!({}), run_at);
    task.dedupe_key = Some(format!("{SEND_DUE_KIND}:{}", run_at.as_second() / 60));
    enqueue(db, &task).await.map(|_| ())
}

/// The [`SEND_DUE_KIND`] handler: the next run, then the stale sweep and the due sends, at the
/// clock's now (TXT-011, QH-001, QH-006, QH-008, D-2628). The next run is queued first, so a failed
/// run never ends the loop; a row whose step fails is logged by ID, held [`RETRY_LAST`] and
/// skipped, never aborting the batch (D-3149). Safe to run twice or on two workers at once: a row
/// is claimed only from `queued`, in one statement.
///
/// # Errors
///
/// A fixed [`HandlerError`] when the connection, the next run's insert or a batch query fails.
pub async fn run_send_due<C: SmsAdapter + Sync>(
    pool: PgPool,
    sender: Arc<OutboxSender<C>>,
    _task: Claimed,
) -> Result<(), HandlerError> {
    let now = sender.clock.now();
    let mut conn = pool.acquire().await.map_err(|_| FAILED)?;
    schedule_send_due(&mut conn, now + SEND_EVERY)
        .await
        .map_err(|_| FAILED)?;
    for id in models::stale_sending(&mut conn, now - STALE_AFTER, BATCH)
        .await
        .map_err(|_| FAILED)?
    {
        if settle_unknown(&mut conn, &sender, (id, Some(now - STALE_AFTER)), now)
            .await
            .is_err()
        {
            tracing::warn!(outbox_id = %id, "an unknown-outcome row could not be settled");
        }
    }
    let lines = load_lines(&mut conn, (sender.number.clone(), None))
        .await
        .map_err(|_| FAILED)?;
    for row in models::due(&mut conn, now, BATCH)
        .await
        .map_err(|_| FAILED)?
    {
        if send_one(&mut conn, &sender, (&lines, &row), now)
            .await
            .is_err()
        {
            tracing::warn!(outbox_id = %row.id, "an outbox row failed to send; held and skipped");
            let _held = models::hold(&mut conn, row.id, now + RETRY_LAST).await;
        }
    }
    Ok(())
}

/// One due row (D-2188, CON-030, QH-001, QH-008, TXT-021): expired, held, or sent on its line.
/// Her phone is read from the vault once, here, and handed to the send (D-3149).
async fn send_one<C: SmsAdapter + Sync>(
    db: &mut PgConnection,
    s: &OutboxSender<C>,
    (lines, row): (&Lines, &Due),
    now: Timestamp,
) -> Result<(), OutboxError> {
    if must_expire(db, row, now).await? {
        return expire(db, row.id).await;
    }
    let phone = match homeowner_phone(db, &s.vault, row.recipient).await {
        Ok(Some(phone)) => phone,
        Ok(None) => return expire(db, row.id).await,
        Err(()) => return models::hold(db, row.id, now + RETRY_LAST).await,
    };
    match quiet_hours(&s.areas, phone.expose_secret(), row.exempt, now) {
        Ok(Decision::Send) => {}
        Ok(Decision::Hold { until }) => return models::hold(db, row.id, until).await,
        Err(_) => return expire(db, row.id).await,
    }
    let to = To::Homeowner {
        vault: &s.vault,
        relation: Relation::OpenRequest,
    };
    let keys = (&s.secret, &s.carrier, &s.vault);
    let sent = send_on_line_as(db, keys, lines, (row.id, to), (now, Some(&phone))).await;
    match sent {
        Ok(_) | Err(LineSendError::Paused) => Ok(()),
        Err(LineSendError::Outbox(e)) => after_error(db, s, row, e, now).await,
    }
}

/// D-3149: her phone under the send grant. `None` when it can never be read (no such contact, no
/// phone on file, a number not `+1` and ten digits, a sealed value that will not open): the row
/// is expired, never left at the head of the queue. `Err` for a failure that may pass (the
/// database, the audit write): the row is held [`RETRY_LAST`].
async fn homeowner_phone(
    db: &mut PgConnection,
    vault: &Vault,
    recipient: Uuid,
) -> Result<Option<SecretString>, ()> {
    let grant = (Reader::Messaging, Purpose::Send);
    match read_contact(db, vault, grant, recipient, &[Field::Phone]).await {
        Ok(contact) => Ok(contact.phone),
        Err(VaultError::NotFound | VaultError::BadPhone | VaultError::Sealed) => Ok(None),
        Err(_) => Err(()),
    }
}

/// QH-001, QH-004, QH-008: whether a row may go at `now`: always while its stored exemption
/// holds, else by the same rule as a first send (`rules::retry`). A number outside the area-code
/// table is an error and is never texted (QH-003).
fn quiet_hours(
    areas: &AreaCodes,
    phone: &str,
    exempt: bool,
    now: Timestamp,
) -> Result<Decision, QuietHoursError> {
    let msg = Outgoing {
        phone,
        kind: Kind::Other,
        channel: Channel::Text,
        emergency_job: false,
        after_hours_business: false,
        emergency_emails: false,
    };
    let decision = rules::retry(areas, &msg, now)?;
    Ok(if exempt { Decision::Send } else { decision })
}

/// D-2188, CON-009, CON-030: a confirmation older than 5 minutes, or any row but the HELP reply
/// queued while the homeowner line was paused, is never sent.
async fn must_expire(
    db: &mut PgConnection,
    row: &Due,
    now: Timestamp,
) -> Result<bool, OutboxError> {
    if row.template_id == CONFIRMATION_TEMPLATE
        && now.duration_since(row.queued_at) > CONFIRM_WITHIN
    {
        return Ok(true);
    }
    if row.template_id == HELP_TEMPLATE {
        return Ok(false);
    }
    models::homeowner_paused_at(db, row.queued_at).await
}

/// Marks row `id` `expired` with one system audit row, in one transaction (D-2188).
async fn expire(db: &mut PgConnection, id: Uuid) -> Result<(), OutboxError> {
    let mut tx = db.begin().await.map_err(|_| OutboxError::Database)?;
    if models::expire(&mut tx, id).await? {
        let entry = AuditEntry {
            actor: Actor::System,
            action: EXPIRED_ACTION,
            subject_kind: "messaging.outbox",
            subject_id: id,
            change: Change::Action,
        };
        audit(&mut tx, &entry)
            .await
            .map_err(|_| OutboxError::Database)?;
    }
    tx.commit().await.map_err(|_| OutboxError::Database)
}

/// What a failed send leaves: a listed number's row expires (CON-013), and so does a row whose
/// recipient cannot be read or is of the wrong kind (D-3149), so no row stays at the head of the
/// queue; a refusal waits for its TXT-014 retry or is given up; an unknown outcome is settled; a
/// row gone, blocked (21610) or already claimed needs nothing.
async fn after_error<C>(
    db: &mut PgConnection,
    s: &OutboxSender<C>,
    row: &Due,
    error: OutboxError,
    now: Timestamp,
) -> Result<(), OutboxError> {
    match error {
        OutboxError::Suppressed
        | OutboxError::Recipient
        | OutboxError::RecipientKind
        | OutboxError::BadPhone
        | OutboxError::PastCustomer => expire(db, row.id).await,
        OutboxError::Carrier => refused(db, s, row, now).await,
        OutboxError::OutcomeUnknown => settle_unknown(db, s, (row.id, None), now).await,
        OutboxError::Database => Err(OutboxError::Database),
        _ => Ok(()),
    }
}

/// TXT-014, TXT-015, QH-008: a refused row waits for its next retry; after the last, with the
/// footer's postal line, it is given up (email copy and portal line, or Casey's call-back).
async fn refused<C>(
    db: &mut PgConnection,
    s: &OutboxSender<C>,
    row: &Due,
    now: Timestamp,
) -> Result<(), OutboxError> {
    match (retries::record_failure(db, row.id, now).await?, &s.postal) {
        (Step::RetryAt(at), _) => models::hold(db, row.id, at).await,
        (Step::GiveUp, None) => models::hold(db, row.id, now + RETRY_LAST).await,
        (Step::GiveUp, Some(postal)) => {
            let reach = Reach::Homeowner {
                vault: &s.vault,
                urgency: Urgency::NoRush,
                no_rush: NO_RUSH_CALL_BACK,
            };
            let to = email_to(row.recipient, row.language.as_deref());
            retries::give_up(db, (&s.secret, postal), (row.id, reach, &to), now)
                .await
                .map(|_| ())
        }
    }
}

/// How long a refused row with no postal line to give it up waits before it is tried again.
const RETRY_LAST: SignedDuration = SignedDuration::from_mins(30);

/// The email recipient of a homeowner's text copy, in the row's language (English when unknown).
fn email_to(id: Uuid, language: Option<&str>) -> Recipient<'static> {
    Recipient {
        id,
        audience: Audience::Homeowner,
        language: if language == Some("es") {
            Language::Es
        } else {
            Language::En
        },
        manage_url: MANAGE_URL,
    }
}

/// TXT-011, TXT-014, D-558, D-2628: moves `sending` row `id` (with a cutoff: only one claimed
/// before it) to `unknown`, dropping its body; an important text's copy is listed for the portal and
/// emailed when an address and the postal line are on file, and one Normal alert is raised, all in
/// one transaction. The email address is read before it (the vault refuses a read inside one).
async fn settle_unknown<C>(
    db: &mut PgConnection,
    s: &OutboxSender<C>,
    (id, stale_before): (Uuid, Option<Timestamp>),
    now: Timestamp,
) -> Result<(), OutboxError> {
    let Some((recipient, template, _)) = models::outbox_target(db, id).await? else {
        return Ok(());
    };
    let important = IMPORTANT_TEMPLATES.contains(&template.as_str());
    let on_file = important
        && s.postal.is_some()
        && matches!(homeowner_email(db, &s.vault, recipient).await, Ok(Some(_)));
    let mut tx = db.begin().await.map_err(|_| OutboxError::Database)?;
    let Some((text, language)) = models::mark_unknown(&mut tx, id, stale_before).await? else {
        return Ok(());
    };
    if important {
        models::insert_portal_notice(&mut tx, id, &text, now).await?;
    }
    if let (true, Some(postal), "homeowner") = (on_file, &s.postal, text.kind.as_str()) {
        let email = NewEmail {
            template_id: TEXT_FAILED_EMAIL,
            subject: &[id],
            class: EmailClass::Notice,
            notice: Notice::TextFailed(&text.body),
        };
        let to = email_to(text.recipient, language.as_deref());
        queue_email(&mut tx, (&s.secret, postal), &to, &email, now)
            .await
            .map_err(|_| OutboxError::Database)?;
    }
    let alert = NewAlert {
        kind: OUTCOME_UNKNOWN_ALERT,
        severity: Severity::Normal,
        subject: Some(("messaging.outbox", id)),
    };
    raise_alert(&mut tx, &alert)
        .await
        .map_err(|_| OutboxError::Database)?;
    tx.commit().await.map_err(|_| OutboxError::Database)
}
