//! Portals (`askhava-portals`, layer 8): the homeowner, business and owner portals and the shared
//! web security every browser-facing page uses (docs/architecture.md section 3).
//!
//! T051 built the shared web security: sessions and their `__Host-` cookies, CSRF tokens, the
//! cross-site refusal, the SEC-012 headers, `no-store` for contact and photo pages and the
//! SEC-048 security events (SEC-009, SEC-011 to SEC-013, SEC-034, SEC-042, SEC-046, SEC-048).
//! T052 built the homeowner's quote page (`quote_page`, PICK-008 to PICK-012, WORD-005, A11Y-033).
//! Portal pages extend the shared layout `app/site/templates/base.html` through `askama.toml`.
//! Other crates reach it only through [`api`].

pub mod api;
mod quote_page;
mod security;
mod session;
#[cfg(test)]
mod tests;
