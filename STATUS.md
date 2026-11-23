# Ask Hava build status

Updated 2026-11-23T07:06:58Z by runner 01ef41068520 (actions).

**Building: T716 in progress since 06:55Z; 626 of 775 done; week 72% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 626 of 775 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T716 claimed 06:55Z, lease until 09:55Z; lane 2: T835 claimed 06:32Z, lease until 09:32Z; lane 3: T730 claimed 06:00Z, lease until 09:00Z; lane 5: T588 claimed 07:02Z, lease until 10:02Z
- **Week:** 72% used, 45.5M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 07:06Z, 0 min before this update
- **Keeper:** last ran 06:49Z, next wake 07:09Z

## Needs attention
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- Strict plan check red: 37 problem(s) on tasks still to run (+514 on DONE tasks); first: T624: stored context_pct 12% but it measures 19%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T716 | intake: the dispatcher's trade questions at step 5: "Which f | QUEUED | 2 | 2 h 6 min | 06:48Z | Round 2 review of head ee718d9818e119e105327a88e6a124fedfc3978a: the round-1 licensing finding is fixed (with_licensed_o |
| T611 | portals: depth, type and press on the portal screens through | MERGE_PENDING | 1 | 2 h 20 min | 06:24Z |  |
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
| T121 | Startup key checks, control router absent in live, supply-ch | BLOCKED | 2 | 12 min | 00:03Z | PLAN GAP: outside ownership: app/portals/src/update_channel.rs / T729 (owner) merged UpdatesForm/ConfirmForm without den |
| T675 | intake and server (harness): the dispatcher asks for photos  | resync | 4 | 81 min | 17:03Z |  |
| T590 | calls, picks and server: the who's-calling and missed-call t | check_failed | 1 | 118 min | 16:40Z | Ci: failure; Coverage: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / test  |
| T122 | docs/account-checklist.md: Part 1 test accounts, Part 2 busi | QUEUED | 1 | 88 min | 16:23Z | Reviewer after the findings round (head d65c4896894680459870359165740cf8b3a2bf57) still ended FINDINGS: / 1. tests/docs/ |
| T725 | site: the home page shows the texting number with its consen | resync | 4 | 82 min | 16:15Z |  |
| T831 | site: the /request start screen's words box sits fully insid | done | 1 | 107 min | 16:13Z |  |
| T729 | portals and privacy: the quote page offers updates by email  | done | 3 | 92 min | 16:10Z | Ci: failure; Coverage: failure / Coverage failed at step: Compare with tests/coverage_floor.json and refuse a lowered fl |
| T397 | server and portals: the owner approves or declines a member- | done | 2 | 2 h 0 min | 15:12Z | PLAN GAP: outside ownership: app/server/tests/door_matrix.rs:1182. owner_jar signs the owner in at key.now + 30s*k per p |
| T833 | pre-hand-in gate test (harness): the Ally-step test holds th | done | 1 | 65 min | 14:05Z |  |
| T117 | docs/runbook.md: outages, safe mode, refunds, disputes, comp | done | 4 | 33 min | 13:54Z | Reviewer after findings round still FINDINGS at 4f79a8b8c4195e68ccfba3a25eaab6d612560536: / 1. docs/runbook.md:139-142 a |
| T830 | server: `server rotate-keys` and `server retire-index-versio | done | 1 | 61 min | 12:46Z |  |
| T729 | portals and privacy: the quote page offers updates by email  | check_failed | 2 | 49 min | 12:26Z |  |
| T610 | site: the D-4246 polish on the Spanish pages, the legal page | done | 2 | 40 min | 12:19Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Crate:     basic |
| T589 | intake and portals: the pick review page is shown in her req | done | 2 | 87 min | 12:04Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Warning:   unmai |
| T675 | intake and server (harness): the dispatcher asks for photos  | BLOCKED | 3 | 2 h 50 min | 11:38Z |  |
| T805 | messaging (harness): the toll-free fallback is refused after | done | 1 | 50 min | 11:32Z |  |
| T829 | server tests (harness): the door matrix's owner sign-ins nev | done | 3 | 46 min | 11:10Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Crate:     basic |
| T725 | site: the home page shows the texting number with its consen | BLOCKED | 3 | 21 min | 10:52Z |  |
| T729 | portals and privacy: the quote page offers updates by email  | resync | 2 | 2 h 32 min | 10:40Z |  |
| T803 | messaging (harness): an opted-out number gets no HELP reply; | BLOCKED | 3 | 25 min | 10:25Z |  |
| T589 | intake and portals: the pick review page is shown in her req | check_failed | 1 | 53 min | 10:04Z |  |
| T725 | site: the home page shows the texting number with its consen | check_failed | 2 | 57 min | 09:57Z |  |
| T610 | site: the D-4246 polish on the Spanish pages, the legal page | check_failed | 1 | 39 min | 09:00Z |  |
| T118 | Journey J3 (operator) keyboard-only, with closing checks | BLOCKED | 4 | 45 min | 08:54Z | PLAN GAP: outside ownership: app/site/templates/request.html, app/site/static/site.css / At 1280x900 the /request chat s |
| T829 | server tests (harness): the door matrix's owner sign-ins nev | check_failed | 2 | 48 min | 08:40Z |  |
| T609 | site (harness): the D-4246 polish on the footer, how it work | done | 3 | 65 min | 07:14Z | PLAN GAP: tests/e2e/web_chat.spec.ts:123 (owned by T489) uses strict locator main.locator('[lang="es"]'); the footer's E |

### Plan and harness merges on main
- 04:05Z Plan: T121 split into T838 (D-5574); Grok G3 fixes for claimable tasks (D-5585) (#1370)
- 00:07Z Plan: pre-flight fixes (D-5567); T837 keeper stage gate, job 32 and copy-flag fixes (D-5568) (#1368)
- 17:04Z Plan: unblock T121/T832/T499/T514 (D-5552); pre-flight fixes job 24 (D-5551) (#1363)
- 12:43Z Plan: pre-flight goal fixes for T666, T778, T820, T524, T825, T831 (D-5526) (#1354)
- 11:22Z Plan: unblock T725 (D-5518) and T803 via T832 Checks timeout 60 min (D-5519) (#1347)
- 11:05Z Plan: T831 fits the /request words box in the first screen; T118 depends on it (A11Y-005, D-5502) (#1343)

## Progress (requirements verified)

**Requirements verified:** 404 of 735 (55.0%) · partial 318 · unstarted 13
**Tasks merged (throughput only):** 626 of 775

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 25 | 10 | 0 | 35 |
| ai | 7 | 19 | 3 | 29 |
| businesses | 29 | 28 | 1 | 58 |
| calls | 17 | 8 | 0 | 25 |
| consent | 17 | 18 | 0 | 35 |
| fees | 27 | 25 | 0 | 52 |
| intake | 11 | 32 | 0 | 43 |
| job-cards | 16 | 8 | 0 | 24 |
| licensing | 36 | 12 | 0 | 48 |
| lifecycle | 21 | 7 | 0 | 28 |
| messaging | 26 | 19 | 0 | 45 |
| ops-recovery | 17 | 31 | 6 | 54 |
| privacy | 12 | 19 | 0 | 31 |
| quiet-hours | 4 | 5 | 0 | 9 |
| quotes-pick | 34 | 22 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 14 | 38 | 3 | 55 |
| terms | 30 | 5 | 0 | 35 |
| website | 22 | 8 | 0 | 30 |
| wording | 29 | 3 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-23T06:49:05Z (week)): week: 148.5M used, 45.5M left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-07T22:04:16Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 269 of 347 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T716 (lease until 2026-11-23T09:55:38Z)
- lane 2: T835 (lease until 2026-11-23T09:32:28Z)
- lane 3: T730 (lease until 2026-11-23T09:00:46Z)
- lane 5: T588 (lease until 2026-11-23T10:02:05Z)

## Last usage reading
- {"task": "T716", "at": "2026-11-23T06:48:28Z", "input_tokens": 286, "cache_creation_input_tokens": 247272, "cache_read_input_tokens": 7217555, "output_tokens": 53198}
