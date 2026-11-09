# Fourth repository audit of November 9, 2026: dispositions

**Input:** a fourth external, read-only review of `main` at `79dcc63` (45 findings, F1 to F45, and 4 related observations), shared by Casey in chat and treated as untrusted input. Three verifiers checked every item against the code for the overseer; this plan change spot-checked each file and line it acts on against `main` at `4c269f60` before planning anything. Decisions D-2175 to D-2184.

Verdicts: **real** (the code on `main` shows it and no task owns the fix), **owned** (a queued task already covers it), **declined** (wrong, deliberate, or opinion), **Casey** (a product question; no rule changed).

## Findings

| Item | Verdict | Evidence on `main` 4c269f60 | Disposition | Owner | Decision |
|---|---|---|---|---|---|
| F1 Most of the product has no route | Owned | `build_router` (`app/server/src/lib.rs:96`) merges only the quote, pick, feedback and webhook routers | Each built page or action has a queued serving task and a route-table row | T300 to T310, T261, T231, T280, T284 | D-2128, D-2175 |
| F2 Client IP from a client-controllable header | Real | `ClientIpPolicy::client_ip` (`app/core/src/rate_limit.rs:146`) trusts the configured edge header; D-709 left it to "verify with host" | A probe on the test project proves the edge overwrites a forged header; live and sandbox refuse to start without an explicit header setting | T135 (extended) | D-2183 |
| F3 No `/healthz` | Owned | none on `main` | T106 builds `app/server/src/healthz.rs`; T135 sets the host's health check | T106, T135 | D-2175 |
| F4 No request body size limit | Declined | axum 0.8's `Bytes` and `Form` extractors apply a 2 MB default body limit without a layer; T286's whole-request limit bounds a slow body | No change | — | D-2175 |
| F5 Stripe IP allowlist refusals are silent | Real | `check_webhook_source` (`app/adapters/src/signatures.rs:307`) refusals log one line and record no security event | Each Stripe, Twilio text and Twilio voice 403 records a capped `webhook_signature_failed` event feeding SEC-048's hourly alert; the nightly list comparison (SEC-049) stays | T319 (new) | D-2182 |
| F6 `catch-panic` enabled, no layer | Real | `Cargo.toml:38`; no `CatchPanicLayer` in `build_router` | `CatchPanicLayer` inside the security headers, 500 with no panic text | T286 (extended) | D-2183 |
| F7 Expired sessions never swept | Real | `app/portals/src/session.rs:165` deletes only a replaced cookie's row | A daily `portals.session_sweep` | T319 (new; T272 does not own `session.rs`) | D-2182 |
| F8 Documentation volume and drifting citations | Declined | opinion; `trace_spec.py` already fails a shifted requirement citation | No change | — | D-2175 |
| F9 Files over 900 lines | Declined | opinion; the 50-line function rule is enforced, file length is not a rule | No change | — | D-2175 |
| F10 Stale CI failure (RUSTSEC-2026-0459) | Declined | `.cargo/audit.toml` already ignores it with its reason (D-2144); the quoted run predates it | No change | — | D-2175 |
| F11 Directory versus text-first positioning | Declined | WEB-003: no directory is built | No change | — | D-2175 |
| F12 Rate limits built, called by nothing | Real | `app/portals/src/limits.rs:69` to `:165` and `app/intake/src/limits.rs:216` have only test callers; D-1951 named callers but only T302's goal said so | Each route's task calls its limit first: T280, T301, T302, T303, T307; T317 calls `admit_new_request` in the inbound delivery's transaction. Quotes by text: no such path exists; the first task that builds one calls `admit_quote` with no IP | T317 (new), T280, T301, T302, T303, T307 (extended) | D-2181 |
| F13 Migration numbering gaps | Declined | numbers are reserved per task by serial position (V9); gaps are expected | No change | — | D-2175 |
| F14 Every security rejection writes a row, unthrottled | Real | `refuse` (`app/portals/src/security.rs:96`) acquires a connection and inserts before any limit | An in-process cap per kind, address and route per minute, never below the hourly alert threshold | T319 (new) | D-2182 |
| F15 Public POSTs have no abuse cap | Declined | token and CSRF checks, one feedback per job (`app/portals/src/feedback.rs:170`) and PICK-055's caps bound them; T297's war games measure abuse | No change | — | D-2175 |
| F16 Raw inbound text never stored | Declined | deliberate (D-801, D-1801); F25's alert covers the failure case | No change | — | D-2175 |
| F17 Documented functional holes | Real | `names: Vec::new()` (`app/server/src/lib.rs:277`, D-1310); TXT-027 not sent (`app/picks/src/release.rs:31`, D-1687); stale `app/intake/src/handlers.rs:20`; the other three paths are F1's | Public names read from the license record; LIST names per text; who's-calling text rebuilt; the module doc corrected; the served quote page's cards (PICK-041) recorded as IC-71 with no owner | T316, T317, T318 (new), T151 (extended); IC-71 unplaced | D-2181 |
| F18 Harness coupled to model names | Declined | opinion; the runner checks the model that ran (D-486) | No change | — | D-2175 |
| F19 `notify.yml` and `inbox.yml` hold `contents: write` | Declined | both push to `claude/kit-state`, which needs it (D-479) | No change | — | D-2175 |
| F20 Four-digit PINs, six tries a day | Declined | CALL-003 and CALL-025 as specified (D-480); the read-back adds the second factor | No change | — | D-2175 |
| F21 Empty CSRF token renders an empty field | Declined | the check refuses an empty token, so it fails closed; style only | No change | — | D-2175 |
| F22 `multiple-versions = "warn"` in `deny.toml` | Declined | deliberate; duplicate versions are reviewed, not blocked | No change | — | D-2175 |
| F23 `views[0]` fallback | Real | `find` (`app/picks/src/service.rs:223`) returns another job's view and panics on an empty list | `find` returns `Option`; callers treat a miss as an error | T318 (new) | D-2181 |
| F24 Lower-case addresses bypass the 911 stripper | Real | `strip_addresses` (`app/intake/src/emergency.rs:258`) needs a capital; "100 smoketree test ave" matches the smoke stem | Case-free suffix and street-word match, no market street names in code | T312 (new) | D-2177 |
| F25 Inbound texts failing for a non-database reason are lost silently | Real | `answer` (`app/server/src/inbound_text.rs:164`) answers 200 and logs only | One body-free High alert per inbox row | T279 (extended) | D-2182 |
| F26 Business clock runs every day | Casey | `app/core/src/clock.rs:16`; D-004 and JOB-007 say every day | Open question in `docs/for-casey-review.md`; kept as is unless Casey says otherwise | Casey | D-2175 |
| F27 Out-of-table area code never texted, never alerted | Declined | QH-003 and D-100: a number outside the table is not texted by design | No change | — | D-2175 |
| F28 Alerts written, never delivered | Owned, extended | `raise_alert` (`app/ops/src/service.rs:96`) only inserts; no handler sends | T204 builds the sender; it now also registers it in `register_handlers` and proves delivery through the worker | T204 (extended) | D-2182 |
| F29 No backup, restore or drill | Owned | `recovery` is planned | T109, T110, T206, T139, T117 | T109, T110, T206, T139, T117 | D-2175 |
| F30 Alert rows not de-duplicated | Real | `migrations/0021_ops_audit.sql:33` has no open-once rule; `check_webhook_silence` would raise every run | One open alert per kind and subject; `raise_alert` returns the open one; the silence check scheduled after it | T320 (new), T204 (extended) | D-2182 |
| F31 Feedback words outside "Delete my data" and the 3-year purge | Real | `migrations/0139_jobs_feedback.sql:12` and `:31`; D-1821 (4) promised a later step | Column-scoped, audited blanking on the text stage and in the delete transaction; withdrawn from the public share | T315 (new) | D-2180 |
| F32 5-year consent retention has no deletion | Real | `migrations/0053_intake_tables.sql:62`; `docs/legal/privacy-policy.md:91` | A daily sweep deletes CON-022 rows at 1,827 days; an opted-out number keeps a digest; the policy line is Casey's to read | T321 (new) | D-2183 |
| F33 `alert_once` spends its marker before the alert | Declined | D-1953's named trade-off | No change | — | D-2175 |
| F34 Download masking misses bare domains and split numbers | Real | `mask_contact_details` (`app/portals/src/data_download.rs:313`) | Bare domains and line-split digit runs masked | T315 (new) | D-2180 |
| F35 Confirm page can't load Stripe under the app-wide CSP | Owned | `app/portals/src/security.rs:35` | T261 adds the Confirm-only CSP and its test | T261 | D-946, D-1754, D-2175 |
| F36 Confirm e2e never exercises the CSP | Owned | `tests/e2e/confirm_pay.spec.ts` | T261's `tests/e2e/confirm_served.spec.ts`, TD-SEC-37 | T261 | D-1754, D-2175 |
| F37 Public base URL accepts a path | Real | `public_base_url` (`app/core/src/settings.rs:397`) checks host, query and fragment only | A path stops start-up | T286 (extended) | D-2183 |
| F38 95% floors only for three crates | Real | `tests/coverage_floor.json:5` | Per-file floors for money and privacy files elsewhere | T324 (new, harness) | D-2184 |
| F39 Read-back can connect a different line | Real | `app/calls/src/pins.rs:299` drops the line; `app/server/src/inbound_voice.rs:206`; `pins.rs:333` picks the newest | The line read back is stored on its try and is the only line the confirm connects | T311 (new) | D-2176 |
| F40 AI daily cap check-then-act | Owned | `app/ai/src/service.rs:257` | T236's single-statement admission (D-1757) | T236 | D-1757, D-2175 |
| F41 `adds_fact` skips sentence-start capitals | Real | `app/ai/src/output_checks.rs:281` | Unknown sentence-start words count unless on a data list of openers | T323 (new) | D-2184 |
| F42 Every business notice held overnight, EMERGENCY too | Real | `app/messaging/src/business_notices.rs:460`; handler not registered (D-1826 (1)) | `emergency: true` passes the night rule; the handler is registered | T313 (new), T251 (extended) | D-2178 |
| F43 EMERGENCY card loses subject and first line | Real | payload at `app/quotes/src/emergency_card_notice.rs:64`; `Parsed` (`business_notices.rs:171`) has no field | Subject and first line read and used | T313 (new) | D-2178 |
| F44 Dispute submission state committed before its task or fee | Real | `app/payments/src/disputes.rs:169`, `:197`; no production caller of `dispute_auto_submit` or `record_owner_submission` | One transaction each; a `picks.dispute_opened` handler; Casey's own-submission button | T314 (new), T205, T304 (extended) | D-2179 |
| F45 Backup address can become a main address | Real | `confirm_backup` (`app/businesses/src/backup_address.rs:199`) does not re-check; `notice_addresses` (`:290`) no de-duplication | Re-check at confirmation; `add_member` rule decided; one mailbox listed once | T322 (new) | D-2184 |

