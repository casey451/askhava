# Ask Hava build status

Updated 2026-11-11T09:41:27Z by runner 6b803a362f31 (manual).

**Building: T337 in progress since 09:25Z; 199 of 371 done; week 63% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 199 of 371 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 2: T337 claimed 09:25Z, lease until 12:25Z
- **Week:** 63% used, 62.3M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 09:41Z, 0 min before this update
- **Keeper:** last ran 09:41Z, next wake 12:28Z

## Needs attention
- PLAN GAP T269 (since 2026-11-11T09:25:53Z, no attempt used; a plan change fixes it): T269: dependency T294 is DONE but its owned file tests/brand_literal_baseline.json is not on main; T269: dependency T291 is DONE but its owned file tests/brand_literal_baseline.json is not on main
- PLAN GAP T278 (since 2026-11-11T09:25:53Z, no attempt used; a plan change fixes it): T278: dependency T293 is DONE but its owned file tests/brand_literal_baseline.json is not on main; T278: dependency T295 is DONE but its owned file tests/brand_literal_baseline.json is not on main
- PLAN GAP T313 (since 2026-11-11T09:25:53Z, no attempt used; a plan change fixes it): T313: dependency T292 is DONE but its owned file tests/brand_literal_baseline.json is not on main
- PLAN GAP T323 (since 2026-11-11T09:25:53Z, no attempt used; a plan change fixes it): T323: dependency T295 is DONE but its owned file tests/brand_literal_baseline.json is not on main
- PLAN GAP T326 (since 2026-11-11T09:25:53Z, no attempt used; a plan change fixes it): T326: dependency T292 is DONE but its owned file tests/brand_literal_baseline.json is not on main
- OWNED GAP T295 (DONE, since 2026-11-11T09:25:53Z; a later task or plan change must add it): T295: tests/brand_literal_baseline.json missing on main
- Strict plan check red: 11 problem(s) on tasks still to run (+19 on DONE tasks); first: T255: stored context_pct 34% but it measures 38%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T327 | harness: every task kind the app queues has a handler in the | MERGE_PENDING | 1 | 48 min | 09:38Z |  |
| T164 | quotes: one quote format per category, screened notes, and e | BLOCKED | 1 | 14 min | 09:03Z | PLAN GAP: app/portals/src/tests/prop_arrival_order.rs:22 (T052) and app/quotes/src/notify.rs:179 / app/quotes/src/models |
| T295 | payments, ai, intake and picks take the brand from the brand | done | 4 | 33 min | 08:57Z | T295 blocked at step 3: tests/brand_literal_baseline.json still lists files T295 does not own: app/messaging/templates/e |
| T212 | intake: photo asks with on-server checks and preview, inspir | done | 2 | 19 min | 08:12Z | Ci: failure; Coverage: failure; Checks: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned |
| T337 | payments (harness): a $0 release proof is bound to the pick' | BLOCKED | 4 | 9 min | 08:05Z | PLAN GAP: app/server/tests/release_recovery.rs:572-576 (td_pick_50_a_zero_fee_releases_free_and_a_cancelled_pick_nothing |
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
| T349 | calls: a PIN draw fails only when the OS source fails, and o | done | 1 | 36 min | 00:40Z |  |
| T207 | businesses: re-apply the license map when a daily list chang | done | 1 | 41 min | 23:39Z |  |
| T311 | calls and server: a voice read-back confirm connects only th | done | 2 | 33 min | 23:21Z | Ci: failure; Coverage: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / test  |
| T338 | adapters (harness): the refused-connection test holds its po | done | 1 | 24 min | 20:45Z |  |
| T155 | businesses: sign-up capability setup pre-ticked from the lic | done | 1 | 57 min | 20:15Z |  |
| T311 | calls and server: a voice read-back confirm connects only th | check_failed | 1 | 21 min | 18:54Z |  |
| T294 | portals and site: every page takes the brand from the brand  | done | 1 | 42 min | 18:32Z |  |
| T231 | portals (harness): the homeowner check-in email buttons and  | done | 2 | 26 min | 17:08Z | T231 built and committed (b2f7b11e, 26074ad1) but smoke red: tests/security/tests/auth_matrix.rs (T088, protected, no se |
| T231 | portals (harness): the homeowner check-in email buttons and  | QUEUED | 1 | 28 min | 15:58Z |  |
| T199 | portals: no automated call to a business, voice codes only o | done | 1 | 64 min | 14:22Z |  |
| T293 | businesses and quotes: sign-up, consent, complaint and quote | done | 1 | 41 min | 12:35Z |  |
| T292 | messaging and calls: every text, email and call takes the br | done | 1 | 21 min | 11:16Z |  |
| T290 | worked examples: pin D-033 for one-cent-wide recurring range | done | 2 | 39 min | 10:21Z | T290 blocked: any new recurring worked-example row breaks tests::worked_examples (fixed RECURRING list of 13 ids, worked |

### Plan and harness merges on main
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
- 00:36Z Plan: fix tasks from the Nov 10 code review — payments, privacy, texting, portals, calls, jobs (D-2620–D-2642) (#439)
- 22:59Z Plan: claim the longest remaining dependency chain first (D-2591) (#436)
- 22:08Z Harness: report queued tasks that add to lists existing tests pin (D-2550) (#433)
- 21:53Z Harness: reuse a recorded green smoke on main's head; fix the start-up smoke timeouts (D-2560) (#434)
- 21:38Z Plan: claim the flaky-test fixes first — T338, then T311 and T349 (D-2540) (#431)
- 19:42Z Plan: second tiering pass — 13 more tasks on Sonnet, T159 on Haiku, reviewer counterexample duty (D-2506) (#427)
- 19:31Z Plan: PIN draws fail only when the OS source fails; one failed draw costs one draw (D-2451, T311) (#422)
- 19:17Z Plan: no Fable window on Thursday — the D-2225 window closes before it opens (D-2485) (#425)
- 18:21Z Plan: two lanes, and each queued task on the lowest model that can do it well (D-2462, D-2463) (#424)
- 16:53Z Plan: matrix doors before the serving tasks — public, owner and action-link doors, HTTP probe for served realm routes (D-2440, T348) (#421)
- 16:29Z Plan: T231 extends the authorization matrix to /c/{token}/{answer} as a harness task (D-2429) (#420)
- 13:07Z Plan: no handyman split-job check — each job judged by its own price (Casey's ruling; D-2407, D-2408, T347) (#416)

## Progress (requirements verified)

**Requirements verified:** 208 of 733 (28.4%) · partial 343 · unstarted 182
**Tasks merged (throughput only):** 199 of 371

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 15 | 14 | 6 | 35 |
| ai | 7 | 17 | 5 | 29 |
| businesses | 16 | 35 | 7 | 58 |
| calls | 11 | 13 | 1 | 25 |
| consent | 8 | 23 | 4 | 35 |
| fees | 10 | 41 | 1 | 52 |
| intake | 19 | 18 | 6 | 43 |
| job-cards | 13 | 9 | 2 | 24 |
| licensing | 24 | 24 | 0 | 48 |
| lifecycle | 18 | 10 | 0 | 28 |
| messaging | 18 | 24 | 3 | 45 |
| ops-recovery | 2 | 5 | 47 | 54 |
| privacy | 6 | 23 | 2 | 31 |
| quiet-hours | 4 | 5 | 0 | 9 |
| quotes-pick | 25 | 24 | 7 | 56 |
| recurring | 5 | 3 | 3 | 11 |
| security | 6 | 43 | 6 | 55 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 1 | 6 | 25 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-11T09:30:03Z (week)): five-hour: 28.6M used, 527k left, resets 2026-11-11T09:44:28Z; week: 131.7M used, 62.3M left, resets 2026-11-12T17:00:00Z; projected finish 2026-12-04T10:07:16Z; waiting until 2026-11-11T09:44:28Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 79 of 162 done
- stage 5: 0 of 58 done
- stage 6: 0 of 34 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 2: T337 (lease until 2026-11-11T12:25:53Z)

## Last usage reading
- {"task": "T327", "at": "2026-11-11T09:38:36Z", "output_tokens": 182491, "cache_read_input_tokens": 28088494, "cache_creation_input_tokens": 636491, "input_tokens": 581}
