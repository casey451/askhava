# Ask Hava build status

Updated 2026-11-12T13:26:30Z by runner 87abedec7b6e (actions).

**Building: T362 in progress since 12:56Z; 245 of 397 done; week 94% used, resets Thu 2026-11-12 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 245 of 397 build tasks (owner gates not counted)
- **Stage:** stage 4; stage proofs recorded: 1, 2, 3
- **Lane:** lane 1: T362 claimed 12:56Z, lease until 15:56Z; lane 3: T385 claimed 12:57Z, lease until 15:57Z
- **Week:** 94% used, -0.5M tokens left; resets Thu 2026-11-12 17:00Z (10:00 am Phoenix)
- **Last run:** 13:22Z, 3 min before this update
- **Keeper:** last ran 13:22Z, next wake 15:59Z

## Needs attention
- BLOCKED T280: plan gap reported: PLAN GAP: app/portals/src/business_auth.rs:130 (SignInMail::addresses) and :176 (CodeAddress::code_to) have nothing to look up. No module stores a business member's sign-in email or texting phone in a readable/searchable form, and no task in kit/tasks.json builds one. Evidence: privacy.contact_index only ever written for HomeownerPhone (app/privacy/src/vault.rs:398,409); email_index (app/privacy/src/reader.rs:321) only computes a digest and nothing calls it; migrations/0056_businesses_team_members.sql:5-11 says email/phone is never stored there; Reader list (app/privacy/src/models.rs:89-102) h; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T280 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T251 (since 2026-11-12T11:06:16Z, no attempt used; a plan change fixes it): T251: measures 47% of the context window, over its 45% limit
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- Strict plan check red: 25 problem(s) on tasks still to run (+115 on DONE tasks); first: T396: stored context_pct 39% but it measures 46%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T369 | businesses: the sign-up after-hours answer ticks only while  | done | 1 | 50 min | 12:41Z |  |
| T378 | adapters, calls and stand-in (harness): every outbound call  | done | 1 | 45 min | 12:34Z |  |
| T323 | ai: a new capitalized name at the start of a sentence counts | done | 1 | 24 min | 11:47Z |  |
| T404 | harness: the door probe's fixtures hold a charged pick and t | done | 1 | 27 min | 11:46Z |  |
| T345 | harness: every external GitHub Actions `uses:` in every work | done | 3 | 27 min | 11:16Z | PLAN GAP: tests/test_runner_cache.py:207,:214 and tests/test_runner_merge_early.py:212,:213 hard-code actions/cache/rest |
| T377 | adapters and stand-in (harness): every text asks Twilio for  | done | 1 | 28 min | 11:15Z |  |
| T280 | server and messaging (harness): the business sign-in pages a | BLOCKED | 1 | 8 min | 11:14Z | PLAN GAP: app/portals/src/business_auth.rs:130 (SignInMail::addresses) and :176 (CodeAddress::code_to) have nothing to l |
| T268 | portals, businesses and messaging: the sign-up consent box,  | done | 1 | 24 min | 11:05Z |  |
| T411 | quotes: the scrubbed job description and the shown and appro | done | 1 | 28 min | 10:36Z |  |
| T343 | harness: the DONE pick tests' fixtures record each job's fac | done | 1 | 24 min | 10:23Z |  |
| T411 | quotes: the scrubbed job description and the shown and appro | resync | 1 | 30 min | 10:00Z |  |
| T268 | portals, businesses and messaging: the sign-up consent box,  | resync | 1 | 21 min | 09:38Z |  |
| T358 | messaging: the Canadian area codes for the HELP roaming line | done | 1 | 40 min | 09:32Z |  |
| T268 | portals, businesses and messaging: the sign-up consent box,  | resync | 1 | 58 min | 09:12Z |  |
| T406 | quotes: a quote's PICK-040 facts are stored with it and read | done | 1 | 41 min | 09:05Z |  |
| T336 | server, adapters and stand-in (harness): a text stopped by a | done | 2 | 56 min | 09:00Z | merging main into the reviewed head conflicts in app/server/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T345 | harness: every external GitHub Actions `uses:` in every work | BLOCKED | 2 | 4 min | 08:47Z |  |
| T322 | businesses: a backup notice address is checked again at conf | done | 1 | 35 min | 08:36Z |  |
| T251 | intake and quotes: the ZIP question, the job location, and t | BLOCKED | 1 | 3 min | 08:17Z | PLAN GAP: app/intake/src/summary_step.rs:415 and app/intake/src/service.rs:672 (T208/T039 files, not T251's). The card s |
| T360 | messaging: the opt-out confirmation email while texting is o | done | 1 | 43 min | 07:42Z |  |
| T336 | server, adapters and stand-in (harness): a text stopped by a | resync | 2 | 47 min | 07:35Z |  |
| T390 | calls and server: a blocked staff call that connects a line  | done | 1 | 29 min | 07:34Z |  |
| T401 | server (harness): the door probe's fixture gains a released  | done | 2 | 37 min | 07:33Z | PLAN GAP: tests/security/tests/no_homeowner_charge.rs:17-27,123 (TD-FEE-07, not T401's) fails any app/ file naming a hom |
| T339 | jobs: each job keeps its category code and remaining candida | done | 1 | 18 min | 07:13Z |  |
| T336 | server, adapters and stand-in (harness): a text stopped by a | resync | 2 | 71 min | 06:06Z |  |
| T405 | payments and adapters (harness): each charged pick's 3-D Sec | done | 1 | 51 min | 06:05Z |  |
| T356 | messaging and server (harness): the worker sends queued text | done | 1 | 86 min | 05:31Z |  |
| T405 | payments and adapters (harness): each charged pick's 3-D Sec | resync | 1 | 33 min | 04:56Z |  |
| T326 | calls (harness): the LINK notice names the line's stored PIN | done | 1 | 25 min | 03:37Z |  |
| T336 | server, adapters and stand-in (harness): a text stopped by a | QUEUED | 1 | 5 min | 03:27Z |  |
| T248 | catalog and businesses: markets as reviewed data, Arizona ZI | done | 1 | 12 min | 03:22Z |  |
| T401 | server (harness): the door probe's fixture gains a released  | BLOCKED | 1 | 19 min | 03:13Z |  |
| T400 | server: the web process stores the catalog file at start-up, | done | 1 | 21 min | 03:07Z |  |
| T248 | catalog and businesses: markets as reviewed data, Arizona ZI | resync | 1 | 67 min | 02:56Z |  |
| T334 | payments: the nightly reconcile catches up after an outage f | done | 2 | 41 min | 02:25Z | merging main into the reviewed head conflicts in app/payments/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T313 | messaging and server: an EMERGENCY card notice goes at night | done | 1 | 19 min | 02:07Z |  |
| T368 | businesses (harness): the capability checklist offers every  | done | 1 | 17 min | 02:06Z |  |
| T325 | server: an inbound text stopped by a module error raises a b | done | 1 | 30 min | 01:42Z |  |
| T368 | businesses (harness): the capability checklist offers every  | resync | 1 | 25 min | 01:13Z |  |
| T317 | server and intake: the inbound text route applies SEC-014's  | done | 2 | 53 min | 00:16Z | PLAN GAP: tests/integration/tests/suspension_mode.rs:69-78 (owned by T279) and app/server/tests/voice_confirm_line.rs:95 |
| T313 | messaging and server: an EMERGENCY card notice goes at night | resync | 1 | 35 min | 00:03Z |  |
| T228 | businesses: leaving Ask Hava, the optional reason and what h | done | 2 | 23 min | 23:45Z | merging main into the reviewed head conflicts in app/picks/CLAUDE.md, app/quotes/src/api.rs; a resync merges it in on th |
| T334 | payments: the nightly reconcile catches up after an outage f | resync | 2 | 96 min | 23:36Z |  |
| T348 | harness: the authorization matrix gets public, owner and act | done | 1 | 43 min | 23:32Z |  |
| T228 | businesses: leaving Ask Hava, the optional reason and what h | resync | 2 | 41 min | 23:04Z |  |
| T317 | server and intake: the inbound text route applies SEC-014's  | BLOCKED | 1 | 4 min | 22:34Z |  |
| T232 | portals and businesses: the Confirm step's "Can't take it" l | done | 1 | 51 min | 22:02Z |  |
| T387 | payments (harness): the branch that flaps payments coverage  | done | 1 | 90 min | 21:49Z |  |
| T388 | picks, intake, messaging and portals: after a Confirm the ho | done | 1 | 34 min | 21:46Z |  |
| T334 | payments: the nightly reconcile catches up after an outage f | QUEUED | 1 | 15 min | 21:21Z |  |
| T228 | businesses: leaving Ask Hava, the optional reason and what h | check_failed | 1 | 31 min | 19:59Z |  |
| T228 | businesses: leaving Ask Hava, the optional reason and what h | resync | 1 | 77 min | 19:09Z |  |
| T316 | businesses: one read of each business's public name for the  | done | 2 | 26 min | 19:02Z | PLAN GAP: app/businesses/src/holding.rs:8 and migrations/0050_licensing_imports.sql:22-55: nothing links a business to a |
| T332 | payments (harness): a late success has its own refund trigge | done | 4 | 25 min | 18:53Z | merging main into the reviewed head conflicts in app/payments/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T345 | harness: every external GitHub Actions `uses:` in every work | BLOCKED | 1 | 8 min | 18:44Z |  |
| T332 | payments (harness): a late success has its own refund trigge | resync | 4 | 46 min | 18:10Z |  |
| T272 | portals: the sign-in link cap and the code-try cap are atomi | done | 1 | 20 min | 18:05Z |  |
| T333 | payments (harness): a succeeded refund is recorded at the am | done | 2 | 32 min | 17:20Z | PLAN GAP: app/payments/src/routes.rs:412 (T259) and app/payments/src/refund_state.rs:34 (T270) / app/picks/src/unreachab |
| T272 | portals: the sign-in link cap and the code-try cap are atomi | resync | 1 | 27 min | 17:17Z |  |
| T165 | businesses: business name, Google and Yelp links and license | done | 1 | 18 min | 17:05Z |  |
| T214 | picks (harness): the homeowner's share box at the pick and t | done | 3 | 45 min | 16:44Z | PLAN GAP: tests/integration/tests/page_pick.rs:131 and tests/security/tests/page_pick_privacy.rs:224 (both T246's; page_ |
| T165 | businesses: business name, Google and Yelp links and license | resync | 1 | 47 min | 16:25Z |  |
| T269 | portals: a business notice's signed link opens sign-in for t | done | 2 | 23 min | 16:14Z | merging main into the reviewed head conflicts in app/portals/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T269 | portals: a business notice's signed link opens sign-in for t | resync | 2 | 44 min | 15:46Z |  |
| T384 | quotes and portals: the homeowner sees an edited quote as up | done | 2 | 18 min | 15:05Z | Ci: failure; E2e: failure; Ally: failure / E2e failed at step: Chromium and its system libraries / E: Failed to fetch ht |
| T214 | picks (harness): the homeowner's share box at the pick and t | BLOCKED | 2 | 3 min | 15:04Z |  |
| T342 | quotes: a quote's contact check also reads the whole field w | done | 1 | 51 min | 14:31Z |  |
| T332 | payments (harness): a late success has its own refund trigge | check_failed | 3 | 48 min | 14:24Z |  |
| T269 | portals: a business notice's signed link opens sign-in for t | check_failed | 1 | 20 min | 14:19Z |  |
| T384 | quotes and portals: the homeowner sees an edited quote as up | check_failed | 1 | 40 min | 13:40Z |  |

