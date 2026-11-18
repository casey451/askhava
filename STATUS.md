# Ask Hava build status

Updated 2026-11-18T00:53:38Z by runner 01d25e9d74c2 (actions).

**Building: T478 in progress since 23:24Z; 419 of 582 done; week 36% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 419 of 582 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 2: T478 claimed 23:24Z, lease until 03:11Z; lane 5: T151 claimed 00:12Z, lease until 03:12Z; lane 6: T108 claimed 00:23Z, lease until 03:23Z; lane 7: T604 claimed 00:31Z, lease until 03:31Z
- **Week:** 36% used, 118.7M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 00:53Z, 0 min before this update
- **Keeper:** last ran 00:53Z, next wake 03:13Z

## Needs attention
- BLOCKED T203: plan gap reported: PLAN GAP: `app/portals/templates/quote_page.html:101` (owned by DONE tasks T052, T095, T294, T308, T384, T388, T393, T440 and T441, and by QUEUED T608). WORD-002 (`docs/spec/wording.md:6`) and TD-WORD-05 (`docs/test-design.md:683`) say the long not-a-contractor statement must be "at the top of the quote page". The template has only the first sentence, and only at the bottom (line 101 of 103); it extends `app/site/templates/base.html`, which has no contractor line. T203 owns only tests/docs/test_wording_statements.py and tests/docs/test_introduction_fee_wording.py and may not write copy (kit/pr; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T203 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- Strict plan check red: 21 problem(s) on tasks still to run (+189 on DONE tasks); first: T402: stored context_pct 40% but it measures 45%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T587 | payments and picks: the old Confirm fee writer is marked tes | MERGE_PENDING | 1 | 48 min | 00:26Z |  |
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
| T202 | site: Spanish pages, the business page blocks and the requir | resync | 2 | 56 min | 11:26Z |  |
| T112 | Draft the privacy policy and SMS terms | done | 3 | 66 min | 11:10Z | Fresh-eyes review of the fix-round head 037ad56a05f50b76c38506da3786bdde73a2be4d ended REVIEW-VERDICT: FINDINGS after th |
| T489 | site and server: the web chat and request pages with the con | done | 3 | 46 min | 11:02Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Date:      2026- |
| T204 | ops: phone pushes by alert email, OPS-023 acknowledgements,  | check_failed | 1 | 35 min | 10:33Z |  |
| T489 | site and server: the web chat and request pages with the con | check_failed | 2 | 73 min | 09:54Z |  |
| T202 | site: Spanish pages, the business page blocks and the requir | check_failed | 1 | 88 min | 09:23Z |  |
| T107 | Consent records kept 5 years, no export, opt-out rate alerts | done | 2 | 85 min | 09:00Z | Second fresh-eyes review of head dfb993caba5ddab51d93dc1f24c5403dc513e3e7 ended REVIEW-VERDICT: FINDINGS (the one findin |
| T112 | Draft the privacy policy and SMS terms | QUEUED | 2 | 52 min | 08:28Z |  |
| T570 | intake: a CLOSE text, a "Close which?" answer and a cancel-i | done | 1 | 39 min | 08:26Z |  |
| T112 | Draft the privacy policy and SMS terms | QUEUED | 1 | 62 min | 07:32Z |  |
| T201 | site: services and help pages, the ROC complaint page | done | 1 | 56 min | 07:22Z |  |
| T489 | site and server: the web chat and request pages with the con | BLOCKED | 1 | 12 min | 07:15Z |  |
| T564 | quotes, picks and intake: the LIST, text picks, lapse text a | done | 1 | 43 min | 06:47Z |  |
| T204 | ops: phone pushes by alert email, OPS-023 acknowledgements,  | resync | 1 | 2 h 21 min | 06:44Z |  |
| T586 | calls and privacy: TwiML text is escaped, a full number pool | done | 1 | 88 min | 06:17Z |  |
| T481 | messaging and payments: mark a replayed text sent from the c | done | 1 | 114 min | 05:36Z |  |
| T103 | intake: the web-chat conversation, where typing a verified m | done | 1 | 106 min | 05:27Z |  |
| T585 | businesses and licensing: a license change that drops job ty | done | 1 | 43 min | 05:10Z |  |
| T571 | server and calls: a press of 9 on the safety call is never l | done | 1 | 45 min | 05:08Z |  |
| T221 | site: the terms, privacy policy, SMS terms, homeowner data a | done | 2 | 29 min | 04:41Z | merging main into the reviewed head conflicts in docs/architecture.md; a resync merges it in on this attempt (D-891) |
| T591 | Runner (harness): free the backup job's disk and cap the smo | done | 1 | 67 min | 04:23Z |  |
| T107 | Consent records kept 5 years, no export, opt-out rate alerts | QUEUED | 1 | 85 min | 03:45Z |  |
| T584 | businesses (harness): sign-up activation retried after a con | done | 2 | 43 min | 03:23Z | PLAN GAP: tests/integration/tests/business_event_emails.rs:318 (owned by T329, not T584) expects `emergency_test` to rea |
| T221 | site: the terms, privacy policy, SMS terms, homeowner data a | resync | 2 | 79 min | 03:14Z |  |
| T584 | businesses (harness): sign-up activation retried after a con | BLOCKED | 1 | 3 min | 02:26Z |  |
| T469 | server (harness): the Resend email client from settings and  | done | 1 | 99 min | 02:12Z |  |
| T221 | site: the terms, privacy policy, SMS terms, homeowner data a | check_failed | 1 | 2 h 47 min | 01:23Z |  |
| T320 | ops: a check that runs on a schedule raises its alert once w | done | 2 | 78 min | 01:13Z | Ci: failure; E2e: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / {"timestamp":"2026-11-16T |
| T373 | businesses: changing a phone's number ends that phone's text | done | 2 | 42 min | 01:04Z | Ci: failure; E2e: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / Finished `dev` profile [u |

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 0 of 735 (0.0%) · partial 677 · unstarted 58
**Tasks merged (throughput only):** 419 of 582

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 0 | 35 | 0 | 35 |
| ai | 0 | 26 | 3 | 29 |
| businesses | 0 | 56 | 2 | 58 |
| calls | 0 | 24 | 1 | 25 |
| consent | 0 | 34 | 1 | 35 |
| fees | 0 | 51 | 1 | 52 |
| intake | 0 | 43 | 0 | 43 |
| job-cards | 0 | 24 | 0 | 24 |
| licensing | 0 | 48 | 0 | 48 |
| lifecycle | 0 | 28 | 0 | 28 |
| messaging | 0 | 44 | 1 | 45 |
| ops-recovery | 0 | 31 | 23 | 54 |
| privacy | 0 | 29 | 2 | 31 |
| quiet-hours | 0 | 9 | 0 | 9 |
| quotes-pick | 0 | 56 | 0 | 56 |
| recurring | 0 | 9 | 2 | 11 |
| security | 0 | 50 | 5 | 55 |
| terms | 0 | 32 | 3 | 35 |
| website | 0 | 25 | 5 | 30 |
| wording | 0 | 23 | 9 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-18T00:26:42Z (week)): week: 75.3M used, 118.7M left, resets 2026-11-19T17:00:00Z; projected finish 2026-11-29T17:05:06Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 62 of 190 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 2: T478 (lease until 2026-11-18T03:11:24Z)
- lane 5: T151 (lease until 2026-11-18T03:12:48Z)
- lane 6: T108 (lease until 2026-11-18T03:23:10Z)
- lane 7: T604 (lease until 2026-11-18T03:31:21Z)

## Last usage reading
- {"task": "T587", "at": "2026-11-18T00:26:07Z", "cache_creation_input_tokens": 121464, "cache_read_input_tokens": 2977344, "input_tokens": 102, "output_tokens": 31179}
