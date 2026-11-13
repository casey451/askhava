# askhava-site: the public website

## Purpose
The layer-8 crate for the public website (`docs/architecture.md` section 3). For now it holds only the shared page layout `templates/base.html` that every site and portal page extends (T051, D-491, D-492). T101 adds the routes, the stylesheet and the plaster-and-brass look of `docs/design/DESIGN_SPEC.md`.

## Never break
- Every portal page extends `base.html` through `app/portals/askama.toml`; renaming a block breaks every page: `cargo test -p askhava-portals`.
- No third-party load in any template or static file: `tests/security/tests/no_third_party.rs`.

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/site/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/site/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
