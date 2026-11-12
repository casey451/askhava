# Ask Hava build status

Updated 2026-11-12T00:51:49Z by runner 2f7ee2336488 (actions).

**Building: T368 in progress since 00:48Z; 217 of 382 done; week 79% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 217 of 382 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 4: T368 claimed 00:48Z, lease until 03:48Z
- **Week:** 79% used, 36.0M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 00:51Z, 0 min before this update
- **Keeper:** last ran 00:50Z, next wake 03:50Z

## Needs attention
- Strict plan check red: 5 problem(s) on tasks still to run (+67 on DONE tasks); first: T106: stored context_pct 15% but it measures 19%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T317 | server and intake: the inbound text route applies SEC-014's  | MERGE_PENDING | 2 | 53 min | 00:16Z | PLAN GAP: tests/integration/tests/suspension_mode.rs:69-78 (owned by T279) and app/server/tests/voice_confirm_line.rs:95 |
| T313 | messaging and server: an EMERGENCY card notice goes at night | MERGE_PENDING | 1 | 35 min | 00:03Z |  |
| T228 | businesses: leaving Ask Hava, the optional reason and what h | MERGE_PENDING | 2 | 23 min | 23:45Z | merging main into the reviewed head conflicts in app/picks/CLAUDE.md, app/quotes/src/api.rs; a resync merges it in on th |
| T334 | payments: the nightly reconcile catches up after an outage f | MERGE_PENDING | 2 | 96 min | 23:36Z | Worker stopped with PLAN GAP: reconcile_open.rs:343 and :423 (T288 tests, night 0 then night 8) fail once the run reads  |
| T348 | harness: the authorization matrix gets public, owner and act | done | 1 | 43 min | 23:32Z |  |
| T228 | businesses: leaving Ask Hava, the optional reason and what h | resync | 2 | 41 min | 23:04Z |  |
| T317 | server and intake: the inbound text route applies SEC-014's  | BLOCKED | 1 | 4 min | 22:34Z |  |
| T232 | portals and businesses: the Confirm step's "Can't take it" l | done | 1 | 51 min | 22:02Z |  |
| T387 | payments (harness): the branch that flaps payments coverage  | done | 1 | 90 min | 21:49Z |  |
| T388 | picks, intake, messaging and portals: after a Confirm the ho | done | 1 | 34 min | 21:46Z |  |
| T334 | payments: the nightly reconcile catches up after an outage f | QUEUED | 1 | 15 min | 21:21Z |  |
| T228 | businesses: leaving Ask Hava, the optional reason and what h | check_failed | 1 | 31 min | 19:59Z |  |
| T228 | businesses: leaving Ask Hava, the optional reason and what h | resync | 1 | 77 min | 19:09Z |  |
| T316 | businesses: one read of each business's public name for the  | done | 2 | 26 min | 19:02Z | PLAN GAP: app/businesses/src/holding.rs:8 and migrations/0050_licensing_imports.sql:22-55: nothing links a business to a |
| T332 | payments (harness): a late success has its own refund trigge | done | 4 | 25 min | 18:53Z | merging main into the reviewed head conflicts in app/payments/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T345 | harness: every external GitHub Actions `uses:` in every work | BLOCKED | 1 | 8 min | 18:44Z | PLAN GAP: tests/test_runner_keeper.py:417,:426,:652; tests/test_runner_pre_smoke.py:263; tests/test_runner_smoke_reuse.p |
| T332 | payments (harness): a late success has its own refund trigge | resync | 4 | 46 min | 18:10Z |  |
| T272 | portals: the sign-in link cap and the code-try cap are atomi | done | 1 | 20 min | 18:05Z |  |
| T333 | payments (harness): a succeeded refund is recorded at the am | done | 2 | 32 min | 17:20Z | PLAN GAP: app/payments/src/routes.rs:412 (T259) and app/payments/src/refund_state.rs:34 (T270) / app/picks/src/unreachab |
| T272 | portals: the sign-in link cap and the code-try cap are atomi | resync | 1 | 27 min | 17:17Z |  |
| T165 | businesses: business name, Google and Yelp links and license | done | 1 | 18 min | 17:05Z |  |
| T214 | picks (harness): the homeowner's share box at the pick and t | done | 3 | 45 min | 16:44Z | PLAN GAP: tests/integration/tests/page_pick.rs:131 and tests/security/tests/page_pick_privacy.rs:224 (both T246's; page_ |
| T165 | businesses: business name, Google and Yelp links and license | resync | 1 | 47 min | 16:25Z |  |
| T269 | portals: a business notice's signed link opens sign-in for t | done | 2 | 23 min | 16:14Z | merging main into the reviewed head conflicts in app/portals/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T269 | portals: a business notice's signed link opens sign-in for t | resync | 2 | 44 min | 15:46Z |  |
| T384 | quotes and portals: the homeowner sees an edited quote as up | done | 2 | 18 min | 15:05Z | Ci: failure; E2e: failure; Ally: failure / E2e failed at step: Chromium and its system libraries / E: Failed to fetch ht |
| T214 | picks (harness): the homeowner's share box at the pick and t | BLOCKED | 2 | 3 min | 15:04Z |  |
| T342 | quotes: a quote's contact check also reads the whole field w | done | 1 | 51 min | 14:31Z |  |
| T332 | payments (harness): a late success has its own refund trigge | check_failed | 3 | 48 min | 14:24Z |  |
| T269 | portals: a business notice's signed link opens sign-in for t | check_failed | 1 | 20 min | 14:19Z |  |
| T384 | quotes and portals: the homeowner sees an edited quote as up | check_failed | 1 | 40 min | 13:40Z |  |
| T214 | picks (harness): the homeowner's share box at the pick and t | BLOCKED | 1 | 15 min | 13:19Z |  |
| T333 | payments (harness): a succeeded refund is recorded at the am | BLOCKED | 1 | 50 min | 13:14Z |  |
| T332 | payments (harness): a late success has its own refund trigge | BLOCKED | 2 | 2 min | 12:40Z |  |
| T332 | payments (harness): a late success has its own refund trigge | BLOCKED | 1 | 11 min | 12:10Z |  |
| T164 | quotes: one quote format per category, screened notes, and e | done | 3 | 19 min | 12:02Z | merging main into the reviewed head conflicts in app/quotes/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T331 | payments (harness): a payment success with no readable pick  | done | 1 | 49 min | 11:35Z |  |
| T164 | quotes: one quote format per category, screened notes, and e | resync | 3 | 12 min | 11:31Z |  |
| T164 | quotes: one quote format per category, screened notes, and e | check_failed | 2 | 39 min | 11:00Z |  |
| T278 | quotes, intake and picks: the check-in, feedback and unreach | done | 1 | 29 min | 10:56Z |  |
| T316 | businesses: one read of each business's public name for the  | BLOCKED | 1 | 2 min | 10:06Z |  |
| T337 | payments (harness): a $0 release proof is bound to the pick' | done | 5 | 23 min | 09:49Z | PLAN GAP: app/server/tests/release_recovery.rs:572-576 (td_pick_50_a_zero_fee_releases_free_and_a_cancelled_pick_nothing |
| T327 | harness: every task kind the app queues has a handler in the | done | 1 | 48 min | 09:38Z |  |
| T164 | quotes: one quote format per category, screened notes, and e | BLOCKED | 1 | 14 min | 09:03Z |  |
| T295 | payments, ai, intake and picks take the brand from the brand | done | 4 | 33 min | 08:57Z | T295 blocked at step 3: tests/brand_literal_baseline.json still lists files T295 does not own: app/messaging/templates/e |
| T212 | intake: photo asks with on-server checks and preview, inspir | done | 2 | 19 min | 08:12Z | Ci: failure; Coverage: failure; Checks: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned |
| T337 | payments (harness): a $0 release proof is bound to the pick' | BLOCKED | 4 | 9 min | 08:05Z |  |
| T212 | intake: photo asks with on-server checks and preview, inspir | check_failed | 1 | 56 min | 07:20Z |  |
| T383 | messaging and portals: the text templates and the homeowner  | done | 1 | 20 min | 05:39Z |  |
| T337 | payments (harness): a $0 release proof is bound to the pick' | BLOCKED | 3 | 16 min | 05:11Z |  |
| T209 | picks and payments: the optional auto-confirm-and-pay box wi | done | 1 | 115 min | 05:06Z |  |
| T295 | payments, ai, intake and picks take the brand from the brand | BLOCKED | 3 | 0 min | 04:59Z |  |
| T337 | payments (harness): a $0 release proof is bound to the pick' | QUEUED | 2 | 25 min | 04:51Z |  |
| T295 | payments, ai, intake and picks take the brand from the brand | QUEUED | 2 | 0 min | 04:49Z |  |
| T295 | payments, ai, intake and picks take the brand from the brand | QUEUED | 1 | 1 min | 04:44Z |  |
| T374 | licensing and businesses: one read of everything a business  | done | 1 | 45 min | 03:08Z |  |
| T157 | picks: the homeowner's backup choice at the pick | done | 2 | 22 min | 02:41Z | E2e: failure; Ally: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / {"timestamp":"2026-11-1 |
| T157 | picks: the homeowner's backup choice at the pick | check_failed | 1 | 66 min | 02:10Z |  |
| T337 | payments (harness): a $0 release proof is bound to the pick' | QUEUED | 1 | 31 min | 02:07Z |  |

