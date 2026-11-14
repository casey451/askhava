# Ask Hava build status

Updated 2026-11-14T06:26:24Z by runner 6aa7f43d7bd8 (actions).

**Building: T437 in progress since 06:17Z; 314 of 436 done; week 37% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 314 of 436 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 1: T437 claimed 06:17Z, lease until 09:17Z; lane 6: T396 claimed 06:18Z, lease until 09:18Z
- **Week:** 37% used, 119.5M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 06:26Z, 0 min before this update
- **Keeper:** last ran 06:26Z, next wake 09:20Z

## Needs attention
- BLOCKED T280: session ended without handing in
- BLOCKED T302: plan gap reported: PLAN GAP: tests/security/tests/auth_matrix.rs:146 and app/server/tests/door_matrix.rs:161 treat only /b/signup itself as the open sign-in door (exact match); every other /b/signup/... path (T450's /b/signup/start, /send, /old, /letter, /identity-code, /email-code, /email-code/resend, /service-area at app/portals/src/business_signup.rs:92-104,303) falls to the Business door (signed-in business only, auth_matrix.rs:162,177; door_matrix.rs:173-174,186). T302 must serve them without a session (TD-SEC-45). T302 owns neither file (kit/prompts/T302.md:34-38,50,146), nor tests/security/door_cases.txt:; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T302 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- Strict plan check red: 43 problem(s) on tasks still to run (+97 on DONE tasks); first: T396: stored context_pct 24% but it measures 17%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T352 | adapters and stand-in (harness): the PaymentIntent carries S | MERGE_PENDING | 1 | 51 min | 06:14Z |  |
| T396 | businesses and messaging (harness): a phone a member links o | resync | 2 | 51 min | 06:03Z | merging main into the reviewed head conflicts in docs/architecture.md; a resync merges it in on this attempt (D-891) |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | resync | 2 | 51 min | 06:02Z | merging main into the reviewed head conflicts in app/portals/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T280 | server and messaging (harness): the business sign-in pages a | BLOCKED | 3 | 22 min | 05:44Z | merging main into the reviewed head conflicts in app/server/CLAUDE.md, docs/for-casey-review.md; a resync merges it in o |
| T393 | portals and licensing: the served quote page's cards show ea | done | 1 | 74 min | 05:40Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | update_conflict | 1 | 39 min | 05:04Z |  |
| T396 | businesses and messaging (harness): a phone a member links o | check_failed | 1 | 45 min | 04:54Z |  |
| T449 | server and intake: emergency first texts count toward the re | done | 1 | 34 min | 04:28Z |  |
| T302 | server and portals: business sign-up is served (every sign-u | BLOCKED | 3 | 15 min | 04:11Z | PLAN GAP: tests/security/tests/auth_matrix.rs:146 and app/server/tests/door_matrix.rs:161 treat only /b/signup itself as |
| T440 | portals: the quote page's tier choice and the Confirm page's | done | 1 | 50 min | 03:43Z |  |
| T344 | picks: the global daily Verify cap holds under concurrent co | done | 1 | 28 min | 03:23Z |  |
| T433 | server and payments: every payments task runs in the product | done | 1 | 57 min | 03:21Z |  |
| T449 | server and intake: emergency first texts count toward the re | resync | 1 | 31 min | 03:17Z |  |
| T450 | portals: the sign-up code screens and the texts choice with  | done | 1 | 79 min | 03:00Z |  |
| T437 | harness: a crate NOTES.md is a shared path wherever its CLAU | resync | 1 | 23 min | 02:41Z |  |
| T449 | server and intake: emergency first texts count toward the re | resync | 1 | 47 min | 02:29Z |  |
| T341 | picks (harness): a pick of a tiers quote names its tier, and | done | 2 | 21 min | 02:25Z | merging main into the reviewed head conflicts in docs/for-casey-review.md; a resync merges it in on this attempt (D-891) |
| T280 | server and messaging (harness): the business sign-in pages a | resync | 3 | 107 min | 02:23Z |  |
| T302 | server and portals: business sign-up is served (every sign-u | BLOCKED | 2 | 1 min | 01:33Z |  |
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

### Plan and harness merges on main
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
- 13:45Z Plan: shorten the critical path, drop legacy dependency edges (D-3446) (#671)
- 12:32Z Plan: pre-flight 14, split three oversized build tasks; T111/T113 brand note (D-3445) (#668)
- 11:32Z Harness: fix-round hand-off, tighter output caps, crate_cap 2 (D-3444) (#665)
- 11:16Z Harness: an API error session death does not use an attempt (D-3443) (#664)
- 10:13Z Plan: give T255 back the attempt an API 403 outage took (D-3442) (#663)
- 09:02Z Plan: T255 plan gap 2, the address is written onto the homeowner's own contact (D-3431) (#660)
- 08:55Z Plan: T428 plan gap, new prep task owns the six pinned tests (D-3430) (#659)

## Progress (requirements verified)

**Requirements verified:** 277 of 733 (37.8%) · partial 296 · unstarted 160
**Tasks merged (throughput only):** 314 of 436

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
- Pacing (session_reading 2026-11-14T06:15:04Z (week)): week: 74.5M used, 119.5M left, resets 2026-11-19T17:00:00Z; projected finish 2026-11-24T07:14:27Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 194 of 215 done
- stage 5: 0 of 67 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T437 (lease until 2026-11-14T09:17:58Z)
- lane 6: T396 (lease until 2026-11-14T09:18:56Z)

## Last usage reading
- {"task": "T352", "at": "2026-11-14T06:14:13Z", "output_tokens": 29405, "input_tokens": 129, "cache_creation_input_tokens": 144997, "cache_read_input_tokens": 2505455}
