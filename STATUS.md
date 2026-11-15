# Ask Hava build status

Updated 2026-11-15T21:26:53Z by runner ef9418c0e6b9 (manual).

**Idle since 21:21Z; 352 of 499 done; week 64% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 352 of 499 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** idle since 21:21Z (no task in progress)
- **Week:** 64% used, 66.0M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 21:23Z, 3 min before this update
- **Keeper:** last ran 21:22Z, next wake 21:42Z

## Needs attention
- BLOCKED T099: plan gap reported: PLAN GAP: tests/e2e/webserver.ts:132 (T492/T227 file, forbidden to T099 by prompt line 126) gives the keyed binary a random Stripe webhook secret never exposed to specs; the stand-in signs with a different secret (stand_ins/stripe.py:82), so no spec can post a valid signed webhook and the post-payment closing checks (portal view, reload, one release row, no second fee, D-1739) cannot run. Reviewer confirmed the gap. Smallest fix: a harness task passing the keyed webhook secret to specs as E2E_KEYED_* . Reviewer round 2 also noted: free-win $0 Confirm release checks possible without webhook (te; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T099 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- Strict plan check red: 21 problem(s) on tasks still to run (+121 on DONE tasks); first: T399: stored context_pct 15% but it measures 21%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
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
| T486 | businesses and catalog: the free-first-win join dates: a pub | done | 1 | 65 min | 23:37Z |  |
| T451 | quotes, picks, jobs, payments and server: the "Your numbers" | done | 1 | 41 min | 23:30Z |  |
| T445 | runner (harness): a failed nightly on an older head keeps th | done | 1 | 24 min | 23:29Z |  |
| T301 | server and portals: the email action links are served (every | done | 3 | 27 min | 23:24Z | merging main into the reviewed head conflicts in tests/a11y/page_states.ts; a resync merges it in on this attempt (D-891 |
| T485 | businesses: a public read-only reader of a sign-up's identit | done | 1 | 28 min | 22:54Z |  |
| T301 | server and portals: the email action links are served (every | resync | 3 | 58 min | 22:46Z |  |
| T443 | --after runs the full smoke itself on a fix-round hand-in; t | done | 1 | 35 min | 22:19Z |  |
| T300 | server and portals: the business portal is served behind the | done | 3 | 17 min | 21:59Z | merging main into the reviewed head conflicts in app/server/src/lib.rs; a resync merges it in on this attempt (D-891) |
| T261 | portals and server (harness): the served Confirm route under | BLOCKED | 2 | 4 min | 21:38Z |  |
| T460 | server: business sign-up's identity steps are served (code s | BLOCKED | 1 | 4 min | 21:31Z |  |

### Plan and harness merges on main
- 22:18Z Plan: sign-up progress reader (T485) and free-win join dates (T486) unblock T460 and T261 (D-3779) (#807)

## Progress (requirements verified)

**Requirements verified:** 288 of 733 (39.3%) · partial 288 · unstarted 157
**Tasks merged (throughput only):** 352 of 499

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 12 | 17 | 6 | 35 |
| ai | 11 | 13 | 5 | 29 |
| businesses | 21 | 34 | 3 | 58 |
| calls | 12 | 12 | 1 | 25 |
| consent | 15 | 16 | 4 | 35 |
| fees | 17 | 34 | 1 | 52 |
| intake | 27 | 13 | 3 | 43 |
| job-cards | 19 | 5 | 0 | 24 |
| licensing | 38 | 10 | 0 | 48 |
| lifecycle | 21 | 7 | 0 | 28 |
| messaging | 22 | 21 | 2 | 45 |
| ops-recovery | 1 | 10 | 43 | 54 |
| privacy | 10 | 19 | 2 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 35 | 20 | 1 | 56 |
| recurring | 8 | 0 | 3 | 11 |
| security | 10 | 40 | 5 | 55 |
| terms | 1 | 2 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 1 | 10 | 21 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-15T19:57:20Z (week)): week: 128.0M used, 66.0M left, resets 2026-11-19T17:00:00Z; projected finish 2026-11-27T05:44:08Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 232 of 235 done
- stage 5: 0 of 110 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing

## Last usage reading
- {"task": "T099", "at": "2026-11-15T21:21:50Z", "input_tokens": 377, "output_tokens": 114147, "cache_read_input_tokens": 16057184, "cache_creation_input_tokens": 375422}
