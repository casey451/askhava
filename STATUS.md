# Ask Hava build status

Updated 2026-11-16T14:43:38Z by runner ba750becde3c (actions).

**Building: T477 in progress since 14:13Z; 360 of 535 done; week 94% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 360 of 535 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T477 claimed 14:13Z, lease until 17:13Z; lane 2: T497 claimed 14:16Z, lease until 17:16Z; lane 3: T101 claimed 14:32Z, lease until 17:32Z; lane 4: T521 claimed 14:35Z, lease until 17:35Z; lane 6: T105 claimed 14:07Z, lease until 17:07Z
- **Week:** 94% used, -1.9M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 14:43Z, 0 min before this update
- **Keeper:** last ran 14:40Z, next wake 17:09Z

## Needs attention
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- Strict plan check red: 9 problem(s) on tasks still to run (+121 on DONE tasks); first: T129: stored context_pct 10% but it measures 14%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T308 | portals (harness): the quote page's close button (JOB-019) i | MERGE_PENDING | 1 | 72 min | 14:42Z |  |
| T529 | payments: an owner refund has its own refund trigger (D-3883 | MERGE_PENDING | 2 | 28 min | 14:39Z | Ci: failure; Coverage: cancelled / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / tes |
| T519 | Copy check (harness): template text a requirement fixes, and | done | 2 | 49 min | 14:07Z | T519 attempt 1: fresh-eyes review found a compliance bypass that remained after the one allowed fix round (head a9dde6a4 |
| T477 | payments, picks and quotes: the scorecard's money, pick and  | QUEUED | 1 | 101 min | 14:05Z | T477 review not approved after the one fix round (D-522). Head cbf7179ee (fix round of 634d3f7dd: jobs_by_trade(db, to)  |
| T105 | ops: High and critical alerts with their automatic acknowled | QUEUED | 1 | 96 min | 13:58Z | Second fresh-eyes review of fix head dcb6f619b44181ec3d8bc89b16743ea25e02c4c0 returned FINDINGS after the one allowed fi |
| T529 | payments: an owner refund has its own refund trigger (D-3883 | check_failed | 1 | 69 min | 13:32Z |  |
| T101 | site: base layout in the design spec's look, works without J | check_failed | 1 | 70 min | 13:31Z | Ci: failure; E2e: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / {"timestamp":"2026-11-16T |
| T519 | Copy check (harness): template text a requirement fixes, and | QUEUED | 1 | 49 min | 13:11Z |  |
| T409 | server (harness): the door probe's owner actor signs in thro | done | 1 | 36 min | 13:05Z |  |
| T531 | messaging and server: OPS-046's first-six-weeks email figure | done | 1 | 36 min | 13:01Z |  |
| T100 | Stage 4 cleanup: dead code, duplicates, docs up to date | proof_missing | 1 | 39 min | 08:16Z |  |
| T517 | Stage 4 cleanup, part 1: duplicate functions in server, port | proof_missing | 1 | 84 min | 03:25Z |  |
| T099 | Journey J2 (business) and its variants, keyboard-only, with  | done | 2 | 92 min | 01:45Z | PLAN GAP: tests/e2e/webserver.ts:132 (T492/T227 file, forbidden to T099 by prompt line 126) gives the keyed binary a ran |
| T516 | Tooling (harness): scripts/check_duplicates.py, the duplicat | done | 3 | 30 min | 01:42Z | Second review of head 85baf148e5f34e6d4d48b8c0a54b3e8ff92fdbdd found FINDINGS after the one fix round: scripts/check_dup |
| T516 | Tooling (harness): scripts/check_duplicates.py, the duplicat | QUEUED | 2 | 40 min | 00:58Z |  |
| T516 | Tooling (harness): scripts/check_duplicates.py, the duplicat | QUEUED | 1 | 47 min | 00:01Z |  |
| T515 | End-to-end harness: specs can deliver a validly signed Strip | done | 2 | 50 min | 23:47Z | PLAN GAP: app/adapters/src/tests/ssrf_guard.rs:488 is owned by T176 and asserts the stand-in PaymentIntent has no livemo |
| T515 | End-to-end harness: specs can deliver a validly signed Strip | BLOCKED | 1 | 17 min | 22:11Z |  |
| T099 | Journey J2 (business) and its variants, keyboard-only, with  | BLOCKED | 1 | 58 min | 21:21Z |  |
| T464 | server and portals: the sign-up capability step is served af | done | 1 | 83 min | 19:56Z |  |
| T495 | e2e: the served Confirm and pay journey in the browser again | done | 1 | 44 min | 18:21Z |  |
| T462 | server: the sign-up inbox check is served behind the busines | done | 3 | 35 min | 18:05Z | PLAN GAP: app/portals/templates/email_check.html:47 owner T149 (done, #354), and app/portals/src/feedback.rs:61 owner T4 |
| T261 | portals and server (harness): the served Confirm route under | done | 6 | 85 min | 16:50Z | PLAN GAP: tests/e2e/confirm_served.spec.ts can't be built in T261. The fix belongs to tests/e2e/webserver.ts, which T227 |
| T455 | server: the Pause, Resume and Ask about a charge email links | done | 1 | 38 min | 14:58Z |  |

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 308 of 733 (42.0%) · partial 269 · unstarted 156
**Tasks merged (throughput only):** 360 of 535

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 13 | 16 | 6 | 35 |
| ai | 10 | 14 | 5 | 29 |
| businesses | 21 | 34 | 3 | 58 |
| calls | 14 | 10 | 1 | 25 |
| consent | 15 | 16 | 4 | 35 |
| fees | 23 | 28 | 1 | 52 |
| intake | 29 | 11 | 3 | 43 |
| job-cards | 20 | 4 | 0 | 24 |
| licensing | 34 | 14 | 0 | 48 |
| lifecycle | 24 | 4 | 0 | 28 |
| messaging | 26 | 17 | 2 | 45 |
| ops-recovery | 1 | 11 | 42 | 54 |
| privacy | 13 | 16 | 2 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 36 | 19 | 1 | 56 |
| recurring | 8 | 0 | 3 | 11 |
| security | 11 | 39 | 5 | 55 |
| terms | 1 | 2 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 2 | 9 | 21 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-16T14:27:56Z (week)): week: 195.9M used, -1.9M left, resets 2026-11-19T17:00:00Z; projected finish 2026-11-28T03:48:59Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 3 of 143 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T477 (lease until 2026-11-16T17:13:09Z)
- lane 2: T497 (lease until 2026-11-16T17:16:59Z)
- lane 3: T101 (lease until 2026-11-16T17:32:48Z)
- lane 4: T521 (lease until 2026-11-16T17:35:03Z)
- lane 6: T105 (lease until 2026-11-16T17:07:02Z)

## Last usage reading
- {"task": "T308", "at": "2026-11-16T14:42:38Z", "output_tokens": 49444, "cache_creation_input_tokens": 213805, "cache_read_input_tokens": 4933751, "input_tokens": 145}
