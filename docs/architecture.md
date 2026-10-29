# Ask Hava architecture

Written in kit step K4 (`BUILD_BRIEF.md:598`). This file finalizes the module boundaries that `BUILD_BRIEF.md:124`–`BUILD_BRIEF.md:152` fixes, and pins the stack versions (`BUILD_BRIEF.md:121`). Since D-453 the server is Rust (`BUILD_BRIEF.md:105`); sections 2, 4, 5, 7 and 11 describe the Rust layout. It restates and arranges rules; where it had to choose, the choice is logged in `docs/decisions.md` (D-090 to D-100) and named here. If this file and a cited source disagree, the source wins and this file is the bug. Every PR that changes a boundary, table owner, state, timeout or pinned version updates this file in the same PR (`BUILD_BRIEF.md:489`).

Citations are `path:LINE`. "Changes file" means `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md`; "v9.3" means `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md`. Requirement IDs (for example `PRIV-005`) are in `docs/spec/`.

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
                     version under [workspace.dependencies]; lints (unsafe_code forbid, clippy) under [workspace.lints]
Cargo.lock           committed
.sqlx/               offline query data from `cargo sqlx prepare --workspace`, committed; `Ci` builds with SQLX_OFFLINE
rust-toolchain.toml  the pinned Rust release (section 11)
clippy.toml          too-many-lines-threshold = 50, excessive-nesting-threshold
deny.toml            cargo-deny policy: license allow-list, crates.io only, banned crates (SEC-021)
app/
  server/            composition root (a crate that depends on every module)
    src/main.rs      builds the Axum router, registers routes and task handlers (the shared composition file)
    src/bin/worker.rs        worker entry point
    src/bin/rules_bridge.rs  JSON-in, JSON-out door to the rules for the twin (T074); behind the server crate's
                     `rules-bridge` feature (`required-features`), never in the production image
  core/              shared kernel (section 5); not a business module
  <module>/          one crate per module in section 3
    Cargo.toml       depends only on the lower crates section 4 allows (the compiler refuses anything else)
    src/lib.rs       `automod::dir!("src")` declares every file, so a task adds a file without editing lib.rs;
                     re-exports `api` as the crate's public surface; under the crate's `testing` feature the same
                     macro declares every module `pub`, so the test crates reach `rules` and the rest, and no
                     production crate may enable `testing` (the architecture test checks it, SEC-029). Cargo turns a
                     feature on for the whole build, so a workspace build makes internals visible everywhere;
                     `Ci` therefore also builds `-p server` alone, where only `api` is public, and that build is
                     the one that enforces the boundary
    src/api.rs       the module's public functions and types; everything else is `pub(crate)`
    src/models.rs    row types and SQLx queries, all in the module's own Postgres schema
    src/rules.rs     pure decision logic (no I/O), the main target of unit and property tests
    src/service.rs   orchestration: transactions, calls to lower crates' api, audit rows
    src/handlers.rs  task handlers registered in main.rs (timers and events)
    src/routes.rs    HTTP routes, only in crates that own a web surface (portals, site, webhooks)
    templates/       Askama templates (checked when the crate compiles); all customer-facing words live in
                     templates and message files, never in `.rs` string literals, so the AI-smell scan sees them.
                     The `.vcf` contact card is not Askama: one function escapes each field per RFC 6350
                     (backslash, comma, semicolon, newline) before it is written
    static/          CSS, fonts, images and `.ts` sources; `npm run build` (tsc) writes the `.js` beside each
                     source in the Docker build stage (T169); the generated `.js` is not committed
migrations/          SQL files `0NNN_<slug>.sql`, NNN the task number, forward-only, each tested; SQLx applies
                     any not yet applied in number order, including a lower number added later (checked, D-453).
                     On a fresh database the files run in number order, not task order; that is safe only
                     because no migration refers to another module's schema (section 4, rule 3)
stand_ins/           Python fakes of Twilio, Stripe, Resend, Lob and the AI, served over HTTP by `stand_ins/server.py`
                     at the vendors' URL shapes, so the Rust adapters use the same client code everywhere (SEC-032)
tests/               Rust layers unit, property, integration, security, recovery: one test crate each (T168, and
                     integration in T009), files in tests/<layer>/tests/, compile-fail cases with their expected
                     `.stderr` in tests/security/ui/; `tests/fixtures` (T010) is the fake-data crate; Playwright in
                     TypeScript for e2e and a11y; pytest for docs and contract
