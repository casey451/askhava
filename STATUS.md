# Ask Hava build status

Updated 2026-11-21T01:32:48Z by runner e6f56fd39ecb (actions).

**Building: T664 in progress since 23:09Z; 562 of 726 done; week 56% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 562 of 726 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T664 claimed 23:09Z, lease until 02:09Z; lane 2: T774 claimed 01:32Z, lease until 04:32Z; lane 3: T777 claimed 01:31Z, lease until 04:31Z; lane 5: T769 claimed 01:14Z, lease until 04:14Z; lane 6: T580 claimed 01:10Z, lease until 04:10Z; lane 7: T775 claimed 01:29Z, lease until 04:29Z
- **Week:** 56% used, 75.0M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 01:32Z, 0 min before this update
- **Keeper:** last ran 01:27Z, next wake 01:47Z

## Needs attention
- BLOCKED T674: plan gap reported: PLAN GAP: outside ownership: tests/a11y/fixtures.ts — GET /photos/{token} needs one NOT_PAGES line there (A11Y-002; a PAGE_STATES entry cannot cover a JPEG or 404). The file is owner-only since T267 moved the registry (page_states.ts:7-8); T674's Scope shares it only until then, and only for PAGE_STATES rows. Reviewer confirmed: no task grants that line (T766 does not). plan_pages.py gives a false negative for T674 because its goal mentions page_states.ts. Smallest plan change: grant T674 the one NOT_PAGES line in fixtures.ts. Worker's uncommitted work was not committed.; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T674 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- PLAN GAP T674 (since 2026-11-21T01:25:56Z, no attempt used; a plan change fixes it): outside ownership: tests/a11y/fixtures.ts — GET /photos/{token} needs one NOT_PAGES line there (A11Y-002; a PAGE_STATES entry cannot cover a JPEG or 404). The file is owner-only since T267 moved the registry (page_states.ts:7-8); T674's Scope shares it only until then, and only for PAGE_STATES rows. Reviewer confirmed: no task grants that line (T766 does not). plan_pages.py gives a false negative for T674 because its goal mentions page_states.ts. Smallest plan change: grant T674 the one NOT_PAGES line in fixtures.ts. Worker's uncommitted work was not committed.
- Strict plan check red: 49 problem(s) on tasks still to run (+438 on DONE tasks); first: T704: stored context_pct 34% but it measures 38%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T674 | server: the homeowner's photo upload page and the short-live | BLOCKED | 1 | 17 min | 01:25Z | PLAN GAP: outside ownership: tests/a11y/fixtures.ts — GET /photos/{token} needs one NOT_PAGES line there (A11Y-002; a PA |
| T677 | quotes, businesses and server: a business that becomes eligi | MERGE_PENDING | 1 | 111 min | 01:02Z |  |
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
| T614 | site: the V6 header in base.html (links, the Menu in the pag | BLOCKED | 2 | 80 min | 18:46Z | PLAN GAP: tests/integration/tests/quote_page.rs:172 (owner T052, protected) bans the word "summary" anywhere in the quot |
| T743 | messaging and privacy: opt-out listings are re-keyed to the  | QUEUED | 1 | 112 min | 18:17Z |  |
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

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 356 of 735 (48.4%) · partial 360 · unstarted 19
**Tasks merged (throughput only):** 562 of 726

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
| terms | 27 | 8 | 0 | 35 |
| website | 20 | 10 | 0 | 30 |
| wording | 28 | 4 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-21T01:26:35Z (week)): week: 119.0M used, 75.0M left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-04T18:23:55Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 205 of 306 done
- stage 6: 0 of 67 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T664 (lease until 2026-11-21T02:09:28Z)
- lane 2: T774 (lease until 2026-11-21T04:32:37Z)
- lane 3: T777 (lease until 2026-11-21T04:31:36Z)
- lane 5: T769 (lease until 2026-11-21T04:14:17Z)
- lane 6: T580 (lease until 2026-11-21T04:10:04Z)
- lane 7: T775 (lease until 2026-11-21T04:29:12Z)

## Last usage reading
- {"task": "T674", "at": "2026-11-21T01:25:56Z", "cache_read_input_tokens": 8255113, "output_tokens": 77625, "cache_creation_input_tokens": 212666, "input_tokens": 142}
