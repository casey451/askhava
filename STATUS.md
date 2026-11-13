# Ask Hava build status

Updated 2026-11-13T08:58:05Z by runner ca8b6b621c5e (manual).

**Building: T249 in progress since 07:51Z; 276 of 416 done; week 14% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 276 of 416 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 1: T249 claimed 07:51Z, lease until 10:51Z
- **Week:** 14% used, 168.0M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 08:57Z, 0 min before this update
- **Keeper:** last ran 08:57Z, next wake 10:54Z

## Needs attention
- BLOCKED T255: plan gap reported: PLAN GAP: app/picks/src/release.rs:337 (owned by T069, T077, T253, T254, T278, T281, T318, T355) - an emergency address cannot reach the release with T255's files. picks::release reads the address only from the proof's contact (release.rs:347, privacy/src/api.rs:257); the sink mints the proof for the phone contact (release_sink.rs:101-133); take_name_address stores the address as a separate phone-less vault contact (conversation.rs:408-421); privacy has no public setter for a stored contact (vault.rs, only private reseal). Minting for the address-only contact would give the masked line a homeo; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T255 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- Strict plan check red: 26 problem(s) on tasks still to run (+149 on DONE tasks); first: T399: stored context_pct 15% but it measures 19%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T255 | intake, payments and server: ask again for an emergency addr | BLOCKED | 2 | 3 min | 07:47Z | PLAN GAP: app/picks/src/release.rs:337 (owned by T069, T077, T253, T254, T278, T281, T318, T355) - an emergency address  |
| T428 | Switch the brand file to Laftly and re-issue the legal recor | BLOCKED | 1 | 40 min | 07:40Z | PLAN GAP: T428 does not own tests/test_check_legal_docs.py:75, tests/integration/tests/fee_at_pick.rs:25, fee_mode_switc |
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
| T255 | intake, payments and server: ask again for an emergency addr | BLOCKED | 1 | 2 min | 23:48Z |  |
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

### Plan and harness merges on main
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
- 14:48Z Plan: trim T251 to its context limit (D-3256) (#590)
- 09:36Z Plan: pre-flight fixes round 9 — stage 5: T303, T151, T321, T110, T210, T109, T116; new T409, T410 (D-3155) (#559)
- 09:22Z Plan: T345 owns the runner tests that hard-code action tags (D-3200) (#572)
- 09:13Z Plan: store the scrubbed job description and shown card so T251 can send approved cards (D-3189) (#568)

## Progress (requirements verified)

**Requirements verified:** 234 of 733 (31.9%) · partial 338 · unstarted 161
**Tasks merged (throughput only):** 276 of 416

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 15 | 14 | 6 | 35 |
| ai | 8 | 16 | 5 | 29 |
| businesses | 17 | 38 | 3 | 58 |
| calls | 10 | 14 | 1 | 25 |
| consent | 11 | 20 | 4 | 35 |
| fees | 13 | 38 | 1 | 52 |
| intake | 24 | 16 | 3 | 43 |
| job-cards | 12 | 11 | 1 | 24 |
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
- Pacing (session_reading 2026-11-13T07:47:59Z (week)): week: 26.0M used, 168.0M left, resets 2026-11-19T17:00:00Z; projected finish 2026-12-06T07:45:46Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 156 of 197 done
- stage 5: 0 of 64 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T249 (lease until 2026-11-13T10:51:53Z)

## Last usage reading
- {"task": "T255", "at": "2026-11-13T07:47:38Z", "input_tokens": 128, "cache_creation_input_tokens": 387353, "cache_read_input_tokens": 5540011, "output_tokens": 33222}
