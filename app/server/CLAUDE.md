# askhava-server: the composition root and both binaries

## Purpose

Builds the two processes from one crate: `server` (the `web` process, Axum) and `worker` (the task runner). It is the only crate that may depend on every module; it wires their routes and task handlers together and owns process concerns (TLS provider, port, signals) (`docs/architecture.md` section 2, D-481).

## What it owns

- `src/lib.rs`: `install_crypto_provider()`, `start_up()` (settings from `std::env::vars()`, then the JSON log and Sentry; D-689), `build_router(pool, test_control)` (merges `core`'s `TestControl::router()`, empty in the release build, SEC-035, D-739; no module routes yet; the request-log middleware, under the request-ID and `TraceLayer` layers that log the route template only), `shutdown_signal()` (SIGINT or SIGTERM), `register_handlers(stream, texts)` (the `core::api::Registry` of every task handler; `texts` registers `intake`'s `jobs.homeowner_text` sender), `job_text_keys(settings, id_secret, pairs)` (the sender's keys from `ASKHAVA_ID_SECRET`, `VAULT_KEY` and `VAULT_INDEX_KEY`; `None` in dev and CI without vault keys, an error in test and live; D-1302).
- `src/main.rs`: the `server` binary; binds `0.0.0.0:$PORT` and serves with graceful shutdown. `server migrate` instead applies pending migrations over `ASKHAVA_MIGRATION_DATABASE_URL` (via `askhava_core::api::migrate`) and exits (D-700, SEC-052). Missing or bad `PORT` exits 1 without echoing the value (D-588).
- `src/bin/worker.rs`: the `worker` binary; it runs `core`'s `Worker::run` on `TestControl::clock()` (the system clock in release, D-740; the off-database stream uses the same clock), the claim loop over `core.task` (4 at once, idle poll 200 ms to 5 s, the reaper when idle), with the shutdown signal and a 20-second drain, after which the loop calls `release_leases` (JOB-006, D-708, D-710).
- `src/offdb_stream.rs` (T017, D-734, D-735): `OffDbStream` (storage, `ID_SECRET`, clock, stand-in purge directory; `from_settings` reads `ASKHAVA_STORAGE_BASE_URL` and `ASKHAVA_ID_SECRET`), `register` (handlers for `ops.offdb_stream` and `ops.offdb_verify`), `verify_audit_chain`, `purge_request_log`, `schedule_verify`, `audit_name`, `RequestLog` (the bounded channel and background writer of request lines, D-737) and the `log_request` middleware that `build_router` puts on every route.
- No tables.

## Rules it enforces

- SEC-029: workspace lints on (`[lints] workspace = true`), `unsafe_code` forbidden, built by `cargo build -p askhava-server --locked` in `scripts/smoke.sh` and `Ci` (D-488).

## Never break

- OPS-011, PRIV-029: every request hands one request-log line holding request ID, route template, status and time, never the path, to `RequestLog` without waiting (overflow dropped and counted); audit row `n` is stored as `audit/<n>.json` with its line and the ID and line hash of the row before it, and waits (retries) up to 2 minutes for a missing lower ID; the nightly `ops.offdb_verify` first queues tomorrow's run, then purges stand-in request days older than 30 and last raises a High `ops.audit_chain_broken` alert on any mismatch; the worker also queues today's and tomorrow's at start (`tests/integration/tests/offdb_stream.rs`).

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

- `tests/integration/tests/offdb_stream.rs` (TD-OPS-03, TD-OPS-19): the stream writer, the chain check and the purge.
- `tests/integration/tests/task_queue.rs` runs `core`'s `Registry::run` and `Worker::run`, the loop this binary starts. T175's architecture test (TD-SEC-10) checks the layer table and lints; T106 adds the health check.
