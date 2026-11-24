> Outside review PF-E2E-CLOCK (ChatGPT), received from Casey Nov 24, 2026 (about 10:45 am MST). Verified by an Opus pass on `main`: no finding needs a fix now (see D-5809); kept for later harness hardening.

# PF-E2E-CLOCK — Preflight audit

**Verdict: CONDITIONAL FAIL — additional harness risks require attention.**

The most important finding beyond D-5809 is that **J1 advances the shared clock while running in the main Playwright project**, despite the configuration explicitly intending to run clock-advancing journeys last. That exposes subsequent tests to an already-advanced application clock while they continue inserting database records using real time.

The package also contains other randomly generated licence fixtures vulnerable to the same daily-list replacement problem previously identified in D-5657.

These are test-harness problems, not established production application defects.

## 1. Findings count

| Classification | Count |
|---|---:|
| Confirmed additional harness defect | 1 |
| Conditional scheduled-worker risks | 3 |
| Previously identified D-5809 risk | 1 |
| Additional payment-timer investigation | 1 |

The three conditional risks should not be treated as reproduced failures.

## 2. Risk table

| # | Spec and seed | Clock move / worker task | Assertion at risk | Likelihood | Smallest fix |
|---|---|---|---|---|---|
| 1 | `j1_homeowner.spec.ts:118–139` — randomly generated ROC licences | Clock jump at `:204`; daily licence-list replacement | Quote display and pick assertions at `:277–305` | Medium; downstream eligibility handling needs verification | Use stable HVAC licences present in the actual fixture list. |
| 2 | `j1_homeowner.spec.ts:310` — STOP variant creates another pair of randomly licensed businesses | Inherits the earlier jump; a delayed licence-list run can execute after seeding | Two recorded cards and two visible quotes at `:314–320` | Medium | Same stable licence fix; check eligibility before sending the request. |
| 3 | `intake_order_paths.spec.ts:37–58` — randomly licensed HVAC businesses | Can inherit J1's jump if executed afterward; daily licence-list replacement | Coverage count at `:113`, card count at `:124`, emergency job status at `:133` | Low to medium; depends on execution order and list timing | Stable fixture licences; seed time-sensitive emergency fields at application time. |

### Why the licence risks are credible

`app/licensing/src/holder.rs:245–254` explicitly reads active classes from the latest ROC list.

A randomly generated licence added to the current database list is therefore not necessarily present in the next imported fixture list.

Merely changing `checked_at` to the application clock does **not** solve this problem. The licence itself must survive the next list import.

The actual ROC fixture and daily-list handler are absent from this package, so I cannot verify which HVAC licence numbers are safe to substitute.

## 3. Confirmed test-ordering defect

**Priority: P1 — fix before further large test batches.**

`tests/e2e/playwright.config.ts:21` defines the late project using:

```typescript
const LATE = [
  "**/j1_full*.spec.ts",
  "**/j2_full*.spec.ts",
  "**/j1_j2_web_email*.spec.ts",
  "**/j3_*.spec.ts",
  "**/j4_*.spec.ts"
];
```

However, `j1_homeowner.spec.ts` matches none of those patterns.

At `j1_homeowner.spec.ts:204`, the test conditionally advances the clock to 8:30 a.m. Arizona time, potentially moving it approximately 12.5 hours forward.

The clock cannot move backward (`app/core/src/test_control.rs:12–15`).

This means the configuration's stated protection against premature fixture expiration is incomplete.

### Additional affected seeds

| File | Real-time dependency |
|---|---|
| `feedback.spec.ts:114` | Job intentionally aged to 29 days, 23 hours |
| `action_links_served.spec.ts:147–150` | Rate-limit buckets based on the current and next real-time hour |
| `business_portal_served.spec.ts:65` | 15-minute login link |
| `action_done_served.spec.ts:38` | Two-hour action link |
| `keyed.ts:85` | One-day business session |
| `j1_homeowner.spec.ts:168` | Visit date calculated using `Date.now()` |

These are clock-domain hazards, not all proven worker races. Some routes intentionally use the system clock.

**Recommended fix:** Put J1's clock-advancing journey into an explicitly ordered late project, and ensure subsequent late journeys use correctly clocked fixtures. Moving the file alone does not eliminate clock contamination between late journeys.

## 4. D-5809 fix should be extended carefully

The existing diagnosis is supported by the supplied code.

`app/jobs/src/timer_backstop.rs`:

- `:62–67` — schedules the daily backstop.
- `:89–108` — selects non-final jobs.
- `:125–128` — calculates whether a current timer is required.
- `:145–147` — creates a missing timer from the job's stored `timer_from`.

