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
//! [`owner_bridge_twiml`] ring Casey's phone and bridge him to the homeowner, an Ask Hava pool
//! number shown (not the main number, D-1707), never his own (OPS-028); the owner-portal button is
//! T108's.
//!
//! T378 (D-2762, D-2771, D-3112, D-3231): each call goes with an `adapters` [`CallPlan`]: the safety
//! call runs [`safety_twiml`] inline (its `Gather` posting to [`SAFETY_KEY_PATH`] on the public
//! base) with Twilio's answering-machine detection, the owner bridge runs [`owner_bridge_twiml`];
//! both post their progress to [`SAFETY_STATUS_PATH`]. The query of both safety URLs is
//! [`SafetyCall::query`] (record IDs and the arrival second, never a number), the bridge's
//! `bridge=<alert>`. [`safety_call_status`] decides a safety call once, by the detection result or
//! a final no-answer ([`safety_answer`]), and [`safety_key_once`] takes its key press once.

use askhava_adapters::api::{CallPlan, IdempotencyKey, PoolNumber, VoiceAdapter};
use askhava_core::api::brand;
use askhava_messaging::api::{RevokeChannel, suppress};
use askhava_ops::api::{Actor, AuditEntry, Change, acknowledge_alert, audit, audit_once, audited};
use askhava_privacy::api::{Field, Purpose, Reader, Vault, read_contact};
use jiff::civil::Date;
use jiff::{SignedDuration, Timestamp};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use sqlx::types::Uuid;
use sqlx::{Connection, PgConnection};

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
/// The claim committed before a safety call is dialled; subject Casey's critical alert, so one
/// emergency gets at most one call however often the task runs (CALL-019, JOB-006, D-1705).
pub const SAFETY_CLAIM_ACTION: &str = "calls.safety_call_dialing";
/// The audit action when the provider refused a claimed safety call; it is not dialled again.
pub const SAFETY_FAILED_ACTION: &str = "calls.safety_call_failed";
/// The audit action of a safety call's result: `to_status` is `person`, `machine`, `unknown` or
/// `nobody` (CALL-022).
pub const SAFETY_RESULT_ACTION: &str = "calls.safety_call_result";
/// The audit action of a key other than 9 pressed on a safety call; `field` names the key
/// (`key_0` to `key_8`, `key_star`, `key_hash`, `key_other`) (CALL-022).
pub const SAFETY_KEY_ACTION: &str = "calls.safety_call_key";
/// The mark, about Casey's alert, that a safety call's result was decided; the first deciding
/// callback writes it and every later one changes nothing (OPS-028, D-2771).
pub const SAFETY_DECIDED_ACTION: &str = "calls.safety_call_decided";
/// The mark, about Casey's alert, that a safety call's key press was taken (CALL-022, D-2771).
pub const SAFETY_KEY_TAKEN_ACTION: &str = "calls.safety_call_key_taken";
/// Where Twilio posts a placed call's progress and detection result (OPS-028, D-2762).
pub const SAFETY_STATUS_PATH: &str = "/webhooks/twilio/voice/status";
/// Where the safety call's `Gather` posts the key pressed (CALL-022, CON-035, D-2771).
pub const SAFETY_KEY_PATH: &str = "/webhooks/twilio/voice/safety-key";

