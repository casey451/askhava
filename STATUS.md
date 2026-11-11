# Ask Hava build status

Updated 2026-11-11T05:36:47Z by runner d4ae3ec32f11 (actions).

**Building: T383 in progress since 05:18Z; 195 of 370 done; week 60% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 195 of 370 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 2: T383 claimed 05:18Z, lease until 08:18Z; lane 3: T327 claimed 05:24Z, lease until 08:24Z
- **Week:** 60% used, 71.2M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 05:26Z, 10 min before this update
- **Keeper:** last ran 05:22Z, next wake 08:20Z

## Needs attention
- nothing

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T337 | payments (harness): a $0 release proof is bound to the pick' | BLOCKED | 3 | 16 min | 05:11Z | T337 built and committed (53e675b1, cf42d54e); smoke/prehandin red on tests T337 does not own: confirm_and_pay.rs td_fee |
| T209 | picks and payments: the optional auto-confirm-and-pay box wi | MERGE_PENDING | 1 | 115 min | 05:06Z |  |
| T295 | payments, ai, intake and picks take the brand from the brand | BLOCKED | 3 | 0 min | 04:59Z | T295 blocked at step 3: tests/brand_literal_baseline.json still lists files T295 does not own: app/messaging/templates/e |
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
| T290 | worked examples: pin D-033 for one-cent-wide recurring range | QUEUED | 1 | 1 min | 09:07Z |  |
| T289 | jobs tests: the refused-move test counts its one bug log the | done | 1 | 44 min | 07:52Z |  |
| T288 | payments: a Stripe charge the nightly reconcile cannot settl | done | 1 | 60 min | 06:30Z |  |

### Plan and harness merges on main
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
- 09:36Z Plan: T290 may update the two tests that pin recurring rows by data; nearest reachable bases (D-2386) (#413)

## Progress (requirements verified)

**Requirements verified:** 204 of 733 (27.8%) · partial 340 · unstarted 189
**Tasks merged (throughput only):** 195 of 370

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 15 | 13 | 7 | 35 |
| ai | 7 | 17 | 5 | 29 |
| businesses | 16 | 35 | 7 | 58 |
| calls | 11 | 13 | 1 | 25 |
| consent | 8 | 23 | 4 | 35 |
| fees | 10 | 41 | 1 | 52 |
| intake | 19 | 15 | 9 | 43 |
| job-cards | 10 | 12 | 2 | 24 |
| licensing | 24 | 24 | 0 | 48 |
| lifecycle | 17 | 11 | 0 | 28 |
| messaging | 18 | 24 | 3 | 45 |
| ops-recovery | 2 | 5 | 47 | 54 |
| privacy | 6 | 22 | 3 | 31 |
| quiet-hours | 4 | 5 | 0 | 9 |
| quotes-pick | 25 | 22 | 9 | 56 |
| recurring | 5 | 3 | 3 | 11 |
| security | 6 | 43 | 6 | 55 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 1 | 6 | 25 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-11T05:22:42Z (week)): five-hour: 14.2M used, 14.9M left, resets 2026-11-11T09:44:28Z; week: 122.8M used, 71.2M left, resets 2026-11-12T17:00:00Z; projected finish 2026-12-04T07:37:31Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 75 of 162 done
- stage 5: 0 of 58 done
- stage 6: 0 of 34 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 2: T383 (lease until 2026-11-11T08:18:35Z)
- lane 3: T327 (lease until 2026-11-11T08:24:57Z)

## Last usage reading
- {"task": "T337", "at": "2026-11-11T05:11:51Z", "cache_creation_input_tokens": 209039, "input_tokens": 155, "cache_read_input_tokens": 5585684, "output_tokens": 41795}
