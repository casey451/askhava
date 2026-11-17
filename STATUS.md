# Ask Hava build status

Updated 2026-11-17T02:04:21Z by runner dde16ca377f9 (actions).

**Building: T103 in progress since 00:02Z; 377 of 556 done; week 11% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 377 of 556 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T103 claimed 00:02Z, lease until 04:55Z; lane 4: T221 claimed 01:54Z, lease until 04:54Z; lane 5: T469 claimed 00:32Z, lease until 04:01Z; lane 7: T586 claimed 02:01Z, lease until 05:01Z
- **Week:** 11% used, 167.3M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 02:02Z, 2 min before this update
- **Keeper:** last ran 02:01Z, next wake 04:04Z

## Needs attention
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T221 | site: the terms, privacy policy, SMS terms, homeowner data a | check_failed | 1 | 2 h 47 min | 01:23Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Version:   0.13. |
| T320 | ops: a check that runs on a schedule raises its alert once w | MERGE_PENDING | 2 | 78 min | 01:13Z | Ci: failure; E2e: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / {"timestamp":"2026-11-16T |
| T373 | businesses: changing a phone's number ends that phone's text | MERGE_PENDING | 2 | 42 min | 01:04Z | Ci: failure; E2e: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / Finished `dev` profile [u |
| T111 | Draft homeowner and business terms with every required claus | done | 4 | 34 min | 23:57Z | Review not approved after the one allowed fix round (head d014c9d4313961aa8e702ef758390e00b95fe641). Drafts, clauses, sc |
| T373 | businesses: changing a phone's number ends that phone's text | check_failed | 1 | 74 min | 23:55Z |  |
| T523 | quotes, jobs, picks and intake: the readers the owner flags  | QUEUED | 2 | 89 min | 23:36Z | Review not approved after the one findings round (head bd99856fca96fb81973c28181ad9fd6e39e7ac91; flag_inputs tests 5 pas |
| T111 | Draft homeowner and business terms with every required claus | BLOCKED | 3 | 37 min | 22:10Z |  |
| T320 | ops: a check that runs on a schedule raises its alert once w | check_failed | 1 | 39 min | 21:46Z |  |
| T523 | quotes, jobs, picks and intake: the readers the owner flags  | BLOCKED | 1 | 5 min | 21:38Z |  |
| T111 | Draft homeowner and business terms with every required claus | QUEUED | 2 | 66 min | 21:28Z |  |
| T408 | payments and server: a release waits while safe mode is on a | BLOCKED | 1 | 14 min | 21:22Z | PLAN GAP: tests/integration/tests/safe_mode.rs:480-484 (owned by T487) fails on any `safe_mode_on(` call outside `paymen |
| T102 | site: home and how-it-works pages with the footer, tagline,  | done | 1 | 46 min | 21:03Z |  |
| T307 | server and portals: the homeowner portal's sign-in, home pag | BLOCKED | 1 | 41 min | 20:41Z | PLAN GAP: tests/security/tests/vault_readers.rs:23 lets only `portals` call `read_own_contact` (line 19 lets only `messa |
| T361 | quotes and picks: a quote keeps its number once shown, and a | done | 2 | 93 min | 20:12Z | PLAN GAP: app/intake/src/job_texts.rs:473 (`(1..).zip(page)`) and :494 (`.filter(\|(n, _)\| others.contains(n))`). kit/tas |
| T487 | ops and payments: safe mode after 24 hours pauses charges, a | done | 1 | 50 min | 19:47Z |  |
| T102 | site: home and how-it-works pages with the footer, tagline,  | resync | 1 | 30 min | 19:37Z |  |
| T324 | harness: 95% branch floors for the money and privacy files o | done | 2 | 69 min | 19:02Z | Review found the per-file floor list still incomplete after one fix round (head 56aef45f3). All 16 floors added so far a |
| T303 | server and portals: owner sign-in, sign-out and step-up are  | done | 1 | 29 min | 18:55Z |  |
| T102 | site: home and how-it-works pages with the footer, tagline,  | resync | 1 | 59 min | 18:42Z |  |
| T166 | site: self-hosted Gelasio and DM Sans fonts and the design-t | done | 1 | 52 min | 18:17Z |  |
| T105 | ops: High and critical alerts with their automatic acknowled | done | 3 | 51 min | 18:12Z | merging main into the reviewed head conflicts in app/ops/src/api.rs; a resync merges it in on this attempt (D-891) |
| T559 | Tests (harness): wait for the code step's autofocus and the  | done | 1 | 64 min | 18:02Z |  |
| T303 | server and portals: owner sign-in, sign-out and step-up are  | resync | 1 | 92 min | 17:52Z |  |
| T361 | quotes and picks: a quote keeps its number once shown, and a | BLOCKED | 1 | 50 min | 17:41Z |  |
| T324 | harness: 95% branch floors for the money and privacy files o | QUEUED | 1 | 61 min | 17:33Z |  |
| T475 | messaging, quotes, payments and ai: the readers behind the O | done | 1 | 114 min | 17:02Z |  |
| T105 | ops: High and critical alerts with their automatic acknowled | resync | 3 | 38 min | 16:53Z |  |
| T473 | Pin backup-runner's Postgres service image by digest | done | 1 | 17 min | 16:46Z |  |
| T101 | site: base layout in the design spec's look, works without J | done | 3 | 80 min | 16:39Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T111 | Draft homeowner and business terms with every required claus | BLOCKED | 1 | 4 min | 16:21Z |  |
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

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 307 of 733 (41.9%) · partial 313 · unstarted 113
**Tasks merged (throughput only):** 377 of 556

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 20 | 11 | 4 | 35 |
| ai | 11 | 13 | 5 | 29 |
| businesses | 21 | 35 | 2 | 58 |
| calls | 13 | 11 | 1 | 25 |
| consent | 14 | 17 | 4 | 35 |
| fees | 24 | 27 | 1 | 52 |
| intake | 28 | 12 | 3 | 43 |
| job-cards | 20 | 4 | 0 | 24 |
| licensing | 37 | 11 | 0 | 48 |
| lifecycle | 21 | 7 | 0 | 28 |
| messaging | 24 | 19 | 2 | 45 |
| ops-recovery | 1 | 22 | 31 | 54 |
| privacy | 12 | 17 | 2 | 31 |
| quiet-hours | 6 | 3 | 0 | 9 |
| quotes-pick | 33 | 22 | 1 | 56 |
| recurring | 8 | 0 | 3 | 11 |
| security | 11 | 39 | 5 | 55 |
| terms | 1 | 20 | 14 | 35 |
| website | 0 | 14 | 14 | 28 |
| wording | 2 | 9 | 21 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-17T02:00:44Z (week)): week: 26.7M used, 167.3M left, resets 2026-11-19T17:00:00Z; projected finish 2026-11-29T04:45:20Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 20 of 164 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T103 (lease until 2026-11-17T04:55:48Z)
- lane 4: T221 (lease until 2026-11-17T04:54:09Z)
- lane 5: T469 (lease until 2026-11-17T04:01:32Z)
- lane 7: T586 (lease until 2026-11-17T05:01:43Z)

## Last usage reading
- {"task": "T221", "at": "2026-11-17T01:23:31Z", "input_tokens": 420, "output_tokens": 130553, "cache_creation_input_tokens": 428679, "cache_read_input_tokens": 18351949}
