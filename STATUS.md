# Ask Hava build status

Updated 2026-11-27T12:11:21Z by runner 9e2f05934667 (actions).

**Building: T820 in progress since 10:58Z; 689 of 802 done; week 14% used, resets Thu 2026-12-03 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 689 of 802 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 2: T820 claimed 10:58Z, lease until 13:58Z; lane 3: T867 claimed 11:15Z, lease until 14:15Z; lane 4: T734 claimed 09:27Z, lease until 12:27Z
- **Week:** 14% used, 161.3M tokens left; resets Thu 2026-12-03 17:00Z (10:00 am Phoenix)
- **Last run:** 11:53Z, 17 min before this update
- **Keeper:** last ran 11:33Z, next wake 12:29Z

## Needs attention
- BLOCKED T657: Ci: failure; Coverage: failure
Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly)
test td_sec_80_a_text_token_elsewhere_or_appended_is_not_found ... ok
test td_sec_80_an_emailed_confirm_session_still_answers_at_29_days ... ok
test td_sec_80_only_a_text_minted_grant_is_text_started ... ok
test td_sec_80_only_confirm_is_minted_by_text ... ok
failures:
---- td_sec_44_the_quote_form_acts_once_and_bounces_a_phone_number stdout ----
thread 'td_sec_44_the_quote_form_acts_once_and_bounces_a_phone_number' (42044) panicked at app/server/tests/action_links_route.rs:546:5:
the second price is not stored
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
failures:
    td_sec_44_the_quote_form_acts_once_and_bounces_a_phone_number
