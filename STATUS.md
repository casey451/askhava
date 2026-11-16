# Ask Hava build status

Updated 2026-11-16T00:29:23Z by runner ff584ab99111 (manual).

**Building: T516 in progress since 00:18Z; 353 of 502 done; week 70% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 353 of 502 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 1: T516 claimed 00:18Z, lease until 03:18Z; lane 2: T099 claimed 00:13Z, lease until 03:13Z
- **Week:** 70% used, 53.7M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 00:18Z, 10 min before this update
- **Keeper:** last ran 00:02Z, next wake 00:22Z

## Needs attention
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- Strict plan check red: 21 problem(s) on tasks still to run (+121 on DONE tasks); first: T399: stored context_pct 15% but it measures 21%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T516 | Tooling (harness): scripts/check_duplicates.py, the duplicat | QUEUED | 1 | 47 min | 00:01Z | review_missing on d8e9b6a9822f (D-3728): not proven reviewed or worker-made / (b) 927e34944d5f T516: scripts/check_dupli |
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
| T460 | server: business sign-up's identity steps are served (code s | done | 2 | 68 min | 01:08Z | PLAN GAP: app/businesses/src/models.rs:269, :307 (pub(crate) SignupState, signup_state; owned by T041, T081, T083). D-36 |
| T454 | server: the served business portal end to end (TD-BIZ-42's r | check_failed | 1 | 12 min | 00:56Z |  |

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 293 of 733 (40.0%) · partial 283 · unstarted 157
**Tasks merged (throughput only):** 353 of 502

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 13 | 16 | 6 | 35 |
| ai | 11 | 13 | 5 | 29 |
| businesses | 21 | 34 | 3 | 58 |
| calls | 12 | 12 | 1 | 25 |
| consent | 15 | 16 | 4 | 35 |
| fees | 18 | 33 | 1 | 52 |
| intake | 29 | 11 | 3 | 43 |
| job-cards | 19 | 5 | 0 | 24 |
| licensing | 38 | 10 | 0 | 48 |
| lifecycle | 21 | 7 | 0 | 28 |
| messaging | 22 | 21 | 2 | 45 |
| ops-recovery | 1 | 10 | 43 | 54 |
| privacy | 11 | 18 | 2 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 35 | 20 | 1 | 56 |
| recurring | 8 | 0 | 3 | 11 |
| security | 10 | 40 | 5 | 55 |
| terms | 1 | 2 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 1 | 10 | 21 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-16T00:01:40Z (week)): week: 140.3M used, 53.7M left, resets 2026-11-19T17:00:00Z; projected finish 2026-11-28T05:09:44Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 233 of 238 done
- stage 5: 0 of 110 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T516 (lease until 2026-11-16T03:18:18Z)
- lane 2: T099 (lease until 2026-11-16T03:13:18Z)

## Last usage reading
- {"task": "T516", "at": "2026-11-16T00:01:04Z", "cache_creation_input_tokens": 189310, "input_tokens": 134, "cache_read_input_tokens": 2034709, "output_tokens": 39179}
