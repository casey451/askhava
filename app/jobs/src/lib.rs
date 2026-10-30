//! Jobs (`askhava-jobs`, layer 4): the job lifecycle state machine (JOB-001 to JOB-003, JOB-005,
//! JOB-006; `docs/architecture.md` section 6).
//!
//! Every job holds one of twelve statuses and moves only along the section 6 arrows; each move
//! writes a status-history row and an audit row and queues the new status's timer in the same
//! transaction. Other crates reach it only through [`api`].

pub mod api;
mod models;
mod rules;
mod service;
mod status;

#[cfg(test)]
mod tests;
