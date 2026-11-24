> Outside review PF-Q1 (ChatGPT), received from Casey Nov 24, 2026 (about 6:20 am MST). Verified by two Opus passes on `main` 26e9bbe3: T817 findings 1 and 4 real, 2 partly (bidi, isolate and tag characters), 3 not real (the sender re-checks quiet hours per row, `app/messaging/src/outbox_sender.rs:285`, `:383`); T843 findings 2 and 4 real, 1 partly (fail-closed hardening), 3 not real. Fixes: D-5793.

# PF-Q1 — Six-task preflight review

**Recommendation: Hold T817 and T843 for plan corrections.** The other four tasks have no confirmed new implementation blocker in the supplied snapshot, although T123 must wait for its existing dependencies.

Reviewed against the packaged `main` snapshot at `16fd7ee8`, including the generated prompts, task dependencies, relevant implementation files, specifications and existing tests.

## 1. Task verdicts

- Tasks reviewed: 6
- Tasks requiring plan corrections: 2
- Ready now: 3
- Waiting on existing dependencies: 1

| Task | Scope | Verdict |
|---|---|---|
| T817 | Consent-send races | GAP |
| T778 | Stuck tasks and health checks | READY |
| T843 | Runner plan-half smoke | GAP |
| T123 | Owner guide | DEPENDENCY WAIT |
| T799 | Copy-review fixes | READY |
| T749 | Retire unused template | READY |

## 2. Consolidated findings

Paths are relative to the supplied repository unless prefixed with `prompts/`.

| Task | Finding | Evidence | Severity | Smallest plan fix |
|---|---|---|---|---|
| T817 | Two-pass zero-width-space normalization misses phrases containing both hidden separators and hidden characters inside words. | `prompts/T817.md:9`; `app/messaging/src/rules.rs:405–419, 505–515, 586–603` | Blocker | Match listed phrases with each U+200B independently treated as an optional separator. Add mixed-placement tests. |
| T817 | Other invisible formatting characters remain capable of breaking revocation phrases. | `prompts/T817.md:9`; `app/messaging/src/rules.rs:586–603`; `docs/spec/consent.md:14,39` | Blocker | Define complete normalization coverage for relevant invisible formatting characters, including bidirectional controls. |
| T817 | A claim delayed across the quiet-hours boundary can use a stale send authorization. | `app/messaging/src/optout.rs:413–427`; `app/messaging/src/service.rs:279–311` | Blocker to verify | Require fresh quiet-hours authorization after a delayed claim, preserving legitimate exceptions. Confirm sender ownership before assigning the fix. |
| T817 | The cited location of `screen` has drifted. | `prompts/T817.md:9`; `app/server/src/inbound_text.rs:966–975` | Minor | Update the pointer from approximately line 867 to line 966, or reference the function by name. |
| T843 | Repeated same-head reuse loses the plan-half base identifier when the cycle records the reuse. | `runner/cycle.py:1577–1580`; `runner/state.py:515–525`; `prompts/T843.md:9` | Blocker | Preserve base provenance through repeated reuse, or explicitly reconstruct it and fail closed when proof disappears. |
| T843 | A plan-half result can become stale or doubtful between execution and consumption, yet still be accepted as green. | `runner/github_env.py:1376–1379,1547–1558`; `runner/cycle.py:1572–1576` | Blocker | Revalidate the full-smoke base, age, intervening commits and doubts before accepting the partial result. Otherwise run the full smoke. |
| T843 | Existing ancestry reuse can skip the entire smoke for a plan-only commit that also qualifies under D-3156. The new acceptance wording does not clearly distinguish this case. | `runner/github_env.py:1579–1581,1592–1629`; `runner/state.py:587–604`; `prompts/T843.md:11` | Minor | Explicitly state whether existing proven-merge reuse takes priority over plan-half execution. |
| T843 | Workflow line references have drifted. | `prompts/T843.md:9`; `.github/workflows/backup-runner.yml:412–422,440–465` | Minor | Update references to the named workflow steps and actual timeout setting. |

## 3. Blocker explanations

### T817 — Consent-send races

**The proposed invisible-character fix is incomplete.**

Consider this synthetic incoming text, using escaped Unicode characters:

```text
stop\u200Btex\u200Bting me
```

The normalization proposed in the prompt produces two readings:

| Normalization | Result |
|---|---|
| Remove every U+200B | stoptexting me |
| Replace every U+200B with a space | stop tex ting me |
| Intended reading | stop texting me |

Neither proposed reading matches the existing revocation phrase. The homeowner's opt-out could therefore be missed.

Use a bounded, phrase-aware matcher rather than generating every possible combination of characters. This also avoids introducing exponential processing overhead.

Add regression tests for mixed invisible-character placement, English and Spanish phrases, and ordinary messages that must not trigger an opt-out.

**Quiet-hours qualification:** The supplied code checks quiet hours before the claim through the production sender, rather than immediately before carrier dispatch. A blocked claim could therefore cross the permitted sending boundary. The package does not contain `outbox_sender.rs`, so the full behavior cannot be verified.

