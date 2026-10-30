# askhava-core: the shared kernel

## Purpose

The layer-0 crate every module may call: settings, the database pool and guarded updates, the clock, derived IDs, webhook dedupe, the `core.task` queue and rate limits (`docs/architecture.md` section 5, D-090). It is not a business module and owns no business data. T011 added settings, the clock and logging; T012 to T014 add the rest.

## What it owns

- `src/lib.rs`: the crate root; one `mod` line per file a task adds.
- `src/api.rs`: the only public surface; re-exports the items below.
- `src/settings.rs`: `Settings::from_pairs`, `Environment`, `Vendor`, `SettingsError`, `check_base_url` (D-688). A shared path: each task adds its own `ASKHAVA_*` key to `KEYS` and a typed field (D-493).
- `src/clock.rs`: `Clock`, `SystemClock`, `ManualClock`, `arizona()`, `arizona_time`, `zone`, `local_time`, `is_business_hours`, `add_business_minutes`.
- `src/logging.rs`: `Redacted<T>`, `init` (re-exported as `init_logging`), `json_layer`, `request_span`, `sentry_options`, `scrub_event`, `scrub_breadcrumb`, `scrub_text` (D-689).
- Tables (from T012 on): the `core` schema only, `core.task` and `core.rate_limit` (D-090, D-091).

## Rules it enforces

- SEC-001, SEC-002, SEC-003, SEC-036: `settings::Settings::from_pairs` (unknown key, missing value, live key outside live, test key in live, an account identifier off its allow-list or on the other's); `settings::check_base_url` (a stand-in URL in live, SEC-032).
- JOB-007: `clock::is_business_hours`, `clock::add_business_minutes` (8:00 a.m. to 8:00 p.m. Arizona time, every day, D-004).
- PRIV-016, PRIV-028: `logging::json_layer` (one JSON line, `message` and `level` on top), `logging::request_span` (route template and request ID, never the URI), `logging::Redacted`.
- PRIV-017: `logging::sentry_options` (`send_default_pii` off) and `logging::scrub_event`.
- SEC-029: one crate per module with `unsafe_code` forbidden and the workspace lints on (`[lints] workspace = true` in `Cargo.toml`).

## Never break

- `core` depends on no other first-party crate: it is layer 0 in `kit/layers.json`; T175's architecture test guards the layers (TD-SEC-10).
- No `unsafe`, no `#[allow]` without a reason, functions under 50 lines (workspace `Cargo.toml` and `clippy.toml`, D-585).
- Any future `src/status.rs` must start with `#![deny(clippy::wildcard_enum_match_arm, clippy::match_wildcard_for_single_variants)]` (SEC-031, D-488).
- Settings come in as pairs (`Settings::from_pairs`), never through `std::env::set_var` (D-481, D-491).
- No settings error echoes a value, only the key: `tests/settings.rs::invalid_values_are_refused_without_echoing_them`.
- `Redacted` has no `Serialize` and prints only `[redacted]`: `tests/logging.rs::redacted_values_never_reach_a_log`.
- Request spans never carry the URI: `tests/logging.rs::request_log_shows_route_template_only`.
- Arizona is a fixed UTC-7 (no daylight saving): `tests/clock.rs::arizona_has_no_daylight_saving`.

## How to change

- Add a kernel file: create `src/<name>.rs` with a `//!` header, add one `mod` line to `src/lib.rs`, re-export its public items through `src/api.rs`, and update this file and `docs/architecture.md` section 5.
- Add a dependency: it must be in `[workspace.dependencies]` and `kit/allowed_crates.txt`, named here with `workspace = true`; a new crate needs a `docs/decisions.md` entry (BUILD_BRIEF.md:122).
- Add a setting: add the `ASKHAVA_*` name to `KEYS` in `src/settings.rs`, a private field with a `///` getter naming its requirement, parse it in `from_pairs` (errors carry the key only), add the row to `docs/deploy.md` when it exists, and a case in `src/tests/settings.rs`. A vendor base URL goes through `check_base_url`.
- Change business hours: `BUSINESS_OPEN_HOUR` and `BUSINESS_CLOSE_HOUR` in `src/clock.rs` (JOB-007 and D-004 first), then `cargo test -p askhava-core tests::clock`.
- Run `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check` and `bash scripts/smoke.sh`.

## Tests that guard it

- `src/tests/settings.rs` (TD-SEC-15, TD-SEC-13), `src/tests/clock.rs` (JOB-007, with property tests), `src/tests/logging.rs` (TD-PRIV-13). T012 to T014 add their own files; T175's architecture test (TD-SEC-10) checks the layer rule and `[lints] workspace = true`.
