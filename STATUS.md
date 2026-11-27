# Ask Hava build status

Updated 2026-11-27T16:50:56Z by runner 278d45f2f121 (actions).

**Idle since 16:48Z; 693 of 804 done; week 17% used, resets Thu 2026-12-03 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 693 of 804 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** idle since 16:48Z (no task in progress)
- **Week:** 17% used, 164.0M tokens left; resets Thu 2026-12-03 17:00Z (10:00 am Phoenix)
- **Last run:** 16:50Z, 0 min before this update
- **Keeper:** last ran 16:50Z, next wake 17:10Z

## Needs attention
- BLOCKED T734: plan gap reported: PLAN GAP: app/portals/templates/business_leave.html:25 owned by T228 and T410, not T734. Smallest plan change: let T734 own that template for one line, <link rel="stylesheet" href="/static/portals/forms.css"> at the top of {% block main %}, as get_help.html:9 and notice_settings.html:8 carry.
Evidence: the six reason radios on /b/leave and /b/pause-reason have no visible edge without forms.css (FORMS_CSS, app/portals/src/feedback.rs:73), so the A11Y-007 edge check (tests/a11y/targets_contrast.spec.ts:80) fails at 320, 768 and 1280 px. The reviewer confirmed from kit/tasks.json that T734 does n; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T734 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T849: E2e: failure; Ally: failure
Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing)
      335 |   await j.drive.press(page.getByRole("button", { name: "Confirm and pay $25" }));
    > 336 |   await expect(page.locator("#confirm-status")).toHaveText("We can't take payments right now. Nothing was charged. Try again in a few minutes.");
          |                                                 ^
      337 |   expect(await standInCharges(j.page.request, seeded.pick)).toEqual([]);
      338 |   return page;
      339 | }
        at pressRefused (/home/runner/work/askhava/askhava/tests/e2e/j3.ts:336:49)
        at step5SafeMode (/home/runner/work/askhava/askhava/tests/e2e/j3.ts:364:19)
        at /home/runner/work/askhava/askhava/tests/a11y/j3_keyboard.spec.ts:152:3
    Error Context: test-results/a11y/j3_keyboard-A11Y-005-A11Y--0a910-t-three-widths-on-each-page-late/error-context.md
    Error Context: test-results/a11y/j3_keyboard-A11Y-005-A11Y--0a910-t-three-widths-on-each-page-late/error-context.md
  1 failed
    [late] › tests/a11y/j3_keyboard.spec.ts:142:1 › A11Y-005 A11Y-021 J3 keyboard-only: sign-in, pause, refund, fee mode, safe mode, audit log, closing checks; axe at three widths on each page 
  1564 passed (13.9m)
