//! The public face of `askhava-ops`: everything other crates may call (D-481).
//!
//! The audit log, written in the caller's transaction and exported by cursor (JOB-005, OPS-010,
//! OPS-011, PRIV-028); alerts holding record IDs only (PRIV-006); and the owner pause switches with
//! their append-only history (OPS-003).

pub use crate::models::{
    Actor, AuditEntry, AuditRow, Change, NewAlert, OpsError, Severity, Switch, is_code_name,
};
pub use crate::service::{
    EXPORT_PAGE_MAX, audit, export_audit, is_paused, raise_alert, set_switch,
};
