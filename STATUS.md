# Ask Hava build status

Updated 2026-11-13T00:31:44Z by runner 90d555630097 (actions).

**Building: T215 in progress since 23:45Z; 262 of 404 done; week 9% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 262 of 404 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 1: T215 claimed 23:45Z, lease until 02:45Z; lane 3: T372 claimed 23:46Z, lease until 02:46Z; lane 6: T249 claimed 00:01Z, lease until 03:01Z
- **Week:** 9% used, 170.9M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 00:31Z, 0 min before this update
- **Keeper:** last ran 00:27Z, next wake 02:47Z

## Needs attention
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T371 (since 2026-11-12T23:46:03Z, no attempt used; a plan change fixes it): T371: dependency T385 is DONE but its owned file app/licensing/src/own_record.rs is not on main; T371: dependency T385 is DONE but its owned file app/businesses/src/signup_record.rs is not on main; T371: dependency T385 is DONE but its owned file tests/integration/tests/signup_own_record.rs is not on main; T371: dependency T385 is DONE but its owned file tests/fixtures/license_source/record/roc/ROC100001 is not on main; T371: dependency T385 is DONE but its owned file tests/fixtures/license_source/roc is not on main; T371: dependency T385 is DONE but its owned file tests/fixtures/license_source/city is not on main
- Strict plan check red: 21 problem(s) on tasks still to run (+138 on DONE tasks); first: T320: measures 46% of the window, over its 45% limit; trim its reads or split it

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T255 | intake, payments and server: ask again for an emergency addr | BLOCKED | 1 | 2 min | 23:48Z | PLAN GAP: Releasable in app/payments/src/release_proof.rs:153 has private fields and no constructor; T255's owned reclea |
| T351 | privacy and quotes: spaced gate codes, lower-case street add | done | 1 | 57 min | 23:46Z |  |
| T215 | businesses and ops: pest identity by the city list plus atte | resync | 1 | 73 min | 23:43Z | merging main into the reviewed head conflicts in app/businesses/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T418 | Performance: lab Core Web Vitals and Ci's warn-only performa | done | 1 | 50 min | 23:24Z |  |
| T347 | licensing (harness): remove the handyman split-job check; ea | MERGE_PENDING | 1 | 52 min | 23:24Z |  |
| T162 | intake: the one intake order for the five paths and the Span | done | 1 | 36 min | 23:01Z |  |
| T385 | adapters, licensing and businesses (harness): the sign-up re | done | 3 | 30 min | 22:57Z | merging main into the reviewed head conflicts in app/businesses/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T419 | calls (harness): TD-CALL-23's no-plaintext scan reads only t | done | 1 | 24 min | 22:37Z |  |
| T351 | privacy and quotes: spaced gate codes, lower-case street add | resync | 1 | 58 min | 22:35Z |  |
| T417 | Performance: seeded fake-data route benchmark and the first  | done | 1 | 36 min | 22:22Z |  |
| T385 | adapters, licensing and businesses (harness): the sign-up re | resync | 3 | 40 min | 22:16Z |  |
| T162 | intake: the one intake order for the five paths and the Span | resync | 1 | 22 min | 21:57Z |  |
| T414 | Stand-ins (harness): end-to-end specs can read the emails th | done | 3 | 28 min | 20:57Z | Ci: failure; Coverage: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / test  |
| T251 | intake and quotes: the ZIP question, the job location, and t | done | 2 | 82 min | 20:23Z | merging main into the reviewed head conflicts in app/server/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T416 | Performance: database pool and query measurements (PERF-004) | done | 1 | 54 min | 19:59Z |  |
| T414 | Stand-ins (harness): end-to-end specs can read the emails th | check_failed | 2 | 15 min | 19:11Z |  |
| T350 | privacy: an opt-out, HELP or START word is never taken or re | done | 1 | 103 min | 19:09Z |  |
| T362 | server: the built static files are served at /static/<crate> | done | 3 | 26 min | 19:03Z | PLAN GAP: tests/a11y/j1_keyboard.spec.ts:171-172 (owned by T098/T214, protected, not in T362's files) presses Tab before |
| T346 | portals: every homeowner Verify code is bound to its purpose | done | 1 | 40 min | 18:57Z |  |
| T251 | intake and quotes: the ZIP question, the job location, and t | resync | 2 | 87 min | 18:46Z |  |
| T376 | ai and stand-in (harness): the intake schema sent to Claude  | done | 1 | 35 min | 18:40Z |  |
| T385 | adapters, licensing and businesses (harness): the sign-up re | check_failed | 2 | 60 min | 18:26Z |  |
| T366 | jobs and server: every dead-lettered task alerts, and a dail | done | 1 | 26 min | 17:53Z |  |
| T414 | Stand-ins (harness): end-to-end specs can read the emails th | check_failed | 1 | 32 min | 17:52Z |  |
| T315 | jobs and portals: feedback words are blanked by the 3-year p | done | 1 | 66 min | 16:18Z |  |
| T370 | intake (harness): the night count the homeowner hears counts | done | 2 | 22 min | 15:30Z | Ci: failure; Checks: failure / Checks failed at step: Rust build (the one boundary build) and tests (all but SPLIT_TESTS |
| T389 | licensing: a homeowner's wrong-license report pauses a pest  | done | 1 | 31 min | 15:11Z |  |
| T362 | server: the built static files are served at /static/<crate> | BLOCKED | 2 | 21 min | 15:06Z |  |
| T385 | adapters, licensing and businesses (harness): the sign-up re | check_failed | 1 | 38 min | 14:42Z |  |
| T370 | intake (harness): the night count the homeowner hears counts | check_failed | 1 | 22 min | 14:29Z |  |
| T385 | adapters, licensing and businesses (harness): the sign-up re | resync | 1 | 60 min | 13:57Z |  |
| T362 | server: the built static files are served at /static/<crate> | check_failed | 1 | 38 min | 13:35Z |  |
| T369 | businesses: the sign-up after-hours answer ticks only while  | done | 1 | 50 min | 12:41Z |  |
| T378 | adapters, calls and stand-in (harness): every outbound call  | done | 1 | 45 min | 12:34Z |  |
| T323 | ai: a new capitalized name at the start of a sentence counts | done | 1 | 24 min | 11:47Z |  |
| T404 | harness: the door probe's fixtures hold a charged pick and t | done | 1 | 27 min | 11:46Z |  |
| T345 | harness: every external GitHub Actions `uses:` in every work | done | 3 | 27 min | 11:16Z | PLAN GAP: tests/test_runner_cache.py:207,:214 and tests/test_runner_merge_early.py:212,:213 hard-code actions/cache/rest |
| T377 | adapters and stand-in (harness): every text asks Twilio for  | done | 1 | 28 min | 11:15Z |  |
| T280 | server and messaging (harness): the business sign-in pages a | BLOCKED | 1 | 8 min | 11:14Z | PLAN GAP: app/portals/src/business_auth.rs:130 (SignInMail::addresses) and :176 (CodeAddress::code_to) have nothing to l |
| T268 | portals, businesses and messaging: the sign-up consent box,  | done | 1 | 24 min | 11:05Z |  |
| T411 | quotes: the scrubbed job description and the shown and appro | done | 1 | 28 min | 10:36Z |  |
| T343 | harness: the DONE pick tests' fixtures record each job's fac | done | 1 | 24 min | 10:23Z |  |
| T411 | quotes: the scrubbed job description and the shown and appro | resync | 1 | 30 min | 10:00Z |  |
| T268 | portals, businesses and messaging: the sign-up consent box,  | resync | 1 | 21 min | 09:38Z |  |
| T358 | messaging: the Canadian area codes for the HELP roaming line | done | 1 | 40 min | 09:32Z |  |
| T268 | portals, businesses and messaging: the sign-up consent box,  | resync | 1 | 58 min | 09:12Z |  |
| T406 | quotes: a quote's PICK-040 facts are stored with it and read | done | 1 | 41 min | 09:05Z |  |
| T336 | server, adapters and stand-in (harness): a text stopped by a | done | 2 | 56 min | 09:00Z | merging main into the reviewed head conflicts in app/server/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T345 | harness: every external GitHub Actions `uses:` in every work | BLOCKED | 2 | 4 min | 08:47Z |  |
| T322 | businesses: a backup notice address is checked again at conf | done | 1 | 35 min | 08:36Z |  |
| T251 | intake and quotes: the ZIP question, the job location, and t | BLOCKED | 1 | 3 min | 08:17Z |  |
| T360 | messaging: the opt-out confirmation email while texting is o | done | 1 | 43 min | 07:42Z |  |
| T336 | server, adapters and stand-in (harness): a text stopped by a | resync | 2 | 47 min | 07:35Z |  |
| T390 | calls and server: a blocked staff call that connects a line  | done | 1 | 29 min | 07:34Z |  |
| T401 | server (harness): the door probe's fixture gains a released  | done | 2 | 37 min | 07:33Z | PLAN GAP: tests/security/tests/no_homeowner_charge.rs:17-27,123 (TD-FEE-07, not T401's) fails any app/ file naming a hom |
| T339 | jobs: each job keeps its category code and remaining candida | done | 1 | 18 min | 07:13Z |  |
| T336 | server, adapters and stand-in (harness): a text stopped by a | resync | 2 | 71 min | 06:06Z |  |
| T405 | payments and adapters (harness): each charged pick's 3-D Sec | done | 1 | 51 min | 06:05Z |  |
| T356 | messaging and server (harness): the worker sends queued text | done | 1 | 86 min | 05:31Z |  |
| T405 | payments and adapters (harness): each charged pick's 3-D Sec | resync | 1 | 33 min | 04:56Z |  |
| T326 | calls (harness): the LINK notice names the line's stored PIN | done | 1 | 25 min | 03:37Z |  |
| T336 | server, adapters and stand-in (harness): a text stopped by a | QUEUED | 1 | 5 min | 03:27Z |  |
| T248 | catalog and businesses: markets as reviewed data, Arizona ZI | done | 1 | 12 min | 03:22Z |  |
| T401 | server (harness): the door probe's fixture gains a released  | BLOCKED | 1 | 19 min | 03:13Z |  |
| T400 | server: the web process stores the catalog file at start-up, | done | 1 | 21 min | 03:07Z |  |
| T248 | catalog and businesses: markets as reviewed data, Arizona ZI | resync | 1 | 67 min | 02:56Z |  |
| T334 | payments: the nightly reconcile catches up after an outage f | done | 2 | 41 min | 02:25Z | merging main into the reviewed head conflicts in app/payments/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T313 | messaging and server: an EMERGENCY card notice goes at night | done | 1 | 19 min | 02:07Z |  |
| T368 | businesses (harness): the capability checklist offers every  | done | 1 | 17 min | 02:06Z |  |
| T325 | server: an inbound text stopped by a module error raises a b | done | 1 | 30 min | 01:42Z |  |
| T368 | businesses (harness): the capability checklist offers every  | resync | 1 | 25 min | 01:13Z |  |

### Plan and harness merges on main
- 00:30Z Plan: rename Ask Hava to Laftly (Laftly LLC, laftly.com), D-3347 (#631)
- 00:14Z Plan: T255 plan gap: re-clear builds Releasable from the stored success (D-3358) (#638)
- 22:08Z Plan: T340, T271, T126 under their context limits (D-3325) (#623)
- 22:01Z plan: T419 fixes the flaky TD-CALL-23 no-plaintext scan, claimed first (D-3324) (#622)
- 20:49Z Harness: worker output discipline and per-role usage recording (D-3305) (#618)
- 20:07Z Harness: up to 2 tasks per crate; one only where the crate's CLAUDE.md is full (D-3303) (#615)
- 18:44Z Plan: performance track — measurement harness T416–T418, goals as non-blocking (D-3292) (#607)
- 18:11Z Plan: pre-flight fixes round 11 — T346 context gap, T162 template key, T372 cite, T255/T347 budgets (D-3286) (#605)
- 18:06Z Plan: T362 takes the keyboard test's load wait (D-3270) (#601)
- 17:59Z Harness: seven lanes without merge collisions — crate exclusion, add-only conflict resolution, lanes 5→7 (D-3269) (#603)
- 16:10Z Plan: pre-flight fixes round 10 — T396, T398, T111, T113, T408, T222; new T414 (D-3263) (#597)
- 15:45Z Plan: new T412/T415 store business sign-in addresses and phones in the vault for T280, T397, T302 and T364 (D-3262) (#596)
- 14:48Z Plan: trim T251 to its context limit (D-3256) (#590)
- 09:36Z Plan: pre-flight fixes round 9 — stage 5: T303, T151, T321, T110, T210, T109, T116; new T409, T410 (D-3155) (#559)
- 09:22Z Plan: T345 owns the runner tests that hard-code action tags (D-3200) (#572)
- 09:13Z Plan: store the scrubbed job description and shown card so T251 can send approved cards (D-3189) (#568)
- 08:32Z Harness: smoke ancestry reuse accepts squash commits whose identical tree passed Ci, Twin and Ally (D-3156) (#560)
- 07:12Z Harness: close merged tasks before the smoke so their lanes free ~20 min sooner (D-3141) (#552)
- 06:05Z Plan: pre-flight fixes round 8 — stage 5: T101, T102, T105, T202, T320, T106, T364; new T408 (D-3144) (#551)
- 05:28Z Harness: stop backup-runner caches evicting CI's Rust caches (D-3143) (#553)
- 05:16Z Harness: run lint and the slow compile-fail/prompt-injection tests in a parallel CI job (D-3142) (#550)
- 04:59Z Plan: pre-flight fixes round 7 — T352, T335, T353, T330, T393, T359, T401 plan gap; new T406, T407 (D-3134) (#548)
- 03:50Z Plan: pre-flight fixes round 6 — T355, T340, T341, T249, T273, T271, T277, T357, T314; new T405 (D-3118) (#542)
- 03:24Z Harness: claim order prefers tasks in crates no other lane is writing (D-3082) (#523)
- 02:11Z Plan: pre-flight fixes round 5 — T378, T343, T268, T329, T306; new T404 (D-3112) (#538)
- 02:03Z Plan: pre-flight fixes round 4 — T356, T362, T365, T367, T300, T369; new T401–T403 (D-3106) (#536)
- 01:55Z Plan: owner approval before a team-linked phone works (BIZ-033, IC-95) — T396–T399 (D-3081) (#531)
- 00:38Z Plan: T261 pre-flight fix and catalog loaded at start-up (T400, IC-96) (D-3100) (#534)

## Progress (requirements verified)

**Requirements verified:** 232 of 733 (31.7%) · partial 337 · unstarted 164
**Tasks merged (throughput only):** 262 of 404

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 15 | 14 | 6 | 35 |
| ai | 7 | 17 | 5 | 29 |
| businesses | 16 | 39 | 3 | 58 |
| calls | 10 | 14 | 1 | 25 |
| consent | 11 | 20 | 4 | 35 |
| fees | 13 | 38 | 1 | 52 |
| intake | 22 | 18 | 3 | 43 |
| job-cards | 12 | 11 | 1 | 24 |
| licensing | 25 | 23 | 0 | 48 |
| lifecycle | 19 | 9 | 0 | 28 |
| messaging | 20 | 23 | 2 | 45 |
| ops-recovery | 2 | 7 | 45 | 54 |
| privacy | 6 | 23 | 2 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 31 | 24 | 1 | 56 |
| recurring | 6 | 2 | 3 | 11 |
| security | 8 | 42 | 5 | 55 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 2 | 5 | 25 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-13T00:00:45Z (week)): week: 23.1M used, 170.9M left, resets 2026-11-19T17:00:00Z; projected finish 2026-12-05T13:31:30Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 142 of 186 done
- stage 5: 0 of 64 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T215 (lease until 2026-11-13T02:45:19Z)
- lane 3: T372 (lease until 2026-11-13T02:46:03Z)
- lane 6: T249 (lease until 2026-11-13T03:01:54Z)

## Last usage reading
- {"task": "T255", "at": "2026-11-12T23:48:10Z", "output_tokens": 18261, "cache_read_input_tokens": 2887666, "input_tokens": 101, "cache_creation_input_tokens": 272582}
