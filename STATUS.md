# Ask Hava build status

Updated 2026-11-27T07:26:16Z by runner 37f92ed09311 (manual).

**Building: T688 in progress since 06:52Z; 685 of 802 done; week 12% used, resets Thu 2026-12-03 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 685 of 802 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 2: T688 claimed 06:52Z, lease until 09:52Z; lane 3: T734 claimed 06:54Z, lease until 09:54Z; lane 4: T657 claimed 06:50Z, lease until 09:50Z; lane 5: T849 claimed 05:54Z, lease until 08:54Z
- **Week:** 12% used, 166.5M tokens left; resets Thu 2026-12-03 17:00Z (10:00 am Phoenix)
- **Last run:** 07:07Z, 18 min before this update
- **Keeper:** last ran 07:07Z, next wake 08:57Z

## Needs attention
- BLOCKED T820: Ci: failure; Checks: failure
Checks failed at step: Rust build (the one boundary build) and tests (all but SPLIT_TESTS, D-3142)
test td_sec_80_a_text_started_session_lasts_30_minutes ... ok
test td_sec_80_an_emailed_confirm_session_still_answers_at_29_days ... ok
test td_sec_80_a_text_token_elsewhere_or_appended_is_not_found ... ok
test td_sec_80_only_a_text_minted_grant_is_text_started ... ok
test td_sec_80_only_confirm_is_minted_by_text ... ok
failures:
---- td_sec_44_the_quote_form_acts_once_and_bounces_a_phone_number stdout ----
thread 'td_sec_44_the_quote_form_acts_once_and_bounces_a_phone_number' (42381) panicked at app/server/tests/action_links_route.rs:546:5:
the second price is not stored
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
failures:
    td_sec_44_the_quote_form_acts_once_and_bounces_a_phone_number
test result: FAILED. 30 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 12.34s
error: test failed, to rerun pass `-p askhava-server --test action_links_route`
##[error]Process completed with exit code 101.
Ci failed at step: All five jobs passed
Checks: failure, Lint and slow tests: success, Python: success, Coverage: success, E2e: success
##[error]Process completed with exit code 1.
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T723 (since 2026-11-24T00:30:33Z, no attempt used; a plan change fixes it): T723: measures 48% of the context window, over its 45% limit
- PLAN GAP T811 (since 2026-11-24T10:37:51Z, no attempt used; a plan change fixes it): T811: measures 50% of the context window, over its 45% limit
- Strict plan check red: 61 problem(s) on tasks still to run (+594 on DONE tasks); first: T704: stored context_pct 38% but it measures 43%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T612 | harness: the WEB-029 and WEB-030 runtime checks on every sit | done | 3 | 2 h 5 min | 05:50Z | Reviewer on head 700f50e1fce2ee8fc4392587b89d7a82a956f72f still ended REVIEW-VERDICT: FINDINGS after the fix round: / 1. |
| T518 | server, ops and payments: the Stripe-or-webhook-errors alert | done | 1 | 51 min | 05:34Z |  |
| T657 | quotes and portals: quote-form and sign-in errors mark the f | QUEUED | 2 | 25 min | 05:11Z | Worker stopped with no edits: start smoke exceeded ~20 min. Finding: J3 step 5 owner sign-in (signInOwner, tests/e2e/own |
| T849 | server: the e2e web binary serves J1/J2 business time from t | check_failed | 1 | 103 min | 05:09Z | E2e: failure; Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflo |
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

**Requirements verified:** 139 of 735 (18.9%) · partial 588 · unstarted 8
**Tasks merged (throughput only):** 685 of 802

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 5 | 30 | 0 | 35 |
| ai | 1 | 25 | 3 | 29 |
| businesses | 2 | 55 | 1 | 58 |
| calls | 1 | 24 | 0 | 25 |
| consent | 3 | 32 | 0 | 35 |
| fees | 1 | 51 | 0 | 52 |
| intake | 2 | 41 | 0 | 43 |
| job-cards | 2 | 22 | 0 | 24 |
| licensing | 0 | 48 | 0 | 48 |
| lifecycle | 0 | 28 | 0 | 28 |
| messaging | 2 | 43 | 0 | 45 |
| ops-recovery | 32 | 21 | 1 | 54 |
| privacy | 2 | 29 | 0 | 31 |
| quiet-hours | 0 | 9 | 0 | 9 |
| quotes-pick | 1 | 55 | 0 | 56 |
| recurring | 3 | 8 | 0 | 11 |
| security | 3 | 49 | 3 | 55 |
| terms | 32 | 3 | 0 | 35 |
| website | 24 | 6 | 0 | 30 |
| wording | 23 | 9 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-27T06:46:44Z (week)): week: 27.5M used, 166.5M left, resets 2026-12-03T17:00:00Z; projected finish 2026-12-10T18:25:01Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 328 of 371 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 2: T688 (lease until 2026-11-27T09:52:09Z)
- lane 3: T734 (lease until 2026-11-27T09:54:33Z)
- lane 4: T657 (lease until 2026-11-27T09:50:45Z)
- lane 5: T849 (lease until 2026-11-27T08:54:12Z)

## Last usage reading
- {"task": "T612", "at": "2026-11-27T05:50:05Z", "output_tokens": 94136, "cache_creation_input_tokens": 297587, "input_tokens": 302, "cache_read_input_tokens": 10731469}
