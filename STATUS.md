# Ask Hava build status

Updated 2026-11-15T03:37:34Z by runner feafd8f43d86 (actions).

**Building: T461 in progress since 03:00Z; 342 of 476 done; week 56% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 342 of 476 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 2: T461 claimed 03:00Z, lease until 06:00Z
- **Week:** 56% used, 84.0M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 03:37Z, 0 min before this update
- **Keeper:** last ran 03:37Z, next wake 06:03Z

## Needs attention
- BLOCKED T261: plan gap reported: PLAN GAP: tests/e2e/confirm_served.spec.ts can't be built in T261. The fix belongs to tests/e2e/webserver.ts, which T227, T385, T452 and T469 own (not T261). Its appEnv (:89) and keyedEnv (:107) set no ASKHAVA_STRIPE_WEBHOOK_SECRET, Stripe account, Stripe stand-in URL, publishable key or launch date, so release_sink::stripe_webhook (app/server/src/release_sink.rs:298-300) returns None, confirm_route returns None, and neither e2e binary serves /b/picks/{pick}/confirm. Also the 800-line budget does not hold: +1936 lines across 18 files (owned files about 1680). Reviewer confirmed the ownership c; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T261 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- Strict plan check red: 28 problem(s) on tasks still to run (+112 on DONE tasks); first: T399: stored context_pct 15% but it measures 19%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T261 | portals and server (harness): the served Confirm page under  | BLOCKED | 3 | 110 min | 02:57Z | PLAN GAP: tests/e2e/confirm_served.spec.ts can't be built in T261. The fix belongs to tests/e2e/webserver.ts, which T227 |
| T461 | server: business sign-up's service area, email code and acti | QUEUED | 1 | 60 min | 02:31Z | runner smoke red on 4d7fe1a7723d (bash scripts/smoke.sh, D-3530): / + [] / - [ / -     'T002', / - ] / FAILED tests/test |
| T460 | server: business sign-up's identity steps are served (code s | done | 2 | 68 min | 01:08Z | PLAN GAP: app/businesses/src/models.rs:269, :307 (pub(crate) SignupState, signup_state; owned by T041, T081, T083). D-36 |
| T454 | server: the served business portal end to end (TD-BIZ-42's r | check_failed | 1 | 12 min | 00:56Z | Ci: failure; Coverage: failure; Checks: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned |
| T486 | businesses and catalog: the free-first-win join dates: a pub | done | 1 | 65 min | 23:37Z |  |
| T451 | quotes, picks, jobs, payments and server: the "Your numbers" | done | 1 | 41 min | 23:30Z |  |
| T445 | runner (harness): a failed nightly on an older head keeps th | done | 1 | 24 min | 23:29Z |  |
| T301 | server and portals: the email action links are served (every | done | 3 | 27 min | 23:24Z | merging main into the reviewed head conflicts in tests/a11y/page_states.ts; a resync merges it in on this attempt (D-891 |
| T485 | businesses: a public read-only reader of a sign-up's identit | done | 1 | 28 min | 22:54Z |  |
| T301 | server and portals: the email action links are served (every | resync | 3 | 58 min | 22:46Z |  |
| T443 | --after runs the full smoke itself on a fix-round hand-in; t | done | 1 | 35 min | 22:19Z |  |
| T300 | server and portals: the business portal is served behind the | done | 3 | 17 min | 21:59Z | merging main into the reviewed head conflicts in app/server/src/lib.rs; a resync merges it in on this attempt (D-891) |
| T261 | portals and server (harness): the served Confirm page under  | BLOCKED | 2 | 4 min | 21:38Z |  |
| T460 | server: business sign-up's identity steps are served (code s | BLOCKED | 1 | 4 min | 21:31Z |  |
| T300 | server and portals: the business portal is served behind the | resync | 3 | 94 min | 21:25Z |  |
| T459 | server: business sign-up's form is served (the consent, the  | done | 1 | 69 min | 20:58Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | done | 5 | 40 min | 20:28Z | merging main into the reviewed head conflicts in app/calls/CLAUDE.md, app/quotes/CLAUDE.md; a resync merges it in on thi |
| T468 | portals, businesses and messaging: public paths for sign-up' | done | 1 | 29 min | 20:19Z |  |
| T474 | harness: clearing a red main counts a failed attempt against | done | 1 | 26 min | 20:18Z |  |
| T467 | picks: a pick whose fee terms changed before its Confirm end | done | 1 | 27 min | 20:15Z |  |
| FIX-MAIN |  | done | 1 | 78 min | 17:42Z |  |
| T467 | picks: a pick whose fee terms changed before its Confirm end | resync | 1 | 43 min | 16:28Z |  |
| T439 | --after refuses a hand-in whose head no Opus reviewer approv | done | 1 | 44 min | 16:06Z |  |
| T465 | docs: move the entry lines left under each crate guide's `## | done | 1 | 31 min | 15:55Z |  |
| T302 | server (harness): business sign-up's open doors and served p | done | 6 | 35 min | 14:36Z | PLAN GAP: T302 cannot be built as scoped. (1) No route constant exists for the email-check and sample-pick screens (app/ |
| T261 | portals and server (harness): the served Confirm page under  | BLOCKED | 1 | 3 min | 14:30Z |  |
| T353 | payments: a won dispute records the countering fee's return  | done | 1 | 30 min | 14:15Z |  |
| T432 | intake and server: ask the name-and-address question in prod | done | 1 | 71 min | 13:54Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | done | 4 | 38 min | 13:32Z |  |
| T466 | portals: one shared form stylesheet for native checkboxes an | done | 1 | 27 min | 13:27Z |  |
| T301 | server and portals: the email action links are served (every | BLOCKED | 2 | 8 min | 12:40Z |  |
| T359 | payments: a successful charge writes its own charge ledger e | done | 3 | 72 min | 12:37Z | Ci: failure; Coverage: failure / Coverage failed at step: Compare with tests/coverage_floor.json and refuse a lowered fl |
| T300 | server and portals: the business portal is served behind the | BLOCKED | 2 | 16 min | 12:20Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | update_conflict | 3 | 23 min | 11:56Z |  |
| T302 | server (harness): business sign-up's open doors and served p | check_failed | 5 | 45 min | 11:56Z |  |
| T329 | businesses: the eight business account events queued today ( | done | 2 | 65 min | 11:49Z | PLAN GAP: kit/prompts/T329.md:166 and :172 (task owns no customer copy; stop if writing words a business reads) contradi |
| T453 | quotes: the job's quote rules are read from the stored job,  | done | 1 | 27 min | 11:02Z |  |
| T452 | e2e and Ally (harness): a keyed web binary beside the suite' | done | 1 | 24 min | 10:58Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | resync | 3 | 31 min | 10:58Z |  |
| T359 | payments: a successful charge writes its own charge ledger e | check_failed | 2 | 75 min | 10:54Z |  |
| T435 | calls and server: calls and texts act for the business only  | done | 1 | 30 min | 10:46Z |  |
| T329 | businesses: the eight business account events queued today ( | BLOCKED | 1 | 2 min | 10:28Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | resync | 3 | 52 min | 10:25Z |  |
| T398 | businesses: notice texts go only to approved phones, and a m | done | 1 | 38 min | 09:41Z |  |
| T302 | server (harness): business sign-up's open doors and served p | BLOCKED | 4 | 2 min | 09:18Z |  |
| T301 | server and portals: the email action links are served (every | BLOCKED | 1 | 4 min | 09:13Z |  |
| T398 | businesses: notice texts go only to approved phones, and a m | resync | 1 | 32 min | 09:01Z |  |
| T300 | server and portals: the business portal is served behind the | BLOCKED | 1 | 5 min | 08:59Z |  |
| T280 | server and messaging (harness): the business sign-in pages a | done | 4 | 91 min | 08:04Z | merging main into the reviewed head conflicts in app/server/CLAUDE.md, docs/for-casey-review.md; a resync merges it in o |
| T396 | businesses and messaging (harness): a phone a member links o | done | 2 | 56 min | 07:52Z | merging main into the reviewed head conflicts in docs/architecture.md; a resync merges it in on this attempt (D-891) |
| T359 | payments: a successful charge writes its own charge ledger e | BLOCKED | 1 | 39 min | 07:43Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | update_conflict | 2 | 42 min | 07:38Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | resync | 2 | 30 min | 06:48Z |  |
| T396 | businesses and messaging (harness): a phone a member links o | resync | 2 | 21 min | 06:40Z |  |
| T352 | adapters and stand-in (harness): the PaymentIntent carries S | done | 1 | 51 min | 06:14Z |  |
| T396 | businesses and messaging (harness): a phone a member links o | resync | 2 | 51 min | 06:03Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | resync | 2 | 51 min | 06:02Z |  |
| T280 | server and messaging (harness): the business sign-in pages a | BLOCKED | 3 | 22 min | 05:44Z |  |
| T393 | portals and licensing: the served quote page's cards show ea | done | 1 | 74 min | 05:40Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | update_conflict | 1 | 39 min | 05:04Z |  |
| T396 | businesses and messaging (harness): a phone a member links o | check_failed | 1 | 45 min | 04:54Z |  |
| T449 | server and intake: emergency first texts count toward the re | done | 1 | 34 min | 04:28Z |  |
| T302 | server (harness): business sign-up's open doors and served p | BLOCKED | 3 | 15 min | 04:11Z |  |
| T440 | portals: the quote page's tier choice and the Confirm page's | done | 1 | 50 min | 03:43Z |  |

### Plan and harness merges on main
- 22:18Z Plan: sign-up progress reader (T485) and free-win join dates (T486) unblock T460 and T261 (D-3779) (#807)
- 19:52Z Plan: stage-5 readers and splits, CON-022 purge via definer functions, Spanish switch gates, replay functions (T475–T484, D-3753) (#798)
- 19:20Z Harness: re-point the stale "second opt-out confirmation" mutant after T357 (D-3751) (#796)
- 18:53Z Plan: T437 re-verification after a false FIX-MAIN failure; T474 so FIX-MAIN blames only what it reverted (D-3752) (#797)
- 18:45Z Plan: email sending wired, pick-end texts complete, Can't-take route, T121 unblocked (T469–T473, D-3750) (#794)
- 16:47Z Plan: pre-claim scout fixes for 27 stage-5 tasks (D-3744) (#793)
- 16:40Z Plan: sign-up email code and activation link get real send paths, rate-limited (new T468, D-3743) (#791)
- 15:37Z Plan: fee_terms_changed pick end reason (new T467) so T261's Confirm page can end a pick safely (D-3722) (#788)
- 13:12Z Plan: T301 door-matrix Quote-link cases follow BIZ-038/CARD-023; action pages link forms.css (D-3706) (#781)
- 12:55Z Plan: shared portal forms stylesheet (T466) so business pages pass A11Y-007/009 (D-3699, D-3700) (#780)
- 12:46Z Plan: split T437 so the NOTES.md rule lands before the crate-doc sweep (D-3698) (#777)
- 10:42Z Plan: T329 notice-email copy gap (D-3687) (#768)
- 10:21Z Plan: restructure the business sign-in chain — T300/T301/T302 gaps, 13 ordered follow-up tasks (D-3674, D-3675, D-3676) (#765)
- 07:59Z Plan: T359 adopts two charge-ledger test expectations (TD-FEE-64), records the checked intent amount (D-3658) (#759)
- 06:29Z Plan: T280 build note out of architecture.md after D-3620; sweep T393's late paragraph (D-3652) (#753)
- 05:23Z Plan: T302 door-matrix gap — exact open sign-up step paths, browser-bound sign-up steps (D-3641) (#745)
- 05:15Z Plan: move build notes out of architecture.md into crate NOTES.md; prompts shrink (D-3620) (#744)
- 03:45Z Plan: pre-claim scout fixes (T396, T300 + new T451) and stale-graph claim guard (D-3618, D-3619) (#733)

## Progress (requirements verified)

**Requirements verified:** 303 of 733 (41.3%) · partial 273 · unstarted 157
**Tasks merged (throughput only):** 342 of 476

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 14 | 15 | 6 | 35 |
| ai | 11 | 13 | 5 | 29 |
| businesses | 18 | 37 | 3 | 58 |
| calls | 14 | 10 | 1 | 25 |
| consent | 15 | 16 | 4 | 35 |
| fees | 19 | 32 | 1 | 52 |
| intake | 29 | 11 | 3 | 43 |
| job-cards | 19 | 5 | 0 | 24 |
| licensing | 38 | 10 | 0 | 48 |
| lifecycle | 24 | 4 | 0 | 28 |
| messaging | 26 | 17 | 2 | 45 |
| ops-recovery | 1 | 10 | 43 | 54 |
| privacy | 13 | 16 | 2 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 35 | 20 | 1 | 56 |
| recurring | 8 | 0 | 3 | 11 |
| security | 10 | 40 | 5 | 55 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 2 | 9 | 21 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-15T02:57:49Z (week)): week: 110.0M used, 84.0M left, resets 2026-11-19T17:00:00Z; projected finish 2026-11-26T02:31:37Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 222 of 231 done
- stage 5: 0 of 91 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 2: T461 (lease until 2026-11-15T06:00:54Z)

## Last usage reading
- {"task": "T261", "at": "2026-11-15T02:57:21Z", "input_tokens": 413, "cache_read_input_tokens": 44276665, "cache_creation_input_tokens": 409210, "output_tokens": 145159}
