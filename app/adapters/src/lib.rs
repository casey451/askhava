//! Vendor adapters (`askhava-adapters`, layer 1; SEC-018, SEC-032, CALL-017).
//!
//! One trait per vendor, each implemented by one HTTP client whose base URL
//! comes from settings, so the stand-in server, the vendor's sandbox and the
//! live API differ only by configuration (`docs/architecture.md` section 7,
//! D-453). Every trait method that acts outside takes an [`api::IdempotencyKey`]
//! (D-596); a URL a person or vendor supplied is fetched only through
//! [`api::fetch_untrusted`] (SEC-041). Other crates reach the public items only through [`api`].

mod ai;
pub mod api;
mod base;
mod config;
mod email;
mod letters;
mod license_sources;
mod payments;
mod sms;
mod storage;
#[cfg(test)]
mod tests;
mod verify;
mod voice;
