//! Picks (`askhava-picks`, layer 6): the homeowner's pick and everything after it
//! (docs/architecture.md section 3).
//!
//! T054 built the page pick's 6-digit Verify code (10 minutes, 5 tries, a 15-minute cooldown, the
//! PICK-055 send caps) and the "<number> YES" text pick: the Terms line first, one job in Quotes
//! ready, a second YES replacing a waiting pick, the status reply, and the CANCEL pause's
//! withdrawal (PICK-013 to PICK-016, PICK-031, PICK-032, PICK-036, PICK-055, CON-029).
//! T062 added the money notices: the FEE-020 card-fix notice to the business's money recipients
//! (D-492). T063 added the Confirm window (4 business hours from the pick notice's send, a setting),
//! the license checks at the pick and at Confirm, the recurring fee line, the business's YES and
//! CONFIRM reply and the pick notice to the owner and every "Confirm and billing" member (PICK-019,
//! PICK-020, PICK-033 to PICK-035, PICK-037). T190 added the pure HOLD, Away and emergency-window
//! rules (PICK-043, PICK-044, BIZ-034). T069 added the release: details to the signed-in portal
//! only on a `ReleaseProof`, never the phone (PICK-022 to PICK-024, PRIV-005, PRIV-020, BIZ-014).
//! T070 added the aftermath: the loss notice with its price comparison, "This job is no longer
//! active." once and the live-pick reply (PICK-026, PICK-027, PICK-038).
//! T095 added the pick code by email when texting is off and the opted-out "<number> YES" pick
//! (PICK-029, PICK-030).
//! T191 added what follows a release the business cannot turn into a call: the TXT-026 reminders,
//! the TXT-030 missed-call text, the 96-hour unreachable refund and close, the 72-hour no-call
//! return, the dispute auto-submit mark and the fraud-warning end (FEE-022, FEE-050, FEE-052,
//! JOB-013, JOB-027).
//! Other crates reach it only through [`api`].

mod aftermath;
pub mod api;
mod confirm;
mod email_code;
mod handlers;
mod models;
mod money_notices;
mod release;
mod rules;
mod service;
mod status;
mod unreachable_refund;

#[cfg(test)]
mod tests;
