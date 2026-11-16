//! The public website (`askhava-site`, layer 8; docs/architecture.md section 3).
//!
//! It holds the shared page layout, `templates/base.html`, which every portal page extends through
//! `app/portals/askama.toml` (T051, D-491, D-492), the site layout `templates/site_layout.html`
//! every site page extends (it fills the WEB-007 banner block, D-3144), and the stylesheet in the
//! daylight-and-evergreen look of `docs/design/DESIGN_SPEC.md` (WEB-001, D-3331), compiled into
//! the binary and served at `/static/site/<name>` ([`assets`], [`routes`], T101). Other crates
//! reach it only through [`api`]. The home and how-it-works pages, in English and the latter
//! in Spanish, and the old directory paths are [`pages`] (T102). Autoescaping stays on in every template (SEC-011).

pub mod api;
mod assets;
mod pages;
mod routes;