`app/jobs/src/rules.rs:27` sets the Confirm window to four business hours.

`tests/e2e/keyed.ts:142–143` creates the awaiting-confirm job using real time.

The combination makes the D-5809 race credible.

I would make the application-time parameter available to every `seedPick` caller, while preserving the correct system-clock timestamps for authentication sessions. This reduces the chance that a future journey introduces the same problem.

## 5. Additional payment-timer investigation

**Status: Cannot confirm from this package.**

J3 step 5 contains another timing-sensitive sequence:

| Operation | Location |
|---|---|
| Create and pay the held pick | `j3.ts:355` |
| Advance the clock by 24 hours | `j3.ts:308,357` |
| Deliver Stripe success | `j3.ts:358` |
| Assert release remains held | `j3.ts:359–360` |
| Assert release eventually occurs | `j3.ts:370` |

The Payment pending timeout is one business hour (`jobs/rules.rs:29`).

However, `jobs/service.rs:269–273` delegates that timeout to `queue_payment_returned` rather than directly moving the job.

The downstream payment handler determines whether the paid pick remains protected. That implementation is missing from the archive.

This is worth checking, but it is not another confirmed instance of the raw-SQL seed defect.

## 6. Safe seeds and exclusions

The following are safe from the identified race under the stated conditions, or have no demonstrated failing assertion in the supplied execution sequence.

| Seed or scheduled task | Reason |
|---|---|
| J3 High alert, `j3.ts:290–295` | Already uses `control.now()`. |
| J3 owner-session aging, `j3.ts:66–75` | Intentionally operates against the system-clock authentication domain. |
| J1 quote login links, `j1_homeowner.spec.ts:155–161` | Already seeded relative to application time. |
| `keyed.ts:127–128` licence identity | Uses ROC100001, identified as stable by D-5762. |
| `j2.ts:30–36` licence identity | Uses the same stable ROC100001 fixture. |
| J3 step 3 refund seed, `j3.ts:226` | Refund assertions execute before J3's explicit 24-hour advance. |
| J3 step 4 authorization seed, `j3.ts:272–284` | Authorization refusal is checked before the explicit jump. |
| Job purge sweep, `jobs/purge_events.rs:31–32` | Retention periods are 30, 180 and 1,096 days. Fresh fixtures do not reach them during these jumps. |
| Masked-line sweep, `calls/close_tasks.rs:85–101` | Requires 30-day-old lines. |
| Consent purge, `intake/consent_purge.rs:172–183` | Five-year retention period. |
| Rate-limit pruning, `core/rate_limit.rs:301–306` | Deletes windows older than 48 hours; distinct from the incorrect-hour bucket problem. |
| Licence-map reapply, `licensing/class_changes.rs:70–75` | Numbers absent from a replacement list are explicitly excluded from class-change processing. |
| Licence record resume, `businesses/record_resume.rs:165–166` | Requires a non-null `waiting_on_record_since`; ordinary `seedPick` businesses do not seed that state. |
| Hourly request counts, `jobs/request_counts.rs:63–79` | Reads job counts without changing job status. |
| JavaScript polling deadlines | Must remain on real elapsed time. |
| Ordinary accessibility page-state fixtures | Their main project runs before J3's clock advance. |
| Historical authorization and refund records | No matching destructive worker selection established in the supplied code. |

## 7. Recommended corrective tasks

### P1 — Clock-advancing project isolation

Correct J1's project placement and add a check that every spec calling `advanceClock` belongs to an appropriate project.

### P1 — Stable licence fixtures

Replace randomly generated HVAC licences in both seed helpers with deterministic, valid fixture records that survive daily imports.

### P2 — Clock-aware seed helpers

Standardize application-time timestamps for lifecycle records, keeping system-time security records separate.

### P2 — Payment timeout verification

Inspect the payment-returned and held-release handlers against J3's 24-hour jump.

## 8. Audit limitations

The archive omits several implementations needed to certify completeness:

- Portal session sweep.
- Daily licence-list import and recheck.
- Stripe reconciliation and payment-returned handlers.
- Deliverability retest selection rules.
- Safe-mode worker implementation.
- Actual licence fixture data.

This was a static source review, not an executed Playwright test run. I cannot certify that every other scheduled-worker race has been eliminated.

**Final recommendation:** Fix the shared-clock ordering and fixture design rather than continuing to repair each individual flake after it consumes another build attempt. Keep every existing assertion intact, add no retries, and leave production timing rules unchanged.