##[error]Process completed with exit code 1.
E2e failed at step: The journeys (npx playwright test tests/e2e)
- BLOCKED T862: plan gap reported: PLAN GAP: outside ownership: app/server/src/homeowner_portal.rs, app/portals/src/homeowner_portal.rs
Evidence: her data download is assembled in app/server/src/homeowner_portal.rs:621-632 (held_file), which calls portals::more_held_items(&mut db, contact) with no vault or HomeownerSessionProof. OnFile::read (app/portals/src/homeowner_portal.rs:75-85) keeps only contact id and phone. Opening the sealed address under (PortalsHomeowner, OwnRecord) needs the vault and proof (privacy api.rs:317-322 read_own_contact). Neither file is owned or shared by T862 (kit/prompts/T862.md:35-44, :56). Smallest; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T862 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T723 (since 2026-11-24T00:30:33Z, no attempt used; a plan change fixes it): T723: measures 48% of the context window, over its 45% limit
- PLAN GAP T811 (since 2026-11-24T10:37:51Z, no attempt used; a plan change fixes it): T811: measures 50% of the context window, over its 45% limit
- PLAN GAP T856 (since 2026-11-27T07:31:36Z, no attempt used; a plan change fixes it): T856: measures 46% of the context window, over its 45% limit
- PLAN GAP T862 (since 2026-11-27T13:22:09Z, no attempt used; a plan change fixes it): outside ownership: app/server/src/homeowner_portal.rs, app/portals/src/homeowner_portal.rs
- Strict plan check red: 62 problem(s) on tasks still to run (+603 on DONE tasks); first: T704: stored context_pct 38% but it measures 43%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T749 | messaging: the unused `business_pick_waiting` text template  | MERGE_PENDING | 1 | 44 min | 16:48Z |  |
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
| T612 | harness: the WEB-029 and WEB-030 runtime checks on every sit | QUEUED | 2 | 96 min | 03:36Z |  |
| T799 | Templates and tests (harness): copy review fixes (refund att | done | 2 | 31 min | 02:52Z | PLAN GAP: contradiction: D-5401 requires es.json text "dentro de 72 horas de recibir los datos del cliente" (5 keys), bu |
| T856 | intake: a request may be texted only with an `sms` consent r | BLOCKED | 1 | 110 min | 02:15Z | PLAN GAP: outside ownership: app/intake/src/replies.rs:367 (and replies.rs:204). The chat raw path web_chat::raw_turn -> |
| T861 | e2e (harness): J1 reads a sent text from the fake carrier on | done | 1 | 42 min | 01:13Z |  |
| T783 | intake: a picture text's media is deleted at the provider af | done | 1 | 2 h 19 min | 00:37Z |  |
| T309 | server and portals: the business Get help form and the site  | done | 1 | 2 h 8 min | 00:28Z |  |
| T854 | portals and action links: sessions and admissions on `real_n | done | 1 | 2 h 5 min | 23:58Z |  |
| T744 | picks: a job that reaches a final status ends its pick await | done | 1 | 98 min | 23:54Z |  |
| T820 | server and portals: "Download my data" also holds withdrawn  | check_failed | 3 | 102 min | 21:46Z |  |
| T657 | quotes and portals: quote-form and sign-in errors mark the f | check_failed | 1 | 75 min | 21:40Z |  |
| T840 | intake and messaging: quote updates reach Email and web-and- | done | 1 | 2 h 10 min | 21:20Z |  |
| T848 | core, payments, inbound text, security: `Clock::real_now()`  | done | 1 | 75 min | 21:12Z |  |
| T847 | e2e: J3 step 5 seeds its refused pick at the app clock, so t | done | 1 | 94 min | 20:01Z |  |
| T612 | harness: the WEB-029 and WEB-030 runtime checks on every sit | BLOCKED | 1 | 50 min | 18:35Z |  |

### Plan and harness merges on main
- 16:39Z Plan: pre-flight fixes before claim (D-5979) (#1515)
- 16:31Z Plan: T849 seeds J3's licence check at the app clock (D-5978) (#1514)
- 16:15Z Plan: efficiency fixes (D-5971); T734 owns its template line, T862 split with T870 (D-5972) (#1513)
- 12:14Z Plan: T657 waits on T811 (D-5954); T849 moves j1_homeowner into the late project (D-5955) (#1500)
- 07:26Z Plan: fraud-refund fix (D-5888), T820 fix (D-5889), T865 draft (D-5900), template line and T868 (D-5901), compliance (D-5907), T856 plan fix (D-5918) (#1
- 23:18Z Plan: T801 web-chat consent chain re-split, per-request SMS consent, pick code by email (D-5848); T612 font fallback (D-5864); pre-flight fixes (D-5876, 
- 18:27Z Plan: test clock for the J1/J2 journeys, T222 as J1 core, T799's Spanish wording (D-5836, D-5837) (#1472)

## Progress (requirements verified)

**Requirements verified:** 465 of 735 (63.3%) · partial 262 · unstarted 8
**Tasks merged (throughput only):** 693 of 802

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
| security | 24 | 28 | 3 | 55 |
| terms | 35 | 0 | 0 | 35 |
| website | 27 | 3 | 0 | 30 |
| wording | 30 | 2 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-27T15:29:08Z (week)): week: 30.0M used, 164.0M left, resets 2026-12-03T17:00:00Z; projected finish 2026-12-11T06:59:48Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 336 of 376 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing

## Last usage reading
- {"task": "T749", "at": "2026-11-27T16:48:37Z", "output_tokens": 6164, "cache_read_input_tokens": 545579, "input_tokens": 47, "cache_creation_input_tokens": 57673}
