# Ask Hava build status

Updated 2026-11-16T17:59:45Z by runner d77b0f992ca9 (actions).

**Building: T324 in progress since 17:53Z; 367 of 539 done; week 98% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 367 of 539 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T324 claimed 17:53Z, lease until 20:53Z; lane 2: T102 claimed 17:43Z, lease until 20:43Z; lane 3: T105 claimed 17:21Z, lease until 20:21Z; lane 4: T166 claimed 17:24Z, lease until 20:24Z; lane 5: T559 claimed 16:58Z, lease until 20:21Z
- **Week:** 98% used, -2.7M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 17:59Z, 0 min before this update
- **Keeper:** last ran 17:59Z, next wake 20:23Z

## Needs attention
- BLOCKED T111: plan gap reported: PLAN GAP: scripts/check_legal_docs.py:403-410 and :458 contradict TERMS-026 (docs/spec/terms.md:29) and D-3877 (kit/prompts/T111.md:9). The Spanish summary must carry the TERMS-026 paragraph in English word for word, which contains "subcontract"; check_legal_docs.py bans \bsubcontract\w* (BANNED :307, trace_spec.py:69) in every DOCUMENTS file and exempts it only through that file's own REQUIRED phrases (check_phrases :393-412, scan in run() :442-464). The new business-terms-summary-es.md has no REQUIRED entry, and the prompt allows no script line beyond the DOCUMENTS/BUSINESS_DOCUMENTS additio; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T111 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T361: plan gap reported: PLAN GAP: app/intake/src/job_texts.rs:473 (`(1..).zip(page)`) and :494 (`.filter(|(n, _)| others.contains(n))`). kit/tasks.json lists T240, T246, T247, T278, T295, T328, T402, T446 and T471 as owners of this file; T441 owns only portals files; app/quotes/src/notify.rs (list_lines) is owned by T047, T293, T328, T384. Evidence: T361 makes list_rows/list_lines number by shown_number (required by kit/prompts/T361.md:9) while page_quotes must stay in arrival order until T441, so lapsed_body pairs page places with shown-number LIST lines. Counterexample: A AI-read at t0, B by form at t0+5m; B shown ; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T361 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- Strict plan check red: 12 problem(s) on tasks still to run (+123 on DONE tasks); first: T116: stored context_pct 8% but it measures 12%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T303 | server and portals: owner sign-in, sign-out and step-up are  | MERGE_PENDING | 1 | 92 min | 17:52Z |  |
| T361 | quotes and picks: a quote keeps its number once shown, and a | BLOCKED | 1 | 50 min | 17:41Z | PLAN GAP: app/intake/src/job_texts.rs:473 (`(1..).zip(page)`) and :494 (`.filter(\|(n, _)\| others.contains(n))`). kit/tas |
| T324 | harness: 95% branch floors for the money and privacy files o | QUEUED | 1 | 61 min | 17:33Z | Review found the per-file floor list still incomplete after one fix round (head 56aef45f3). All 16 floors added so far a |
| T475 | messaging, quotes, payments and ai: the readers behind the O | MERGE_PENDING | 1 | 114 min | 17:02Z |  |
| T105 | ops: High and critical alerts with their automatic acknowled | resync | 3 | 38 min | 16:53Z | merging main into the reviewed head conflicts in app/ops/src/api.rs; a resync merges it in on this attempt (D-891) |
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
| T495 | e2e: the served Confirm and pay journey in the browser again | done | 1 | 44 min | 18:21Z |  |
| T462 | server: the sign-up inbox check is served behind the busines | done | 3 | 35 min | 18:05Z | PLAN GAP: app/portals/templates/email_check.html:47 owner T149 (done, #354), and app/portals/src/feedback.rs:61 owner T4 |

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 309 of 733 (42.2%) · partial 273 · unstarted 151
**Tasks merged (throughput only):** 367 of 539

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 13 | 17 | 5 | 35 |
| ai | 11 | 13 | 5 | 29 |
| businesses | 20 | 35 | 3 | 58 |
| calls | 14 | 10 | 1 | 25 |
| consent | 15 | 16 | 4 | 35 |
| fees | 21 | 30 | 1 | 52 |
| intake | 29 | 11 | 3 | 43 |
| job-cards | 20 | 4 | 0 | 24 |
| licensing | 38 | 10 | 0 | 48 |
| lifecycle | 24 | 4 | 0 | 28 |
| messaging | 26 | 17 | 2 | 45 |
| ops-recovery | 1 | 13 | 40 | 54 |
| privacy | 13 | 16 | 2 | 31 |
| quiet-hours | 7 | 2 | 0 | 9 |
| quotes-pick | 35 | 20 | 1 | 56 |
| recurring | 8 | 0 | 3 | 11 |
| security | 11 | 39 | 5 | 55 |
| terms | 1 | 2 | 32 | 35 |
| website | 0 | 5 | 23 | 28 |
| wording | 2 | 9 | 21 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-16T17:45:11Z (week)): week: 196.7M used, -2.7M left, resets 2026-11-19T17:00:00Z; projected finish 2026-11-28T04:38:56Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 10 of 147 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T324 (lease until 2026-11-16T20:53:22Z)
- lane 2: T102 (lease until 2026-11-16T20:43:33Z)
- lane 3: T105 (lease until 2026-11-16T20:21:54Z)
- lane 4: T166 (lease until 2026-11-16T20:24:50Z)
- lane 5: T559 (lease until 2026-11-16T20:21:27Z)

## Last usage reading
- {"task": "T303", "at": "2026-11-16T17:52:28Z", "cache_creation_input_tokens": 478958, "input_tokens": 414, "output_tokens": 132498, "cache_read_input_tokens": 24819917}
