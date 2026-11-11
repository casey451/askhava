# Code review of merged code, November 10, 2026: dispositions

**Input:** three independent Opus code reviews of `main` at `d8924453`, requested after Casey's chat of November 10 (about 23:20Z): payments (F1 to F5), privacy and every contact read (F1 to F4), messaging and texting compliance (F1 to F6, plus CON-016). Each report was read as untrusted input; the plan builder checked every finding against `main` at `d8924453` (file and line, and whether a queued task already owns it). Decisions D-2620 to D-2641 (D-2631 onward: the plan review's fix round and batch 2).

| Item | Verdict | Evidence on `main` d8924453 | Disposition | Owner | Decision |
|---|---|---|---|---|---|
| Payments F1 (High) A successful charge writes no `charge` ledger entry | Confirmed | `record_fee_at_confirm` (`app/payments/src/service.rs:401`) has no caller; `effects` and `write_evidence` (`handlers.rs:144`, `charge.rs:656`) write no ledger row; the adapter's `PaymentIntent` has no charge ID (`app/adapters/src/payments.rs:32`); `is_repeat` reads only ledger rows (`free_wins.rs:170`) | The success transaction writes the entry under `record_fee_at_confirm`'s key, with the `ch_` ID; a fee-free job type's $0 entry is written at Confirm | T352 (new, harness), T359 (new), T261 (order only) | D-2623 |
| Payments F2 (Medium) A won dispute is never recorded | Confirmed | `dispute_closed` (`routes.rs:489`) reads no status; nothing writes `CounteringFeeReturn` | `countering-fee-return` and a `dispute-reversal` stored code on `won`; reconcile compares open or lost disputes; FEE-028 names the code | T353 (new) | D-2625 |
| Payments F3 (Medium) No charge queues its receipt | Confirmed | `queue_receipt` (`receipts.rs:56`) has no caller; D-1557 and D-1561's follow-up is orphaned | Queued in the success transaction when the evidence row is new | T359 | D-2623 |
| Payments F4 (Medium) No description or statement suffix on the PaymentIntent | Confirmed | `create_unconfirmed_intent` (`adapters/src/payments.rs:227`) sends neither; D-1476 named T132, whose goal never said so | T132's goal states it, through a defaulted trait method; the job number comes from the Confirm route | T132 (extended) | D-2624 |
| Payments F5 (Low) The evidence record lacks the charge ID | Confirmed | `write_evidence` stores no `ch_` | Stored with F1 | T352, T359 | D-2623 |
| Privacy F1 (High) Scrubber and CARD-008 gate miss common contact shapes | Confirmed (code read; the reviewer's probe output) | `mark_codes` (`scrub.rs:640`), `mark_bare_houses`/`titled` (`:608`, `:624`), `TRIGGERS` (`:197`), `sign_off` (`:693`), `digit_form`; `final_gate` (`quotes/src/gate.rs:48`) | Each root cause fixed; the shapes join the recall corpus; false-positive guards | T351 (new) | D-2622 |
| Privacy F2 (Medium) Photos are never re-sealed on key rotation | Confirmed | Only an INSERT touches `privacy.photo` (`photos.rs:358`); `rotate_batch` (`vault.rs:668`) moves contacts and names only | `rotate_photos`, new object before the row moves; the drill runs both | T354 (new) | D-2626 |
| Privacy F3 (Low) The release claim can race a fraud end | Plausible, latent; accepted as a fix | `show`/`record` (`picks/src/release.rs:282`, `:394`) unlocked; `fraud_warning` (`unreachable_refund.rs:490`) trusts the caller's snapshot | Row lock in the claim; the fraud end refuses a released pick | T355 (new) | D-2627 |
| Privacy F4 (Low) `no_fee_release` clears on any $0 entry of the same business and job | Dropped | D-2302 accepted the same-business, same-job overlap; the scenario needs a caller that clears an unconfirmed pick, and the planned callers do not (T261 after the press, T255 on a settled pick) | No change | - | D-2620 |
| Messaging F1 (High) A phrase opt-out after a stated name is lost, and the word is remembered as a name | Confirmed | `inbound_text.rs:233` classifies scrubbed text; `mark_stated` (`scrub.rs:657`) and `mark_known` (`:704`) | No opt-out, HELP or START word is ever a name; a drift test against messaging's lists | T350 (new) | D-2621 |
| Messaging F2 (Medium) START into an open request sends no consent text | Confirmed | `Resubscribed` handled as `Open` (`inbound_text.rs:387`); `rules.rs:100` | `restart_after_start` queues the consent text under the START row | T357 (new) | D-2629 |
| Messaging F3 (Medium) No email confirmation while texting is off or suspended | Confirmed | `confirm` reads only the ops pause (`optout.rs:195`); the route drops `email` (`inbound_text.rs:372`) | Paused decided from the line state; the email confirmation queued | T357, T360 (new) | D-2629 |
| Messaging F4 (Medium) No sender, and no quiet-hours check at send | Confirmed | `send_on_line` and `send_checked` have test callers only; no not-before column; D-2188's duty unowned | `not_before`, a send-instant check, a fenced worker sender, stale rows expired, unknown outcomes never resent | T356 (new, harness) | D-2628 |
| Messaging F5 (High in effect) CANCEL never pauses the job | Not owned as the report thought | T330 handles the queued event only and owns no route file; nothing calls `on_opt_out` (D-1229) | The route calls `on_opt_out`; T330's handler does the rest | T357, T330 | D-2629 |
| Messaging F6 (Low) The two area-code tables disagree | Confirmed | `keywords.rs:25` has 382, 683, 753, 942; `area_codes.json` lacks them and has 257 | One table; the four codes added | T358 (new, Sonnet) | D-2630 |
| Messaging, not built: CON-016 | Confirmed | `inbound_text.rs:380`, `:393` end at "suppressed" | Email at most daily, else an `ops` audit row T204 counts | T357, T360, T204 | D-2629 |

## Plan changes made

- New tasks T350 to T358; T132, T261, T204 and T299 extended or reordered.
- Order: T351 after T350; T352 after T331, T332, T335, T341, T343 and before T132; T353 after T352; T354 after T273; T355 after T318 and T278; T357 after T336 and T356, before T299 and T204; T358 before T204; T350 to T358 before T100.
- Migrations: T352 0180, T353 0181, T354 0182, T356 0183; queued T103, T105, T320, T204, T151, T321, T109 and T110 move to 0184 to 0191 (V9).
- FEE-028 names `dispute-reversal` (D-2625); TD-FEE-38 and TD-FEE-58 gain a case each; new TD-CON-32, TD-CON-33, TD-AI-27, TD-FEE-64, TD-FEE-65, TD-PRIV-24, TD-PICK-59, TD-QH-05, TD-QH-06; IC-86 to IC-88; R-40, R-41.
- Independent reviews when built: T352, T353 (money); T350, T351, T354, T355, T356, T357 (privacy and consent).
- No task IN_PROGRESS or MERGE_PENDING was edited (T207 and T311 were MERGE_PENDING).

## Plan review fix round (D-2631)

| Finding | Change |
|---|---|
| H1 T352 over the harness cap once `late_success.rs` is adopted | Split: T352 (harness: adapter `latest_charge`, stand-in, contract test, the eight done tests that build `PaymentIntent`) and T359 (build, Opus, money review: the success write) |
| H2 T261's `record_fee_at_confirm` call needs a card authorization | Reverted; T359 makes `waive_at_confirm` write the `fee-free-job-type` entry for a stored fee-free pick; T261 runs after T359; TD-FEE-58 back to `main`'s text, the case in TD-FEE-64 |
| H3 T355's locking does not close the race | Both sides lock the pick row first, then read in a separate statement; interleaving tests both ways |
| H4 T357 needs a table it does not create and customer copy it may not write | Split: T360 (the two emails, their EN/ES templates through the copy panel, CON-016 as an `ops` audit row) and T357 (the route only) |
| M1 T132 cannot get the job number | A described `open_intent` entry taking `IntentText`; T132 owns `confirm_pay.rs` and runs after T261 |
| M2 a (job, business) key hides a second charge after a refund and re-pick | The charge entry is keyed per pick; T359 owns `reconcile.rs` and matches the key; test added |
| M3 T353 asserted T204's statement | Assertion dropped; T204 shows the won dispute |
| M4 T356's status check, claim fence and stuck rows | The migration widens the status check (`expired`, `unknown`); claim takes only due `queued` rows; a stuck or unknown row is never resent blind; merged with batch 2 jobs F1 (T356 becomes harness and rewrites TD-TXT-04) |
| Lows | T351 guard cases added; T354 moves a row only while it still holds the old version and storage key; T357 and IC-88 name T330; the $15 dispute fee cited to `docs/brains/payments-stripe.md:102` |

## Batch 2 (D-2631 to D-2641)

| Item | Verdict | Evidence on `main` | Disposition | Owner | Decision |
|---|---|---|---|---|---|
| Portals F1 A pick is bound to a list position | Confirmed | `page_quotes` (`app/quotes/src/api.rs:103`) numbers confirmed quotes per request; `page_pick` (`app/picks/src/service.rs:486`) takes the position | Stable `shown_number`; forms carry the business; mismatch answers `Taken` | T361 (new, stage 5) | D-2632 |
| Portals F2 Nothing serves `/static/` | Confirmed | `build_router` (`app/server/src/lib.rs:126`) has no static route; `docs/architecture.md` section 3 says so | `ServeDir` under the security headers; T261 and T303 after it | T362 (new) | D-2633 |
| Portals F3 The pick's code step skips per-IP limits | Confirmed | `pick_flow.rs` calls neither admit function | Both called before Verify | T363 (new, stage 5) | D-2634 |
| Portals F4 Remembered-device rows count against sign-in links | Confirmed (by the reviewer's reading; T272 rewrites the count) | `links_since` (`app/portals/src/models.rs:62`) | Folded into queued T272 | T272 | D-2635 |
| Plan gap: business step-up never served | Confirmed | `record_step_up` has test callers only | Serve the page and the SEC-038 routes | T364 (new, stage 5) | D-2636 |
| Quotes F1 Smoke alarm, sprinkler and wall removal reach handymen | Confirmed | `never_handyman` (`app/licensing/src/handyman.rs:118`) scans text for gas only | Folded into queued T347 | T347 | D-2637 |
| Quotes F2 A negated answer skips the question | Confirmed | `keyword_answer` (`app/intake/src/question_bank.rs:138`) | Folded into queued T295 | T295 | D-2638 |
| Calls F3 Read-back confirm connects another line | Confirmed | `voice_confirm` (`app/calls/src/pins.rs:347`) re-reads the lines | New task after T349 and T326; T349 unchanged | T365 (new) | D-2639 |
| Jobs F1 A text can go out twice | Confirmed | `claim` takes `sending` rows (`app/messaging/src/models.rs:211`); the done test relies on a deduping fake | Merged into T356 (harness) | T356 | D-2628 |
| Jobs F2 A dead job timer leaves the job stuck | Confirmed | dedupe key on the dead row (`app/jobs/src/service.rs:226`); only `NO_HANDLER` alerts | Every dead letter alerts; daily timer backstop | T366 (new) | D-2640 |
| Jobs F3 A dead purge sweep stops for good | Confirmed | `run_sweep` schedules after the sweep (`app/jobs/src/purge_events.rs:97`); no `alert_if_last` | Schedule first, alert, bounded scan | T367 (new) | D-2641 |
| Jobs F4 Sweeps read the wall clock | Confirmed (dev and CI only) | `purge_events.rs:100` and the listed handlers | The worker's clock passed in | T367 | D-2641 |
| Caller-ID spoofing note | Not a task | accepted residual risk R4/S7 | For the overseer to raise with Casey | - | - |

- New migrations in serial order: T354 0180, T356 0181, T359 0182, T353 0183, T361 0197; queued T236 moves to 0198 (V9).
- IC-89 and IC-90 and risk R-42 added; R-40, R-41, IC-86 to IC-88 updated.
- No task IN_PROGRESS or MERGE_PENDING was edited (T207 and T311 MERGE_PENDING; T349 QUEUED and unchanged).
