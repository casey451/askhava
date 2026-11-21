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
//! built. The clock is read again for each row, so a long batch never sends past 8 p.m. (D-4764).
//!
//! Business rows (D-4051, D-4913; TXT-035, CON-019, BIZ-037) are taken only while the business
//! line is open, which needs [`OutboxSender::with_business`]: without it they wait. A row's
//! recipient is the phone's line ID; a line that is not in the sender directory (a business ID
//! queued before D-2042 closed), a phone `may_text` refuses and a number that can never be read
//! expire the row. The number is read through [`LinePhones`] only after the line is open, Arizona
//! quiet hours passed and `may_text` passed; it goes to the area-code window check (QH-001,
//! QH-003) and the carrier call and nowhere else. [`expire_job_rows`] is the job's side of QH-007.
//!
//! A homeowner's given-up text with no email on file raises Casey's call-back, due as her job's
//! urgency sets (INT-023, TXT-015, D-4052): the composition passes an [`UrgencySource`]
//! ([`OutboxSender::with_urgency`]), since `messaging` never reads `intake`; without one the
//! call-back is no-rush, and a failed read holds the row for its retry.

use std::pin::Pin;
use std::sync::{Arc, LazyLock};

use askhava_adapters::api::SmsAdapter;
use askhava_core::api::{Claimed, Clock, HandlerError, NewTask, Registry, TaskError, enqueue};
use askhava_ops::api::{
    Actor, AuditEntry, Change, NewAlert, Severity, Switch, audit, is_paused, raise_alert,
};
use askhava_privacy::api::{Field, Purpose, Reader, Vault, VaultError, read_contact};
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection, PgPool};

use crate::business_notices::{SendChecks, held_until};
use crate::email::{
    EmailClass, NewEmail, Notice, OnDomain, PostalAddress, Recipient, homeowner_email, queue_email,
};
use crate::mode::{LineSendError, Lines, SendingNumber, send_on_line};
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
/// The homeowner portal, the absolute "Manage email notices" link of a text's email copy, as every
/// other sender renders it (SEC-025, SEC-028, D-4327).
static MANAGE_URL: LazyLock<String> = LazyLock::new(|| OnDomain("/h").to_string());
/// The first hold of a row the carrier's rate limit refused (TXT-014, D-4052).
const RATE_HOLD: SignedDuration = SignedDuration::from_mins(1);
/// The longest hold of a rate-limited row (D-4052).
const RATE_HOLD_MAX: SignedDuration = SignedDuration::from_mins(15);
const FAILED: HandlerError = HandlerError("outbox sender database call failed");

/// Why [`LinePhones::phone`] gave no number (D-4913).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneUnread {
    /// It can never be read (no such line, a sealed value that will not open): the row expires.
    Never,
    /// The read may pass later (the database, the audit write): the row is held.
    Later,
}

/// Answers a business line's texting number at send time; the composition implements it over the
/// vault, since `messaging` never names that read (BIZ-037, SEC-047, D-4913).
pub trait LinePhones: Send + Sync {
    /// The number of linked phone `line`, read on bare connection `conn`.
    fn phone(
        &self,
        conn: &mut PgConnection,
        line: Uuid,
    ) -> impl Future<Output = Result<SecretString, PhoneUnread>> + Send;
}

/// What [`UrgencySource::urgency`] answers: her job's urgency, or an error when it could not be
/// read (the row is then held for its retry, never guessed).
pub type UrgencyRead<'a> = Pin<Box<dyn Future<Output = Result<Urgency, OutboxError>> + Send + 'a>>;

/// Answers how soon the homeowner with a given vault contact needs her call-back when her text is
/// given up with no email on file; the composition implements it over `intake`, as `messaging`
/// never reads that crate (INT-023, TXT-015, D-4052).
pub trait UrgencySource: Send + Sync {
    /// The urgency of the job of the homeowner with vault contact `contact` at `now`, read on
    /// `conn`.
    fn urgency<'a>(
        &'a self,
        conn: &'a mut PgConnection,
        contact: Uuid,
        now: Timestamp,
    ) -> UrgencyRead<'a>;
}

