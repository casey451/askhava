# Sixth audit (line-by-line delta) of November 9–10, 2026: dispositions

**Input:** Casey's sixth audit, `ask_hava_line_by_line_delta_audit_2026-11-09.md`, a read of `main` at `a4c0b6e5` (before plan-audit5, #399, merged), items U-01 to U-08, treated as untrusted input. A verifier checked each item against `main` at `978c68bf` for the overseer and found one further gap in T328; this plan change spot-checked every file and line it acts on. Decisions D-2301 to D-2307.

| Item | Verdict | Evidence on `main` 978c68bf | Disposition | Owner | Decision |
|---|---|---|---|---|---|
| U-01 (P0) Payment and refund task handlers not registered | Covered by plan-audit5 | the audit read the tree before #399 | No change | T327, T328, T332 (and T329, T330 for the other kinds) | D-2301 |
| U-02 (P1) A $0 release proof is not bound to its pick; the production $0 handoff has no owner | New | `no_fee_release` checks the caller's (business, job) only (`app/payments/src/release_proof.rs:225`); `picks::settle` does not check (`app/picks/src/settle.rs:66`); `payments.pick_fee` holds the pick's business and job since T287 (`app/payments/src/pick_fee.rs:100`) | `no_fee_release` checks the pick's own `pick_fee` row; the Confirm route's $0 handoff stated; IC-33 re-pointed. A fee bypass, not a wrong-party release | T337 (new), T261, T255 | D-2302 |
| U-03 (P1) Signed success with no readable pick dropped | Covered by plan-audit5 | AH-004 | No change | T331 | D-2301 |
| U-04 (P1) Early fraud warning lost before evidence exists | Covered by plan-audit5 | AH-005 | No change | T331 | D-2301 |
| U-05 (P1) A succeeded refund is booked at the full charge | New | `succeeded` and `append` (`app/payments/src/refunds.rs:376`, `:474`) and `own_refund` (`app/payments/src/reconcile.rs:357`) record the charged amount; `Refund` carries amount and currency (`refund_stripe.rs:53`) | Only full refunds are issued (now stated); a differing amount or a non-USD currency is booked at Stripe's figure with a High alert and no "refunded in full" notice; a missing amount is re-read once | T333 (new, harness), T334 | D-2303 |
| U-06 (P1) A failed inbound text has no recovery | New (partly owned) | T325 alerts only; Twilio keeps the body to day 31 (`app/messaging/src/purge.rs:3`); D-2284's reason was incomplete | A bounded retry (3 tries) reads the body back by SID, scrubs it, runs the steps once, never stores it | T336 (new, harness), T325 | D-2307 |
| U-07 (P2) Reconcile reads 7 days, unbounded | New | `LOOKBACK` (`reconcile.rs:57`); `list_charges` returns a `Vec` (`:63`); no checkpoint | A checkpoint, a 25-day catch-up cap with an alert past it, a paged read in bounded batches | T334 (new) | D-2304 |
| U-08 (P2) `core.rate_limit` never pruned | New | `prune` (`app/core/src/rate_limit.rs:230`) has no production caller; buckets hold client addresses | A daily batched prune, retention stated | T319 (extended) | D-2306 |
| Verifier: T328 cannot register reconcile or dispute submit | New | `ReconcileStripe` (`reconcile.rs:63`) and `DisputeStripe` (`app/payments/src/disputes.rs:42`) have only test fakes; T132 does not name them | The real-client reads, the two implementations, stand-in endpoints and contract tests, before T328 and T132 | T335 (new, harness) | D-2305 |

## Plan changes made

- New tasks: T337 (the $0 binding, placed before T255 in file order), T333 (harness), T334, T335 (harness), T336 (harness).
- T261 states the $0 handoff and depends on T337; T255 depends on T337; T328 depends on T335; T319 adds the rate-limit prune and owns `app/core/src/rate_limit.rs`.
- Ordering for later owners of the same files: T132 after T335; T129 and T131 after T336; T135 after T319; T299 after T336; T100 after T333 to T337.
- Migrations of queued T103, T105, T320, T204, T151, T321, T109 and T110 move up one (0178 to 0185) for T334's 0177 (V9).
- IC-33 re-pointed to T261 and T337; rows IC-79 to IC-81 and risk R-34 added.
- Independent reviews when built: T333, T334, T335 and T337 (money), T336 (privacy and security).
- No task IN_PROGRESS or MERGE_PENDING was edited (T284 was IN_PROGRESS).
