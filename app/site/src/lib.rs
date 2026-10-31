//! The public website (`askhava-site`, layer 8; docs/architecture.md section 3).
//!
//! For now this crate holds only the shared page layout, `templates/base.html`, which every
//! portal page extends through `app/portals/askama.toml` (T051, D-491, D-492). It has no code
//! yet; T101 adds the routes, the stylesheet and the plaster-and-brass look of
//! `docs/design/DESIGN_SPEC.md`. Autoescaping stays on in every template (SEC-011).
