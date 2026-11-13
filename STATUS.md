# Ask Hava build status

Updated 2026-11-13T23:02:06Z by runner 31c17d5aa447 (actions).

**Building: T355 in progress since 22:11Z; 299 of 428 done; week 30% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 299 of 428 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 1: T355 claimed 22:11Z, lease until 01:11Z; lane 3: T415 claimed 22:05Z, lease until 01:05Z; lane 4: T391 claimed 22:36Z, lease until 01:36Z
- **Week:** 30% used, 125.6M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 23:02Z, 0 min before this update
- **Keeper:** last ran 23:01Z, next wake 01:08Z

## Needs attention
- BLOCKED T280: plan gap reported: PLAN GAP: app/portals/src/business_auth.rs:176-178 CodeAddress::code_to(db, member) gets only a team-member row ID; no public API in any crate maps a member ID to business, mailbox or own staff-phone lines (businesses::api and team.rs lookups need the business or mailbox first: team.rs:613 members, api.rs:178 businesses_of_mailbox, channel_preference.rs:132 notice_recipients; locked_member team.rs:253 is private). server runs no SQL (0 files with sqlx::query) and T280 owns no businesses file (kit/prompts/T280.md:34-39, :49), T415 adds only contact_ids_in_use (bulk). Reviewer confirmed. Smalles; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T280 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T328: plan gap reported: PLAN GAP: app/intake/src/job_texts.rs is outside T328's scope (owned by T240, T247, T278, T402). The PICK-046 text cannot be sent from picks: picks may not depend on intake (kit/layers.json), and picks.pick stores no homeowner contact (migrations/0079_picks_code.sql:18-32). payments.pick_ended fires when no picks.release row exists, so release.contact_id is unavailable. The jobs.homeowner_text path (app/intake/src/job_texts.rs:56-74, 128-136, 358) has no PICK-046 template, no quote-list link, no EmergencyJob exemption, and is keyed once per job. Reviewer confirmed. Fix: a task that adds a pick; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T328 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- Strict plan check red: 28 problem(s) on tasks still to run (+196 on DONE tasks); first: T396: measures 51% of the window, over its 50% limit; trim its reads or split it

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T328 | picks and server: an ended pick offers the next quote (`paym | BLOCKED | 1 | 4 min | 23:00Z | PLAN GAP: app/intake/src/job_texts.rs is outside T328's scope (owned by T240, T247, T278, T402). The PICK-046 text canno |
| T340 | picks and payments: the fee at the pick uses the job's lates | done | 1 | 40 min | 22:32Z |  |
| T357 | server, messaging and intake: START into an open request res | done | 2 | 21 min | 22:14Z | Ci: failure; Lint and slow tests: failure / Lint and slow tests failed at step: Rust format and lint (clippy once, befor |
| T438 | Token usage counts each reply once: dedupe transcript usage  | MERGE_PENDING | 2 | 19 min | 22:10Z | PLAN GAP: tests/test_failure_drills.py:1046 (test_usage_feeds_the_pacing_windows; owned by no task, T438 may not change  |
| T280 | server and messaging (harness): the business sign-in pages a | BLOCKED | 2 | 7 min | 22:01Z | PLAN GAP: app/portals/src/business_auth.rs:176-178 CodeAddress::code_to(db, member) gets only a team-member row ID; no p |
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

### Plan and harness merges on main
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
- 01:57Z Harness: crate_cap back to 1, D-3303 resync guardrail tripped (D-3386) (#645)
- 01:29Z Harness: fail CI, smoke and prehandin on leftover merge-conflict markers (D-3307) (#619)
- 01:16Z Plan: T255 follow-ups (D-3384) and pre-flight 12, T426 owns the legal-docs test (D-3385) (#640)
- 00:30Z Plan: rename Ask Hava to Laftly (Laftly LLC, laftly.com), D-3347 (#631)
- 00:14Z Plan: T255 plan gap: re-clear builds Releasable from the stored success (D-3358) (#638)

## Progress (requirements verified)

**Requirements verified:** 281 of 733 (38.3%) · partial 292 · unstarted 160
**Tasks merged (throughput only):** 299 of 428

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 15 | 14 | 6 | 35 |
| ai | 11 | 13 | 5 | 29 |
| businesses | 20 | 35 | 3 | 58 |
| calls | 10 | 14 | 1 | 25 |
| consent | 14 | 17 | 4 | 35 |
| fees | 14 | 37 | 1 | 52 |
| intake | 27 | 13 | 3 | 43 |
| job-cards | 19 | 5 | 0 | 24 |
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
- Pacing (session_reading 2026-11-13T22:48:18Z (week)): week: 68.4M used, 125.6M left, resets 2026-11-19T17:00:00Z; projected finish 2026-12-06T19:21:51Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 179 of 208 done
- stage 5: 0 of 66 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T355 (lease until 2026-11-14T01:11:47Z)
- lane 3: T415 (lease until 2026-11-14T01:05:51Z)
- lane 4: T391 (lease until 2026-11-14T01:36:08Z)

## Last usage reading
- {"task": "T328", "at": "2026-11-13T23:00:58Z", "cache_creation_input_tokens": 209848, "output_tokens": 39613, "cache_read_input_tokens": 4737864, "input_tokens": 181}
