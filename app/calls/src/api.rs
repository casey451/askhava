//! The public face of `askhava-calls`: everything other crates may call (D-481).
//!
//! The masked-number pool (CALL-001, CALL-002, CALL-017): [`assign_number`] gives a connected
//! job its [`Line`] from the fixed pool in settings, built on standard programmable voice only
//! (CALL-021); and [`automated_call_gate`], the suppression-list check before an automated call
//! (CON-035, CALL-014).

pub use crate::models::CallsError;
pub use crate::service::{
    ASSIGNED_ACTION, AutomatedCall, CallGate, Connected, Line, POOL_ALERT,
    SAFETY_NOT_DIALABLE_ALERT, assign_number, automated_call_gate,
};
