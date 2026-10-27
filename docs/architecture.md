# Ask Hava architecture

Written in kit step K4 (`BUILD_BRIEF.md:598`). This file finalizes the module boundaries that `BUILD_BRIEF.md:124`–`BUILD_BRIEF.md:152` fixes, and pins the stack versions (`BUILD_BRIEF.md:121`). It restates and arranges rules; where it had to choose, the choice is logged in `docs/decisions.md` (D-090 to D-100) and named here. If this file and a cited source disagree, the source wins and this file is the bug. Every PR that changes a boundary, table owner, state, timeout or pinned version updates this file in the same PR (`BUILD_BRIEF.md:489`).

Citations are `path:LINE`. "Changes file" means `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md`; "v9.3" means `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md`. Requirement IDs (for example `PRIV-005`) are in `docs/spec/`.

---

## 1. Shape at a glance

One web app and one PostgreSQL database do all the work; every outside service sits behind an adapter (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:44`; `BUILD_BRIEF.md:146`).

```
 Homeowner (text, web chat,      Business (text,          Casey (owner portal,
 quote page, homeowner portal)   business portal)         alerts)
            \                         |                        /
             +------------- web process (FastAPI + Jinja2) ---+
                                      |
              worker process (scheduler + task runner, same code)
                                      |
                PostgreSQL 16: one schema per module, contact vault encrypted
                                      |
                         adapters (real | sandbox | stand-in)
   Twilio texts | Twilio calls | Twilio Verify | Stripe | Claude API (zero retention)
   Resend email | Lob letters | license sources | object storage
   Kept outside the database: Twilio and Stripe records, web request log,
   audit stream, AI structured readings (OPS-011)
```

## 2. Processes and code layout

| Process | Runs | Notes |
|---|---|---|
| `web` | `uvicorn` serving the FastAPI app: pages, portals, web chat, webhooks, `/healthz` | Stateless; any number of replicas; server-rendered, works without JS (`BUILD_BRIEF.md:106`) |
| `worker` | The scheduler and the task runner: polls `core.task` with `SELECT … FOR UPDATE SKIP LOCKED` | Timers, outbox sends, retries, nightly jobs (`BUILD_BRIEF.md:108`; JOB-006). One image, started with a different command |

Both processes run the same Docker image (`BUILD_BRIEF.md:119`), pinned by digest (SEC-021).

```
app/
  main.py            composition root: builds the FastAPI app, registers routes and task handlers
  worker.py          worker entry point
  core/              shared kernel (section 4); not a business module
  <module>/          one package per module in section 3
    api.py           the module's public functions and pydantic types; the only thing other modules import
    models.py        SQLAlchemy tables, all in the module's own Postgres schema
    rules.py         pure decision logic (no I/O), the main target of unit and property tests
    service.py       orchestration: transactions, calls to lower modules' api.py, audit rows
    handlers.py      task handlers registered in main.py (timers and events)
    routes.py        HTTP routes, only in modules that own a web surface (portals, site, webhooks)
migrations/          Alembic, forward-only, each migration tested (BUILD_BRIEF.md:107)
stand_ins/           fake Twilio, fake Resend, fake Lob, stand-in AI, Stripe mock config (BUILD_BRIEF.md:91)
tests/               one folder per test layer (docs/test-design.md section 2)
twin/                the twin and its hard-failing gate (docs/twin-warnings.md section 6)
```

## 3. Modules, what each owns, and its tables

Each module owns its files and its tables, and no module reaches into another's tables (`BUILD_BRIEF.md:126`). Each module's tables live in a Postgres schema named after it (D-091). "Calls" lists the lower modules whose `api.py` it may import; `core` is always allowed and is not repeated.

| Module | Owns (`BUILD_BRIEF.md:128`–`BUILD_BRIEF.md:146`) | Tables (schema-qualified) | Calls |
|---|---|---|---|
| `ops` | Alerts, pause switches, safe mode, status banner, web-and-email mode, audit log | `ops.audit_log` (append-only), `ops.alert`, `ops.alert_ack`, `ops.pause_switch`, `ops.switch_history`, `ops.safe_mode`, `ops.status_banner` | — |
| `adapters` | One interface per vendor; real, sandbox and stand-in implementations chosen by configuration | none | — |
| `catalog` | Categories, job types, license rules, fee rules, feature switches (fee mode, category on/off) and numeric settings | `catalog.category`, `catalog.job_type`, `catalog.license_rule`, `catalog.fee_rule`, `catalog.setting`, `catalog.setting_history` | `ops` |
| `privacy` | The plain-code scrubber; the contact vault (field-level encrypted); the inbound reader that is the only code to see raw homeowner text (section 8); retention and purge jobs; photos | `privacy.contact`, `privacy.contact_index` (blind index), `privacy.message_body` (homeowner message text only), `privacy.photo`, `privacy.retention_hold`, `privacy.purge_run` | `ops` |
| `ai` | Request reading and quote summarizing through the AI adapter; strict schemas; accepts only `ScrubbedText` | `ai.reading` (fields only, no personal details), `ai.call_stat` | `adapters`, `privacy` (the `ScrubbedText` type only), `ops` |
| `messaging` | Outbox and inbox (never send twice); templates in English and Spanish; quiet hours; opt-out keywords and the opt-out list; the sender directory (which numbers are business phones); delivery tracking; email and letters | `messaging.outbox`, `messaging.inbox` (scrubbed homeowner text and flags; business text as sent), `messaging.sender_directory` (blind-indexed business and staff phones), `messaging.delivery_event`, `messaging.opt_out`, `messaging.opt_out_event`, `messaging.email_outbox`, `messaging.letter`, `messaging.webhook_event` | `adapters`, `privacy`, `catalog`, `ops` |
| `payments` | Stripe customers, SetupIntents, card authorization, charges, refunds, disputes, free first wins, the ledger, nightly reconcile | `payments.customer`, `payments.payment_method`, `payments.card_authorization`, `payments.charge_attempt`, `payments.ledger_entry` (append-only), `payments.refund`, `payments.dispute`, `payments.free_win`, `payments.webhook_event`, `payments.reconcile_run` | `adapters`, `catalog`, `ops` |
| `licensing` | Agency daily lists; live lookup at pick; other agencies; the city license list; handyman rules | `licensing.list_import`, `licensing.agency_record`, `licensing.city_license_entry`, `licensing.business_license`, `licensing.license_check` | `adapters`, `catalog`, `ops` |
| `businesses` | Sign-up, identity chain, job-type checklist, towns, tags, business consent records, pause rules, staff phones, complaints | `businesses.business`, `businesses.town`, `businesses.job_type_choice`, `businesses.tag`, `businesses.staff_phone`, `businesses.consent_record`, `businesses.identity_attempt`, `businesses.signup_review`, `businesses.pause_event`, `businesses.complaint` | `licensing`, `payments`, `messaging`, `catalog`, `adapters`, `ops` |
| `jobs` | The job lifecycle state machine (section 6); job numbers; status history; quote-page token hashes | `jobs.job`, `jobs.status_history`, `jobs.job_number` | `messaging`, `catalog`, `ops` |
| `calls` | Masked numbers from a pool, PIN routing, call sessions, call logs (never recorded) | `calls.number_pool`, `calls.call_session`, `calls.call_log`, `calls.webhook_event` | `businesses`, `privacy`, `messaging`, `adapters`, `ops` |
| `quotes` | Matching and job cards; quote intake and parsing; eligibility rules; arrival order; original wording beside any summary | `quotes.job_card`, `quotes.quote`, `quotes.quote_bounce` | `jobs`, `businesses`, `licensing`, `catalog`, `ai`, `privacy`, `messaging`, `ops` |
| `picks` | Pick by code; contact-method choice; confirm window; payment hand-off; release of contact details; losers notified | `picks.pick`, `picks.code_attempt`, `picks.release` | `quotes`, `jobs`, `payments`, `calls`, `businesses`, `licensing`, `privacy`, `messaging`, `adapters`, `ops` |
| `intake` | Inbound SMS and web chat routing (from flags and `ScrubbedText`, never raw homeowner text); conversation state; homeowner consent; the emergency safety flow and its keyword list; category matching; homeowners and properties | `intake.homeowner`, `intake.property`, `intake.conversation`, `intake.consent_record`, `intake.intake_answer`, `intake.emergency_event`, `intake.recruiting_demand` | `picks`, `quotes`, `jobs`, `businesses`, `ai`, `privacy`, `catalog`, `messaging`, `ops` |
| `recovery` | Backup checks, the restore drill, replay with derived IDs | `recovery.backup_check`, `recovery.restore_drill`, `recovery.replay_run`, `recovery.replay_item` | the `api.py` of every module in layers 0–7 |
| `portals` | Homeowner page and portal, business portal, owner portal; their sign-in, sessions and second factors | `portals.session`, `portals.login_link`, `portals.known_device`, `portals.owner_credential`, `portals.passkey`, `portals.totp_secret` (encrypted), `portals.recovery_code` (hashed) | the `api.py` of every module in layers 0–7 |
| `site` | Public website built from the prototype; the web-chat page | none (web-chat text goes to `intake`) | `intake`, `catalog` |

`core` owns `core.task` (the task queue and timers) and `core.rate_limit` (D-090).

Twelve v9.3 record types map onto these tables (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:79`–`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:96`): Contact → `privacy.contact`; Homeowner, Property → `intake`; Request → `jobs.job`; Photo → `privacy.photo`; Contractor → `businesses.business` plus `licensing.business_license` and `payments.customer`; Job card, Quote → `quotes`; Pick → `picks.pick`; Ledger entry → `payments.ledger_entry`; Message → `messaging.outbox`/`messaging.inbox`, with homeowner message text in `privacy.message_body`; Alert/audit → `ops`.

