# askhava-server: the composition root and both binaries

## Purpose
Builds the two processes from one crate: `server` (the `web` process, Axum) and `worker` (the task runner).

## Never break
- PICK-022, PRIV-005, D-1739: only `release_sink` mints a release proof, Guarded by `tests/release_recovery.rs`. More: `(never 1)` in NOTES.md.
- `rules_bridge` answers only through each crate's public `api`, reads nothing but stdin and the compiled-in files, Guarded by `tests/rules_bridge_contract.rs`. More: `(never 2)` in NOTES.md.
- SEC-016: the voice route answers nothing before `verify_webhook` passed, Guarded by `tests/inbound_voice.rs`. More: `(never 3)` in NOTES.md.
- SEC-016, SEC-017, PRIV-016: the dialed-status route (`call_status`, under its own refusal layer, D-3525) writes nothing before `verify_webhook` passed, Guarded by `tests/call_status.rs`. More: `(never 26)` in NOTES.md.
- SEC-016, SEC-039: no module sees a Twilio delivery before `verify_webhook` passed over `ASKHAVA_PUBLIC_BASE_URL` plus path and query; Guarded by `tests/integration/tests/inbound_text_route.rs`. More: `(never 4)` in NOTES.md.
- OPS-011, PRIV-029: every request hands one request-log line holding request ID, Guarded by `tests/integration/tests/offdb_stream.rs`. More: `(never 5)` in NOTES.md.
- SEC-016, SEC-048, D-3471, D-3473: on the four webhook routers a 403 means a source or signature refusal and nothing else, Guarded by `tests/security/tests/security_event_cap.rs`. More: `(never 7)` in NOTES.md.
- `install_crypto_provider()` runs first in both binaries (D-481, D-488).
- Both binaries exit 0 on SIGTERM; the web server drains in-flight requests first (D-491).
- Errors at start never echo a setting's value; both binaries call `start_up()` right after the TLS provider, More: `(never 10)` in NOTES.md.
- Routes are merged before the layers in `build_router()`, so every route gets the request ID and the route-template span (PRIV-016, PRIV-028).
- SEC-012, D-2633: `/static/` is mounted only as a nested service, never a one-line route or `*_PATH`/`*_ROUTE` constant; Guarded by `tests/static_files_route.rs`. More: `(never 11)` in NOTES.md.
- D-1870, D-2208, SEC-012: the layers, innermost first, are `catch_panic`, Guarded by `tests/request_timeout.rs`. More: `(never 12)` in NOTES.md.
- Any future `src/status.rs` must start with `#![deny(clippy::wildcard_enum_match_arm, clippy::match_wildcard_for_single_variants)]` (SEC-031, D-488).
- OPS-004, JOB-006, D-2285: a task kind the app queues is registered in `worker_registry` or listed with its owner in `tests/task_kinds_deferred.json`; Guarded by `tests/integration/tests/task_registry.rs`. More: `(never 14)` in NOTES.md.
- The worker gives back its leases after the drain on every signal (rust brain R14). More: `(never 22)` in NOTES.md.
- SEC-004, SEC-019, SEC-034, SEC-048, T300: every `business_portal` route reads `business_principal` first; Guarded by `tests/business_portal_route.rs`. More: `(never 17)` in NOTES.md.
- SEC-019, SEC-042, SEC-048, D-3641: the sign-up form `POST` admits its limit first, Guarded by `tests/business_signup_route.rs`. More: `(never 18)` in NOTES.md.
- SEC-019, SEC-048, D-3641, D-3779: each sign-up identity step acts only for the sign-up its verified `__Host-askhava-signup` cookie names, Guarded by `tests/business_signup_route.rs`. More: `(never 19)` in NOTES.md.
- SEC-014, BIZ-040, D-3806: sign-up email codes go only to the sign-up mailbox, Guarded by `tests/business_signup_route.rs`. More: `(never 20)` in NOTES.md.
- SEC-026, SEC-025, D-3669, D-3675: an action link's `GET` keeps nothing (its `use_action_link` rolls back), Guarded by `tests/action_links_route.rs`. More: `(never 21)` in NOTES.md.
- OPS-007: `/healthz` shows no secret or setting (`health.rs`).
- TD-SEC-41, D-1868, D-2269: `tests/route_wiring.rs` checks only the rows listed in `docs/architecture.md` section 13: a `yes` row must be served by `build_router` and a row naming a task must not be, More: `(never 23)` in NOTES.md.
- BIZ-025, FEE-014, D-3790 to D-3792: the portal home's "Your numbers" come only from `quote_numbers`, Guarded by `tests/integration/tests/business_numbers.rs`. More: `(never 24)` in NOTES.md.
- SEC-008, SEC-034, SEC-042, SEC-046, CON-035, D-4010: every homeowner portal route (`homeowner_portal`) is `no-store`; Guarded by `tests/homeowner_portal_route.rs`. More: `(never 25)` in NOTES.md.
- BIZ-037, SEC-047, D-4913: only `business_text_phone` reads a phone under `BusinessText`, Guarded by `src/business_text_phone.rs`. More: `(never 29)` in NOTES.md.
- PRIV-024, PRIV-016, D-5412: `server rotate-keys` stops each batch loop at its cap, Guarded by `tests/rotate_keys_command.rs`. More: `(never 30)` in NOTES.md.
- BIZ-033, SEC-038, SEC-019, D-3081, D-3889: `/b/phones` acts only for the owner after a fresh step-up and only on the `number_set_at` shown; Guarded by `tests/phone_approval_route.rs`. More: `(never 31)` in NOTES.md.
- JOB-016, SEC-006, D-4423: `/owner/problem-complaint` counts only for the owner session, after the cross-site and CSRF checks, records at most one complaint per job and nothing on a refusal; both routes `no-store`: `tests/owner_problem_route.rs` (TD-BIZ-59).

## Detail
Everything else about this crate (what it owns, the code behind each requirement ID, change recipes, the tests that guard it, decisions and history) is in `app/server/NOTES.md`, one entry per line, each starting with its requirement IDs. Grep it by requirement ID, file or word (`grep -n 'FEE-039' app/server/NOTES.md`); never read it whole. Add your own entries at its end, one line each (`docs/code-docs.md` section 2).
