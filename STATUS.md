# Ask Hava build status

Updated 2026-11-18T11:27:23Z by runner 3103c90462d8 (actions).

**Building: T583 in progress since 11:19Z; 440 of 621 done; week 53% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 440 of 621 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T583 claimed 11:19Z, lease until 14:19Z; lane 2: T662 claimed 11:26Z, lease until 14:26Z; lane 5: T408 claimed 09:13Z, lease until 13:23Z; lane 6: T200 claimed 09:05Z, lease until 13:36Z
- **Week:** 53% used, 83.7M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 11:27Z, 0 min before this update
- **Keeper:** last ran 11:27Z, next wake 13:26Z

## Needs attention
- BLOCKED T563: plan gap reported: PLAN GAP: app/messaging/src/optout.rs:311 (suppress/record, owned by T029 and later tasks; T563 owns no messaging file) and kit/prompts/T563.md:9 (CROSS_SCHEMA_CALLS fixed at exactly one entry, messaging.last_message_at(). The sweep (head d7eb5dc64, all TD-CON-30 tests green, smoke and prehandin green) checks suppression via messaging::api::send_window before its per-number transaction, then clears the vault phone inside it. Review finding (unresolved): an opt-out via messaging::api::suppress (portal/email/owner paths) or a texted STOP committing between that check and the clear lets the sweep; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T563 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- Strict plan check red: 26 problem(s) on tasks still to run (+225 on DONE tasks); first: T402: measures 47% of the window, over its 45% limit; trim its reads or split it

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T563 | intake and privacy: the daily consent sweep removes consent  | BLOCKED | 1 | 85 min | 11:21Z | PLAN GAP: app/messaging/src/optout.rs:311 (suppress/record, owned by T029 and later tasks; T563 owns no messaging file)  |
| T658 | privacy: the scrubber removes lower-case Spanish addresses a | MERGE_PENDING | 2 | 51 min | 11:04Z | Fix-round review of head 18e4fd102f38620e11d1055ecb4d77813e2b2965 returned FINDINGS after the one allowed fix round (fir |
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
| T597 | Runner (harness): the early merge-close step also pushes the | done | 1 | 26 min | 20:28Z |  |
| T582 | quotes and messaging (harness): a late read-back confirm is  | done | 1 | 36 min | 20:13Z |  |
| T115 | docs/a11y.md (plan, statement, VoiceOver script), statement  | BLOCKED | 2 | 68 min | 19:54Z |  |
| T104 | Wording scan: brand, banned words and the not-a-contractor p | BLOCKED | 3 | 46 min | 19:24Z |  |
| T605 | businesses, intake and picks: the leaving answers, not-cover | done | 1 | 46 min | 19:15Z |  |
| T528 | ops: the alert, audit-row and digest-row readers and writers | done | 2 | 37 min | 19:05Z | PLAN GAP: tests/integration/tests/business_leaving.rs:463 (owned by T228, also listed by T343) asserts that businesses/s |
| T115 | docs/a11y.md (plan, statement, VoiceOver script), statement  | QUEUED | 1 | 58 min | 18:25Z |  |
| T596 | Runner (harness): an ancestor's running smoke finishes and c | done | 1 | 41 min | 17:49Z |  |
| T491 | picks: the Confirm window's end, every way it comes, runs th | done | 1 | 2 h 2 min | 17:44Z |  |
| T599 | Tests (harness): the PICK-046 email's no-amount check ignore | done | 1 | 32 min | 17:36Z |  |
| T104 | Wording scan: brand, banned words and the not-a-contractor p | QUEUED | 2 | 42 min | 17:35Z |  |
| T104 | Wording scan: brand, banned words and the not-a-contractor p | QUEUED | 1 | 51 min | 16:52Z |  |
| T528 | ops: the alert, audit-row and digest-row readers and writers | BLOCKED | 1 | 24 min | 16:50Z |  |
| T595 | server and site: a same-site web chat POST passes its CSRF c | done | 1 | 45 min | 16:19Z |  |
| T479 | ops and portals: "Not a fit for us" on the job page, and the | done | 2 | 47 min | 16:17Z | Ci: failure; Checks: failure / Checks failed at step: Rust build (the one boundary build) and tests (all but SPLIT_TESTS |
| T113 | Draft the Twilio campaign description from the real template | done | 2 | 35 min | 15:39Z | PLAN GAP: tests/docs/test_terms_clauses.py:494 is T111's test (DONE, kit/tasks.json:18231) and asserts `report.checked = |
| T471 | picks and intake: her "backup has your pick" text on every p | done | 1 | 47 min | 14:20Z |  |
| T321 | messaging: the consent-purge functions, the inbound-text clo | done | 1 | 49 min | 13:42Z |  |
| T204 | ops: phone pushes by alert email, OPS-023 acknowledgements,  | done | 2 | 94 min | 13:39Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Version:   3.20. |
| T363 | portals: the served pick and its code step apply the per-IP  | done | 1 | 62 min | 13:38Z |  |
| T479 | ops and portals: "Not a fit for us" on the job page, and the | check_failed | 1 | 50 min | 13:19Z |  |
| T594 | site: the end-to-end spec of the web chat and request page a | BLOCKED | 1 | 42 min | 13:18Z |  |
| T202 | site: Spanish pages, the business page blocks and the requir | done | 2 | 43 min | 13:01Z | merging main into the reviewed head conflicts in app/site/src/api.rs, docs/architecture.md, tests/a11y/page_states.ts; a |
| T113 | Draft the Twilio campaign description from the real template | BLOCKED | 1 | 10 min | 12:21Z |  |
| T441 | portals: the quote page shows the kept number, and the pick  | done | 1 | 74 min | 11:32Z |  |
| T572 | intake: the under-13 stop also clears the vault contact's na | done | 1 | 48 min | 11:31Z |  |
| T446 | intake and messaging: PICK-046's "couldn't take it" notice g | done | 1 | 87 min | 11:29Z |  |

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 0 of 735 (0.0%) · partial 683 · unstarted 52
**Tasks merged (throughput only):** 440 of 621

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 0 | 35 | 0 | 35 |
| ai | 0 | 26 | 3 | 29 |
| businesses | 0 | 56 | 2 | 58 |
| calls | 0 | 25 | 0 | 25 |
| consent | 0 | 35 | 0 | 35 |
| fees | 0 | 51 | 1 | 52 |
| intake | 0 | 43 | 0 | 43 |
| job-cards | 0 | 24 | 0 | 24 |
| licensing | 0 | 48 | 0 | 48 |
| lifecycle | 0 | 28 | 0 | 28 |
| messaging | 0 | 44 | 1 | 45 |
| ops-recovery | 0 | 34 | 20 | 54 |
| privacy | 0 | 30 | 1 | 31 |
| quiet-hours | 0 | 9 | 0 | 9 |
| quotes-pick | 0 | 56 | 0 | 56 |
| recurring | 0 | 9 | 2 | 11 |
| security | 0 | 50 | 5 | 55 |
| terms | 0 | 32 | 3 | 35 |
| website | 0 | 25 | 5 | 30 |
| wording | 0 | 23 | 9 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-18T11:22:33Z (week)): week: 110.3M used, 83.7M left, resets 2026-11-19T17:00:00Z; projected finish 2026-12-01T05:06:26Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 83 of 228 done
- stage 6: 0 of 40 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T583 (lease until 2026-11-18T14:19:34Z)
- lane 2: T662 (lease until 2026-11-18T14:26:44Z)
- lane 5: T408 (lease until 2026-11-18T13:23:31Z)
- lane 6: T200 (lease until 2026-11-18T13:36:41Z)

## Last usage reading
- {"task": "T563", "at": "2026-11-18T11:21:55Z", "cache_creation_input_tokens": 469777, "output_tokens": 111668, "input_tokens": 341, "cache_read_input_tokens": 14421572}
