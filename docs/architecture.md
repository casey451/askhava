# Ask Hava architecture

Written in kit step K4 (`BUILD_BRIEF.md:598`). This file finalizes the module boundaries that `BUILD_BRIEF.md:124`–`BUILD_BRIEF.md:152` fixes, and pins the stack versions (`BUILD_BRIEF.md:121`). Since D-453 the server is Rust (`BUILD_BRIEF.md:105`); sections 2, 4, 5, 7 and 11 describe the Rust layout. It restates and arranges rules; where it had to choose, the choice is logged in `docs/decisions.md` (D-090 to D-100) and named here. If this file and a cited source disagree, the source wins and this file is the bug. Every PR that changes a boundary, table owner, state, timeout or pinned version updates this file in the same PR (`BUILD_BRIEF.md:489`).

Citations are `path:LINE`. "Changes file" means `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md`; "v9.3" means `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md`. Requirement IDs (for example `PRIV-005`) are in `docs/spec/`.

**Parts (D-6022).** This file is the index. Sections 1 to 6 and 13 stay here: section 2's processes table and code layout, the locked sections 3 to 6 (`scripts/check_plan_lock.py`) and section 13's route table, which the route tests and the runner read. The rest moved verbatim into `docs/architecture/`: section 2's feature notes, one file per first-named crate (`docs/architecture/02-adapters.md`, `docs/architecture/02-ai.md`, `docs/architecture/02-businesses.md`, `docs/architecture/02-calls.md`, `docs/architecture/02-core.md`, `docs/architecture/02-intake.md`, `docs/architecture/02-jobs.md`, `docs/architecture/02-messaging.md`, `docs/architecture/02-ops.md`, `docs/architecture/02-payments.md`, `docs/architecture/02-picks.md`, `docs/architecture/02-portals.md`, `docs/architecture/02-quotes.md`, `docs/architecture/02-server.md`, `docs/architecture/02-site.md`); section 7 (`docs/architecture/07-adapters.md`); section 8 (`docs/architecture/08-contact-vault.md`); section 9 (`docs/architecture/09-environments.md`); section 10 (`docs/architecture/10-off-database-records.md`); section 11 (`docs/architecture/11-pinned-versions.md`); section 12's rules (`docs/architecture/12-keeping-this-file-true.md`); and the build machinery: section 7's runner paragraph with section 12's runner bullets (`docs/architecture/harness-runner.md`) and section 12's CI, smoke and plan-check bullets (`docs/architecture/harness-checks.md`). Read the parts your task lists. "Update `docs/architecture.md`" means this index plus the part a fact belongs in; until T885 makes `docs/architecture/` a shared path, put a new note in this index at the end of the section it belongs to, and the next plan change moves it into its part.

---

## 1. Shape at a glance