/// The gate and phone reader of a sender built without [`OutboxSender::with_business`]: it
/// refuses every phone and reads none; no business row reaches it, since the line has no number.
pub struct NoBusiness;

impl SendChecks for NoBusiness {
    async fn may_text(&self, _: &mut PgConnection, _: Uuid, _: Uuid) -> bool {
        false
    }

    async fn accept_by(&self, _: &mut PgConnection, _: Uuid, _: Timestamp) -> Option<Timestamp> {
        None
    }
}

impl LinePhones for NoBusiness {
    async fn phone(&self, _: &mut PgConnection, _: Uuid) -> Result<SecretString, PhoneUnread> {
        Err(PhoneUnread::Never)
    }
}

/// What the sender needs: the keys, the carrier, the vault, the configured homeowner number (none:
/// the line has no sender and nothing goes, TXT-022), the footer's postal line for a text's email
/// copy (none in dev and CI: no copy is queued) and the worker's clock; for business rows also the
/// business number, the CON-019 gate and the phone reader ([`OutboxSender::with_business`]); and
/// the call-back urgency source ([`OutboxSender::with_urgency`], none: no rush).
pub struct OutboxSender<C, G = NoBusiness, P = NoBusiness> {
    secret: SecretString,
    carrier: C,
    vault: Vault,
    number: Option<SendingNumber>,
    postal: Option<PostalAddress>,
    clock: Arc<dyn Clock>,
    areas: AreaCodes,
    business: Option<SendingNumber>,
    gate: G,
    phones: P,
    urgency: Option<Arc<dyn UrgencySource>>,
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
            business: None,
            gate: NoBusiness,
            phones: NoBusiness,
            urgency: None,
        })
    }

    /// TXT-035, CON-019, D-4051, D-4913: the sender that also sends business rows, from `number`
    /// (none: they wait), each checked by `gate.may_text` and addressed by `phones` at send time.
    pub fn with_business<G: SendChecks, P: LinePhones>(
        self,
        (gate, phones): (G, P),
        number: Option<SendingNumber>,
    ) -> OutboxSender<C, G, P> {
        OutboxSender {
            secret: self.secret,
            carrier: self.carrier,
            vault: self.vault,
            number: self.number,
            postal: self.postal,
            clock: self.clock,
            areas: self.areas,
            business: number,
            gate,
            phones,
            urgency: self.urgency,
        }
    }
}

impl<C, G, P> OutboxSender<C, G, P> {
    /// INT-023, TXT-015, D-4052: the sender whose given-up homeowner texts take their call-back
    /// urgency from `source` (without it every call-back is no-rush).
    #[must_use]
    pub fn with_urgency(self, source: Arc<dyn UrgencySource>) -> Self {
        Self {
            urgency: Some(source),
            ..self
        }
    }
}

