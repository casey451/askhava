# askhava-server: the composition root and both binaries

## Purpose

Builds the two processes from one crate: `server` (the `web` process, Axum) and `worker` (the task runner). It is the only crate that may depend on every module; it wires their routes and task handlers together and owns process concerns (TLS provider, port, signals) (`docs/architecture.md` section 2, D-481).

## What it owns

- `src/lib.rs`: `install_crypto_provider()`, `build_router()` (empty until a module adds routes), `shutdown_signal()` (SIGINT or SIGTERM); later `register_handlers()`.
- `src/main.rs`: the `server` binary; binds `0.0.0.0:$PORT` and serves with graceful shutdown. Missing or bad `PORT` exits 1 without echoing the value (D-588).
- `src/bin/worker.rs`: the `worker` binary; idles until a signal until T013 adds the claim loop (D-588).
- No tables.

## Rules it enforces

- SEC-029: workspace lints on (`[lints] workspace = true`), `unsafe_code` forbidden, built by `cargo build -p askhava-server --locked` in `scripts/smoke.sh` and `Ci` (D-488).

## Never break

- `install_crypto_provider()` runs first in both binaries, so sqlx and reqwest share the aws-lc-rs provider (D-481, D-488).
- Both binaries exit 0 on SIGTERM; the web server drains in-flight requests first (D-491).
- Errors at start never echo a setting's value.
- `/healthz` belongs to T106 (`src/healthz.rs`); do not add it here.

## How to change

- Add a module's routes: merge its `routes()` in `build_router()` in `src/lib.rs`; add the path dependency to `Cargo.toml` with `workspace = true`.
- Register a task handler: add it in `register_handlers()` (`src/lib.rs`) and `src/bin/worker.rs`.
- Run `cargo build -p askhava-server --locked`, `cargo clippy --workspace --all-targets -- -D warnings` and `bash scripts/smoke.sh`.

## Tests that guard it

- None yet. T175's architecture test (TD-SEC-10) checks the layer table and lints; T106 adds the health check.
