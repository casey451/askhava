//! The shared kernel (`askhava-core`, D-090; SEC-029).
//!
//! Every module needs settings, the database pool, the clock, derived IDs,
//! the task queue and rate limits; none of them belongs to a business module,
//! so they live here, in layer 0, where any crate may call them
//! (`docs/architecture.md` section 5). Each task that adds a file adds one
//! `mod` line here; other crates reach the public items only through [`api`].

pub mod api;
mod clock;
mod logging;
mod settings;
#[cfg(test)]
mod tests;
