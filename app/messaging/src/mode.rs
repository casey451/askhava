//! Web-and-email mode, number suspension and the one configured number per audience (TXT-021 to
//! TXT-024, TXT-034, TXT-035, CON-033; D-144, D-206, D-210, D-275, D-297, D-303, D-315, D-1804 to
//! D-1808).
//!
//! Each audience (homeowners, businesses) has its own [`Line`]: its texting switch, its one
//! configured [`SendingNumber`] and the provider's [`NumberState`]. A text goes only from that
//! number while the switch is on and the number works ([`Line::sender`]), and [`send_on_line`] is
//! the send that takes its `From` from there and refuses while the owner's `texting` or `all`
//! pause is on; there is no list of numbers to fall back on, so no second number is ever used
//! (TXT-022). One audience's switch or suspension never changes the other's line (TXT-035).
//! While texting is paused an update goes by email and the quote page or portal, or, for a
//! homeowner with no email, onto Casey's call list ([`update_route`], TXT-023). An emergency
//! message while paused gets the plan of [`emergency_plan`] (TXT-024); [`raise_emergency`] raises
//! its alert once per inbox message and keeps its `voice_call` for one message per homeowner and
//! suspension, and that is the `switch_on` intake's `enqueue_safety_call` takes (D-491).
//! [`TollFree`] is CON-033's code rule.

use askhava_adapters::api::{SentText, SmsAdapter};
use askhava_ops::api::{
    Actor, AuditEntry, Change, NewAlert, OpsError, Severity, Switch, audit_once, is_paused,
    raise_alert,
};
use askhava_privacy::api::Vault;
use jiff::Timestamp;
use jiff::civil::Date;
use secrecy::SecretString;
use sqlx::PgConnection;
use sqlx::types::Uuid;
use thiserror::Error;

use crate::models::OutboxError;
use crate::optout::send_checked_as;
use crate::service::To;
use crate::templates::Audience;

/// The critical alert kind raised for an emergency message while texting is paused (TXT-024).
pub const PAUSED_EMERGENCY_ALERT: &str = "messaging.paused_emergency";

/// The `ops.audit_log` action claiming one inbox message's emergency handling (D-1808).
const HANDLED_ACTION: &str = "messaging.paused_emergency_handled";

/// The `ops.audit_log` action claiming the one safety call per homeowner and suspension (D-1808).
const CALL_ACTION: &str = "messaging.paused_safety_call";

/// Why a mode change or a toll-free configuration was refused (TXT-022, TXT-024, CON-033).
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ModeError {
    /// The number is not a `+1` and ten-digit E.164 number.
    #[error("not a +1 E.164 number")]
    BadNumber,
    /// Texting goes back on only after the provider confirmed the number restored (D-144).
    #[error("the provider has not confirmed the number restored")]
    NotRestored,
    /// CON-033: no refusal of the homeowner campaign is recorded.
    #[error("no recorded campaign refusal")]
    NoRefusal,
    /// CON-033: Casey has not given his go.
    #[error("no go from the owner")]
    NoGo,
    /// TXT-022, CON-033: the fallback never stands in for a suspended number.
    #[error("the homeowner number is suspended")]
    Suspended,
    /// CON-033: the toll-free number carries homeowner templates only.
    #[error("business templates never go from the toll-free number")]
    BusinessTemplate,
}

/// One audience's configured texting number (TXT-022): checked once, private field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendingNumber(String);

impl SendingNumber {
    /// Checks `raw` is `+1` and ten digits.
    ///
    /// # Errors
    ///
    /// [`ModeError::BadNumber`].
    pub fn parse(raw: &str) -> Result<Self, ModeError> {
        let digits = raw.strip_prefix("+1").ok_or(ModeError::BadNumber)?;
        let ok = digits.len() == 10 && digits.bytes().all(|b| b.is_ascii_digit());
        ok.then(|| Self(raw.to_owned())).ok_or(ModeError::BadNumber)
    }

    /// The number as configured.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the provider reported about a line's number and what Casey did (TXT-022, TXT-024, D-144).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberState {
    /// Texting may go.
    Working,
    /// Suspended at `since`: all texting on the number paused.
    Suspended {
        /// When the suspension began.
        since: Timestamp,
    },
    /// The provider reports it restored; texting stays off until Casey switches it on.
    Restored {
        /// When the suspension began.
        since: Timestamp,
    },
}

