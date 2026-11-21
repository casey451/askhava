# Ask Hava build status

Updated 2026-11-21T08:43:34Z by runner 5effdc8ef3e2 (actions).

**Building: T615 in progress since 07:56Z; 576 of 726 done; week 70% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 576 of 726 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T615 claimed 07:56Z, lease until 10:56Z; lane 2: T776 claimed 08:42Z, lease until 11:42Z
- **Week:** 70% used, 52.4M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 08:41Z, 2 min before this update
- **Keeper:** last ran 08:08Z, next wake 10:58Z

## Needs attention
- BLOCKED T754: plan gap reported: PLAN GAP: tests/integration/tests/stripe_webhook_dispatch.rs:420-430 (done test td_fee_43_a_database_error_is_resent_and_handled, owned by T785 after T259/T331/T352) expects a verified payment_intent.succeeded naming pi_FakeNone for a pick stored with pi_Fake9 (refused WrongIntent, release_proof.rs:220-221) to change nothing, including ops.alert rows; kit/prompts/T754.md:9 (m3) requires one High alert for every WrongIntent refusal. T754 does not own that test file (T754.md:37-40, :53, :104). Reviewer confirmed (GAP-VERDICT: CONFIRMED). Smallest fix: (1) give T754 a one-time edit of stripe_webh; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T754 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- Strict plan check red: 54 problem(s) on tasks still to run (+450 on DONE tasks); first: T704: stored context_pct 34% but it measures 38%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T674 | server: the homeowner's photo upload page and the short-live | done | 2 | 40 min | 08:06Z | merging main into the reviewed head conflicts in app/server/src/main.rs; a resync merges it in on this attempt (D-891) |
| T615 | site and server: a styled 404 page for unknown paths, the fa | check_failed | 1 | 55 min | 07:33Z | Ci: failure; Coverage: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / test  |
| T674 | server: the homeowner's photo upload page and the short-live | resync | 2 | 2 h 38 min | 07:19Z |  |
| T754 | portals and payments: a $0 Confirm after a PaymentIntent was | BLOCKED | 2 | 46 min | 06:41Z | PLAN GAP: tests/integration/tests/stripe_webhook_dispatch.rs:420-430 (done test td_fee_43_a_database_error_is_resent_and |
| T755 | messaging: a sole opt-out word counts with any spacing ("STO | done | 1 | 66 min | 05:21Z |  |
| T777 | businesses: a sign-in code is texted only to a line whose cu | done | 2 | 39 min | 05:05Z | PLAN GAP: outside ownership: app/server/tests/business_signin_route.rs / T777's approved_at filter in sign_in_lines fail |
| T614 | site: the V6 header in base.html (links, the Menu in the pag | done | 3 | 53 min | 04:37Z | PLAN GAP: tests/integration/tests/quote_page.rs:172 (owner T052, protected) bans the word "summary" anywhere in the quot |
| T691 | intake (harness): START into an open request resends the con | QUEUED | 1 | 27 min | 04:35Z | session ended without handing in |
| T304 | server and portals: the owner refund review, the audit log v | BLOCKED | 1 | 91 min | 04:20Z | PLAN GAP: app/portals/templates/owner_audit_log.html (owned by T205) and app/portals/templates/health.html (owned by T47 |
| T457 | portals and server: the Confirm email link asks for a code w | done | 1 | 35 min | 04:16Z |  |
| T781 | recovery: a failed or timed-out nightly export raises one al | done | 1 | 116 min | 04:14Z |  |
| T776 | intake: a move out of Arizona and a property-management requ | BLOCKED | 1 | 11 min | 04:00Z | PLAN GAP: outside ownership: app/intake/src/matching.rs / Worker: matching.rs:52 keyword RULES cover only plumbing, HVAC |
| T684 | picks and server: the open-cards email goes at the times eac | done | 2 | 93 min | 03:50Z | PLAN GAP: app/quotes/src/models.rs:265 `card_sent_at` is pub(crate); T684 owns no app/quotes/src file. quotes::api has n |
| T780 | payments and portals: the scorecard's refund and dispute rat | done | 1 | 33 min | 03:33Z |  |
| T775 | intake: a check-in reply that names a 911 danger gets the 91 | done | 1 | 94 min | 03:03Z |  |
| T754 | portals and payments: a $0 Confirm after a PaymentIntent was | BLOCKED | 1 | 3 min | 02:52Z |  |
| T457 | portals and server: the Confirm email link asks for a code w | resync | 1 | 70 min | 02:50Z |  |
| T654 | licensing and quotes: every ROC list import runs each active | BLOCKED | 1 | 2 min | 02:42Z | PLAN GAP: outside ownership: app/quotes/src/service.rs, app/quotes/src/late_cards.rs, app/intake/src/cards_out.rs, app/i |
| T774 | tests (harness): the quote page's no-AI-summary check leaves | done | 1 | 65 min | 02:38Z |  |
| T777 | businesses: a sign-in code is texted only to a line whose cu | BLOCKED | 1 | 37 min | 02:09Z |  |
| T769 | site: the business page's WEB-024 line follows the stored bu | done | 1 | 41 min | 01:56Z |  |
| T580 | messaging: a failed, blocked or expired text reads as failed | done | 1 | 43 min | 01:53Z |  |
| T664 | portals: the Confirm press re-checks the license and account | done | 2 | 2 h 38 min | 01:47Z | PLAN GAP: outside ownership: app/picks/src/handlers.rs, app/picks/src/confirm.rs / The pick's window deadline (picks.pic |
| T674 | server: the homeowner's photo upload page and the short-live | BLOCKED | 1 | 17 min | 01:25Z |  |
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

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 361 of 735 (49.1%) · partial 355 · unstarted 19
**Tasks merged (throughput only):** 576 of 726

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 18 | 17 | 0 | 35 |
| ai | 8 | 18 | 3 | 29 |
| businesses | 23 | 34 | 1 | 58 |
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
| quotes-pick | 26 | 30 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 13 | 39 | 3 | 55 |
| terms | 29 | 6 | 0 | 35 |
| website | 20 | 10 | 0 | 30 |
| wording | 29 | 3 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-21T08:07:43Z (week)): week: 141.6M used, 52.4M left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-05T09:26:51Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 219 of 306 done
- stage 6: 0 of 67 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T615 (lease until 2026-11-21T10:56:09Z)
- lane 2: T776 (lease until 2026-11-21T11:42:46Z)

## Last usage reading
- {"task": "T674", "at": "2026-11-21T08:06:44Z", "cache_read_input_tokens": 1225466, "cache_creation_input_tokens": 77787, "output_tokens": 11086, "input_tokens": 69}
