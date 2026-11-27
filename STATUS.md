# Ask Hava build status

Updated 2026-11-27T04:13:43Z by runner 4e32ecc653df (actions).

**Building: T849 in progress since 03:25Z; 681 of 799 done; week 11% used, resets Thu 2026-12-03 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 681 of 799 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 2: T849 claimed 03:25Z, lease until 06:25Z; lane 3: T612 claimed 03:44Z, lease until 06:44Z; lane 4: T518 claimed 02:04Z, lease until 05:04Z; lane 5: T843 claimed 03:33Z, lease until 06:33Z
- **Week:** 11% used, 167.0M tokens left; resets Thu 2026-12-03 17:00Z (10:00 am Phoenix)
- **Last run:** 04:10Z, 2 min before this update
- **Keeper:** last ran 04:10Z, next wake 05:07Z

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
- BLOCKED T856: plan gap reported: PLAN GAP: outside ownership: app/intake/src/replies.rs:367 (and replies.rs:204). The chat raw path web_chat::raw_turn -> replies::receive_raw_reply -> answer_name_address -> receive_answer (replies.rs:367) and reply_in's receive_text call (replies.rs:204) reach models::record_consent; T856 does not own replies.rs, TextIn has ~100 unowned builders, so only a runtime flag (set_chat_turn, D-5891) is possible in owned files. Reviewer (agent aec58822cf2c18f01) confirmed the gap with evidence at head c3beeeb4ca4cae1ff578a5f1aa7ea50d5c8e35e3; its only finding is that D-5891 should also cite replies.r; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T856 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T723 (since 2026-11-24T00:30:33Z, no attempt used; a plan change fixes it): T723: measures 48% of the context window, over its 45% limit
- PLAN GAP T811 (since 2026-11-24T10:37:51Z, no attempt used; a plan change fixes it): T811: measures 50% of the context window, over its 45% limit
- PLAN GAP T856 (since 2026-11-27T02:15:00Z, no attempt used; a plan change fixes it): outside ownership: app/intake/src/replies.rs:367 (and replies.rs:204). The chat raw path web_chat::raw_turn -> replies::receive_raw_reply -> answer_name_address -> receive_answer (replies.rs:367) and reply_in's receive_text call (replies.rs:204) reach models::record_consent; T856 does not own replies.rs, TextIn has ~100 unowned builders, so only a runtime flag (set_chat_turn, D-5891) is possible in owned files. Reviewer (agent aec58822cf2c18f01) confirmed the gap with evidence at head c3beeeb4ca4cae1ff578a5f1aa7ea50d5c8e35e3; its only finding is that D-5891 should also cite replies.rs:204.
- Strict plan check red: 66 problem(s) on tasks still to run (+588 on DONE tasks); first: T704: stored context_pct 38% but it measures 43%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T522 | server and portals: the business text-consent box is served  | MERGE_PENDING | 1 | 104 min | 04:07Z |  |
| T612 | harness: the WEB-029 and WEB-030 runtime checks on every sit | QUEUED | 2 | 96 min | 03:36Z | Reviewer on head 700f50e1fce2ee8fc4392587b89d7a82a956f72f still ended REVIEW-VERDICT: FINDINGS after the fix round: / 1. |
| T799 | Templates and tests (harness): copy review fixes (refund att | done | 2 | 31 min | 02:52Z | PLAN GAP: contradiction: D-5401 requires es.json text "dentro de 72 horas de recibir los datos del cliente" (5 keys), bu |
| T856 | intake: a request may be texted only with an `sms` consent r | BLOCKED | 1 | 110 min | 02:15Z | PLAN GAP: outside ownership: app/intake/src/replies.rs:367 (and replies.rs:204). The chat raw path web_chat::raw_turn -> |
| T861 | e2e (harness): J1 reads a sent text from the fake carrier on | done | 1 | 42 min | 01:13Z |  |
| T783 | intake: a picture text's media is deleted at the provider af | done | 1 | 2 h 19 min | 00:37Z |  |
| T309 | server and portals: the business Get help form and the site  | done | 1 | 2 h 8 min | 00:28Z |  |
| T854 | portals and action links: sessions and admissions on `real_n | done | 1 | 2 h 5 min | 23:58Z |  |
| T744 | picks: a job that reaches a final status ends its pick await | done | 1 | 98 min | 23:54Z |  |
| T820 | server and portals: "Download my data" also holds withdrawn  | check_failed | 3 | 102 min | 21:46Z | Hand-in time passed. Worker added unit tests in app/portals/src/data_download.rs (commit f4a102dca); portals-only covera |
| T657 | quotes and portals: quote-form and sign-in errors mark the f | check_failed | 1 | 75 min | 21:40Z | Ci: failure; E2e: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / {"timestamp":"2026-11-26T |
| T840 | intake and messaging: quote updates reach Email and web-and- | done | 1 | 2 h 10 min | 21:20Z |  |
| T848 | core, payments, inbound text, security: `Clock::real_now()`  | done | 1 | 75 min | 21:12Z |  |
| T847 | e2e: J3 step 5 seeds its refused pick at the app clock, so t | done | 1 | 94 min | 20:01Z |  |
| T612 | harness: the WEB-029 and WEB-030 runtime checks on every sit | BLOCKED | 1 | 50 min | 18:35Z |  |

### Plan and harness merges on main
- 23:18Z Plan: T801 web-chat consent chain re-split, per-request SMS consent, pick code by email (D-5848); T612 font fallback (D-5864); pre-flight fixes (D-5876, 
- 18:27Z Plan: test clock for the J1/J2 journeys, T222 as J1 core, T799's Spanish wording (D-5836, D-5837) (#1472)

## Progress (requirements verified)

**Requirements verified:** 467 of 735 (63.5%) · partial 260 · unstarted 8
**Tasks merged (throughput only):** 681 of 799

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 28 | 7 | 0 | 35 |
| ai | 11 | 15 | 3 | 29 |
| businesses | 35 | 22 | 1 | 58 |
| calls | 17 | 8 | 0 | 25 |
| consent | 18 | 17 | 0 | 35 |
| fees | 27 | 25 | 0 | 52 |
| intake | 12 | 31 | 0 | 43 |
| job-cards | 17 | 7 | 0 | 24 |
| licensing | 41 | 7 | 0 | 48 |
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
- Pacing (session_reading 2026-11-27T03:37:37Z (week)): week: 27.0M used, 167.0M left, resets 2026-12-03T17:00:00Z; projected finish 2026-12-10T17:36:54Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 324 of 371 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 2: T849 (lease until 2026-11-27T06:25:59Z)
- lane 3: T612 (lease until 2026-11-27T06:44:20Z)
- lane 4: T518 (lease until 2026-11-27T05:04:26Z)
- lane 5: T843 (lease until 2026-11-27T06:33:11Z)

## Last usage reading
- {"task": "T522", "at": "2026-11-27T04:07:46Z", "output_tokens": 62522, "input_tokens": 217, "cache_creation_input_tokens": 233834, "cache_read_input_tokens": 12043377}
