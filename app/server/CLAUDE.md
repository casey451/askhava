# askhava-server: the composition root and both binaries

## Purpose

Builds the two processes from one crate: `server` (the `web` process, Axum) and `worker` (the task runner). It is the only crate that may depend on every module; it wires their routes and task handlers together and owns process concerns (TLS provider, port, signals) (`docs/architecture.md` section 2, D-481).

## What it owns

- `src/lib.rs`: `install_crypto_provider()`, `start_up()` (settings from `std::env::vars()`, then the JSON log and Sentry; D-689), `build_router()` (no routes yet, under the request-ID and `TraceLayer` layers that log the route template only), `shutdown_signal()` (SIGINT or SIGTERM); `register_handlers()` does not exist yet (T013, the handler-registry task, creates it).
- `src/main.rs`: the `server` binary; binds `0.0.0.0:$PORT` and serves with graceful shutdown. Missing or bad `PORT` exits 1 without echoing the value (D-588).
- `src/bin/worker.rs`: the `worker` binary; idles until a signal until T013 adds the claim loop (D-588).
- No tables.

## Rules it enforces

- SEC-029: workspace lints on (`[lints] workspace = true`), `unsafe_code` forbidden, built by `cargo build -p askhava-server --locked` in `scripts/smoke.sh` and `Ci` (D-488).

## Never break

- `install_crypto_provider()` runs first in both binaries, so sqlx and reqwest share the aws-lc-rs provider (D-481, D-488).
- Both binaries exit 0 on SIGTERM; the web server drains in-flight requests first (D-491).
- Errors at start never echo a setting's value; both binaries call `start_up()` right after the TLS provider, so a bad setting exits 1 before anything is served or claimed (SEC-002).
- Routes are merged before the layers in `build_router()`, so every route gets the request ID and the route-template span (PRIV-016, PRIV-028).
- Any future `src/status.rs` must start with `#![deny(clippy::wildcard_enum_match_arm, clippy::match_wildcard_for_single_variants)]` (SEC-031, D-488).
- `/healthz` belongs to T106 (`src/healthz.rs`); do not add it here.

## How to change

- Add a module's routes: merge its `routes()` in `build_router()` in `src/lib.rs`; add the path dependency to `Cargo.toml` with `workspace = true`.
- Register a task handler: add it in `register_handlers()` (`src/lib.rs`; it does not exist yet, T013, the handler-registry task, creates it) and `src/bin/worker.rs`.
- Run `cargo build -p askhava-server --locked`, `cargo clippy --workspace --all-targets -- -D warnings` and `bash scripts/smoke.sh`.

## Tests that guard it

- None yet. T175's architecture test (TD-SEC-10) checks the layer table and lints; T106 adds the health check.