### Plan and harness merges on main
- 09:36Z Plan: pre-flight fixes round 9 — stage 5: T303, T151, T321, T110, T210, T109, T116; new T409, T410 (D-3155) (#559)
- 09:22Z Plan: T345 owns the runner tests that hard-code action tags (D-3200) (#572)
- 09:13Z Plan: store the scrubbed job description and shown card so T251 can send approved cards (D-3189) (#568)
- 08:32Z Harness: smoke ancestry reuse accepts squash commits whose identical tree passed Ci, Twin and Ally (D-3156) (#560)
- 07:12Z Harness: close merged tasks before the smoke so their lanes free ~20 min sooner (D-3141) (#552)
- 06:05Z Plan: pre-flight fixes round 8 — stage 5: T101, T102, T105, T202, T320, T106, T364; new T408 (D-3144) (#551)
- 05:28Z Harness: stop backup-runner caches evicting CI's Rust caches (D-3143) (#553)
- 05:16Z Harness: run lint and the slow compile-fail/prompt-injection tests in a parallel CI job (D-3142) (#550)
- 04:59Z Plan: pre-flight fixes round 7 — T352, T335, T353, T330, T393, T359, T401 plan gap; new T406, T407 (D-3134) (#548)
- 03:50Z Plan: pre-flight fixes round 6 — T355, T340, T341, T249, T273, T271, T277, T357, T314; new T405 (D-3118) (#542)
- 03:24Z Harness: claim order prefers tasks in crates no other lane is writing (D-3082) (#523)
- 02:11Z Plan: pre-flight fixes round 5 — T378, T343, T268, T329, T306; new T404 (D-3112) (#538)
- 02:03Z Plan: pre-flight fixes round 4 — T356, T362, T365, T367, T300, T369; new T401–T403 (D-3106) (#536)
- 01:55Z Plan: owner approval before a team-linked phone works (BIZ-033, IC-95) — T396–T399 (D-3081) (#531)
- 00:38Z Plan: T261 pre-flight fix and catalog loaded at start-up (T400, IC-96) (D-3100) (#534)
- 00:00Z Plan: pre-flight fixes round 3 — T360, T368, T350, T358, T366, T385 (D-3094) (#528)
- 23:50Z Plan: Gate C preconditions, vendor-leak check, quote cards, AI writing calls (D-3080) (#518)
- 23:07Z Plan: IC-74 owners T390/T391, migration renumber, budget refresh, IC-95 logged (D-3079) (#517)
- 23:02Z Plan: pre-flight fixes for T317, T323, T313, T315, T347 and the T345 gap (D-3083) (#524)
- 22:35Z Harness: smoke ancestry reuse across DONE task merges and a running duplicate smoke yields (D-3058) (#516)
- 21:48Z Harness: no five-hour pacing budget (D-3056) (#513)
- 20:03Z Harness: the critical-path test checks what the runner can claim, not live in-flight work (D-3057) (#514)
- 19:58Z Plan: pre-flight fixes for five queued tasks before they're claimed (D-3055) (#512)
- 18:49Z Plan: five lanes (D-3043) (#504)
- 18:37Z Plan: T387 steadies payments branch coverage and adds margin, claimed first (D-3049) (#507)
- 15:44Z Plan: T214 owns the final-Pick share box end to end and ticks it in five finished tests (D-3032) (#495)
- 15:02Z Plan: T333 may book the webhook refund amount and hold the refunded notice on an unchecked refund (D-3018) (#487)
- 14:48Z Plan: T214 builds the pick notice's "Can't take it" value; new T386 adds the line to both languages (D-3024) (#489)
- 14:34Z Harness: browser setup survives an Ubuntu mirror outage (D-3030) (#492)
- 13:45Z Plan: four lanes (D-2986) (#483)

## Progress (requirements verified)

**Requirements verified:** 209 of 733 (28.5%) · partial 355 · unstarted 169
**Tasks merged (throughput only):** 245 of 397

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 13 | 16 | 6 | 35 |
| ai | 7 | 17 | 5 | 29 |
| businesses | 15 | 40 | 3 | 58 |
| calls | 10 | 14 | 1 | 25 |
| consent | 8 | 23 | 4 | 35 |
| fees | 12 | 39 | 1 | 52 |
| intake | 21 | 17 | 5 | 43 |
| job-cards | 11 | 11 | 2 | 24 |
| licensing | 20 | 28 | 0 | 48 |
| lifecycle | 18 | 10 | 0 | 28 |
| messaging | 17 | 25 | 3 | 45 |
| ops-recovery | 2 | 7 | 45 | 54 |
| privacy | 7 | 22 | 2 | 31 |
| quiet-hours | 4 | 5 | 0 | 9 |
| quotes-pick | 30 | 25 | 1 | 56 |
| recurring | 6 | 2 | 3 | 11 |
| security | 6 | 43 | 6 | 55 |
| terms | 0 | 3 | 32 | 35 |
| website | 0 | 3 | 25 | 28 |
| wording | 2 | 5 | 25 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-12T13:22:02Z (week)): week: 194.5M used, -494k left, resets 2026-11-12T17:00:00Z; projected finish 2026-12-05T20:03:43Z; waiting until 2026-11-12T17:00:00Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 125 of 179 done
- stage 5: 0 of 64 done
- stage 6: 0 of 39 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T362 (lease until 2026-11-12T15:56:41Z)
- lane 3: T385 (lease until 2026-11-12T15:57:41Z)

## Last usage reading
- {"task": "T369", "at": "2026-11-12T12:41:55Z", "cache_read_input_tokens": 20521304, "output_tokens": 125817, "cache_creation_input_tokens": 629909, "input_tokens": 381}
