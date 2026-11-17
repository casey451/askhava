# Ask Hava build status

Updated 2026-11-17T07:40:25Z by runner eabc0711c109 (actions).

**Building: T107 in progress since 07:34Z; 387 of 559 done; week 16% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 387 of 559 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 2: T107 claimed 07:34Z, lease until 10:34Z; lane 6: T112 claimed 07:36Z, lease until 10:36Z
- **Week:** 16% used, 160.7M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 07:40Z, 0 min before this update
- **Keeper:** last ran 07:40Z, next wake 10:37Z

## Needs attention
- BLOCKED T489: plan gap reported: PLAN GAP: tests/security/tests/auth_matrix.rs:722. The `POST /request` route this task must serve (D-3878, D-3882) makes `sec_048_every_refusal_is_a_security_event` panic at auth_matrix.rs:181:9 ("/request has no door in auth_matrix.rs: add its matrix rule (SEC-019)"). sec_048 (:720-:723) builds its POST list from route_table() and calls door(path, false), ignoring the served-by-site flag that td_sec_01 (:637) and :693 pass; so any site POST outside the listed prefixes panics. POST /request is the first site POST. The file is owned by T348 (harness; also T088, T231, T302, T308); T489 does not ; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T489 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- Strict plan check red: 2 problem(s) on tasks still to run (+147 on DONE tasks); first: T115: stored context_pct 13% but it measures 19%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T112 | Draft the privacy policy and SMS terms | QUEUED | 1 | 62 min | 07:32Z | Fresh-eyes review of the fix-round head b41671e9e0621319f78668d66ed284b6d6f5aa4f ended REVIEW-VERDICT: FINDINGS after th |
| T201 | site: services and help pages, the ROC complaint page | MERGE_PENDING | 1 | 56 min | 07:22Z |  |
| T489 | site and server: the web chat and request pages with the con | BLOCKED | 1 | 12 min | 07:15Z | PLAN GAP: tests/security/tests/auth_matrix.rs:722. The `POST /request` route this task must serve (D-3878, D-3882) makes |
| T564 | quotes, picks and intake: the LIST, text picks, lapse text a | MERGE_PENDING | 1 | 43 min | 06:47Z |  |
| T204 | ops: phone pushes by alert email, OPS-023 acknowledgements,  | MERGE_PENDING | 1 | 2 h 21 min | 06:44Z |  |
| T586 | calls and privacy: TwiML text is escaped, a full number pool | MERGE_PENDING | 1 | 88 min | 06:17Z |  |
| T481 | messaging and payments: mark a replayed text sent from the c | done | 1 | 114 min | 05:36Z |  |
| T103 | intake: the web-chat conversation, where typing a verified m | done | 1 | 106 min | 05:27Z |  |
| T585 | businesses and licensing: a license change that drops job ty | done | 1 | 43 min | 05:10Z |  |
| T571 | server and calls: a press of 9 on the safety call is never l | done | 1 | 45 min | 05:08Z |  |
| T221 | site: the terms, privacy policy, SMS terms, homeowner data a | done | 2 | 29 min | 04:41Z | merging main into the reviewed head conflicts in docs/architecture.md; a resync merges it in on this attempt (D-891) |
| T591 | Runner (harness): free the backup job's disk and cap the smo | done | 1 | 67 min | 04:23Z |  |
| T107 | Consent records kept 5 years, no export, opt-out rate alerts | QUEUED | 1 | 85 min | 03:45Z | Second fresh-eyes review of head dfb993caba5ddab51d93dc1f24c5403dc513e3e7 ended REVIEW-VERDICT: FINDINGS (the one findin |
| T584 | businesses (harness): sign-up activation retried after a con | done | 2 | 43 min | 03:23Z | PLAN GAP: tests/integration/tests/business_event_emails.rs:318 (owned by T329, not T584) expects `emergency_test` to rea |
| T221 | site: the terms, privacy policy, SMS terms, homeowner data a | resync | 2 | 79 min | 03:14Z |  |
| T584 | businesses (harness): sign-up activation retried after a con | BLOCKED | 1 | 3 min | 02:26Z |  |
| T469 | server (harness): the Resend email client from settings and  | done | 1 | 99 min | 02:12Z |  |
| T221 | site: the terms, privacy policy, SMS terms, homeowner data a | check_failed | 1 | 2 h 47 min | 01:23Z |  |
| T320 | ops: a check that runs on a schedule raises its alert once w | done | 2 | 78 min | 01:13Z | Ci: failure; E2e: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / {"timestamp":"2026-11-16T |
| T373 | businesses: changing a phone's number ends that phone's text | done | 2 | 42 min | 01:04Z | Ci: failure; E2e: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / Finished `dev` profile [u |
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

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 302 of 733 (41.2%) · partial 328 · unstarted 103
**Tasks merged (throughput only):** 387 of 559

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 19 | 12 | 4 | 35 |
| ai | 11 | 14 | 4 | 29 |
| businesses | 21 | 35 | 2 | 58 |
| calls | 13 | 11 | 1 | 25 |
| consent | 14 | 18 | 3 | 35 |
| fees | 24 | 27 | 1 | 52 |
| intake | 25 | 18 | 0 | 43 |
| job-cards | 20 | 4 | 0 | 24 |
| licensing | 36 | 12 | 0 | 48 |
| lifecycle | 21 | 7 | 0 | 28 |
| messaging | 24 | 19 | 2 | 45 |
| ops-recovery | 1 | 23 | 30 | 54 |
| privacy | 12 | 17 | 2 | 31 |
| quiet-hours | 6 | 3 | 0 | 9 |
| quotes-pick | 33 | 22 | 1 | 56 |
| recurring | 8 | 0 | 3 | 11 |
| security | 11 | 39 | 5 | 55 |
| terms | 1 | 23 | 11 | 35 |
| website | 0 | 15 | 13 | 28 |
| wording | 2 | 9 | 21 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-17T07:33:16Z (week)): week: 33.3M used, 160.7M left, resets 2026-11-19T17:00:00Z; projected finish 2026-11-29T06:24:12Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 30 of 167 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 2: T107 (lease until 2026-11-17T10:34:41Z)
- lane 6: T112 (lease until 2026-11-17T10:36:38Z)

## Last usage reading
- {"task": "T112", "at": "2026-11-17T07:32:37Z", "cache_creation_input_tokens": 210385, "cache_read_input_tokens": 4797203, "output_tokens": 52160, "input_tokens": 163}