impl NumberState {
    /// TXT-022: a suspension at `at`; one already running keeps its start.
    #[must_use]
    pub fn suspend(self, at: Timestamp) -> Self {
        match self {
            Self::Suspended { since } => Self::Suspended { since },
            Self::Working | Self::Restored { .. } => Self::Suspended { since: at },
        }
    }

    /// TXT-024, D-144: the provider confirms the number restored; texting stays paused.
    #[must_use]
    pub fn provider_restored(self) -> Self {
        match self {
            Self::Suspended { since } => Self::Restored { since },
            other => other,
        }
    }

    /// TXT-024, D-144: Casey switches texting back on, only after the provider's confirmation.
    ///
    /// # Errors
    ///
    /// [`ModeError::NotRestored`] while the number is still suspended.
    pub fn switch_on(self) -> Result<Self, ModeError> {
        match self {
            Self::Suspended { .. } => Err(ModeError::NotRestored),
            Self::Working | Self::Restored { .. } => Ok(Self::Working),
        }
    }

    /// Whether texting on the number is paused (suspended, or restored and waiting for Casey).
    #[must_use]
    pub fn paused(self) -> bool {
        self != Self::Working
    }
}

/// The launch-channel setting (TXT-021, D-297): the service may start in web-and-email mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchChannel {
    /// Texts as usual.
    Sms,
    /// Web chat, the quote page, the portal and email only (TXT-034).
    WebAndEmail,
}

/// One audience's texting line (TXT-021, TXT-022, TXT-035).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The audience's own texting switch; off is web-and-email mode for that audience.
    pub texting_on: bool,
    /// The one configured number, if any.
    pub number: Option<SendingNumber>,
    /// The provider's state of that number.
    pub state: NumberState,
}

impl Line {
    /// TXT-021, D-297: a line started on `launch`; texting is on only for SMS with a number.
    #[must_use]
    pub fn at_launch(launch: LaunchChannel, number: Option<SendingNumber>) -> Self {
        Self {
            texting_on: launch == LaunchChannel::Sms && number.is_some(),
            number,
            state: NumberState::Working,
        }
    }

    /// TXT-021, TXT-022, TD-TXT-08: the number a text goes from: the configured one while the
    /// switch is on and it works, otherwise none. Never any other number.
    #[must_use]
    pub fn sender(&self) -> Option<&SendingNumber> {
        let open = self.texting_on && !self.state.paused();
        self.number.as_ref().filter(|_| open)
    }
}

/// Both audiences' lines; one's switch or suspension never touches the other (TXT-035, D-303).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lines {
    /// The homeowner line.
    pub homeowner: Line,
    /// The business line (its switch is off by default, TXT-035).
    pub business: Line,
}

impl Lines {
    /// The line of `audience`.
    #[must_use]
    pub fn line(&self, audience: Audience) -> &Line {
        match audience {
            Audience::Homeowner => &self.homeowner,
            Audience::Business => &self.business,
        }
    }
}

/// Why [`send_on_line`] sent nothing.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum LineSendError {
    /// The recipient's line has no open sender, or the owner paused texting (OPS-003, TXT-021).
    #[error("texting on this line is paused")]
    Paused,
    /// The send itself failed (`optout::send_checked`).
    #[error(transparent)]
    Outbox(#[from] OutboxError),
}

