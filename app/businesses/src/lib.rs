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
//! T082 added the E-SIGN consent records, the welcome request, the business STOP email request,
//! the auto-reply rule and the recruiting-email rules in `consent` (CON-019, CON-020, BIZ-017,
//! BIZ-032, BIZ-048).
//!
//! T083 added the complaints (the BIZ-011 reply, the BIZ-029 reply box, the BIZ-012 pause and
//! the digest listing) and the owner's phone-link email request in `complaints` (BIZ-033).
//!
//! T149 added the sign-up inbox check with its sample card and the sample pick step in
//! `deliverability_check` (BIZ-042) and each person's backup notice address, which
//! `notice_addresses` now adds, in `backup_address` (BIZ-043).
//!
//! T374 added the licensing holder read in `holding` (LIC-019, BIZ-049, CARD-001, D-2712).
//!
//! Other crates reach it only through [`api`].

mod about_us;
pub mod api;
mod backup_address;
mod capability;
mod channel_preference;
mod complaint_holds;
mod complaints;
mod consent;
mod deliverability_check;
mod email_verification;
mod emergency_switch;
mod holding;
mod identity;
mod models;
mod open_cards_times;
mod rules;
mod service;
mod signup;
mod status;
mod team;

#[cfg(test)]
mod tests;
