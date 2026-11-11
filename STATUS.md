# Ask Hava build status

Updated 2026-11-11T15:05:44Z by runner 399858e112d9 (actions).

**Building: T269 in progress since 15:02Z; 204 of 372 done; week 68% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 204 of 372 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 3: T269 claimed 15:02Z, lease until 18:02Z
- **Week:** 68% used, 50.6M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 15:04Z, 1 min before this update
- **Keeper:** last ran 14:40Z, next wake 15:00Z

## Needs attention
- BLOCKED T214: plan gap reported: PLAN GAP: tests/integration/tests/page_pick.rs:131 and tests/security/tests/page_pick_privacy.rs:224 (both T246's; page_pick.rs also T343's) and tests/e2e/j1_homeowner.spec.ts:203 (T098's) press the final Pick without the share box, which PICK-049 now requires. Smallest plan change: give T214 those three test files for a fake-value change only (add ("share","yes") to each posted final Pick, and check the box before #final-pick in j1_homeowner), or move the server-side refusal to a harness task that owns them. Evidence: kit/tasks.json ownership (T214 owns none of the three); page_pick.rs:131 po; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T214 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T332: Ci: failure; Ally: failure; E2e: failure
Ally failed at step: Chromium and its system libraries
E: Failed to fetch https://security.ubuntu.com/ubuntu/pool/universe/u/unifont/fonts-unifont_16.0.04-1build1_all.deb  Temporary failure resolving 'security.ubuntu.com'
E: Failed to fetch https://security.ubuntu.com/ubuntu/pool/universe/f/fonts-wqy-zenhei/fonts-wqy-zenhei_0.9.45-8build1_all.deb  Temporary failure resolving 'security.ubuntu.com'
E: Failed to fetch https://security.ubuntu.com/ubuntu/pool/main/x/xfonts-encodings/xfonts-encodings_1.0.5-0ubuntu3_all.deb  Temporary failure resolving 'security.ubuntu.com'
E: Failed to fetch https://security.ubuntu.com/ubuntu/pool/main/x/xfonts-utils/xfonts-utils_7.7%2b7build1_amd64v3.deb  Temporary failure resolving 'security.ubuntu.com'
E: Failed to fetch https://security.ubuntu.com/ubuntu/pool/universe/x/xfonts-cyrillic/xfonts-cyrillic_1.0.5%2bnmu1build1_all.deb  Temporary failure resolving 'security.ubuntu.com'
E: Failed to fetch https://security.ubuntu.com/ubuntu/pool/universe/x/xfonts-scalable/xfonts-scalable_1.0.3-1.3build1_all.deb  Temporary failure resolving 'security.ubuntu.com'
E: Unable to fetch some archives, maybe run apt update or try with --fix-missing?
Failed to install browsers
Error: Installation process exited with code: 100
##[error]Process completed with exit code 1.
E2e failed at step: Chromium and its system libraries
Error: Installation process exited with code: 100
##[error]Process completed with exit code 1.
Ci fai
- Strict plan check red: 24 problem(s) on tasks still to run (+35 on DONE tasks); first: T340: stored context_pct 35% but it measures 39%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T384 | quotes and portals: the homeowner sees an edited quote as up | MERGE_PENDING | 2 | 18 min | 15:05Z | Ci: failure; E2e: failure; Ally: failure / E2e failed at step: Chromium and its system libraries / E: Failed to fetch ht |
| T214 | picks: the homeowner's share box at the pick and the 'Can't  | BLOCKED | 2 | 3 min | 15:04Z | PLAN GAP: tests/integration/tests/page_pick.rs:131 and tests/security/tests/page_pick_privacy.rs:224 (both T246's; page_ |
| T342 | quotes: a quote's contact check also reads the whole field w | MERGE_PENDING | 1 | 51 min | 14:31Z |  |
| T332 | payments (harness): a late success has its own refund trigge | check_failed | 3 | 48 min | 14:24Z | PLAN GAP: tests/integration/tests/confirm_and_pay.rs:393-413 (td_fee_25_an_ended_pick_is_never_cleared, TD-FEE-25, T189  |
| T269 | portals: a business notice's signed link opens sign-in for t | check_failed | 1 | 20 min | 14:19Z | Ci: failure; E2e: failure; Ally: failure / E2e failed at step: Chromium and its system libraries / E: Failed to fetch ht |
| T384 | quotes and portals: the homeowner sees an edited quote as up | check_failed | 1 | 40 min | 13:40Z |  |
| T214 | picks: the homeowner's share box at the pick and the 'Can't  | BLOCKED | 1 | 15 min | 13:19Z |  |
| T333 | payments (harness): a succeeded refund is recorded at the am | BLOCKED | 1 | 50 min | 13:14Z | PLAN GAP: app/payments/src/routes.rs:412 (T259) and app/payments/src/refund_state.rs:34 (T270) / app/picks/src/unreachab |
| T332 | payments (harness): a late success has its own refund trigge | BLOCKED | 2 | 2 min | 12:40Z |  |
| T332 | payments (harness): a late success has its own refund trigge | BLOCKED | 1 | 11 min | 12:10Z |  |
| T164 | quotes: one quote format per category, screened notes, and e | done | 3 | 19 min | 12:02Z | merging main into the reviewed head conflicts in app/quotes/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T331 | payments (harness): a payment success with no readable pick  | done | 1 | 49 min | 11:35Z |  |
| T164 | quotes: one quote format per category, screened notes, and e | resync | 3 | 12 min | 11:31Z |  |
| T164 | quotes: one quote format per category, screened notes, and e | check_failed | 2 | 39 min | 11:00Z |  |
| T278 | quotes, intake and picks: the check-in, feedback and unreach | done | 1 | 29 min | 10:56Z |  |
| T316 | businesses: one read of each business's public name for the  | BLOCKED | 1 | 2 min | 10:06Z | PLAN GAP: app/businesses/src/holding.rs:8 and migrations/0050_licensing_imports.sql:22-55: nothing links a business to a |
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
| T349 | calls: a PIN draw fails only when the OS source fails, and o | done | 1 | 36 min | 00:40Z |  |
| T207 | businesses: re-apply the license map when a daily list chang | done | 1 | 41 min | 23:39Z |  |
| T311 | calls and server: a voice read-back confirm connects only th | done | 2 | 33 min | 23:21Z | Ci: failure; Coverage: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / test  |
| T338 | adapters (harness): the refused-connection test holds its po | done | 1 | 24 min | 20:45Z |  |
| T155 | businesses: sign-up capability setup pre-ticked from the lic | done | 1 | 57 min | 20:15Z |  |
| T311 | calls and server: a voice read-back confirm connects only th | check_failed | 1 | 21 min | 18:54Z |  |
| T294 | portals and site: every page takes the brand from the brand  | done | 1 | 42 min | 18:32Z |  |
| T231 | portals (harness): the homeowner check-in email buttons and  | done | 2 | 26 min | 17:08Z | T231 built and committed (b2f7b11e, 26074ad1) but smoke red: tests/security/tests/auth_matrix.rs (T088, protected, no se |
| T231 | portals (harness): the homeowner check-in email buttons and  | QUEUED | 1 | 28 min | 15:58Z |  |

### Plan and harness merges on main
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

## Progress (requirements verified)

**Requirements verified:** 213 of 733 (29.1%) · partial 342 · unstarted 178
**Tasks merged (throughput only):** 204 of 372

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 15 | 14 | 6 | 35 |
| ai | 7 | 17 | 5 | 29 |
| businesses | 16 | 35 | 7 | 58 |
| calls | 11 | 13 | 1 | 25 |
| consent | 8 | 23 | 4 | 35 |
| fees | 13 | 38 | 1 | 52 |
| intake | 19 | 18 | 6 | 43 |
| job-cards | 13 | 9 | 2 | 24 |
| licensing | 25 | 23 | 0 | 48 |
| lifecycle | 18 | 10 | 0 | 28 |
| messaging | 18 | 24 | 3 | 45 |
| ops-recovery | 2 | 6 | 46 | 54 |
| privacy | 6 | 23 | 2 | 31 |
| quiet-hours | 4 | 5 | 0 | 9 |
| quotes-pick | 25 | 27 | 4 | 56 |
| recurring | 6 | 2 | 3 | 11 |
| security | 6 | 43 | 6 | 55 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 1 | 6 | 25 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-11T14:40:15Z (week)): five-hour: 7.4M used, 21.7M left, resets on first use; week: 143.4M used, 50.6M left, resets 2026-11-12T17:00:00Z; projected finish 2026-12-04T19:20:35Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 84 of 164 done
- stage 5: 0 of 58 done
- stage 6: 0 of 34 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 3: T269 (lease until 2026-11-11T18:02:25Z)

## Last usage reading
- {"task": "T384", "at": "2026-11-11T15:05:27Z", "cache_read_input_tokens": 2960348, "input_tokens": 119, "output_tokens": 23149, "cache_creation_input_tokens": 244078}
