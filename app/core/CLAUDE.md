# askhava-core: the shared kernel

## Purpose

The layer-0 crate every module may call: settings, the database pool and guarded updates, the clock, derived IDs, webhook dedupe, the `core.task` queue and rate limits (`docs/architecture.md` section 5, D-090). It is not a business module and owns no business data. T011 added settings, the clock and logging; T012 the pool, migrations and `Guarded<T>`; T013 the queue and rate limits; T014 derived IDs and webhook dedupe.

## What it owns

- `src/lib.rs`: the crate root; one `mod` line per file a task adds.
- `src/api.rs`: the only public surface; re-exports the items below.
- `src/settings.rs`: `Settings::from_pairs`, `Environment`, `Vendor`, `SettingsError`, `check_base_url` (D-688). A shared path: each task adds its own `ASKHAVA_*` key to `KEYS` and a typed field (D-493).
- `src/clock.rs`: `Clock`, `SystemClock`, `ManualClock`, `arizona()`, `arizona_time`, `zone`, `local_time`, `is_business_hours`, `add_business_minutes`.
- `src/logging.rs`: `Redacted<T>`, `init` (re-exported as `init_logging`), `json_layer`, `request_span`, `sentry_options`, `scrub_event`, `scrub_breadcrumb`, `scrub_text` (D-689).
- `src/db.rs`: `MIGRATOR` (every file in `migrations/`), `app_pool`, `migrate` (the `server migrate` step), `Guarded<T>`, `DbError` (D-698).
- `build.rs`: rebuilds the crate when `migrations/` changes, so `MIGRATOR` sees a new file (D-488). `sqlx.toml`: `'TIMESTAMPTZ'` → `jiff_sqlx::Timestamp` (uppercase key). This crate names `jiff-sqlx` with its `postgres` feature (T013); any other crate whose `query!` reads or writes a `timestamptz` column adds `jiff-sqlx = { workspace = true, features = ["postgres"] }` first, binds `ts.to_sqlx() as _` (`jiff_sqlx::ToSqlx`) and reads `.to_jiff()`.
- `src/tasks.rs`: `NewTask`, `enqueue`, `claim`, `finish`, `renew`, `fail`, `release_leases`, `reap_expired`, `backoff`, `WorkerId`, `Lease`, `Claimed`, `Outcome`, `Registry` (`register`, `run`), `Worker` (`run`: the claim loop the `worker` binary starts, D-710), `WORKER_CONCURRENCY`, `HandlerError`, `TaskError`, and the fixed `last_error` sentences `NO_HANDLER`, `TIMED_OUT`, `LEASE_EXPIRED` (D-708).
- `src/rate_limit.rs`: `ClientIpPolicy` (`from_pairs` over `ASKHAVA_CLIENT_IP_HEADER` and `ASKHAVA_PROXIED_HOSTS`, `client_ip`), `CLOUDFLARE_RANGES`, `hit` (re-exported as `rate_limit_hit`), `prune` (as `prune_rate_limits`), `ip_bucket`, `RateDecision`, `RateLimitError` (D-709).
- `src/test_control.rs`: `TestControl` (`from_settings`, `router`, `clock`), `TestControlError`, `TEST_CONTROL_COMPILED`. The routes (`/__test/clock`, `/__test/clock/advance`, `/__test/messages`) compile only under the `test-control` feature, which turns on the optional `reqwest` dependency (D-739, D-740).
- `src/ids.rs` (T014, D-815): `IdSecret` (versioned `ID_SECRET`, 32+ bytes, redacted `Debug`), `IdKind`, `derive_id`, `card_id`, `quote_id`, `pick_id`, `LinkKind`, `LinkSalt` (`generate` from the OS source, `from_bytes`), `LinkToken` (`parse`, `as_str`, `hash`; no `Display`), `link_token`, `IdError`.
- `src/webhooks.rs` (T014, D-815): `Dedupe` (`First`, `Repeat`, `from_returning`, `is_first`), `EventKey` (`new`, `pair`, `as_str`), `WebhookError`; the module docs hold the `webhook_event` table shape and the insert each module writes.
- Migration `0020_core_task_queue.sql`: `core.task` (partial due index, unique `dedupe_key`, 2% autovacuum) and `core.rate_limit` (bucket, window start, hits).
- Migration `0019_core_db_migrations.sql`: the `core` schema and the app role's grants. `scripts/bootstrap_roles.sql`: the two roles, their timeouts and grants (D-699).
- Tables (from T013 on): the `core` schema only, `core.task` and `core.rate_limit` (D-090, D-091).

