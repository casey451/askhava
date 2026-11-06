//! The homeowner's voice code call, the safety voice call and Casey's bridge to a homeowner
//! (CALL-014, CALL-015, CALL-019, CALL-022, OPS-028; D-206, D-275, D-366, D-384, D-385, D-491,
//! D-1705 to D-1707).
//!
//! [`may_code_call`] lets the code call go only on the homeowner's own tap and never to a business
//! (CALL-014, BIZ-046); [`code_call_twiml`] opens "This is Ask Hava LLC", speaks slowly, says the
//! code twice, reads the main number as the callback (never the number placing the call) and offers
//! "press 1 to hear it again" (CALL-015). The opted-out case is `service::automated_call_gate`'s.
//!
//! The safety voice call is off unless a [`SafetySwitch`] was turned on with a recorded attorney
//! [`Approval`] and every CALL-022 [`Conditions`] item met; at launch it is off (D-384). Intake
//! queues a `core.task` [`SAFETY_CALL_KIND`] event (it may not call `calls`, D-491) that
//! [`place_safety_call`] handles: within 30 minutes of the message, from a pool number other than
//! the main number, after the suppression-list check, to that contact only, with the
//! [`safety_script`]; each placed call is audited with its message, script version and time.
//! [`safety_call_key`] lists the number on "press 9"; [`safety_call_answered`] acknowledges Casey's
//! critical alert through `ops` when a person answered (OPS-028). [`ring_owner`] and
//! [`owner_bridge_twiml`] ring Casey's phone and bridge him to the homeowner, the Ask Hava number
//! shown, never his own (OPS-028); the owner-portal button is T108's.

use askhava_adapters::api::{IdempotencyKey, PoolNumber, VoiceAdapter};
use askhava_messaging::api::{RevokeChannel, suppress};
use askhava_ops::api::{Actor, AuditEntry, Change, acknowledge_alert, audit};
use askhava_privacy::api::{Field, Purpose, Reader, Vault, read_contact};
use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use sqlx::PgConnection;
use sqlx::types::Uuid;

use crate::models::CallsError;
use crate::routing::Party;
use crate::rules::AnsweredBy;
use crate::service::{AutomatedCall, CallGate, automated_call_gate};

/// The `core.task` kind intake queues for a safety voice call (CALL-019, D-491).
pub const SAFETY_CALL_KIND: &str = "calls.safety_call";
/// A safety call goes only this soon after the message that asked for it (CALL-022).
pub const SAFETY_WITHIN: SignedDuration = SignedDuration::from_mins(30);
/// The script version each placed safety call logs (CALL-022).
pub const SAFETY_SCRIPT_VERSION: &str = "safety_script_v1";
/// The audit action of a placed safety call; subject the triggering message (CALL-022).
pub const SAFETY_CALL_ACTION: &str = "calls.safety_call_placed";
/// The audit action of a "press 9" on a safety call (CALL-022, CON-035).
pub const SAFETY_STOP_ACTION: &str = "calls.safety_call_press_9";

/// CALL-014, BIZ-046, D-366: a code call goes only when she tapped for it, and only to a homeowner.
#[must_use]
pub fn may_code_call(tapped: bool, to: Party) -> bool {
    tapped && matches!(to, Party::Homeowner(_))
}

/// The digits of `text` spoken one at a time with pauses ("9, 2, 8"), `None` unless it is an
/// optional `+` and 4 to 15 digits.
fn spoken(text: &str) -> Option<String> {
    let digits = text.strip_prefix('+').unwrap_or(text);
    let ok = (4..=15).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit());
    let mut parts: Vec<String> = digits.chars().map(String::from).collect();
    if text.starts_with("+1") && parts.len() == 11 {
        parts.remove(0);
    }
    ok.then(|| parts.join(", "))
}

/// The main number spoken as the callback in the groups people say ("9 2 8, 5 5 5, 0 1 0 0" for a
/// `+1` number, else digit by digit), `None` when it is not E.164 or is `from`, the number placing
/// the call (CALL-014, CALL-022).
fn callback(main: &str, from: &PoolNumber) -> Option<String> {
    if !main.starts_with('+') || main == from.as_str() {
        return None;
    }
    let said = spoken(main)?;
    let digits: Vec<&str> = said.split(", ").collect();
    Some(match digits.as_slice() {
        [a, b, c, d, e, f, g, h, i, j] => format!("{a} {b} {c}, {d} {e} {f}, {g} {h} {i} {j}"),
        _ => said,
    })
}

