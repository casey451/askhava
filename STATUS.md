# Ask Hava build status

Updated 2026-11-19T03:01:37Z by runner d50fd4f98c8d (actions).

**Building: T715 in progress since 01:59Z; 471 of 663 done; week 72% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 471 of 663 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T715 claimed 01:59Z, lease until 04:59Z; lane 2: T520 claimed 01:57Z, lease until 06:00Z; lane 3: T114 claimed 02:14Z, lease until 05:45Z; lane 4: T484 claimed 02:04Z, lease until 06:01Z; lane 5: T563 claimed 02:48Z, lease until 05:48Z; lane 6: T490 claimed 02:10Z, lease until 05:10Z; lane 7: T203 claimed 02:39Z, lease until 05:39Z
- **Week:** 72% used, 41.7M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 03:00Z, 1 min before this update
- **Keeper:** last ran 02:34Z, next wake 02:54Z

## Needs attention
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- Strict plan check red: 59 problem(s) on tasks still to run (+279 on DONE tasks); first: T397: stored context_pct 13% but it measures 17%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T306 | server and portals: the complaints page, the emergency-email | done | 1 | 38 min | 02:21Z |  |
| T203 | Wording scan: the required statements, business copy lines a | check_failed | 2 | 41 min | 01:40Z | Ci: failure; Python: failure / Python failed at step: Unit, contract and drill tests (pytest) / E         +     'termita |
| T563 | intake and privacy: the daily consent sweep removes consent  | check_failed | 2 | 99 min | 01:37Z | Ci: failure; Coverage: failure / Coverage failed at step: Compare with tests/coverage_floor.json and refuse a lowered fl |
| T505 | Decisions digest, piece 6: the next entries after the header | done | 1 | 99 min | 01:27Z |  |
| T488 | portals and server: the business sign-up served in Spanish o | done | 1 | 111 min | 01:20Z |  |
| T306 | server and portals: the complaints page, the emergency-email | resync | 1 | 77 min | 00:59Z |  |
| T205 | portals: owner refunds with a review step and the audit log  | done | 2 | 2 h 26 min | 00:48Z | PLAN GAP: `payments` has no public reader that ties a dispute to its pick (or its charge, business or job). Without one, |
| T708 | intake: a quote-count update due in quiet hours goes at the  | done | 1 | 50 min | 00:40Z |  |
| T364 | server and portals: the business step-up code page is served | done | 2 | 70 min | 00:32Z | merging main into the reviewed head conflicts in app/server/src/main.rs; a resync merges it in on this attempt (D-891) |
| T701 | messaging: a per-number opt-out lock, and the listing key an | done | 1 | 45 min | 23:02Z |  |
| T583 | intake and server: the quote-count update text is sent at mo | done | 2 | 55 min | 22:50Z | PLAN GAP: kit/prompts/T583.md:51 (budget of 450 changed lines in 3 files) The smallest plan change: raise T583's budget  |
| T504 | Decisions digest, piece 5: the next entries after the header | done | 1 | 39 min | 22:47Z |  |
| T364 | server and portals: the business step-up code page is served | resync | 2 | 52 min | 22:05Z |  |
| T651 | server and portals: owner step-up try limit, the passkey pat | done | 1 | 2 h 7 min | 21:52Z |  |
| T722 | payments: read a pick's disputes and record Casey's evidence | done | 1 | 45 min | 21:33Z |  |
| T503 | Decisions digest, piece 4: the next entries after the header | done | 1 | 31 min | 21:30Z |  |
| T483 | messaging, portals and server: the owner's "away until" date | done | 1 | 2 h 48 min | 21:13Z |  |
| T702 | privacy and messaging: no blind-index key version is retired | done | 1 | 40 min | 21:07Z |  |
| T706 | payments: held_release, the FEE-017 re-check that clears a p | done | 1 | 100 min | 20:54Z |  |
| T715 | intake (harness): corrections to T671's walker: alarm answer | BLOCKED | 1 | 48 min | 20:53Z | PLAN GAP: `app/server/tests/name_address_answer.rs:262`–`:264` (owned by T432, not this task) expects the name-and-addre |
| T502 | Decisions digest, piece 3: the next entries after the header | done | 1 | 38 min | 20:31Z |  |
| T364 | server and portals: the business step-up code page is served | check_failed | 1 | 2 h 1 min | 20:29Z |  |
| T702 | privacy and messaging: no blind-index key version is retired | resync | 1 | 59 min | 20:21Z |  |
| T501 | Decisions digest, piece 2: the next entries after the header | done | 1 | 73 min | 19:42Z |  |
| T711 | intake (harness): the dispatcher's two jobs in one request:  | done | 1 | 77 min | 19:29Z |  |
| T624 | server: two failed in-process health checks in a row raise o | check_failed | 1 | 96 min | 18:26Z | Ci: failure; Python: failure / Python failed at step: Unit, contract and drill tests (pytest) / T705: about 21877 tokens |
| T205 | portals: owner refunds with a review step and the audit log  | BLOCKED | 1 | 58 min | 18:23Z |  |
| T500 | Decisions digest, piece 1: from the first entry, at most 300 | done | 1 | 71 min | 18:18Z |  |
| T671 | intake (harness): a homeowner's texts after the consent text | done | 2 | 102 min | 17:25Z | PLAN GAP: three tests outside T671's scope fail because of the new behaviour (worker edited none of them): / 1. tests/in |
| T621 | portals: the scorecard's go and stop line settings and its p | done | 1 | 38 min | 17:03Z |  |
| T720 | Decision digest prerequisites (harness): the digest plan and | done | 1 | 67 min | 16:51Z |  |
| T624 | server: two failed in-process health checks in a row raise o | resync | 1 | 49 min | 16:38Z |  |
| T592 | jobs, quotes and site: the home page's numbers read every jo | done | 1 | 40 min | 16:19Z |  |
| T645 | intake and messaging: the web chat honours an opt-out word,  | BLOCKED | 1 | 4 min | 16:05Z | PLAN GAP: app/site/src/chat.rs:340 (owned by T593, then T646 and T649; T593 is not merged yet) calls `chat_message(db, & |
| T621 | portals: the scorecard's go and stop line settings and its p | resync | 1 | 55 min | 15:55Z |  |
| T665 | server tests (harness): the door matrix fills a business job | done | 1 | 81 min | 15:47Z |  |
| T525 | server and ops: the hourly vault-read check and the daily da | done | 1 | 102 min | 15:41Z |  |
| T200 | ops and portals: the owner help page with its screened reply | done | 2 | 2 h 25 min | 14:32Z | Review not approved after the one fix round (head 346acc872b66e2a8cd86b345b919fe4573f7db87). / Remaining finding (review |
| T671 | intake (harness): a homeowner's texts after the consent text | BLOCKED | 1 | 44 min | 13:58Z |  |
| T472 | server (harness): serve the "Can't take it" page and button  | done | 1 | 62 min | 12:45Z |  |
| T662 | jobs, intake, calls and server: a masked call line closes wh | done | 1 | 73 min | 12:39Z |  |
| T679 | quotes and businesses: a launch-open category takes requests | done | 1 | 37 min | 12:17Z |  |
| T583 | intake and server: the quote-count update text is sent at mo | BLOCKED | 1 | 53 min | 12:13Z |  |
| T700 | Runner (harness): a merge-line task whose turn ran out goes  | done | 1 | 29 min | 12:08Z |  |
| T200 | ops and portals: the owner help page with its screened reply | QUEUED | 1 | 2 h 57 min | 12:02Z |  |
| T408 | server: a release waits while safe mode is on and goes ahead | BLOCKED | 2 | 2 h 21 min | 11:34Z | PLAN GAP: kit/prompts/T408.md:54 "Size budget: at most 6 files, 800 changed lines" cannot be met. The prompt's own plan  |
| T563 | intake and privacy: the daily consent sweep removes consent  | BLOCKED | 1 | 85 min | 11:21Z |  |
| T658 | privacy: the scrubber removes lower-case Spanish addresses a | done | 2 | 51 min | 11:04Z | Fix-round review of head 18e4fd102f38620e11d1055ecb4d77813e2b2965 returned FINDINGS after the one allowed fix round (fir |
| T650 | intake: the under-13 stop also deletes the request's job pho | done | 1 | 90 min | 10:39Z |  |
| T641 | Runner (harness): merge train depth 2, the next branch's upd | done | 1 | 55 min | 10:29Z |  |
| T644 | tests (harness): the remaining Playwright specs and helpers  | done | 1 | 43 min | 09:48Z |  |
| T660 | quotes (harness): the quote-note AI screener takes scrubbed  | done | 1 | 56 min | 08:52Z |  |
| T658 | privacy: the scrubber removes lower-case Spanish addresses a | QUEUED | 1 | 89 min | 08:11Z |  |
| T640 | harness: Ci handles the merge train's throwaway refs and cou | done | 1 | 64 min | 08:02Z |  |
| T619 | quotes, picks and intake: the quote-median, lapsed-pick, win | done | 2 | 40 min | 08:02Z | T619 readers are correct per three Opus reviews, but the test file still misses wrong implementations; no reviewer appro |
| T482 | messaging and intake: the plain-word auto-answers after HELP | done | 1 | 2 h 29 min | 07:56Z |  |
| T643 | portals (harness): a real browser's same-origin form POST pa | done | 1 | 2 h 24 min | 07:49Z |  |
| T642 | Runner (harness): the keeper starts a run for each free lane | done | 1 | 83 min | 06:05Z |  |
| T601 | businesses and quotes: the card pool reads every service are | done | 1 | 50 min | 05:14Z |  |
| T523 | jobs, ops and intake: the check-in answer recorder and the c | done | 3 | 46 min | 05:06Z | Review not approved after the one findings round (head bd99856fca96fb81973c28181ad9fd6e39e7ac91; flag_inputs tests 5 pas |
| T638 | harness: Ci skips a compile its job already passed on a prov | done | 1 | 88 min | 04:35Z |  |
| T623 | payments: a Stripe key refusal (401 or 403) takes the Stripe | done | 1 | 63 min | 03:46Z |  |
| T151 | ops and messaging: HELP requests with one owner notification | done | 1 | 34 min | 03:40Z |  |
| T647 | payments: rebuild_charge reports Booked inside a caller's tr | done | 1 | 82 min | 03:35Z |  |
| T619 | quotes, picks and intake: the quote-median, lapsed-pick, win | QUEUED | 1 | 69 min | 03:31Z |  |
| T639 | Runner (harness): the smoke warm-up is skipped when the pre- | done | 1 | 76 min | 03:14Z |  |
| T108 | portals: owner tools for pauses, fee mode, categories and se | done | 1 | 2 h 45 min | 03:08Z |  |

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 294 of 735 (40.0%) · partial 391 · unstarted 50
**Tasks merged (throughput only):** 471 of 663

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 17 | 18 | 0 | 35 |
| ai | 9 | 17 | 3 | 29 |
| businesses | 17 | 40 | 1 | 58 |
| calls | 12 | 13 | 0 | 25 |
| consent | 14 | 21 | 0 | 35 |
| fees | 23 | 28 | 1 | 52 |
| intake | 10 | 33 | 0 | 43 |
| job-cards | 14 | 10 | 0 | 24 |
| licensing | 33 | 15 | 0 | 48 |
| lifecycle | 18 | 10 | 0 | 28 |
| messaging | 20 | 24 | 1 | 45 |
| ops-recovery | 3 | 32 | 19 | 54 |
| privacy | 9 | 21 | 1 | 31 |
| quiet-hours | 6 | 3 | 0 | 9 |
| quotes-pick | 27 | 29 | 0 | 56 |
| recurring | 7 | 2 | 2 | 11 |
| security | 9 | 41 | 5 | 55 |
| terms | 25 | 7 | 3 | 35 |
| website | 9 | 16 | 5 | 30 |
| wording | 12 | 11 | 9 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-19T02:34:03Z (week)): week: 152.3M used, 41.7M left, resets 2026-11-19T17:00:00Z; projected finish 2026-12-03T04:39:17Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 114 of 262 done
- stage 6: 0 of 48 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T715 (lease until 2026-11-19T04:59:03Z)
- lane 2: T520 (lease until 2026-11-19T06:00:16Z)
- lane 3: T114 (lease until 2026-11-19T05:45:12Z)
- lane 4: T484 (lease until 2026-11-19T06:01:37Z)
- lane 5: T563 (lease until 2026-11-19T05:48:39Z)
- lane 6: T490 (lease until 2026-11-19T05:10:22Z)
- lane 7: T203 (lease until 2026-11-19T05:39:05Z)

## Last usage reading
- {"task": "T306", "at": "2026-11-19T02:21:39Z", "cache_creation_input_tokens": 64200, "cache_read_input_tokens": 980197, "output_tokens": 10392, "input_tokens": 65}