/// Registers [`SEND_DUE_KIND`] with `sender` (D-2628, T327's `worker_registry`).
pub fn register_send_due<C, G, P>(registry: &mut Registry, sender: Arc<OutboxSender<C, G, P>>)
where
    C: SmsAdapter + Send + Sync + 'static,
    G: SendChecks + 'static,
    P: LinePhones + 'static,
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
pub async fn run_send_due<C: SmsAdapter + Sync, G: SendChecks, P: LinePhones>(
    pool: PgPool,
    sender: Arc<OutboxSender<C, G, P>>,
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
    let numbers = (sender.number.clone(), sender.business.clone());
    let lines = load_lines(&mut conn, numbers).await.map_err(|_| FAILED)?;
    let business = lines.line(Audience::Business).sender().is_some();
    for row in models::due(&mut conn, now, (BATCH, business))
        .await
        .map_err(|_| FAILED)?
    {
        let now = sender.clock.now();
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
/// Her phone is read here for the expiry and window checks; the send reads it once more, in
/// `send_checked`, which hands it on to the claim (D-3149). A closed job's row was expired by
/// [`expire_job_rows`], so `Relation::OpenRequest` holds for a row that reaches here (D-4764).
async fn send_one<C: SmsAdapter + Sync, G: SendChecks, P: LinePhones>(
    db: &mut PgConnection,
    s: &OutboxSender<C, G, P>,
    (lines, row): (&Lines, &Due),
    now: Timestamp,
) -> Result<(), OutboxError> {
    if must_expire(db, row, now).await? {
        return expire(db, row.id).await;
    }
    if row.business {
        return send_business(db, s, (lines, row), now).await;
    }
    let phone = match homeowner_phone(db, &s.vault, row.recipient).await {
        Ok(Some(phone)) => phone,
        Ok(None) => return expire(db, row.id).await,
        Err(()) => return models::hold(db, row.id, now + RETRY_LAST).await,
    };
    let to = To::Homeowner {
        vault: &s.vault,
        relation: Relation::OpenRequest,
    };
    in_window(db, s, (lines, row), (to, &phone), now).await
}

/// One due business row (TXT-035, CON-019, BIZ-037, QH-001, QH-003; D-4051, D-4913), in this
/// order: it waits while the business line is closed or the owner paused texting; Arizona quiet
/// hours hold it; a recipient that is no linked phone (a business ID queued before D-2042 closed)
/// and a phone `may_text` refuses expire it; only then is the number read, once: one that can
/// never be read expires the row and a read that may pass holds it. The area-code window check
/// needs the number, so a row it holds is the one case that is read and not sent.
async fn send_business<C: SmsAdapter + Sync, G: SendChecks, P: LinePhones>(
    db: &mut PgConnection,
    s: &OutboxSender<C, G, P>,
    (lines, row): (&Lines, &Due),
    now: Timestamp,
) -> Result<(), OutboxError> {
    let line = row.recipient;
    if lines.line(Audience::Business).sender().is_none() || owner_paused(db).await? {
        return Ok(());
    }
    if let (false, Some(until)) = (row.exempt, held_until(now)) {
        return models::hold(db, row.id, until).await;
    }
    let Some(business) = models::line_business(db, line).await? else {
        return expire(db, row.id).await;
    };
    if !s.gate.may_text(db, business, line).await {
        return expire(db, row.id).await;
    }
    let phone = match s.phones.phone(db, line).await {
        Ok(phone) => phone,
        Err(PhoneUnread::Never) => return expire(db, row.id).await,
        Err(PhoneUnread::Later) => return models::hold(db, row.id, now + RETRY_LAST).await,
    };
    let to = To::Business(phone.expose_secret());
    in_window(db, s, (lines, row), (to, &phone), now).await
}

/// OPS-003: whether the owner's `texting` or `all` pause is on, as `mode::send_on_line` reads it;
/// asked before a business number is read, so a paused row reads nothing (D-4913).
async fn owner_paused(db: &mut PgConnection) -> Result<bool, OutboxError> {
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

/// QH-001, QH-003, QH-008, TXT-021: the last steps of a row whose number `phone` is read: held to
/// the next window of both zones, expired for an area code outside the table, else sent on its
/// line with what a failed send leaves ([`after_error`]).
async fn in_window<C: SmsAdapter + Sync, G, P>(
    db: &mut PgConnection,
    s: &OutboxSender<C, G, P>,
    (lines, row): (&Lines, &Due),
    (to, phone): (To<'_>, &SecretString),
    now: Timestamp,
) -> Result<(), OutboxError> {
    match quiet_hours(&s.areas, phone.expose_secret(), row.exempt, now) {
        Ok(Decision::Send) => {}
        Ok(Decision::Hold { until }) => return models::hold(db, row.id, until).await,
        Err(_) => return expire(db, row.id).await,
    }
    let keys = (&s.secret, &s.carrier, &s.vault);
    match send_on_line(db, keys, lines, (row.id, to), now).await {
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

/// D-2188, CON-009, CON-030: a confirmation older than 5 minutes, or any homeowner row but the
/// HELP reply queued while the homeowner line was paused, is never sent.
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
    if row.template_id == HELP_TEMPLATE || row.business {
        return Ok(false);
    }
    models::homeowner_paused_at(db, row.queued_at).await
}

/// Marks row `id` `expired` with one system audit row, in one transaction (D-2188).
async fn expire(db: &mut PgConnection, id: Uuid) -> Result<(), OutboxError> {
    let mut tx = db.begin().await.map_err(|_| OutboxError::Database)?;
    if models::expire(&mut tx, id).await? {
        audit_expired(&mut tx, id).await?;
    }
    tx.commit().await.map_err(|_| OutboxError::Database)
}

/// The one system [`EXPIRED_ACTION`] audit row of expired row `id`.
async fn audit_expired(db: &mut PgConnection, id: Uuid) -> Result<(), OutboxError> {
    let entry = AuditEntry {
        actor: Actor::System,
        action: EXPIRED_ACTION,
        subject_kind: "messaging.outbox",
        subject_id: id,
        change: Change::Action,
    };
    let logged = audit(db, &entry).await;
    logged.map(|_| ()).map_err(|_| OutboxError::Database)
}

/// The texts [`expire_job_rows`] keeps, as `LIKE` patterns over the template ID (QH-007, D-4764,
/// D-4899): the close announcements (JOB-012, INT-019), the opt-out confirmation and every HELP
/// reply (CON-009, CON-018), and the notices a close itself produces (PICK-026, PICK-038, the
/// pick's end, the release, the refund decisions and the feedback request).
pub const CLOSE_TEXTS: [&str; 17] = [
    "closed_no_pick",
    "intake_closed_no_reply",
    "job_closed",
    "jobs_closed",
    "closed_no_quotes",
    "closed_no_quote_left",
    "line_closed_cancelled",
    "opt_out_confirmation",
    "help_reply%",
    "job_no_longer_active",
    "pick_lost%",
    "pick_expired",
    "pick_withdrawn",
    "unreachable_%",
    "release_notice",
    "release_text",
    "feedback_request",
];

/// QH-007, D-4764: `job` reached its final status at `closed_at` (the job's own time, never the
/// caller's clock), so every queued or held row about it created before then is expired, each
/// with one [`EXPIRED_ACTION`] audit row, except the [`CLOSE_TEXTS`]; a row created at or after
/// the close is never touched. Returns how many rows expired. `messaging` cannot read `jobs`, so
/// the job's side calls this (T744's `jobs.final_status` handler).
///
/// # Errors
///
/// [`OutboxError::Database`]; nothing is then expired.
pub async fn expire_job_rows(
    conn: &mut PgConnection,
    job: Uuid,
    closed_at: Timestamp,
) -> Result<usize, OutboxError> {
    let exempt = CLOSE_TEXTS.map(str::to_owned);
    let mut tx = conn.begin().await.map_err(|_| OutboxError::Database)?;
    let ids = models::expire_job_rows(&mut tx, (job, closed_at), &exempt).await?;
    for &id in &ids {
        audit_expired(&mut tx, id).await?;
    }
    tx.commit().await.map_err(|_| OutboxError::Database)?;
    Ok(ids.len())
}

/// What a failed send leaves: a listed number's row expires (CON-013), and so does a row whose
/// recipient can never be read or is of the wrong kind (D-3149), so no row stays at the head of the
/// queue; a vault read that may pass is `Database`, so the batch loop holds it (D-3581); a refusal waits for its TXT-014 retry or is given up; a rate limit is held uncounted (D-4052); an unknown outcome is settled; a
/// row gone, blocked (21610) or already claimed needs nothing.
async fn after_error<C, G, P>(
    db: &mut PgConnection,
    s: &OutboxSender<C, G, P>,
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
        OutboxError::RateLimited => rate_limited(db, row, now).await,
        OutboxError::OutcomeUnknown => settle_unknown(db, s, (row.id, None), now).await,
        OutboxError::Database => Err(OutboxError::Database),
        _ => Ok(()),
    }
}

/// TXT-014, TXT-015, QH-008, INT-023: a refused row waits for its next retry; after the last, with
/// the footer's postal line, it is given up (email copy and portal line, or Casey's call-back, due
/// as her job's urgency sets, D-4052). A business row's last refusal expires it: its notice has its
/// own email (BIZ-037, D-4898).
async fn refused<C, G, P>(
    db: &mut PgConnection,
    s: &OutboxSender<C, G, P>,
    row: &Due,
    now: Timestamp,
) -> Result<(), OutboxError> {
    match (retries::record_failure(db, row.id, now).await?, &s.postal) {
        (Step::RetryAt(at), _) => models::hold(db, row.id, at).await,
        (Step::GiveUp, _) if row.business => expire(db, row.id).await,
        (Step::GiveUp, None) => models::hold(db, row.id, now + RETRY_LAST).await,
        (Step::GiveUp, Some(postal)) => {
            let reach = Reach::Homeowner {
                vault: &s.vault,
                urgency: urgency(db, s, row.recipient, now).await?,
                no_rush: NO_RUSH_CALL_BACK,
            };
            let to = email_to(row.recipient, row.language.as_deref());
            retries::give_up(db, (&s.secret, postal), (row.id, reach, &to), now)
                .await
                .map(|_| ())
        }
    }
}

/// INT-023, TXT-015, D-4052: how soon the homeowner with vault contact `contact` needs her
/// call-back, from the sender's [`UrgencySource`]; with none, no rush.
async fn urgency<C, G, P>(
    db: &mut PgConnection,
    s: &OutboxSender<C, G, P>,
    contact: Uuid,
    now: Timestamp,
) -> Result<Urgency, OutboxError> {
    match &s.urgency {
        Some(source) => source.urgency(db, contact, now).await,
        None => Ok(Urgency::NoRush),
    }
}

/// TXT-011, TXT-014, D-4052: a row the carrier's rate limit refused (nothing was sent) is held with
/// no counted failure, so rate limits never give a text up. The hold comes from the row's current
/// one: [`RATE_HOLD`] past the time from its queueing to that hold's end, at most
/// [`RATE_HOLD_MAX`]; a row tried as soon as it is due thus waits 1, 2, 4, 8, then 15 minutes.
async fn rate_limited(db: &mut PgConnection, row: &Due, now: Timestamp) -> Result<(), OutboxError> {
    let held = sqlx::query_scalar!(
        "SELECT not_before FROM messaging.outbox WHERE id = $1",
        row.id
    )
    .fetch_optional(&mut *db)
    .await
    .map_err(|_| OutboxError::Database)?
    .flatten()
    .map(|t| t.to_jiff());
    models::hold(db, row.id, now + rate_hold(row.queued_at, held)).await
}

/// The next rate-limit hold of a row queued at `queued_at` whose current hold ends at `held`.
fn rate_hold(queued_at: Timestamp, held: Option<Timestamp>) -> SignedDuration {
    let waited = held.map_or(SignedDuration::ZERO, |until| {
        until.duration_since(queued_at)
    });
    (waited.max(SignedDuration::ZERO) + RATE_HOLD).min(RATE_HOLD_MAX)
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
        manage_url: MANAGE_URL.as_str(),
    }
}

/// TXT-011, TXT-014, D-558, D-2628: moves `sending` row `id` (with a cutoff: only one claimed
/// before it) to `unknown`, dropping its body; an important text's copy is listed for the portal and
/// emailed when an address and the postal line are on file, and one Normal alert is raised, all in
/// one transaction. The email address is read before it (the vault refuses a read inside one).
async fn settle_unknown<C, G, P>(
    db: &mut PgConnection,
    s: &OutboxSender<C, G, P>,
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