twin/                the twin (Python) and its hard-failing gate, calling the Rust rules through rules_bridge
```

## 3. Modules, what each owns, and its tables

Each module owns its files and its tables, and no module reaches into another's tables (`BUILD_BRIEF.md:126`). Each module's tables live in a Postgres schema named after it (D-091). "Calls" lists the lower crates whose `api` it may use, which are exactly the path dependencies in its `Cargo.toml`; `core` is always allowed and is not repeated.

| Module | Owns (`BUILD_BRIEF.md:128`–`BUILD_BRIEF.md:146`) | Tables (schema-qualified) | Calls |
|---|---|---|---|
| `ops` | Alerts, pause switches, safe mode, status banner, web-and-email mode, audit log; which alerts push to Casey's phone, the 9 a.m. and 3 p.m. daytime batch push (OPS-032) and the weekly digest of everything else; safe mode only from pushed alerts; the safety-voice-call setting, off until a TCPA attorney's written approval is recorded (CALL-019); web-and-email mode able to run the whole service (TXT-034); the business-texting switch and the filed written answer it needs (TXT-035); alerts to Casey by email and, for critical safety alerts, a call (OPS-035); help requests: HELP auto-answers (CON-034), one HELP notification per request to the alert address with hourly and daily limits, the owner help page and its screened reply box, and the business "Get help" form (OPS-036, BIZ-047, D-348); the sending-limit alert (OPS-004, D-351) (D-218, D-219, D-275, D-277, D-278, D-297, D-303, D-314) | `ops.audit_log` (append-only), `ops.alert`, `ops.alert_ack`, `ops.pause_switch`, `ops.switch_history`, `ops.safe_mode`, `ops.status_banner`, `ops.help_request`, `ops.help_message` | — |
| `adapters` | One interface per vendor; real, sandbox and stand-in implementations chosen by configuration | none | — |
| `catalog` | Categories, job types, license rules, fee rules, feature switches (fee mode, category on/off) and numeric settings | `catalog.category`, `catalog.job_type`, `catalog.license_rule`, `catalog.fee_rule`, `catalog.setting`, `catalog.setting_history` | `ops` |
| `privacy` | The plain-code scrubber; the contact vault (field-level encrypted); the inbound reader that is the only code to see raw homeowner text (section 8); retention and purge jobs; photos | `privacy.contact`, `privacy.contact_index` (blind index), `privacy.message_body` (homeowner message text only), `privacy.photo`, `privacy.retention_hold`, `privacy.purge_run` | `ops` |
| `ai` | Request reading through the AI adapter, and quote summarizing only while the owner setting "AI quote summaries" is on (off by default and at launch, PICK-003, D-373); strict schemas; accepts only `ScrubbedText`, homeowner and business quote text alike (AI-002) | `ai.reading` (fields only, no personal details), `ai.call_stat` | `adapters`, `privacy` (the `ScrubbedText` type only), `ops` |
| `messaging` | Outbox and inbox (never send twice); templates in English and Spanish; quiet hours; opt-out keywords and the one suppression list fed by every channel (text, help email, email replies, help form, portal switch, owner-recorded requests) and checked before every text and automated call (CON-035, D-372); the sender directory (which numbers are business phones); delivery tracking; email and letters; the business-notice fan-out by each business's chosen channels (email by default; text or both only while business texting is on), every channel of one notice sent or held together, and email bounce and complaint handling (BIZ-019, BIZ-040, QH-005, D-307, D-308); one fixed sender name and address for business notices (SEC-025); the open-cards email at up to two times a day, with a waiting pick first (CARD-022, D-380), and the monthly missed-card account email (CARD-022, BIZ-044, D-330, D-331); the email footer with the LLC postal address and "Manage email notices" (SEC-028); the email-limit deferral of non-urgent emails above 90% of the daily limit, nothing dropped (OPS-038, D-381); no homeowner details in any business text (TXT-036) | `messaging.outbox`, `messaging.inbox` (scrubbed homeowner text and flags; business text as sent), `messaging.sender_directory` (blind-indexed business and staff phones), `messaging.delivery_event`, `messaging.opt_out`, `messaging.opt_out_event`, `messaging.email_outbox`, `messaging.email_event`, `messaging.letter`, `messaging.webhook_event` | `adapters`, `privacy`, `catalog`, `ops` |
| `payments` | Stripe customers, optional saved cards, card authorization, on-session Confirm-and-pay charges started only by the business's press on the Confirm page (FEE-014, FEE-036, D-365), the one off-session exception being the optional auto-confirm charge on a quote the business ticked, offered only while Stripe's written answer is filed (PICK-048, D-401), the money view's set-aside and manual expenses outside the ledger (FEE-046, D-410), refunds, disputes, the per-charge evidence record (FEE-042), free first wins, the ledger, nightly reconcile | `payments.customer`, `payments.payment_method`, `payments.card_authorization`, `payments.charge_attempt`, `payments.charge_evidence` (append-only), `payments.ledger_entry` (append-only), `payments.refund`, `payments.dispute`, `payments.free_win`, `payments.webhook_event`, `payments.reconcile_run` | `adapters`, `catalog`, `ops` |
| `licensing` | Agency daily lists; live lookup at pick; other agencies; the city license list; handyman rules | `licensing.list_import`, `licensing.agency_record`, `licensing.city_license_entry`, `licensing.business_license`, `licensing.license_check` | `adapters`, `catalog`, `ops` |
| `businesses` | Sign-up, identity chain, job-type checklist, towns, tags, business consent records, pause rules (including PAUSE and BUSY, D-198), staff phones, team members and their roles (D-191), complaints and business replies (D-200), the channel preference and verified notice emails (BIZ-036, BIZ-040, SEC-024, D-307, D-312); the sign-up deliverability check with a sample card, backup notice addresses, the "Emergency emails at any hour" switch with its test emergency email, and the "reply OK, star, add to contacts" step; no automated-call consent is stored since no business is ever called (BIZ-037, BIZ-039, BIZ-042, BIZ-043, CON-019, D-328, D-329, D-344, D-346, D-352) | `businesses.business`, `businesses.town`, `businesses.job_type_choice`, `businesses.tag`, `businesses.staff_phone`, `businesses.team_member`, `businesses.consent_record`, `businesses.identity_attempt`, `businesses.signup_review`, `businesses.pause_event`, `businesses.complaint`, `businesses.channel_preference`, `businesses.email_verification` | `licensing`, `payments`, `messaging`, `catalog`, `adapters`, `ops` |
| `jobs` | The job lifecycle state machine (section 6); job numbers; status history; quote-page token hashes | `jobs.job`, `jobs.status_history`, `jobs.job_number` | `messaging`, `catalog`, `ops` |
| `calls` | Masked numbers from a pool, PIN routing, call sessions, call logs (never recorded), built on standard programmable voice, never Twilio Proxy (CALL-021, D-296) | `calls.number_pool`, `calls.call_session`, `calls.call_log`, `calls.webhook_event` | `businesses`, `privacy`, `messaging`, `adapters`, `ops` |
| `quotes` | Who receives each job card, and the cards themselves; quote intake and parsing, including the required quote facts, pre-filled per job type from the business's last answers (D-233, D-283); eligibility rules; arrival order; original wording beside any summary | `quotes.job_card`, `quotes.quote`, `quotes.quote_bounce` | `jobs`, `businesses`, `licensing`, `catalog`, `ai`, `privacy`, `messaging`, `ops` |
| `picks` | Pick by code; contact-method choice; confirm window of 4 business hours by email alone, counted from when the pick email is sent, with a reminder every business hour, each carrying the Hold link, the last one marked "Last reminder" (PICK-019, PICK-037, D-345, D-380); the homeowner's end-time and lapse re-pick texts (PICK-045, PICK-046); EMERGENCY card and pick emails, at night only to people with "Emergency emails at any hour" on (CARD-021, D-346); no automated call to any business phone (BIZ-039, D-344); payment hand-off; release of contact details (portal and email only, PICK-024); losers notified | `picks.pick`, `picks.code_attempt`, `picks.release`, `picks.confirm_reminder` | `quotes`, `jobs`, `payments`, `calls`, `businesses`, `licensing`, `privacy`, `messaging`, `adapters`, `ops` |
| `intake` | Inbound SMS and web chat routing (from flags and `ScrubbedText`, never raw homeowner text); conversation state; homeowner consent; the one 911 text and its keyword list for gas, a CO alarm going off, smoke or fire, sparking or burning electrical and a downed power line (INT-002, INT-004, D-436); the one intake order (INT-043); photos, inspiration pictures and concept images (INT-044 to INT-046), photo checks running on Ask Hava's own server, never an outside AI, until zero retention (D-432); category matching; homeowners and properties | `intake.homeowner`, `intake.property`, `intake.conversation`, `intake.consent_record`, `intake.intake_answer`, `intake.emergency_event`, `intake.recruiting_demand` | `picks`, `quotes`, `jobs`, `businesses`, `ai`, `privacy`, `catalog`, `messaging`, `ops` |
| `recovery` | Backup checks, the restore drill, replay with derived IDs | `recovery.backup_check`, `recovery.restore_drill`, `recovery.replay_run`, `recovery.replay_item` | the `api` of every module in layers 0–7 |
| `portals` | Homeowner page and portal, business portal, owner portal; their sign-in, sessions and second factors; the business action pages and signed single-purpose links in business emails that replace text keywords (BIZ-038, SEC-025, D-309), which act only from the page's button, never on opening (SEC-026), with Quote links that need no session, Confirm by link plus a Verify code, and the shared-inbox switch (SEC-004, SEC-027, D-333); the per-job notice log (BIZ-045, D-332); no service worker until the later app phase (BIZ-041) | `portals.session`, `portals.login_link`, `portals.known_device`, `portals.owner_credential`, `portals.passkey`, `portals.totp_secret` (encrypted), `portals.recovery_code` (hashed), `portals.action_link` (hashed) | the `api` of every module in layers 0–7 |
| `site` | Public website in the plaster and brass look of `docs/design/DESIGN_SPEC.md` (D-451); the web-chat page | none (web-chat text goes to `intake`) | `intake`, `catalog` |

`core` owns `core.task` (the task queue and timers) and `core.rate_limit` (D-090).

Twelve v9.3 record types map onto these tables (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:79`–`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:96`): Contact → `privacy.contact`; Homeowner, Property → `intake`; Request → `jobs.job`; Photo → `privacy.photo`; Contractor → `businesses.business` plus `licensing.business_license` and `payments.customer`; Job card, Quote → `quotes`; Pick → `picks.pick`; Ledger entry → `payments.ledger_entry`; Message → `messaging.outbox`/`messaging.inbox`, with homeowner message text in `privacy.message_body`; Alert/audit → `ops`.