/// CALL-014, CALL-015, D-366: the code call's TwiML: "This is Ask Hava LLC", the code said slowly
/// twice, the main number as the callback, then "Press 1 to hear it again" (`again` is the app's
/// own path that replays it). `None` when the code is not 4 to 8 digits, `main` is not E.164 or is
/// `from`. It holds the code: send it only as the webhook's reply, never log it.
#[must_use]
pub fn code_call_twiml(
    from: &PoolNumber,
    main: &str,
    code: &SecretString,
    again: &str,
) -> Option<SecretString> {
    let code = code.expose_secret();
    let said = spoken(code).filter(|_| (4..=8).contains(&code.len()))?;
    let back = callback(main, from)?;
    let words = format!(
        "This is Ask Hava LLC. Your code is {said}. Again, your code is {said}. \
         To call us back, dial {back}. Press 1 to hear it again."
    );
    Some(SecretString::from(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response><Gather input=\"dtmf\" \
         numDigits=\"1\" timeout=\"10\" action=\"{again}\"><Say><prosody rate=\"slow\">{words}\
         </prosody></Say></Gather><Hangup/></Response>"
    )))
}

/// The TCPA attorney's written approval Casey records in the owner portal (CALL-019, D-275).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    /// The approval's date.
    pub date: Date,
    /// Its reference, which must not be blank.
    pub reference: String,
}

/// The CALL-022 items, each true only once built and tested (D-384).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "CALL-022 lists independent yes-or-no conditions"
)]
pub struct Conditions {
    /// Gas, CO, smoke, fire, burning, spark and shock words only (intake's trigger).
    pub narrowed_trigger: bool,
    /// Only within 30 minutes of the message, and only to that number.
    pub recent_only: bool,
    /// The under-30-second script with "press 9".
    pub script: bool,
    /// Placed from a number other than the main number.
    pub other_number: bool,
    /// The suppression list checked before dialing.
    pub list_checked: bool,
    /// The consent text's recorded-call sentence goes live the same day (CON-031).
    pub consent_text: bool,
    /// Each call logged with message, script version, time, result and key press.
    pub logged: bool,
}

impl Conditions {
    /// Every item met.
    #[must_use]
    pub fn all_met(&self) -> bool {
        let c = self;
        c.narrowed_trigger
            && c.recent_only
            && c.script
            && c.other_number
            && c.list_checked
            && c.consent_text
            && c.logged
    }
}

/// The owner-portal "safety voice call" switch (CALL-019, CALL-022): off unless turned on through
/// [`SafetySwitch::turn_on`]. The field is private, so no other path makes an "on" switch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SafetySwitch {
    on: bool,
}

impl SafetySwitch {
    /// The switch at launch: off (D-384).
    #[must_use]
    pub fn off() -> Self {
        Self { on: false }
    }

    /// CALL-019, CALL-022, D-275, D-384: on only with a recorded approval with a reference and
    /// every condition met; otherwise `None`, whatever the approval.
    #[must_use]
    pub fn turn_on(approval: Option<&Approval>, met: &Conditions) -> Option<Self> {
        let approved = approval.is_some_and(|a| !a.reference.trim().is_empty());
        (approved && met.all_met()).then_some(Self { on: true })
    }

    /// Whether safety calls may be placed.
    #[must_use]
    pub fn is_on(self) -> bool {
        self.on
    }
}

/// CALL-019, CALL-022, D-206: the safety call's words, under 30 seconds: the identity opening,
/// leave and call 911, "Ask Hava is not an emergency service", the main number as the callback and
/// "press 9". No job, quote or request detail. `None` when `main` is not E.164 or is `from`.
#[must_use]
pub fn safety_script(main: &str, from: &PoolNumber) -> Option<String> {
    let back = callback(main, from)?;
    Some(format!(
        "This is Ask Hava L L C. You sent us a message about a danger. Leave the building now \
         and call 9 1 1. Ask Hava is not an emergency service. To call us back, dial {back}. \
         Press 9 to stop all calls from Ask Hava."
    ))
}

/// The safety call's TwiML: [`safety_script`] in a one-digit `Gather` posting to `stop` (the app's
/// own path that takes "press 9"), then a hang-up. `None` as for [`safety_script`].
#[must_use]
pub fn safety_twiml(main: &str, from: &PoolNumber, stop: &str) -> Option<String> {
    let words = safety_script(main, from)?;
    Some(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response><Gather input=\"dtmf\" \
         numDigits=\"1\" timeout=\"5\" action=\"{stop}\"><Say>{words}</Say></Gather><Hangup/>\
         </Response>"
    ))
}