## Related observations

| Observation | Verdict | Evidence | Disposition | Owner | Decision |
|---|---|---|---|---|---|
| R1 `decide_at_96_hours` commits refund and close, then queues the decision email on a fresh connection | Real | `app/picks/src/unreachable_refund.rs:365` | T281's step is retried after a failure even when the job is Closed; the email's key keeps it once | T281 (extended) | D-2179 |
| R2 `schedule_check_ins` takes a bare connection and has no caller | Owned | `app/jobs/src/followups.rs:138`; `app/jobs/CLAUDE.md:44` | T278 calls it inside the release's transaction | T278 | D-2175 |
| R3 `queue_card_notice` and `queue_emergency_card` have only test callers | Owned, extended | `app/quotes/src/api.rs:44`, `:48` | T251's card sender calls `queue_card_notice`, and now `queue_emergency_card` on the emergency path | T251 (extended), T313 | D-2178 |
| R4 `dispute_auto_submit` unwired | Real | `app/picks/src/unreachable_refund.rs:467` | Same as F44 | T314 (new) | D-2179 |

## Plan fixes made on the way

- T103, T105, T204, T151, T109 and T110 (QUEUED, nothing written) move their migrations from 0174, 0175, 0176, 0177, 0181 and 0182 to 0176, 0177, 0179, 0180, 0183 and 0184, so T311's 0174, T315's 0175, T320's 0178 and T321's 0181 rise in the runner's serial order (V9).
- T317 is a harness task because it adopts the four done tests that build `Webhooks` by name; it also adds the route's `Environment`, which T299 (a build task that could not change those tests) needed. T279 and T299 are told to add no other field.
- T100 (stage-4 cleanup) depends on T311 to T319, T322 and T323; T126 (stage-5 cleanup) on T320, T321 and T324; T204 on T320; T299 on T317.
- Register rows IC-65 to IC-73 and risk R-32 added. IC-71 (the served quote page's cards show no business name, license lines or review links, PICK-041) has no owner and is left for the overseer.
- The overseer runs an independent review of T311 (security), T314 (money) and T315 (privacy) when each hands in.
- No task IN_PROGRESS or MERGE_PENDING was edited.
