# Ask Hava build status

Updated 2026-11-14T03:33:30Z by runner 224fe4c7f956 (actions).

**Building: T440 in progress since 02:53Z; 309 of 435 done; week 35% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 309 of 435 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 5: T440 claimed 02:53Z, lease until 05:53Z
- **Week:** 35% used, 120.4M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 03:33Z, 0 min before this update
- **Keeper:** last ran 03:30Z, next wake 05:56Z

## Needs attention
- BLOCKED T302: plan gap reported: PLAN GAP: T302 depends on T450 (D-3602), whose work is not on main: grep IdentityCode|EmailCode in app/portals/src finds nothing, app/portals/src/tests/signup_code_screens.rs does not exist. T302 owns no portals file, so it cannot serve the email-code and identity-code steps (TD-SEC-45) without T450's Screen::IdentityCode, Screen::EmailCode, route constants and channel parser. kit/state.json on claude/kit-state has no T450 and T302's deps there stop at T415, so the runner started T302 before T450. Reviewer confirmed every claim.; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T302 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T280 (since 2026-11-14T02:23:41Z, no attempt used; a plan change fixes it): T280: measures 51% of the context window, over its 50% limit
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T396 (since 2026-11-14T00:51:46Z, no attempt used; a plan change fixes it): T396: measures 53% of the context window, over its 50% limit
- Strict plan check red: 33 problem(s) on tasks still to run (+204 on DONE tasks); first: T396: measures 53% of the window, over its 50% limit; trim its reads or split it

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T344 | picks: the global daily Verify cap holds under concurrent co | MERGE_PENDING | 1 | 28 min | 03:23Z |  |
| T433 | server and payments: every payments task runs in the product | MERGE_PENDING | 1 | 57 min | 03:21Z |  |
| T449 | server and intake: emergency first texts count toward the re | MERGE_PENDING | 1 | 31 min | 03:17Z | merging main into the reviewed head conflicts in app/server/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T450 | portals: the sign-up code screens and the texts choice with  | done | 1 | 79 min | 03:00Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | MERGE_PENDING | 1 | 23 min | 02:41Z | merging main into the reviewed head conflicts in app/calls/CLAUDE.md, app/intake/CLAUDE.md, app/messaging/CLAUDE.md, app |
| T449 | server and intake: emergency first texts count toward the re | resync | 1 | 47 min | 02:29Z |  |
| T341 | picks (harness): a pick of a tiers quote names its tier, and | done | 2 | 21 min | 02:25Z | merging main into the reviewed head conflicts in docs/for-casey-review.md; a resync merges it in on this attempt (D-891) |
| T280 | server and messaging (harness): the business sign-in pages a | resync | 3 | 107 min | 02:23Z | merging main into the reviewed head conflicts in app/server/CLAUDE.md, docs/for-casey-review.md; a resync merges it in o |
| T302 | server and portals: business sign-up is served (every sign-u | BLOCKED | 2 | 1 min | 01:33Z | PLAN GAP: T302 depends on T450 (D-3602), whose work is not on main: grep IdentityCode\|EmailCode in app/portals/src finds |
| T447 | messaging: a passing vault error holds an outbox row instead | done | 1 | 26 min | 01:30Z |  |
| T341 | picks (harness): a pick of a tiers quote names its tier, and | resync | 2 | 67 min | 01:10Z |  |
| T328 | picks and server: an ended pick offers the next quote (`paym | done | 2 | 61 min | 01:04Z | PLAN GAP: app/intake/src/job_texts.rs is outside T328's scope (owned by T240, T247, T278, T402). The PICK-046 text canno |
| T302 | server and portals: business sign-up is served (every sign-u | BLOCKED | 1 | 2 min | 00:54Z |  |
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
| T330 | picks, intake and server: a withdrawn pick tells its busines | resync | 1 | 39 min | 14:30Z |  |
| T367 | jobs, messaging, privacy, intake and payments: the daily swe | done | 1 | 60 min | 14:23Z |  |
| T386 | messaging (harness): the pick notice's "Can't take it" line  | BLOCKED | 1 | 16 min | 13:41Z |  |
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

### Plan and harness merges on main
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

## Progress (requirements verified)

**Requirements verified:** 277 of 733 (37.8%) · partial 296 · unstarted 160
**Tasks merged (throughput only):** 309 of 435

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 15 | 14 | 6 | 35 |
| ai | 11 | 13 | 5 | 29 |
| businesses | 20 | 35 | 3 | 58 |
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
| quotes-pick | 36 | 19 | 1 | 56 |
| recurring | 6 | 2 | 3 | 11 |
| security | 9 | 41 | 5 | 55 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 2 | 7 | 23 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-14T02:47:16Z (week)): week: 73.6M used, 120.4M left, resets 2026-11-19T17:00:00Z; projected finish 2026-11-27T05:15:31Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 189 of 214 done
- stage 5: 0 of 67 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 5: T440 (lease until 2026-11-14T05:53:03Z)

## Last usage reading
- {"task": "T344", "at": "2026-11-14T03:23:17Z", "cache_read_input_tokens": 1954809, "cache_creation_input_tokens": 102563, "input_tokens": 102, "output_tokens": 25415}