/// One queued safety call: IDs and the message's arrival only (PRIV-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SafetyCall {
    /// The homeowner's vault contact ID: the only number dialled.
    pub contact: Uuid,
    /// The triggering `messaging.inbox` message.
    pub message: Uuid,
    /// When it arrived.
    pub arrived_at: Timestamp,
    /// Casey's critical alert about it (OPS-028).
    pub alert: Uuid,
}

impl SafetyCall {
    /// Reads intake's task payload; `None` unless every ID and the time parse.
    #[must_use]
    pub fn from_payload(payload: &Value) -> Option<Self> {
        let id = |k: &str| payload.get(k)?.as_str()?.parse::<Uuid>().ok();
        let arrived_at = payload.get("arrived_at")?.as_str()?.parse().ok()?;
        Some(Self {
            contact: id("contact")?,
            message: id("message")?,
            arrived_at,
            alert: id("alert")?,
        })
    }
}

/// What [`place_safety_call`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyOutcome {
    /// The switch is off: no call; Casey's alert pushes as before (OPS-028).
    SwitchOff,
    /// The message is over 30 minutes old (CALL-022).
    TooLate,
    /// The only number to place it from is the main number (CALL-022).
    FromMain,
    /// The number is on the suppression list or cannot be dialled (CON-035).
    Refused(CallGate),
    /// Placed: the provider's call SID.
    Placed(String),
}

/// What dials a safety call: the voice adapter, the pool number to call from, the main number, the
/// switch and the secret its idempotency key derives from (SEC-018).
pub struct SafetyDialer<'a, V> {
    /// The voice adapter.
    pub voice: &'a V,
    /// The pool number shown.
    pub from: &'a PoolNumber,
    /// The Ask Hava main number, read as the callback.
    pub main: &'a str,
    /// The safety voice call switch.
    pub switch: SafetySwitch,
    /// The key secret (`ASKHAVA_ID_SECRET`).
    pub secret: &'a SecretString,
}

/// CALL-019, CALL-022: the checks before any lookup, in order: switch, 30 minutes, other number.
fn precheck(switch: SafetySwitch, from_main: bool, age: SignedDuration) -> Option<SafetyOutcome> {
    if !switch.is_on() {
        Some(SafetyOutcome::SwitchOff)
    } else if age > SAFETY_WITHIN || age.is_negative() {
        Some(SafetyOutcome::TooLate)
    } else {
        from_main.then_some(SafetyOutcome::FromMain)
    }
}

/// The handler of [`SAFETY_CALL_KIND`] (CALL-019, CALL-022, D-491, D-1705): checks the switch, the
/// 30 minutes and the calling number, then the suppression list (`automated_call_gate`), reads her
/// phone under the bridge-call grant and dials once (key derived from the alert, so a re-run never
/// dials twice), and audits the placed call with the message, [`SAFETY_SCRIPT_VERSION`] and time.
/// Call it outside a transaction: the vault refuses a read inside one.
///
/// # Errors
///
/// [`CallsError::Messaging`] when the list or phone cannot be read, [`CallsError::Voice`] when the
/// provider refuses, [`CallsError::Ops`] when the audit row cannot be written.
pub async fn place_safety_call<V: VoiceAdapter>(
    db: &mut PgConnection,
    vault: &Vault,
    dialer: &SafetyDialer<'_, V>,
    call: &SafetyCall,
    now: Timestamp,
) -> Result<SafetyOutcome, CallsError> {
    let from_main = dialer.from.as_str() == dialer.main;
    if let Some(stop) = precheck(
        dialer.switch,
        from_main,
        now.duration_since(call.arrived_at),
    ) {
        return Ok(stop);
    }
    let gate = automated_call_gate(db, vault, (call.contact, AutomatedCall::Safety), now).await?;
    if gate != CallGate::Dial {
        return Ok(SafetyOutcome::Refused(gate));
    }
    let phone = phone_of(db, vault, call.contact).await?;
    let alert = call.alert.to_string();
    let key = IdempotencyKey::derive(dialer.secret, SAFETY_CALL_KIND, &[&alert]);
    let state = dialer
        .voice
        .open_call(&key, phone.expose_secret(), dialer.from)
        .await
        .map_err(|_| CallsError::Voice)?;
    log(db, SAFETY_CALL_ACTION, call.message).await?;
    Ok(SafetyOutcome::Placed(state.sid))
}

