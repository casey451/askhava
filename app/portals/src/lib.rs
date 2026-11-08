//! Portals (`askhava-portals`, layer 8): the homeowner, business and owner portals and the shared
//! web security every browser-facing page uses (docs/architecture.md section 3).
//!
//! T051 built the shared web security: sessions and their `__Host-` cookies, CSRF tokens, the
//! cross-site refusal, the SEC-012 headers, `no-store` for contact and photo pages and the
//! SEC-048 security events (SEC-009, SEC-011 to SEC-013, SEC-034, SEC-042, SEC-046, SEC-048).
//! T052 built the homeowner's quote page (`quote_page`, PICK-008 to PICK-012, WORD-005, A11Y-033).
//! T055 built the pick review step and the pick result page (`pick_flow`, PICK-016, PICK-017,
//! PICK-045, PICK-054, A11Y-014); T187 its code-and-contact step (PICK-018, A11Y-010 to A11Y-016).
//! T224 built the business's Confirm review step (`confirm_pay`, PICK-021, FEE-014, FEE-045,
//! CON-029).
//! T068 built the business's "Why was I charged?" page (`receipts_page`, FEE-031, FEE-053).
//! T193 built the business sign-up screens (`business_signup`, BIZ-024, BIZ-026, BIZ-030,
//! BIZ-031, LIC-047, WEB-024).
//! T084 built business sign-in by a single-use 15-minute emailed link, the 30-day business session
//! and the signed-in business principal routes are scoped by (`business_auth`, SEC-004, SEC-019).
//! T230 built the homeowner's private feedback page after Done (`feedback`, JOB-017).
//! T086 built owner sign-in: argon2id password plus a passkey, TOTP or recovery code, 12-hour
//! sessions ending after 30 minutes idle, the 5-minute step-up, the one-time bootstrap and the
//! owner-only powers (`owner_auth`, SEC-005 to SEC-007, SEC-043, SEC-044).
//! Portal pages extend the shared layout `app/site/templates/base.html` through `askama.toml`.
//! Other crates reach it only through [`api`].

pub mod api;
mod business_auth;
mod business_portal;
mod business_signup;
mod confirm_pay;
mod feedback;
mod job_release;
mod models;
mod notice_settings;
mod owner_auth;
mod pick_flow;
mod quote_page;
mod receipts_page;
mod security;
mod session;
#[cfg(test)]
mod tests;
