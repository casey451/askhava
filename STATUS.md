# Ask Hava build status

Updated 2026-11-28T13:08:01Z by runner 56386cb11f43 (actions).

**Idle since 13:07Z; 698 of 816 done; week 25% used, resets Thu 2026-12-03 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 698 of 816 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** idle since 13:07Z (no task in progress)
- **Week:** 25% used, 146.0M tokens left; resets Thu 2026-12-03 17:00Z (10:00 am Phoenix)
- **Last run:** 13:07Z, 0 min before this update
- **Keeper:** last ran 12:53Z, next wake 13:13Z

## Needs attention
- BLOCKED T881: plan gap reported: PLAN GAP: outside ownership: kit/state_schema.json
The wip.kind enum (kit/state_schema.json:429-434) is ["review","plan_gap"]. T881's acceptance line (kit/prompts/T881.md:11) needs scripts/check_state.py to pass on wip records of kinds stopped and blocked, so the schema must gain both kinds. T881 owns only runner/cycle.py, runner/wip.py and tests/test_runner_wip.py (T881.md:37-39, :45).
Second conflict: goal change (3) (wip_used no longer stops a later attempt's head being kept) breaks tests/test_runner_wip.py:180 (`"wip" not in entry`), which is not in the prompt's superseded-assertion list (; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T881 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T723 (since 2026-11-24T00:30:33Z, no attempt used; a plan change fixes it): T723: measures 48% of the context window, over its 45% limit
- PLAN GAP T881 (since 2026-11-28T13:07:02Z, no attempt used; a plan change fixes it): outside ownership: kit/state_schema.json
- Strict plan check red: 59 problem(s) on tasks still to run (+608 on DONE tasks); first: T704: stored context_pct 38% but it measures 44%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T881 | runner (harness): a blocked or stopped attempt's work is kep | BLOCKED | 1 | 7 min | 13:07Z | PLAN GAP: outside ownership: kit/state_schema.json / The wip.kind enum (kit/state_schema.json:429-434) is ["review","pla |
| T857 | intake: `queue_text` and `queue_body` behind the guard; a pa | QUEUED | 1 | 2 h 29 min | 12:51Z | Reviewer after the findings round still ended FINDINGS (head 087e75d9e56d022ae4ccc32727da20b554b306d1): app/intake/CLAUD |
| T872 | intake: the age stop closes and purges even when its text is | done | 1 | 73 min | 08:47Z |  |
| T879 | harness: a flake-run workflow that runs named tests many tim | done | 2 | 57 min | 06:56Z | Reviewer after the findings round still ends REVIEW-VERDICT: FINDINGS (head 97d7be0793d47a6bd0fca5b8b6517f58cab29aaf). F |
| T879 | harness: a flake-run workflow that runs named tests many tim | QUEUED | 1 | 62 min | 05:51Z |  |
| T849 | server: the e2e web binary serves J1/J2 business time from t | BLOCKED | 5 | 64 min | 04:41Z | PLAN GAP: tests/e2e/j1_homeowner.spec.ts:302 (not owned by T849; the per-IP limit is in app/portals/src/pick_flow.rs:846 |
| T811 | server: the Confirm link's code goes by email to a person wi | done | 1 | 2 h 19 min | 02:05Z |  |
| T871 | portals: the link-only code send and check that email a pers | done | 1 | 114 min | 23:00Z |  |
| T856 | intake: a request may be texted only with an `sms` consent r | done | 2 | 2 h 16 min | 19:33Z | PLAN GAP: outside ownership: app/intake/src/replies.rs:367 (and replies.rs:204). The chat raw path web_chat::raw_turn -> |
| T749 | messaging: the unused `business_pick_waiting` text template  | check_failed | 1 | 44 min | 16:48Z | Ci: failure; Checks: failure / Checks failed at step: Rust build (the one boundary build) and tests (all but SPLIT_TESTS |
| T849 | server: the e2e web binary serves J1/J2 business time from t | check_failed | 4 | 118 min | 15:27Z |  |
| T817 | messaging: a text re-checks the opt-out list after its claim | done | 1 | 67 min | 14:27Z |  |
| T822 | docs: the privacy policy names the email link and the fuller | done | 1 | 54 min | 14:13Z |  |
| T862 | privacy: the email she types in her own conversation is kept | BLOCKED | 1 | 4 min | 13:22Z | PLAN GAP: outside ownership: app/server/src/homeowner_portal.rs, app/portals/src/homeowner_portal.rs / Evidence: her dat |
| T734 | server and adapters: a business can leave in the portal, wit | BLOCKED | 3 | 26 min | 13:10Z | PLAN GAP: app/portals/templates/business_leave.html:25 owned by T228 and T410, not T734. Smallest plan change: let T734  |

### Plan and harness merges on main
- 12:32Z Plan: efficiency changes: early full e2e for cross-cutting tasks, split after a second plan gap, keep blocked work (T881, T882), checklist and goal cap (
- 05:06Z Plan: T849 clears J1's own per-IP code buckets before step 6 (D-5998) (#1529)
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

## Progress (requirements verified)

**Requirements verified:** 486 of 736 (66.0%) · partial 241 · unstarted 9
**Tasks merged (throughput only):** 698 of 816

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 30 | 5 | 0 | 35 |
| ai | 11 | 15 | 3 | 29 |
| businesses | 37 | 20 | 1 | 58 |
| calls | 18 | 7 | 0 | 25 |
| consent | 19 | 16 | 0 | 35 |
| fees | 27 | 25 | 0 | 52 |
| intake | 11 | 32 | 1 | 44 |
| job-cards | 17 | 7 | 0 | 24 |
| licensing | 41 | 7 | 0 | 48 |
| lifecycle | 23 | 5 | 0 | 28 |
| messaging | 25 | 20 | 0 | 45 |
| ops-recovery | 37 | 16 | 1 | 54 |
| privacy | 14 | 17 | 0 | 31 |
| quiet-hours | 5 | 4 | 0 | 9 |
| quotes-pick | 39 | 17 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 27 | 25 | 3 | 55 |
| terms | 35 | 0 | 0 | 35 |
| website | 29 | 1 | 0 | 30 |
| wording | 31 | 1 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-28T12:53:05Z (week)): week: 48.0M used, 146.0M left, resets 2026-12-03T17:00:00Z; projected finish 2026-12-12T10:36:38Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 341 of 388 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing

## Last usage reading
- {"task": "T881", "at": "2026-11-28T13:07:02Z", "output_tokens": 28923, "cache_read_input_tokens": 1566975, "input_tokens": 59, "cache_creation_input_tokens": 103111}