## 4. Dependency rules

1. **Layers.** A module may import only the `api.py` of a module in a lower layer, never its `models.py`, `service.py` or `rules.py` (D-091):

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

   `adapters` imports only `core` and vendor SDKs. Same-layer imports are allowed only where section 3's "Calls" column lists them (none today).
2. **Events go up, calls go down.** When a lower module must tell a higher one something (Stripe says a charge succeeded; an inbound text arrives; a license lapses; an alert is raised), it enqueues a `core.task` with an event kind and IDs only, never personal details. `app/main.py` registers which module's handler runs for each kind. No module imports a higher one.
3. **Tables.** Each SQLAlchemy model sets `schema=` to its own module. A test reads every model and every `import` statement (Python's `ast`, no extra library) and fails on any cross-schema foreign key, any import of another module's internals, and any upward import.
4. **Money** moves only through `payments` (FEE-032). **Vault plaintext** comes only from `privacy.api` and only to the readers in section 8.
5. **Invariants** (`BUILD_BRIEF.md:148`–`BUILD_BRIEF.md:152`): every outside action carries an idempotency key derived from what it identifies (SEC-018); every state change writes an audit row through `ops.api.audit()` in the same transaction (JOB-005); no homeowner contact detail leaves the vault except on a confirmed, paid pick (PRIV-005); the ledger always reconciles to Stripe (FEE-029, FEE-030).

## 5. Shared kernel (`core`)

`core` holds what every module needs and owns no business data (D-090):

- **Settings:** one pydantic-settings class reads environment variables at startup and refuses to start on a missing value, a live key in a non-live environment, or a test key in live (SEC-002; `BUILD_BRIEF.md:521`).
- **Database:** engine, session and unit of work; row locks with "only if still in the expected status" updates (JOB-004; `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:689`). Two database roles: a migration role that owns the schemas, and an app role with no `UPDATE` or `DELETE` on append-only tables (D-092).
- **Clock:** UTC storage; Arizona time (no daylight saving) and the business clock of 8:00 a.m.–8:00 p.m. every day (JOB-007, D-004); the local time in a recipient's zone, which `messaging` uses for quiet hours (D-003, D-025); tests inject the clock.
- **Webhook dedupe:** one helper that records a vendor event ID in the calling module's own `webhook_event` table and reports whether it was new, so every module dedupes the same way (SEC-017).
- **Derived IDs:** HMAC-SHA-256 under a server secret over what an ID identifies, for job cards, quotes, picks, quote-page links and Stripe idempotency keys (CARD-017, PICK-028, OPS-016).
- **Task queue:** `core.task` (kind, IDs-only payload, `run_at`, attempts, lease, unique dedupe key). Workers claim with `FOR UPDATE SKIP LOCKED`, hold a lease, and a crashed worker's lease expires so another picks the task up. Every handler first re-checks the current status and does nothing if things moved on (JOB-006).
- **Rate limits:** `core.rate_limit` counters per phone, IP and business (SEC-014, SEC-015).
- **Logging:** a request ID on every log line; no phone numbers, names or tokens in logs (PRIV-016); Sentry with `send_default_pii=False` plus a scrubber (PRIV-017).

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
| Intake | Consent, answers, the "I'm safe" reply after an emergency | With consent recorded: reminder at 24 h, close at 72 h. With consent still pending: no reminder and no closing text (only the consent text may go to that number, CON-025); close silently at 72 h, and a returning web-chat visitor just sees the request closed | Closed early, reason "no reply"; the text "Text us any time to restart" only when consent was recorded (D-095) | INT-019, INT-005, CON-025 |
| Open | The first quote | 6 p.m. Arizona time the next day | No quotes yet, with the "no quotes yet" text | JOB-009, JOB-010 |
| No quotes yet | A quote | 5 days | Closed | JOB-011 |
| Quotes ready | A pick | 3 days, restarted on every entry; at once if no choosable quote is left | Closed | JOB-012, JOB-025 |
| Awaiting confirm | The business's Confirm | 2 business hours | Quotes ready | JOB-013, PICK-019 |
| Payment pending | A working card and Stripe's answer | 1 business hour after a decline or "needs card" (FEE-020, FEE-036); Stripe asked directly at 30 min (FEE-019); pick released if Stripe is unreachable for 4 h (FEE-018) | Quotes ready | JOB-014 |
| Connected | First call; DONE, NO or PROBLEM at day 3 or day 14 | No call in 72 h → Quotes ready; refund decided at 96 h (FEE-022); no reply 7 days after the day-14 check-in → Closed | Quotes ready, Done or Closed | JOB-015, JOB-026 |
| Done, Closed, Closed early, Not covered, Cancelled | Nothing (final) | — | — | JOB-001; `sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:160` |

Timers that are not job statuses but still bound every wait: each job card's 24-hour quote window (PICK-001); quote-count updates at most every 3 hours (TXT-017); pick code 10 minutes, 5 tries, 15-minute cooldown (PICK-013); sign-in links 15 minutes (SEC-004); masked line closes at 30 days or earlier (CALL-009); text retries 3 times over 30 minutes then email (TXT-014); quote-page tokens die 30 days after close (PICK-011); a failed refund retries hourly (FEE-026); the "we have your message" AI retry every 30 minutes (INT-021); safe mode 24 hours after an unacknowledged High alert (OPS-001).

**Pick status** (owned by `picks`, kept beside the job status from the pick onward, including while the job is Connected, where paid, free and no-show still apply): awaiting confirm, needs card, charging, auth needed, payment failed, paid, free, expired, cancelled, no-show (`sources/01_spec/Havasu_Home_Quotes_Technical_Blueprint_v9.3.md:93`). Release happens only on paid or free (PICK-022).

## 7. Adapters and stand-ins

One interface (a `typing.Protocol`) per vendor in `adapters`; each has a real implementation, and a stand-in that `stand_ins/` serves (`BUILD_BRIEF.md:158`–`BUILD_BRIEF.md:165`). The setting `ADAPTER_MODE_<VENDOR>` picks `stand_in`, `sandbox` or `live`; `live` is accepted only when `APP_ENV=live`, and `stand_in` never in live (SEC-002). Every inbound webhook is signature-checked in the adapter before any module sees it, and recorded by event ID in the owning module's `webhook_event` table (SEC-016, SEC-017).

| Adapter | Outbound calls | Inbound (owner module) | Phase 1 stand-in | Phase 2 |
|---|---|---|---|---|
| `sms` (Twilio Messaging) | Send text with template ID and language | Incoming text, delivery status (`messaging`) | Fake Twilio from the twin | Twilio test credentials and magic numbers (`BUILD_BRIEF.md:190`) |
| `voice` (Twilio Voice) | Open/close call session, identity and code calls | Call routing, call finished (`calls`) | Fake Twilio | Twilio sandbox |
| `verify` (Twilio Verify) | Send and check a code | — | Fake Twilio | Twilio sandbox |
| `payments` (Stripe) | Customer, SetupIntent, off-session PaymentIntent, refund, event list | Payment, refund, dispute events (`payments`) | `stripe/stripe-mock` plus the twin's fake Stripe for webhooks and timing | Stripe test mode |
| `ai` (Claude API) | Read request, read quote, screen photo (scrubbed text only) | — | Deterministic stand-in AI | Real AI from the Railway test project, fake data only (D-040) |
| `email` (Resend) | Send email | Bounce, complaint (`messaging`) | Fake Resend | Resend test |
| `letters` (Lob) | Send identity letter | Letter status (`messaging`) | Fake Lob | Lob test |
| `license_sources` | Fetch agency daily lists; one throttled live lookup | — | Fixture lists | Real public lists (verification only, `sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:250`) |
| `storage` (object storage) | Write off-database stream and nightly encrypted export | — | Local directory | Bucket chosen at the account checkpoint (D-094) |

Each stand-in has contract tests written from the vendor's published API documentation, run again against the real sandbox in Phase 2 (`BUILD_BRIEF.md:165`, `BUILD_BRIEF.md:191`).

**What K8 built (D-121 to D-125).** `stand_ins/` holds in-process Python stand-ins, standard library only, with the vendors' response shapes, error envelopes, idempotency-key behavior and webhook signatures: `twilio.py` (Messages with magic numbers, opt-out words and segment counting, plus Verify), `stripe.py` (a stateful fake with stripe-mock's resource shapes, declines by test payment method, refund limits, events and signed webhooks), `resend.py` (Svix signatures), `lob.py`, and `ai.py` (keyword rules, `ScrubbedText` only, no tools, output validated against the fixed intake fields). The contract tests are in `tests/contract/`; in Phase 2 the same cases run against each vendor's sandbox. Twilio Voice and the HTTP serving layer (so the app talks to them over a socket, and `stripe/stripe-mock` itself in CI) come with the adapter tasks.