The residual race between the final suppression-list read and the carrier call is explicitly acknowledged and accepted in D-5388. I am not counting that as a new finding.

T817 must also wait for T741, which is already correctly declared as a dependency (`tasks_excerpt.json:53`).

### T843 — Runner plan-half smoke

**The important issue is whether partial verification remains distinguishable from full verification throughout its lifetime.**

The proposed implementation records its provenance through the `why` field:

```text
Full green on A
    ↓
Plan-half green on B, based on A
    ↓
Same-head reuse on B
    ↓
Reuse record no longer identifies A
```

The existing cycle retains the timestamp but replaces the explanatory `why` field. It also retains only the latest 20 smoke records (`runner/cycle.py:1577–1580`).

Preserving the original timestamp prevents indefinite age extension, as the prompt intends, but does not by itself preserve proof of which full smoke justified the partial run.

The new tests should cover repeated same-head reuse, eviction of the original records, expiration while the plan-half smoke is running, a newly recorded red result before consumption, and a full-smoke fallback when proof is unavailable.

This is worth fixing before implementation. A runner optimization must never allow insufficiently verified code to be treated as fully verified.

## 4. Tasks without confirmed new blockers

### T778 — Stuck tasks and health checks

**Ready.**

The proposed fixes correspond to the actual implementation:

- The backstop currently returns immediately when the original timer is alive (`app/jobs/src/timer_backstop.rs:128–134`).
- The payment-returned task's deduplication key exists in the expected format (`app/jobs/src/payment_wait.rs:29–45`).
- The worker currently reaps expired tasks only on an empty claim (`app/core/src/tasks.rs:559–570`).
- Queue health lacks the `attempts < max_attempts` filter used by the claim operation (`app/core/src/queue_health.rs:30–35`; `app/core/src/tasks.rs:205–211`).
- Database failure and queue-read failure currently produce the same health state (`app/server/src/healthz.rs:100–112`).

D-5526 already addresses the fully occupied worker problem. No additional ownership expansion appears necessary in the supplied files.

### T123 — Owner guide

**Dependency wait.**

The existing dependency declarations correctly prevent premature execution:

| Dependency | Status |
|---|---|
| T305 | BLOCKED |
| T815 | QUEUED |
| Remaining 20 direct dependencies | DONE |

Evidence: `tasks_excerpt.json:205–227`.

The guide should identify `ASKHAVA_ALERT_EMAIL` as the configured alert address rather than inventing or embedding a private production address (`app/server/src/lib.rs:1546–1592`).

I cannot verify the complete owner-route inventory because the supplied architecture document is truncated before section 13. The full transitive dependency graph is also unavailable.

No new confirmed plan defect.

### T799 — Copy-review fixes

**Ready.**

I independently applied the proposed replacements in memory and reproduced the Rust segment-counting algorithm for both Laftly and Testbrand.

All 20 changed language-template combinations fit their proposed segment budgets.

| Template | Laftly | Testbrand | Planned maximum |
|---|---:|---:|---:|
| English refund — no attempts | 2 | 2 | 2 |
| English refund — one attempt | 2 | 2 | 2 |
| English reminder with email | 3 | 3 | 3 |
| Spanish reminder | 5 | 5 | 5 |
| Spanish reminder with email | 7 | 7 | 7 |
| Spanish refund — no attempts | 6 | 6 | 6 |
| Spanish refund — one attempt | 6 | 6 | 6 |
| Spanish signup reply | 2 | 2 | 2 |
| Spanish name and address | 3 | 3 | 3 |

Evidence: `app/messaging/src/templates.rs:177–195,310–337`; `app/core/data/brand.json`; `tests/integration/tests/brand_setting.rs:50–65`.

The two explicitly authorized existing-test changes are valid:

`tests/integration/tests/unreachable_refund.rs:223` and `app/intake/src/tests/conversation.rs:289`.

The new three-hour reminder wording also agrees with TXT-026 (`docs/spec/messaging.md:30`).

No additional segment increases or test ownership changes identified.

### T749 — Retire unused template

**Ready.**

A recursive search of the supplied `app/`, `tests/` and `scripts/` files found exactly two references to `business_pick_waiting`:

```text
app/messaging/templates/en.json:132
app/messaging/templates/es.json:165
```

The registry requires matching English and Spanish keys but does not require this particular template (`app/messaging/src/templates.rs:200–217`).

Deleting both entries is consistent with the plan. Retain the existing claim-time search against the complete repository.

## 5. Recommended disposition

| Task | Action |
|---|---|
| T817 | Correct plan before claiming |
| T778 | Release |
| T843 | Correct plan before claiming |
| T123 | Wait for existing dependencies |
| T799 | Release |
| T749 | Release |

**Review limitations:** This was a static preflight review, not a full build or test execution. The package contains selected repository files, and both `architecture.md` and `cycle.py` are truncated. No repository files were modified. No outside sources were used.

The highest-priority corrections are T817's incomplete opt-out recognition and T843's preservation of full-smoke verification provenance. Both should be resolved at the plan level rather than left for the builder to discover during execution.
