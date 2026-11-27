# Ask Hava build status

Updated 2026-11-27T00:46:10Z by runner 5ca4c08df875 (actions).

**Building: T856 in progress since 00:24Z; 676 of 799 done; week 7% used, resets Thu 2026-12-03 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 676 of 799 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T856 claimed 00:24Z, lease until 03:24Z; lane 5: T861 claimed 00:31Z, lease until 03:31Z
- **Week:** 7% used, 176.9M tokens left; resets Thu 2026-12-03 17:00Z (10:00 am Phoenix)
- **Last run:** 00:46Z, 0 min before this update
- **Keeper:** last ran 00:39Z, next wake 03:27Z

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
- PLAN GAP T811 (since 2026-11-24T10:37:51Z, no attempt used; a plan change fixes it): T811: measures 48% of the context window, over its 45% limit
- Strict plan check red: 62 problem(s) on tasks still to run (+582 on DONE tasks); first: T704: stored context_pct 38% but it measures 42%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T783 | intake: a picture text's media is deleted at the provider af | MERGE_PENDING | 1 | 2 h 19 min | 00:37Z |  |
| T309 | server and portals: the business Get help form and the site  | MERGE_PENDING | 1 | 2 h 8 min | 00:28Z |  |
| T854 | portals and action links: sessions and admissions on `real_n | MERGE_PENDING | 1 | 2 h 5 min | 23:58Z |  |
| T744 | picks: a job that reaches a final status ends its pick await | done | 1 | 98 min | 23:54Z |  |
| T820 | server and portals: "Download my data" also holds withdrawn  | check_failed | 3 | 102 min | 21:46Z | Hand-in time passed. Worker added unit tests in app/portals/src/data_download.rs (commit f4a102dca); portals-only covera |
| T657 | quotes and portals: quote-form and sign-in errors mark the f | check_failed | 1 | 75 min | 21:40Z | Ci: failure; E2e: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / {"timestamp":"2026-11-26T |
| T840 | intake and messaging: quote updates reach Email and web-and- | done | 1 | 2 h 10 min | 21:20Z |  |
| T848 | core, payments, inbound text, security: `Clock::real_now()`  | done | 1 | 75 min | 21:12Z |  |
| T847 | e2e: J3 step 5 seeds its refused pick at the app clock, so t | done | 1 | 94 min | 20:01Z |  |
| T612 | harness: the WEB-029 and WEB-030 runtime checks on every sit | BLOCKED | 1 | 50 min | 18:35Z | PLAN GAP: app/site/static/site.css:96-99 (owner T613, not T612): the "DM Sans fallback" face names only local("Arial") a |

### Plan and harness merges on main
- 23:18Z Plan: T801 web-chat consent chain re-split, per-request SMS consent, pick code by email (D-5848); T612 font fallback (D-5864); pre-flight fixes (D-5876, 
- 18:27Z Plan: test clock for the J1/J2 journeys, T222 as J1 core, T799's Spanish wording (D-5836, D-5837) (#1472)

## Progress (requirements verified)

**Requirements verified:** 467 of 735 (63.5%) · partial 260 · unstarted 8
**Tasks merged (throughput only):** 676 of 799

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
- Pacing (session_reading 2026-11-27T00:29:39Z (week)): week: 17.1M used, 176.9M left, resets 2026-12-03T17:00:00Z; projected finish 2026-12-10T15:47:11Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 319 of 371 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T856 (lease until 2026-11-27T03:24:55Z)
- lane 5: T861 (lease until 2026-11-27T03:31:39Z)

## Last usage reading
- {"task": "T783", "at": "2026-11-27T00:37:53Z", "cache_read_input_tokens": 5782596, "input_tokens": 214, "output_tokens": 54658, "cache_creation_input_tokens": 216487}
