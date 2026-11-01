//! Picks (`askhava-picks`, layer 6): the homeowner's pick and everything after it
//! (docs/architecture.md section 3).
//!
//! T054 built the page pick's 6-digit Verify code (10 minutes, 5 tries, a 15-minute cooldown, the
//! PICK-055 send caps) and the "<number> YES" text pick: the Terms line first, one job in Quotes
//! ready, a second YES replacing a waiting pick, the status reply, and the CANCEL pause's
//! withdrawal (PICK-013 to PICK-016, PICK-031, PICK-032, PICK-036, PICK-055, CON-029).
//! Other crates reach it only through [`api`].

pub mod api;
mod models;
mod rules;
mod service;
mod status;

#[cfg(test)]
mod tests;
