# Ask Hava build status

Updated 2026-11-21T05:01:06Z by runner 436a388db578 (actions).

**Building: T777 in progress since 04:26Z; 570 of 726 done; week 65% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 570 of 726 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 5: T777 claimed 04:26Z, lease until 07:26Z; lane 6: T755 claimed 04:14Z, lease until 07:14Z; lane 7: T674 claimed 04:40Z, lease until 07:40Z
- **Week:** 65% used, 63.8M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 05:00Z, 0 min before this update
- **Keeper:** last ran 04:56Z, next wake 07:17Z

## Needs attention
- BLOCKED T304: plan gap reported: PLAN GAP: app/portals/templates/owner_audit_log.html (owned by T205) and app/portals/templates/health.html (owned by T476) fail Ally once served: bare <table> at owner_audit_log.html:8, health.html:19 and :28 with no role=region tabindex=0 aria-label wrapper (as app/site/templates/business.html:43); reflow_spacing.spec.ts A11Y-008/009 fail at 320px, plus color-contrast incomplete on audit log td:nth-child(5). T304 Scope (kit/prompts/T304.md:35-51) does not allow editing them. Reviewer confirmed ownership via kit/tasks.json and the missing wrappers.; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T304 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T754: plan gap reported: PLAN GAP: outside ownership: app/adapters/src/payments.rs, stand_ins/stripe.py, stand_ins/server.py
T754 requires cancelling the PaymentIntent at Stripe after a $0 press, but PaymentsAdapter/IntentStripe (app/adapters/src/payments.rs:343-397, app/payments/src/charge.rs:67) have no cancel method and stand_ins/stripe.py and stand_ins/server.py (route at server.py:457) have no cancel route. None is owned by T754 (owns confirm_pay.rs, release_proof.rs, routes.rs, zero_fee_after_intent.rs); stand_ins/ is protected. Existing callers of end_intent_unless_charging (confirm_moves.rs:137, cant_take.rs, ; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T754 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T776: plan gap reported: PLAN GAP: outside ownership: app/intake/src/matching.rs
Worker: matching.rs:52 keyword RULES cover only plumbing, HVAC, electrical, handyman, pool, so no text reaches `tell` as local_moving, home_watch or turnover_cleaning; the T776 refusals can't be reached or tested end to end. matching.rs is owned by T042/T183/T716, not T776. Reviewer (Opus) confirmed: integration test intake_refusals_wired 0 passed, 3 failed (no trade read, nothing sent). Smallest fix: add keyword rules for those three trades (from docs/catalog/categories.yaml) in matching.rs, by a prior task or by adding matching.rs to T7; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T776 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- PLAN GAP T754 (since 2026-11-21T02:52:28Z, no attempt used; a plan change fixes it): outside ownership: app/adapters/src/payments.rs, stand_ins/stripe.py, stand_ins/server.py
- PLAN GAP T776 (since 2026-11-21T04:00:50Z, no attempt used; a plan change fixes it): outside ownership: app/intake/src/matching.rs
- Strict plan check red: 57 problem(s) on tasks still to run (+450 on DONE tasks); first: T704: stored context_pct 34% but it measures 38%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T614 | site: the V6 header in base.html (links, the Menu in the pag | MERGE_PENDING | 3 | 53 min | 04:37Z | PLAN GAP: tests/integration/tests/quote_page.rs:172 (owner T052, protected) bans the word "summary" anywhere in the quot |
| T691 | intake (harness): START into an open request resends the con | QUEUED | 1 | 27 min | 04:35Z | session ended without handing in |
| T304 | server and portals: the owner refund review, the audit log v | BLOCKED | 1 | 91 min | 04:20Z | PLAN GAP: app/portals/templates/owner_audit_log.html (owned by T205) and app/portals/templates/health.html (owned by T47 |
| T457 | portals and server: the Confirm email link asks for a code w | MERGE_PENDING | 1 | 35 min | 04:16Z | merging main into the reviewed head conflicts in app/server/src/main.rs; a resync merges it in on this attempt (D-891) |
| T781 | recovery: a failed or timed-out nightly export raises one al | MERGE_PENDING | 1 | 116 min | 04:14Z |  |
| T776 | intake: a move out of Arizona and a property-management requ | BLOCKED | 1 | 11 min | 04:00Z | PLAN GAP: outside ownership: app/intake/src/matching.rs / Worker: matching.rs:52 keyword RULES cover only plumbing, HVAC |
| T684 | picks and server: the open-cards email goes at the times eac | done | 2 | 93 min | 03:50Z | PLAN GAP: app/quotes/src/models.rs:265 `card_sent_at` is pub(crate); T684 owns no app/quotes/src file. quotes::api has n |
| T780 | payments and portals: the scorecard's refund and dispute rat | done | 1 | 33 min | 03:33Z |  |
| T775 | intake: a check-in reply that names a 911 danger gets the 91 | done | 1 | 94 min | 03:03Z |  |
| T754 | portals and payments: a $0 Confirm after a PaymentIntent was | BLOCKED | 1 | 3 min | 02:52Z | PLAN GAP: outside ownership: app/adapters/src/payments.rs, stand_ins/stripe.py, stand_ins/server.py / T754 requires canc |
| T457 | portals and server: the Confirm email link asks for a code w | resync | 1 | 70 min | 02:50Z |  |
| T654 | licensing and quotes: every ROC list import runs each active | BLOCKED | 1 | 2 min | 02:42Z | PLAN GAP: outside ownership: app/quotes/src/service.rs, app/quotes/src/late_cards.rs, app/intake/src/cards_out.rs, app/i |
| T774 | tests (harness): the quote page's no-AI-summary check leaves | done | 1 | 65 min | 02:38Z |  |
| T777 | businesses: a sign-in code is texted only to a line whose cu | BLOCKED | 1 | 37 min | 02:09Z | PLAN GAP: outside ownership: app/server/tests/business_signin_route.rs / T777's approved_at filter in sign_in_lines fail |
| T769 | site: the business page's WEB-024 line follows the stored bu | done | 1 | 41 min | 01:56Z |  |
| T580 | messaging: a failed, blocked or expired text reads as failed | done | 1 | 43 min | 01:53Z |  |
| T664 | portals: the Confirm press re-checks the license and account | done | 2 | 2 h 38 min | 01:47Z | PLAN GAP: outside ownership: app/picks/src/handlers.rs, app/picks/src/confirm.rs / The pick's window deadline (picks.pic |
| T674 | server: the homeowner's photo upload page and the short-live | BLOCKED | 1 | 17 min | 01:25Z | PLAN GAP: outside ownership: tests/a11y/fixtures.ts — GET /photos/{token} needs one NOT_PAGES line there (A11Y-002; a PA |
| T677 | quotes, businesses and server: a business that becomes eligi | done | 1 | 111 min | 01:02Z |  |
| T526 | server, intake, businesses and site: business texting follow | done | 1 | 71 min | 00:19Z |  |
| T773 | Runner (harness): the merge train can carry a third car, bui | done | 1 | 33 min | 23:17Z |  |
| T682 | picks: a pick waiting for Confirm gets its hourly reminders, | done | 2 | 91 min | 22:27Z | app/picks/src/reminder_run.rs:316 (remind): a waiting pick whose job was cancelled or closed after the notice keeps gett |
| T673 | intake (harness): the dispatcher builds and shows the card,  | done | 1 | 106 min | 22:03Z |  |
| T739 | picks: the owner's re-send of a release goes only while the  | done | 1 | 52 min | 21:49Z |  |
| T752 | catalog and twin (harness): the `launch_open` comment says a | done | 1 | 39 min | 21:24Z |  |
| T750 | docs: the privacy policy draft says an opted-out number is k | done | 1 | 38 min | 21:20Z |  |
| T558 | Decisions digest, piece 13: the next entries after the heade | done | 1 | 28 min | 20:57Z |  |
| T682 | picks: a pick waiting for Confirm gets its hourly reminders, | QUEUED | 1 | 105 min | 20:49Z |  |
| T743 | messaging and privacy: opt-out listings are re-keyed to the  | done | 2 | 2 h 11 min | 20:36Z | Second review (head 7921ab0285f5916db9a9df89ee828c150908805e) ended FINDINGS: tests/integration/tests/suppression_rekey. |
| T772 | picks: a public reader of a waiting pick's Confirm deadline  | done | 1 | 117 min | 20:02Z |  |
| T756 | privacy: the scrubber catches phones split by connector word | done | 1 | 59 min | 19:57Z |  |
| T530 | portals: the owner help page and its screened reply box (D-3 | done | 3 | 49 min | 19:43Z | merging main into the reviewed head conflicts in docs/for-casey-review.md; a resync merges it in on this attempt (D-891) |
| T661 | privacy: a business notice address with spaces, commas or co | done | 1 | 27 min | 19:04Z |  |
| T614 | site: the V6 header in base.html (links, the Menu in the pag | BLOCKED | 2 | 80 min | 18:46Z |  |
| T743 | messaging and privacy: opt-out listings are re-keyed to the  | QUEUED | 1 | 112 min | 18:17Z |  |
| T573 | messaging and server: queued business texts are addressed to | done | 4 | 41 min | 18:01Z | model claude-opus-5-5 ran, task names claude-fable-5-2 / re-queued after plan fix D-4969: Coverage time-limit cancel, fi |
| T686 | businesses and server: a 90-day-old inbox or alert Yes gets  | done | 1 | 76 min | 17:39Z |  |
| T672 | intake: the dispatcher asks "How soon?", the night count and | done | 3 | 52 min | 17:14Z | merging main into the reviewed head conflicts in docs/for-casey-review.md; a resync merges it in on this attempt (D-891) |
| T530 | portals: the owner help page and its screened reply box (D-3 | resync | 3 | 71 min | 16:40Z |  |
| T681 | picks and jobs: the businesses that lost a pick and every qu | done | 1 | 50 min | 16:38Z |  |
| T771 | a11y tests (harness): reflow_spacing's hidden-control check  | done | 2 | 50 min | 16:34Z | PLAN GAP: kit/prompts/T771.md:9 (D-4970, docs/decisions.md:2604) contradicts itself: rule (1) checks the hidden Menu sum |
| T664 | portals: the Confirm press re-checks the license and account | BLOCKED | 1 | 5 min | 16:15Z |  |
| T684 | picks and server: the open-cards email goes at the times eac | BLOCKED | 1 | 4 min | 15:41Z |  |
| T770 | ci (harness): the Coverage job's time limit goes from 25 to  | done | 1 | 61 min | 15:35Z |  |
| T771 | a11y tests (harness): reflow_spacing's hidden-control check  | BLOCKED | 1 | 3 min | 15:22Z |  |
| T557 | Decisions digest, piece 12: the next entries after the heade | done | 1 | 65 min | 14:55Z |  |
| T672 | intake: the dispatcher asks "How soon?", the night count and | resync | 3 | 2 h 13 min | 14:53Z |  |
| T206 | recovery: the local restore drill with no double send or cha | done | 1 | 89 min | 14:50Z |  |
| T767 | tests (harness): the Confirm press fixtures get an Active ac | done | 1 | 108 min | 14:41Z |  |
| T670 | picks: the pick notice's send never records a pressed pick a | done | 1 | 86 min | 14:26Z |  |
| T667 | picks and calls: a reconnected job's failed masked line is r | done | 1 | 58 min | 13:54Z |  |
| T727 | Runner (harness): a retry after a review failure or a plan-f | done | 2 | 87 min | 13:23Z | Ci: failure; Coverage: cancelled / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / tes |
| T496 | a11y (harness): stub Stripe.js in the Ally suite and scan th | done | 1 | 81 min | 13:06Z |  |
| T648 | recovery: the monthly test restore into a throwaway cluster, | done | 2 | 112 min | 12:29Z | PLAN GAP: app/recovery/src/restore.rs:44 include_str!("../../../scripts/bootstrap_roles.sql") is required by the goal, b |
| T573 | messaging and server: queued business texts are addressed to | check_failed | 3 | 70 min | 12:25Z |  |
| T614 | site: the V6 header in base.html (links, the Menu in the pag | BLOCKED | 1 | 16 min | 11:39Z |  |
| T463 | businesses and server: "Busy for <n> days" pauses job cards  | done | 1 | 58 min | 11:16Z |  |
| T768 | runner tests (harness): the seed test fakes free disk, so a  | done | 1 | 22 min | 10:38Z |  |
| T573 | messaging and server: queued business texts are addressed to | QUEUED | 2 | 53 min | 09:48Z |  |
| T530 | portals: the owner help page and its screened reply box (D-3 | QUEUED | 2 | 2 h 25 min | 09:46Z |  |
| T727 | Runner (harness): a retry after a review failure or a plan-f | check_failed | 1 | 75 min | 09:38Z |  |
| T617 | site: DM Sans from one weight-axis variable file (smaller th | done | 2 | 2 h 16 min | 09:22Z | PLAN GAP: tests/a11y/design_tokens.spec.ts:221 and :242 (protected, not owned by T617) contradict the prompt's single DM |
| T766 | server tests (harness): a photo-link door and a {job} fill i | done | 1 | 53 min | 09:05Z |  |
| T116 | Stage 0.5 static page set and the DNS records file | done | 1 | 50 min | 08:48Z |  |
| T669 | picks: "Can't take it" and a business leaving end a pick in  | done | 2 | 80 min | 08:32Z | runner smoke red on c403ea4898ac (bash scripts/smoke.sh, D-3530): / line = cache.seed(tmp_path, smoke, debug, lambda arg |
| T672 | intake: the dispatcher asks "How soon?", the night count and | QUEUED | 2 | 89 min | 08:05Z |  |
| T561 | server and portals: the homeowner's "Download my data" and h | done | 1 | 33 min | 07:51Z |  |
| T762 | runner and workflow (harness): fix-round workers commit with | done | 1 | 68 min | 07:44Z |  |
| T764 | ci and pre-hand-in gate (harness): scripts/bootstrap_roles.s | done | 1 | 54 min | 07:29Z |  |
| T669 | picks: "Can't take it" and a business leaving end a pick in  | QUEUED | 1 | 56 min | 07:07Z |  |
| T573 | messaging and server: queued business texts are addressed to | BLOCKED | 1 | 5 min | 06:27Z |  |
| T561 | server and portals: the homeowner's "Download my data" and h | resync | 1 | 60 min | 06:26Z |  |
| T765 | recovery (tests): the backup tests' fake binaries can no lon | done | 1 | 65 min | 06:11Z |  |
| T751 | stand-ins (harness): the Stripe stand-in detaches a payment  | done | 1 | 51 min | 05:02Z |  |

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 359 of 735 (48.8%) · partial 357 · unstarted 19
**Tasks merged (throughput only):** 570 of 726

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 18 | 17 | 0 | 35 |
| ai | 8 | 18 | 3 | 29 |
| businesses | 22 | 35 | 1 | 58 |
| calls | 15 | 10 | 0 | 25 |
| consent | 17 | 18 | 0 | 35 |
| fees | 25 | 27 | 0 | 52 |
| intake | 12 | 31 | 0 | 43 |
| job-cards | 12 | 12 | 0 | 24 |
| licensing | 31 | 17 | 0 | 48 |
| lifecycle | 21 | 7 | 0 | 28 |
| messaging | 26 | 19 | 0 | 45 |
| ops-recovery | 13 | 29 | 12 | 54 |
| privacy | 10 | 21 | 0 | 31 |
| quiet-hours | 3 | 6 | 0 | 9 |
| quotes-pick | 25 | 31 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 13 | 39 | 3 | 55 |
| terms | 29 | 6 | 0 | 35 |
| website | 20 | 10 | 0 | 30 |
| wording | 29 | 3 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-21T04:38:20Z (week)): week: 130.2M used, 63.8M left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-04T15:44:29Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 213 of 306 done
- stage 6: 0 of 67 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 5: T777 (lease until 2026-11-21T07:26:16Z)
- lane 6: T755 (lease until 2026-11-21T07:14:59Z)
- lane 7: T674 (lease until 2026-11-21T07:40:32Z)

## Last usage reading
- {"task": "T614", "at": "2026-11-21T04:37:13Z", "cache_read_input_tokens": 4568972, "cache_creation_input_tokens": 159305, "output_tokens": 52898, "input_tokens": 133}
