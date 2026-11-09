//! The public face of `askhava-calls`: everything other crates may call (D-481).
//!
//! The masked-number pool (CALL-001, CALL-002, CALL-017): [`assign_number`] gives a connected
//! job its [`Line`] from the fixed pool in settings, built on standard programmable voice only
//! (CALL-021); and [`automated_call_gate`], the suppression-list check before an automated call
//! (CON-035, CALL-014). T077: [`route`] and [`answer_call`], PIN routing and the paid-business
//! read-back (CALL-003 to CALL-005, CALL-025). T078: [`dial_twiml`], never recorded (CALL-006);
//! [`finished_call`], [`log_finished_call`] and [`call_log`], the call log, and the pure
//! [`talked`] and [`business_attempt`] rules (CALL-007, CALL-008). T079: [`close_line`] and
//! [`close_expired`] (CALL-009, CALL-010), [`gate`] and [`phone_works`] (CALL-012),
//! [`request_link`] (CALL-011) and [`greeting_twiml`] (CALL-020). T080: [`PinWait`],
//! [`pin_prompt_twiml`] and [`pin_heard`] (CALL-013, CALL-023); [`may_code_call`] and
//! [`code_call_twiml`] (CALL-014, CALL-015); [`SafetySwitch`], [`place_safety_call`],
//! [`safety_call_key`] and [`safety_call_answered`] (CALL-019, CALL-022, OPS-028); [`ring_owner`]
//! and [`owner_bridge_twiml`] (OPS-028). T262: a random stored PIN per line ([`stored_pin`]),
//! the stored per-day PIN tries and the inbound voice turn ([`voice_turn`], [`voice_confirm`],
//! [`readback_twiml`], [`hangup_twiml`]) (CALL-003 to CALL-005, CALL-025, SEC-017, D-1755).

pub use crate::handlers::{
    FinishedCall, LoggedCall, call_log, dial_twiml, finished_call, log_finished_call,
};
pub use crate::lifecycle::{
    BUSINESS_NOTICE_KIND, CANCELLED_TEMPLATE, CloseReason, GREETING_EN, GREETING_ES, LINE_DAYS,
    LINK_TEMPLATE, LINK_TEXT_TO, PhoneStanding, close_expired, close_line, expired, gate,
    greeting_twiml, last4, phone_works, request_link,
};
pub use crate::models::CallsError;
pub use crate::pins::{
    PIN_DRAWS, PIN_TRY_LABEL, VoiceCall, VoiceKeys, VoiceReply, arizona_day, call_start,
    caller_digest, pin_tries_today, stored_pin, voice_confirm, voice_turn,
};
pub use crate::routing::{
    Answer, Attestation, CallTurn, IncomingCall, JobPin, PIN_ALERT, PIN_GOODBYE, PIN_PROMPT,
    PIN_WAIT_KEY, PIN_WAIT_MIN, Party, PinHeard, PinWait, READBACK_AFTER, READBACK_CONFIRM,
    RoutedLine, Step, TRIES_PER_CALL, TRIES_PER_DAY, answer_call, hangup_twiml, job_pin, pin_heard,
    pin_prompt_twiml, readback_twiml, route,
};
pub use crate::rules::{
    AnsweredBy, Caller, REPORT_MARK, answered_by, business_attempt, reached_report_mark, talked,
};
pub use crate::service::{
    ASSIGNED_ACTION, AutomatedCall, CallGate, Connected, Line, POOL_ALERT,
    SAFETY_NOT_DIALABLE_ALERT, assign_number, automated_call_gate,
};
pub use crate::voice::{
    Approval, Conditions, SAFETY_CALL_ACTION, SAFETY_CALL_KIND, SAFETY_CLAIM_ACTION,
    SAFETY_FAILED_ACTION, SAFETY_KEY_ACTION, SAFETY_RESULT_ACTION, SAFETY_SCRIPT_VERSION,
    SAFETY_STOP_ACTION, SAFETY_WITHIN, SafetyCall, SafetyDialer, SafetyOutcome, SafetySwitch,
    VoiceError, code_call_twiml, may_code_call, owner_bridge_twiml, place_safety_call, ring_owner,
    safety_call_answered, safety_call_key, safety_script, safety_twiml,
};

/// Whether job `job` ever had a masked line, open or closed (CALL-001, CALL-010, D-1688): the
/// release's line retry (`picks.line_retry`) opens one only when this is false, so a line closed
/// after 30 days is never reopened.
///
/// # Errors
///
/// [`CallsError::Database`].
pub async fn job_had_line(
    db: &mut sqlx::PgConnection,
    job: sqlx::types::Uuid,
) -> Result<bool, CallsError> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM calls.number_pool WHERE job_id = $1) AS "had!""#,
        job
    )
    .fetch_one(db)
    .await
    .map_err(|_| CallsError::Database)
}
