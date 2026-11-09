# Fifth repository audit of November 9, 2026: dispositions

**Input:** a fifth external, static review of `main` at `72a168d5` (`ask_hava_github_code_audit_2026-11-09.md`, items AH-001 to AH-020), shared by Casey and treated as untrusted input. A verifier checked each item against the code for the overseer; this plan change spot-checked every file and line it acts on against `main` at `72a168d5`. Decisions D-2263 to D-2268.

Verdicts: **new** (real, and no task owned it), **already owned** (a fourth-audit finding with a queued task). The audit's overall verdict, "not approved for production or real customer/money traffic", matches the plan: nothing goes live before Gate C.

## Items

| Item | Verdict | Evidence on `main` 72a168d5 | Disposition | Owner | Decision |
|---|---|---|---|---|---|
| AH-001 (P0) Task kinds queued with no handler in the production worker | New | `register_handlers` (`app/server/src/lib.rs:339`) and `app/server/src/bin/worker.rs:50` register no payments kind but `payments.pause_watch`; `payments.charge_check`, `pay_timer`, `refund_retry`, `refund_check`, `late_success_refund`, `dispute_submit`, `reconcile`, `pick_ended` (no handler function), `messaging.charge_receipt`, `calls.safety_call`, `picks.pick_withdrawn` and `intake.opted_out_live_job` (no handler functions) and eight `businesses.*` kinds are queued with no handler; `NO_HANDLER` (`app/core/src/tasks.rs:395`) dead-letters silently; IC-27's owners never had the duty | A CI check that every queued kind is registered or deferred to a named owner, plus a High alert on any no-handler dead letter; every kind given one owner; the Confirm route waits for the payments jobs | T327 (check), T328 (payments, `pick_ended`, reconcile seed, `late_success` caller), T329 (`businesses.*`), T313 (`charge_receipt`), T330 (`pick_withdrawn`, `opted_out_live_job`, `safety_call`); T261 depends on T328. Already owned: `messaging.business_notice` T313, `picks.fraud_warning` T281, `picks.address_needed` T255, `intake.cards_approved` and `intake.emergency_released` T251, `businesses.welcome` and `phone_stopped` T268, `help_request_opened` T151 | D-2264 to D-2267 |
| AH-002 (P0) Pages and actions with no live route | Already owned | F1 | No change | T300 to T310, T261, T231, T280, T284 | D-2263 |
| AH-003 (P0) Read-back can connect a different line | Already owned | F39 | No change | T311 | D-2263 |
| AH-004 (P1) A Stripe success with no readable `pick_id` is acknowledged silently | New | `dispatch_of` (`app/payments/src/routes.rs:254`), `pick_of` (`:290`), 200 for no dispatch (`:170`); a logged choice in D-2073 (1) | `Dispatch::Unreadable`: event row, High alert, 200; never a release from untrusted metadata | T331 (new, harness) | D-2268 |
| AH-005 (P1) An early fraud warning with no evidence row is dropped | New | `Acted::Unknown` and rollback (`routes.rs:417`, `:534`), 200 (`:333`); D-2072, D-2073 (3) | A warning matching `payments.pick_intent` but no evidence row answers 500 with one High alert; one matching no pick intent commits its row with a High alert | T331 (new, harness) | D-2268 |
| AH-006 (P1) Inbound SMS errors committed and answered 200 | Already owned (in part) | F25 | T325's body-free alert stands; a replay path is not taken (D-801 keeps no body to replay) | T325 | D-2263 |
| AH-007 (P1) Client IP from an unverified edge header | Already owned | F2 | No change | T135 | D-2263 |
| AH-008 (P1) Alerts stored but not delivered | Already owned | F28 | No change | T204 | D-2263 |
| AH-009 (P1) Emergency notices held to morning | Already owned | F42, F43 | No change | T313, T251 | D-2263 |
| AH-010 (P1) Dispute state not atomic | Already owned | F44 | No change | T314, T205, T304 | D-2263 |
| AH-011 (P1) Retention and deletion holes | Already owned | F31, F32 | No change | T315, T321 | D-2263 |
| AH-012 (P1) Security-rejection rows amplify a flood | Already owned | F14 | No change | T319 | D-2263 |
| AH-013 (P1) Rate limits not invoked | Already owned | F12 | No change | T317, T280, T301, T302, T303, T307 | D-2263 |
| AH-014 (P2) Wrong job view fallback or panic | Already owned | F23 | No change | T318 | D-2263 |
| AH-015 (P2) Lower-case street addresses trip the emergency check | Already owned | F24 | No change | T312 | D-2263 |
| AH-016 (P2) AI daily cap check-then-send | Already owned | F40 | No change | T236 | D-2263 |
| AH-017 (P2) CSP blocks Stripe on the Confirm page | Already owned | F35, F36 | No change | T261 | D-2263 |
| AH-018 (P2) Security maintenance jobs and alert dedupe | Already owned | F5, F7, F30 | No change | T319, T320, T204 | D-2263 |
| AH-019 (P2) Public business identity and notice gaps | Already owned (in part) | F17 | No change; the quote-card part stays IC-71 with no owner | T316, T317, T318; IC-71 unowned | D-2263 |
| AH-020 (P2) Coverage floors | Already owned | F38 | No change | T324 | D-2263 |

## Plan changes made

- New tasks T327 (harness), T328, T329, T330, T331 (harness); T313 also registers `messaging.charge_receipt`; T261 depends on T328; T100 depends on T327 to T331.
- IC-27 now names T327 with TD-OPS-36 and TD-OPS-37; register rows IC-75 to IC-78 and risk R-33 added.
- The overseer runs an independent money review of T328 and T331 when each hands in.
- No task IN_PROGRESS or MERGE_PENDING was edited (T283 was MERGE_PENDING).
