# Laftly FAIL-PATTERNS-1 — failure analysis and prevention plan

**Verdict: 268 failed attempts analysed; top attributable cause plan / ownership gap (40.3%); proposed fixes would save ~95 attempts (scenario, not measured).**

## 1. Root-cause clusters

Source: supplied JSON, prompt template, build brief excerpt and code-docs only. The 198 task records cover Nov 10–26 UTC. There are 268 non-success, non-resync records across 170 tasks: 118 BLOCKED, 85 check_failed, 60 QUEUED, 3 update_conflict, 2 proof_missing. Exclude 190 done and 75 resync records. Records are not necessarily charges against the current attempt counter. Requeues and repeated attempt numbers complicate counting; use array index as record key.

Known failed-worker token sum: **1,611,855,957** across 229/268 records; 39 missing values are not zeros. Resyncs separately account for 579,228,973 known worker tokens. These are reported worker token sums, not billable costs or reviewer/CI usage.

Attribution: match each distinct latest failure, current reason or requeue note once to the latest still-unassigned compatible failed record on the same task (deduplicate by leading text). Notes lack attempt timestamps; matches are provisional. Earlier CI rows without a compatible note are classified by their single gate or multi-gate result, not by invented cause. For notes with multiple causes, classify the primary reported obstruction. A bare resync note is not charged as a failure; only explicit update_conflict records count there. The stricter latest-note-only pass found 63 plan/ownership records; 108 is the note-enriched estimate, not a confirmed exact count. See the companion CSV for each assignment.