## 4. Dependency rules

1. **Layers.** A module may use only the `api` of a module in a lower layer, never its `models`, `service` or `rules` (D-091). In Rust this is a compile error, not a convention: a crate lists only its allowed lower crates in `Cargo.toml`, Cargo refuses a dependency cycle, and only `api` is `pub` (D-453):

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

   `adapters` uses only `core` and the HTTP and crypto crates. Same-layer imports are allowed only where section 3's "Calls" column lists them (none today).
2. **Events go up, calls go down.** When a lower module must tell a higher one something (Stripe says a charge succeeded; an inbound text arrives; a license lapses; an alert is raised), it enqueues a `core.task` with an event kind and IDs only, never personal details. `app/server/src/main.rs` registers which module's handler runs for each kind. No module imports a higher one.
3. **Tables.** Every query and migration names its own module's schema. The architecture test (T009, SEC-029) reads every crate's `Cargo.toml` and every migration and fails on a dependency not allowed by the layer table, any cross-schema foreign key, and any query that names another module's schema; SQLx checks each query against the schema when it compiles.
4. **Money** moves only through `payments` (FEE-032). **Vault plaintext** comes only from `privacy::api` and only to the readers in section 8. **Release needs proof:** the function that reads contact fields for a release takes a `ReleaseProof`, a type with private fields that only `payments` creates, from a signature-checked Stripe event for a succeeded charge or from a fee decision of no fee due; code anywhere else cannot build one, and a compile-fail test proves it (SEC-030).
5. **Statuses are enums.** Job, pick, quote, charge and consent statuses are Rust enums; every `match` on them names every status: the modules that act on statuses deny clippy `wildcard_enum_match_arm` (not the whole workspace, since `Option` and foreign non-exhaustive enums need a wildcard arm), and a source scan fails on `_ =>`, `matches!` or `if let` on a status enum outside its own helpers, so a new status does not build until every place handles it (SEC-031).
6. **Invariants** (`BUILD_BRIEF.md:148`–`BUILD_BRIEF.md:152`): every outside action carries an idempotency key derived from what it identifies (SEC-018); every state change writes an audit row through `ops.api.audit()` in the same transaction (JOB-005); no homeowner contact detail leaves the vault except on a confirmed, paid pick (PRIV-005); the ledger always reconciles to Stripe (FEE-029, FEE-030).

## 5. Shared kernel (`core`)

`core` holds what every module needs and owns no business data (D-090):

- **Settings:** one settings struct, parsed from environment variables with serde at startup, and refuses to start on a missing value, a live key in a non-live environment, or a test key in live (SEC-002; `BUILD_BRIEF.md:521`).
- **Database:** the SQLx connection pool and a transaction helper; row locks with "only if still in the expected status" updates (JOB-004; `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:689`). Two database roles: a migration role that owns the schemas, and an app role with no `UPDATE` or `DELETE` on append-only tables (D-092).
- **Clock:** UTC storage; Arizona time (no daylight saving) and the business clock of 8:00 a.m.–8:00 p.m. every day (JOB-007, D-004); the local time in a recipient's zone, which `messaging` uses for quiet hours (D-003, D-025); tests inject the clock.
- **Webhook dedupe:** one helper that records a vendor event ID in the calling module's own `webhook_event` table and reports whether it was new, so every module dedupes the same way (SEC-017).
- **Derived IDs:** HMAC-SHA-256 under a server secret over what an ID identifies, for job cards, quotes, picks, quote-page links and Stripe idempotency keys (CARD-017, PICK-028, OPS-016).
- **Task queue:** `core.task` (kind, IDs-only payload, `run_at`, attempts, lease, unique dedupe key). Workers claim with `FOR UPDATE SKIP LOCKED`, hold a lease, and a crashed worker's lease expires so another picks the task up. Every handler first re-checks the current status and does nothing if things moved on (JOB-006).
- **Rate limits:** `core.rate_limit` counters per phone, IP and business (SEC-014, SEC-015).
- **Logging:** `tracing` with a request ID on every line; contact values are wrapped in a `Redacted` type whose `Debug` and `Display` print only a marker, so they cannot reach a log by accident; no phone numbers, names or tokens in logs (PRIV-016); Sentry with default PII off plus a scrubber (PRIV-017). Secrets are `secrecy::SecretString`, zeroed on drop.

