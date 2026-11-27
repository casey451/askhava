# Laftly — FAIL-PATTERNS-1: First-attempt failure analysis

**268 failed attempts analysed; top identifiable cause plan gap (31.7%); proposed fixes would save ~79 attempts (planning estimate, range 63–94).**

## 1. Root-cause clusters

**Scope:** 198 selected tasks, 533 event rows: 190 `done`, 75 `resync`, 268 other outcomes (118 BLOCKED, 85 check_failed, 60 QUEUED, 3 update_conflict, 2 proof_missing). The 75 free resyncs are separately costed, not counted as charged failures. These are failure records, not a reconstruction of the billing/attempt counter. The archive's 61% first-try pass figure since Nov 19 is stated in its README, not independently calculable from this failure-selected dataset. The status snapshot has 191 DONE, six QUEUED and one BLOCKED; it is not identical to the attempt log.

**Method:** each event gets exactly one primary label. Use the latest reason only for the latest non-success event when that event is not a resync; earlier notes are matched only to a unique compatible remaining QUEUED/BLOCKED event, with repeated copies of the latest reason excluded. Those note matches and even latest-reason joins are inferential because reasons lack event IDs. Explicit result codes identify proof and update conflicts. A CI gate failure is a symptom, not proof of a code defect or flaky test. Unknown records remain unknown instead of copying the latest reason backward. Multiple checks per attempt are not multiple failures. Tokens are worker_tokens only; null is unknown, not zero. Counts describe the classification, not definitive causal truth. The CSV ledger contains every non-success event and attribution basis.

