//! The public face of `askhava-calls`: everything other crates may call (D-481).
//!
//! The masked-number pool (CALL-001, CALL-002, CALL-017): [`assign_number`] gives a connected
//! job its [`Line`] from the fixed pool in settings, built on standard programmable voice only
//! (CALL-021); and [`automated_call_gate`], the suppression-list check before an automated call
//! (CON-035, CALL-014). T077: [`route`] and [`answer_call`], PIN routing and the paid-business
//! read-back (CALL-003 to CALL-005, CALL-025). T078: [`dial_twiml`], never recorded (CALL-006);
//! [`finished_call`], [`log_finished_call`] and [`call_log`], the call log, and the pure
//! [`talked`] and [`business_attempt`] rules (CALL-007, CALL-008).

pub use crate::handlers::{
    FinishedCall, LoggedCall, call_log, dial_twiml, finished_call, log_finished_call,
};
pub use crate::models::CallsError;
pub use crate::routing::{
    Answer, Attestation, CallTurn, IncomingCall, JobPin, PIN_ALERT, Party, READBACK_AFTER,
    RoutedLine, Step, TRIES_PER_CALL, TRIES_PER_DAY, answer_call, job_pin, route,
};
pub use crate::rules::{
    AnsweredBy, Caller, REPORT_MARK, answered_by, business_attempt, reached_report_mark, talked,
};
pub use crate::service::{
    ASSIGNED_ACTION, AutomatedCall, CallGate, Connected, Line, POOL_ALERT,
    SAFETY_NOT_DIALABLE_ALERT, assign_number, automated_call_gate,
};
