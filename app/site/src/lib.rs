//! The public website (`askhava-site`, layer 8; docs/architecture.md section 3).
//!
//! It holds the shared page layout, `templates/base.html`, which every portal page extends through
//! `app/portals/askama.toml` (T051, D-491, D-492), the site layout `templates/site_layout.html`
//! every site page extends (it fills the WEB-007 banner block, D-3144), and the stylesheet in the
//! daylight-and-evergreen look of `docs/design/DESIGN_SPEC.md` (WEB-001, D-3331), compiled into
//! the binary and served at `/static/site/<name>` ([`assets`], [`routes`], T101). Other crates
//! reach it only through [`api`]. The home and how-it-works pages, in English and the latter
//! in Spanish, and the old directory paths are [`pages`] (T102), as are the services, coverage,
//! help and ROC complaint pages (T201). The legal and data pages are
//! `legal_pages` (T221). The request page and its web chat at `/request` are [`chat`] (T489). Autoescaping stays on in every template; only `legal.html` prints a
//! draft's rendered Markdown, whose raw HTML is shown as text (SEC-011).

pub mod api;
mod assets;
mod chat;
mod legal_pages;
mod pages;
mod routes;
#[cfg(test)]
mod tests;
