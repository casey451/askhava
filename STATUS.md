# Ask Hava build status

Updated 2026-11-11T02:46:17Z by runner daef586a5cd2 (actions).

**Building: T374 in progress since 02:22Z; 193 of 362 done; week 57% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 193 of 362 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 2: T374 claimed 02:22Z, lease until 05:22Z
- **Week:** 57% used, 78.3M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 02:46Z, 0 min before this update
- **Keeper:** last ran 02:42Z, next wake 05:25Z

## Needs attention
- nothing

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T157 | picks: the homeowner's backup choice at the pick | MERGE_PENDING | 2 | 22 min | 02:41Z | E2e: failure; Ally: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / {"timestamp":"2026-11-1 |
| T157 | picks: the homeowner's backup choice at the pick | check_failed | 1 | 66 min | 02:10Z |  |
| T337 | payments: a $0 release proof is bound to the pick's own stor | QUEUED | 1 | 31 min | 02:07Z | T337 code+tests done (commit 6b896e8f) but smoke/prehandin red: tests/integration/tests/confirm_and_pay.rs td_fee_25_a_p |
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
| T286 | server: every request has a whole-request time limit, answer | done | 1 | 26 min | 04:57Z |  |
| T285 | runner: a task closes with a visible mark when a file it own | done | 1 | 19 min | 03:45Z |  |
| T284 | server: every route the architecture lists is wired in the r | done | 3 | 34 min | 03:02Z | Ci: failure; Checks: failure / Checks failed at step: Rust build (the one boundary build) and tests / test tests::ssrf_g |

### Plan and harness merges on main
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
- 03:30Z Plan: pentest assessment — Actions pinned, purpose-bound codes, Verify sends outside transactions (D-2346–D-2350, T345–T346) (#407)

## Progress (requirements verified)

**Requirements verified:** 206 of 733 (28.1%) · partial 336 · unstarted 191
**Tasks merged (throughput only):** 193 of 362

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 15 | 13 | 7 | 35 |
| ai | 8 | 16 | 5 | 29 |
| businesses | 16 | 35 | 7 | 58 |
| calls | 11 | 13 | 1 | 25 |
| consent | 8 | 23 | 4 | 35 |
| fees | 10 | 41 | 1 | 52 |
| intake | 19 | 15 | 9 | 43 |
| job-cards | 10 | 12 | 2 | 24 |
| licensing | 24 | 23 | 1 | 48 |
| lifecycle | 17 | 11 | 0 | 28 |
| messaging | 19 | 23 | 3 | 45 |
| ops-recovery | 2 | 5 | 47 | 54 |
| privacy | 6 | 22 | 3 | 31 |
| quiet-hours | 4 | 5 | 0 | 9 |
| quotes-pick | 25 | 21 | 10 | 56 |
| recurring | 5 | 3 | 3 | 11 |
| security | 6 | 43 | 6 | 55 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 1 | 6 | 25 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-11T02:10:52Z (week)): five-hour: 12.0M used, 17.1M left, resets 2026-11-11T04:21:24Z; week: 115.7M used, 78.3M left, resets 2026-11-12T17:00:00Z; projected finish 2026-12-03T06:13:27Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 73 of 158 done
- stage 5: 0 of 58 done
- stage 6: 0 of 30 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 2: T374 (lease until 2026-11-11T05:22:52Z)

## Last usage reading
- {"task": "T157", "at": "2026-11-11T02:41:38Z", "output_tokens": 43467, "cache_creation_input_tokens": 318212, "cache_read_input_tokens": 4971390, "input_tokens": 162}
