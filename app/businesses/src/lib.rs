//! Businesses (`askhava-businesses`, layer 4): each business account and the records matching
//! reads (D-112).
//!
//! T041 added the account status (BIZ-009) with its moves in `status`, the towns, the job-type
//! checklist within the license (BIZ-004), the overflow switch (BIZ-005), the Spanish tick
//! (BIZ-006), the business-reported insurance answer (BIZ-008) and the after-hours tick with
//! `night_count` (BIZ-007, D-491) in `models`, `rules` and `service`.
//!
//! T220 added the team members with their roles, the staff phones they link and the notice-address
//! lookup in `team` (BIZ-023, CALL-012, D-491, D-492).
//!
//! T081 added sign-up in `signup` (BIZ-001, BIZ-019 to BIZ-021, BIZ-053, FEE-011, TERMS-033), the
//! identity chain and activation in `identity` (BIZ-002, BIZ-003, BIZ-016) and the About us screen
//! in `about_us` (BIZ-022).
//!
//! Other crates reach it only through [`api`].

mod about_us;
pub mod api;
mod identity;
mod models;
mod rules;
mod service;
mod signup;
mod status;
mod team;

#[cfg(test)]
mod tests;
