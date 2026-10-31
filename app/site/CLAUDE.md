# askhava-site: the public website

## Purpose

The layer-8 crate for the public website (`docs/architecture.md` section 3). For now it holds only the shared page layout `templates/base.html` that every site and portal page extends (T051, D-491, D-492). T101 adds the routes, the stylesheet and the plaster-and-brass look of `docs/design/DESIGN_SPEC.md`.

## What it owns

- `templates/base.html`: the plain shared layout: `lang`, the `title` and `main` blocks, a skip link to `#main`, a header with the wordmark link.
- `src/lib.rs`: the crate doc only; no code yet.

## Rules it enforces

- SEC-011: `base.html` escapes every value (no `|safe`, checked by `scripts/check_rust_rules.py`).
- SEC-013: `base.html` loads no script, style or font from another host (`tests/security/tests/no_third_party.rs`).
- A11Y-030: a page's `title` block begins "Error: " after a failed submit; the layout keeps `main` first for the error summary.

## Never break

- Every portal page extends `base.html` through `app/portals/askama.toml`; renaming a block breaks every page: `cargo test -p askhava-portals`.
- No third-party load in any template or static file: `tests/security/tests/no_third_party.rs`.

## How to change

- Restyle or add a layout block: edit `templates/base.html`, keep the `title`, `main` and `lang` blocks, run `cargo test -p askhava-portals` and `python3 scripts/check_copy.py`.

## Tests that guard it

- `tests/security/tests/no_third_party.rs` (TD-SEC-04): `cargo test -p askhava-security --test no_third_party`.
- `app/portals/src/security.rs` `tests` (TD-SEC-04): `cargo test -p askhava-portals`.
