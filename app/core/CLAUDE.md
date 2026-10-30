# askhava-core: the shared kernel

## Purpose

The layer-0 crate every module may call: settings, the database pool and guarded updates, the clock, derived IDs, webhook dedupe, the `core.task` queue and rate limits (`docs/architecture.md` section 5, D-090). It is not a business module and owns no business data. Right now it is an empty crate (T009); T011 to T014 add its files.

## What it owns

- `src/lib.rs`: the crate root; one `mod` line per file a task adds.
- Tables (from T012 on): the `core` schema only, `core.task` and `core.rate_limit` (D-090, D-091).

## Rules it enforces

- SEC-029: one crate per module with `unsafe_code` forbidden and the workspace lints on (`[lints] workspace = true` in `Cargo.toml`).

## Never break

- `core` depends on no other first-party crate: it is layer 0 in `kit/layers.json`; T175's architecture test guards the layers (TD-SEC-10).
- No `unsafe`, no `#[allow]` without a reason, functions under 50 lines (workspace `Cargo.toml` and `clippy.toml`, D-585).
- Any future `src/status.rs` must start with `#![deny(clippy::wildcard_enum_match_arm, clippy::match_wildcard_for_single_variants)]` (SEC-031, D-488).
- Settings come in as pairs (`Settings::from_pairs`), never through `std::env::set_var` (D-481, D-491).

## How to change

- Add a kernel file: create `src/<name>.rs` with a `//!` header, add one `mod` line to `src/lib.rs`, re-export public items through `src/api.rs` (it does not exist yet: the first task that adds a public item creates `src/api.rs` and adds `pub mod api;` to `src/lib.rs`), and update this file and `docs/architecture.md` section 5.
- Add a dependency: it must be in `[workspace.dependencies]` and `kit/allowed_crates.txt`, named here with `workspace = true`; a new crate needs a `docs/decisions.md` entry (BUILD_BRIEF.md:122).
- Run `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check` and `bash scripts/smoke.sh`.

## Tests that guard it

- None yet. T011 to T014 add `src/tests/` files; T175's architecture test (TD-SEC-10) checks the layer rule and `[lints] workspace = true`.
