# Ask Hava build status

Updated 2026-11-18T20:31:41Z by runner 17bc804178af (actions).

**Building: T651 in progress since 19:44Z; 455 of 663 done; week 67% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 455 of 663 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T651 claimed 19:44Z, lease until 22:44Z; lane 2: T502 claimed 19:53Z, lease until 22:53Z; lane 3: T483 claimed 18:25Z, lease until 22:51Z; lane 5: T706 claimed 19:14Z, lease until 23:19Z; lane 6: T702 claimed 20:27Z, lease until 23:27Z; lane 7: T715 claimed 20:05Z, lease until 23:05Z
- **Week:** 67% used, 53.0M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 20:31Z, 0 min before this update
- **Keeper:** last ran 20:31Z, next wake 22:47Z

## Needs attention
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- Strict plan check red: 50 problem(s) on tasks still to run (+262 on DONE tasks); first: T488: stored context_pct 29% but it measures 33%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T364 | server and portals: the business step-up code page is served | MERGE_PENDING | 1 | 2 h 1 min | 20:29Z |  |
| T702 | privacy and messaging: no blind-index key version is retired | resync | 1 | 59 min | 20:21Z | merging main into the reviewed head conflicts in kit/prompts/T702.md; a resync merges it in on this attempt (D-891) |
| T501 | Decisions digest, piece 2: the next entries after the header | done | 1 | 73 min | 19:42Z |  |
| T711 | intake (harness): the dispatcher's two jobs in one request:  | done | 1 | 77 min | 19:29Z |  |
| T624 | server: two failed in-process health checks in a row raise o | check_failed | 1 | 96 min | 18:26Z | Ci: failure; Python: failure / Python failed at step: Unit, contract and drill tests (pytest) / T705: about 21877 tokens |
| T205 | portals: owner refunds with a review step and the audit log  | BLOCKED | 1 | 58 min | 18:23Z | PLAN GAP: `payments` has no public reader that ties a dispute to its pick (or its charge, business or job). Without one, |
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
| T583 | intake and server: the quote-count update text is sent at mo | BLOCKED | 1 | 53 min | 12:13Z | PLAN GAP: kit/prompts/T583.md:51 (budget of 450 changed lines in 3 files) The smallest plan change: raise T583's budget  |
| T700 | Runner (harness): a merge-line task whose turn ran out goes  | done | 1 | 29 min | 12:08Z |  |
| T200 | ops and portals: the owner help page with its screened reply | QUEUED | 1 | 2 h 57 min | 12:02Z |  |
| T408 | server: a release waits while safe mode is on and goes ahead | BLOCKED | 2 | 2 h 21 min | 11:34Z | PLAN GAP: kit/prompts/T408.md:54 "Size budget: at most 6 files, 800 changed lines" cannot be met. The prompt's own plan  |
| T563 | intake and privacy: the daily consent sweep removes consent  | BLOCKED | 1 | 85 min | 11:21Z | PLAN GAP: app/messaging/src/optout.rs:311 (suppress/record, owned by T029 and later tasks; T563 owns no messaging file)  |
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
| T151 | ops and messaging: HELP requests with one owner notification | resync | 1 | 2 h 14 min | 02:26Z |  |
| T618 | Harness: a 4-hourly advisory watch on main that alerts Casey | done | 1 | 28 min | 02:22Z |  |
| T604 | Dependencies: sentry without default features, so native-tls | done | 1 | 78 min | 01:49Z |  |
| T478 | jobs, intake, calls, messaging and businesses: the scorecard | done | 1 | 2 h 23 min | 01:47Z |  |
| T587 | payments and picks: the old Confirm fee writer is marked tes | done | 1 | 48 min | 00:26Z |  |
| T606 | harness: scripts/check_copy.py allows one aria-hidden arrow  | done | 1 | 62 min | 23:23Z |  |
| T203 | Wording scan: the required statements, business copy lines a | BLOCKED | 1 | 3 min | 23:08Z | PLAN GAP: `app/portals/templates/quote_page.html:101` (owned by DONE tasks T052, T095, T294, T308, T384, T388, T393, T44 |
| T115 | docs/a11y.md (plan, statement, VoiceOver script), statement  | done | 3 | 37 min | 22:46Z | PLAN GAP: `package-lock.json` (unchanged from main) fails `npm audit --audit-level=low` on GHSA-fc5q-39xc-vjpf; nothing  |
| T603 | Perf and twin (harness): the web vitals report records LCP a | done | 1 | 40 min | 22:05Z |  |
| T104 | Wording scan: brand, banned words and the not-a-contractor p | done | 4 | 44 min | 21:44Z | Review not approved after the one fix round (head 01543c2e001f3583c873583dd24865826cfa3ae5; tests/docs/test_wording_scan |
| T594 | site: the end-to-end spec of the web chat and request page a | done | 2 | 83 min | 21:03Z | PLAN GAP: app/server/src/chat_codes.rs:187 and app/site/templates/request.html, both owned by T489. Smallest plan change |
| T106 | ops: /healthz and the web and worker liveness exits | done | 1 | 77 min | 20:38Z |  |
| T602 | privacy: the inbound email signature check refuses an out-of | done | 1 | 30 min | 20:37Z |  |

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 293 of 735 (39.9%) · partial 392 · unstarted 50
**Tasks merged (throughput only):** 455 of 662

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
| security | 8 | 42 | 5 | 55 |
| terms | 25 | 7 | 3 | 35 |
| website | 9 | 16 | 5 | 30 |
| wording | 12 | 11 | 9 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-18T20:22:36Z (week)): week: 141.0M used, 53.0M left, resets 2026-11-19T17:00:00Z; projected finish 2026-12-03T04:19:18Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 98 of 262 done
- stage 6: 0 of 48 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T651 (lease until 2026-11-18T22:44:55Z)
- lane 2: T502 (lease until 2026-11-18T22:53:32Z)
- lane 3: T483 (lease until 2026-11-18T22:51:15Z)
- lane 5: T706 (lease until 2026-11-18T23:19:53Z)
- lane 6: T702 (lease until 2026-11-18T23:27:42Z)
- lane 7: T715 (lease until 2026-11-18T23:05:10Z)

## Last usage reading
- {"task": "T364", "at": "2026-11-18T20:29:26Z", "input_tokens": 214, "output_tokens": 73457, "cache_read_input_tokens": 12948727, "cache_creation_input_tokens": 248170}
