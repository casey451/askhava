# Ask Hava build status

Updated 2026-11-20T20:14:31Z by runner 5b08505974c2 (actions).

**Building: T682 in progress since 19:03Z; 551 of 713 done; week 44% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 551 of 713 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 4: T682 claimed 19:03Z, lease until 22:03Z; lane 7: T743 claimed 18:24Z, lease until 21:24Z
- **Week:** 44% used, 103.7M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 20:11Z, 2 min before this update
- **Keeper:** last ran 20:11Z, next wake 21:27Z

## Needs attention
- BLOCKED T614: plan gap reported: PLAN GAP: tests/integration/tests/quote_page.rs:172 (owner T052, protected) bans the word "summary" anywhere in the quote page's HTML, but T614 must put <details class="menu"><summary>Menu</summary> in the header of every page (docs/design/DESIGN_SPEC.md:102); quote_page.html extends base.html. Smallest plan change: a harness task narrows the check at quote_page.rs:170-174 to the page's <main>. Also D-4970's "two rules only" is not enough: a third rule is needed in site.css: .menu > summary { margin: calc((var(--header-h) - 44px) / 2) 0 calc((var(--header-h) - 44px) / 2) -12px; } (else the row; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T614 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- Strict plan check red: 46 problem(s) on tasks still to run (+424 on DONE tasks); first: T704: stored context_pct 34% but it measures 38%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T772 | picks: a public reader of a waiting pick's Confirm deadline  | MERGE_PENDING | 1 | 117 min | 20:02Z |  |
| T756 | privacy: the scrubber catches phones split by connector word | MERGE_PENDING | 1 | 59 min | 19:57Z |  |
| T530 | portals: the owner help page and its screened reply box (D-3 | done | 3 | 49 min | 19:43Z | merging main into the reviewed head conflicts in docs/for-casey-review.md; a resync merges it in on this attempt (D-891) |
| T661 | privacy: a business notice address with spaces, commas or co | done | 1 | 27 min | 19:04Z |  |
| T614 | site: the V6 header in base.html (links, the Menu in the pag | BLOCKED | 2 | 80 min | 18:46Z | PLAN GAP: tests/integration/tests/quote_page.rs:172 (owner T052, protected) bans the word "summary" anywhere in the quot |
| T743 | messaging and privacy: opt-out listings are re-keyed to the  | QUEUED | 1 | 112 min | 18:17Z | Second review (head 7921ab0285f5916db9a9df89ee828c150908805e) ended FINDINGS: tests/integration/tests/suppression_rekey. |
| T573 | messaging and server: queued business texts are addressed to | done | 4 | 41 min | 18:01Z | model claude-opus-5-5 ran, task names claude-fable-5-2 / re-queued after plan fix D-4969: Coverage time-limit cancel, fi |
| T686 | businesses and server: a 90-day-old inbox or alert Yes gets  | done | 1 | 76 min | 17:39Z |  |
| T672 | intake: the dispatcher asks "How soon?", the night count and | done | 3 | 52 min | 17:14Z | merging main into the reviewed head conflicts in docs/for-casey-review.md; a resync merges it in on this attempt (D-891) |
| T530 | portals: the owner help page and its screened reply box (D-3 | resync | 3 | 71 min | 16:40Z |  |
| T681 | picks and jobs: the businesses that lost a pick and every qu | done | 1 | 50 min | 16:38Z |  |
| T771 | a11y tests (harness): reflow_spacing's hidden-control check  | done | 2 | 50 min | 16:34Z | PLAN GAP: kit/prompts/T771.md:9 (D-4970, docs/decisions.md:2604) contradicts itself: rule (1) checks the hidden Menu sum |
| T664 | portals: the Confirm press re-checks the license and account | BLOCKED | 1 | 5 min | 16:15Z | PLAN GAP: outside ownership: app/picks/src/handlers.rs, app/picks/src/confirm.rs / The pick's window deadline (picks.pic |
| T684 | picks and server: the open-cards email goes at the times eac | BLOCKED | 1 | 4 min | 15:41Z | PLAN GAP: app/quotes/src/models.rs:265 `card_sent_at` is pub(crate); T684 owns no app/quotes/src file. quotes::api has n |
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
| T726 | Runner (harness): a failed attempt's unpushed work can be ke | done | 2 | 36 min | 05:00Z | merging main into the reviewed head conflicts in kit/plan.lock, kit/runner.json; a resync merges it in on this attempt ( |
| T543 | picks, businesses, portals and server: "Away until" is store | done | 1 | 64 min | 04:57Z |  |
| T434 | portals: the monthly intake accuracy report page and the mon | done | 1 | 112 min | 04:55Z |  |
| T110 | recovery: replay with derived IDs | done | 1 | 2 h 36 min | 04:55Z |  |
| T663 | picks, jobs and payments (harness): a confirmed pick moves i | done | 3 | 65 min | 04:54Z | Ci: failure; Coverage: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / test  |
| T556 | Decisions digest, piece 11: the next entries after the heade | done | 1 | 51 min | 04:50Z |  |
| T726 | Runner (harness): a failed attempt's unpushed work can be ke | resync | 2 | 32 min | 04:08Z |  |
| T747 | intake and jobs: the homeowner's INT-029 emergency choice ma | done | 1 | 71 min | 03:25Z |  |
| T726 | Runner (harness): a failed attempt's unpushed work can be ke | resync | 2 | 2 h 16 min | 03:22Z |  |
| T617 | site: DM Sans from one weight-axis variable file (smaller th | BLOCKED | 1 | 2 min | 03:04Z |  |
| T648 | recovery: the monthly test restore into a throwaway cluster, | BLOCKED | 1 | 21 min | 02:55Z |  |
| T763 | tests (harness): the web chat spec's fake numbers never coll | done | 1 | 25 min | 02:17Z |  |
| T672 | intake: the dispatcher asks "How soon?", the night count and | BLOCKED | 1 | 13 min | 02:12Z |  |
| T218 | Wording and template scans from the brains: sales phrases, n | done | 1 | 74 min | 01:46Z |  |
| T663 | picks, jobs and payments (harness): a confirmed pick moves i | check_failed | 2 | 2 h 7 min | 01:40Z |  |
| T527 | messaging, portals and server: the business-texting switch t | done | 1 | 93 min | 01:39Z |  |
| T726 | Runner (harness): a failed attempt's unpushed work can be ke | QUEUED | 1 | 56 min | 01:00Z |  |
| T613 | site: the D-4246 polish layer in site.css (tokens, layered s | done | 1 | 85 min | 00:47Z |  |
| T758 | intake: Spanish gas and burning reports reach the 911 list,  | done | 1 | 90 min | 00:38Z |  |
| T109 | recovery: backup checks and the nightly encrypted export | done | 2 | 32 min | 00:37Z | merging main into the reviewed head conflicts in docs/for-casey-review.md; a resync merges it in on this attempt (D-891) |
| T541 | payments, portals and adapters: a card saved or changed from | done | 3 | 25 min | 00:00Z | Ci: failure; E2e: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / {"timestamp":"2026-11-19T |
| T726 | Runner (harness): a failed attempt's unpushed work can be ke | resync | 1 | 84 min | 23:31Z |  |
| T109 | recovery: backup checks and the nightly encrypted export | resync | 2 | 93 min | 23:26Z |  |
| T707 | server: an emergency pick's address re-release waits while s | check_failed | 1 | 80 min | 22:59Z | Ci: failure; Coverage: cancelled / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / tes |
| T541 | payments, portals and adapters: a card saved or changed from | check_failed | 2 | 91 min | 22:55Z |  |
| T480 | messaging, server and portals: the stored "Spanish conversat | done | 1 | 104 min | 22:23Z |  |
| T717 | intake (harness): corrections to T711's two jobs: two_jobs a | done | 1 | 2 h 11 min | 22:21Z |  |
| T607 | site: site.css brought up to the V6 reference (laftly-v6.css | done | 2 | 41 min | 21:57Z | review_missing on 2c1fc315ebe4 (D-3728): not proven reviewed or worker-made / (b) 5cb49e3317bc T607: NOTES entry in the  |
| T663 | picks, jobs and payments (harness): a confirmed pick moves i | BLOCKED | 1 | 30 min | 21:32Z |  |
| T753 | runner (harness): `fable` is a model the plan can name, a ta | done | 1 | 112 min | 20:59Z |  |
| T307 | server and portals: the homeowner portal's sign-in, home pag | done | 2 | 2 h 45 min | 20:33Z | PLAN GAP: tests/security/tests/vault_readers.rs:23 lets only `portals` call `read_own_contact` (line 19 lets only `messa |

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 362 of 735 (49.3%) · partial 354 · unstarted 19
**Tasks merged (throughput only):** 551 of 713

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
| licensing | 33 | 15 | 0 | 48 |
| lifecycle | 21 | 7 | 0 | 28 |
| messaging | 26 | 19 | 0 | 45 |
| ops-recovery | 15 | 27 | 12 | 54 |
| privacy | 10 | 21 | 0 | 31 |
| quiet-hours | 3 | 6 | 0 | 9 |
| quotes-pick | 26 | 30 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 13 | 39 | 3 | 55 |
| terms | 27 | 8 | 0 | 35 |
| website | 20 | 10 | 0 | 30 |
| wording | 28 | 4 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-20T18:52:51Z (week)): week: 90.3M used, 103.7M left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-06T02:37:27Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 194 of 295 done
- stage 6: 0 of 65 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 4: T682 (lease until 2026-11-20T22:03:22Z)
- lane 7: T743 (lease until 2026-11-20T21:24:48Z)

## Last usage reading
- {"task": "T772", "at": "2026-11-20T20:02:46Z", "cache_creation_input_tokens": 268362, "output_tokens": 74265, "cache_read_input_tokens": 8596400, "input_tokens": 242}