### Plan and harness merges on main
- 00:38Z Plan: T261 pre-flight fix and catalog loaded at start-up (T400, IC-96) (D-3100) (#534)
- 00:00Z Plan: pre-flight fixes round 3 — T360, T368, T350, T358, T366, T385 (D-3094) (#528)
- 23:50Z Plan: Gate C preconditions, vendor-leak check, quote cards, AI writing calls (D-3080) (#518)
- 23:07Z Plan: IC-74 owners T390/T391, migration renumber, budget refresh, IC-95 logged (D-3079) (#517)
- 23:02Z Plan: pre-flight fixes for T317, T323, T313, T315, T347 and the T345 gap (D-3083) (#524)
- 22:35Z Harness: smoke ancestry reuse across DONE task merges and a running duplicate smoke yields (D-3058) (#516)
- 21:48Z Harness: no five-hour pacing budget (D-3056) (#513)
- 20:03Z Harness: the critical-path test checks what the runner can claim, not live in-flight work (D-3057) (#514)
- 19:58Z Plan: pre-flight fixes for five queued tasks before they're claimed (D-3055) (#512)
- 18:49Z Plan: five lanes (D-3043) (#504)
- 18:37Z Plan: T387 steadies payments branch coverage and adds margin, claimed first (D-3049) (#507)
- 15:44Z Plan: T214 owns the final-Pick share box end to end and ticks it in five finished tests (D-3032) (#495)
- 15:02Z Plan: T333 may book the webhook refund amount and hold the refunded notice on an unchecked refund (D-3018) (#487)
- 14:48Z Plan: T214 builds the pick notice's "Can't take it" value; new T386 adds the line to both languages (D-3024) (#489)
- 14:34Z Harness: browser setup survives an Ubuntu mirror outage (D-3030) (#492)
- 13:45Z Plan: four lanes (D-2986) (#483)
- 13:14Z Plan: T332 updates the ended-pick test — success stored, one refund queued, still never cleared (D-3007) (#484)
- 12:26Z Plan: T332 may expect AlreadyQueued after its own check round queues the late success (D-2970) (#482)
- 11:00Z Plan: sign-up reads the business's own state record — name check and identity contacts (T385, D-2954) (#475)
- 10:38Z Plan: businesses give their public name at sign-up; T316 reads it (D-2943) (#473)
- 09:54Z Harness: a planned file deletion is not a plan gap — 5 eligible tasks were held (D-2922) (#470)
- 09:40Z Plan: split T164 — new T384 shows edited/withdrawn quotes in place; LIST numbers match picks (D-2912) (#468)
- 08:37Z Plan: T337 also owns release_recovery.rs for a fixture-only fix (D-2896) (#464)
- 07:43Z Harness: td_pick_09 asks on the fixed clock — test failing everywhere since 07:00Z (D-2886) (#461)
- 06:32Z Harness: a worker's plan-gap report blocks the task instead of a retry on the same prompt (D-2859) (#458)
- 06:12Z Harness: the state schema accepts the hand-in update merge — runner stopped with state_invalid (D-2870) (#459)
- 05:26Z Plan: T337 owns its confirm_and_pay fixture rows; paid-pick guard keeps a test (D-2832) (#453)
- 05:08Z Plan: new T383 removes the brand words left in the text templates and homeowner page; T295 waits for it (D-2843) (#455)
- 04:14Z Plan: three lanes (D-2810) (#451)
- 04:04Z Plan: shorten the stage-6 critical path by removing ordering-only dependencies (D-2821) (#452)
- 03:53Z Harness: merge main before the hand-in push when the head is behind (D-2798) (#450)
- 03:41Z Plan: fix tasks from the Nov 11 vendor-double review (D-2760–D-2770) (#446)
- 03:25Z Harness: CI speed — cache saves on main only, pytest in its own job, C YAML loader (D-2782) (#449)
- 01:56Z Harness: the test suite drops the live job's smoke switch and run ids (D-2660) (#441)
- 01:46Z Plan: fix tasks from the Nov 11 businesses and catalog review (D-2700–D-2712) (#442)

## Progress (requirements verified)

**Requirements verified:** 214 of 733 (29.2%) · partial 345 · unstarted 174
**Tasks merged (throughput only):** 217 of 382

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 13 | 16 | 6 | 35 |
| ai | 7 | 17 | 5 | 29 |
| businesses | 16 | 36 | 6 | 58 |
| calls | 9 | 15 | 1 | 25 |
| consent | 8 | 23 | 4 | 35 |
| fees | 12 | 39 | 1 | 52 |
| intake | 21 | 16 | 6 | 43 |
| job-cards | 12 | 10 | 2 | 24 |
| licensing | 22 | 26 | 0 | 48 |
| lifecycle | 18 | 10 | 0 | 28 |
| messaging | 18 | 24 | 3 | 45 |
| ops-recovery | 2 | 6 | 46 | 54 |
| privacy | 7 | 22 | 2 | 31 |
| quiet-hours | 4 | 5 | 0 | 9 |
| quotes-pick | 31 | 24 | 1 | 56 |
| recurring | 6 | 2 | 3 | 11 |
| security | 6 | 43 | 6 | 55 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 2 | 5 | 25 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-12T00:46:13Z (week)): week: 158.0M used, 36.0M left, resets 2026-11-12T17:00:00Z; projected finish 2026-12-05T23:19:00Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 97 of 172 done
- stage 5: 0 of 58 done
- stage 6: 0 of 37 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 4: T368 (lease until 2026-11-12T03:48:23Z)

## Last usage reading
- {"task": "T317", "at": "2026-11-12T00:16:53Z", "output_tokens": 147374, "cache_creation_input_tokens": 766037, "cache_read_input_tokens": 24069543, "input_tokens": 464}
