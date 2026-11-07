# Repository audit of November 6, 2026: dispositions

**Input:** an external repository-wide code audit (ChatGPT), pinned to `main` at `bb6d771` with build state `38e9ea0`, received as an attachment. Treated as untrusted input: every finding was checked against `main` at `5aac33d` (T083 merged) before anything was planned. **Plan change:** D-1751 to D-1761, branch `claude/plan-audit-findings`. A07 and the release composition of A08 are dispositioned by the separate release plan change (D-1739, D-1740; tasks T253 to T255) and are not repeated here.

Verdicts: **confirmed** (the code shows it on current `main`), **owned** (already in a queued task's goal and files), **not reproducible**, **product decision**.

| Finding | Verdict | Evidence on `main` 5aac33d | Owner | Test-design rows | Decision |
|---|---|---|---|---|---|
| A01 First-contact SMS race loses a text | Confirmed | `app/server/src/inbound_text.rs:356` maps every non-database vault error, `PhoneTaken` included, to `Halt::Failed`; `:185`–`:188` commit the delivery (inbox row) unless the database failed; `app/privacy/src/vault.rs:264`, `:289` answer `PhoneTaken` to the loser. T241 (route owner) is DONE; no queued task owned the route | T256 (new) | TD-TXT-32 | D-1751 |
| A02 Scrubber name memory not carried across texts | Confirmed | `app/server/src/inbound_text.rs:297` (`vault_name: None`), `:299` (`NameMemory::default()` per webhook), and `read` runs before `sender` (`:207`, `:209`); AI-003 and AI-020 (`docs/spec/ai.md:7`, `:24`). T023 (scrubber) is DONE; no queued task owned conversation-aware scrubbing | T257 (new) | TD-AI-19 | D-1752 |
| A03 Refund adapter discards Stripe's status | Confirmed | `app/adapters/src/payments.rs:166`–`:176` parse only `id`; `app/payments/src/refunds.rs:279` makes any answer `Refunded`; `app/payments/src/reconcile.rs:370` counts any answer as refunded. T066/T067 DONE; T132 owns only the adapter and stand-in, not `refunds.rs` | T258 (new); webhook events T259 | TD-FEE-42, TD-FEE-43 | D-1753, D-1754 |
| A04 Global AI cap is check-then-insert | Confirmed | `app/ai/src/service.rs:257` reads `calls_today` before the call, `:273` records after; `app/ai/src/models.rs:270`. T236 (queued) owns `service.rs`, `models.rs` and migration 0197 | T236 (extended) | TD-AI-20 | D-1757 |
| A05 Coverage checker accepts missing measurements | Confirmed | `scripts/coverage_floor.py:79` and `:86` judge only when a value exists (`is not None`); the audit reproduced an empty report passing with the real checker | T264 (new, harness) | TD-OPS-30 | D-1759 |
| A06 Total coverage floor still 0.0 | Confirmed | `tests/coverage_floor.json:3`; D-694 said the first covered code would raise it; the `app/catalog/src/rules.rs` key names a missing file (fee rules are `app/payments/src/rules.rs`) | T264 (new, harness) | TD-OPS-30 | D-1759 |
| A07 Release/portal reread, recovery, masked-line repair | Out of scope here | Dispositioned by D-1739 (T253 to T255) | T253, T254, T255 | TD-PICK-48 to TD-PICK-50, TD-INT-39 | D-1739 |
| A08 (release composition) `ReleaseSink`, router merge, paid/free status | Out of scope here | Dispositioned by D-1739 | T254, T255 | TD-PICK-49, TD-PICK-50 | D-1739 |
| A08 (rest) Served Confirm route, Confirm-only CSP, `handleNextAction`, webhook dispatch, substitute-only e2e | Confirmed | No `/b/picks/{pick}/confirm` route in `app/server/src/lib.rs` (D-1740 (1) left it unowned); `app/portals/src/security.rs:67` overwrites every CSP; `app/portals/templates/confirm_pay.html:39` loads Stripe.js; `app/portals/static/confirm_pay.ts:95` ignores `handleNextAction`'s result; `app/payments/src/routes.rs:124` handles only `payment_intent.succeeded`, no event-ID record (SEC-017); no `IntentStripe` outside `tests/integration/tests/confirm_and_pay.rs`; the stand-in has no confirm endpoint. T084 (in progress) owns session scoping only and is not changed; T132 is stage 6 | T259, T260 (harness), T261 (new; T261 depends on T084 and T232); T099 now depends on T261 | TD-FEE-43, TD-FEE-44, TD-FEE-45, TD-SEC-37 | D-1754 |
| A09 Call PIN = line ID mod 10,000; no durable try counts | Confirmed | `app/calls/src/routing.rs:90`; `tries_today` is caller input (`routing.rs:123`, `:163`); D-1684/D-1685 named T078/T080, both DONE; no inbound-voice route (`app/calls/CLAUDE.md`, D-1708) | T262 (new) | TD-CALL-13, TD-CALL-14 | D-1755 |
| A10 Identity send key reused after a record contact change | Confirmed | `app/businesses/src/identity.rs:156` keys on business and step only; D-1712 | T196 (extended; it already owns `identity.rs`) | TD-BIZ-40 | D-1756 |
| A11 Monthly fee limit is a soft cap; `set_fee_limit` not atomic | Confirmed; product decision taken (hard cap) | `app/payments/src/charge.rs:413`–`:443` (check outside any lock), `:394` (delete then insert); D-1427's last sentence accepts overshoot | T263 (new); T209 now depends on it | TD-FEE-46 | D-1758 (replaces D-1427's soft-guard choice; the rest of D-1427 stands) |
| A12 Integration-closure register | Confirmed (no mechanical check) | Handoffs to DONE tasks: D-1684, D-1685 (T078/T080), D-1688 (T079), D-1712 (T193's note), D-1715 (no owner for its wiring), D-990 (no owner after T156/T208) | `docs/integration-closure.md`; T265 (new, harness) | TD-OPS-31; D-1715 gates TD-BIZ-41 (T251), TD-TXT-17 extended (T147) | D-1760 |

## Follow-ups the overseer added after the release plan change (D-1761)

| Item | Verdict | Evidence | Owner | Test-design rows |
|---|---|---|---|---|
| PICK-018 contact choice and shared email never stored (D-1740 (2)) | Confirmed | `app/portals/src/pick_flow.rs:38` ("checked and passed on unstored", D-1264); `picks::api::page_pick` takes no choice (`app/picks/src/service.rs:392`) | T266 (new) | TD-PICK-52 |
| Release email `{link}` not filled (D-1565 (2)) | Confirmed | D-1565 (2); no queued task's goal filled or checked the link | T147 (fills), T194 (checks) | TD-TXT-18 (extended), TD-PICK-51 |
| Merge order with D-1739 | Plan note | T254 is not on `main`, so this change cannot name it | the second plan change to merge: T254 into T261's deps; T254, T255 into T266's deps and `release_sink.rs` into T266's owns | - |

## Improvements list (audit section "Improvements justified by the code")

Not made into new tasks; tracked as register rows with existing owners (D-1760 (4)):

| Item | Register row | Owner |
|---|---|---|
| Worker leases are at-least-once; test lease loss during vendor calls | IC-28 | T237, T140 |
| Bounds on trusted vendor response sizes | IC-29 | T131, T132 |
| Real-role full journeys, shipped image | IC-30 | T140, T121 |
| Stale module guides | IC-31 | T100, T126, T142 |
| Measure queue/log overhead before optimizing | not tracked: no change proposed; load tests sit with the war games (T237) | - |

## Context-budget note

The audit noted stored `context_pct` drift. On `main` 5aac33d, `python3 scripts/gen_prompts.py --check --strict` reported 27 problems, all stored values differing from their measurement. The full `gen_prompts.py` run in this change re-measures every stored value (D-1760 (5)); the strict check is clean afterwards (this change's check output).

## Still unowned after this change (not security, money or privacy)

- D-1715: the sign-up consent box, the welcome request after activation, the welcome and phone-stopped email handlers (IC-21).
- D-990, D-1198: the AI writing calls and wiring `ai::api` into intake (IC-23).
