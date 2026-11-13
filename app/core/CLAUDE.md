# askhava-core: the shared kernel

## Purpose
The layer-0 crate every module may call: settings, the database pool and guarded updates, the clock, derived IDs, webhook dedupe, the `core.task` queue and rate limits (`docs/architecture.md` section 5, D-090). It is not a business module and owns no business data. T011 added settings, the clock and logging; T012 the pool, migrations and `Guarded<T>`; T013 the queue and rate limits;

## Never break
- `core` depends on no other first-party crate: it is layer 0 in `kit/layers.json`; T175's architecture test guards the layers (TD-SEC-10).
- No `unsafe`, no `#[allow]` without a reason, functions under 50 lines (workspace `Cargo.toml` and `clippy.toml`, D-585).
- Any future `src/status.rs` must start with `#![deny(clippy::wildcard_enum_match_arm, clippy::match_wildcard_for_single_variants)]` (SEC-031, D-488).
- Settings come in as pairs (`Settings::from_pairs`), never through `std::env::set_var` (D-481, D-491).
- No settings error echoes a value, only the key: `tests/settings.rs::invalid_values_are_refused_without_echoing_them`.
- No `sqlx::query` event reaches the JSON log and the metrics read no SQL, bind value or URL; the pool's maximum and acquire timeout stay as set: `tests/integration/tests/db_metrics.rs` (TD-OPS-41). Never put a global level filter back in `logging::init`: it would hide sqlx's DEBUG events from the metrics layer.
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
- A derived ID or token never changes for the same inputs, secret and version (replay depends on it): `src/tests/ids.rs`, `src/tests/prop_derived_ids.rs`. Never change a kind label or the field encoding in `ids.rs` without a recovery plan. A derived ID is pinned to the secret version it was made under; a rotation never re-derives a stored one (D-819).
- No `IdSecret`, `LinkSalt` or `LinkToken` value reaches `Debug`: `tests/ids.rs::weak_secrets_are_refused_and_nothing_secret_is_printed`.
- One event delivered 100 times has one effect: `tests/integration/tests/webhook_dedupe.rs::one_event_delivered_100_times_changes_nothing_after_the_first`.
- Every grant is a plain top-level statement that fails alone on a missing role; never inside a `DO` block or behind a `pg_roles` lookup (D-701).
- A new table in any schema gets SELECT, INSERT only for the app role; a table that needs UPDATE or DELETE gets an explicit `GRANT ... Guarded by `tests/integration/tests/schema_grants.rs`. More: `(never 21)` in NOTES.md.
- Customer-visible code never writes the brand: it reads `brand()` or a template token; `tests/docs/test_brand_literals.py` fails on any literal under `app/`; since T295 there is no baseline, and a `tests/brand_literal_baseline.json` file fails the guard (D-1933, D-2833).
- The brand file is a protected path: a rename is a harness change with its decision, and the legal drafts in `docs/legal/` must be re-issued under the new name (`scripts/check_legal_docs.py` fails until they are, TERMS-016). T428 (D-3347, D-3425 to D-3428) switched it to Laftly (Laftly LLC, `laftly.com`, `jobs@notify.laftly.com`, prefix LAFTLY), with `forbidden_alone` still `["Hava"]`; More: `(never 23)` in NOTES.md.

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/core/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/core/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
