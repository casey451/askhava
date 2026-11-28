# Ask Hava build status

Updated 2026-11-28T03:38:08Z by runner 820c69ead2c2 (actions).

**Building: T849 in progress since 03:36Z; 696 of 813 done; week 22% used, resets Thu 2026-12-03 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 696 of 813 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T849 claimed 03:36Z, lease until 06:36Z
- **Week:** 22% used, 150.0M tokens left; resets Thu 2026-12-03 17:00Z (10:00 am Phoenix)
- **Last run:** 03:08Z, 29 min before this update
- **Keeper:** last ran 02:41Z, next wake 03:01Z

## Needs attention
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T723 (since 2026-11-24T00:30:33Z, no attempt used; a plan change fixes it): T723: measures 48% of the context window, over its 45% limit
- PLAN GAP T862 (since 2026-11-28T03:36:40Z, no attempt used; a plan change fixes it): T862: docs/handoff/drafts/T862.md is named by its requirements, tests or reads, but neither it nor a dependency owns it and it does not exist yet
- PLAN GAP T872 (since 2026-11-28T03:36:40Z, no attempt used; a plan change fixes it): T872: docs/handoff/drafts/T872.md is named by its requirements, tests or reads, but neither it nor a dependency owns it and it does not exist yet
- PLAN GAP T879 (since 2026-11-28T03:36:40Z, no attempt used; a plan change fixes it): T879: prompt kit/prompts/T879.md is missing; run scripts/gen_prompts.py; T879: branch claude/task/T879-plan-health is named by its requirements, tests or reads, but neither it nor a dependency owns it and it does not exist yet
- Strict plan check red: 58 problem(s) on tasks still to run (+607 on DONE tasks); first: T704: stored context_pct 38% but it measures 44%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T811 | server: the Confirm link's code goes by email to a person wi | done | 1 | 2 h 19 min | 02:05Z |  |
| T871 | portals: the link-only code send and check that email a pers | done | 1 | 114 min | 23:00Z |  |
| T856 | intake: a request may be texted only with an `sms` consent r | done | 2 | 2 h 16 min | 19:33Z | PLAN GAP: outside ownership: app/intake/src/replies.rs:367 (and replies.rs:204). The chat raw path web_chat::raw_turn -> |
| T749 | messaging: the unused `business_pick_waiting` text template  | check_failed | 1 | 44 min | 16:48Z | Ci: failure; Checks: failure / Checks failed at step: Rust build (the one boundary build) and tests (all but SPLIT_TESTS |
| T849 | server: the e2e web binary serves J1/J2 business time from t | check_failed | 4 | 118 min | 15:27Z | PLAN GAP: outside ownership: tests/e2e/web_chat.spec.ts, tests/a11y/page_states.ts / D-5836 puts inbound SMS and action  |
| T817 | messaging: a text re-checks the opt-out list after its claim | done | 1 | 67 min | 14:27Z |  |
| T822 | docs: the privacy policy names the email link and the fuller | done | 1 | 54 min | 14:13Z |  |
| T862 | privacy: the email she types in her own conversation is kept | BLOCKED | 1 | 4 min | 13:22Z | PLAN GAP: outside ownership: app/server/src/homeowner_portal.rs, app/portals/src/homeowner_portal.rs / Evidence: her dat |
| T734 | server and adapters: a business can leave in the portal, wit | BLOCKED | 3 | 26 min | 13:10Z | PLAN GAP: app/portals/templates/business_leave.html:25 owned by T228 and T410, not T734. Smallest plan change: let T734  |
| T867 | picks and payments (harness): an early fraud warning before  | done | 2 | 101 min | 12:57Z | Ci: failure; Coverage: failure / Coverage failed at step: Compare with tests/coverage_floor.json and refuse a lowered fl |
| T820 | server and portals: "Download my data" also holds withdrawn  | done | 4 | 106 min | 12:45Z | Hand-in time passed. Worker added unit tests in app/portals/src/data_download.rs (commit f4a102dca); portals-only covera |
| T734 | server and adapters: a business can leave in the portal, wit | check_failed | 2 | 2 h 44 min | 12:11Z |  |
| T778 | jobs, core and server: a dead Payment pending timer is reviv | done | 1 | 114 min | 11:24Z |  |
| T867 | picks and payments (harness): an early fraud warning before  | check_failed | 1 | 60 min | 10:24Z |  |
| T868 | tests (harness): two DONE tests that can fail by chance are  | done | 1 | 85 min | 10:14Z |  |
| T849 | server: the e2e web binary serves J1/J2 business time from t | BLOCKED | 3 | 7 min | 09:16Z |  |
| T734 | server and adapters: a business can leave in the portal, wit | QUEUED | 1 | 2 h 19 min | 09:14Z |  |
| T869 | harness: the PII scan allows the Co-Authored-By no-reply add | done | 1 | 82 min | 08:53Z |  |
| T849 | server: the e2e web binary serves J1/J2 business time from t | check_failed | 2 | 45 min | 08:29Z |  |
| T688 | server: a call to the Laftly main number hears the greeting  | done | 1 | 85 min | 08:17Z |  |
| T657 | quotes and portals: quote-form and sign-in errors mark the f | check_failed | 3 | 75 min | 08:05Z | Worker stopped with no edits: start smoke exceeded ~20 min. Finding: J3 step 5 owner sign-in (signInOwner, tests/e2e/own |
| T849 | server: the e2e web binary serves J1/J2 business time from t | resync | 2 | 102 min | 07:36Z |  |
| T612 | harness: the WEB-029 and WEB-030 runtime checks on every sit | done | 3 | 2 h 5 min | 05:50Z | Reviewer on head 700f50e1fce2ee8fc4392587b89d7a82a956f72f still ended REVIEW-VERDICT: FINDINGS after the fix round: / 1. |
| T518 | server, ops and payments: the Stripe-or-webhook-errors alert | done | 1 | 51 min | 05:34Z |  |
| T657 | quotes and portals: quote-form and sign-in errors mark the f | QUEUED | 2 | 25 min | 05:11Z |  |
| T849 | server: the e2e web binary serves J1/J2 business time from t | check_failed | 1 | 103 min | 05:09Z |  |
| T843 | runner (harness): a 35-minute pre-session smoke cap, and a p | done | 1 | 78 min | 04:51Z |  |
| T518 | server, ops and payments: the Stripe-or-webhook-errors alert | resync | 1 | 2 h 9 min | 04:13Z |  |
| T522 | server and portals: the business text-consent box is served  | done | 1 | 104 min | 04:07Z |  |

### Plan and harness merges on main
- 03:05Z Plan: pre-claim routine (drafts, chance-failure runs, coverage floors); T879, T880; T872/T862 drafts (D-5996) (#1526)
- 00:49Z Plan: web-chat update choice: email, text or both (D-5995) (#1524)
- 23:50Z Plan: PF-CHAIN2 fixes for the web-chat consent chain; seams T873-T876 (D-5989) (#1523)
- 22:40Z Harness: workers post a short progress note about every 10 minutes (D-5988) (#1521)
- 20:24Z Plan: T857/T865 drafts bound to T856's guard; under-18 refused text; T872 (D-5982) (#1520)
- 17:23Z Harness: Sonnet-worker trial on T871, T857, T865 from Opus drafts (D-5981) (#1518)
- 17:02Z Plan: one task at a time, token use assessed after each (D-5980) (#1516)
- 16:39Z Plan: pre-flight fixes before claim (D-5979) (#1515)
- 16:31Z Plan: T849 seeds J3's licence check at the app clock (D-5978) (#1514)
- 16:15Z Plan: efficiency fixes (D-5971); T734 owns its template line, T862 split with T870 (D-5972) (#1513)
- 12:14Z Plan: T657 waits on T811 (D-5954); T849 moves j1_homeowner into the late project (D-5955) (#1500)
- 07:26Z Plan: fraud-refund fix (D-5888), T820 fix (D-5889), T865 draft (D-5900), template line and T868 (D-5901), compliance (D-5907), T856 plan fix (D-5918) (#1

## Progress (requirements verified)

**Requirements verified:** 489 of 736 (66.4%) · partial 238 · unstarted 9
**Tasks merged (throughput only):** 696 of 811

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 30 | 5 | 0 | 35 |
| ai | 11 | 15 | 3 | 29 |
| businesses | 37 | 20 | 1 | 58 |
| calls | 18 | 7 | 0 | 25 |
| consent | 19 | 16 | 0 | 35 |
| fees | 27 | 25 | 0 | 52 |
| intake | 12 | 31 | 1 | 44 |
| job-cards | 17 | 7 | 0 | 24 |
| licensing | 41 | 7 | 0 | 48 |
| lifecycle | 23 | 5 | 0 | 28 |
| messaging | 25 | 20 | 0 | 45 |
| ops-recovery | 37 | 16 | 1 | 54 |
| privacy | 15 | 16 | 0 | 31 |
| quiet-hours | 5 | 4 | 0 | 9 |
| quotes-pick | 40 | 16 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 27 | 25 | 3 | 55 |
| terms | 35 | 0 | 0 | 35 |
| website | 29 | 1 | 0 | 30 |
| wording | 31 | 1 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-28T02:41:09Z (week)): week: 44.0M used, 150.0M left, resets 2026-12-03T17:00:00Z; projected finish 2026-12-12T02:17:00Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 339 of 385 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T849 (lease until 2026-11-28T06:36:40Z)

## Last usage reading
- {"task": "T811", "at": "2026-11-28T02:05:19Z", "cache_read_input_tokens": 6181030, "input_tokens": 154, "output_tokens": 38145, "cache_creation_input_tokens": 139861}