| Primary cluster | Events | % of 268 | Known worker tokens (M) | Events with tokens | Example IDs and verbatim excerpts |
|---|---:|---:|---:|---:|---|
| Unknown / overwritten | 101 | 37.7% | 740.52 | 83/101 | T221: “no event-linked reason; latest note not assigned backward”; T202: “no event-linked reason; latest note not assigned backward”; T280: “no event-linked reason; latest note not assigned backward”; T385: “no event-linked reason; latest note not assigned backward”; T820: “no event-linked reason; latest note not assigned backward” |
| Plan gap | 85 | 31.7% | 402.03 | 70/85 | T099: “gives the keyed binary a random Stripe webhook secret never exposed to specs”; T205: “`payments` has no public reader that ties a dispute to its pick”; T408: “Size budget: at most 6 files, 800 changed lines”; T801: “assert a consent text queued to a chat-verified number that never texted in”; T836: “asserts cancel-in-progress is exactly False” |
| Review / behavioral findings | 23 | 8.6% | 209.72 | 23/23 | T222: “falsely says T098's j1_homeowner.spec.ts runs the Spanish variant”; T104: “pytest 154 passed, prehandin.sh green”; T519: “fresh-eyes review found a compliance bypass that remained after the one allowed fix round”; T743: “assert_clean checks a digest only as a hex string”; T834: “GATED is defined but never used” |
| Accessibility failure | 8 | 3.0% | 52.97 | 8/8 | T101: “Tab from "${before.name}" reaches the next control in source order”; T410: “J1 steps 5 and 6 keyboard-only at 320 px”; T737: “targets and contrast on /owner/ai”; T804: “J1 steps 5 and 6 keyboard-only at 1280 px”; T826: “J1 steps 5 and 6 keyboard-only at 768 px” |
| Dependency audit | 8 | 3.0% | 29.35 | 8/8 | T204: “RUSTSEC-2026-0554”; T489: “Rust dependency audit (cargo-audit, cargo-deny)”; T589: “Rust dependency audit (cargo-audit, cargo-deny)”; T714: “failed the dependency audit on every Rust change”; T829: “Rust dependency audit (cargo-audit, cargo-deny)” |
| E2E failure | 8 | 3.0% | 35.63 | 7/8 | T157: “J1 step 6 (partial): Pick #2 and the review step”; T320: “a new card pressed twice charges once and stores one terms row”; T373: “a signed-in owner's Ask about a charge link answers with the charge once”; T741: “5. High alert”; T657: “E2e failed at step: The journeys” |
| Coverage-run test failure | 7 | 2.6% | 38.90 | 4/7 | T212: “td_pick_09_license_at_the_pick_and_at_confirm”; T311: “td_call_13_five_hundred_lines_get_unique_stored_pins”; T414: “link_request”; T454: “architecture_migrations”; T615: “flaky TD-INT-47 fixed by T789” |
| Infrastructure / cancellation | 6 | 2.2% | 35.97 | 5/6 | T384: “Temporary failure resolving 'security.ubuntu.com'”; T529: “Coverage: cancelled”; T573: “Coverage time-limit cancel, fixed by T770”; T727: “The operation was canceled.”; T803: “raised the Checks limit to 60 minutes; every named mutant was already killed” |
| No hand-in / deadline | 5 | 1.9% | 2.26 | 5/5 | T266: “session ended without handing in”; T312: “session ended without handing in”; T428: “session ended without handing in”; T691: “session ended without handing in”; T820: “Hand-in time passed.” |
| Rust build / test failure | 4 | 1.5% | 8.68 | 3/4 | T370: “td_call_23_a_blocked_call_that_connects_queues_one_notice_and_one_pending_link”; T479: “Rust build (the one boundary build) and tests”; T493: “Rust build (the one boundary build) and tests”; T802: “the second price is not stored” |
| Proof / provenance | 3 | 1.1% | 14.80 | 3/3 | T100: “proof_missing; backup (structured fields; no reason text)”; T517: “proof_missing; backup (structured fields; no reason text)”; T607: “no --worker-agent transcript holds Git” |
| Coverage floor | 3 | 1.1% | 26.46 | 3/3 | T359: “FAIL: app/payments/ 94.98 is below the floor 95.0”; T563: “FAIL: app/privacy/ 94.66 is below the floor 95.0”; T729: “FAIL: app/privacy/ 94.86 is below the floor 95.0” |
| Merge conflict (charged) | 3 | 1.1% | 1.51 | 3/3 | T437: “merging main into the task branch conflicts in app/businesses/CLAUDE.md again after 2 resyncs in this attempt” |
| Lint failure | 1 | 0.4% | 0.31 | 1/1 | T357: “Ci: failure; Lint and slow tests: failure Lint and slow tests failed at step: Rust format and lint (clippy once, before the build, D-488)   ” |
| Python failure | 1 | 0.4% | 1.19 | 1/1 | T624: “Ci: failure; Python: failure Python failed at step: Unit, contract and drill tests (pytest) T705: about 21877 tokens, 11% of the window T711” |
| Model mismatch | 1 | 0.4% | 6.05 | 1/1 | T655: “model claude-opus-5-5 ran, task names claude-fable-5-2” |
| Smoke / baseline failure | 1 | 0.4% | 5.51 | 1/1 | T669: “runner smoke red on c403ea4898ac (bash scripts/smoke.sh, D-3530):         line = cache.seed(tmp_path, smoke, debug, lambda args: (0, _metada” |

**Total known failure tokens: 1,611.86M (229/268 events).** Plan-gap events alone account for 402.03M known worker tokens; review findings account for 209.72M. The 101 unknown events carry 740.52M known tokens, so the true ordering could differ. No token amount is a dollar cost or necessarily net wasted work: some work may survive the retry.

### Secondary signals (not additive)

- `resync`: 75 records across 59 tasks; 579.23M known worker tokens across 56 events. Examples: T151 “conflicts in docs/for-casey-review.md”; T102 “conflicts in app/server/src/lib.rs, tests/a11y/page_states.ts”; T221 “conflicts in docs/architecture.md”. Combined known tokens across failed and free-resync events: 2,191.08M. Free resync means no attempt charge, not free compute or necessarily all wasted work.
- Accessibility: seven of the eight attributed Ally failures reference J1 keyboard traversal (T101/T410/T649/T804/T815/T819/T826). T362 reproduced a load-order race: “with the built .js the 3 tests fail; with the .js removed they pass; with await page.waitForLoadState("load") added after the heading check at :171 they pass.” That is evidence of a real historical timing issue, not proof that all later failures are identical. T737 is a different target/contrast failure. Keep real keyboard and contrast checks.
- Coverage is not synonymous with coverage floor: T311's coverage run failed a test, whereas T359's reported 94.98 is below 95.0; T727 was cancelled. T615 explicitly mentions a flaky TD-INT-47 repaired by T789. Do not claim every E2E or coverage failure is a flake. Gate tags among failed records (overlap):

- `Ci`: 68
- `Checks`: 40
- `Coverage`: 28
- `Ally`: 20
- `E2e`: 16
- `Lint and slow tests`: 5
- `Python`: 3
- `backup`: 2
- `Twin`: 2
- `Governance`: 1

## 2. Top six prevention changes (ranked by opportunity)

Estimates below are conservative scenario assumptions applied to assigned events, not measured treatment effects. They count events that could be avoided as failed worker attempts, not elimination of all engineering work. Categories are mutually exclusive for the estimate. No floor, protected-path rule, review, or independent gate should be weakened.

### 1. Plan-gap preflight — overseer/planning rule (85 events; estimate 42–60 saved)

Evidence: T099 missing shared signing secret, T205 missing public reader, T801 stale consent assertions in another owner's tests, T836 contradictory CI assertion, T408/T583 budget mismatch. These cannot be repaired by a more capable model alone. T295 was blocked before worker execution; a preflight must be earlier than claim, not simply another worker instruction.

**Insert in `TEMPLATE.md`, `## Scope`, after the size-budget paragraph:**

> Release preflight (overseer, before claim): pin the base SHA and validate the task's goal, acceptance IDs, named tests, decisions, ownership, shared-path restrictions, required API/data/fixture prerequisites and size budget against that SHA. Record each required dependency and each existing assertion the change will invalidate, with file:line and owner. If any required capability or permitted edit is missing, or two binding assertions conflict, do not release; create the smallest logged plan correction or prerequisite task and regenerate the prompt. Do not grant blanket ownership or weaken tests. The worker receives a short preflight certificate and rechecks only changed prerequisites on claim. A hard implementation is not a plan gap.

**Mechanical detection:** build a reverse map from requirement/test IDs and file owners; scan known call sites, constructor shapes, architecture boundaries, shared fixture env keys and protected-test assertions; compare needed edits against owns/shared, current dependency state, and budget. Use a read-only semantic reviewer for ambiguous contradictions; do not let it auto-rewrite spec. Replay named tests on base to distinguish known-red infrastructure. Invalidate certificate when relevant files or dependency heads change. Block on proven contradiction, flag uncertain cases for focused review. Split T408/T583-like cases by independently testable boundaries; raise budgets only through approved plan changes. Estimate 50–70% of 85, rounded down at lower bound; cross-module and explicit contradictory-test examples make this plausible, but discovery is imperfect. Risk: release latency and false positives; cache SHA-keyed evidence and use a short human-readable certificate.

### 2. Acceptance evidence before final review — prompt + planning (23 events; estimate 9–14 saved)

Evidence: T104 passed smoke/prehandin but still had findings; T222 claimed Spanish journey coverage incorrectly; T743 misses alternate digest encodings; T834 omitted required workflow cases. One-fix-round policy alone cannot substitute for complete first implementation.

**Insert after `## Run your tests` and `${run_tests}`:**

> Before final hand-in, make a compact acceptance matrix: every required sentence/TD ID -> implementation path:line -> positive and negative test path:line -> actual command and result. For privacy, money, consent and security include denial, replay, cross-owner and alternate-encoding cases when applicable. For test-only tasks, state which wrong implementation each test would reject; run applicable named mutants. Never claim a journey is driven unless the executed test actually exercises it. Re-read all acceptance rows and all prior reviewer findings, including unchanged assumptions, before committing the fix round. Missing evidence is a failure, not a passing assertion.

Use existing D-481 mutants and a targeted independent review before expensive full gates; keep mandatory fresh-eyes approval on exact head. Estimate 40–60% of 23: omissions and weak assertions are amenable to a matrix, subtle logic issues less so. Risk: extra turns; cap the matrix to one compact row per requirement and avoid a full extra reviewer for low-risk deterministic tasks.

### 3. Ally baseline attribution and focused journey checks — prompt + harness (8 events; estimate 4–6 saved)

Evidence: T362 load-state experiment, T410/T804/T815/T826 repeated J1 traversal signatures; T737 is a separate real accessibility issue.

**Replace the `## End routine` step 1 clause:**

Existing: `A FAIL solely on untouched pages/routes: do not fix; begin report ALLY RED: <failing test> and its line; hand in.` (backticks omitted here only for readability; source has them.)

> For each Ally failure, save the test ID, browser project, viewport, exact assertion and trace. If a touched route fails, reproduce it with the exact CI browser configuration and fix the cause; preserve keyboard order, focus, target size and contrast assertions. If it appears unrelated, run the identical test against the pinned clean base with the same image/seed and record both outcomes. Do not change another owner's tests. If base is independently red or a shared test race is demonstrated, submit the evidence to the runner for FIX-MAIN/owned harness repair and resume after the repair; keep final Ally green as a merge requirement. A skipped or timed-out local suite is not a green result.

Run J1 keyboard on relevant page, route, CSS, JS and fixture changes even where a static path heuristic would miss the relationship. Use explicit page-ready conditions, not arbitrary sleeps or blanket retries. Estimate 4–6 of 8 based on repeated signatures, but only after matched-base evidence. Risk: extra browser runs; use one targeted test per signature then the mandatory final suite.

### 4. Dependency-audit freshness gate — overseer/harness (8 events; estimate 5–7 saved)

Evidence: T204/T489 audit step, T714 says RUSTSEC-2026-0594 failed every Rust change, fixed on main by #1396. These are not proven worker defects. T115 separately falls in plan gaps due to an npm advisory and forbidden path.

**Insert in `## Start routine` after step 3:**

> Before claim, the runner checks dependency audits for the exact base lockfiles using current advisory data and the CI audit policy. Cache by lockfile hash, tool version, advisory snapshot and short expiry; refresh when advisories change. If a base advisory blocks unchanged lockfiles, stop affected claims and route a single FIX-MAIN or owned dependency repair. Workers changing dependencies still run their own audits. No automatic advisory suppression; only a separately approved, narrowly scoped and time-bounded exception with rationale is allowed under the existing governance rules.

Estimate 5–7 of 8 if common baseline advisories are detected before fanout. Risk: advisory fetch outages or stale cache; record audit snapshot and distinguish network failure from vulnerability. Never let a stale green override a current red.

### 5. E2E contract precheck — planning + prompt (8 events; estimate 2–4 saved)

Evidence: T320 confirm double charge/terms row journey, T373 charge help journey, T741 high-alert step, T099/T119 missing fake-provider configuration (plan-gap examples, not counted twice).

**Insert immediately before `## Run your tests`:**

> For a journey-affecting task, list the routes, seeded rows, fake provider settings, clock controls, signed fake webhooks and expected side effects required by each changed or dependent journey. Confirm those capabilities exist at the claim base. Run each impacted journey against that base to identify existing red tests; after implementation run the same named journey with CI-equivalent environment and assert both user-visible behavior and durable closing state. Report exact test IDs and traces. No fake green, live provider use, arbitrary sleeps or hidden retries.

Preflight detects fixture ownership gaps; targeted execution catches behavior regressions earlier. Estimate 2–4 of 8 because these are failures, not proven test flakes. Risk: slow startup; reuse a controlled ephemeral test environment keyed by base/head and reset state between tests.

### 6. Separate coverage test execution from floor comparison — prompt/harness (7 events; estimate 1–3 saved)

Evidence: T212 `confirm_window`, T311 `call_pins`, T414 `link_request`, T615 flaky TD-INT-47. T359/T563/T729 are true floor misses and are a distinct cluster; T727 is cancellation.

Existing `## Definition of done`: `Ci branch coverage for touched crates with floors in tests/coverage_floor.json` (excerpt of actual rule; file name is backticked in source). Keep this rule, and **append:**

> Run the exact coverage-producing test targets for touched crates and dependent integration/server tests early, using the pinned coverage toolchain and the CI command from the checked-out workflow. Record separately (a) instrumented test pass/fail, (b) successful coverage report generation, and (c) overall, crate and per-file floor comparison against tests/coverage_floor.json. Never treat a partial report as passing or lower a floor to get green. If the instrumented test fails, diagnose the named assertion before adding coverage tests. If the baseline fails under identical instrumentation, route to the owner or FIX-MAIN; final coverage still must pass.

Do not invent an llvm-cov CLI invocation: the workflow and script contents are absent from the package, so derive the exact command from the real repo at implementation. Reserve a deterministic coverage check for the end of the worker loop, and add branch tests for actual uncovered branches, not only line coverage. Estimate 1–3 of 7; ordinary test bugs remain engineering work, and the additional 3 floor events offer separate upside not included in the estimate. Risk: duplicate compile/test expense; share instrumentation artifacts only under compatible toolchain/feature/target/head keys.

**Combined estimate:** 63–94 charged-failure events avoided across the six clusters (midpoint ~79 as an illustrative planning target, not a prediction). This is 24–35% of all 268 failure records. It cannot be converted to a first-pass improvement percentage without successful first attempts and an eligible task denominator. Savings for free resyncs, unidentified failures and other clusters are additional possible upside, not included.

## 3. Pre-start predictors — denominator unavailable

The JSON is intentionally failure-selected. It does not include the population of tasks that succeeded first try. A true first-attempt failure rate by any bucket would require all eligible first claims with their pre-claim metadata. The following tables are **sample composition**, not risk rates. “First charged-failure record” means the first logged event was a result other than done/resync, not necessarily the current task's final attempt count. Each dimension is separate; rows across dimensions overlap. `review_areas` is multi-label. Historical metadata may have been changed by plan fixes; the export is not a claim-time snapshot. Dependency counts are absent.

| Dimension / bucket | Selected tasks | First event charged failure | First event free resync | Failure rate in all tasks |
|---|---:|---:|---:|---|
| Context budget: 21–30% | 61 | 50 | 11 | unavailable |
| Context budget: ≤10% | 14 | 14 | 0 | unavailable |
| Context budget: 31%+ | 47 | 40 | 7 | unavailable |
| Context budget: 11–20% | 76 | 59 | 17 | unavailable |
| Owned files: 4–6 | 80 | 63 | 17 | unavailable |
| Owned files: 1–3 | 63 | 55 | 8 | unavailable |
| Owned files: 7+ | 55 | 45 | 10 | unavailable |
| Changed lines: 351–600 | 125 | 99 | 26 | unavailable |
| Changed lines: ≤350 | 52 | 46 | 6 | unavailable |
| Changed lines: 601+ | 21 | 18 | 3 | unavailable |
| Type: harness | 66 | 59 | 7 | unavailable |
| Type: build | 126 | 98 | 28 | unavailable |
| Type: cleanup | 2 | 2 | 0 | unavailable |
| Type: docs | 4 | 4 | 0 | unavailable |
| Stage: 4 | 70 | 49 | 21 | unavailable |
| Stage: 5 | 128 | 114 | 14 | unavailable |
| Module: portals | 23 | 19 | 4 | unavailable |
| Module: (none) | 36 | 34 | 2 | unavailable |
| Module: messaging | 11 | 7 | 4 | unavailable |
| Module: site | 21 | 20 | 1 | unavailable |
| Module: ops | 4 | 4 | 0 | unavailable |
| Module: recovery | 2 | 2 | 0 | unavailable |
| Module: picks | 11 | 9 | 2 | unavailable |
| Module: intake | 19 | 17 | 2 | unavailable |
| Module: quotes | 7 | 6 | 1 | unavailable |
| Module: businesses | 13 | 7 | 6 | unavailable |
| Module: catalog | 1 | 0 | 1 | unavailable |
| Module: server | 34 | 25 | 9 | unavailable |
| Module: payments | 10 | 9 | 1 | unavailable |
| Module: calls | 2 | 2 | 0 | unavailable |
| Module: privacy | 2 | 1 | 1 | unavailable |
| Module: licensing | 2 | 1 | 1 | unavailable |
| Configured model: claude-opus-5-5 | 178 | 144 | 34 | unavailable |
| Configured model: claude-sonnet-5-5 | 20 | 19 | 1 | unavailable |
| Review area count: 3 | 57 | 47 | 10 | unavailable |
| Review area count: 2 | 71 | 60 | 11 | unavailable |
| Review area count: 4 | 20 | 18 | 2 | unavailable |
| Review area count: 1 | 46 | 34 | 12 | unavailable |
| Review area count: 5 | 1 | 1 | 0 | unavailable |
| Review area count: 0 | 3 | 3 | 0 | unavailable |
| Review area: accessibility | 34 | 33 | 1 | unavailable |
| Review area: compliance | 51 | 43 | 8 | unavailable |
| Review area: docs | 2 | 2 | 0 | unavailable |
| Review area: harness | 53 | 48 | 5 | unavailable |
| Review area: licensing | 10 | 8 | 2 | unavailable |
| Review area: money | 51 | 43 | 8 | unavailable |
| Review area: privacy | 112 | 91 | 21 | unavailable |
| Review area: recovery | 7 | 7 | 0 | unavailable |
| Review area: security | 124 | 97 | 27 | unavailable |

**Signals worth collecting prospectively:** unresolved ownership, missing fixture capabilities, changes to shared files, repeated spec/test contradictions, known-red dependency advisory, low remaining branch-coverage headroom, projected CI critical path versus timeout, and task budget versus actual dependency surface. Examples: T408/T583 budget, T099 fixture, T836 contradiction, T714 advisory, T359 floor headroom, T803 timeout, T437 shared-file contention. These are causal candidates, not calibrated predictors. Log task metadata at claim (including deps, actual model/override, exact SHA and baseline gate status) for *every* task. Compare first-pass rates by bucket with numerator/denominator, Wilson intervals and time/stage controls; do not change model assignment based on this export.

## 4. Quick wins vs larger changes

1. **Immediate:** stop new claims with known-red baseline audits or repeated shared test failures; route to one repair owner (T714/T362). Preserve all gates.
2. **Immediate:** small acceptance matrix and prior-findings checklist (T222/T743/T834); no broad extra reading.
3. **Immediate:** record event-linked reason, stage, failed job/step, exit/cancel outcome, head/base SHA, tokens, provenance and retry decision on every attempt. Start with the ledger's unknown 101; T221/T202 illustrate overwritten reasons. Write append-only, never overwrite reason history.
4. **Next:** ownership/API/fixture/contradiction preflight, first on money, consent, security, shared tests and high-dependency work (T099/T205/T801/T836). Expand after measuring precision.
5. **Next:** targeted browser and instrumented coverage parity; disk/runtime budget before starting heavy checks (T820/T803). Review existing fixes before duplicating T770/T832/T789.
6. **Larger:** shared-path merge contention reduction and failure-accounting state machine; measure resync cost separately (T437/T151/T102). Follow code-docs §2/§9 rather than weakening docs standards.

## 5. Systemic runner findings

- **Confirmed charged conflict records:** T437 has three `update_conflict` events; note says “again after 2 resyncs in this attempt, so this attempt fails”. It also says “false 'reverted by FIX-MAIN' failure; T437's work is on main”. Verify merge reconciliation against exact commits and required check proofs. Use narrow serialization for overlapping shared-file regions, generate append-only entries, and avoid global reformatting under in-flight work (existing code-docs §2/§9). Free resyncs should stay separate from worker defects. The package does not prove all 75 resyncs can be avoided.
- **Infrastructure charged or requeued:** T384 DNS outage before Chromium installation; T714 advisory affecting all Rust changes; T727 “Coverage: cancelled”; T803 Checks timeout; T573 coverage time-limit fix. Build brief §16 already permits one extra attempt for approved plan or external setup repair (D-677/D-3031), but does not make these costless. Add distinct dispositions `BASE_RED`, `INFRA_BEFORE_TEST`, `TIMEOUT_UNCLASSIFIED`, `PLAN_INVALID`, `WORKER_DEFECT`, `REVIEW_REJECTED`, `PROOF_MISSING`, `MERGE_CONFLICT`, and retain `UNKNOWN`. A verified non-worker fault should hold the lane or resume the same attempt after repair under a logged policy change; never automatically exempt failed assertions or self-declared plan gaps. A cancelled job is incomplete, not passed; use exact CI conclusion and cancellation initiator, not a single aggregate red bit. No merge before full required proof on the valid head. Existing policy changes need a decision and tests.
- **Storage/deadlines:** T820 “full integration+portals+server measurement never finished (disk full)”; T714 “start smoke test did not finish within the 30-minute background limit”; T266/T312/T691 “session ended without handing in”. Require disk free-space and projected target growth checks before large jobs, separate cache quotas, per-task log paths and reserve hand-in time based on measured p95 gate runtime. Preserve a reviewed-head checkpoint and retry only unfinished work with verified artifacts. No unrestricted deletion of shared build targets.
- **Provenance is a mechanical issue as well as a worker issue:** T100/T517 `proof_missing` with backup check; T607 “not proven reviewed or worker-made”. The template already requires plain `git commit` transcript and reviewed head. Automate capture of commit SHA/transcript reference and review head, verify before PR, and recover missing transport metadata without claiming unproven review. No proof bypass.
- **Actual model mismatch:** T655 “model claude-opus-5-5 ran, task names claude-fable-5-2”; T573 and T615 mention the same mismatch/fallback. Build brief §13 expressly allows Opus fallback for Fable refusal/limit. Record requested, effective and fallback reason separately, resolve mismatch preclaim; do not infer that a refusal occurred in any specific attempt absent provider evidence or change the configured tier from this biased export.
- **Observability is inadequate:** T221/T202 latest reasons are resyncs although earlier CI failures exist. Store immutable attempt-event IDs even when numeric attempts repeat. Require actual gate-level outcomes, timestamps, base/head, worker tokens, reviewer tokens, retry cause and reason history. Treat event charges and actual consumed tokens independently. Suggested regression fixtures: duplicate event ingestion, partial cancellation, known-red baseline, overlapping shared edits, missing proof, wrong model fallback, and repeated plan-fix requeue. Assert idempotence, no attempt forgiveness for unverified worker faults, no double-count, and no unreviewed head merge.

## Can't tell

- True first-pass rate by model, budget, type, stage, dependency count, module or review area; all successful first attempts are missing. The README's 61% is not verifiable here.
- Exact causes of 101 unattributed events, timing of all note-to-attempt joins, or whether repeated failures had the same cause. The ledger marks heuristic joins rather than presenting them as fact.
- Whether any particular historical baseline failure persists today, whether later J1 failures are identical to T362, or whether all dependency advisories are still active; only archive evidence was used.
- Number of genuinely flaky tests versus deterministic failures; T615 has explicit fix evidence and T362 an experiment, but most CI records lack matched-base retries.
- Total tokens including null worker counts, reviewer and runner tokens, cached input weighting, or actual financial spend; cannot infer from worker_tokens alone.
- Exact count of charged attempts from state-counter semantics, effect of late plan changes on historical budgets, and whether cancellations were timeout, superseding push or external interruption unless notes say so.
- Exact current CI command lines, implementation locations or whether proposed harness changes duplicate current code; the archive includes only TEMPLATE.md, code-docs.md and the quality/runner excerpt, not runner source, task graph or full CI workflow. No repository tests were executed.

**Implementation priority:** evidence-linked preflight and immutable failure ledger first. Retain all existing security, money, privacy, licensing, accessibility, coverage, Twin and fresh-eyes gates. The goal is preventing impossible work and avoiding duplicate failed runs, not redefining failure away.