One web app and one PostgreSQL database do all the work; every outside service sits behind an adapter (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:44`; `BUILD_BRIEF.md:146`).

```
 Homeowner (text, web chat,      Business (email, portal  Casey (owner portal,
 quote page, homeowner portal)   links; no calls; text    alert emails, HELP
                                 only if chosen)          notifications, calls)
            \                         |                        /
             +------------ web process (Rust: Axum + Askama) -+
                                      |
          worker process (Rust: scheduler + task runner, same crates)
                                      |
                PostgreSQL 16: one schema per module, contact vault encrypted
                                      |
                         adapters (real | sandbox | stand-in)
   Twilio texts (homeowner number; a business number only if approved, TXT-035)
   Twilio calls | Twilio Verify | Stripe | Claude API (zero retention)
   Resend email | Lob letters | license sources | object storage
   Kept outside the database: Twilio and Stripe records, web request log,
   audit stream, AI structured readings (OPS-011)
```

## 2. Processes and code layout

| Process | Runs | Notes |
|---|---|---|
| `web` | The `server` binary (Axum on Tokio): pages, portals, web chat, webhooks, `/healthz` | Stateless; any number of replicas; server-rendered Askama pages that work without JS (`BUILD_BRIEF.md:106`) |
| `worker` | The `worker` binary: the scheduler and the task runner, polling `core.task` with `SELECT … FOR UPDATE SKIP LOCKED` | Timers, outbox sends, retries, nightly jobs (`BUILD_BRIEF.md:108`; JOB-006). Same image, started with a different command |

Both binaries are built from one Cargo workspace into one Docker image (`BUILD_BRIEF.md:119`), pinned by digest (SEC-021).

```
Cargo.toml           the workspace: members "app/*" and the Rust test-layer crates; every approved crate and its exact
                     version under [workspace.dependencies]; lints under [workspace.lints] (unsafe_code forbid; clippy
                     too_many_lines and excessive_nesting denied with their thresholds in clippy.toml; missing_docs;
                     every member sets [lints] workspace = true, which the architecture test checks, D-481)
Cargo.lock           committed
Dockerfile           multi-stage: cargo-chef planner, a builder that cooks the dependencies then builds both binaries
                     (`--release --locked`), and a `debian:trixie-slim` runtime pinned by digest with `ca-certificates`
                     and `libssl3t64` (Debian 13's name), running as user 10001; `RUSTUP_TOOLCHAIN` is set in the
                     builder so rustup does not fetch clippy and rustfmt; no `# syntax=` line; `Ci` logs in to Docker
                     Hub or uses a mirror because anonymous pulls are rate-limited (T019, D-491)
.sqlx/               offline query data from `cargo sqlx prepare --workspace -- --all-targets --all-features` (and `--check` placed before the `--`),
                     committed; every build and `Ci` run with SQLX_OFFLINE=true, and `Ci` fails on a stale entry
                     through `cargo sqlx prepare --check` after `sqlx migrate run` (D-481)
rust-toolchain.toml  the pinned Rust release (section 11)
clippy.toml          too-many-lines-threshold = 50, excessive-nesting-threshold = 4 (an unset or 0 threshold turns the
                     lint off, D-488, D-585)
deny.toml            cargo-deny policy: license allow-list, crates.io only, banned crates, yanked and unknown
                     sources denied, every ignore with a reason and expiry (SEC-021, SEC-045)
app/
  server/            composition root (a crate that depends on every module)
    src/lib.rs       `build_router()` and `register_handlers()`: the one composition both binaries call, and the
                     place that creates `privacy`'s `ReleaseMint` and hands it to `payments` (section 4, rule 4);
                     the shared composition file (D-481)
    src/main.rs      the `web` binary: installs the rustls crypto provider, builds the router, binds
                     `0.0.0.0:$PORT`, serves with graceful shutdown on SIGINT and SIGTERM (a container stop sends
                     SIGTERM; without the handler the exit is 137, D-491); `server migrate` runs `sqlx::migrate!` as
                     the release step (section 9); a missing or bad `PORT` exits 1 without echoing the value, and
                     `/healthz` is left to T106 (D-588)
    src/bin/worker.rs        the worker binary: same provider install, one Tokio runtime; it runs `core`'s
                             `Worker::run` (the claim loop, section 5: 4 tasks at once in a `JoinSet`, polling
                             200 ms to 5 s while idle) over `register_handlers()`; on SIGINT or SIGTERM it stops
                             claiming, drains for 20 s, gives back its leases and exits 0 (T013, D-708, D-710)
    src/bin/rules_bridge.rs  JSON-in, JSON-out door to each crate's `api::rules` for the twin (T074): one
                     long-lived process, one JSON object per line each way, requests `{id, call, args}`, answers
                     `{"ok": {id, result}}` or `{"error": {id, code}}` (about 0.05 ms a call measured, D-491); calls
                     `fee` (payments), `quiet_hours` (messaging), `transition` (jobs), `card` and `job_types`
                     (quotes over the license map and catalog) (D-1592); plain binary, no feature, linted with the
                     rest; its input and output follow a hand-written JSON schema
                     (`app/server/rules_bridge.schema.json`) that a contract test checks on the Python side
                     (`tests/contract/test_rules_bridge.py`) and the Rust side
                     (`app/server/tests/rules_bridge_contract.rs`), both with a small subset validator, since no
                     JSON-schema crate is allowed and Ci has no jsonschema package (D-481, D-1592); building it
                     compiles every crate's query macros, so `twin.yml` sets `SQLX_OFFLINE=true`
  core/              shared kernel (section 5); not a business module; `build.rs` is `sqlx migrate build-script --source ../../migrations` with a `//!` line (D-488)
                     so a new migration file is seen without a clean build (T012); `src/db.rs` holds `MIGRATOR`, `app_pool`,
                     `migrate` (run by `server migrate` over `ASKHAVA_MIGRATION_DATABASE_URL`) and `Guarded<T>` (D-698, D-700);
                     `sqlx.toml` maps `'TIMESTAMPTZ'` to `jiff_sqlx::Timestamp` (D-488)
                     `src/test_control.rs` is the test-control router (`/__test/clock`, `/__test/clock/advance`,
                     `/__test/messages`, proxied to the Twilio stand-in's sent log) and the clock both binaries run on;
                     the routes compile only under core's `test-control` feature, the one Cargo feature allowed, which
                     no manifest and no Docker build enables (e2e builds pass `--features askhava-core/test-control`),
                     and need a per-run bearer secret on every `/__test/` request (SEC-035, D-739, D-740, D-742)
  <module>/          one crate per module in section 3, package name `askhava-<module>` (lib `askhava_<module>`), so
                     no first-party crate shadows Rust's built-in `core` or another standard name (D-484)
    Cargo.toml       depends only on the lower crates section 4 allows (the compiler refuses anything else)
    src/lib.rs       hand-written: `pub mod api;` plus private `mod models; mod rules; mod service; mod handlers;
                     mod routes;` and `#[cfg(test)] mod tests;` (no automod, D-481)
    src/api.rs       the module's public functions and types; `pub mod rules` inside it re-exports the pure
                     decision functions (no I/O, no tables), which is how the rules_bridge and the property tests
                     of other crates reach them without a feature; everything else in the crate is private
    src/models.rs    row types and SQLx queries, all in the module's own Postgres schema
    src/rules.rs     pure decision logic (no I/O), the main target of unit and property tests
    src/status.rs    the crate's status enums and every `match` on them, with clippy
                     `wildcard_enum_match_arm` and `match_wildcard_for_single_variants` denied on this module only (SEC-031, D-488); nothing outside it compares a
                     status with `==` or glob-imports one (`scripts/check_rust_rules.py`; D-590 lists the shapes it catches)
    src/service.rs   orchestration: transactions, calls to lower crates' api, audit rows; errors are a
                     `thiserror` enum per crate (no anyhow), mapped once in `server` to responses, never carrying a
                     `Redacted` value in `Display`
    src/handlers.rs  task handlers registered in server/src/lib.rs (timers and events)
    src/routes.rs    HTTP routes, only in crates that own a web surface (portals, site, webhooks)
    src/tests/       the crate's unit and property tests, one file per task, each declared by one `mod` line in
                     src/tests/mod.rs (a shared path); they see the crate's private items under `#[cfg(test)]`
                     and need no feature (D-481)
    templates/       Askama templates (checked when the crate compiles), served through askama_web; all
                     customer-facing words live in templates and message files, never in `.rs` string literals,
                     so the AI-smell scan sees them. A scan bans `|safe`, `escape = "none"` and a user value in an
                     href, src, style or script context. The `.vcf` contact card is not Askama: one function
                     escapes each field per RFC 6350 (backslash, comma, semicolon, newline) before it is written.
                     The shared layout of `docs/design/DESIGN_SPEC.md` lives in `app/site/templates/` and the
                     other crates reach it through `askama.toml` `dirs` (D-481)
    static/          CSS, fonts, images and `.ts` sources; `npm run build` (tsc) writes the `.js` beside each
                     source in the Docker build stage and before local and CI e2e runs (T173); the generated
                     `.js` is not committed (`npm run build` is `tsc --ignoreConfig --strict` over
                     `app/*/static/*.ts`); the image keeps only the `.js`, at `/srv/askhava/static/<crate>/`,
                     the `/static/<crate>/` paths templates load (D-1281); `server`'s `static_files.rs` serves
                     them with tower-http `ServeDir` under the security headers, from `ASKHAVA_STATIC_DIR`
                     (else the image's folder in test and live, `app/<crate>/static/` in dev and CI), a year's
                     cache only for content-hashed names (T362, D-2633, D-3241)
migrations/          SQL files `0NNN_<slug>.sql`, NNN the owning task's serial position in the runner's order (D-485; gaps are
                     allowed, numbers only rise, and a number is frozen once applied to the test project, D-491), each
                     beginning `-- <module> (T###)`, `<module>` being the schema the migration touches (D-492) (line 2 when
                     `-- no-transaction` must be line 1), forward-only, each
                     tested; SQLx applies
                     any not yet applied in number order, including a lower number added later (checked, D-453).
                     On a fresh database the files run in number order, not task order; the architecture test
                     (`tests/integration/tests/architecture_migrations.rs`, T226) fails if a migration's number is
                     higher than that of a migration whose task depends on its own, whatever schemas they touch
                     (two tasks that may run in either order never fail), and each migration file names its
                     owning module in a leading comment (D-481, D-600); it also fails on dynamic SQL, which hides
                     a schema name from it: a `DO` statement, an `EXECUTE` that runs a string or a `format`/`quote_*`
                     call in a migration, or `DO`/`EXECUTE` in a query macro's SQL under `app/`, unless the file is
                     in the test's `DYNAMIC_SQL_ALLOWED` list with a reason (only 0019's role guard; a stale entry
                     fails too) (TD-SEC-36, T252, D-1677)
stand_ins/           Python fakes of Twilio, Stripe, Resend, Lob and the AI, served over HTTP by `stand_ins/server.py`
                     at the vendors' URL shapes, so the Rust adapters use the same client code everywhere (SEC-032)
tests/               the Rust test-layer crates integration, security and recovery (package `askhava-recovery-tests`, since the
                     recovery module already holds the name, D-488) (T168, and integration in
                     T175), files in tests/<layer>/tests/, each reaching the app crates only through their `api`;
                     compile-fail cases with their expected `.stderr` in tests/security/ui/, built by trybuild in a
                     crate with no feature on, so a proof type's privacy is tested as production sees it;
                     `tests/fixtures` (T010) is the fake-data crate; Playwright in TypeScript for e2e and a11y;
                     `tests/e2e/fixtures.ts` gives every spec the `control` fixture over the test-control router,
                     `tests/e2e/webserver.ts` (the config's `globalSetup`, D-958) builds and starts the stand-ins,
                     worker and web binary with `test-control` and stops them after the run (since T536 the
                     worker shares the keyed binary's environment and all three share one ID secret, and the
                     clock-advancing journeys run last in each config's `late` project, D-4649);
                     `tests/a11y/` (T053, D-1238 to D-1241) reuses that `globalSetup`, reads its page list from
                     the GET routes in app/*/src (a route with no seeded state fails), takes each route's states
                     and seed function from the shared data file `tests/a11y/page_states.ts` (T267, D-1788,
                     D-2106; the checks stay in `fixtures.ts`), seeds each state's fake
                     data through psql on DATABASE_URL and runs axe and the structure, target, contrast, reflow,
                     spacing and motion checks at 320, 768 and 1280 px (CSP bypassed only for text spacing,
                     D-1241; JSON report in test-results/a11y-report); `Ci`'s `E2e` job runs tests/e2e and
                     `Ally` runs tests/a11y, each with Node 22, `npm run build`, cached Chromium, a test-control
                     build and a Postgres service (stripe-mock too in `E2e`), T173, D-1282; and
                     `package.json`, `tsconfig.json` and `eslint.config.js` at the root pin the TypeScript tooling (D-741);
                     pytest for docs and contract. Every database test uses `#[sqlx::test(migrator = ...)]`, one
                     fresh database per test (D-481); `tests/mutation/mutants.toml` holds the named mutants
                     (exact text replacements and the tests that must fail), applied in place by
                     `scripts/run_named_mutants.py`: `Ci`'s `Checks` runs those whose rule or test files a branch changed
                     (a stale `find` is reported there, failed only at night),
                     nightly-main's `named-mutants` job all of them, and its `cargo-mutants` job the listed rule
                     files, report only for now (T056, D-1285, D-1286)
twin/                the twin (Python) and its hard-failing gate, calling the Rust rules through rules_bridge
wargames/            the Phase 2 war games (Python, standard library): personas, scenario runner, chaos injectors and the
                     money reconciliation, driving the Railway test site's real entry points (T234, T235, D-1154)
```

Section 2's feature notes are in `docs/architecture/02-adapters.md`, `docs/architecture/02-ai.md`, `docs/architecture/02-businesses.md`, `docs/architecture/02-calls.md`, `docs/architecture/02-core.md`, `docs/architecture/02-intake.md`, `docs/architecture/02-jobs.md`, `docs/architecture/02-messaging.md`, `docs/architecture/02-ops.md`, `docs/architecture/02-payments.md`, `docs/architecture/02-picks.md`, `docs/architecture/02-portals.md`, `docs/architecture/02-quotes.md`, `docs/architecture/02-server.md`, `docs/architecture/02-site.md` (D-6022). New notes go here, below this line, until T885.


## 3. Modules, what each owns, and its tables

Each module owns its files and its tables, and no module reaches into another's tables (`BUILD_BRIEF.md:126`). Each module's tables live in a Postgres schema named after it (D-091). "Calls" lists the lower crates whose `api` it may use, which are exactly the path dependencies in its `Cargo.toml`; `core` is always allowed and is not repeated.

| Module | Owns (`BUILD_BRIEF.md:128`–`BUILD_BRIEF.md:146`) | Tables (schema-qualified) | Calls |
|---|---|---|---|
| `ops` | Alerts, pause switches, safe mode, status banner, web-and-email mode, audit log (decisions and records only: every page that shows `ops` figures is a `portals` page, every email or call `ops` wants sent is a `messaging` handler on a `core.task` event, D-491); which alerts push to Casey's phone, the 9 a.m. and 3 p.m. daytime batch push (OPS-032) and the weekly digest of everything else; safe mode only from pushed alerts; the safety-voice-call setting, off until a TCPA attorney's written approval is recorded (CALL-019); web-and-email mode able to run the whole service (TXT-034); the business-texting switch and the filed written answer it needs (TXT-035); alerts to Casey by email and, for critical safety alerts, a call (OPS-035); help requests: HELP auto-answers (CON-034), one HELP notification per request to the alert address with hourly and daily limits, the owner help page and its screened reply box, and the business "Get help" form (OPS-036, BIZ-047, D-348); the sending-limit alert (OPS-004, D-351). `ops` is layer 1, so it never sends or reads anything itself: it records alerts and help requests as rows holding IDs only and enqueues `core.task` events, and the handlers of `messaging` (alert emails, HELP notifications, Casey's critical call through the voice adapter) and `portals` (the owner help page, the scorecard page reading `payments::api` and `intake::api`) do the sending and reading; a homeowner's help-message text lives in `privacy.message_body` and `ops.help_message` holds its reference (PRIV-001, D-482) (D-218, D-219, D-275, D-277, D-278, D-297, D-303, D-314) | `ops.audit_log` (append-only), `ops.alert`, `ops.alert_ack`, `ops.push_item` (OPS-032's non-alert push items, T204, D-4044), `ops.pause_switch`, `ops.switch_history`, `ops.safe_mode`, `ops.status_banner` (built by T279, D-1843), `ops.help_request`, `ops.help_message` (IDs and a `privacy.message_body` reference, never text), `ops.not_a_fit_answer`, `ops.minutes_entry` (T158, D-493) | — |
| `adapters` | One interface per vendor; real, sandbox and stand-in implementations chosen by configuration | none | — |
| `catalog` | Categories, job types, license rules, fee rules, feature switches (fee mode, category on/off) and numeric settings | `catalog.category`, `catalog.job_type`, `catalog.license_rule`, `catalog.fee_rule`, `catalog.setting`, `catalog.setting_history`; markets as reviewed data and job locations by ZIP centroid (INT-048, T248, D-1518): `catalog.market`, `catalog.market_town`, `catalog.zcta_centroid` | `ops` |
| `privacy` | The plain-code scrubber; the contact vault (field-level encrypted); the inbound reader that is the only code to see raw homeowner text (section 8); retention and purge jobs; photos | `privacy.contact`, `privacy.contact_index` (blind index), `privacy.message_body` (scrubbed homeowner message text, and scrubbed help-request text from a business text or a help email, T151, D-4044; owner-only read, cut at 30 days), `privacy.photo` (each photo's job, key version and storage key only; the encrypted photo bytes are in object storage behind the `storage` adapter, never in PostgreSQL, PRIV-023), `privacy.retention_hold`, `privacy.purge_run`; `privacy.business_mailbox`, `privacy.business_mailbox_index` and `privacy.staff_phone` (each business member's sign-in address by mailbox ID and texting phone by line ID, sealed, with the address's blind index; not homeowner contact fields, T412, D-3262) | `adapters`, `ops` |
| `ai` | Request reading through the AI adapter, and quote summarizing only while the owner setting "AI quote summaries" is on (off by default and at launch, PICK-003, D-373); strict schemas; accepts only `ScrubbedText`, homeowner and business quote text alike (AI-002) | `ai.reading` (fields only, no personal details), `ai.call_stat` | `adapters`, `privacy` (the `ScrubbedText` type only), `catalog` (the open trades for AI-005 and the CARD-010 restricted-word list, D-492), `ops` |
| `messaging` | Outbox and inbox (never send twice); templates in English and Spanish; quiet hours; opt-out keywords and the one suppression list fed by every channel (text, help email, email replies, help form, portal switch, owner-recorded requests) and checked before every text and automated call (CON-035, D-372); the sender directory (which numbers are business phones); delivery tracking; email and letters; the business-notice fan-out by each business's chosen channels (email by default; text or both only while business texting is on), every channel of one notice sent or held together, and email bounce and complaint handling (BIZ-019, BIZ-040, QH-005, D-307, D-308); one fixed sender name and address for business notices (SEC-025); the open-cards email at up to two times a day, with a waiting pick first (CARD-022, D-380; its lines come from `quotes::api` and `picks::api` through the `core.task` event that `quotes` enqueues, since `messaging` is below them, D-482), and the monthly missed-card account email (CARD-022, BIZ-044, D-330, D-331); the email footer with the LLC postal address and "Manage email notices" (SEC-028); the email-limit deferral of non-urgent emails above 90% of the daily limit, nothing dropped (OPS-038, D-381); no homeowner details in any business text (TXT-036). Business notices arrive with their recipients resolved: the enqueuing module (`quotes`, `picks`, `businesses`) looks them up through `businesses::api::notice_addresses` and puts opaque address IDs in the event payload, and `messaging` sends to what it is given, never calling `businesses` (D-491) | `messaging.outbox`, `messaging.inbox` (scrubbed homeowner text and flags; business text as sent), `messaging.sender_directory` (blind-indexed business and staff phones), `messaging.delivery_event`, `messaging.opt_out`, `messaging.opt_out_event`, `messaging.email_outbox`, `messaging.email_event`, `messaging.letter`, `messaging.mode_state` (texting switch, number suspension and launch channel, append-only, T279, D-1843), `messaging.inbox_contact` (each inbound text's vault contact, written once by `on_inbound_text`, the CON-022 five-year clock's inbound half, T321, D-3155), `messaging.webhook_event` | `adapters`, `privacy`, `catalog`, `ops` |
| `payments` | Stripe customers, optional saved cards, card authorization, on-session Confirm-and-pay charges started only by the business's press on the Confirm page (FEE-014, FEE-036, D-365), the one off-session exception being the optional auto-confirm charge on a quote the business ticked, offered only while Stripe's written answer is filed (PICK-048, D-401), the money view's set-aside and manual expenses outside the ledger (FEE-046, D-410), refunds, disputes, the per-charge evidence record (FEE-042), free first wins, the ledger, nightly reconcile | `payments.customer`, `payments.payment_method`, `payments.card_authorization`, `payments.charge_attempt`, `payments.charge_evidence` (append-only), `payments.charge_three_ds` (each charged pick's 3-D Secure result as a boolean only, unknown read as false, T405, D-3118), `payments.ledger_entry` (append-only), `payments.refund`, `payments.dispute`, `payments.free_win`, `payments.webhook_event`, `payments.reconcile_run`, `payments.expense` (Casey's manual expenses list, FEE-046, never in the ledger; D-491), `payments.fee_terms` (the one row of fee terms in force, written by `switch_fee_mode`, T287, D-1871), `payments.pick_fee` (the fee fixed at each pick, written once by `picks` through `payments::api` in the pick's transaction, FEE-005, T287, D-1871), `payments.reconcile_open` (charges the nightly reconcile could not settle yet, retried until settled, T288, D-1872) | `adapters`, `privacy`, `catalog`, `ops` |
| `licensing` | Agency daily lists; live lookup at pick; other agencies; the city license list; handyman rules | `licensing.list_import`, `licensing.agency_record`, `licensing.city_license_entry`, `licensing.business_license`, `licensing.license_check` | `adapters`, `catalog`, `ops` |
| `businesses` | Sign-up, identity chain, job-type checklist, towns, tags, business consent records, pause rules (including PAUSE and BUSY, D-198), staff phones, team members and their roles (D-191), complaints and business replies (D-200), the channel preference and verified notice emails (BIZ-036, BIZ-040, SEC-024, D-307, D-312); the sign-up deliverability check with a sample card, backup notice addresses, the "Emergency emails at any hour" switch with its test emergency email, and the "reply OK, star, add to contacts" step; no automated-call consent is stored since no business is ever called (BIZ-037, BIZ-039, BIZ-042, BIZ-043, CON-019, D-328, D-329, D-344, D-346, D-352) | `businesses.business`, `businesses.town`, `businesses.service_area` (base ZIP and radius, BIZ-058, T248, D-1518), `businesses.job_type_choice`, `businesses.tag`, `businesses.staff_phone`, `businesses.team_member`, `businesses.consent_record`, `businesses.identity_attempt`, `businesses.signup_review`, `businesses.pause_event`, `businesses.complaint`, `businesses.channel_preference`, `businesses.email_verification` | `licensing`, `payments`, `messaging`, `catalog`, `adapters`, `ops` |
| `jobs` | The job lifecycle state machine (section 6); job numbers; status history; quote-page token hashes; private feedback and its share tick and removal reasons (JOB-017, T230, D-493) | `jobs.job`, `jobs.status_history`, `jobs.job_number`, `jobs.feedback`, `jobs.job_facts` (append-only narrowing rows of each job's category and candidate job type codes, T339, D-2330, D-3112) | `messaging`, `catalog`, `ops` |
| `calls` | Masked numbers from a pool, PIN routing, call sessions, call logs (never recorded), built on standard programmable voice, never Twilio Proxy (CALL-021, D-296) | `calls.number_pool`, `calls.call_session`, `calls.call_log`, `calls.call_start` (each call's start by parent CallSid, T262, D-1836), `calls.webhook_event`, `calls.safety_switch` (the owner-portal safety voice call switch with Casey's attorney approval, append-only, T407, D-3134) | `businesses`, `privacy`, `messaging`, `adapters`, `ops` |
| `quotes` | Who receives each job card, and the cards themselves; quote intake and parsing, including the required quote facts, pre-filled per job type from the business's last answers (D-233, D-283); eligibility rules; arrival order; original wording beside any summary | `quotes.job_card`, `quotes.quote`, `quotes.quote_bounce`, `quotes.quote_facts` (each quote's PICK-040 facts, T406, D-3134), `quotes.job_description` and `quotes.shown_card` (a job's scrubbed describing words and each card shown or released, with its digest, category and candidates, kept 3 years under PRIV-008, T411, D-3189) | `jobs`, `businesses`, `licensing`, `catalog`, `ai`, `privacy`, `messaging`, `ops` |
| `picks` | Pick by code; contact-method choice; confirm window of 4 business hours by email alone, counted from when the pick email is sent, with a reminder every business hour, each carrying the Hold link, the last one marked "Last reminder" (PICK-019, PICK-037, D-345, D-380); the homeowner's end-time and lapse re-pick texts (PICK-045, PICK-046); EMERGENCY pick emails, at night only to people with "Emergency emails at any hour" on (PICK-037, D-346; the EMERGENCY card email itself is `quotes`' card content, CARD-021, sent through `messaging`'s fan-out, D-482); no automated call to any business phone (BIZ-039, D-344); payment hand-off; release of contact details (portal and email only, PICK-024); losers notified | `picks.pick`, `picks.code_attempt`, `picks.release`, `picks.confirm_reminder` | `quotes`, `jobs`, `payments`, `calls`, `businesses`, `licensing`, `privacy`, `messaging`, `adapters`, `ops` |
| `intake` | Inbound SMS and web chat routing (from flags and `ScrubbedText`, never raw homeowner text); conversation state; homeowner consent; the one 911 text and its keyword list for gas, a CO alarm going off, smoke or fire, sparking or burning electrical and a downed power line (INT-002, INT-004, D-436); the one intake order (INT-043); photos, inspiration pictures and concept images (INT-044 to INT-046), photo checks running on Ask Hava's own server, never an outside AI, until zero retention (D-432); category matching; homeowners and properties | `intake.homeowner`, `intake.property`, `intake.conversation`, `intake.consent_record`, `intake.intake_answer`, `intake.emergency_event`, `intake.recruiting_demand`, `intake.golden_result` (OPS-040, T156, D-493), `intake.web_chat_session` (IDs, a key digest and times only, T103, D-4044); `intake.conversation.request_source` records how the request started (INT-047, T103, D-493, D-4044) | `picks`, `quotes`, `jobs`, `businesses`, `ai`, `privacy`, `catalog`, `messaging`, `ops` |
| `recovery` | Backup checks, the restore drill, replay with derived IDs | `recovery.backup_check`, `recovery.restore_drill`, `recovery.replay_run`, `recovery.replay_item` | the `api` of every module in layers 0–7 |
| `portals` | Homeowner page and portal, business portal, owner portal; their sign-in, sessions and second factors; the business action pages and signed single-purpose links in business emails that replace text keywords (BIZ-038, SEC-025, D-309), which act only from the page's button, never on opening (SEC-026), with Quote links that need no session, Confirm by link plus a Verify code, and the shared-inbox switch (SEC-004, SEC-027, D-333); the per-job notice log (BIZ-045, D-332); no service worker until the later app phase (BIZ-041) | `portals.session`, `portals.login_link`, `portals.code_try` (numbered code-check reservations, T272, D-1837), `portals.known_device`, `portals.owner_credential`, `portals.passkey`, `portals.totp_secret` (encrypted), `portals.recovery_code` (hashed), `portals.action_link` (hashed) | the `api` of every module in layers 0–7 |
| `site` | Public website in the daylight-and-evergreen look of `docs/design/DESIGN_SPEC.md` (D-3331); the web-chat page | none (web-chat text goes to `intake`) | `intake`, `catalog`, `ops`, `payments`, `businesses` (D-491) |

`core` owns `core.task` (the task queue and timers) and `core.rate_limit` (D-090).

Twelve v9.3 record types map onto these tables (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:79`–`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:96`): Contact → `privacy.contact`; Homeowner, Property → `intake`; Request → `jobs.job`; Photo → `privacy.photo`; Contractor → `businesses.business` plus `licensing.business_license` and `payments.customer`; Job card, Quote → `quotes`; Pick → `picks.pick`; Ledger entry → `payments.ledger_entry`; Message → `messaging.outbox`/`messaging.inbox`, with homeowner message text in `privacy.message_body`; Alert/audit → `ops`.

## 4. Dependency rules

1. **Layers.** A module may use only the `api` of a module in a lower layer, never its `models` or `service`; the pure decision functions a lower crate re-exports as `api::rules` are part of its `api` and may be called (D-091, D-481, D-484). In Rust this is a compile error, not a convention: a crate lists only its allowed lower crates in `Cargo.toml`, Cargo refuses a dependency cycle, and only `api` is `pub` (D-453):

   | Layer | Modules |
   |---|---|
   | 0 | `core` |
   | 1 | `ops`, `adapters` |
   | 2 | `catalog`, `privacy` |
   | 3 | `ai`, `messaging`, `payments`, `licensing` |
   | 4 | `businesses`, `jobs` |
   | 5 | `calls`, `quotes` |
   | 6 | `picks` |
   | 7 | `intake` |
   | 8 | `recovery`, `portals`, `site` |

   `adapters` uses only `core` and the HTTP and crypto crates. Same-layer imports are allowed only where section 3's "Calls" column lists them (none today). The layer table is kept in machine-readable form in `kit/layers.json` (protected), which the architecture test reads (D-479). There is no `testing` feature: unit and property tests live inside each crate, and the test-layer crates see only `api` (D-481).
2. **Events go up, calls go down.** When a lower module must tell a higher one something (Stripe says a charge succeeded; an inbound text arrives; a license lapses; an alert is raised), it enqueues a `core.task` with an event kind and IDs only, never personal details. `app/server/src/lib.rs` (`register_handlers()`) registers which module's handler runs for each kind (D-481). No module imports a higher one.
3. **Tables.** Every query and migration names its own module's schema. The architecture test (T175, SEC-029) reads every crate's `Cargo.toml` and every migration and fails on a dependency not allowed by the layer table, any cross-schema foreign key, and any query that names another module's schema (the exceptions: `core` may name `public`, to lock it down and hold `_sqlx_migrations`; and a `core` migration may name other modules' schemas only when every statement in it is `ALTER DEFAULT PRIVILEGES IN SCHEMA <schema>, … REVOKE <privileges> ON TABLES FROM askhava_app`, which can only take a default away, T250, D-1632), on a migration whose first line (or second, after `-- no-transaction`) does not name its module and task, whose number does not rise, whose owner is missing from `kit/tasks.json`, or whose number order disagrees with its tasks' dependency order; and on any SQL outside the query macros (the function forms, `AssertSqlSafe`, `QueryBuilder`, `Executor`, `query_unchecked!`), found by a lexer scan with comments and strings removed (D-491); and on dynamic SQL, which a scan of names cannot see through (in a migration a `DO` statement, an `EXECUTE` that runs a string or a `format` or `quote_*` call; in a query macro's SQL `DO` or `EXECUTE`), outside a short allow list in the test that gives each file's reason (T252, D-1677); SQLx checks each query against the schema when it compiles.
4. **Money** moves only through `payments` (FEE-032). **Vault plaintext** comes only from `privacy::api` and only to the readers in section 8. **Release needs proof:** the function that reads contact fields for a release takes a `ReleaseProof`. Because `privacy` (layer 2) cannot name a `payments` (layer 3) type, the design is a capability (D-481): `privacy::api` defines `ReleaseProof` and a non-`Clone` `ReleaseMint`, both with private fields; `privacy::api::release_mint()` can be called once per process and panics on a second call; `server/src/lib.rs` calls it once in `register_handlers()` and hands the mint to `payments` (which therefore calls `privacy`, D-488), and `payments` alone turns a signature-checked Stripe event for a succeeded charge, or a fee decision of no fee due, into a proof. What this proves and what it does not (the skeleton trial, D-488): the compile-fail cases in a crate with no feature on prove that no other crate can construct a proof or a mint (struct literal, `Default`, `Clone`); they cannot stop a crate that depends on `privacy` from calling `release_mint()` itself, so the once-guard and the architecture test's source scan for `ReleaseMint`, `release_mint`, `SessionMint` and `session_mint` outside `privacy`, `payments`, `server` (and `portals` for the session mint) are what stop a second minter (SEC-030). Because the mint is taken once per process, tests that build the composition share one fixture per test binary. The types are T022's; the minter and the server hookup are T189's. The homeowner and owner realm reads take the capability types of SEC-046 the same way.
5. **Statuses are enums.** Job, pick, quote, charge and consent statuses are Rust enums; every `match` on them names every status: the modules that act on statuses deny clippy `wildcard_enum_match_arm` (not the whole workspace, since `Option` and foreign non-exhaustive enums need a wildcard arm), and a source scan fails on `_ =>`, `matches!` or `if let` on a status enum outside its own helpers, so a new status does not build until every place handles it (SEC-031).
6. **Invariants** (`BUILD_BRIEF.md:148`–`BUILD_BRIEF.md:152`): every outside action carries an idempotency key derived from what it identifies (SEC-018); every state change writes an audit row through `ops.api.audit()` in the same transaction (JOB-005); no homeowner contact detail leaves the vault except on a confirmed pick that is paid or has a recorded $0 fee decision (PRIV-005, D-477); the ledger always reconciles to Stripe (FEE-029, FEE-030).

## 5. Shared kernel (`core`)

`core` holds what every module needs and owns no business data (D-090):

- **Settings:** one settings struct built by `Settings::from_pairs(impl IntoIterator<Item = (String, String)>)` (a hand-written parser over the `ASKHAVA_` prefix: unknown `ASKHAVA_*` keys are refused, other variables ignored, and no error message ever echoes a value; `from_iter` would trip clippy's `should_implement_trait`, D-491), which the binaries feed from `std::env::vars()` and tests from a map (edition 2024 makes `set_var` unsafe, and `unsafe` is forbidden, D-481); it refuses to start on a missing value, a live key in a non-live environment, a test key in live, or a vendor account identifier from the other environment's allow-list (SEC-002, SEC-036; `BUILD_BRIEF.md:521`).
- **Database:** the SQLx connection pool and a transaction helper; the `Guarded<T>` outcome type (`Applied` or `MovedOn`) for "only if still in the expected status" updates, each module writing its own guarded `UPDATE … WHERE id = $1 AND status = $2 RETURNING …` as a literal macro, since one shared statement cannot name another module's table (JOB-004, D-491; `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:689`). Two database roles: a migration role that owns the schemas, and an app role with no `UPDATE` or `DELETE` on append-only tables (D-092). Roles are created outside migrations, by the host or a bootstrap script with passwords from host variables; migrations hold only idempotent `GRANT`s and `ALTER DEFAULT PRIVILEGES` per schema; `server migrate` (`sqlx::migrate!`) is the release step; the append-only tests run `SET ROLE askhava_app` so the rule is exercised, not assumed (D-481). The pool's `max_connections` per process is set so that web replicas times pool plus the worker stays under the host's connection limit.
- **Clock:** UTC storage; Arizona time (no daylight saving) and the business clock of 8:00 a.m.–8:00 p.m. every day (JOB-007, D-004); the local time in a recipient's zone, which `messaging` uses for quiet hours (D-003, D-025); tests inject the clock. `Clock::real_now()` (D-5836) is the instant for signature freshness, rate limits, security events and sessions; it equals `now()` everywhere but the test-control clock, where `now()` carries the advanced offset and `real_now()` stays at system time, so the e2e web binary can serve business time from the test-control clock (T849) while those reads stay real (T848); the worker's session sweep and rate-limit prune take `real_now()` as their cutoff and `now()` for their next run; the business portal stays on `SystemClock` because its statement compares with the ledger's database time; the named test-only exceptions are a code-send admission that carries a per-number cap, which shares the code's instant, and the voice daily caps on the webhook clock (production identical, D-5836).
- **Webhook dedupe:** one helper that records a vendor event ID in the calling module's own `webhook_event` table and reports whether it was new, so every module dedupes the same way (SEC-017).
- **Derived IDs:** HMAC-SHA-256 under a server secret over what an ID identifies, for job cards, quotes, picks, quote-page links and Stripe idempotency keys (CARD-017, PICK-028, OPS-016).
- **Task queue:** `core.task` (kind, IDs-only payload, `run_at`, attempts, lease, unique dedupe key). Workers claim with `FOR UPDATE SKIP LOCKED` (`attempts < max_attempts`, lease expired or null, ordered by `run_at`, the lease end computed in Rust from an injected clock), finish or fail with the lease owner in the `WHERE` (fencing: a stale owner's finish returns false), hold a lease that is renewed at half-life, and a crashed worker's lease expires so another picks the task up; `register_handlers()` returns the registry, which tests hold in a `OnceLock` (D-488); the trial of D-491 measured 100 tasks of 20 ms across two workers in 1.0 s and about 350 to 400 instant tasks per second, each exactly once. Every handler first re-checks the current status and does nothing if things moved on (JOB-006). The worker is one Tokio runtime running a claim loop of fixed concurrency (a `JoinSet` behind a semaphore), a poll interval with backoff, lease renewal for long handlers, a per-handler timeout, and nightly jobs inserted under date-derived dedupe keys so several replicas never double-run; on SIGTERM it stops claiming, drains within the host's grace period and releases its leases; `web` uses `axum::serve(...).with_graceful_shutdown` (D-481). Timestamps are `timestamptz` read and written through `jiff-sqlx` (D-481).
- **Rate limits:** `core.rate_limit` counters per phone, IP and business (SEC-014, SEC-015).
- **Brand:** the one brand file `app/core/data/brand.json`, compiled in: display, business, short, legal and spoken legal names, domain, email sender, card-statement prefix, the words never used alone and the Spanish forms; `brand()` serves it (installed and checked at start-up, a fake one installable in a test) and `Brand::fill` fills the `{brand}`, `{brand_business}`, `{brand_short}`, `{brand_legal}` and `{domain}` tokens of text templates; the build's Python checks read it through `scripts/brand.py`. No customer-visible line names the brand outside it, except the versioned legal records (T291 to T295, D-1932, D-1933).
- **Logging:** `tracing` with a request ID on every line; contact values are wrapped in a `Redacted` type whose `Debug` and `Display` print only a marker, so they cannot reach a log by accident; no phone numbers, names or tokens in logs (PRIV-016); Sentry with default PII off plus a scrubber (PRIV-017). Secrets are `secrecy::SecretString`, zeroed on drop.

## 6. Job lifecycle state machine

Statuses are JOB-001's twelve. A move not listed here is refused and logged as a bug (JOB-002). Every move takes the row lock, checks the expected status, writes `jobs.status_history` and an audit row, and enqueues the next timer (JOB-004 to JOB-006).

```
Intake ──> Open ──> Quotes ready ──> Awaiting confirm ──> Payment pending ──> Connected ──> Done
  │          │  \        ▲  ▲  │            │                    │                │
  │          │   └> No quotes yet           └────────────────────┴────────────────┘
  │          │          │     │               back to Quotes ready (no confirm, card fails,
  │          │          └─────┘ (a quote)     ineligible at Confirm, no call in 72 h, fraud warning before release)
  │          └─> Not covered
  ├─> Not covered (no open category or no active business, INT-017, D-482)
  └─> Closed early
Quotes ready, No quotes yet, Connected ──> Closed     Any non-final status ──> Cancelled
```

(`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:155`–`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:177`, as changed by D-011, D-030, D-043, D-045, D-046.)

**Every waiting status has a timeout** (JOB-003; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:111`). "Business hours" count only 8 a.m.–8 p.m. Arizona time (D-004).

| Status | Waiting for | Timeout | Then | Requirement |
|---|---|---|---|---|
| Intake | Answers, the "I'm safe" reply after an emergency (consent is recorded with the first text, so nothing waits for it, D-419) | Reminder at 24 h, close at 72 h; a returning web-chat visitor sees the request closed | Closed early, reason "no reply", with the INT-019 closing text (D-095, D-419) | INT-019, INT-005, CON-025, CON-027 |
| Intake → Not covered | Nothing: the move happens at once when no open category or no active business in the town covers the request (INT-017, INT-022, D-482) | — | Not covered, with the INT-017 "We don't cover" text and the OPS-041 owner email | INT-016, INT-017, OPS-041 |
| Open | The first quote | 6 p.m. Arizona time the next day | No quotes yet, with the "no quotes yet" text | JOB-009, JOB-010 |
| No quotes yet | A quote | 5 days | Closed | JOB-011 |
| Quotes ready | A pick | 3 days, restarted on every entry; at once if no choosable quote is left and every job card's 24-hour quote window has ended (D-140) | Closed | JOB-012, JOB-025 |
| Awaiting confirm | The business's Confirm (the portal review step, one tap from the pick email; CONFIRM by text only for a business that chose text, PICK-033) | 4 business hours from when the pick notice (email and any chosen text together) is sent (D-148, D-310, D-345), or from the end of the business's BIZ-034 away time, plus 2 business hours once on a HOLD (PICK-043), plus 1 business hour once when all its emails bounced or failed (D-332, D-345); 60 clock minutes on an emergency job (PICK-044, D-482); a second "<n> YES" replaces the pick (PICK-032) | Quotes ready | JOB-013, PICK-019, PICK-043 |
| Payment pending | A working card and Stripe's answer | 1 business hour after a decline or "needs card" (FEE-020, FEE-036), 60 clock minutes on an emergency job (JOB-014, PICK-044, D-482); Stripe asked directly at 30 min (FEE-019); pick released if Stripe is unreachable for 4 h (FEE-018); these timers stop while charges are paused (FEE-038); a Stripe early fraud warning on a charge whose details were not yet released refunds it at once, the pick ending as fraud-refunded and the job returning to Quotes ready with her backup, whether the pick still awaited Confirm or was settled but not released (FEE-052, D-491, D-5888) | Quotes ready | JOB-014, FEE-052 |
| Connected | First call; DONE, NO or PROBLEM at day 3 or day 14 | Call contact: no call in 72 h → Quotes ready; email contact: a day-3 NO → Quotes ready (D-143); unreachable refund decided at 96 h → Closed (FEE-022, JOB-027); no reply 7 days after the day-14 check-in → Closed; an early fraud warning before release → Quotes ready, the pick fraud-refunded (FEE-052, D-5888) | Quotes ready, Done or Closed | JOB-015, JOB-026, JOB-027 |
| Done, Closed, Closed early, Not covered, Cancelled | Nothing (final) | — | — | JOB-001; `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:160` |

Timers that are not job statuses but still bound every wait: each job card's 24-hour quote window (PICK-001); quote-count updates at most every 3 hours (TXT-017); pick code 10 minutes, 5 tries, 15-minute cooldown (PICK-013); sign-in links 15 minutes (SEC-004); masked line closes at 30 days or earlier (CALL-009); text retries 3 times over 30 minutes then email (TXT-014); quote-page tokens die 30 days after close (PICK-011); a failed refund retries hourly (FEE-026); the "we have your message" AI retry every 30 minutes (INT-021); safe mode 24 hours after an unacknowledged High alert, pausing charges and releases only (OPS-001, D-133), with self-cleared delivery alerts acknowledged automatically (OPS-023); a job paused by a sole-word CANCEL keeps its status and timers (CON-029).

**Pick status** (owned by `picks`, kept beside the job status from the pick onward, including while the job is Connected, where paid, free and no-show still apply): awaiting confirm, needs card, charging, auth needed, payment failed, paid, free, expired, cancelled, no-show, fraud-refunded (FEE-052, D-491) (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:93`). Release happens only on paid or free (PICK-022).

**Who makes the payment moves** (T663, D-4349, D-4783): Awaiting confirm → Payment pending is the Confirm press's `picks::api::confirm_move` (as the business, under the pick's row lock; T664 calls it); Payment pending → Connected is `picks::api::settle`, in the settle's transaction before the release's claim (D-2930), which makes both moves for a job still in Awaiting confirm (PICK-048 auto-confirm) and, for a job in any other status, ends the pick as `payment_failed` (`Settled::Left`, refunded by the sink); Payment pending → Quotes ready is `picks::api::end_pending` (the pick and its PaymentIntent ended through `payments`' own pick end) or `pick_ended` on `payments.pick_ended`. `jobs`' Payment pending timer moves nothing: it queues one `picks.payment_returned` task (`jobs::payment_wait`), whose handler restarts the wait while `payments::api::payment_held` holds the pick (a pay timer running or stopped, an attempt in flight, charges paused, or a held success (a succeeded attempt not refunded, D-5888), which also queues `picks.held_success` to T408's held release), raises one High `picks.payment_held_long` alert per pick held over 24 hours, and otherwise ends the pick through `end_pending`; the wait's value is unchanged.

## 7. Adapters and stand-ins

Moved verbatim to `docs/architecture/07-adapters.md` (the runner's logic paragraph: `docs/architecture/harness-runner.md`) (D-6022). New notes for this section go here, below this line, until T885.

## 8. Contact vault data flow

Moved verbatim to `docs/architecture/08-contact-vault.md` (D-6022). New notes for this section go here, below this line, until T885.

## 9. Environments and deployment shape

Moved verbatim to `docs/architecture/09-environments.md` (D-6022). New notes for this section go here, below this line, until T885.

## 10. Off-database records and recovery

Moved verbatim to `docs/architecture/10-off-database-records.md` (D-6022). New notes for this section go here, below this line, until T885.

## 11. Pinned stack versions

Moved verbatim to `docs/architecture/11-pinned-versions.md` (D-6022). New notes for this section go here, below this line, until T885.

## 12. Keeping this file true

Moved verbatim to `docs/architecture/12-keeping-this-file-true.md` (the rules) `docs/architecture/harness-runner.md` (the runner bullets) and `docs/architecture/harness-checks.md` (the CI, smoke and plan-check bullets) (D-6022). New notes for this section go here, below this line, until T885.

## 13. Routes

Every HTTP route the `web` process serves or a queued task's goal says it will serve, one row per method and path (T284, D-1868, D-2269). `Wired` is `yes` for a route `server::build_router` serves on `main` today, and otherwise the ID of the queued task that wires it; that task marks its own row `yes` in the same pull request, and a task that serves a route not listed adds its row as `yes`. `app/server/tests/route_wiring.rs` (TD-SEC-41) compiles this table in and probes the router `build_router` builds with every optional part configured: a `yes` row must reach its route, a task row must not, and a malformed row, a duplicate row or a task ID not in `kit/tasks.json` fails with the row quoted, and a header other than `| Method | Path | Module | Wired |` or a missing separator fails with that line quoted. T285's runner check reads the same table at each task's close. `Module` is the crate that owns the handler or page. A path parameter is written `{name}`, as in the code. The test-control routes (`/__test/...`, compiled only with the `test-control` feature, SEC-035) are not app routes and are not listed. Routes whose path no constant or goal fixes yet (T133's signed inbound email, the owner pages of T304, T305 and T310, T309's help forms, and the public site pages no goal fixes a path for: home and how it works (T102), web chat (T103), services and "What we cover now" (T201), the Spanish pages including the Spanish data pages (T202, T221)) get their row from the task that wires them (D-2270). The site's legal pages (T221's goal: `/terms`, `/privacy`, `/sms-terms`, `/your-data`, `/business-data`) and T201's help page (`askhava.com/help`, D-289) and ROC page (`askhava.com/help/roc-complaint`, WEB-011) are listed, and so are T102's old directory paths (`/categories`, `/events`, WEB-012, D-3144). The T116 Stage 0.5 static set is not served by the `web` process and is not listed. The test checks only the rows listed: an axum router cannot list its routes, so a route served with no row passes, and the task that merges a route adds its row.

| Method | Path | Module | Wired |
|---|---|---|---|
| GET | /q/{token} | portals | yes |
| POST | /q/{token} | portals | yes |
| POST | /q/{token}/pick | portals | yes |
| POST | /q/{token}/code | portals | yes |
| POST | /q/{token}/close | portals | yes |
| GET | /f/{token} | portals | yes |
| POST | /f/{token} | portals | yes |
| GET | /static/portals/feedback.css | portals | yes |
| GET | /static/portals/forms.css | portals | yes |
| GET | /static/portals/quote_page.js | server | yes |
| GET | /static/site/site.css | site | yes |
| GET | /c/{token}/{answer} | portals | yes |
| POST | /c/{token}/{answer} | portals | yes |
| POST | /webhooks/twilio/sms | server | yes |
| POST | /webhooks/twilio/status | server | yes |
| POST | /webhooks/twilio/voice | server | yes |
| POST | /webhooks/twilio/voice/status | server | yes |
| POST | /webhooks/twilio/voice/dialed-status | server | yes |
| POST | /webhooks/twilio/voice/safety-key | server | yes |
| POST | /webhooks/stripe | payments | yes |
| GET | /healthz | server | yes |
| GET | /b | portals | yes |
| POST | /b/open-cards | portals | yes |
| POST | /b/review-links | portals | yes |
| GET | /b/picks/{pick} | portals | yes |
| GET | /b/billing | portals | yes |
| GET | /b/billing/statement.csv | portals | yes |
| GET | /b/notices | portals | yes |
| POST | /b/notices | portals | yes |
| GET | /b/texts | portals | yes |
| POST | /b/texts | portals | yes |
| GET | /b/step-up | portals | yes |
| POST | /b/step-up/send | portals | yes |
| POST | /b/step-up/check | portals | yes |
| GET | /b/picks/{pick}/confirm | portals | yes |
| POST | /b/picks/{pick}/confirm | portals | yes |
| POST | /b/picks/{pick}/confirm/code | portals | yes |
| GET | /b/picks/{pick}/cant-take | portals | yes |
| POST | /b/picks/{pick}/cant-take | portals | yes |
| GET | /b/jobs/{j} | portals | yes |
| GET | /b/sign-in | portals | yes |
| POST | /b/sign-in | portals | yes |
| GET | /b/sign-in/{token} | portals | yes |
| POST | /b/sign-in/{token} | portals | yes |
| GET | /b/a/{kind}/{object}/{token} | portals | yes |
| POST | /b/a/{kind}/{object}/{token} | portals | yes |
| GET | /b/signup | portals | yes |
| POST | /b/signup/start | portals | yes |
| POST | /b/signup/send | portals | yes |
| POST | /b/signup/old | portals | yes |
| POST | /b/signup/letter | portals | yes |
| POST | /b/signup/identity-code | portals | yes |
| POST | /b/signup/email-code | portals | yes |
| POST | /b/signup/email-code/resend | portals | yes |
| POST | /b/signup/service-area | portals | yes |
| GET | /b/signup/inbox | portals | yes |
| POST | /b/signup/inbox | portals | yes |
| GET | /b/signup/capability | portals | yes |
| POST | /b/signup/capability | portals | yes |
| GET | /b/complaints | portals | yes |
| POST | /b/complaints/{complaint}/reply | portals | yes |
| POST | /b/not-a-fit/{object}/{token} | portals | yes |
| GET | /b/charges/{pick} | portals | yes |
| GET | /b/emergency-emails | portals | yes |
| POST | /b/emergency-emails | portals | yes |
| GET | /b/job-types | portals | yes |
| POST | /b/job-types | portals | yes |
| GET | /b/picks/{pick}/notices | portals | yes |
| GET | /b/phones | portals | yes |
| POST | /b/phones/approve | portals | yes |
| POST | /b/phones/reject | portals | yes |
| GET | /owner/sign-in | portals | yes |
| POST | /owner/sign-in | portals | yes |
| POST | /owner/sign-in/passkey | portals | yes |
| GET | /owner/step-up | portals | yes |
| POST | /owner/step-up | portals | yes |
| POST | /owner/sign-out | portals | yes |
| POST | /owner/sign-out-everywhere | portals | yes |
| GET | /owner/home | server | yes |
| GET | /owner/settings | server | yes |
| POST | /owner/pause | server | yes |
| POST | /owner/fee-mode | server | yes |
| POST | /owner/category | server | yes |
| POST | /owner/texting | server | yes |
| POST | /owner/banner | server | yes |
| POST | /owner/alert-ack | server | yes |
| POST | /owner/call-homeowner | server | yes |
| POST | /owner/opt-out | server | yes |
| POST | /owner/stripe-answer | server | yes |
| POST | /owner/safety-call | server | yes |
| POST | /owner/spanish-conversations | server | yes |
| POST | /owner/business-texting | server | yes |
| GET | /owner/release | server | yes |
| POST | /owner/release | server | yes |
| GET | /owner/problem-reports | server | yes |
| POST | /owner/problem-complaint | server | yes |
| GET | /owner/refunds/{pick} | server | yes |
| POST | /owner/refunds/{pick}/refund | server | yes |
| POST | /owner/refunds/{pick}/evidence | server | yes |
| GET | /owner/audit-log | server | yes |
| GET | /owner/health | server | yes |
| GET | /owner/scorecard | server | yes |
| POST | /owner/scorecard/minutes | server | yes |
| GET | /owner/intake-report | server | yes |
| GET | /owner/money | server | yes |
| GET | /owner/money/csv | server | yes |
| POST | /owner/money/expense | server | yes |
| GET | /owner/digest | portals | yes |
| GET | /owner/help | portals | yes |
| POST | /owner/help/reply | portals | yes |
| POST | /owner/help/close | portals | yes |
| GET | /h | portals | yes |
| GET | /h/sign-in | portals | yes |
| POST | /h/sign-in | portals | yes |
| POST | /h/sign-in/code | portals | yes |
| POST | /h/texts-off | portals | yes |
| GET | /h/my-data | portals | yes |
| POST | /h/my-data/request | portals | yes |
| POST | /h/my-data/code | portals | yes |
| POST | /h/my-data/resend | portals | yes |
| GET | /h/requests/{job}/photos | portals | yes |
| POST | /h/requests/{job}/photos | portals | yes |
| GET | /photos/{token} | server | yes |
| GET | /terms | site | yes |
| GET | /privacy | site | yes |
| GET | /sms-terms | site | yes |
| GET | /your-data | site | yes |
| GET | /business-data | site | yes |
| GET | /es/your-data | site | yes |
| GET | /es/business-data | site | yes |
| GET | /help | site | yes |
| GET | /accessibility | site | yes |
| GET | /help/roc-complaint | site | yes |
| GET | / | site | yes |
| GET | /how-it-works | site | yes |
| GET | /es/how-it-works | site | yes |
| GET | /categories | site | yes |
| GET | /events | site | yes |
| GET | /services | site | yes |
| GET | /coverage | site | yes |
| GET | /request | site | yes |
| POST | /request | site | yes |
| GET | /business | site | yes |
| GET | /es/business | site | yes |
| GET | /business/help | site | yes |
| GET | /es | site | yes |
| POST | /q/{token}/updates | portals | yes |
| POST | /q/{token}/updates/confirm | portals | yes |
| POST | /b/quote/{job}/auto-confirm | portals | T731 |
| POST | /b/jobs/{j}/withdraw | portals | yes |
| GET | /b/settings/service-area | portals | yes |
| POST | /b/settings/service-area | portals | yes |
| POST | /b/shared-inbox | portals | yes |
| GET | /b/contact-card.vcf | portals | yes |
| GET | /b/leave | portals | T734 |
| POST | /b/leave | portals | T734 |
| GET | /b/pause-reason | portals | T734 |
| POST | /b/pause-reason | portals | T734 |
| GET | /b/settings/towns-hours | portals | T735 |
| POST | /b/settings/towns-hours | portals | T735 |
| GET | /owner/ai | portals | yes |
| POST | /owner/ai/terms | portals | yes |
| POST | /owner/ai/policy | portals | yes |
| POST | /owner/ai/switch | portals | yes |
| POST | /b/team/role | portals | yes |
| POST | /b/team/remove | portals | yes |
| POST | /b/billing/limit | portals | yes |
| GET | /b/team | portals | yes |
| GET | /help/my-data/{object}/{token} | portals | yes |
| POST | /help/my-data/{object}/{token} | portals | yes |
| GET | /b/addresses | portals | yes |
| POST | /b/addresses/request | portals | yes |
| POST | /b/addresses/confirm | portals | yes |
| POST | /b/addresses/remove-backup | portals | yes |
| GET | /b/help | portals | yes |
| POST | /b/help | portals | yes |
| POST | /help | server | yes |