## 6. Job lifecycle state machine

Statuses are JOB-001's twelve. A move not listed here is refused and logged as a bug (JOB-002). Every move takes the row lock, checks the expected status, writes `jobs.status_history` and an audit row, and enqueues the next timer (JOB-004 to JOB-006).

```
Intake ──> Open ──> Quotes ready ──> Awaiting confirm ──> Payment pending ──> Connected ──> Done
  │          │  \        ▲  ▲  │            │                    │                │
  │          │   └> No quotes yet           └────────────────────┴────────────────┘
  │          │          │     │               back to Quotes ready (no confirm, card fails,
  │          │          └─────┘ (a quote)     ineligible at Confirm, no call in 72 h)
  │          └─> Not covered
  └─> Closed early
Quotes ready, No quotes yet, Connected ──> Closed     Any non-final status ──> Cancelled
```

(`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:155`–`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:177`, as changed by D-011, D-030, D-043, D-045, D-046.)

**Every waiting status has a timeout** (JOB-003; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:111`). "Business hours" count only 8 a.m.–8 p.m. Arizona time (D-004).

| Status | Waiting for | Timeout | Then | Requirement |
|---|---|---|---|---|
| Intake | Answers, the "I'm safe" reply after an emergency (consent is recorded with the first text, so nothing waits for it, D-419) | Reminder at 24 h, close at 72 h; a returning web-chat visitor sees the request closed | Closed early, reason "no reply", with the INT-019 closing text (D-095, D-419) | INT-019, INT-005, CON-025, CON-027 |
| Open | The first quote | 6 p.m. Arizona time the next day | No quotes yet, with the "no quotes yet" text | JOB-009, JOB-010 |
| No quotes yet | A quote | 5 days | Closed | JOB-011 |
| Quotes ready | A pick | 3 days, restarted on every entry; at once if no choosable quote is left and every job card's 24-hour quote window has ended (D-140) | Closed | JOB-012, JOB-025 |
| Awaiting confirm | The business's Confirm (the portal review step, one tap from the pick email; CONFIRM by text only for a business that chose text, PICK-033) | 4 business hours from when the pick notice (email and any chosen text together) is sent (D-148, D-310, D-345), plus 1 business hour once when all its emails bounced or failed (D-332, D-345); 60 clock minutes on an emergency job (PICK-044); a second "<n> YES" replaces the pick (PICK-032) | Quotes ready | JOB-013, PICK-019 |
| Payment pending | A working card and Stripe's answer | 1 business hour after a decline or "needs card" (FEE-020, FEE-036); Stripe asked directly at 30 min (FEE-019); pick released if Stripe is unreachable for 4 h (FEE-018); these timers stop while charges are paused (FEE-038) | Quotes ready | JOB-014 |
| Connected | First call; DONE, NO or PROBLEM at day 3 or day 14 | Call contact: no call in 72 h → Quotes ready; email contact: a day-3 NO → Quotes ready (D-143); unreachable refund decided at 96 h → Closed (FEE-022, JOB-027); no reply 7 days after the day-14 check-in → Closed | Quotes ready, Done or Closed | JOB-015, JOB-026, JOB-027 |
| Done, Closed, Closed early, Not covered, Cancelled | Nothing (final) | — | — | JOB-001; `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:160` |

Timers that are not job statuses but still bound every wait: each job card's 24-hour quote window (PICK-001); quote-count updates at most every 3 hours (TXT-017); pick code 10 minutes, 5 tries, 15-minute cooldown (PICK-013); sign-in links 15 minutes (SEC-004); masked line closes at 30 days or earlier (CALL-009); text retries 3 times over 30 minutes then email (TXT-014); quote-page tokens die 30 days after close (PICK-011); a failed refund retries hourly (FEE-026); the "we have your message" AI retry every 30 minutes (INT-021); safe mode 24 hours after an unacknowledged High alert, pausing charges and releases only (OPS-001, D-133), with self-cleared delivery alerts acknowledged automatically (OPS-023); a job paused by a sole-word CANCEL keeps its status and timers (CON-029).

**Pick status** (owned by `picks`, kept beside the job status from the pick onward, including while the job is Connected, where paid, free and no-show still apply): awaiting confirm, needs card, charging, auth needed, payment failed, paid, free, expired, cancelled, no-show (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:93`). Release happens only on paid or free (PICK-022).

## 7. Adapters and stand-ins

One trait per vendor in `adapters`, implemented by an HTTP client (reqwest with rustls) whose base URL points at the stand-in server, the vendor's sandbox or the live API; the stand-ins in `stand_ins/` are served over HTTP (SEC-032) (`BUILD_BRIEF.md:158`–`BUILD_BRIEF.md:165`). The setting `ADAPTER_MODE_<VENDOR>` picks `stand_in`, `sandbox` or `live`; `live` is accepted only when `APP_ENV=live`, and `stand_in` never in live (SEC-002). Every inbound webhook is signature-checked in the adapter before any module sees it, and recorded by event ID in the owning module's `webhook_event` table (SEC-016, SEC-017).

