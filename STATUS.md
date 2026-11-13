# Ask Hava build status

Updated 2026-11-13T14:53:22Z by runner 128fb2f384de (actions).

**Building: T330 in progress since 14:41Z; 282 of 420 done; week 19% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 282 of 420 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 2: T330 claimed 14:41Z, lease until 17:41Z; lane 5: T266 claimed 14:42Z, lease until 17:42Z
- **Week:** 19% used, 152.4M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 14:53Z, 0 min before this update
- **Keeper:** last ran 14:49Z, next wake 17:44Z

## Needs attention
- PLAN GAP T319 (since 2026-11-13T13:50:53Z, no attempt used; a plan change fixes it): T319: tests/budget.rs is named by D-3445, but no task owns it and it does not exist
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T436 (since 2026-11-13T13:50:53Z, no attempt used; a plan change fixes it): T436: tests/budget.rs is named by D-3445, but no task owns it and it does not exist
- Strict plan check red: 26 problem(s) on tasks still to run (+155 on DONE tasks); first: T399: stored context_pct 15% but it measures 19%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T266 | portals, picks and privacy: the homeowner's contact choice a | QUEUED | 1 | 63 min | 14:38Z | session ended without handing in |
| T330 | picks, intake and server: a withdrawn pick tells its busines | resync | 1 | 39 min | 14:30Z | merging main into the reviewed head conflicts in app/messaging/CLAUDE.md, app/server/src/lib.rs; a resync merges it in o |
| T367 | jobs, messaging, privacy, intake and payments: the daily swe | done | 1 | 60 min | 14:23Z |  |
| T386 | messaging (harness): the pick notice's "Can't take it" line  | BLOCKED | 1 | 16 min | 13:41Z | PLAN GAP: tests/integration/tests/voice_calls.rs:229-251 (td_call_06_no_route_lets_a_business_text_a_homeowner) keeps a  |
| T255 | intake, privacy, payments and server: ask again for an emerg | done | 4 | 71 min | 12:52Z | PLAN GAP: app/picks/src/release.rs:337 (owned by T069, T077, T253, T254, T278, T281, T318, T355) - an emergency address  |
| T312 | intake: a street address written in lower case never trips t | done | 2 | 51 min | 12:32Z | session ended without handing in |
| T428 | Switch the brand file to Laftly and re-issue the legal recor | done | 3 | 46 min | 11:14Z | session ended without handing in |
| T312 | intake: a street address written in lower case never trips t | QUEUED | 1 | 24 min | 10:17Z |  |
| T428 | Switch the brand file to Laftly and re-issue the legal recor | QUEUED | 2 | 22 min | 10:15Z |  |
| T255 | intake, privacy, payments and server: ask again for an emerg | BLOCKED | 3 | 26 min | 09:48Z |  |
| T431 | Brand rename prep: the legal-docs version case and five paym | done | 1 | 30 min | 09:32Z |  |
| T249 | quotes and portals: radius matching by ZIP centroid, the cov | done | 1 | 71 min | 09:03Z |  |
| T255 | intake, privacy, payments and server: ask again for an emerg | BLOCKED | 2 | 3 min | 07:47Z |  |
| T428 | Switch the brand file to Laftly and re-issue the legal recor | BLOCKED | 1 | 40 min | 07:40Z |  |
| T420 | Brand rename prep: intake and ai unit tests take the brand f | done | 1 | 28 min | 06:38Z |  |
| T215 | businesses and ops: pest identity by the city list plus atte | done | 1 | 22 min | 05:08Z |  |
| T215 | businesses and ops: pest identity by the city list plus atte | resync | 1 | 20 min | 04:22Z |  |
| T371 | licensing and server: the worker fetches and records the dai | done | 1 | 70 min | 04:18Z |  |
| T427 | WORD-022 phrases catch both brand names in the spec trace, t | done | 1 | 29 min | 03:17Z |  |
| T429 | Copy, voice and AI-smell docs name Laftly | done | 1 | 22 min | 03:13Z |  |
| T430 | payments: a Confirm needs the current card-authorization wor | done | 1 | 39 min | 03:13Z |  |
| T426 | Brand rename prep: notice-address and sign-in security tests | done | 1 | 35 min | 02:49Z |  |
| T425 | Brand rename prep: the last integration tests and four secur | done | 1 | 28 min | 02:39Z |  |
| T423 | Brand rename prep: integration tests (first-quote move to si | done | 1 | 66 min | 01:47Z |  |
| T422 | Brand rename prep: integration tests (accepted-quote email t | done | 1 | 44 min | 01:20Z |  |
| T421 | Brand rename prep: messaging, quotes, picks and portals unit | done | 1 | 26 min | 01:18Z |  |
| T424 | Brand rename prep: integration tests (open-cards digest to s | done | 1 | 32 min | 01:15Z |  |
| T372 | businesses and licensing: each recorded ROC list re-applies  | done | 1 | 81 min | 01:07Z |  |
| T255 | intake, privacy, payments and server: ask again for an emerg | BLOCKED | 1 | 2 min | 23:48Z |  |
| T351 | privacy and quotes: spaced gate codes, lower-case street add | done | 1 | 57 min | 23:46Z |  |
| T215 | businesses and ops: pest identity by the city list plus atte | resync | 1 | 73 min | 23:43Z |  |
| T418 | Performance: lab Core Web Vitals and Ci's warn-only performa | done | 1 | 50 min | 23:24Z |  |
| T347 | licensing (harness): remove the handyman split-job check; ea | done | 1 | 52 min | 23:24Z |  |
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

### Plan and harness merges on main
- 14:47Z Plan: fix T386 and D-3445 plan gaps (D-3467) (#673)
- 13:45Z Plan: shorten the critical path, drop legacy dependency edges (D-3446) (#671)
- 12:32Z Plan: pre-flight 14, split three oversized build tasks; T111/T113 brand note (D-3445) (#668)
- 11:32Z Harness: fix-round hand-off, tighter output caps, crate_cap 2 (D-3444) (#665)
- 11:16Z Harness: an API error session death does not use an attempt (D-3443) (#664)
- 10:13Z Plan: give T255 back the attempt an API 403 outage took (D-3442) (#663)
- 09:02Z Plan: T255 plan gap 2, the address is written onto the homeowner's own contact (D-3431) (#660)
- 08:55Z Plan: T428 plan gap, new prep task owns the six pinned tests (D-3430) (#659)
- 05:17Z Harness: merge line updates only the oldest pending branch (D-3418) (#653)
- 04:59Z Harness: release a dead worker's lease early (D-3417) (#651)
- 04:27Z Harness: safety hand-in reports unreleased leases (D-3419) (#654)
- 01:57Z Harness: crate_cap back to 1, D-3303 resync guardrail tripped (D-3386) (#645)
- 01:29Z Harness: fail CI, smoke and prehandin on leftover merge-conflict markers (D-3307) (#619)
- 01:16Z Plan: T255 follow-ups (D-3384) and pre-flight 12, T426 owns the legal-docs test (D-3385) (#640)
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

## Progress (requirements verified)

**Requirements verified:** 233 of 733 (31.8%) · partial 340 · unstarted 160
**Tasks merged (throughput only):** 282 of 420

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 15 | 14 | 6 | 35 |
| ai | 8 | 16 | 5 | 29 |
| businesses | 17 | 38 | 3 | 58 |
| calls | 10 | 14 | 1 | 25 |
| consent | 11 | 20 | 4 | 35 |
| fees | 13 | 38 | 1 | 52 |
| intake | 23 | 17 | 3 | 43 |
| job-cards | 12 | 12 | 0 | 24 |
| licensing | 26 | 22 | 0 | 48 |
| lifecycle | 19 | 9 | 0 | 28 |
| messaging | 20 | 23 | 2 | 45 |
| ops-recovery | 2 | 8 | 44 | 54 |
| privacy | 6 | 23 | 2 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 29 | 26 | 1 | 56 |
| recurring | 6 | 2 | 3 | 11 |
| security | 8 | 42 | 5 | 55 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 1 | 8 | 23 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-13T14:38:38Z (week)): week: 41.6M used, 152.4M left, resets 2026-11-19T17:00:00Z; projected finish 2026-12-06T19:07:19Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 162 of 201 done
- stage 5: 0 of 65 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 2: T330 (lease until 2026-11-13T17:41:21Z)
- lane 5: T266 (lease until 2026-11-13T17:42:55Z)

## Last usage reading
- {"task": "T330", "at": "2026-11-13T14:30:29Z", "cache_creation_input_tokens": 821176, "cache_read_input_tokens": 43723133, "input_tokens": 522, "output_tokens": 171690}
