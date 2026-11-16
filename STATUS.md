# Ask Hava build status

Updated 2026-11-16T19:41:43Z by runner b79bf6a7ad64 (actions).

**Building: T481 in progress since 19:16Z; 371 of 540 done; week 1% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 371 of 540 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 2: T481 claimed 19:16Z, lease until 22:16Z; lane 3: T487 claimed 18:57Z, lease until 22:27Z; lane 7: T361 claimed 18:39Z, lease until 21:39Z
- **Week:** 1% used, 190.6M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 19:41Z, 0 min before this update
- **Keeper:** last ran 19:38Z, next wake 21:42Z

## Needs attention
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T523 (since 2026-11-16T19:07:14Z, no attempt used; a plan change fixes it): T523: dependency T105 is DONE but its owned file app/ops/src/alerts.rs is not on main; T523: dependency T105 is DONE but its owned file migrations/0192_ops_alerts_safe_mode.sql is not on main; T523: dependency T105 is DONE but its owned file tests/integration/tests/ops_alert_conditions.rs is not on main; T523: dependency T105 is DONE but its owned file app/jobs/src/request_counts.rs is not on main
- Strict plan check red: 12 problem(s) on tasks still to run (+129 on DONE tasks); first: T116: stored context_pct 8% but it measures 12%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T102 | site: home and how-it-works pages with the footer, tagline,  | MERGE_PENDING | 1 | 30 min | 19:37Z | merging main into the reviewed head conflicts in app/quotes/src/lib.rs; a resync merges it in on this attempt (D-891) |
| T324 | harness: 95% branch floors for the money and privacy files o | MERGE_PENDING | 2 | 69 min | 19:02Z | Review found the per-file floor list still incomplete after one fix round (head 56aef45f3). All 16 floors added so far a |
| T303 | server and portals: owner sign-in, sign-out and step-up are  | MERGE_PENDING | 1 | 29 min | 18:55Z | merging main into the reviewed head conflicts in app/server/src/lib.rs; a resync merges it in on this attempt (D-891) |
| T102 | site: home and how-it-works pages with the footer, tagline,  | resync | 1 | 59 min | 18:42Z |  |
| T166 | site: self-hosted Gelasio and DM Sans fonts and the design-t | done | 1 | 52 min | 18:17Z |  |
| T105 | ops: High and critical alerts with their automatic acknowled | done | 3 | 51 min | 18:12Z | merging main into the reviewed head conflicts in app/ops/src/api.rs; a resync merges it in on this attempt (D-891) |
| T559 | Tests (harness): wait for the code step's autofocus and the  | done | 1 | 64 min | 18:02Z |  |
| T303 | server and portals: owner sign-in, sign-out and step-up are  | resync | 1 | 92 min | 17:52Z |  |
| T361 | quotes and picks: a quote keeps its number once shown, and a | BLOCKED | 1 | 50 min | 17:41Z | PLAN GAP: app/intake/src/job_texts.rs:473 (`(1..).zip(page)`) and :494 (`.filter(\|(n, _)\| others.contains(n))`). kit/tas |
| T324 | harness: 95% branch floors for the money and privacy files o | QUEUED | 1 | 61 min | 17:33Z |  |
| T475 | messaging, quotes, payments and ai: the readers behind the O | done | 1 | 114 min | 17:02Z |  |
| T105 | ops: High and critical alerts with their automatic acknowled | resync | 3 | 38 min | 16:53Z |  |
| T473 | Pin backup-runner's Postgres service image by digest | done | 1 | 17 min | 16:46Z |  |
| T101 | site: base layout in the design spec's look, works without J | done | 3 | 80 min | 16:39Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T111 | Draft homeowner and business terms with every required claus | BLOCKED | 1 | 4 min | 16:21Z | PLAN GAP: scripts/check_legal_docs.py:403-410 and :458 contradict TERMS-026 (docs/spec/terms.md:29) and D-3877 (kit/prom |
| T105 | ops: High and critical alerts with their automatic acknowled | check_failed | 2 | 104 min | 15:51Z |  |
| T497 | Stand-ins (harness): sticky vendor faults for the J4 outage  | done | 1 | 75 min | 15:32Z |  |
| T101 | site: base layout in the design spec's look, works without J | check_failed | 2 | 39 min | 15:11Z |  |
| T521 | catalog: each category's plural display name and a list read | done | 1 | 34 min | 15:09Z |  |
| T477 | payments, picks and quotes: the scorecard's money, pick and  | done | 2 | 42 min | 14:55Z | T477 review not approved after the one fix round (D-522). Head cbf7179ee (fix round of 634d3f7dd: jobs_by_trade(db, to)  |
| T308 | portals (harness): the quote page's close button (JOB-019) i | done | 1 | 72 min | 14:42Z |  |
| T529 | payments: an owner refund has its own refund trigger (D-3883 | done | 2 | 28 min | 14:39Z | Ci: failure; Coverage: cancelled / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / tes |
| T519 | Copy check (harness): template text a requirement fixes, and | done | 2 | 49 min | 14:07Z | T519 attempt 1: fresh-eyes review found a compliance bypass that remained after the one allowed fix round (head a9dde6a4 |
| T477 | payments, picks and quotes: the scorecard's money, pick and  | QUEUED | 1 | 101 min | 14:05Z |  |
| T105 | ops: High and critical alerts with their automatic acknowled | QUEUED | 1 | 96 min | 13:58Z |  |
| T529 | payments: an owner refund has its own refund trigger (D-3883 | check_failed | 1 | 69 min | 13:32Z |  |
| T101 | site: base layout in the design spec's look, works without J | check_failed | 1 | 70 min | 13:31Z |  |
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

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 317 of 733 (43.2%) · partial 273 · unstarted 143
**Tasks merged (throughput only):** 371 of 540

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 15 | 16 | 4 | 35 |
| ai | 11 | 13 | 5 | 29 |
| businesses | 22 | 33 | 3 | 58 |
| calls | 14 | 10 | 1 | 25 |
| consent | 15 | 16 | 4 | 35 |
| fees | 23 | 28 | 1 | 52 |
| intake | 30 | 10 | 3 | 43 |
| job-cards | 20 | 4 | 0 | 24 |
| licensing | 39 | 9 | 0 | 48 |
| lifecycle | 24 | 4 | 0 | 28 |
| messaging | 26 | 17 | 2 | 45 |
| ops-recovery | 1 | 19 | 34 | 54 |
| privacy | 13 | 16 | 2 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 35 | 20 | 1 | 56 |
| recurring | 8 | 0 | 3 | 11 |
| security | 11 | 39 | 5 | 55 |
| terms | 1 | 2 | 32 | 35 |
| website | 0 | 6 | 22 | 28 |
| wording | 2 | 9 | 21 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-16T19:13:14Z (week)): week: 3.4M used, 190.6M left, resets 2026-11-19T17:00:00Z; projected finish 2026-11-28T06:56:20Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 14 of 148 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 2: T481 (lease until 2026-11-16T22:16:25Z)
- lane 3: T487 (lease until 2026-11-16T22:27:38Z)
- lane 7: T361 (lease until 2026-11-16T21:39:42Z)

## Last usage reading
- {"task": "T102", "at": "2026-11-16T19:37:31Z", "cache_creation_input_tokens": 85181, "output_tokens": 14123, "input_tokens": 81, "cache_read_input_tokens": 1571188}
