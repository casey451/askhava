# Repository audit of November 6, 2026: dispositions

**Input:** an external repository-wide code audit (ChatGPT), pinned to `main` at `bb6d771` with build state `38e9ea0`, received as an attachment. Treated as untrusted input: every finding was checked against `main` at `5aac33d` (T083 merged) before anything was planned. **Plan change:** D-1751 to D-1761 and D-1783, branch `claude/plan-audit-findings`. A07 and the release composition of A08 are dispositioned by the separate release plan change (D-1739, D-1740; tasks T253 to T255) and are not repeated here.

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
| A08 (rest) Served Confirm route, Confirm-only CSP, `handleNextAction`, webhook dispatch, substitute-only e2e | Confirmed | No `/b/picks/{pick}/confirm` route in `app/server/src/lib.rs` (D-1740 (1) left it unowned); `app/portals/src/security.rs:67` overwrites every CSP; `app/portals/templates/confirm_pay.html:39` loads Stripe.js; `app/portals/static/confirm_pay.ts:95` ignores `handleNextAction`'s result; `app/payments/src/routes.rs:124` handles only `payment_intent.succeeded`, no event-ID record (SEC-017); no `IntentStripe` outside `tests/integration/tests/confirm_and_pay.rs`; the stand-in has no confirm endpoint. T084 (retrying after its first attempt failed `Ally`, D-1772) owns session scoping only and is not changed; T132 is stage 6 | T259, T260 (harness), T261 (new; T261 depends on T084 and T232); T099 now depends on T261 | TD-FEE-43, TD-FEE-44, TD-FEE-45, TD-SEC-37 | D-1754 |
| A09 Call PIN = line ID mod 10,000; no durable try counts | Confirmed | `app/calls/src/routing.rs:90`; `tries_today` is caller input (`routing.rs:123`, `:163`); D-1684/D-1685 named T078/T080, both DONE; no inbound-voice route (`app/calls/CLAUDE.md`, D-1708) | T262 (new) | TD-CALL-13, TD-CALL-14 | D-1755 |
| A10 Identity send key reused after a record contact change | Confirmed | `app/businesses/src/identity.rs:156` keys on business and step only; D-1712 | T196 (extended; it already owns `identity.rs`) | TD-BIZ-40 | D-1756 |
| A11 Monthly fee limit is a soft cap; `set_fee_limit` not atomic | Confirmed; product decision taken (hard cap) | `app/payments/src/charge.rs:413`–`:443` (check outside any lock), `:394` (delete then insert); D-1427's last sentence accepts overshoot | T263 (new); T209 now depends on it | TD-FEE-46 | D-1758 (sets aside D-1427's last sentence, its soft-guard choice; the rest of D-1427 stands) |
| A12 Integration-closure register | Confirmed (no mechanical check) | Handoffs to DONE tasks: D-1684, D-1685 (T078/T080), D-1688 (T079), D-1712 (T193's note), D-1715 (no owner for its wiring), D-990 (no owner after T156/T208) | `docs/integration-closure.md`; T265 (new, harness) | TD-OPS-31; D-1715 gates TD-BIZ-41 (T251), TD-TXT-17 extended (T147) | D-1760 |

## Follow-ups the overseer added after the release plan change (D-1761)

| Item | Verdict | Evidence | Owner | Test-design rows |
|---|---|---|---|---|
| PICK-018 contact choice and shared email never stored (D-1740 (2)) | Confirmed | `app/portals/src/pick_flow.rs:38` ("checked and passed on unstored", D-1264); `picks::api::page_pick` takes no choice (`app/picks/src/service.rs:392`) | T266 (new) | TD-PICK-52 |
| Release email `{link}` not filled (D-1565 (2)) | Confirmed | D-1565 (2); no queued task's goal filled or checked the link | T147 (fills), T194 (checks) | TD-TXT-18 (extended), TD-PICK-51 |
| Cross-links with D-1739 (#313, merged first) | Applied after merging `main` | T254 now on `main` | T261 depends on T254; T266 depends on T254 and T255 and owns `app/server/src/release_sink.rs` and `app/server/tests/release_contact_choice.rs` | TD-PICK-52 (release cases) |

## Fix round after the Fable review of 04637b9 (request changes)

| Item | What changed |
|---|---|
| B1 T258 would break nine test files it does not own and missed `app/payments/src/models.rs` | The typed refund is additive: payments-owned `RefundStripe` trait in `app/payments/src/refund_stripe.rs`; `PaymentsAdapter::create_refund` unchanged until T132; T258 owns `models.rs` (D-1753) |
| B2 T265's rule would stall the runner (`runner/state.py:403` re-runs every earlier stage's exit) | A row is judged at stage N only when every owner's stage is at or below N; a row with no owner fails everywhere (D-1760 (2), TD-OPS-31, register Rule) |
| B3 FEE-028 cited the wrong decision | `docs/spec/fees.md:32` now cites D-1753 |
| S2 release-email link format | `link_token(secret, LinkKind::Action, person, LinkSalt(job ‖ expiry))`, no `core` change (T147, T194, D-1761) |
| S3 PIN-try digest | `pin-try|v1` HMAC under `ID_SECRET` (PRIV-024); T262 owns `app/calls/src/models.rs` (D-1755) |
| S4 T257's artificial T086 dependency | Dropped; T257 placed after T255, migration 0130 (D-1752) |
| S5 SEC-047 on the new per-text read | Kept on, counted once per text under the existing grant, D-750's floor; tested (D-1752) |
| S6 IC-21 had no owner | New task T268 (D-1783) |
| S7 T264's decision must say "coverage floor" | In T264's goal (`scripts/coverage_floor.py:104`) |
| S8 counts and states | Register count, T084's state, reviews recorded in D-1752 and D-1753 |
| S1 lock the register | Two-step follow-up, register row IC-35 (D-1760 (2)) |
| Overseer: `fixtures.ts` shared path also holds the checks | New harness task T267, register row IC-34, governance section 2 reader row (D-1783) |

## Second fix round after the Fable re-review of 2943cc9 (request changes)

| Item | What changed |
|---|---|
| B1 still open: the public refund functions and T064's `run_late_refund` must take `RefundStripe`, breaking four DONE tasks' fakes | T258 is a harness task owning `app/payments/src/late.rs` and `tests/integration/tests/{disputes,late_success,reconcile,ledger_1000_jobs}.rs`; each fake gains one `impl RefundStripe`, no expected value changes (D-1753) |
| B4 no task could edit `tests/a11y/page_states.ts` after T267 | Added now to `shared_paths` and `ALLOWED_SHARED`; the template names `fixtures.ts` until T267, then `page_states.ts`; T267 owns only the two test files (D-1783) |
| S-a D-1758 marked all of D-1427 superseded | Reworded ("sets aside only D-1427's last sentence"); `superseded_map` no longer lists D-1427 |
| S-b T265 rule inconsistency | An unknown task id, like no owner, fails at every stage (T265, TD-OPS-31, D-1760) |
| S-c T132 retires `create_refund` | Stated in T132's goal; D-1753 listed |
| S-d where `EntryKind` lives | `ledger.rs:14`, match in `models.rs:782` (T258, D-1753) |
| S-e where CON-020's welcome lands | An email built from `welcome_lines` in the shared email layout, never a text, so it is not held by TXT-035 (T268) |

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

- D-990, D-1198: the AI writing calls and wiring `ai::api` into intake (IC-23).
