# Ask Hava build status

Updated 2026-11-21T18:10:19Z by runner 5b5195da7475 (actions).

**Building: T608 in progress since 15:59Z; 584 of 732 done; week 31% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 584 of 732 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T608 claimed 15:59Z, lease until 18:59Z; lane 2: T690 claimed 16:56Z, lease until 19:56Z; lane 4: T615 claimed 17:10Z, lease until 20:10Z
- **Week:** 31% used, 128.7M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 17:55Z, 14 min before this update
- **Keeper:** last ran 17:40Z, next wake 19:02Z

## Needs attention
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- PLAN GAP T790 (since 2026-11-21T17:21:47Z, no attempt used; a plan change fixes it): T790: prompt kit/prompts/T790.md is missing; run scripts/gen_prompts.py; T790: branch claude/task/T790-plan-health is named by its requirements, tests or reads, but neither it nor a dependency owns it and it does not exist yet
- Strict plan check red: 39 problem(s) on tasks still to run (+461 on DONE tasks); first: T624: stored context_pct 12% but it measures 17%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T787 | intake: crate guide trimmed to 5,500 bytes, the detail moved | MERGE_PENDING | 1 | 68 min | 16:45Z |  |
| T786 | server: crate guide trimmed to 5,500 bytes, the detail moved | done | 1 | 34 min | 16:33Z |  |
| T655 | businesses: an unknown ROC license status warns at 24 hours, | done | 2 | 2 h 34 min | 16:05Z | model claude-opus-5-5 ran, task names claude-fable-5-2 |
| T789 | Tests (harness): TD-INT-47's no-contact check ignores the ra | done | 1 | 27 min | 15:38Z |  |
| T304 | server and portals: the owner refund review, the audit log v | done | 2 | 2 h 44 min | 15:18Z | PLAN GAP: app/portals/templates/owner_audit_log.html (owned by T205) and app/portals/templates/health.html (owned by T47 |
| T776 | intake: a move out of Arizona and a property-management requ | done | 3 | 99 min | 14:37Z | PLAN GAP: outside ownership: app/quotes/src/card_pool.rs (also docs/catalog/categories.yaml) / Worker: D-5160's three tr |
| T655 | businesses: an unknown ROC license status warns at 24 hours, | QUEUED | 1 | 2 h 36 min | 13:24Z |  |
| T707 | server: an emergency pick's address re-release waits while s | QUEUED | 2 | 77 min | 12:31Z | Reviewer after the findings round still ends REVIEW-VERDICT: FINDINGS (head 2f9bf610c3700bb46a4670ade27d67afda63591a). / |
| T581 | messaging, intake and server: a failed text's call-back is d | done | 1 | 76 min | 12:03Z |  |
| T615 | site and server: a styled 404 page for unknown paths, the fa | BLOCKED | 3 | 2 h 11 min | 11:53Z | Ci: failure; Coverage: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / test  |
| T754 | portals and payments: a $0 Confirm after a PaymentIntent was | check_failed | 3 | 70 min | 10:58Z | PLAN GAP: tests/integration/tests/stripe_webhook_dispatch.rs:420-430 (done test td_fee_43_a_database_error_is_resent_and |
| T458 | businesses, calls and server: the email's "Link this phone"  | done | 1 | 112 min | 10:35Z |  |
| T654 | licensing and quotes: every ROC list import runs each active | done | 2 | 74 min | 10:15Z | PLAN GAP: outside ownership: app/quotes/src/service.rs, app/quotes/src/late_cards.rs, app/intake/src/cards_out.rs, app/i |
| T615 | site and server: a styled 404 page for unknown paths, the fa | check_failed | 2 | 92 min | 09:28Z |  |
| T776 | intake: a move out of Arizona and a property-management requ | BLOCKED | 2 | 11 min | 08:54Z |  |
| T674 | server: the homeowner's photo upload page and the short-live | done | 2 | 40 min | 08:06Z | merging main into the reviewed head conflicts in app/server/src/main.rs; a resync merges it in on this attempt (D-891) |
| T615 | site and server: a styled 404 page for unknown paths, the fa | check_failed | 1 | 55 min | 07:33Z |  |
| T674 | server: the homeowner's photo upload page and the short-live | resync | 2 | 2 h 38 min | 07:19Z |  |
| T754 | portals and payments: a $0 Confirm after a PaymentIntent was | BLOCKED | 2 | 46 min | 06:41Z |  |
| T755 | messaging: a sole opt-out word counts with any spacing ("STO | done | 1 | 66 min | 05:21Z |  |
| T777 | businesses: a sign-in code is texted only to a line whose cu | done | 2 | 39 min | 05:05Z | PLAN GAP: outside ownership: app/server/tests/business_signin_route.rs / T777's approved_at filter in sign_in_lines fail |
| T614 | site: the V6 header in base.html (links, the Menu in the pag | done | 3 | 53 min | 04:37Z | PLAN GAP: tests/integration/tests/quote_page.rs:172 (owner T052, protected) bans the word "summary" anywhere in the quot |
| T691 | intake (harness): START into an open request resends the con | QUEUED | 1 | 27 min | 04:35Z | session ended without handing in |
| T304 | server and portals: the owner refund review, the audit log v | BLOCKED | 1 | 91 min | 04:20Z |  |
| T457 | portals and server: the Confirm email link asks for a code w | done | 1 | 35 min | 04:16Z |  |
| T781 | recovery: a failed or timed-out nightly export raises one al | done | 1 | 116 min | 04:14Z |  |
| T776 | intake: a move out of Arizona and a property-management requ | BLOCKED | 1 | 11 min | 04:00Z |  |
| T684 | picks and server: the open-cards email goes at the times eac | done | 2 | 93 min | 03:50Z | PLAN GAP: app/quotes/src/models.rs:265 `card_sent_at` is pub(crate); T684 owns no app/quotes/src file. quotes::api has n |
| T780 | payments and portals: the scorecard's refund and dispute rat | done | 1 | 33 min | 03:33Z |  |
| T775 | intake: a check-in reply that names a 911 danger gets the 91 | done | 1 | 94 min | 03:03Z |  |
| T754 | portals and payments: a $0 Confirm after a PaymentIntent was | BLOCKED | 1 | 3 min | 02:52Z |  |
| T457 | portals and server: the Confirm email link asks for a code w | resync | 1 | 70 min | 02:50Z |  |
| T654 | licensing and quotes: every ROC list import runs each active | BLOCKED | 1 | 2 min | 02:42Z |  |
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

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 0 of 735 (0.0%) · partial 716 · unstarted 19
**Tasks merged (throughput only):** 584 of 731

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 0 | 35 | 0 | 35 |
| ai | 0 | 26 | 3 | 29 |
| businesses | 0 | 57 | 1 | 58 |
| calls | 0 | 25 | 0 | 25 |
| consent | 0 | 35 | 0 | 35 |
| fees | 0 | 52 | 0 | 52 |
| intake | 0 | 43 | 0 | 43 |
| job-cards | 0 | 24 | 0 | 24 |
| licensing | 0 | 48 | 0 | 48 |
| lifecycle | 0 | 28 | 0 | 28 |
| messaging | 0 | 45 | 0 | 45 |
| ops-recovery | 0 | 42 | 12 | 54 |
| privacy | 0 | 31 | 0 | 31 |
| quiet-hours | 0 | 9 | 0 | 9 |
| quotes-pick | 0 | 56 | 0 | 56 |
| recurring | 0 | 11 | 0 | 11 |
| security | 0 | 52 | 3 | 55 |
| terms | 0 | 35 | 0 | 35 |
| website | 0 | 30 | 0 | 30 |
| wording | 0 | 32 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-21T16:58:08Z (week)): week: 65.3M used, 128.7M left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-05T15:46:20Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 227 of 312 done
- stage 6: 0 of 67 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T608 (lease until 2026-11-21T18:59:45Z)
- lane 2: T690 (lease until 2026-11-21T19:56:59Z)
- lane 4: T615 (lease until 2026-11-21T20:10:57Z)

## Last usage reading
- {"task": "T787", "at": "2026-11-21T16:45:03Z", "cache_creation_input_tokens": 124365, "input_tokens": 98, "cache_read_input_tokens": 1595918, "output_tokens": 31069}
