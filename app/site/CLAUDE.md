# askhava-site: the public website

## Purpose
The layer-8 crate for the public website (`docs/architecture.md` section 3). It holds the shared page layout `templates/base.html` that every site and portal page extends (T051, D-491, D-492), the site layout `templates/site_layout.html` with the WEB-007 banner, and the compiled-in stylesheet served at `/static/site/<name>` in the daylight-and-evergreen look of `docs/design/DESIGN_SPEC.md` (T101, D-3331).

## Never break
- Every portal page extends `base.html` through `app/portals/askama.toml`; renaming a block breaks every page: `cargo test -p askhava-portals`.
- `base.html`'s `banner` block stays empty and only `site_layout.html` fills it, so no portal struct needs a `banner_shown` field; the banner is hidden by default: `cargo test -p askhava-site` (`td_web_01_banner_hidden_by_default_and_shown_when_on`).
- The site's files are served by one `nest_service("/static/site", ...)` with no `.route(` and no `/`-prefixed `*_PATH`/`*_ROUTE` constant (D-3106): `app/server/tests/route_wiring.rs`.
- No third-party load in any template or static file: `tests/security/tests/no_third_party.rs`.
- WEB-012, D-3144, D-4252: the old paths `/categories` and `/events` answer 200 with the home page; the one catch-all is the router's fallback `not_found`, so any other unknown path stays a 404 (the styled page, `no-store`, banner read at most once a minute) and never shadows a route: `tests/e2e/site_pages.spec.ts`, `tests/e2e/not_found.spec.ts`, `app/server/tests/route_wiring.rs`.
- WEB-028, D-466: the home page's numbers come only from `intake::api::home_numbers` and show nothing below `ASKHAVA_HOME_NUMBERS_MIN_JOBS`; no number is typed: `tests/integration/tests/home_numbers.rs`.

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/site/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/site/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