/// TXT-021, TXT-022, TXT-035, OPS-003, D-1808: sends outbox row `id` through
/// `optout::send_checked` from the line of `to`'s audience ([`Line::sender`]), never another
/// number; [`LineSendError::Paused`] with nothing sent while that line is closed or the owner's
/// `texting` or `all` pause switch is on (both audiences).
///
/// # Errors
///
/// [`LineSendError::Paused`], or any error of `send_checked` as [`LineSendError::Outbox`].
pub async fn send_on_line(
    db: &mut PgConnection,
    keys: (&SecretString, &(impl SmsAdapter + Sync), &Vault),
    lines: &Lines,
    row: (Uuid, To<'_>),
    now: Timestamp,
) -> Result<Option<SentText>, LineSendError> {
    send_on_line_as(db, keys, lines, row, (now, None)).await
}

/// [`send_on_line`] with the recipient's number when the caller already read it from the vault
/// for row `id` (the outbox sender, D-3149), passed on to `optout::send_checked_as`.
pub(crate) async fn send_on_line_as(
    db: &mut PgConnection,
    (secret, carrier, vault): (&SecretString, &(impl SmsAdapter + Sync), &Vault),
    lines: &Lines,
    (id, to): (Uuid, To<'_>),
    (now, phone): (Timestamp, Option<&SecretString>),
) -> Result<Option<SentText>, LineSendError> {
    let audience = match to {
        To::Homeowner { .. } => Audience::Homeowner,
        To::Business(_) => Audience::Business,
    };
    let from = lines.line(audience).sender().ok_or(LineSendError::Paused)?;
    for switch in [Switch::Texting, Switch::All] {
        if is_paused(db, switch)
            .await
            .map_err(|_| OutboxError::Database)?
        {
            return Err(LineSendError::Paused);
        }
    }
    let from = from.as_str();
    let dest = (to, from);
    Ok(send_checked_as(db, (secret, carrier, vault), id, dest, (now, phone)).await?)
}

/// How one update reaches a person (TXT-021, TXT-022, TXT-023, TXT-034).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateRoute {
    /// By text from the line's configured number.
    Text(SendingNumber),
    /// By email, and on the quote page or the portal.
    Email,
    /// Only on the quote page or the portal: a business with no address on file (BIZ-046: never
    /// a call to a business).
    PortalOnly,
    /// On the quote page, and the homeowner goes on Casey's call list (TXT-023).
    CallList,
}

/// TXT-021 to TXT-023, D-1804: text while the audience's line is open; otherwise email when one
/// is on file, else the page plus Casey's call list for a homeowner (the switch off or the
/// number suspended alike) and the portal alone for a business.
#[must_use]
pub fn update_route(lines: &Lines, audience: Audience, has_email: bool) -> UpdateRoute {
    match (lines.line(audience).sender(), has_email, audience) {
        (Some(number), _, _) => UpdateRoute::Text(number.clone()),
        (None, true, _) => UpdateRoute::Email,
        (None, false, Audience::Homeowner) => UpdateRoute::CallList,
        (None, false, Audience::Business) => UpdateRoute::PortalOnly,
    }
}

/// The facts about one inbound homeowner message the emergency plan needs (TXT-024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmergencyFacts {
    /// Intake's 911-list match (INT-002); the AI flag never stands in for it.
    pub matched: bool,
    /// Intake's narrower `safety_call_trigger` (CALL-022).
    pub call_trigger: bool,
    /// When the message arrived.
    pub arrived_at: Timestamp,
    /// Whether an email is on file for her.
    pub has_email: bool,
    /// Whether her number is on the suppression list (CON-013, D-210).
    pub opted_out: bool,
    /// The owner-portal safety voice call switch (CALL-019, CALL-022, D-275).
    pub safety_switch_on: bool,
}

/// What an emergency message gets while homeowner texting is paused (TXT-024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmergencyPlan {
    /// The critical [`PAUSED_EMERGENCY_ALERT`]; false for an opted-out number, whose critical
    /// alert `optout::on_inbound_text` raises on the same inbox row (CON-017, D-1808).
    pub critical_alert: bool,
    /// The INT-004 text by email.
    pub safety_by_email: bool,
    /// The INT-004 text in the web chat.
    pub safety_in_web_chat: bool,
    /// The site status banner turns on without Casey, texting off or suspended (D-144, D-1808).
    pub banner_on: bool,
    /// A safety voice call is asked for; [`raise_emergency`] keeps one per homeowner and suspension.
    pub voice_call: bool,
}

/// TXT-024, CALL-019, CALL-022, D-144, D-206, D-210, D-275, D-1808: `None` while the homeowner
/// line is open (the INT-004 text goes as usual) or for a message with no 911-list match. Paused
/// (switched off or suspended): the INT-004 text in the web chat and by email when on file, the
/// banner, the critical alert unless optout's covers it; and the voice call only for her own
/// message sent since a suspension began (`Suspended`, or `Restored` and still waiting for
/// Casey), with the switch on, intake's call trigger, and her number not opted out.
#[must_use]
pub fn emergency_plan(line: &Line, facts: &EmergencyFacts) -> Option<EmergencyPlan> {
    if line.sender().is_some() || !facts.matched {
        return None;
    }
    let during = suspended_since(line).is_some_and(|since| facts.arrived_at >= since);
    Some(EmergencyPlan {
        critical_alert: !facts.opted_out,
        safety_by_email: facts.has_email,
        safety_in_web_chat: true,
        banner_on: true,
        voice_call: during && facts.safety_switch_on && facts.call_trigger && !facts.opted_out,
    })
}

/// When the line's running suspension began; a restored number waiting for Casey is still in it
/// (D-144, D-1806, D-1808).
fn suspended_since(line: &Line) -> Option<Timestamp> {
    match line.state {
        NumberState::Suspended { since } | NumberState::Restored { since } => Some(since),
        NumberState::Working => None,
    }
}

