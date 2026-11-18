//! Ops (`askhava-ops`, layer 1): the audit log, alerts and pause switches (JOB-005, OPS-003, OPS-010, OPS-011, PRIV-028).
//!
//! `ops` records decisions and records only: it writes rows that hold IDs and code names, never a
//! contact value, and never sends or reads anything outside its own `ops` schema
//! (`docs/architecture.md` section 3, D-491). Other crates reach it only through [`api`].

mod alerts;
pub mod api;
mod audit_subjects;
mod digest_rows;
mod help_requests;
mod models;
mod owner_alerts;
mod safe_mode;
mod scorecard_inputs;
mod service;
mod status_banner;
mod volume_check;
