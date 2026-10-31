//! Businesses (`askhava-businesses`, layer 4): each business account and the records matching
//! reads (D-112).
//!
//! T041 added the account status (BIZ-009) with its moves in `status`, the towns, the job-type
//! checklist within the license (BIZ-004), the overflow switch (BIZ-005), the Spanish tick
//! (BIZ-006), the business-reported insurance answer (BIZ-008) and the after-hours tick with
//! `night_count` (BIZ-007, D-491) in `models`, `rules` and `service`.
//!
//! Other crates reach it only through [`api`].

pub mod api;
mod models;
mod rules;
mod service;
mod status;

#[cfg(test)]
mod tests;