**The runner's logic** is `runner/`: `state.py` (leases, eligibility, backoff), `verify.py` (path check, model check from session records, the DONE check), `outbox.py` (outbox files, the account checkpoint and other owner gates, Gate P) and `cycle.py` (one cycle, with git, GitHub and the worker session behind the `Env` protocol). The scheduled session supplies the real `Env`; `tests/test_failure_drills.py` supplies a fake one.

## 8. Contact vault data flow

**What is in the vault** (`privacy.contact` and friends): homeowner names, phone numbers, emails, street addresses, on-site contacts, original photos, and the raw text of homeowner messages (texts and web-chat lines, both ways) (PRIV-001; D-093). Every other table refers to them by ID. Business text is not homeowner data: a business's quote stays in `quotes.quote` as its original wording (PICK-003), checked there for contact details (PICK-007).

**Protection:** each field is encrypted with AES-256-GCM under `VAULT_KEY`, a variable separate from the database credentials (PRIV-002); the associated data binds each ciphertext to its table, column and row, so a ciphertext copied to another row fails to decrypt; each ciphertext names its key version so the key can be rotated. Phone lookup uses a blind index, HMAC-SHA-256 of the normalized number under `VAULT_INDEX_KEY`, so the database can find a number without holding it in clear. Every plaintext read writes an audit row naming the reader, the purpose and the record (PRIV-003).

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
   - **From a business phone:** the text is business text. It is stored in `messaging.inbox` and passed on (as an event with its ID) to `intake`, which routes it to `quotes` or `picks`.
   - **From anyone else (a homeowner):** `messaging` hands the raw text, with the keyword lists it reads from `catalog`, to `privacy.api.read_inbound()`. This is the only code that sees raw homeowner text. It stores the text in `privacy.message_body`; stores contact details the person gave ("my name is …", "my email is …") in the contact record (INT-012); matches the emergency keyword stems (INT-002) and the sole-word opt-out, HELP and START words (CON-008, CON-018) on the raw text; and returns the flags plus `ScrubbedText` (names after "this is / my name is", street-number patterns, 7+ digit runs, emails, and the homeowner's vault name removed; AI-002, AI-003). Short commands ("2 YES", "CLOSE 1042", "1", "ok") survive the scrub.
   - **Order** (`messaging`, then `intake`): dedupe; emergency check; then opt-out, HELP and START. An emergency match from an opted-out number still raises the critical alert to Casey and sends no text (CON-017, CON-013). Then `intake` gets an event with the message ID, the flags and the `ScrubbedText` (stored in `messaging.inbox`) and runs the safety flow, consent and routing. Web chat follows the same path from `site`.
2. **Intake** never sees raw homeowner text; it works from the flags and `ScrubbedText`. `businesses` keeps the sender directory current through `messaging.api` whenever a main or staff phone is added or removed.
3. **AI.** `ai` accepts only `ScrubbedText`, a type only `privacy` can build (AI-004); it returns schema-checked fields or nothing (AI-005). No AI call happens in live until zero retention is signed (AI-008).
4. **Card.** `quotes` builds the card from the scrubbed description and fixed fields and runs the final plain-code gate on every card (CARD-007, CARD-008). Cards carry town and kind of place only (CARD-006).
5. **Pick and release.** `picks` asks `payments` whether the pick is paid (or free); only then does it read first name, address, photos and, when the homeowner chose email contact, the email they chose to share (PICK-018), and hand them to `messaging` for the details text and, through `picks.api`, to the business portal (PICK-022, PRIV-005). The homeowner's phone number is never released; `calls` reads it only to connect a masked call (PICK-023, CALL-002). The owner portal's release power only re-sends such a release (D-074).
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

**Policy (D-096):** the newest stable release published on or before October 13, 2026 (14 days before K4), unless pip-audit reports a known vulnerability fixed only in a later release, in which case the fixed release. Versions and dates were read from PyPI's JSON API in this session on October 27, 2026. K8 writes them into `pyproject.toml` and commits `uv.lock` with hashes (`BUILD_BRIEF.md:114`).

**Runtime and platform**

| Component | Version | How verified | Note |
|---|---|---|---|
| Python | 3.12.13 | `uv python list 3.12` in this session (newest 3.12 build listed) | Build VM has 3.12.3; the Docker base tag and digest are **verify at K8** |
| PostgreSQL | 16 (16.15 in the build VM) | `psql --version` in this session | Railway's minor version and the CI service image digest: **verify at K8** |
| uv | 0.13.0 (2026-10-09) | PyPI | Build VM has 0.11.32 |
| fastapi | 0.143.0 (2026-10-08) | PyPI | |
| jinja2 | 3.1.6 (2025-03-05) | PyPI | |
| sqlalchemy | 2.1.4 (2026-10-07) | PyPI | |
| alembic | 1.20.0 (2026-09-11) | PyPI | |
| pydantic | 2.14.1 (2026-10-11) | PyPI | |
| twilio | 9.12.0 (2026-10-12) | PyPI | |
| stripe | 16.0.0 (2026-10-01) | PyPI | |
| anthropic | 1.13.0 (2026-10-09) | PyPI | 2.x was released after the cutoff |
| sentry-sdk | 2.72.0 (2026-10-12) | PyPI | |
| uvicorn | 0.54.0 (2026-09-25) | PyPI | new library, D-097 |
| psycopg[binary] | 3.3.6 (2026-09-18) | PyPI | new library, D-097 |
| pydantic-settings | 2.15.0 (2026-08-07) | PyPI | new library, D-097 |
| resend | 2.49.1 (2026-10-03) | PyPI | new library, D-097 |
| lob-python | 5.2.1 (2026-09-02) | PyPI | new library, D-097 |
| cryptography | 50.0.2 (2026-09-30) | PyPI | new library, D-097 |
| argon2-cffi | 25.1.0 (2025-06-03) | PyPI | new library, D-097 |
| webauthn | 3.0.1 (2026-09-25) | PyPI | new library, D-097, D-098 |
| pyotp | 2.10.0 (2026-06-14) | PyPI | new library, D-097, D-098 |
| python-multipart | 0.0.32 (2026-06-04) | PyPI | new library, D-097 |
| pillow | 13.0.0 (2026-10-16) | PyPI | new library, D-097; after the cutoff because 12.3.0 has known vulnerabilities (D-096) |
| tzdata | 2026.5 (2026-10-03) | PyPI | new library, D-097 |
| pyyaml | 6.0.3 (2025-09-25) | PyPI | D-036, version updated by D-096 |

**Development and checks**

| Component | Version | How verified | Note |
|---|---|---|---|
| pytest | 9.1.1 (2026-06-19) | PyPI | |
| hypothesis | 6.169.0 (2026-10-11) | PyPI | |
| playwright | 1.64.0 (2026-10-13) | PyPI | Browser downloads: K8 checks whether they install in sessions (`BUILD_BRIEF.md:287`) |
| mutmut | 3.8.0 (2026-09-12) | PyPI | |
| ruff | 0.17.0 (2026-10-09) | PyPI | |
| mypy | 2.4.0 (2026-10-01) | PyPI | |
| bandit | 1.9.4 (2026-02-25) | PyPI | |
| pip-audit | 2.10.1 (2026-06-10) | PyPI | |
| pytest-cov / coverage | 7.1.0 (2026-03-21) / 7.16.2 (2026-09-27) | PyPI | new library, D-097 |
| pytest-playwright | 0.10.0 (2026-10-08) | PyPI | new library, D-097 |
| httpx | 0.28.1 (2024-12-06) | PyPI | new library (FastAPI's test client needs it), D-097 |
| types-PyYAML | 6.0.12.20260906 (2026-09-06) | PyPI | new library, D-097 |
| axe-core (JavaScript, injected) | 4.14.0 (2026-10-05) | npm registry | new library, D-097; vendored file, no Node at runtime |
| gitleaks | **verify at K8** | GitHub API not reachable from this session | |
| stripe/stripe-mock image | **verify at K8** | not checked | pinned by digest |
| OWASP ZAP baseline image | **verify at K8** (Phase 2) | not checked | pinned by digest |
| Docker base image | **verify at K8** | not checked | `python:3.12.13-slim` family, pinned by digest (SEC-021) |

**Resolution and audit run in this session:** `uv pip compile` of the runtime and development lists above for Python 3.12 on x86-64 Linux resolved 118 packages. `pip-audit 2.10.1 -r <compiled list> --no-deps --disable-pip` first reported 38 known vulnerabilities in pillow 12.3.0, all fixed in 13.0.0; with pillow 13.0.0 it reported "No known vulnerabilities found". This audit covers PyPI packages only; axe-core and the images are not covered by it. The compiled list was a scratch file and is not committed: K8 re-runs the resolution and pip-audit when it writes `uv.lock`, and pip-audit runs on every PR after that (SEC-021).

The task that builds photo handling checks pillow 13.x's release notes for breaking changes before writing the code (pillow 13.0.0 is a new major version).

## 12. Keeping this file true

- A new module, table, cross-module call, status, timeout, adapter or pinned version changes this file in the same PR, with a `docs/decisions.md` entry when it is a choice (`BUILD_BRIEF.md:489`).
- **Build order and file ownership** live in `kit/tasks.json` (K6): each build task names the module and the files it owns (`app/<module>/…`, its own `migrations/versions/t###_<slug>.py`, its own test files), and `python3 scripts/validate_tasks.py` fails if two tasks that can run in either order own the same file, or if a non-`harness` task owns a protected path (D-111, D-118). `app/main.py`, this file, `docs/decisions.md` and `docs/for-casey-review.md` are the only shared files. Stage 2 builds layers 0 to 7 far enough for job cards, the quote page and the pick (including the license imports and business records that matching reads); later stages add to each module's own files in dependency order (D-112).
- A new library needs a decisions entry with purpose, license and a pip-audit result (`BUILD_BRIEF.md:122`).
- The threat model (`docs/threat-model.md`) changes with any new trust boundary or asset.