test result: FAILED. 30 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 14.90s
error: test failed, to rerun pass `-p askhava-server --test action_links_route`
error: process didn't exit successfully: `/home/runner/.rustup/toolchains/nightly-2026-10-15-x86_64-unknown-linux-gnu/bin/cargo test --tests --manifest-path /home/runner/work/askhava/askhava/Cargo.tom
##[error]Process completed with exit code 101.
Ci failed at step: All five jobs passed
Checks: success, Lint and slow tests: success, Python: success, Coverage: failure, E2e: success
##[error]Process completed with exit code 1.
- BLOCKED T849: plan gap reported: PLAN GAP: outside ownership: tests/e2e/web_chat.spec.ts, tests/a11y/page_states.ts
D-5836 puts inbound SMS and action links on the test-control clock. web_chat.spec.ts:64-68 (consentsSince with no contact, DB real clock_timestamp) and page_states.ts:249-250 (action link expires_at = now() + 2 hours in DB time) are not owned by T849. After j1_homeowner.spec.ts:219 advances the clock up to a day, 4 web_chat tests (:111,:131,:183,:206) and 3 site_speed.spec.ts:256 tests (404) fail. Reviewer confirmed. Fix: T849 owns web_chat.spec.ts and filters consentsSince by contact, and page_states.ts seeds l; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T849 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T723 (since 2026-11-24T00:30:33Z, no attempt used; a plan change fixes it): T723: measures 48% of the context window, over its 45% limit
- PLAN GAP T811 (since 2026-11-24T10:37:51Z, no attempt used; a plan change fixes it): T811: measures 50% of the context window, over its 45% limit
- PLAN GAP T849 (since 2026-11-27T09:16:18Z, no attempt used; a plan change fixes it): outside ownership: tests/e2e/web_chat.spec.ts, tests/a11y/page_states.ts
- PLAN GAP T856 (since 2026-11-27T07:31:36Z, no attempt used; a plan change fixes it): T856: measures 46% of the context window, over its 45% limit
- Strict plan check red: 63 problem(s) on tasks still to run (+596 on DONE tasks); first: T704: stored context_pct 38% but it measures 43%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T778 | jobs, core and server: a dead Payment pending timer is reviv | done | 1 | 114 min | 11:24Z |  |
| T867 | picks and payments (harness): an early fraud warning before  | check_failed | 1 | 60 min | 10:24Z | Ci: failure; Coverage: failure / Coverage failed at step: Compare with tests/coverage_floor.json and refuse a lowered fl |
| T868 | tests (harness): two DONE tests that can fail by chance are  | done | 1 | 85 min | 10:14Z |  |
| T849 | server: the e2e web binary serves J1/J2 business time from t | BLOCKED | 3 | 7 min | 09:16Z | PLAN GAP: outside ownership: tests/e2e/web_chat.spec.ts, tests/a11y/page_states.ts / D-5836 puts inbound SMS and action  |
| T734 | server and adapters: a business can leave in the portal, wit | QUEUED | 1 | 2 h 19 min | 09:14Z | session ended without handing in |
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
| T612 | harness: the WEB-029 and WEB-030 runtime checks on every sit | QUEUED | 2 | 96 min | 03:36Z |  |
| T799 | Templates and tests (harness): copy review fixes (refund att | done | 2 | 31 min | 02:52Z | PLAN GAP: contradiction: D-5401 requires es.json text "dentro de 72 horas de recibir los datos del cliente" (5 keys), bu |
| T856 | intake: a request may be texted only with an `sms` consent r | BLOCKED | 1 | 110 min | 02:15Z | PLAN GAP: outside ownership: app/intake/src/replies.rs:367 (and replies.rs:204). The chat raw path web_chat::raw_turn -> |
| T861 | e2e (harness): J1 reads a sent text from the fake carrier on | done | 1 | 42 min | 01:13Z |  |
| T783 | intake: a picture text's media is deleted at the provider af | done | 1 | 2 h 19 min | 00:37Z |  |
| T309 | server and portals: the business Get help form and the site  | done | 1 | 2 h 8 min | 00:28Z |  |
| T854 | portals and action links: sessions and admissions on `real_n | done | 1 | 2 h 5 min | 23:58Z |  |
| T744 | picks: a job that reaches a final status ends its pick await | done | 1 | 98 min | 23:54Z |  |
| T820 | server and portals: "Download my data" also holds withdrawn  | check_failed | 3 | 102 min | 21:46Z | Hand-in time passed. Worker added unit tests in app/portals/src/data_download.rs (commit f4a102dca); portals-only covera |
| T657 | quotes and portals: quote-form and sign-in errors mark the f | check_failed | 1 | 75 min | 21:40Z |  |
| T840 | intake and messaging: quote updates reach Email and web-and- | done | 1 | 2 h 10 min | 21:20Z |  |
| T848 | core, payments, inbound text, security: `Clock::real_now()`  | done | 1 | 75 min | 21:12Z |  |
| T847 | e2e: J3 step 5 seeds its refused pick at the app clock, so t | done | 1 | 94 min | 20:01Z |  |
| T612 | harness: the WEB-029 and WEB-030 runtime checks on every sit | BLOCKED | 1 | 50 min | 18:35Z |  |

### Plan and harness merges on main
- 07:26Z Plan: fraud-refund fix (D-5888), T820 fix (D-5889), T865 draft (D-5900), template line and T868 (D-5901), compliance (D-5907), T856 plan fix (D-5918) (#1
- 23:18Z Plan: T801 web-chat consent chain re-split, per-request SMS consent, pick code by email (D-5848); T612 font fallback (D-5864); pre-flight fixes (D-5876, 
- 18:27Z Plan: test clock for the J1/J2 journeys, T222 as J1 core, T799's Spanish wording (D-5836, D-5837) (#1472)

## Progress (requirements verified)

**Requirements verified:** 464 of 735 (63.1%) · partial 263 · unstarted 8
**Tasks merged (throughput only):** 689 of 802

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 28 | 7 | 0 | 35 |
| ai | 11 | 15 | 3 | 29 |
| businesses | 35 | 22 | 1 | 58 |
| calls | 17 | 8 | 0 | 25 |
| consent | 18 | 17 | 0 | 35 |
| fees | 25 | 27 | 0 | 52 |
| intake | 12 | 31 | 0 | 43 |
| job-cards | 17 | 7 | 0 | 24 |
| licensing | 40 | 8 | 0 | 48 |
| lifecycle | 22 | 6 | 0 | 28 |
| messaging | 24 | 21 | 0 | 45 |
| ops-recovery | 35 | 18 | 1 | 54 |
| privacy | 14 | 17 | 0 | 31 |
| quiet-hours | 4 | 5 | 0 | 9 |
| quotes-pick | 37 | 19 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 23 | 29 | 3 | 55 |
| terms | 35 | 0 | 0 | 35 |
| website | 27 | 3 | 0 | 30 |
| wording | 30 | 2 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-27T10:51:07Z (week)): week: 32.7M used, 161.3M left, resets 2026-12-03T17:00:00Z; projected finish 2026-12-11T06:36:13Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 332 of 374 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 2: T820 (lease until 2026-11-27T13:58:44Z)
- lane 3: T867 (lease until 2026-11-27T14:15:35Z)
- lane 4: T734 (lease until 2026-11-27T12:27:02Z)

## Last usage reading
- {"task": "T778", "at": "2026-11-27T11:24:12Z", "cache_read_input_tokens": 5756666, "input_tokens": 167, "output_tokens": 49927, "cache_creation_input_tokens": 176111}