/// Her phone, read under `(Calls, BridgeCall)` (PRIV-003).
async fn phone_of(
    db: &mut PgConnection,
    vault: &Vault,
    contact: Uuid,
) -> Result<SecretString, CallsError> {
    let access = (Reader::Calls, Purpose::BridgeCall);
    let details = read_contact(db, vault, access, contact, &[Field::Phone])
        .await
        .map_err(|_| CallsError::Messaging)?;
    details.phone.ok_or(CallsError::Messaging)
}

/// One system audit row about the triggering message, naming the script version (CALL-022).
async fn log(db: &mut PgConnection, action: &'static str, message: Uuid) -> Result<(), CallsError> {
    let entry = AuditEntry {
        actor: Actor::System,
        action,
        subject_kind: "messaging.inbox",
        subject_id: message,
        change: Change::Field(SAFETY_SCRIPT_VERSION),
    };
    audit(db, &entry).await.map_err(|_| CallsError::Ops)?;
    Ok(())
}

/// CALL-022, CON-035: a "9" pressed on a safety call lists her number on the one suppression list
/// and is audited; any other key does nothing. Returns whether she was listed now.
///
/// # Errors
///
/// [`CallsError::Messaging`] or [`CallsError::Ops`].
pub async fn safety_call_key(
    db: &mut PgConnection,
    vault: &Vault,
    call: &SafetyCall,
    digits: &str,
    now: Timestamp,
) -> Result<bool, CallsError> {
    if digits.trim() != "9" {
        return Ok(false);
    }
    let phone = phone_of(db, vault, call.contact).await?;
    let how = (RevokeChannel::Owner, "voice:press_9");
    let listed = suppress(db, vault, phone.expose_secret(), how, now)
        .await
        .map_err(|_| CallsError::Messaging)?;
    log(db, SAFETY_STOP_ACTION, call.message).await?;
    Ok(listed)
}

/// OPS-028, D-1706: a safety call a person answered acknowledges Casey's critical alert through
/// `ops` (audited, listed in the weekly digest); any other answer leaves it pushing. Returns
/// whether it was acknowledged now.
///
/// # Errors
///
/// [`CallsError::Ops`].
pub async fn safety_call_answered(
    db: &mut PgConnection,
    call: &SafetyCall,
    answered: AnsweredBy,
    now: Timestamp,
) -> Result<bool, CallsError> {
    if answered != AnsweredBy::Person {
        return Ok(false);
    }
    acknowledge_alert(db, call.alert, now)
        .await
        .map_err(|_| CallsError::Ops)
}

/// OPS-028, D-1707: rings Casey's phone from the Ask Hava pool number `from` for the alert `alert`
/// (one call per alert and press, by `press`); his answered leg then gets [`owner_bridge_twiml`].
///
/// # Errors
///
/// [`CallsError::Voice`].
pub async fn ring_owner<V: VoiceAdapter>(
    voice: &V,
    secret: &SecretString,
    (owner_phone, from): (&SecretString, &PoolNumber),
    (alert, press): (Uuid, Uuid),
) -> Result<String, CallsError> {
    let ids = [alert.to_string(), press.to_string()];
    let key = IdempotencyKey::derive(secret, "calls.owner_bridge", &[&ids[0], &ids[1]]);
    let state = voice
        .open_call(&key, owner_phone.expose_secret(), from)
        .await
        .map_err(|_| CallsError::Voice)?;
    Ok(state.sid)
}

/// OPS-028, CALL-006: Casey's leg's TwiML: reads `safety_words` (the INT-004 text, no job detail)
/// to him, then dials the homeowner with the Ask Hava number `from` shown, never his own, never
/// recorded and with nothing played to her. `None` when her number is not E.164.
#[must_use]
pub fn owner_bridge_twiml(
    from: &PoolNumber,
    homeowner: &SecretString,
    safety_words: &str,
) -> Option<SecretString> {
    let to = homeowner.expose_secret();
    let digits = to.strip_prefix('+')?;
    if !(8..=15).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let from = from.as_str();
    let words = safety_words
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    Some(SecretString::from(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Response><Say>{words}</Say>\
         <Dial callerId=\"{from}\" record=\"do-not-record\"><Number>{to}</Number></Dial>\
         </Response>"
    )))
}
