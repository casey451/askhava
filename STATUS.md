# Ask Hava build status

Updated 2026-11-23T11:31:47Z by runner f93d0581a8d9 (actions).

**Building: T819 in progress since 11:03Z; 630 of 775 done; week 74% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 630 of 775 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T819 claimed 11:03Z, lease until 14:03Z; lane 4: T730 claimed 10:33Z, lease until 13:33Z
- **Week:** 74% used, 41.2M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 11:31Z, 0 min before this update
- **Keeper:** last ran 11:27Z, next wake 13:35Z

## Needs attention
- BLOCKED T121: Ci: failure; Checks: failure
Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny)
Crate:     basic-toml
Version:   0.1.10
Warning:   unmaintained
Title:     `basic-toml` is unmaintained
Date:      2026-09-26
ID:        RUSTSEC-2026-0374
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0374
Crate:     proc-macro-error2
Version:   2.0.1
Warning:   unmaintained
Title:     proc-macro-error2 is unmaintained
Date:      2026-06-07
ID:        RUSTSEC-2026-0173
URL:       https://rustsec.org/advisories/RUSTSEC-2026-0173
##[error]Process completed with exit code 1.
Ci failed at step: All five jobs passed
Checks: failure, Lint and slow tests: success, Python: success, Coverage: success, E2e: success
##[error]Process completed with exit code 1.
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- PLAN GAP T836 (since 2026-11-23T07:59:33Z, no attempt used; a plan change fixes it): T836: kit/runner_log.jsonl is named by D-500, but no task owns it and it does not exist
- Strict plan check red: 39 problem(s) on tasks still to run (+517 on DONE tasks); first: T624: stored context_pct 12% but it measures 19%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T646 | site and server: the web chat's new-request limit counts the | MERGE_PENDING | 2 | 54 min | 11:30Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T653 | licensing and server (harness): a typed BTR number is stored | MERGE_PENDING | 1 | 117 min | 11:19Z |  |
| T714 | portals: a Confirm link sent by text is marked text-started  | MERGE_PENDING | 1 | 95 min | 11:00Z |  |
| T666 | jobs, quotes and server: GET /b/jobs/{j} serves the business | QUEUED | 1 | 2 h 1 min | 10:28Z | Reviewer after the findings round (head bf41c81b22bdbd874d7d53cb8ba64ca6bb0737cf): app/server/tests/job_page_route.rs (w |
| T121 | Startup key checks, control router absent in live, supply-ch | check_failed | 3 | 103 min | 10:16Z | PLAN GAP: outside ownership: app/portals/src/update_channel.rs / T729 (owner) merged UpdatesForm/ConfirmForm without den |
| T118 | Journey J3 (operator) keyboard-only, with closing checks | done | 5 | 82 min | 09:58Z | PLAN GAP: outside ownership: app/site/templates/request.html, app/site/static/site.css / At 1280x900 the /request chat s |
| T646 | site and server: the web chat's new-request limit counts the | check_failed | 1 | 59 min | 08:59Z |  |
| T588 | portals and server: a refused business sign-up keeps every f | done | 1 | 101 min | 08:44Z |  |
| T730 | messaging: a homeowner who chose email gets each quote updat | BLOCKED | 2 | 2 min | 08:25Z | PLAN GAP: outside ownership: app/messaging/src/quote_text_sent.rs / Evidence: last_quote_text (quote_text_sent.rs:45-52) |
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

### Plan and harness merges on main
- 10:11Z Plan: T716 purges held first-text words on an under-13 stop; T730 counts update emails (D-5617) (#1386)
- 07:11Z Plan: T716 owns cards_out.rs so the card send uses the counted pairs (D-5606) (#1382)
- 04:05Z Plan: T121 split into T838 (D-5574); Grok G3 fixes for claimable tasks (D-5585) (#1370)
- 00:07Z Plan: pre-flight fixes (D-5567); T837 keeper stage gate, job 32 and copy-flag fixes (D-5568) (#1368)
- 17:04Z Plan: unblock T121/T832/T499/T514 (D-5552); pre-flight fixes job 24 (D-5551) (#1363)
- 12:43Z Plan: pre-flight goal fixes for T666, T778, T820, T524, T825, T831 (D-5526) (#1354)

## Progress (requirements verified)

**Requirements verified:** 402 of 735 (54.7%) · partial 320 · unstarted 13
**Tasks merged (throughput only):** 630 of 775

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 25 | 10 | 0 | 35 |
| ai | 7 | 19 | 3 | 29 |
| businesses | 29 | 28 | 1 | 58 |
| calls | 17 | 8 | 0 | 25 |
| consent | 17 | 18 | 0 | 35 |
| fees | 27 | 25 | 0 | 52 |
| intake | 11 | 32 | 0 | 43 |
| job-cards | 15 | 9 | 0 | 24 |
| licensing | 36 | 12 | 0 | 48 |
| lifecycle | 21 | 7 | 0 | 28 |
| messaging | 25 | 20 | 0 | 45 |
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
- Pacing (session_reading 2026-11-23T11:02:07Z (week)): week: 152.8M used, 41.2M left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-07T22:05:54Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 273 of 347 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T819 (lease until 2026-11-23T14:03:21Z)
- lane 4: T730 (lease until 2026-11-23T13:33:05Z)

## Last usage reading
- {"task": "T646", "at": "2026-11-23T11:30:41Z", "output_tokens": 9073, "cache_creation_input_tokens": 64867, "input_tokens": 50, "cache_read_input_tokens": 790522}