| Adapter | Outbound calls | Inbound (owner module) | Phase 1 stand-in | Phase 2 |
|---|---|---|---|---|
| `sms` (Twilio Messaging) | Send text with template ID and language, from the homeowner campaign number, or from the separate business number only while business texting is on (TXT-035) | Incoming text, delivery status (`messaging`) | Fake Twilio from the twin | Twilio test credentials and magic numbers (`BUILD_BRIEF.md:190`) |
| `voice` (standard programmable voice; Twilio Programmable Voice at launch) | Open/close call session (incoming-call webhook, then dial out with the pool number as caller ID), Verify code calls only to a homeowner who taps "Call me with the code instead" (PICK-013, SEC-008) and never to a business (BIZ-046, D-366), the homeowner safety call behind its attorney switch and the CALL-022 conditions, off at launch (CALL-019, CALL-022, D-384), and Casey's critical-alert call (OPS-035); never an automated call to a business phone (BIZ-039, D-344); never Twilio Proxy, which is a Public Beta with no SLA (CALL-021, D-296) | Call routing, call finished (`calls`) | Fake Twilio | Twilio sandbox |
| `verify` (Twilio Verify) | Send and check a code by SMS or email (voice only for a homeowner's tap, D-366) | — | Fake Twilio | Twilio sandbox |
| `payments` (Stripe) | Customer, optional SetupIntent, PaymentIntent confirmed on-session from the Confirm page (never off-session, D-365), PaymentIntent fetch, refund, event list | Payment, refund, dispute events (`payments`) | `stripe/stripe-mock` plus the twin's fake Stripe for webhooks and timing | Stripe test mode |
| `ai` (Claude API) | Read request, read quote, screen photo (scrubbed text only; photos only after metadata is stripped, PRIV-014); summarize a quote only while AI quote summaries are on (PICK-003, D-373) | — | Deterministic stand-in AI | Real AI from the Railway test project, fake data only (D-040) |
| `email` (Resend) | Send email from the askhava.com sending subdomain with SPF, DKIM and DMARC (SEC-025) | Bounce, complaint (`messaging`; a hard bounce or complaint marks a business address unverified, BIZ-040) | Fake Resend | Resend test |
| `letters` (Lob) | Send identity letter | Letter status (`messaging`) | Fake Lob | Lob test |
| `license_sources` | Fetch agency daily lists; one throttled live lookup | — | Fixture lists | Real public lists (verification only, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:250`) |
| `storage` (object storage) | Write off-database stream and nightly encrypted export | — | Local directory | Bucket chosen at the account checkpoint (D-094) |

Each stand-in has contract tests written from the vendor's published API documentation, run again against the real sandbox in Phase 2 (`BUILD_BRIEF.md:165`, `BUILD_BRIEF.md:191`).

**What K8 built (D-121 to D-126).** `stand_ins/` holds Python stand-ins, standard library only, with the vendors' response shapes, error envelopes, idempotency-key behavior and webhook signatures: `twilio.py` (Messages with magic numbers, opt-out words and segment counting, plus Verify), `stripe.py` (a stateful fake with stripe-mock's resource shapes, declines by test payment method, refund limits, events and signed webhooks), `resend.py` (Svix signatures), `lob.py`, and `ai.py` (keyword rules, `ScrubbedText` only, no tools, output validated against the fixed intake fields). The contract tests are in `tests/contract/`; in Phase 2 the same cases run against each vendor's sandbox. Twilio Voice comes with the adapter tasks; the HTTP serving layer (`stand_ins/server.py`, so the Rust app talks to them over a socket) is T167, ahead of the adapters (D-453), and `stripe/stripe-mock` itself runs in CI.

**The runner's logic** is `runner/`: `state.py` (the real `kit/state.json` shape: list of tasks, one top-level lease; claims, eligibility, backoff), `verify.py` (path check, model check from session records, the DONE check), `outbox.py` (outbox files, the account checkpoint and other owner gates, Gate P) and `cycle.py` (one cycle, with git, GitHub and the worker session behind the `Env` protocol; inbox files are acknowledged only after the state recording them is pushed). The scheduled session supplies the real `Env`; `tests/test_failure_drills.py` supplies a fake one.

## 8. Contact vault data flow

**What is in the vault** (`privacy.contact` and friends): homeowner names, phone numbers, emails, street addresses, on-site contacts, original photos, and the raw text of homeowner messages (texts and web-chat lines, both ways) (PRIV-001; D-093). Every other table refers to them by ID. Business text is not homeowner data: a business's quote stays in `quotes.quote` as its original wording (PICK-003), checked there for contact details (PICK-007).

**Protection:** each field is encrypted with AES-256-GCM under `VAULT_KEY`, a variable separate from the database credentials (PRIV-002); the associated data binds each ciphertext to its table, column and row, so a ciphertext copied to another row fails to decrypt; each ciphertext names its key version so the key can be rotated. Phone lookup uses a blind index, HMAC-SHA-256 of the normalized number under `VAULT_INDEX_KEY`, so the database can find a number without holding it in clear. Every plaintext read writes an audit row naming the reader, the purpose and the record (PRIV-003). Photos are encrypted the same way, under `VAULT_KEY` with their job and photo ID as associated data, before the `storage` adapter writes them, and are served only by the app's photo route (PRIV-023, PRIV-014, D-454). An outside key service (envelope encryption under AWS or Google Cloud KMS) is an option for the account checkpoint, not part of the build; the key-version field lets it be added later (D-454).

**Who may read plaintext** (D-093, from PRIV-004), each read through `privacy.api` and audit-logged:

- `messaging`: to send a text, email or letter, and to hand a raw inbound homeowner message to the inbound reader;
- `picks`: the release, and the only path by which a business sees released details (the business portal asks `picks.api`, never `privacy`);
- `calls`: to bridge a masked call; the number goes straight to the voice adapter;
- `portals`, homeowner realm: a signed-in homeowner's own record (PRIV-021);
- `portals`, owner realm: the contact on a call-back alert only (INT-023, CON-016, TXT-015, TXT-023). Alerts in `ops.alert` carry the contact ID, never the number (PRIV-006); Casey's view reads the number at the moment he opens the alert;
- `privacy` itself: the inbound reader, scrubbing, purges.

Nobody else.

**Flow of one request:**

1. **Inbound.** Twilio's webhook (signature checked) reaches `messaging`, which dedupes by message ID (TXT-012) and looks the sender up in its sender directory.
   - **From a business phone:** the text is business text. It is stored in `messaging.inbox`. For a business that chose text it is passed on (as an event with its ID) to `intake`, which routes it to `quotes` or `picks`; for a business on email it is never routed as a quote, command or homeowner request and is answered only as TXT-037 says (D-303).
   - **From anyone else (a homeowner):** `messaging` hands the raw text, with the keyword lists it reads from `catalog`, to `privacy.api.read_inbound()`. This is the only code that sees raw homeowner text. It stores the text in `privacy.message_body`; stores contact details the person gave ("my name is …", "my email is …") in the contact record (INT-012); matches the emergency keyword stems (INT-002) and the sole-word opt-out, HELP and START words (CON-008, CON-018) on the raw text; and returns the flags plus `ScrubbedText` (names after "this is / my name is", street-number patterns, 7+ digit runs, emails, and the homeowner's vault name removed; AI-002, AI-003). Short commands ("2 YES", "CLOSE 1042", "1", "ok") survive the scrub.
   - **Order** (`messaging`, then `intake`): dedupe; emergency check; then opt-out, HELP and START. An emergency match from an opted-out number still raises the critical alert to Casey and sends no text (CON-017, CON-013). Then `intake` gets an event with the message ID, the flags and the `ScrubbedText` (stored in `messaging.inbox`) and runs the safety flow, consent and routing. Web chat follows the same path from `site`.
2. **Intake** never sees raw homeowner text; it works from the flags and `ScrubbedText`. `businesses` keeps the sender directory current through `messaging.api` whenever a main or staff phone is added or removed.
3. **AI.** `ai` accepts only `ScrubbedText`, a type only `privacy` can build (AI-004); it returns schema-checked fields or nothing (AI-005). No AI call happens in live until zero retention is signed or the owner's "AI on scrubbed text" switch is on after the provider's terms are recorded (AI-008, D-412); either way photos never go to the AI before zero retention (CARD-013).
4. **Card.** `quotes` builds the card from the scrubbed description and fixed fields and runs the final plain-code gate on every card (CARD-007, CARD-008). Cards carry town and kind of place only (CARD-006).
5. **Pick and release.** `picks` asks `payments` whether the pick is paid (or free); only then does it read first name, address, photos and, when the homeowner chose email contact, the email they chose to share (PICK-018), and hand them, through `picks.api`, to the signed-in business portal only (PICK-022, PRIV-005); `messaging` sends a release email that carries no homeowner detail, only the job number and a sign-in link (PICK-024, SEC-034, D-454), and never a text (TXT-036, D-306). The homeowner's phone number is never released; `calls` reads it only to connect a masked call (PICK-023, CALL-002). The owner portal's release power only re-sends such a release (D-074).
6. **Purge.** A daily `privacy` job deletes name, address, email and photos 180 days after close unless a dispute or complaint hold exists; keeps the phone, encrypted, only with its consent and opt-out records for 5 years after the last message; deletes photos 30 days after close; and after 30 days cuts every message body (TXT-020, D-079): an outbound text keeps only its template ID; an inbound text, which has no template ID, keeps only its kind (for example "inbound homeowner text" or "inbound business text"), its time, its sender reference and its job ID, and its raw text in `privacy.message_body` and its `ScrubbedText` copy in `messaging.inbox` are both deleted. The job's scrubbed description in `jobs.job` is kept 3 years (PRIV-008), and a business's quote wording stays in `quotes.quote` (PICK-003) (PRIV-007, PRIV-009).

## 9. Environments and deployment shape

| Environment | Where | Data | Adapters | Keys |
|---|---|---|---|---|
| Build | Cloud session VM, local PostgreSQL 16 | Fake only | `stand_in` | None in Phase 1; sandbox network secrets in Phase 2 (`BUILD_BRIEF.md:296`) |
| CI | GitHub Actions, Postgres service container | Fake only | `stand_in` | None (`BUILD_BRIEF.md:517`) |
| Test | Railway **test** project | Fake only | `sandbox` | Test keys in test-project variables |
| Live | Railway **live** project | Real | `live` | Live keys entered by Casey at Gate C |

(`BUILD_BRIEF.md:512`–`BUILD_BRIEF.md:521`.) "Railway" means the host chosen per the changes file (OPS-012, D-024).

Each Railway project has three services: `web`, `worker` and PostgreSQL 16, plus its own phone numbers, webhook endpoints, `VAULT_KEY`, `VAULT_INDEX_KEY`, `ID_SECRET`, storage bucket and vendor keys; nothing is shared between test and live (SEC-003). The test project auto-deploys from `main`; the live project has auto-deploy off and deploys only when Casey presses Deploy (OPS-022; `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:305`). Claude never holds a hosting key. Migrations run as a separate release step with the migration role before the new `web` and `worker` start. The live project serves askhava.com through Cloudflare DNS; the test project uses only its own host-provided domain.

## 10. Off-database records and recovery

Written as they happen to the `storage` adapter (OPS-010, OPS-011; `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:690`): the audit stream, the web request log (request IDs, routes with tokens redacted, status codes) and the AI structured readings. With Twilio's and Stripe's own records, they are the replay inputs after a restore (OPS-015 to OPS-019). The nightly encrypted export goes to the same bucket under a separate key, kept 30 days (OPS-013). Point-in-time recovery comes from the host (OPS-012).

## 11. Pinned stack versions

**Policy (D-096, D-453):** the newest stable release published on or before October 14, 2026 (14 days before this plan change), unless cargo-audit or npm audit reports a known vulnerability fixed only in a later release, in which case the fixed release. Versions, dates and licenses were read from the crates.io and npm registry APIs and the Rust release channel in this session on October 28, 2026. T009 writes them into the workspace `Cargo.toml` as exact (`=`) versions and commits `Cargo.lock`; T020 writes `package.json` and commits `package-lock.json` (`BUILD_BRIEF.md:114`). The Python versions this section listed before D-453 now apply only to the tooling, stand-ins and twin (pytest 9.1.1 and PyYAML 6.0.1 in `Ci`).

**Toolchain and platform**

| Component | Version | How verified | Note |
|---|---|---|---|
| Rust | 1.99.0 (2026-10-01), `rust-toolchain.toml` with clippy and rustfmt | Rust release channel; installed and used in this session | 1.99.1 (2026-10-15) is past the cutoff; T009 reads its notes and takes it only for a security fix |
| Edition | 2024 | the scratch workspace built with it | |
| PostgreSQL | 16 (16.15 in the build VM) | `psql --version` | Railway's minor version and the CI service image digest: **verify at T009** |
| Node | 22 (22.23.4 in the build VM) | `node --version` | Only for `tsc`, eslint and Playwright; never in the production image |

**Runtime crates** (each MIT, Apache-2.0, BSD-3-Clause, Unlicense or MPL-2.0; `deny.toml` allows those plus the permissive licenses their dependencies use: MIT-0, ISC, Zlib, Unicode-3.0 and CDLA-Permissive-2.0, and skips the unpublished first-party crates)

| Crate | Version (date) | License | Use |
|---|---|---|---|
| axum (features `multipart`) | 0.8.9 (2026-04-14) | MIT | HTTP server and routing |
| axum-extra (`cookie`) | 0.12.6 (2026-04-14) | MIT | Signed `__Host-` cookies |
| tokio | 1.53.3 (2026-10-13) | MIT | Async runtime; 1.54.1 is past the cutoff |
| tower / tower-http | 0.5.3 / 0.7.1 | MIT | Middleware: headers, body limits, static files, tracing |
| askama | 0.16.1 (2026-09-04) | MIT OR Apache-2.0 | Compile-time HTML templates, autoescaped |
| serde / serde_json | 1.0.229 / 1.0.154 | MIT OR Apache-2.0 | Parsing with `deny_unknown_fields` |
| serde-saphyr | 2.0.0 (2026-10-13) | MIT OR Apache-2.0 | Reading the catalog YAML (`serde_yaml` is unmaintained) |
| sqlx (`postgres`, `runtime-tokio`, `tls-rustls`, `macros`, `migrate`, `uuid`, `json`) | 0.9.0 (2026-05-21) | MIT OR Apache-2.0 | Postgres pool, compile-checked queries, migrations |
| reqwest (`rustls`, `json`, `form`) | 0.13.5 (2026-09-08) | MIT OR Apache-2.0 | Every vendor API client |
| hmac / sha1 / sha2 | 0.13.0 / 0.11.0 / 0.11.1 | MIT OR Apache-2.0 | Webhook signatures (Twilio HMAC-SHA1, Stripe and Svix HMAC-SHA256), derived IDs, blind index |
| aes-gcm | 0.11.1 (2026-08-21) | Apache-2.0 OR MIT | Vault field encryption (PRIV-002) |
| argon2 | 0.6.0 (2026-08-27) | MIT OR Apache-2.0 | Owner password hashing |
| webauthn-rs | 0.5.5 (2026-04-30) | MPL-2.0 | Owner passkeys (D-098); brings in OpenSSL, so the image carries `libssl3` |
| totp-rs | 6.0.0 (2026-08-06) | MIT | Owner TOTP second factor (D-098) |
| rand | 0.10.3 (2026-09-20) | MIT OR Apache-2.0 | Codes and tokens from the OS random source |
| subtle | 2.6.1 (2024-06-24) | BSD-3-Clause | Constant-time comparison of codes and tokens |
| secrecy / zeroize | 0.10.3 / 1.9.1 | Apache-2.0 OR MIT | Secrets kept out of `Debug` and zeroed on drop |
| uuid | 1.28.0 (2026-10-10) | Apache-2.0 OR MIT | Row IDs; 1.29.0 is past the cutoff |
| jiff (`tzdb-bundle-always`) | 0.2.38 (2026-10-06) | Unlicense OR MIT | Arizona time and area-code time zones for quiet hours, with the zone data built in |
| image (`jpeg`, `png`, `webp`) | 0.25.10 (2026-03-10) | MIT OR Apache-2.0 | Photo re-encoding with metadata removed (PRIV-014). It cannot decode HEIC, which TD-PRIV-08 needs: T090 picks a HEIC decoder under the new-library rule (logged with its license and audit result) |
| tracing / tracing-subscriber (`json`, `env-filter`) | 0.1.44 / 0.3.23 | MIT | Structured logs with redaction |
| sentry (`rustls`, `reqwest`, `tracing`, `panic`, `backtrace`) | 0.49.3 (2026-09-21) | MIT | Error reports with PII off |
| thiserror / base64 / automod | 2.0.21 / 0.23.1 / 1.0.17 | MIT OR Apache-2.0 | Error types; encoding; per-crate file declarations |

**Development and checks**

| Component | Version (date) | License | Note |
|---|---|---|---|
| proptest | 1.12.0 (2026-10-10) | MIT OR Apache-2.0 | Property tests |
| trybuild | 1.0.122 (2026-10-07) | MIT OR Apache-2.0 | Compile-fail proofs (SEC-030, SEC-031) |
| cargo-mutants | 27.1.0 (2026-06-02) | MIT | Nightly mutation tests |
| cargo-audit | 0.22.2 (2026-06-05) | Apache-2.0 OR MIT | RustSec advisories on every PR |
| cargo-deny | 0.20.2 (2026-07-09) | MIT OR Apache-2.0 | Licenses, bans, sources |
| cargo-llvm-cov | 0.9.1 (2026-09-06) | Apache-2.0 OR MIT | Coverage; branch mode on a pinned nightly used only to measure |
| sqlx-cli | 0.9.0 (2026-05-21) | MIT OR Apache-2.0 | `cargo sqlx prepare` for the committed offline query data |
| typescript | 7.0.2 (2026-07-08) | Apache-2.0 | `tsc --strict` for the browser extras |
| @playwright/test | 1.64.1 (2026-10-14) | Apache-2.0 | e2e and accessibility journeys |
| @axe-core/playwright | 4.13.0 (2026-08-11) | MPL-2.0 | axe in Playwright; 4.14.0 is past the cutoff |
| eslint / typescript-eslint | 10.12.0 / 8.71.2 | MIT | TypeScript lint |
| gitleaks | 8.28.0 (pinned with its SHA-256 in `ci.yml`) | MIT | Secret scan |
| stripe/stripe-mock image, OWASP ZAP image, Docker base image | **verify at T009 / T019** | — | Pinned by digest (SEC-021); the base is a Debian slim image with `libssl3` and CA certificates |

**Resolution and audit run in this session:** a scratch workspace with every crate above at its exact version, on Rust 1.99.0, resolved 401 crates and built; `cargo clippy --workspace --all-targets -- -D warnings` passed. `cargo audit` (0.22.2, 1,381 advisories) found no vulnerabilities and one unmaintained warning: RUSTSEC-2026-0374, `basic-toml`, used only at compile time by `askama_derive` to read Askama's config. `cargo deny check licenses bans sources advisories` (0.20.2) passed with the license list above, crates.io as the only source, wildcard versions denied except workspace paths, and that one advisory ignored with its reason; duplicate versions are warnings only. T009 writes this `deny.toml` and re-runs both; `Ci` runs `cargo audit` (fails on any vulnerability) and `cargo deny` on every PR. The scratch workspace is not committed. npm packages are audited when T020 writes `package-lock.json`.

No Web Push crate is pinned: the installable business app with push is a later, optional phase (BIZ-041, D-307), and the spec change that adds it picks the crate under the policy above and logs its purpose, license and audit result.

## 12. Keeping this file true

- A new module, table, cross-module call, status, timeout, adapter or pinned version changes this file in the same PR, with a `docs/decisions.md` entry when it is a choice (`BUILD_BRIEF.md:489`).
- **Build order and file ownership** live in `kit/tasks.json` (K6): each build task names the module and the files it owns (`app/<module>/src/…`, its own `migrations/0NNN_<slug>.sql`, its own test files), and `python3 scripts/validate_tasks.py` fails if two tasks that can run in either order own the same file, or if a non-`harness` task owns a protected path (D-111, D-118). The shared files are this file, `docs/decisions.md`, `docs/for-casey-review.md`, `app/server/src/main.rs`, the workspace `Cargo.toml`, `Cargo.lock`, `.sqlx/`, `package.json`, `package-lock.json` and every crate's `Cargo.toml` (D-453): a crate can only list a path dependency once that crate exists, so the task that first needs `picks` to call `payments`, or the server or a test crate to use a new module, adds that line. A new outside crate still needs its decision entry and must be in the workspace table. The first task of each module creates its crate's `Cargo.toml` and `src/lib.rs`. Stage 2 builds layers 0 to 7 far enough for job cards, the quote page and the pick (including the license imports and business records that matching reads); later stages add to each module's own files in dependency order (D-112).
- A new library needs a decisions entry with purpose, license and a cargo-audit (or npm audit) result, and must pass cargo-deny (`BUILD_BRIEF.md:122`).
- The threat model (`docs/threat-model.md`) changes with any new trust boundary or asset.
- The not-a-contractor phrase list (WORD-022) lives in one place, `NOT_A_CONTRACTOR` in `scripts/trace_spec.py`; the spec check, the catalog check (`scripts/check_catalog.py`) and the app's wording scan (T104) all read it (D-360). It also holds the WORD-023 banned descriptions ("connect you with", "referral network/service", D-377).
- The legal document drafts live in `docs/legal/` (TERMS-028, D-389). `scripts/check_legal_docs.py` keeps them true to the spec: each key phrase it lists must appear word for word in its spec requirement and in its draft, and it reuses the banned-phrase list from `scripts/trace_spec.py`; `tests/test_check_legal_docs.py` runs it in `Ci`. When the site pages are built (T111, T112), the published terms and policy come from these drafts.
- Legal holds live in the catalog and its check: pest control, when on, is for Department of Agriculture licensees only and never advertised (LIC-012, D-402); sale, purchase and refinance inspections are fee-free job types (FEE-045, D-403); and no category is a warranty or protection plan (`scripts/check_catalog.py`, LIC-015, LIC-042, D-376).
- Fix round 13 adds three catalog files, all checked by `scripts/check_catalog.py`: `docs/catalog/license_map.yaml` (which license classes receive which job types, LIC-043, D-392), `docs/catalog/question_bank.yaml` (the intake question sets and trade safety prompts, LIC-040, INT-042, D-394) and, written by T154, `docs/catalog/intake_golden.yaml` (golden sample requests, OPS-040, D-395). `catalog` loads them; `licensing.class_map` (T153) decides coverage from the map; `intake.question_bank`, `intake.routing_confidence` and `intake.summary_step` (T156) ask the questions and confirm before cards go; `businesses.capability` (T155) pre-ticks the checklist; `picks.backup_choice` and `picks.auto_confirm` (T157) handle the backup and the optional auto-confirm charge; `ops.scorecard`, `ops.intake_report` and `payments.money_view` (T158) are owner pages; `portals.push` (T160) is the optional push channel. Each category has a `launch_open` flag (LIC-046, D-396).
- The voice guide for texts and emails is `docs/voice.md` (WORD-024, D-406); the before-build list is `docs/PREBUILD.md` (D-416).
- Fix round 14 (D-429 to D-449): `docs/catalog/trade_kb/` holds the Grok trade knowledge base as the expert brief for intake (read by `catalog`, never deciding who gets a card); `categories.yaml` names each category's `quote_format` and `design` flag (PICK-053, D-433, D-441); `ai.writer` and `ai.output_checks` (T163) write and check the AI-reworded questions, the job card and the project file inside AI-019, falling back to the fixed templates; `quotes.job_page` and `quotes.update_texts` (T163) are the business job page and the short portal-link texts; `intake.order`, `intake.photos` and `intake.design` (T162); `quotes.formats`, `quotes.notes`, `quotes.edit_withdraw`, `picks.share_consent` and `picks.cant_take` (T164); `businesses.links`, `businesses.leaving`, `businesses.pest_identity` and `ops.coverage_email` (T165). The concept-image provider is a new adapter chosen at the account checkpoint, receiving only the written project file; until then its switch is off. The safety-contacts file is withdrawn (D-436).
- Fix round 15 (D-455 to D-467): no new module. A `g` source code on askhava.com links is recorded by `businesses` at sign-up with the optional "Where did you hear about Ask Hava?" answer (BIZ-053, T081) and by the web chat with the request (INT-047, T103), never reaching `catalog` routing or a card; `ops.scorecard` (T158) adds the D-458 measures; `jobs` timers (T032) send the TXT-042 all-quotes-in text and the JOB-012 closing note; `businesses.leaving` (T165) asks the BIZ-054 reason; the home-page numbers (WEB-028, T166) are computed from the ledger and audit log; the contact card and after-Done line (PICK-054) belong to `picks` (T055). Every enabled category is launch-open (D-459).
- Design (D-451): `docs/design/DESIGN_SPEC.md` sets the look for `site`, `portals` and emails; `site` (T166) serves the self-hosted fonts and the plaster, brass and paper textures from `app/site/static/`, and `tests/a11y/design_tokens.spec.ts` checks every color pair against the spec's contrast values.
- AI smell (D-452): `docs/ai_smell.yaml` lists the words, patterns and page habits that read as machine-made; `scripts/check_ai_smell.py` scans the spec's quoted copy, the voice guide, the question bank, the design reference and everything customer-facing under `app/` (templates, message files, stylesheets), and `tests/test_check_ai_smell.py` runs it on every push, so a hit fails `Ci`. Every task prompt adds a cold read of changed copy (WORD-028).
- Code documentation (D-473): `docs/code-docs.md` is the standard a session that has never seen the code relies on. Every crate keeps a `CLAUDE.md` (purpose, what it owns, rules with requirement IDs, never-break invariants with their tests, change recipes, tests) as a shared path; every Rust file opens with a `//!` comment; every public item has a `///` comment (the workspace lints deny `missing_docs`, T009); every finished build task names its requirements in its own files. `scripts/check_code_docs.py` checks this and `tests/test_check_code_docs.py` runs it in `Ci`. `docs/README.md` (T141) is the start-here map, and T171's cold-start drill tests the docs with a fresh session before "Finished".
- Copy quality (D-474): `docs/copy/copy-panel.md` is the copy panel (eight writers' methods, the ten-test scorecard and the reviewer persona every task's cold read runs); `docs/copy/site-copy.md` holds the approved site wording, block by block; `docs/copy/voice-of-customer.md` holds real customers' phrases; `scripts/check_copy.py` checks sentence length, reading grade and superlatives in the site copy and the page templates, and `tests/test_check_copy.py` runs it in `Ci` (WORD-029, T161).
