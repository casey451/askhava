# Ask Hava build status

Updated 2026-11-23T23:45:04Z by runner c885e3e908d7 (actions).

**Building: T737 in progress since 22:49Z; 641 of 779 done; week 86% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 641 of 779 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T737 claimed 22:49Z, lease until 01:49Z; lane 2: T716 claimed 22:02Z, lease until 02:45Z; lane 3: T738 claimed 23:08Z, lease until 02:08Z
- **Week:** 86% used, 20.4M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 23:44Z, 0 min before this update
- **Keeper:** last ran 23:37Z, next wake 01:05Z

## Needs attention
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- Strict plan check red: 50 problem(s) on tasks still to run (+535 on DONE tasks); first: T624: stored context_pct 12% but it measures 19%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T649 | intake, site and server: after the code, the web chat's name | MERGE_PENDING | 1 | 2 h 19 min | 22:52Z |  |
| T819 | portals: her feedback words are scrubbed before they are sto | MERGE_PENDING | 3 | 85 min | 22:43Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T816 | messaging: the data-request confirm link goes by email to th | done | 1 | 112 min | 22:39Z |  |
| T737 | portals, ai and server: the owner records the AI provider's  | check_failed | 1 | 65 min | 21:59Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T745 | intake: a reader for a job's intake answers through `intake: | done | 1 | 2 h 19 min | 21:22Z |  |
| T836 | ci and runner workflow (harness): the backup-runner gate ski | BLOCKED | 1 | 11 min | 20:40Z | PLAN GAP: tests/test_runner_five_lanes.py:176 (test_no_workflow_a_hand_in_starts_cancels_another_lanes_run_when_five_han |
| T668 | server: the business job page at /b/jobs/{j} offers "Not a f | done | 1 | 56 min | 20:07Z |  |
| T812 | businesses and portals: a passed step-up records the session | done | 1 | 89 min | 19:31Z |  |
| T730 | messaging: a homeowner who chose email gets each quote updat | done | 4 | 76 min | 19:13Z | PLAN GAP: outside ownership: app/messaging/src/quote_text_sent.rs / Evidence: last_quote_text (quote_text_sent.rs:45-52) |
| T222 | Journeys J1 and J2, the full variants: web chat, photos, que | QUEUED | 2 | 97 min | 17:54Z | Reviewer after the fix round still ended REVIEW-VERDICT: FINDINGS over 0aedbd3769e83a983d24d694c7abe05f8773a57a: / 1. do |
| T782 | server: the released job's page shows its real job number | done | 1 | 97 min | 16:56Z |  |
| T121 | Startup key checks, control router absent in live, supply-ch | done | 4 | 100 min | 16:52Z | PLAN GAP: outside ownership: app/portals/src/update_channel.rs / T729 (owner) merged UpdatesForm/ConfirmForm without den |
| T714 | portals: a Confirm link sent by text is marked text-started  | done | 4 | 88 min | 15:49Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Warning:   unmai |
| T222 | Journeys J1 and J2, the full variants: web chat, photos, que | QUEUED | 1 | 67 min | 15:38Z |  |
| T666 | jobs, quotes and server: GET /b/jobs/{j} serves the business | done | 2 | 67 min | 14:25Z | Reviewer after the findings round (head bf41c81b22bdbd874d7d53cb8ba64ca6bb0737cf): app/server/tests/job_page_route.rs (w |
| T819 | portals: her feedback words are scrubbed before they are sto | check_failed | 2 | 68 min | 13:45Z |  |
| T646 | site and server: the web chat's new-request limit counts the | done | 3 | 99 min | 13:40Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Crate:     basic |
| T730 | messaging: a homeowner who chose email gets each quote updat | check_failed | 3 | 2 h 46 min | 13:19Z |  |
| T714 | portals: a Confirm link sent by text is marked text-started  | BLOCKED | 3 | 36 min | 13:10Z |  |
| T653 | licensing and server (harness): a typed BTR number is stored | done | 2 | 65 min | 13:02Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Warning:   unmai |
| T714 | portals: a Confirm link sent by text is marked text-started  | check_failed | 2 | 47 min | 12:27Z |  |
| T819 | portals: her feedback words are scrubbed before they are sto | check_failed | 1 | 54 min | 11:57Z |  |
| T646 | site and server: the web chat's new-request limit counts the | check_failed | 2 | 54 min | 11:30Z |  |
| T653 | licensing and server (harness): a typed BTR number is stored | check_failed | 1 | 117 min | 11:19Z |  |
| T714 | portals: a Confirm link sent by text is marked text-started  | check_failed | 1 | 95 min | 11:00Z |  |
| T666 | jobs, quotes and server: GET /b/jobs/{j} serves the business | QUEUED | 1 | 2 h 1 min | 10:28Z |  |
| T121 | Startup key checks, control router absent in live, supply-ch | check_failed | 3 | 103 min | 10:16Z |  |
| T118 | Journey J3 (operator) keyboard-only, with closing checks | done | 5 | 82 min | 09:58Z | PLAN GAP: outside ownership: app/site/templates/request.html, app/site/static/site.css / At 1280x900 the /request chat s |
| T646 | site and server: the web chat's new-request limit counts the | check_failed | 1 | 59 min | 08:59Z |  |
| T588 | portals and server: a refused business sign-up keeps every f | done | 1 | 101 min | 08:44Z |  |
| T730 | messaging: a homeowner who chose email gets each quote updat | BLOCKED | 2 | 2 min | 08:25Z |  |
| T730 | messaging: a homeowner who chose email gets each quote updat | QUEUED | 1 | 2 h 19 min | 08:20Z |  |
| T716 | intake: the dispatcher's trade questions at step 5: "Which f | BLOCKED | 3 | 81 min | 08:17Z | PLAN GAP: outside ownership: app/intake/src/conversation.rs (purge_child, ~348-362), app/intake/src/models.rs (purge_req |
| T835 | runner (harness): a cancelled run on a superseded head neith | done | 1 | 52 min | 07:24Z |  |
| T716 | intake: the dispatcher's trade questions at step 5: "Which f | QUEUED | 2 | 2 h 6 min | 06:48Z |  |
| T611 | portals: depth, type and press on the portal screens through | done | 1 | 2 h 20 min | 06:24Z |  |
| T838 | portals, server and calls: the sign-in, quote-action and sig | done | 1 | 86 min | 06:07Z |  |
| T834 | ci (harness): Twin and Ally skip their build when nothing th | done | 3 | 48 min | 05:51Z | After the fix round the reviewer (head f9e32434db512dbbf1f29deb2d94b5848d2e3874) still ends REVIEW-VERDICT: FINDINGS. Fi |
| T803 | messaging (harness): an opted-out number gets no HELP reply; | done | 4 | 81 min | 05:08Z | Ci: failure; Checks: cancelled / Checks failed at step: Named mutants of the files this change touches (scripts/run_name |
| T834 | ci (harness): Twin and Ally skip their build when nothing th | QUEUED | 2 | 85 min | 04:57Z |  |
| T716 | intake: the dispatcher's trade questions at step 5: "Which f | check_failed | 1 | 117 min | 03:28Z |  |
| T834 | ci (harness): Twin and Ally skip their build when nothing th | QUEUED | 1 | 81 min | 03:19Z |  |
| T712 | server and portals: the owner portal lists PROBLEM reports w | done | 1 | 39 min | 02:36Z |  |
| T645 | intake and messaging: the web chat honours an opt-out word,  | done | 2 | 2 h 6 min | 01:58Z | PLAN GAP: app/site/src/chat.rs:340 (owned by T593, then T646 and T649; T593 is not merged yet) calls `chat_message(db, & |
| T837 | runner (harness): the keeper judges work with the real stage | done | 1 | 41 min | 01:47Z |  |
| T712 | server and portals: the owner portal lists PROBLEM reports w | resync | 1 | 90 min | 01:40Z |  |
| T832 | ci (harness): the Checks job allows 60 minutes, so a full na | done | 1 | 79 min | 01:09Z |  |
| T725 | site: the home page shows the texting number with its consen | done | 4 | 49 min | 00:39Z | merging main into the reviewed head conflicts in app/server/src/main.rs; a resync merges it in on this attempt (D-891) |
| T675 | intake and server (harness): the dispatcher asks for photos  | done | 4 | 45 min | 00:33Z | merging main into the reviewed head conflicts in app/server/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T121 | Startup key checks, control router absent in live, supply-ch | BLOCKED | 2 | 12 min | 00:03Z |  |

### Plan and harness merges on main
- 23:24Z Plan: token efficiency and phase-2 pre-flight (D-5676); smoke throughput harness task T843, T836 unblocked (D-5682) (#1412)
- 20:12Z Plan: outside pre-flight fixes for 22 queued tasks, template a11y line, four test-design rows (D-5670) (#1410)
- 19:49Z Plan: pre-flight fixes for ten claimable tasks, J3 licence fixture T841 (D-5657); baseline audit gate deferred (D-5663); outside pre-flight fixes T744/T6
- 16:44Z Plan: T840 update-email entry point and first-quote resend (D-5640); six-task pre-flight fixes (D-5646) (#1402)
- 13:33Z Plan: Email-only quote updates and counts (T840), under-13 purge completeness (T839), T716 test precondition (D-5639) (#1399)
- 10:11Z Plan: T716 purges held first-text words on an under-13 stop; T730 counts update emails (D-5617) (#1386)
- 07:11Z Plan: T716 owns cards_out.rs so the card send uses the counted pairs (D-5606) (#1382)
- 04:05Z Plan: T121 split into T838 (D-5574); Grok G3 fixes for claimable tasks (D-5585) (#1370)
- 00:07Z Plan: pre-flight fixes (D-5567); T837 keeper stage gate, job 32 and copy-flag fixes (D-5568) (#1368)

## Progress (requirements verified)

**Requirements verified:** 428 of 735 (58.2%) · partial 294 · unstarted 13
**Tasks merged (throughput only):** 641 of 778

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 27 | 8 | 0 | 35 |
| ai | 8 | 18 | 3 | 29 |
| businesses | 32 | 25 | 1 | 58 |
| calls | 17 | 8 | 0 | 25 |
| consent | 19 | 16 | 0 | 35 |
| fees | 27 | 25 | 0 | 52 |
| intake | 12 | 31 | 0 | 43 |
| job-cards | 16 | 8 | 0 | 24 |
| licensing | 39 | 9 | 0 | 48 |
| lifecycle | 22 | 6 | 0 | 28 |
| messaging | 25 | 20 | 0 | 45 |
| ops-recovery | 17 | 31 | 6 | 54 |
| privacy | 13 | 18 | 0 | 31 |
| quiet-hours | 4 | 5 | 0 | 9 |
| quotes-pick | 36 | 20 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 20 | 32 | 3 | 55 |
| terms | 30 | 5 | 0 | 35 |
| website | 24 | 6 | 0 | 30 |
| wording | 30 | 2 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-23T23:01:09Z (week)): week: 173.6M used, 20.4M left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-08T00:50:45Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 284 of 351 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T737 (lease until 2026-11-24T01:49:24Z)
- lane 2: T716 (lease until 2026-11-24T02:45:04Z)
- lane 3: T738 (lease until 2026-11-24T02:08:57Z)

## Last usage reading
- {"task": "T649", "at": "2026-11-23T22:52:47Z", "output_tokens": 85056, "input_tokens": 258, "cache_creation_input_tokens": 256403, "cache_read_input_tokens": 17279929}