| Cluster | Attempts | % of 268 | Known tokens (records known) | Example evidence (task#attempt; quoted excerpts) |
|---|---:|---:|---:|---|
| Plan / ownership gap | 108 | 40.3% | 539,921,981 (89/108) | T261#3: “PLAN GAP: tests/e2e/confirm_served.spec.ts can't be built in T261. The fix belongs to tests/e2e/webserver.ts, which T227, T385, T452 and T469 own (not T261). Its appEnv (:89) and keyedEnv (:107) set no ASKHAVA_STRIPE_WEBHOOK_SECRE…”<br>T672#2: “plan gap reported: PLAN GAP: tests/integration/tests/intake_order_two_jobs.rs:278 (owned by T711 and T717, not T672) also pins the old order, but D-4822 named only two functions in intake_order_two_jobs_rules.rs. Evidence: test td…”<br>T801#1: “PLAN GAP: app/server/tests/web_chat_stops.rs:572 and :592 (owned by T645) assert a consent text queued to a chat-verified number that never texted in; fixing review finding (web_chat consent must never permit a text, CON-005/CON-0…”<br>T671#1: “PLAN GAP: three tests outside T671's scope fail because of the new behaviour (worker edited none of them): 1. tests/integration/tests/inbound_text_guards.rs:306-308 (T317): quoted_job helper expects begin_job to return Opened::Cre…” |
| Review findings unresolved | 28 | 10.4% | 280,818,578 (28/28) | T222#2: “Reviewer over 457819c6bc470ff6f69d271d7ab023fb3c0c25eb ended REVIEW-VERDICT: FINDINGS (hand_in_by too close for a fix round): 1. tests/e2e/j2_full.ts:190 closingChecks omits no-duplicate-message, audit-row-per-status-change and no…”<br>T470#2: “Fix-round review (after the one allowed findings round) still found a problem at head 1830747e4; no reviewer approved a head. 1. app/server/src/email_due.rs:85 and :95: current() accepts a mailbox only when businesses_of_mailbox (…”<br>T666#1: “Reviewer after the findings round (head bf41c81b22bdbd874d7d53cb8ba64ca6bb0737cf): app/server/tests/job_page_route.rs (whole file) and app/quotes/src/carded_job.rs:84-92: no test reaches QuoteStanding::Withdrawn, and nothing calls…”<br>T542#1: “Reviewer after the findings round still ended REVIEW-VERDICT: FINDINGS at head 4de98e6fb74e1e5394629d7a1abe0694167723a2: 1. app/server/tests/address_changes_route.rs:466-477 the "no pending row" check is vacuous (compares open wit…” |
| Other CI checks | 28 | 10.4% | 141,532,159 (22/28) | T663#2: “Ci: failure; Coverage: failure Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) test backup::tests::td_priv_14_an_export_is_unreadable_without_the_offline_key ... ok test backup::tests::the_path_help…”<br>T754#3: “Ci: failure; Checks: failure Checks failed at step: Rust build (the one boundary build) and tests (all but SPLIT_TESTS, D-3142) Running tests/status_enums.rs (target/debug/deps/status_enums-3039d66fa660459e) running 3 tests test t…”<br>T479#1: “Ci: failure; Checks: failure Checks failed at step: Rust build (the one boundary build) and tests (all but SPLIT_TESTS, D-3142) test pick_046_opted_out_with_an_email_gets_one_email_and_no_text ... FAILED test pick_046_no_consent_r…”<br>T454#1: “Ci: failure; Coverage: failure; Checks: failure Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) test td_sec_35_core_default_revoke_exception_is_narrow ... ok test td_sec_10_migrations_and_sql_obey_t…” |
| Unknown worker/block cause | 26 | 9.7% | 183,875,853 (20/26) | T222#1: `QUEUED`; checks `none`; no attributable note<br>T118#1: `QUEUED`; checks `none`; no attributable note<br>T470#1: `QUEUED`; checks `none`; no attributable note<br>T730#1: `QUEUED`; checks `none`; no attributable note |
| Accessibility (cause unverified) | 14 | 5.2% | 124,295,039 (13/14) | T261#4: “Ally: failure Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) + "/b/picks/{pick}/confirm", + ] 13 \| expect(ROUTES.length).toBeGreaterThan(0); 14 \| const missing = ROUTES.filter…”<br>T649#1: “Ally: failure Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) 88 \| const at = await where(page); > 89 \| expect(at.index, `Tab from "${before.name}" reaches the next control in …”<br>T804#1: “Ally: failure Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) 88 \| const at = await where(page); > 89 \| expect(at.index, `Tab from "${before.name}" reaches the next control in …”<br>T737#1: “Ally: failure Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) 79 \| expect((await axeScan(page, ["color-contrast", "link-in-text-block"])).violations).toEqual([]); > 80 \| expect…” |
| E2E journey (cause unverified) | 11 | 4.1% | 59,333,545 (10/11) | T608#1: “Ci: failure; E2e: failure E2e failed at step: The journeys (npx playwright test tests/e2e) {"timestamp":"2026-11-21T18:51:43.176909Z","level":"INFO","message":"task run","task":1264,"kind":"ops.offdb_stream","outcome":"Ok(Done)","…”<br>T373#1: “Ci: failure; E2e: failure E2e failed at step: The journeys (npx playwright test tests/e2e) Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.21s stand-ins listening on http://127.0.0.1:4010 {"timestamp":"2026-11-16T2…”<br>T657#1: “Ci: failure; E2e: failure E2e failed at step: The journeys (npx playwright test tests/e2e) {"timestamp":"2026-11-26T21:50:01.830209Z","level":"INFO","message":"task run","task":2774,"kind":"ops.offdb_stream","outcome":"Ok(Done)","…”<br>T741#1: “Ci: failure; E2e: failure E2e failed at step: The journeys (npx playwright test tests/e2e) {"timestamp":"2026-11-24T15:55:11.197743Z","level":"INFO","message":"task run","task":3351,"kind":"ops.offdb_stream","outcome":"Ok(Done)","…” |
| Dependency audit | 10 | 3.7% | 54,459,149 (10/10) | T730#3: “Ci: failure; Checks: failure Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) Warning: unmaintained Title: `basic-toml` is unmaintained Date: 2026-09-26 ID: RUSTSEC-2026-0374 URL: https://rustsec.org/advisori…”<br>T489#2: “Ci: failure; Checks: failure Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) Date: 2026-11-16 ID: RUSTSEC-2026-0554 URL: https://github.com/actix/actix-web/releases/tag/actix-http-v3.21.2 Severity: 6.9 (medi…”<br>T121#3: “Ci: failure; Checks: failure Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) Crate: basic-toml Version: 0.1.10 Warning: unmaintained Title: `basic-toml` is unmaintained Date: 2026-09-26 ID: RUSTSEC-2026-0374…”<br>T653#1: “Ci: failure; Checks: failure Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) Warning: unmaintained Title: `basic-toml` is unmaintained Date: 2026-09-26 ID: RUSTSEC-2026-0374 URL: https://rustsec.org/advisori…” |
| Session / hand-in incomplete | 10 | 3.7% | 9,695,294 (10/10) | T255#2: “session ended without handing in”<br>T820#2: “Hand-in time passed. Worker added unit tests in app/portals/src/data_download.rs (commit f4a102dca); portals-only coverage of data_download.rs measured 73.68 (floor 81.0), the full integration+portals+server measurement never fini…”<br>T803#3: “Worker made no change. All 83 named mutants are killed; the Checks job timed out (40 min limit) because the Rust build took ~26 min and mutants.toml changes trigger all 83 mutants (cut off after mutant 72). Needs a harness fix (ra…”<br>T714#3: “Worker made no commits: start smoke test did not finish within the 30-minute background limit (312 suites ok, no failures seen, partial). Previous CI failure is cargo-audit RUSTSEC-2026-0594 (cookie 0.18.2, fixed in 0.18.3), which…” |
| Infrastructure / cancellation | 7 | 2.6% | 36,065,067 (5/7) | T593#1: “Ci: failure; Coverage: cancelled Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) test server::tests::the_notice_senders_need_the_postal_setting_in_test_and_live ... ok test server::tests::with_a_pub…”<br>T573#3: “Ci: failure; Coverage: cancelled Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) test pages::tests::web_009_the_request_box_posts_with_a_label_and_the_token ... ok test pages::tests::waits_read_in_m…”<br>T727#1: “Ci: failure; Coverage: cancelled Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) test td_pick_52_the_first_release_follows_the_stored_choice ... ok test result: ok. 2 passed; 0 failed; 0 ignored; 0 …”<br>T529#1: “Ci: failure; Coverage: cancelled Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) test td_job_08_later_at_day_3_asks_again_at_day_14 ... ok test td_txt_13_problem_reply_in_both_languages ... ok test …” |
| Coverage job (cause unverified) | 7 | 2.6% | 45,421,681 (5/7) | T820#1: `check_failed`; checks `Ci; Coverage`; no attributable note<br>T707#1: `check_failed`; checks `Ci; Coverage`; no attributable note<br>T615#1: `check_failed`; checks `Ci; Coverage`; no attributable note<br>T590#1: `check_failed`; checks `Ci; Coverage`; no attributable note |
| Missing proof / model mismatch | 6 | 2.2% | 35,545,489 (6/6) | T573#2: “model claude-opus-5-5 ran, task names claude-fable-5-2 re-queued after plan fix D-4969: Coverage time-limit cancel, fixed by T770 (D-4969, #1206)”<br>T607#1: “review_missing on 2c1fc315ebe4 (D-3728): not proven reviewed or worker-made (b) 5cb49e3317bc T607: NOTES entry in the section format (D-4752): no --worker-agent transcript holds Git's `[claude/task/T607-site-v6-bringup <abbrev>]` …”<br>T655#1: “model claude-opus-5-5 ran, task names claude-fable-5-2”<br>T615#3: “model claude-opus-5-5 ran, task names claude-fable-5-2” |
| Multiple CI gates (cause unverified) | 5 | 1.9% | 39,028,655 (3/5) | T118#2: `check_failed`; checks `Ci; Ally; E2e`; no attributable note<br>T541#1: `check_failed`; checks `Ci; Coverage; E2e`; no attributable note<br>T646#1: `check_failed`; checks `Ally; Checks`; no attributable note<br>T269#1: `check_failed`; checks `Ci; E2e; Ally`; no attributable note |
| Coverage floor missed | 3 | 1.1% | 26,458,345 (3/3) | T359#2: “Ci: failure; Coverage: failure Coverage failed at step: Compare with tests/coverage_floor.json and refuse a lowered floor (D-694) app/licensing: 96.06557377049181 app/messaging: 87.97814207650273 app/ops: 94.31818181818181 app/pay…”<br>T563#2: “Ci: failure; Coverage: failure Coverage failed at step: Compare with tests/coverage_floor.json and refuse a lowered floor (D-694) app/picks/src/settle.rs: 95.83333333333333 (floor 95.0) app/picks/src/unreachable_refund.rs: 89.7959…”<br>T729#2: “Ci: failure; Coverage: failure Coverage failed at step: Compare with tests/coverage_floor.json and refuse a lowered floor (D-694) app/picks/src/settle.rs: 100.0 (floor 95.0) app/picks/src/unreachable_refund.rs: 89.79591836734694 (…” |
| Merge/update conflict | 3 | 1.1% | 1,513,749 (3/3) | T437#2: “merging main into the reviewed head conflicts in app/calls/CLAUDE.md, app/quotes/CLAUDE.md; a resync merges it in on this attempt (D-891) re-queued after plan fix D-3698: D-3698 (#777): T437 no longer edits crate CLAUDE.md/NOTES.m…” |
| Baseline smoke red | 2 | 0.7% | 33,891,373 (2/2) | T675#3: “runner smoke red on 4ac8ae78a989 (bash scripts/smoke.sh, D-3530): env = {"PATH": path, "PREHANDIN_ROOT": str(work), "HOME": str(tmp_path), "DATABASE_URL": "postgres://fake@localhost/none", "E2E_PORT": "39871", "FAKE_CODE": str(cod…”<br>T669#1: “runner smoke red on c403ea4898ac (bash scripts/smoke.sh, D-3530): line = cache.seed(tmp_path, smoke, debug, lambda args: (0, _metadata(MEMBERS))) left = sorted(str(p.relative_to(debug)) for p in debug.rglob("*") if p.is_file()) > …” |

A gate label is not a root cause. Coverage jobs may stop before computing a floor (T212 confirm_window panicked; T727 `Coverage: cancelled`). Only T359, T563 and T729 have attributable floor-comparison failures in this classification. Ally and E2E signatures may be regression, fixture defect or flake; repetition does not prove flakiness. Other CI checks includes assertion, lint, Python and unclassified checks. No verified model refusal count; T655 instead says `model claude-opus-5-5 ran, task names claude-fable-5-2`.

## 2. Six highest-volume actionable fixes

Central savings scenario: 95/268 (~35%). Range 61–124 is a planning scenario, not a statistical interval. No replay or holdout validates it. No savings assumed for unknown records. Keep all mandatory gates, authorized ownership, and independent review.

### 1. Scope closure gate — overseer before claim

**Target:** plan/ownership (108 assigned records). **Estimated saved:** 55 (35–70).

**Current anchor:** TEMPLATE.md, `## Scope`: “Files you own (change only these, plus the shared paths below)” and “Files you read (do not change).” Rules that cannot bend already require a proven PLAN GAP report, but it consumes an attempt (D-2859).

**Proposed exact policy insertion:**

> Before release, the overseer must attach a scope-closure report at the proposed claim SHA. For each acceptance and named test, identify the implementation entry point, public API, schema/fixture, guarding tests, owner, dependency status and allowed edits. Compare required changes to owns and shared-path restrictions. Flag missing readers, hard-coded expectations, contradictory decisions, unmerged dependencies and budget overflow. If a conflict is supported by file:line and a proving command, return PLAN_NOT_READY; repair the task entry or create an owning predecessor and regenerate the prompt. Do not launch a worker until the prerequisite is merged and the proof is refreshed. No blanket scope expansion.

**Evidence/detection:** T099 webhook secret unavailable to keyed specs; T113 `== 10` must become 11; T205 “no public reader that ties a dispute to its pick”; T302 dependency T450 not on main; T801 protected tests expect a consent SMS that the new rule prohibits. Read actual task entries, dependency graph, test design, owners and source references (not supplied here). Search requirement IDs and changed symbols in protected tests; use a targeted read-only verifier for ambiguous closure.

**Reasoning/risk:** 55/108 assumes roughly half are detectable in a bounded closure scan. Target repeated plan gaps first. Additional preflight time and false stops; require evidence for holds and authorized plan repair, not broad edits.

### 2. Acceptance-to-negative-test matrix before first review

**Target:** unresolved review findings (28). **Estimated saved:** 12 (7–16).

**Current anchor:** TEMPLATE.md, `## End routine`, step 3: “Report changed files, commands run and actual results. Claim no passing check without a tool run in this repository.”

**Proposed exact insertion:**

> Before first review, attach one row per acceptance item: requirement/decision, implementation file:line, positive test, boundary/negative test, and actual command/result. For every changed guard or scanner, plant a representative wrong implementation or input in an isolated scratch tree and show the test rejects it. For every reviewer finding, map the fix to a regression test and check neighboring cases before the independent re-review. If a fix is outside scope, report PLAN GAP. Do not request approval without complete evidence. Keep the independent reviewer and one fix round.

**Evidence:** T111 `$25.50` parsed as allowed $25; T122 conditional homeowner campaign wording passes; T542 vacuous no-pending-row assertion; T743 “assert_clean checks a digest only as a hex string” and misses decimal bytes.

**Reasoning/risk:** 12/28 assumes ~43% caught by requirement-driven negative testing. Additional test writing; cap matrix to changed requirements; never weaken guards or count scratch mutations as production edits.

### 3. CI parity and exact failing-test reproduction

**Target:** other CI checks (28). **Estimated saved:** 9 (6–12).

**Current anchor:** TEMPLATE.md, `## Run your tests`, and End routine step 1: “Run "Run your tests" until green; then full smoke once”.

**Proposed exact insertion:**

> Before first review, run the exact CI commands covering touched crates, guarding integration tests, Python/lint and named mutants as applicable, with pinned toolchain and CI features/env. Keep a compact command manifest (SHA, command, exit, duration, log path). For an assertion failure, reproduce on branch and claim base with equivalent environment. If the same failure is verified on base, report MAIN RED and route to FIX-MAIN; if branch-only, fix the owned cause or report a proven scope gap. Do not use an unrelated crate-only pass as evidence of the full guard. Full required CI remains unchanged.

**Evidence:** T212 `td_pick_09_license_at_the_pick_and_at_confirm` fails in Coverage and Checks; T454 `architecture_migrations` fails in both; T357 clippy too-many-lines; T802 `action_links_route` fails. T624 Python failure is a separate check.

**Reasoning/risk:** 9/28 assumes one-third caught before hand-in; some failures are outside ownership or late main changes. Risk: duplicate compilation; reuse artifacts keyed by SHA/toolchain/features, not stale green results.

### 4. Shared Ally signature quarantine and base/head comparison

**Target:** accessibility failures (14). **Estimated saved:** 6 (4–8).

**Current anchor:** TEMPLATE.md, End routine step 1: “A `FAIL` solely on untouched pages/routes: do not fix; begin report `ALLY RED: <failing test>` and its line; hand in.”

**Proposed exact policy:**

> At claim and hand-in, compare affected Ally results on exact base and candidate SHA using identical browser, viewport, fixture and launch channel. Run the affected spec early (for J1: npx playwright test tests/a11y/j1_keyboard.spec.ts, with the CI project and setup), and record failure signature and first diverging focus target. If repeated base failures are reproduced, park affected lanes and route to one owned repair; resume after green baseline. Retry once for diagnosis only. Full Ally on final candidate/merge remains required. Never loosen focus order, contrast, reflow, axe or target-size assertions.

**Evidence:** T410/T804/T815/T819/T826 share J1 steps 5/6 keyboard-order failure at varying widths. T737 instead reports targets/contrast on `/owner/ai`. Repetition warrants investigation, not automatic flake status.

**Reasoning/risk:** 6/14 assumes shared fixture or baseline repair can remove roughly six failures; verify before broad rollout. Parking lanes and duplicate runs cost time. Cache baseline results by exact environment and SHA.

### 5. E2E fixture contract and journey rehearsal

**Target:** E2E journeys (11). **Estimated saved:** 4 (2–6).

**Current anchor:** TEMPLATE.md, `## Run your tests`: `${run_tests}`.

**Proposed exact insertion:**

> For tasks touching route composition, UI, launch channels, messaging, clocks or stand-ins, list affected journey IDs and fixture contracts in the task entry. Before release, verify route registrations, synthetic keys, clock control and data setup against acceptance. Run affected journey specs with the CI project/channel before review; preserve failure trace and base SHA. Never edit another owner's fixture without a logged plan change. Full E2E remains mandatory on the final merged commit.

**Evidence:** T657/T741 both report J3 operator step 5 High alert in web-and-email; T320 confirm_served double-press; T373 action_links_served owner charge help. Related plan gaps T099/T119/T261 demonstrate fixture incompatibility but are counted only in cluster 1.

**Reasoning/risk:** 4/11 assumes shared fixture repairs and early targeted tests catch ~36%. More Playwright runtime; share bootstrapped fixtures safely, isolate per-test state, retain full gate.

### 6. Baseline dependency audit and infrastructure circuit breaker

**Target:** dependency audit (10), infrastructure/cancellation (7). **Estimated saved:** 9 (7–12), central 7 audit + 2 infrastructure.

**Current anchor:** TEMPLATE.md, Start routine step 2: “If `main` is red, stop and report: the only task then is "fix main".” Build brief D-3031 setup-fault exception.

**Proposed exact policy:**

> Before expensive claim, run or reuse a fresh baseline audit for exact Cargo.lock/package-lock hash and advisory database snapshot; check runner disk, cache health and recent gate runtimes. On identical baseline dependency failure, freeze affected lanes, create one owned dependency/harness repair and retry only after green. Log job, step, SHA, advisory ID, setup/test phase and cause. For cancellation distinguish timeout, superseded workflow, explicit cancellation and unknown. Only independently verified non-worker causes qualify for bounded no-charge infrastructure replay after a logged fix. Never auto-ignore advisories, raise limits blindly or treat cancellation as a pass.

**Evidence:** T204/T489 RUSTSEC-2026-0554; T714 D-5638 says cookie advisory “failed the dependency audit on every Rust change”; T384 DNS setup failure; T803 D-5519 raised Checks timeout. T820 disk full is separately categorized, not counted here.

**Reasoning/risk:** 7/10 dependency and 2/7 infrastructure preventable in central scenario. Risk stale audit cache/broad freezes; bind to lock hash and DB freshness, cap replay, retain mandatory gates.

**Additional coverage-specific quick win (not counted in 95):** add a CI-equivalent branch-coverage prehand-in check for affected files AND aggregate crate floors, comparing to `tests/coverage_floor.json`. T359 `FAIL: app/payments/ 94.98 is below the floor 95.0`; T729 `FAIL: app/privacy/ 94.86 is below the floor 95.0`; T820 partial 73.68 vs 81.0 with disk full. Exact llvm-cov command/flags and comparison script are not included; have the harness expose a verified wrapper from actual CI, no floor reduction or partial-run pass.

## 3. Pre-start predictive signals

Actual first-try failure rates cannot be calculated. The export selects tasks with failures or blocked status (including resync-only records), not all released tasks. 170/198 have a first non-resync failure, 28 have a first non-resync done. The latter are resync-only cases, not representative successes. The brief's 61% first-pass since Nov 19 is a stated baseline, not reproducible here. The following are conditional on selection, not population risks. Current budget/model/task fields may reflect post-failure revisions.

| Signal / bucket | Selected tasks | First non-resync failure | Conditional % | At least 2 failure records | Example failure IDs |
|---|---:|---:|---:|---:|---|
| context_pct 0–10 | 14 | 14 | 100.0% | 4 | T099, T221, T112 |
| context_pct 11–20 | 76 | 63 | 82.9% | 19 | T100, T101, T202 |
| context_pct 21–30 | 61 | 52 | 85.2% | 24 | T231, T200, T104 |
| context_pct 31–40 | 36 | 31 | 86.1% | 14 | T157, T214, T255 |
| context_pct 41–100 | 11 | 10 | 90.9% | 5 | T612, T653, T671 |
| files 1–2 | 31 | 29 | 93.5% | 8 | T222, T104, T107 |
| files 3–4 | 64 | 53 | 82.8% | 23 | T099, T203, T112 |
| files 5–6 | 48 | 39 | 81.2% | 10 | T231, T202, T204 |
| files 7–100 | 55 | 49 | 89.1% | 25 | T200, T100, T101 |
| lines 0–200 | 15 | 15 | 100.0% | 6 | T345, T370, T386 |
| lines 201–400 | 55 | 49 | 89.1% | 13 | T200, T113, T122 |
| lines 401–600 | 107 | 86 | 80.4% | 36 | T231, T099, T101 |
| lines 601–10000 | 21 | 20 | 95.2% | 11 | T100, T111, T117 |
| type: build | 126 | 102 | 81.0% | 33 | T099, T200, T101 |
| type: harness | 66 | 62 | 93.9% | 31 | T231, T111, T113 |
| type: docs | 4 | 4 | 100.0% | 2 | T112, T117, T122 |
| type: cleanup | 2 | 2 | 100.0% | 0 | T100, T517 |
| stage: 5 | 128 | 117 | 91.4% | 45 | T200, T101, T221 |
| stage: 4 | 70 | 53 | 75.7% | 21 | T231, T099, T100 |
| module: unspecified | 36 | 36 | 100.0% | 15 | T099, T100, T222 |
| module: server | 34 | 27 | 79.4% | 11 | T280, T300, T301 |
| module: portals | 23 | 19 | 82.6% | 8 | T231, T200, T205 |
| module: site | 21 | 20 | 95.2% | 9 | T101, T221, T202 |
| module: intake | 19 | 17 | 89.5% | 7 | T212, T251, T255 |
| module: businesses | 13 | 8 | 61.5% | 0 | T228, T316, T329 |
| module: messaging | 11 | 8 | 72.7% | 4 | T204, T107, T386 |
| module: picks | 11 | 9 | 81.8% | 1 | T157, T266, T328 |
| module: payments | 10 | 9 | 90.0% | 5 | T290, T295, T332 |
| module: quotes | 7 | 6 | 85.7% | 3 | T164, T214, T384 |
| module: ops | 4 | 4 | 100.0% | 1 | T105, T320, T528 |
| module: recovery | 2 | 2 | 100.0% | 0 | T109, T648 |
| module: calls | 2 | 2 | 100.0% | 1 | T311, T590 |
| module: privacy | 2 | 1 | 50.0% | 0 | T658 |
| module: licensing | 2 | 2 | 100.0% | 1 | T385, T653 |
| module: catalog | 1 | 0 | 0.0% | 0 | — |
| model: claude-opus-5-5 | 178 | 151 | 84.8% | 59 | T231, T200, T100 |
| model: claude-sonnet-5-5 | 20 | 19 | 95.0% | 7 | T099, T221, T222 |
| review: security | 124 | 102 | 82.3% | 44 | T231, T200, T100 |
| review: privacy | 112 | 94 | 83.9% | 37 | T231, T200, T100 |
| review: harness | 53 | 51 | 96.2% | 25 | T231, T119, T214 |
| review: money | 51 | 45 | 88.2% | 19 | T099, T100, T105 |
| review: compliance | 51 | 44 | 86.3% | 20 | T200, T221, T202 |
| review: accessibility | 34 | 33 | 97.1% | 12 | T099, T101, T221 |
| review: licensing | 10 | 9 | 90.0% | 3 | T251, T316, T385 |
| review: recovery | 7 | 7 | 100.0% | 1 | T100, T109, T117 |
| review: docs | 2 | 2 | 100.0% | 1 | T123, T615 |
| review area count 0–1 | 49 | 39 | 79.6% | 12 | T222, T104, T113 |
| review area count 2–2 | 71 | 63 | 88.7% | 24 | T099, T101, T221 |
| review area count 3–3 | 57 | 48 | 84.2% | 18 | T231, T200, T105 |
| review area count 4–99 | 21 | 20 | 95.2% | 12 | T100, T119, T164 |

Review groups overlap. Dependency counts and edges are not exported; no dependency bucket can be calculated. No model-ranking recommendation from biased data. The useful pre-start signals are concrete closure defects: T408/T583 budget mismatch, T302/T645 missing merged prerequisites, T113/T801 protected tests, T205/T664 missing/private public reader. Add claim-time snapshots and all success records, compare by week/stage, control for task type/risk area, validate on temporal holdout.

## 4. Ranked implementation order

1. **Now:** scope-closure release check on high-risk queued tasks, starting with protected test and fixture ownership (T099/T113/T801).
2. **Now:** isolate shared baseline audit and J1/J3 signatures with exact SHA evidence; avoid launching affected work until confirmed repair (T714/T815/T741).
3. **Now:** acceptance/negative-test evidence matrix and targeted CI parity manifest (T111/T743/T454).
4. **Next:** verified coverage wrapper, disk/time readiness checks, fixture contract and bounded retry (T359/T820/T803).
5. **Larger:** immutable per-attempt event ledger and distinct charge classes; shared-path coordination and CI deduplication (T437/T607/T573).

## 5. Systemic runner problems

- **Plan defects charged to workers by design.** Template Rules: “A plan-gap report consumes the attempt (D-2859).” T099 needed T515; T302 repeatedly encountered missing prerequisites/ownership; T261 three gaps before two Ally failures. Introduce PLAN_NOT_READY before claim. Post-claim verified blueprint defect parks task; independent confirmation and logged plan revision required for bounded replay. Preserve historical costs.
- **Setup and cancellations can consume attempts.** T384 DNS failed installing Chromium; T727 Coverage cancelled; T803 Checks timeout despite all 83 named mutants killed per note; T573 D-4969 Coverage time-limit cancellation. D-3031 only covers external setup faults before tests. Add `cancel_reason`, `gate_phase`, `started_tests`, `base_repro`, distinct charged/noncharged status and replay ceiling. Not all cancellations are infrastructure.
- **Dependency broadcast failures:** T714 D-5638 “failed the dependency audit on every Rust change”; T725 note says earlier audit is “main-wide”. Fix once and stop affected lane claims rather than asking each worker to fix global lockfile.
- **Resync churn:** 75 resyncs cost 579M known tokens. T437 has three update_conflict failures and note “again after 2 resyncs in this attempt”; D-3698 moved sweep away from in-flight task. Code-docs §2 already forbids condensing shared docs while writers active. Enforce claim-time write-set overlap checks, serialize broad rewrites only, keep append-only paths per §9, validate merges and preserve review.
- **Proof/model bookkeeping:** T607 lacks required worker transcript Git line, T573/T655 actual model mismatch. Validate resolved model/window/fallback before worker turns, capture commit SHA/provenance mechanically and preserve plain commit transcript required by D-3728. T100/T517 lack explanation; do not infer model failure.
- **Hand-in time/disk:** T820 “full integration+portals+server measurement never finished (disk full)”; T714 312 suites passed before smoke timeout. Schedule heavy coverage/smoke early, reserve hand-in margin from measured durations, free disk safely, avoid duplicate cold builds. No mandatory test waiver.

### Proposed event contract (harness design; not implemented)

Persist append-only records per execution UUID and hand-in: task ID/revision, prompt hash, original budget and owns/reads/deps, claim/head/merge SHA, lane, requested/resolved model, attempt number, charge class, start/end, terminal result, stable reason code, full bounded reason text, evidence artifact IDs, failing gate/step/test/signature, baseline comparison, cancellation source, worker/reviewer tokens, timing and requeue decision ID. Never overwrite causes. Store successes too. Validate CI result belongs to exact final SHA. Report episodes and charged attempts separately. Synthetic failure drill for each class. No secrets in ledger.

## Can't tell / limitations

- Exact earlier causes, true number of chargeable attempts or whether note matches are precisely correct. 39 missing failed-token values; no billing/cache breakdown.
- True first-pass rates by type, model, stage, module, budget, dependency count or review areas; no full success denominator. Current metadata may be revised.
- Whether repeated J1/J3 failures are nondeterminism, deterministic baseline regressions or different branch regressions; need matched base/head and retry traces.
- Whether every audit failure was baseline-wide beyond explicit notes; need lock hashes and advisory timestamps.
- Exact CI commands, ownership graph, runner code, status of repairs or actual saved attempts/tokens. T803/T714 fixes reportedly already landed; verify before changing again.
- Estimates are planning scenarios, not causal estimates. Do not add resync savings to failed-attempt savings or convert reported tokens to billed dollars without billing data.

Companion file `laftly_failure_ledger.csv`: each failed record, zero-based record_index within task, assigned cluster, evidence source and original note when matched. Audit aid, not ground truth.
