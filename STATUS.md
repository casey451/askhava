# Ask Hava build status

Updated 2026-11-19T19:07:31Z by runner 599c4d8662c4 (actions).

**Building: T161 in progress since 17:58Z; 501 of 696 done; week 4% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 501 of 696 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 2: T161 claimed 17:58Z, lease until 20:58Z; lane 3: T307 claimed 17:47Z, lease until 20:47Z; lane 5: T607 claimed 17:15Z, lease until 20:15Z; lane 6: T480 claimed 17:18Z, lease until 20:18Z
- **Week:** 4% used, 183.7M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 18:53Z, 14 min before this update
- **Keeper:** last ran 18:29Z, next wake 20:17Z

## Needs attention
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- Strict plan check red: 60 problem(s) on tasks still to run (+339 on DONE tasks); first: T403: stored context_pct 13% but it measures 18%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T555 | Decisions digest, piece 10: the next entries after the heade | MERGE_PENDING | 1 | 2 h 7 min | 17:51Z |  |
| T715 | intake (harness): corrections to T671's walker: alarm answer | MERGE_PENDING | 3 | 113 min | 17:43Z | PLAN GAP: one TD-INT-52 sentence can't be met by intake as planned: "a two-word first text naming a trade opens its job  |
| T530 | portals: the owner help page and its screened reply box (D-3 | QUEUED | 1 | 117 min | 17:38Z | Review round 2 on head 9cae918e901f6738321e0450589f137ab939da2f: REVIEW-VERDICT: FINDINGS (the one fix round is used). / |
| T724 | harness: Ci's Rust-test jobs, the worker job and the nightly | done | 1 | 50 min | 17:09Z |  |
| T210 | payments: the money view's sales-tax set-aside and manual ex | done | 1 | 53 min | 16:29Z |  |
| T407 | calls, portals and server: the owner-portal safety voice cal | done | 1 | 102 min | 16:01Z |  |
| T541 | payments, portals and adapters: a card saved or changed from | check_failed | 1 | 2 h 16 min | 15:25Z | Ci: failure; Coverage: failure; E2e: failure / Coverage failed at step: Compare with tests/coverage_floor.json and refus |
| T470 | messaging and server: the worker task that sends queued emai | done | 3 | 2 h 18 min | 15:24Z | Fix-round review (after the one allowed findings round) still found a problem at head 1830747e4; no reviewer approved a  |
| T593 | site and server: the home hero in D-3331's wording with the  | done | 2 | 75 min | 15:11Z | Ci: failure; Coverage: cancelled / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / tes |
| T554 | Decisions digest, piece 9: the next entries after the header | done | 1 | 110 min | 15:03Z |  |
| T652 | ops: safe mode starts only on OPS-001's alert kinds, and a d | done | 2 | 112 min | 15:03Z | PLAN GAP: tests/integration/tests/safe_mode.rs:246,281,320,378,407 (owned by T487 and T408; T652's prompt :11 and :47 sa |
| T158 | portals: the owner's pilot scorecard page with go and stop l | done | 1 | 2 h 57 min | 14:37Z |  |
| T540 | portals and server: the owner's release button re-sends an e | done | 1 | 53 min | 13:47Z |  |
| T470 | messaging and server: the worker task that sends queued emai | QUEUED | 2 | 2 h 21 min | 13:03Z |  |
| T593 | site and server: the home hero in D-3331's wording with the  | check_failed | 1 | 95 min | 12:35Z |  |
| T536 | E2e harness: clock-advancing journeys last, a keyed worker,  | done | 2 | 107 min | 12:33Z | PLAN GAP: tests/e2e/j2.ts:150 (owned by T099, `e2e-j2-business`; T644 also lists it). Smallest plan change: make the `no |
| T109 | recovery: backup checks and the nightly encrypted export | BLOCKED | 1 | 20 min | 11:47Z | PLAN GAP: `deny.toml:3` (owned by T009 and T604, both outside T109; T109 does not own it): every `age` release fails `ca |
| T540 | portals and server: the owner's release button re-sends an e | resync | 1 | 54 min | 11:29Z |  |
| T705 | Integration tests (harness): one shared log capture that a c | done | 1 | 114 min | 11:11Z |  |
| T470 | messaging and server: the worker task that sends queued emai | QUEUED | 1 | 93 min | 10:35Z |  |
| T710 | quotes and licensing (harness): a launch-open category takes | done | 1 | 85 min | 10:06Z |  |
| T553 | Decisions digest, piece 8: the next entries after the header | done | 1 | 76 min | 10:04Z |  |
| T622 | portals: the scorecard's first-six-weeks email panel and OPS | done | 1 | 2 h 41 min | 09:47Z |  |
| T375 | portals and server: the business's Job types page lets the o | done | 1 | 74 min | 09:18Z |  |
| T652 | ops: safe mode starts only on OPS-001's alert kinds, and a d | BLOCKED | 1 | 5 min | 09:10Z |  |
| T536 | E2e harness: clock-advancing journeys last, a keyed worker,  | BLOCKED | 1 | 46 min | 09:00Z |  |
| T476 | portals: the owner health page with every OPS-008 figure and | done | 1 | 2 h 27 min | 08:50Z |  |
| T161 | wording: the WORD-024 voice rule's pressure-word scan, the r | BLOCKED | 1 | 4 min | 08:41Z | PLAN GAP: `tests/docs/test_site_copy_blocks.py` must check that the home page and footer show the blocks in `docs/copy/s |
| T456 | picks and server: "Hold for 2 more hours" is stored, honoure | done | 1 | 97 min | 08:35Z |  |
| T216 | site: the hero photo and mark (photography assets) | done | 1 | 97 min | 08:10Z |  |
| T310 | server and portals: the owner home and settings pages and th | done | 1 | 74 min | 07:19Z |  |
| T562 | server and portals: "Why was I charged?" is served with the  | done | 1 | 2 h 43 min | 07:09Z |  |
| T229 | Legal drafts: reading-grade check and plain-language rewrite | done | 1 | 2 h 17 min | 06:59Z |  |
| T506 | Decisions digest, piece 7: the next entries after the header | done | 1 | 102 min | 06:55Z |  |
| T408 | server: a release waits while safe mode is on and goes ahead | done | 3 | 2 h 48 min | 06:17Z | PLAN GAP: kit/prompts/T408.md:54 "Size budget: at most 6 files, 800 changed lines" cannot be met. The prompt's own plan  |
| T203 | Wording scan: the required statements, business copy lines a | done | 4 | 42 min | 04:50Z | PLAN GAP: app/portals/templates/es_business_signup.html:82 ("Licencia de control de plagas") and :88 ("Mi licencia inclu |
| T490 | server (harness): the email provider passed to the sign-up m | done | 1 | 2 h 25 min | 04:35Z |  |
| T520 | portals and messaging: the portal stylesheets and email colo | done | 1 | 2 h 21 min | 04:18Z |  |
| T563 | intake and privacy: the daily consent sweep removes consent  | done | 3 | 77 min | 04:05Z | Ci: failure; Coverage: failure / Coverage failed at step: Compare with tests/coverage_floor.json and refuse a lowered fl |
| T399 | server: the email's Approve link approves the phone, only wi | done | 1 | 46 min | 04:01Z |  |
| T484 | portals and quotes: "Call from the Laftly number", the owner | done | 1 | 82 min | 03:26Z |  |
| T114 | Terms-match-code checks and recurring pricing copy | done | 1 | 71 min | 03:26Z |  |
| T715 | intake (harness): corrections to T671's walker: alarm answer | BLOCKED | 2 | 83 min | 03:22Z |  |
| T203 | Wording scan: the required statements, business copy lines a | BLOCKED | 3 | 29 min | 03:08Z |  |
| T306 | server and portals: the complaints page, the emergency-email | done | 1 | 38 min | 02:21Z |  |
| T203 | Wording scan: the required statements, business copy lines a | check_failed | 2 | 41 min | 01:40Z |  |
| T563 | intake and privacy: the daily consent sweep removes consent  | check_failed | 2 | 99 min | 01:37Z |  |
| T505 | Decisions digest, piece 6: the next entries after the header | done | 1 | 99 min | 01:27Z |  |
| T488 | portals and server: the business sign-up served in Spanish o | done | 1 | 111 min | 01:20Z |  |
| T306 | server and portals: the complaints page, the emergency-email | resync | 1 | 77 min | 00:59Z |  |
| T205 | portals: owner refunds with a review step and the audit log  | done | 2 | 2 h 26 min | 00:48Z | PLAN GAP: `payments` has no public reader that ties a dispute to its pick (or its charge, business or job). Without one, |
| T708 | intake: a quote-count update due in quiet hours goes at the  | done | 1 | 50 min | 00:40Z |  |
| T364 | server and portals: the business step-up code page is served | done | 2 | 70 min | 00:32Z | merging main into the reviewed head conflicts in app/server/src/main.rs; a resync merges it in on this attempt (D-891) |
| T701 | messaging: a per-number opt-out lock, and the listing key an | done | 1 | 45 min | 23:02Z |  |
| T583 | intake and server: the quote-count update text is sent at mo | done | 2 | 55 min | 22:50Z | PLAN GAP: kit/prompts/T583.md:51 (budget of 450 changed lines in 3 files) The smallest plan change: raise T583's budget  |
| T504 | Decisions digest, piece 5: the next entries after the header | done | 1 | 39 min | 22:47Z |  |
| T364 | server and portals: the business step-up code page is served | resync | 2 | 52 min | 22:05Z |  |
| T651 | server and portals: owner step-up try limit, the passkey pat | done | 1 | 2 h 7 min | 21:52Z |  |
| T722 | payments: read a pick's disputes and record Casey's evidence | done | 1 | 45 min | 21:33Z |  |
| T503 | Decisions digest, piece 4: the next entries after the header | done | 1 | 31 min | 21:30Z |  |
| T483 | messaging, portals and server: the owner's "away until" date | done | 1 | 2 h 48 min | 21:13Z |  |
| T702 | privacy and messaging: no blind-index key version is retired | done | 1 | 40 min | 21:07Z |  |
| T706 | payments: held_release, the FEE-017 re-check that clears a p | done | 1 | 100 min | 20:54Z |  |
| T715 | intake (harness): corrections to T671's walker: alarm answer | BLOCKED | 1 | 48 min | 20:53Z |  |
| T502 | Decisions digest, piece 3: the next entries after the header | done | 1 | 38 min | 20:31Z |  |
| T364 | server and portals: the business step-up code page is served | check_failed | 1 | 2 h 1 min | 20:29Z |  |
| T702 | privacy and messaging: no blind-index key version is retired | resync | 1 | 59 min | 20:21Z |  |
| T501 | Decisions digest, piece 2: the next entries after the header | done | 1 | 73 min | 19:42Z |  |
| T711 | intake (harness): the dispatcher's two jobs in one request:  | done | 1 | 77 min | 19:29Z |  |

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 330 of 735 (44.9%) · partial 368 · unstarted 37
**Tasks merged (throughput only):** 501 of 695

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 18 | 17 | 0 | 35 |
| ai | 8 | 18 | 3 | 29 |
| businesses | 21 | 36 | 1 | 58 |
| calls | 14 | 11 | 0 | 25 |
| consent | 16 | 19 | 0 | 35 |
| fees | 22 | 30 | 0 | 52 |
| intake | 9 | 34 | 0 | 43 |
| job-cards | 12 | 12 | 0 | 24 |
| licensing | 34 | 14 | 0 | 48 |
| lifecycle | 18 | 10 | 0 | 28 |
| messaging | 22 | 22 | 1 | 45 |
| ops-recovery | 9 | 27 | 18 | 54 |
| privacy | 9 | 21 | 1 | 31 |
| quiet-hours | 5 | 4 | 0 | 9 |
| quotes-pick | 25 | 31 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 11 | 39 | 5 | 55 |
| terms | 27 | 7 | 1 | 35 |
| website | 17 | 10 | 3 | 30 |
| wording | 23 | 5 | 4 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-19T17:57:32Z (week)): week: 10.3M used, 183.7M left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-04T07:09:58Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 144 of 291 done
- stage 6: 0 of 52 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 2: T161 (lease until 2026-11-19T20:58:49Z)
- lane 3: T307 (lease until 2026-11-19T20:47:50Z)
- lane 5: T607 (lease until 2026-11-19T20:15:21Z)
- lane 6: T480 (lease until 2026-11-19T20:18:37Z)

## Last usage reading
- {"task": "T555", "at": "2026-11-19T17:51:20Z", "cache_creation_input_tokens": 358983, "cache_read_input_tokens": 9988713, "output_tokens": 73658, "input_tokens": 247}
