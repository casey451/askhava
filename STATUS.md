# Ask Hava build status

Updated 2026-11-17T18:08:47Z by runner 575b42f0c7a4 (actions).

**Building: T115 in progress since 17:27Z; 403 of 570 done; week 25% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 403 of 570 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 7: T115 claimed 17:27Z, lease until 21:08Z
- **Week:** 25% used, 142.0M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 17:57Z, 11 min before this update
- **Keeper:** last ran 17:57Z, next wake 20:30Z

## Needs attention
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T488 (since 2026-11-17T15:29:29Z, no attempt used; a plan change fixes it): T488: dependency T202 is DONE but its owned file app/site/templates/es_home.html is not on main; T488: dependency T202 is DONE but its owned file tests/docs/test_required_lines.py is not on main; T488: dependency T202 is DONE but its owned file app/site/templates/business.html is not on main; T488: dependency T202 is DONE but its owned file app/site/templates/es_business.html is not on main; T488: dependency T202 is DONE but its owned file app/site/templates/business_help.html is not on main
- Strict plan check red: 11 problem(s) on tasks still to run (+166 on DONE tasks); first: T402: stored context_pct 40% but it measures 44%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T596 | Runner (harness): an ancestor's running smoke finishes and c | MERGE_PENDING | 1 | 41 min | 17:49Z |  |
| T491 | picks: the Confirm window's end, every way it comes, runs th | MERGE_PENDING | 1 | 2 h 2 min | 17:44Z |  |
| T599 | Tests (harness): the PICK-046 email's no-amount check ignore | MERGE_PENDING | 1 | 32 min | 17:36Z |  |
| T104 | Wording scan: brand, banned words and the not-a-contractor p | QUEUED | 2 | 42 min | 17:35Z | Review not approved after the one fix round (head 01543c2e001f3583c873583dd24865826cfa3ae5; tests/docs/test_wording_scan |
| T104 | Wording scan: brand, banned words and the not-a-contractor p | QUEUED | 1 | 51 min | 16:52Z |  |
| T528 | ops: the alert, audit-row and digest-row readers and writers | BLOCKED | 1 | 24 min | 16:50Z | PLAN GAP: tests/integration/tests/business_leaving.rs:463 (owned by T228, also listed by T343) asserts that businesses/s |
| T595 | server and site: a same-site web chat POST passes its CSRF c | MERGE_PENDING | 1 | 45 min | 16:19Z |  |
| T479 | ops and portals: "Not a fit for us" on the job page, and the | done | 2 | 47 min | 16:17Z | Ci: failure; Checks: failure / Checks failed at step: Rust build (the one boundary build) and tests (all but SPLIT_TESTS |
| T113 | Draft the Twilio campaign description from the real template | MERGE_PENDING | 2 | 35 min | 15:39Z | PLAN GAP: tests/docs/test_terms_clauses.py:494 is T111's test (DONE, kit/tasks.json:18231) and asserts `report.checked = |
| T471 | picks and intake: her "backup has your pick" text on every p | done | 1 | 47 min | 14:20Z |  |
| T321 | messaging: the consent-purge functions, the inbound-text clo | done | 1 | 49 min | 13:42Z |  |
| T204 | ops: phone pushes by alert email, OPS-023 acknowledgements,  | done | 2 | 94 min | 13:39Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Version:   3.20. |
| T363 | portals: the served pick and its code step apply the per-IP  | done | 1 | 62 min | 13:38Z |  |
| T479 | ops and portals: "Not a fit for us" on the job page, and the | check_failed | 1 | 50 min | 13:19Z |  |
| T594 | site: the end-to-end spec of the web chat and request page a | BLOCKED | 1 | 42 min | 13:18Z | PLAN GAP: app/server/src/chat_codes.rs:187 and app/site/templates/request.html, both owned by T489. Smallest plan change |
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

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 336 of 733 (45.8%) · partial 323 · unstarted 74
**Tasks merged (throughput only):** 403 of 570

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 19 | 13 | 3 | 35 |
| ai | 12 | 14 | 3 | 29 |
| businesses | 23 | 33 | 2 | 58 |
| calls | 14 | 10 | 1 | 25 |
| consent | 15 | 19 | 1 | 35 |
| fees | 24 | 27 | 1 | 52 |
| intake | 26 | 17 | 0 | 43 |
| job-cards | 18 | 6 | 0 | 24 |
| licensing | 40 | 8 | 0 | 48 |
| lifecycle | 27 | 1 | 0 | 28 |
| messaging | 24 | 19 | 2 | 45 |
| ops-recovery | 1 | 27 | 26 | 54 |
| privacy | 14 | 15 | 2 | 31 |
| quiet-hours | 6 | 3 | 0 | 9 |
| quotes-pick | 37 | 19 | 0 | 56 |
| recurring | 8 | 1 | 2 | 11 |
| security | 12 | 38 | 5 | 55 |
| terms | 12 | 17 | 6 | 35 |
| website | 1 | 23 | 4 | 28 |
| wording | 3 | 13 | 16 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-17T16:52:53Z (week)): week: 52.0M used, 142.0M left, resets 2026-11-19T17:00:00Z; projected finish 2026-11-29T08:40:45Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 46 of 178 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 7: T115 (lease until 2026-11-17T21:08:47Z)

## Last usage reading
- {"task": "T596", "at": "2026-11-17T17:49:19Z", "output_tokens": 47905, "input_tokens": 88, "cache_read_input_tokens": 2836773, "cache_creation_input_tokens": 160807}