/// What [`raise_emergency`] did for one inbox message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Raised {
    /// The [`PAUSED_EMERGENCY_ALERT`] raised, the `alert` of intake's safety-call ask.
    pub alert: Option<Uuid>,
    /// The `switch_on` for intake's `enqueue_safety_call`: true for the first message per
    /// homeowner and suspension whose plan asked for a call (then `alert` is set).
    pub voice_call: bool,
}

/// TXT-024, CALL-019, D-1808: in the caller's transaction, claims inbox row `message` once
/// (`ops::audit_once`; `None` on a retry: the first run's commit holds its alert and call), raises
/// the plan's critical alert, and keeps the plan's voice call only when no earlier message from
/// contact `her` in this suspension claimed one, so a second emergency message queues no second
/// call. Both claims hold the ops advisory lock to the end of the caller's transaction.
///
/// # Errors
///
/// [`OpsError::Database`].
pub async fn raise_emergency(
    db: &mut PgConnection,
    (plan, line): (&EmergencyPlan, &Line),
    (her, message): (Uuid, Uuid),
) -> Result<Option<Raised>, OpsError> {
    if !audit_once(db, &claim(HANDLED_ACTION, "messaging.inbox", message)).await? {
        return Ok(None);
    }
    let alert = if plan.critical_alert {
        let new = NewAlert {
            kind: PAUSED_EMERGENCY_ALERT,
            severity: Severity::Critical,
            subject: Some(("messaging.inbox", message)),
        };
        Some(raise_alert(db, &new).await?)
    } else {
        None
    };
    let voice_call = match suspended_since(line) {
        Some(since) if plan.voice_call && alert.is_some() => {
            // Her contact ID XOR the suspension's start: one key per homeowner and suspension;
            // random v4 contact IDs make a clash between two homeowners practically impossible.
            let key =
                Uuid::from_u128(her.as_u128() ^ u128::from(since.as_millisecond().cast_unsigned()));
            audit_once(db, &claim(CALL_ACTION, "messaging.contact_suspension", key)).await?
        }
        _ => false,
    };
    Ok(Some(Raised { alert, voice_call }))
}

/// A system audit row claiming `(kind, id)` for `action`.
fn claim(action: &'static str, kind: &'static str, id: Uuid) -> AuditEntry {
    AuditEntry {
        actor: Actor::System,
        action,
        subject_kind: kind,
        subject_id: id,
        change: Change::Action,
    }
}

/// A recorded refusal of the homeowner 10DLC campaign (CON-033): its date and the provider's
/// reference (an error code such as 30951).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// When it was refused.
    pub date: Date,
    /// The provider's reference.
    pub reference: String,
}

/// The toll-free fallback for the homeowner stream (CON-033, D-315); private fields, so only
/// [`TollFree::configure`] makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TollFree {
    verified: bool,
}

impl TollFree {
    /// CON-033, TXT-022, D-315, D-1806: configured only with a recorded refusal (non-empty
    /// reference) and Casey's go, and never while the homeowner number is suspended or waiting
    /// for Casey after one. Starts unverified.
    ///
    /// # Errors
    ///
    /// [`ModeError::NoRefusal`], [`ModeError::NoGo`], [`ModeError::Suspended`].
    pub fn configure(
        refusal: Option<&Refusal>,
        casey_go: bool,
        homeowner: NumberState,
    ) -> Result<Self, ModeError> {
        if !refusal.is_some_and(|r| !r.reference.trim().is_empty()) {
            return Err(ModeError::NoRefusal);
        }
        if !casey_go {
            return Err(ModeError::NoGo);
        }
        if homeowner.paused() {
            return Err(ModeError::Suspended);
        }
        Ok(Self { verified: false })
    }

    /// The provider verified the toll-free number.
    #[must_use]
    pub fn verified(self) -> Self {
        Self { verified: true }
    }

    /// CON-033, D-315: web-and-email until verified, then SMS.
    #[must_use]
    pub fn launch_channel(&self) -> LaunchChannel {
        if self.verified {
            LaunchChannel::Sms
        } else {
            LaunchChannel::WebAndEmail
        }
    }

    /// CON-033, TXT-035: homeowner templates only.
    ///
    /// # Errors
    ///
    /// [`ModeError::BusinessTemplate`] for a business template.
    pub fn may_carry(&self, audience: Audience) -> Result<(), ModeError> {
        match audience {
            Audience::Homeowner => Ok(()),
            Audience::Business => Err(ModeError::BusinessTemplate),
        }
    }
}
