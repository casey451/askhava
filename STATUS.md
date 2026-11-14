# Ask Hava build status

Updated 2026-11-14T14:34:27Z by runner eeddc510e237 (actions).

**Building: T302 in progress since 14:01Z; 326 of 451 done; week 44% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 326 of 451 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 4: T302 claimed 14:01Z, lease until 17:01Z
- **Week:** 44% used, 104.4M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 14:33Z, 0 min before this update
- **Keeper:** last ran 14:33Z, next wake 17:03Z

## Needs attention
- BLOCKED T261: plan gap reported: PLAN GAP: app/picks/src/cant_take.rs:64-71 hard-codes end reason "cant_take" (models::end_pick is pub(crate), models.rs:95); migrations/0158_picks_share_box.sql:28-29 CHECK allows only replaced/paused/ineligible/cant_take. T261 goal (1) and D-1871 (3) need the pick to end through the Can't take it path with its own reason, but T261 owns neither app/picks nor migrations, and no other task in kit/tasks.json builds a new reason (only T287 and T261 mention it). Reviewer confirmed with file:line evidence. Secondary: the task needs about 11 files and over 600 lines against a 7-file, 600-line budget.; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T261 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- Strict plan check red: 31 problem(s) on tasks still to run (+105 on DONE tasks); first: T397: stored context_pct 20% but it measures 13%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T261 | portals and server (harness): the served Confirm page under  | BLOCKED | 1 | 3 min | 14:30Z | PLAN GAP: app/picks/src/cant_take.rs:64-71 hard-codes end reason "cant_take" (models::end_pick is pub(crate), models.rs: |
| T353 | payments: a won dispute records the countering fee's return  | done | 1 | 30 min | 14:15Z |  |
| T432 | intake and server: ask the name-and-address question in prod | done | 1 | 71 min | 13:54Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | MERGE_PENDING | 4 | 38 min | 13:32Z | merging main into the reviewed head conflicts in app/calls/CLAUDE.md, app/quotes/CLAUDE.md; a resync merges it in on thi |
| T466 | portals: one shared form stylesheet for native checkboxes an | done | 1 | 27 min | 13:27Z |  |
| T301 | server and portals: the email action links are served (every | BLOCKED | 2 | 8 min | 12:40Z | PLAN GAP: app/server/tests/door_matrix.rs:1078 and :1127 (T348's harness file, not owned by T301). judge_opened requires |
| T359 | payments: a successful charge writes its own charge ledger e | done | 3 | 72 min | 12:37Z | Ci: failure; Coverage: failure / Coverage failed at step: Compare with tests/coverage_floor.json and refuse a lowered fl |
| T300 | server and portals: the business portal is served behind the | BLOCKED | 2 | 16 min | 12:20Z | PLAN GAP: app/portals/templates/business_home.html:27 and app/portals/templates/notice_settings.html:20 (T085/T199 and T |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | update_conflict | 3 | 23 min | 11:56Z |  |
| T302 | server (harness): business sign-up's open doors and served p | check_failed | 5 | 45 min | 11:56Z | PLAN GAP: T302 cannot be built as scoped. (1) No route constant exists for the email-check and sample-pick screens (app/ |
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
| T344 | picks: the global daily Verify cap holds under concurrent co | done | 1 | 28 min | 03:23Z |  |
| T433 | server and payments: every payments task runs in the product | done | 1 | 57 min | 03:21Z |  |
| T449 | server and intake: emergency first texts count toward the re | resync | 1 | 31 min | 03:17Z |  |
| T450 | portals: the sign-up code screens and the texts choice with  | done | 1 | 79 min | 03:00Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | resync | 1 | 23 min | 02:41Z |  |
| T449 | server and intake: emergency first texts count toward the re | resync | 1 | 47 min | 02:29Z |  |
| T341 | picks (harness): a pick of a tiers quote names its tier, and | done | 2 | 21 min | 02:25Z | merging main into the reviewed head conflicts in docs/for-casey-review.md; a resync merges it in on this attempt (D-891) |
| T280 | server and messaging (harness): the business sign-in pages a | resync | 3 | 107 min | 02:23Z |  |
| T302 | server (harness): business sign-up's open doors and served p | BLOCKED | 2 | 1 min | 01:33Z |  |
| T447 | messaging: a passing vault error holds an outbox row instead | done | 1 | 26 min | 01:30Z |  |
| T341 | picks (harness): a pick of a tiers quote names its tier, and | resync | 2 | 67 min | 01:10Z |  |
| T328 | picks and server: an ended pick offers the next quote (`paym | done | 2 | 61 min | 01:04Z | PLAN GAP: app/intake/src/job_texts.rs is outside T328's scope (owned by T240, T247, T278, T402). The PICK-046 text canno |
| T302 | server (harness): business sign-up's open doors and served p | BLOCKED | 1 | 2 min | 00:54Z |  |
| T448 | portals: the data-download mask joins digit runs across any  | done | 1 | 38 min | 00:49Z |  |
| T444 | intake (test): the job's ZIP is read only from a property in | done | 1 | 24 min | 00:31Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | resync | 1 | 25 min | 00:23Z |  |
| T391 | calls, businesses, messaging and server: the main phone's "L | done | 1 | 47 min | 23:23Z |  |
| T415 | businesses, messaging and server: the production phone link  | done | 1 | 69 min | 23:15Z |  |
| T355 | picks: the release claim re-checks the pick under its row lo | done | 1 | 59 min | 23:11Z |  |
| T341 | picks (harness): a pick of a tiers quote names its tier, and | BLOCKED | 1 | 3 min | 23:11Z |  |
| T328 | picks and server: an ended pick offers the next quote (`paym | BLOCKED | 1 | 4 min | 23:00Z |  |
| T340 | picks and payments: the fee at the pick uses the job's lates | done | 1 | 40 min | 22:32Z |  |
| T357 | server, messaging and intake: START into an open request res | done | 2 | 21 min | 22:14Z | Ci: failure; Lint and slow tests: failure / Lint and slow tests failed at step: Rust format and lint (clippy once, befor |
| T438 | Token usage counts each reply once: dedupe transcript usage  | done | 2 | 19 min | 22:10Z | PLAN GAP: tests/test_failure_drills.py:1046 (test_usage_feeds_the_pacing_windows; owned by no task, T438 may not change  |
| T280 | server and messaging (harness): the business sign-in pages a | BLOCKED | 2 | 7 min | 22:01Z |  |
| T354 | privacy: key rotation re-seals stored photos too, so droppin | done | 1 | 69 min | 21:39Z |  |
| T335 | adapters, payments and stand-in (harness): the reconcile's p | done | 1 | 59 min | 21:20Z |  |
| T412 | privacy: each business member's sign-in address and texting  | done | 1 | 111 min | 21:18Z |  |
| T442 | mutation (harness): every named mutant is killed by a determ | done | 1 | 59 min | 20:40Z |  |
| T357 | server, messaging and intake: START into an open request res | check_failed | 1 | 23 min | 20:39Z |  |
| T271 | server and calls (harness): the dialed leg asks for status c | done | 1 | 53 min | 20:22Z |  |
| T318 | calls, picks and quotes: the who's-calling text goes out at  | done | 1 | 52 min | 20:18Z |  |
| T277 | intake and privacy: a check-in reply reaches its job, and a  | done | 1 | 45 min | 20:13Z |  |
| T357 | server, messaging and intake: START into an open request res | resync | 1 | 48 min | 20:09Z |  |
| T314 | payments and picks: a dispute's auto-submit mark and its tas | done | 1 | 29 min | 19:56Z |  |
| T438 | Token usage counts each reply once: dedupe transcript usage  | BLOCKED | 1 | 10 min | 19:35Z |  |
| T357 | server, messaging and intake: START into an open request res | resync | 1 | 2 h 1 min | 18:26Z |  |
| T281 | picks: the unreachable-homeowner steps run at 24, 48, 72 and | done | 1 | 107 min | 18:15Z |  |
| T436 | portals, core and server: expired sessions are swept and old | done | 1 | 34 min | 17:52Z |  |
| T273 | privacy: a photo's plaintext is returned only after its open | done | 1 | 41 min | 17:10Z |  |
| T436 | portals, core and server: expired sessions are swept and old | resync | 1 | 28 min | 16:53Z |  |
| T386 | messaging (harness): the pick notice's "Can't take it" line  | done | 2 | 22 min | 16:46Z | merging main into the reviewed head conflicts in docs/for-casey-review.md; a resync merges it in on this attempt (D-891) |
| T319 | portals and server: security-event rows are capped per addre | done | 1 | 17 min | 16:40Z |  |
| T386 | messaging (harness): the pick notice's "Can't take it" line  | resync | 2 | 35 min | 16:03Z |  |
| T319 | portals and server: security-event rows are capped per addre | resync | 1 | 54 min | 15:51Z |  |
| T266 | portals, picks and privacy: the homeowner's contact choice a | done | 2 | 66 min | 15:48Z | session ended without handing in |
| T330 | picks, intake and server: a withdrawn pick tells its busines | done | 1 | 24 min | 15:05Z |  |
| T266 | portals, picks and privacy: the homeowner's contact choice a | QUEUED | 1 | 63 min | 14:38Z |  |

### Plan and harness merges on main
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
- 01:13Z Plan: T302 plan gap — new T450 sign-up code screens and verified consent (D-3602) (#730)
- 00:07Z Plan: follow-up tasks T447–T449 from spot-review of pre-D-3444 fix rounds (D-3581) (#724)
- 00:00Z Plan: fix T280/T328/T341 plan gaps; T444–T446; nightly gate robustness (D-3562, D-3563, D-3579, D-3580) (#723)
- 21:01Z Plan: fewer smoke runs, scoped fix rounds, batched reads, decision excerpts (D-3530) (#698)
- 20:53Z Plan: T438 plan gap, leave the ledger windows as today (D-3536) (#696)
- 19:27Z Plan: pre-flight 15, split T341 and T361 (D-3497) (#691)
- 19:23Z Plan: deterministic kills for proptest-only named mutants (D-3499) (#694)
- 19:02Z Plan: report-only symbol pre-flight in plan_health (D-3496) (#688)
- 18:48Z Plan: crate guides to ≤8 KB, detail one entry per line in NOTES.md (D-3474) (#687)
- 17:36Z Plan: count usage once per message, prove fix rounds were re-reviewed (D-3475) (#685)
- 15:02Z Plan: split T328 and trim T321 to fit the context limit (D-3468) (#676)
- 14:47Z Plan: fix T386 and D-3445 plan gaps (D-3467) (#673)

## Progress (requirements verified)

**Requirements verified:** 272 of 733 (37.1%) · partial 303 · unstarted 158
**Tasks merged (throughput only):** 326 of 451

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 13 | 16 | 6 | 35 |
| ai | 11 | 13 | 5 | 29 |
| businesses | 18 | 37 | 3 | 58 |
| calls | 10 | 14 | 1 | 25 |
| consent | 12 | 19 | 4 | 35 |
| fees | 14 | 37 | 1 | 52 |
| intake | 26 | 14 | 3 | 43 |
| job-cards | 18 | 6 | 0 | 24 |
| licensing | 33 | 15 | 0 | 48 |
| lifecycle | 23 | 5 | 0 | 28 |
| messaging | 24 | 19 | 2 | 45 |
| ops-recovery | 2 | 8 | 44 | 54 |
| privacy | 9 | 20 | 2 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 35 | 20 | 1 | 56 |
| recurring | 6 | 2 | 3 | 11 |
| security | 9 | 41 | 5 | 55 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 2 | 9 | 21 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-14T14:15:57Z (week)): week: 89.6M used, 104.4M left, resets 2026-11-19T17:00:00Z; projected finish 2026-11-24T18:17:45Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 206 of 226 done
- stage 5: 0 of 71 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 4: T302 (lease until 2026-11-14T17:01:27Z)

## Last usage reading
- {"task": "T261", "at": "2026-11-14T14:30:33Z", "output_tokens": 11875, "cache_read_input_tokens": 1445207, "cache_creation_input_tokens": 96920, "input_tokens": 65}
