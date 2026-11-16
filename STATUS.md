# Ask Hava build status

Updated 2026-11-16T01:39:52Z by runner ff584ab99111 (manual).

**Building: T516 in progress since 01:12Z; 353 of 505 done; week 73% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 353 of 505 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 1: T516 claimed 01:12Z, lease until 04:12Z; lane 2: T099 claimed 00:13Z, lease until 03:13Z
- **Week:** 73% used, 47.7M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 01:18Z, 21 min before this update
- **Keeper:** last ran 01:00Z, next wake 03:15Z

## Needs attention
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- Strict plan check red: 21 problem(s) on tasks still to run (+121 on DONE tasks); first: T399: stored context_pct 15% but it measures 21%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T516 | Tooling (harness): scripts/check_duplicates.py, the duplicat | QUEUED | 2 | 40 min | 00:58Z | Second review of head 85baf148e5f34e6d4d48b8c0a54b3e8ff92fdbdd found FINDINGS after the one fix round: scripts/check_dup |
| T516 | Tooling (harness): scripts/check_duplicates.py, the duplicat | QUEUED | 1 | 47 min | 00:01Z |  |
| T515 | End-to-end harness: specs can deliver a validly signed Strip | done | 2 | 50 min | 23:47Z | PLAN GAP: app/adapters/src/tests/ssrf_guard.rs:488 is owned by T176 and asserts the stand-in PaymentIntent has no livemo |
| T515 | End-to-end harness: specs can deliver a validly signed Strip | BLOCKED | 1 | 17 min | 22:11Z |  |
| T099 | Journey J2 (business) and its variants, keyboard-only, with  | BLOCKED | 1 | 58 min | 21:21Z | PLAN GAP: tests/e2e/webserver.ts:132 (T492/T227 file, forbidden to T099 by prompt line 126) gives the keyed binary a ran |
| T464 | server and portals: the sign-up capability step is served af | done | 1 | 83 min | 19:56Z |  |
| T495 | e2e: the served Confirm and pay journey in the browser again | done | 1 | 44 min | 18:21Z |  |
| T462 | server: the sign-up inbox check is served behind the busines | done | 3 | 35 min | 18:05Z | PLAN GAP: app/portals/templates/email_check.html:47 owner T149 (done, #354), and app/portals/src/feedback.rs:61 owner T4 |
| T261 | portals and server (harness): the served Confirm route under | done | 6 | 85 min | 16:50Z | PLAN GAP: tests/e2e/confirm_served.spec.ts can't be built in T261. The fix belongs to tests/e2e/webserver.ts, which T227 |
| T455 | server: the Pause, Resume and Ask about a charge email links | done | 1 | 38 min | 14:58Z |  |
| T462 | server: the sign-up inbox check is served behind the busines | BLOCKED | 2 | 2 min | 14:14Z |  |
| T261 | portals and server (harness): the served Confirm route under | check_failed | 5 | 44 min | 13:32Z |  |
| T462 | server: the sign-up inbox check is served behind the busines | check_failed | 1 | 30 min | 12:13Z |  |
| T261 | portals and server (harness): the served Confirm route under | check_failed | 4 | 2 h 6 min | 11:16Z |  |
| T461 | server: business sign-up's service area, email code and acti | done | 4 | 54 min | 08:38Z | PLAN GAP: T461's acceptance says a flagged (NeedsReview) sign-up's done screen says it waits for review, but the wording |
| T454 | server: the served business portal end to end (TD-BIZ-42's r | done | 2 | 53 min | 07:15Z | Ci: failure; Coverage: failure; Checks: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned |
| T493 | portals (harness): the Confirm page's own Stripe CSP marker  | done | 2 | 50 min | 06:16Z | Ci: failure; Checks: failure / Checks failed at step: Rust build (the one boundary build) and tests (all but SPLIT_TESTS |
| T461 | server: business sign-up's service area, email code and acti | BLOCKED | 3 | 49 min | 06:10Z |  |
| T493 | portals (harness): the Confirm page's own Stripe CSP marker  | check_failed | 1 | 46 min | 04:47Z |  |
| T461 | server: business sign-up's service area, email code and acti | QUEUED | 2 | 93 min | 04:34Z |  |
| T494 | portals: the Confirm route's $0 free-first-win decision from | done | 1 | 29 min | 04:31Z |  |
| T492 | e2e (harness): the keyed end-to-end binary gets the Stripe s | done | 1 | 31 min | 04:26Z |  |
| T261 | portals and server (harness): the served Confirm route under | BLOCKED | 3 | 110 min | 02:57Z |  |
| T461 | server: business sign-up's service area, email code and acti | QUEUED | 1 | 60 min | 02:31Z |  |

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 264 of 733 (36.0%) · partial 312 · unstarted 157
**Tasks merged (throughput only):** 353 of 502

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 12 | 17 | 6 | 35 |
| ai | 11 | 13 | 5 | 29 |
| businesses | 21 | 34 | 3 | 58 |
| calls | 10 | 14 | 1 | 25 |
| consent | 11 | 20 | 4 | 35 |
| fees | 13 | 38 | 1 | 52 |
| intake | 26 | 14 | 3 | 43 |
| job-cards | 18 | 6 | 0 | 24 |
| licensing | 33 | 15 | 0 | 48 |
| lifecycle | 21 | 7 | 0 | 28 |
| messaging | 22 | 21 | 2 | 45 |
| ops-recovery | 1 | 10 | 43 | 54 |
| privacy | 10 | 19 | 2 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 31 | 24 | 1 | 56 |
| recurring | 6 | 2 | 3 | 11 |
| security | 9 | 41 | 5 | 55 |
| terms | 1 | 2 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 1 | 10 | 21 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-16T01:00:07Z (week)): week: 146.3M used, 47.7M left, resets 2026-11-19T17:00:00Z; projected finish 2026-11-28T09:10:59Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 233 of 238 done
- stage 5: 0 of 113 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T516 (lease until 2026-11-16T04:12:02Z)
- lane 2: T099 (lease until 2026-11-16T03:13:18Z)

## Last usage reading
- {"task": "T516", "at": "2026-11-16T00:58:23Z", "cache_creation_input_tokens": 145701, "output_tokens": 34484, "cache_read_input_tokens": 2140646, "input_tokens": 124}
