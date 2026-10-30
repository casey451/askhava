# askhava-server: the composition root and both binaries

## Purpose

Builds the two processes from one crate: `server` (the `web` process, Axum) and `worker` (the task runner). It is the only crate that may depend on every module; it wires their routes and task handlers together and owns process concerns (TLS provider, port, signals) (`docs/architecture.md` section 2, D-481).

## What it owns

- `src/lib.rs`: `install_crypto_provider()`, `start_up()` (settings from `std::env::vars()`, then the JSON log and Sentry; D-689), `build_router()` (no routes yet, under the request-ID and `TraceLayer` layers that log the route template only), `shutdown_signal()` (SIGINT or SIGTERM), `register_handlers()` (the `core::api::Registry` of every task handler; empty until a module registers one, T013).
- `src/main.rs`: the `server` binary; binds `0.0.0.0:$PORT` and serves with graceful shutdown. `server migrate` instead applies pending migrations over `ASKHAVA_MIGRATION_DATABASE_URL` (via `askhava_core::api::migrate`) and exits (D-700, SEC-052). Missing or bad `PORT` exits 1 without echoing the value (D-588).
- `src/bin/worker.rs`: the `worker` binary; the claim loop over `core.task` (4 at once, idle poll 200 ms to 5 s, the reaper when idle), and on a signal a 20-second drain, then `release_leases` (JOB-006, D-708).
- No tables.

## Rules it enforces

- SEC-029: workspace lints on (`[lints] workspace = true`), `unsafe_code` forbidden, built by `cargo build -p askhava-server --locked` in `scripts/smoke.sh` and `Ci` (D-488).

## Never break

- `install_crypto_provider()` runs first in both binaries, so sqlx and reqwest share the aws-lc-rs provider (D-481, D-488).
- Both binaries exit 0 on SIGTERM; the web server drains in-flight requests first (D-491).
- Errors at start never echo a setting's value; both binaries call `start_up()` right after the TLS provider, so a bad setting exits 1 before anything is served or claimed (SEC-002).
- Routes are merged before the layers in `build_router()`, so every route gets the request ID and the route-template span (PRIV-016, PRIV-028).
- Any future `src/status.rs` must start with `#![deny(clippy::wildcard_enum_match_arm, clippy::match_wildcard_for_single_variants)]` (SEC-031, D-488).
- The worker gives back its leases after the drain on every signal, so a deploy never strands a task for a lease length (rust brain R14).
- `/healthz` belongs to T106 (`src/healthz.rs`); do not add it here.

## How to change

- Add a module's routes: merge its `routes()` in `build_router()` in `src/lib.rs`; add the path dependency to `Cargo.toml` with `workspace = true`.
- Register a task handler: one `registry.register(kind, handler)` line in `register_handlers()` (`src/lib.rs`); the worker runs whatever it returns.
- Run `cargo build -p askhava-server --locked`, `cargo clippy --workspace --all-targets -- -D warnings` and `bash scripts/smoke.sh`.

## Tests that guard it

- `tests/integration/tests/task_queue.rs` runs `core`'s `Registry::run`, which the worker calls. T175's architecture test (TD-SEC-10) checks the layer table and lints; T106 adds the health check.