## Rules it enforces

- SEC-001, SEC-002, SEC-003, SEC-036: `settings::Settings::from_pairs` (unknown key, missing value, live key outside live, test key in live, an account identifier off its allow-list or on the other's); `settings::check_base_url` (a stand-in URL in live, SEC-032).
- JOB-004: `db::Guarded` is the outcome of every "only if still in the expected status" change; each module writes its own `UPDATE … WHERE id = $1 AND status = $2 RETURNING …` and reads it with `Guarded::from_returning` (D-491).
- D-092: `db::migrate` runs only over `ASKHAVA_MIGRATION_DATABASE_URL`; `db::app_pool` over the app role's URL.
- JOB-007: `clock::is_business_hours`, `clock::add_business_minutes` (8:00 a.m. to 8:00 p.m. Arizona time, every day, D-004).
- PRIV-016, PRIV-028: `logging::json_layer` (one JSON line, `message` and `level` on top), `logging::request_span` (route template and request ID, never the URI), `logging::Redacted`.
- JOB-006: `tasks::claim` (`FOR UPDATE SKIP LOCKED`, `attempts < max_attempts`, lease null or expired, lease end computed in Rust from the injected clock); `finish`, `renew` and `fail` fenced by lease owner and claim attempt; `enqueue` refuses a duplicate dedupe key; handlers re-check status first.
- SEC-040, SEC-014: `rate_limit::hit` is one atomic upsert; `ClientIpPolicy::client_ip` reads only the edge header the setting names, and `CF-Connecting-IP` only on a proxied hostname from Cloudflare's ranges; never `X-Forwarded-For`.
- PRIV-017: `logging::sentry_options` (`send_default_pii` off) and `logging::scrub_event`.
- SEC-035: `test_control` (no routes and the system clock without the feature; with it, dev and CI only, a per-run bearer secret of 32+ characters, wrong secret answered 404, messages read from the Twilio stand-in's sent log, never `messaging` tables).
- CARD-017, PICK-028, SEC-037: `ids` (HMAC-SHA256 over the secret version, a kind label and length-prefixed record IDs; link tokens add a 16-byte OS-random salt and are stored only as SHA256).
- SEC-017, SEC-039: `webhooks::Dedupe::from_returning` over each module's own `INSERT … ON CONFLICT (source, event_key) DO NOTHING RETURNING event_key`; `EventKey::pair` for Twilio (MessageSid, MessageStatus) and (CallSid, step).
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
- The app role owns nothing and cannot `UPDATE`, `DELETE` or `TRUNCATE` an append-only table: `tests/integration/tests/migrations.rs::app_role_cannot_rewrite_an_append_only_table`.
- Every grant in a migration fails loudly on a missing role (never `IF EXISTS`), and a grant never sits in a `-- no-transaction` file: `migrations.rs::every_grant_reports_a_missing_role`.
- Migrations write `ALTER DEFAULT PRIVILEGES IN SCHEMA x` with no `FOR ROLE`; they and `#[sqlx::test]` run as the database's one migration identity (`askhava_ci` in smoke and `Ci`, `askhava_migrator` on Railway); test database names are a hash shared across the cluster, so never mix roles (D-491, D-492).
- `DbError` never carries a URL or the database's message, only a migration number (`Migration`, `Edited`, `Removed`, `Dirty`, D-701): `migrations.rs::server_migrate_names_the_migration_it_refuses`.
- A task payload holds IDs only and `last_error` is a `&'static str`; a rate-limit bucket is an IP or a blind-index hash, never a plaintext phone or email (PRIV-002, D-491).
- A queue claim commits before its handler runs, and no `finish`/`fail`/`renew` drops the owner-and-attempt fence: `tests/integration/tests/task_queue.rs::an_expired_lease_is_reclaimed_and_the_old_owner_is_fenced`.
- `test-control` is the one feature (T175's architecture test) and is enabled only on the command line, never in a manifest or the Dockerfile: `tests/security/tests/control_router_absent.rs::td_sec_15_no_manifest_turns_the_feature_on`, `image_pinned.rs`.
- A derived ID or token never changes for the same inputs, secret and version (replay depends on it): `src/tests/ids.rs`, `src/tests/prop_derived_ids.rs`. Never change a kind label or the field encoding in `ids.rs` without a recovery plan.
- No `IdSecret` or `LinkToken` value reaches `Debug`: `tests/ids.rs::weak_secrets_are_refused_and_nothing_secret_is_printed`.
- One event delivered 100 times has one effect: `tests/integration/tests/webhook_dedupe.rs::one_event_delivered_100_times_changes_nothing_after_the_first`.
- Every grant is a plain top-level statement that fails alone on a missing role; never inside a `DO` block or behind a `pg_roles` lookup (D-701).

## How to change

- Add a kernel file: create `src/<name>.rs` with a `//!` header, add one `mod` line to `src/lib.rs`, re-export its public items through `src/api.rs`, and update this file and `docs/architecture.md` section 5.
- Add a dependency: it must be in `[workspace.dependencies]` and `kit/allowed_crates.txt`, named here with `workspace = true`; a new crate needs a `docs/decisions.md` entry (BUILD_BRIEF.md:122).
- Add a setting: add the `ASKHAVA_*` name to `KEYS` in `src/settings.rs`, a private field with a `///` getter naming its requirement, parse it in `from_pairs` (errors carry the key only), add the row to `docs/deploy.md` when it exists, and a case in `src/tests/settings.rs`. A vendor base URL goes through `check_base_url`.
- Add a migration: `migrations/0NNN_<slug>.sql`, line 1 `-- <module> (T###)` (line 2 if `-- no-transaction` must be line 1); new tables in a schema get the default grants, an append-only table adds `REVOKE UPDATE, DELETE, TRUNCATE ON … FROM askhava_app`; then `sqlx migrate run`, `cargo sqlx prepare --workspace -- --all-targets --all-features` and `cargo test -p askhava-integration --test migrations`. An applied migration is never edited or renumbered (D-491).
- Change a role timeout: `scripts/bootstrap_roles.sql` and D-699 first; rerun the script on each cluster.
- Add a task kind: write the handler in its module (re-check status first, fixed error sentences), register it in `register_handlers()` in `app/server/src/lib.rs`, enqueue with `enqueue(&mut tx, …)` in the state change's transaction and a derived `dedupe_key`; then `cargo test -p askhava-integration --test task_queue`.
- Change backoff, lease or worker limits: `BACKOFF_FIRST`/`BACKOFF_MAX` and `Lease::WORKER`, `WORKER_CONCURRENCY` and `POLL_MIN`/`POLL_MAX` in `src/tasks.rs`, `DRAIN` in `app/server/src/bin/worker.rs`, with a decision entry.
- Add a test-control route: inside `mod enabled` in `src/test_control.rs` only (above the `/__test/{*rest}` catch-all, behind the bearer `layer`, input structs `deny_unknown_fields`), a case in `control_router_absent.rs`, a method in `tests/e2e/fixtures.ts`; run that test with and without `--features askhava-core/test-control`.
- Change business hours: `BUSINESS_OPEN_HOUR` and `BUSINESS_CLOSE_HOUR` in `src/clock.rs` (JOB-007 and D-004 first), then `cargo test -p askhava-core tests::clock`.
- Run `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check` and `bash scripts/smoke.sh`.

## Tests that guard it

- `src/tests/settings.rs` (TD-SEC-15, TD-SEC-13), `src/tests/clock.rs` (JOB-007, with property tests), `src/tests/logging.rs` (TD-PRIV-13). `tests/integration/tests/migrations.rs` (JOB-004, D-092: migration order, missing-role reporting, append-only as the app role, stale status and row-lock wait). `tests/integration/tests/task_queue.rs` (JOB-006, TD-SEC-19: exactly once for 100 and 1000 tasks over two workers, lease reclaim and fencing, dedupe, backoff and dead-letter, the real `Worker::run` loop running every task once and giving its leases back on shutdown, 5 of 20 concurrent hits, the client IP). `tests/security/tests/control_router_absent.rs` (TD-SEC-15, SEC-035; its feature-on cases run with `--features askhava-core/test-control`). `src/tests/ids.rs` and `src/tests/prop_derived_ids.rs` (TD-CARD-06, TD-PICK-12, TD-SEC-16, TD-SEC-18 key cases) and `tests/integration/tests/webhook_dedupe.rs` (SEC-017, TD-SEC-18 Twilio pairs, TD-SEC-16 re-issue, revoke and replay); T175's architecture test (TD-SEC-10) checks the layer rule and `[lints] workspace = true`.