/// Why placing a call failed: a `calls` step, or the voice provider.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum VoiceError {
    /// A lookup, the list check or the audit row failed.
    #[error(transparent)]
    Calls(#[from] CallsError),
    /// The voice provider refused or failed to place the call.
    #[error("the voice provider could not place the call")]
    Provider,
    /// The call's script could not be built, so no call was placed (D-2762).
    #[error("the call's script could not be built")]
    Script,
}

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
    let digits_only = (4..=8).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_digit());
    let said = spoken(code).filter(|_| digits_only)?;
    let back = callback(main, from)?;
    let words = format!(
        "This is {}. Your code is {said}. Again, your code is {said}. \
         To call us back, dial {back}. Press 1 to hear it again.",
        brand().legal_name()
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
    /// Every item met. `logged` may be set only once the voice route records every answer through
    /// [`safety_call_answered`] and every key through [`safety_call_key`] (T096, D-1705).
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
    let (spoken, name) = (brand().spoken_legal_name(), brand().display_name());
    Some(format!(
        "This is {spoken}. You sent us a message about a danger. Leave the building now \
         and call 9 1 1. {name} is not an emergency service. To call us back, dial {back}. \
         Press 9 to stop all calls from {name}."
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

    /// The query of this call's key and status URLs: record IDs and the arrival second only, never
    /// a number (PRIV-016, D-3231). Twilio signs it, so nobody can change it.
    #[must_use]
    pub fn query(&self) -> String {
        format!(
            "alert={}&message={}&contact={}&arrived={}",
            self.alert,
            self.message,
            self.contact,
            self.arrived_at.as_second()
        )
    }

    /// Reads [`SafetyCall::query`] back; `None` for any other query.
    #[must_use]
    pub fn from_query(query: &str) -> Option<Self> {
        let parts: Vec<(&str, &str)> = query.split('&').filter_map(|p| p.split_once('=')).collect();
        let [("alert", a), ("message", m), ("contact", c), ("arrived", t)] = parts.as_slice()
        else {
            return None;
        };
        Some(Self {
            contact: c.parse().ok()?,
            message: m.parse().ok()?,
            arrived_at: Timestamp::from_second(t.parse().ok()?).ok()?,
            alert: a.parse().ok()?,
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
    /// A call for this alert was already claimed: nothing dialled (CALL-019, JOB-006).
    AlreadyPlaced,
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
    /// `ASKHAVA_PUBLIC_BASE_URL`, which the call's key and status URLs start with (SEC-016).
    pub public_base: &'a str,
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
/// phone under the bridge-call grant, commits the [`SAFETY_CLAIM_ACTION`] claim on the alert
/// (`ops::api::audit_once`; a claim already there is [`SafetyOutcome::AlreadyPlaced`], so a retried
/// or doubled task never dials twice), dials (key derived from the alert) and audits the placed
/// call with the message, [`SAFETY_SCRIPT_VERSION`] and time, or a refused one with
/// [`SAFETY_FAILED_ACTION`] (it is not retried: at most one call per emergency). The call runs
/// [`safety_twiml`] inline with machine detection, its key and status URLs on the public base
/// ([`safety_plan`], D-2762); a script that cannot be built is logged as a refused call and is
/// [`VoiceError::Script`].
/// Call it outside a transaction: the vault refuses a read inside one.
///
/// # Errors
///
/// [`VoiceError::Calls`] when the list or phone cannot be read or the audit row cannot be written,
/// [`VoiceError::Provider`] when the provider refuses, [`VoiceError::Script`] as above.
pub async fn place_safety_call<V: VoiceAdapter>(
    db: &mut PgConnection,
    vault: &Vault,
    dialer: &SafetyDialer<'_, V>,
    call: &SafetyCall,
    now: Timestamp,
) -> Result<SafetyOutcome, VoiceError> {
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
    if !claim(db, call.alert).await? {
        return Ok(SafetyOutcome::AlreadyPlaced);
    }
    let Some(plan) = safety_plan(dialer, call) else {
        log(db, SAFETY_FAILED_ACTION, call.message, script_field()).await?;
        return Err(VoiceError::Script);
    };
    let alert = call.alert.to_string();
    let key = IdempotencyKey::derive(dialer.secret, SAFETY_CALL_KIND, &[&alert]);
    let Ok(state) = dialer
        .voice
        .open_call(&key, phone.expose_secret(), dialer.from, &plan)
        .await
    else {
        log(db, SAFETY_FAILED_ACTION, call.message, script_field()).await?;
        return Err(VoiceError::Provider);
    };
    log(db, SAFETY_CALL_ACTION, call.message, script_field()).await?;
    Ok(SafetyOutcome::Placed(state.sid))
}

/// The safety call's plan (CALL-022, OPS-028, D-2762, D-2771): [`safety_twiml`] with its `Gather`
/// posting to the absolute [`SAFETY_KEY_PATH`] URL (inline TwiML has no URL to resolve a relative
/// one against; `&` written `&amp;` in the attribute), progress and detection to
/// [`SAFETY_STATUS_PATH`], both with [`SafetyCall::query`]. `None` as for [`safety_twiml`].
fn safety_plan<V>(dialer: &SafetyDialer<'_, V>, call: &SafetyCall) -> Option<CallPlan> {
    let (base, query) = (dialer.public_base, call.query());
    let stop = format!("{base}{SAFETY_KEY_PATH}?{}", query.replace('&', "&amp;"));
    let twiml = safety_twiml(dialer.main, dialer.from, &stop)?;
    let status = format!("{base}{SAFETY_STATUS_PATH}?{query}");
    Some(CallPlan::new(SecretString::from(twiml), status)?.detecting_machine())
}

/// The claim on `alert`, committed before dialling; false when one is already there.
async fn claim(db: &mut PgConnection, alert: Uuid) -> Result<bool, CallsError> {
    let entry = AuditEntry {
        actor: Actor::System,
        action: SAFETY_CLAIM_ACTION,
        subject_kind: "ops.alert",
        subject_id: alert,
        change: script_field(),
    };
    audit_once(db, &entry).await.map_err(|_| CallsError::Ops)
}

/// The script version as the audit row's field (CALL-022).
fn script_field() -> Change {
    Change::Field(SAFETY_SCRIPT_VERSION)
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

/// One system audit row about the triggering message (CALL-022): its time is the row's.
async fn log(
    db: &mut PgConnection,
    action: &'static str,
    message: Uuid,
    change: Change,
) -> Result<(), CallsError> {
    let entry = AuditEntry {
        actor: Actor::System,
        action,
        subject_kind: "messaging.inbox",
        subject_id: message,
        change,
    };
    audit(db, &entry).await.map_err(|_| CallsError::Ops)?;
    Ok(())
}

/// CALL-022, CON-035: every key pressed on a safety call is audited about the message; a "9" lists
/// her number on the one suppression list ([`SAFETY_STOP_ACTION`]), any other key is only logged
/// ([`SAFETY_KEY_ACTION`], the key in `field`). Returns whether she was listed now.
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
    let pressed = digits.trim();
    if pressed != "9" {
        log(
            db,
            SAFETY_KEY_ACTION,
            call.message,
            Change::Field(key_name(pressed)),
        )
        .await?;
        return Ok(false);
    }
    let phone = phone_of(db, vault, call.contact).await?;
    let how = (RevokeChannel::Owner, "voice:press_9");
    let listed = suppress(db, vault, phone.expose_secret(), how, now)
        .await
        .map_err(|_| CallsError::Messaging)?;
    log(db, SAFETY_STOP_ACTION, call.message, Change::Field("key_9")).await?;
    Ok(listed)
}

/// The code name of a pressed key other than 9 (never the raw input, PRIV-028).
fn key_name(pressed: &str) -> &'static str {
    match pressed {
        "0" => "key_0",
        "1" => "key_1",
        "2" => "key_2",
        "3" => "key_3",
        "4" => "key_4",
        "5" => "key_5",
        "6" => "key_6",
        "7" => "key_7",
        "8" => "key_8",
        "*" => "key_star",
        "#" => "key_hash",
        _ => "key_other",
    }
}

/// CALL-022, OPS-028, D-1706: logs the call's result ([`SAFETY_RESULT_ACTION`], `to_status`
/// `person`, `machine`, `unknown` or `nobody`) about the message; a person answering also
/// acknowledges Casey's critical alert through `ops` (audited, listed in the weekly digest); any
/// other answer leaves it pushing. Returns whether it was acknowledged now.
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
    let to = match answered {
        AnsweredBy::Person => "person",
        AnsweredBy::Machine => "machine",
        AnsweredBy::Unknown => "unknown",
        AnsweredBy::Nobody => "nobody",
    };
    let result = Change::Status { from: "placed", to };
    log(db, SAFETY_RESULT_ACTION, call.message, result).await?;
    if answered != AnsweredBy::Person {
        return Ok(false);
    }
    acknowledge_alert(db, call.alert, now)
        .await
        .map_err(|_| CallsError::Ops)
}

/// OPS-028, D-2771: who answered a safety call, from one Twilio callback: the detection result
/// (`AnsweredBy` `human` a person, `machine_…` or `fax` a machine, anything else unknown) or a final
/// `CallStatus` of `no-answer`, `busy`, `failed` or `canceled` (nobody). `None` for every other
/// callback: with asynchronous detection `answered` comes before the result and without it.
#[must_use]
pub fn safety_answer(call_status: Option<&str>, detected: Option<&str>) -> Option<AnsweredBy> {
    if let Some(d) = detected {
        return Some(crate::rules::answered_by("completed", Some(d)));
    }
    matches!(
        call_status,
        Some("no-answer" | "busy" | "failed" | "canceled")
    )
    .then_some(AnsweredBy::Nobody)
}

/// CALL-022, OPS-028, D-2771: one safety-call callback. A deciding one ([`safety_answer`]) runs
/// [`safety_call_answered`] once per call: the [`SAFETY_DECIDED_ACTION`] mark on the alert and the
/// result commit together, so the first deciding callback wins. `None` when the callback decides
/// nothing or the call was already decided; else whether the alert was acknowledged now.
///
/// # Errors
///
/// [`CallsError::Database`] or [`CallsError::Ops`]; nothing is kept then.
pub async fn safety_call_status(
    db: &mut PgConnection,
    call: &SafetyCall,
    (call_status, detected): (Option<&str>, Option<&str>),
    now: Timestamp,
) -> Result<Option<bool>, CallsError> {
    let Some(answered) = safety_answer(call_status, detected) else {
        return Ok(None);
    };
    let mut tx = db.begin().await.map_err(|_| CallsError::Database)?;
    if !mark(&mut tx, SAFETY_DECIDED_ACTION, call.alert).await? {
        return Ok(None);
    }
    let acknowledged = safety_call_answered(&mut tx, call, answered, now).await?;
    tx.commit().await.map_err(|_| CallsError::Database)?;
    Ok(Some(acknowledged))
}

/// CALL-022, CON-035, D-2771: the safety call's key press, taken once per call: [`safety_call_key`]
/// unless the [`SAFETY_KEY_TAKEN_ACTION`] mark is on the alert, then the mark. A failed step leaves
/// no mark. `None` for a repeat; else whether her number was listed now. Call it outside a
/// transaction (the vault read).
///
/// # Errors
///
/// As [`safety_call_key`].
pub async fn safety_key_once(
    db: &mut PgConnection,
    vault: &Vault,
    call: &SafetyCall,
    digits: &str,
    now: Timestamp,
) -> Result<Option<bool>, CallsError> {
    let subject = ("ops.alert", call.alert);
    if audited(db, SAFETY_KEY_TAKEN_ACTION, subject)
        .await
        .map_err(|_| CallsError::Ops)?
    {
        return Ok(None);
    }
    let listed = safety_call_key(db, vault, call, digits, now).await?;
    mark(db, SAFETY_KEY_TAKEN_ACTION, call.alert).await?;
    Ok(Some(listed))
}

/// Writes the system mark `action` about the alert once; false when it was already there.
async fn mark(
    db: &mut PgConnection,
    action: &'static str,
    alert: Uuid,
) -> Result<bool, CallsError> {
    let entry = AuditEntry {
        actor: Actor::System,
        action,
        subject_kind: "ops.alert",
        subject_id: alert,
        change: Change::Action,
    };
    audit_once(db, &entry).await.map_err(|_| CallsError::Ops)
}

/// OPS-028, D-1707, D-2762: rings Casey's phone from the Ask Hava pool number `from` for the alert
/// `alert` (one call per alert and press, by `press`); the call runs [`owner_bridge_twiml`] for
/// `homeowner` and `safety_words` inline, so her number is never in a URL, and posts its progress to
/// [`SAFETY_STATUS_PATH`]`?bridge=<alert>` on `public_base`, which changes nothing.
///
/// # Errors
///
/// [`VoiceError::Script`] when her number is not E.164 (nothing dialled), else
/// [`VoiceError::Provider`].
pub async fn ring_owner<V: VoiceAdapter>(
    voice: &V,
    secret: &SecretString,
    (owner_phone, from): (&SecretString, &PoolNumber),
    (alert, press): (Uuid, Uuid),
    (homeowner, safety_words): (&SecretString, &str),
    public_base: &str,
) -> Result<String, VoiceError> {
    let status = format!("{public_base}{SAFETY_STATUS_PATH}?bridge={alert}");
    let plan = owner_bridge_twiml(from, homeowner, safety_words)
        .and_then(|twiml| CallPlan::new(twiml, status))
        .ok_or(VoiceError::Script)?;
    let ids = [alert.to_string(), press.to_string()];
    let key = IdempotencyKey::derive(secret, "calls.owner_bridge", &[&ids[0], &ids[1]]);
    let state = voice
        .open_call(&key, owner_phone.expose_secret(), from, &plan)
        .await
        .map_err(|_| VoiceError::Provider)?;
    Ok(state.sid)
}

/// OPS-028, CALL-006: Casey's leg's TwiML: reads `safety_words` (the INT-004 text, no job detail)
/// to him, then dials the homeowner with the pool number `from` shown (not the main number,
/// D-1707), never his own, never recorded and with nothing played to her. `None` when her number is not E.164.
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
