# Laftly — PF-P1-2a Preflight Review

**12 tasks checked; 1 blocker, 4 majors, 3 minors.**

**Recommendation: Conditional NO-GO.** Resolve the blocker and major findings before claiming the affected tasks. Unaffected tasks can proceed subject to their dependencies and outstanding verification.

## 1. Findings

| # | Task | Severity | What breaks | Evidence (package file:line, quoted) | Smallest fix to the plan |
|---|---|---|---|---|---|
| 1 | T744 | **BLOCKER** | Acceptance requires every final job's PaymentIntent to be closed, but the plan explicitly excludes the race in which an intent has not yet been stored when the job ends. T689 owns that fix and is not a dependency. | `kit/prompts/T744.md:9`: “Case B, no PaymentIntent stored yet, is out of scope here and is T689’s.” Line 11: “no PaymentIntent of a final job stays open.” | Restrict T744's acceptance to Case A. Assign the unconditional guarantee to T689's acceptance. |
| 2 | T843 | **MAJOR** | Plan-half smoke provenance can disappear when historical smoke records are removed. A subsequent reuse could be accepted without proof that its original full-smoke base still exists. | `kit/prompts/T843.md:9`: “from the second reuse on the marker is gone.” The same goal permits an unmatched reuse timestamp to remain reusable for six hours. `runner/state.py:521`: `runs[-1].get("reused") or runs[-1].get("at")`. | Preserve plan-half provenance across every reuse, or reject a reuse whose origin can no longer be established. Add a test where both the original base and plan-half marker have left `smoke_runs`. |
| 3 | T518 | **MAJOR** | The webhook outcome layer cannot observe all the errors it is required to count. Repeated webhook timeouts could occur while the database and worker health checks remain green. | `kit/prompts/T518.md:9`: “the timeout’s 503 and a panic’s 500 come from outer layers … which a route layer does not see.” `app/server/tests/request_timeout.rs:2`: “a never-answering handler and a stalled body 503.” `docs/spec/ops-recovery.md:8`: “Stripe or webhook errors last over 30 minutes.” | Place outcome observation where it can see the final webhook response, while preserving the security layer ordering. Add timeout-generated 503 coverage. |
| 4 | T120 | **MAJOR** | The sentinel journeys are not assigned to the late Playwright project. If they use the shared test-control clock, advancing it through J1–J4 can expire other tests' sessions and links. | `tests/e2e/playwright.config.ts:11–13`: “the test clock is forward-only and shared by every binary.” Line 21's `LATE` patterns omit `sentinel.spec.ts`. Lines 38–39 assign unmatched specs to `main`. `kit/prompts/T120.md:35–36` does not grant ownership of the configuration. | Add `sentinel.spec.ts` to the late project and grant the necessary configuration ownership, or explicitly require an isolated database and clock for the sentinel journeys. |
| 5 | T120 | **MAJOR** | A panic in one synthetic handler does not establish that every production handler holding contact data satisfies TD-PRIV-13. The plan's implementation and locked test-design coverage differ. | `kit/prompts/T120.md:9`: “a test handler holding `Redacted` contact values behind the same layers.” Line 64: “A forced panic in every handler that holds contact data gives a Sentry stub event with no sentinel.” | Specify how each applicable production handler is exercised, or obtain a logged decision narrowing the test-design requirement to the synthetic layer test. |
| 6 | T823 | **MINOR** | Stale citation for the address-screening function. | `kit/prompts/T823.md:9` cites `replies.rs:316` to `:327`. Actual `app/intake/src/replies.rs:330`: `pub fn reads_as_address(raw: &str) -> bool`. | Cite `reads_as_address` by function name and update the line range. |
| 7 | T612 | **MINOR** | The font-source citation is one line off. | `kit/prompts/T612.md:9` identifies line 98 as the source declaration. `app/site/static/site.css:98`: `font-family: "DM Sans fallback";`. Line 99: `src: local("Arial"), local("Roboto");`. | Refresh the citation to line 99 and identify the face by name. |
| 8 | T843 | **MINOR** | The reuse-age citation points to unrelated code. | `kit/prompts/T843.md:9` cites `runner/state.py:528` to `:531` for reuse-age handling. Those lines now define `done_merges`. The actual timestamp and age checks are at lines 521–525. | Refresh the citation and use the `reusable_smoke` function name. |

## 2. Clean

These six tasks had no independently substantiated new findings in the supplied snapshot. This is not an unconditional execution clearance.

| Task | Description | Status |
|---|---|---|
| T533 | J1 and J2 web-and-email journeys | Clean* |
| T532 | J1 and J2 keyboard-only journeys | Clean* |
| T119 | J4 vendor outage journey | Clean* |
| T548 | Browser-tamper security tests | Clean* |
| T522 | Business text-consent page | Clean* |
| T799 | English and Spanish copy corrections | Clean* |

\* Within the supplied evidence and subject to the outstanding checks below.

The ownership lists of the 12 reviewed tasks also contain no duplicate explicitly owned file paths.

## 3. Can't tell

| Task | Outstanding verification | Required evidence |
|---|---|---|
| T533 | Whether email pick codes, web-chat consent, zero-text operation and keyboard navigation work together after the new consent chain. | T856–T865 and the resulting implementation. |
| T532 | Whether the shared journey helpers permit keyboard-only interaction without copying mouse-driven steps. | `j1_full.ts`, `j2_full.ts` and the complete accessibility Playwright configuration. |
| T119 | Whether enabling the homeowner sender breaks existing journey tests. The supplied J1 still reads outbox bodies directly at `j1_homeowner.spec.ts:88` and `:236`. | The post-consent-chain J1 implementation and full E2E results. |
| T120 | Whether every required contact-holding handler can be exercised using only the owned test files. | Complete server handlers, route architecture and sentinel security fixtures. |
| T548 | Whether the closed tamper list covers every currently served route and form. | `docs/architecture.md` section 13 and the complete route and form implementations. |
| T522 | Whether the new consent routes and changed Not a fit template satisfy the existing authentication matrix and template context. | Business layout, consent APIs, door matrix and route wiring. |
| T612 | Whether every runtime check passes against actual rendered pages, including forced colors and blocked-font fallbacks. | Complete running application and Playwright execution. |
| T799 | Whether every replacement fits its recorded SMS segment allowance. The two existing string assertions are correctly identified and owned. | `app/messaging/templates/en.json`, `es.json` and the segment counter. |
| T823 | Whether `answer_how_soon`, emergency-choice persistence and the twin can support the new behavior without further ownership changes. | `app/intake/src/service.rs`, the twin and the post-consent-chain intake implementation. |
| T843 | Whether every permitted plan-only path is safe to exclude from a full smoke, and whether the cycle preserves provenance correctly. | `runner/cycle.py`, complete smoke-reuse tests and all Rust and TypeScript plan-file readers. |
| T744 | Whether all existing exhaustive `PendingEnd` matches and SQL end-reason constraints accept the new variant. | Complete picks crate, payments implementation and migration history. |
| All | Transitive dependency and ownership conflicts against tasks outside this packet. | Full `kit/tasks.json`. |

**Packet discrepancy:** The review brief lists fewer shared paths than the actual generated worker prompts. I treated the expressly permitted shared edits in the worker prompts as authorized rather than reporting false ownership blockers. These two definitions should be synchronized.

**Review limitation:** This was a static preflight review of the supplied ZIP, not an executed build or test run. No external repository was accessed, and no source files were modified.
