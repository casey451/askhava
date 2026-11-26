# Ask Hava build status

Updated 2026-11-26T18:28:57Z by runner a508df213427 (manual).

**Building: T847 in progress since 18:26Z; 672 of 788 done; week 2% used, resets Thu 2026-12-03 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 672 of 788 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T847 claimed 18:26Z, lease until 21:26Z; lane 2: T612 claimed 17:45Z, lease until 20:45Z; lane 3: T820 claimed 17:44Z, lease until 20:44Z; lane 4: T783 claimed 17:46Z, lease until 20:46Z
- **Week:** 2% used, 186.7M tokens left; resets Thu 2026-12-03 17:00Z (10:00 am Phoenix)
- **Last run:** 18:28Z, 0 min before this update
- **Keeper:** last ran 18:19Z, next wake 20:46Z

## Needs attention
- CLAIMS STOPPED: nightly success on 0f6466a20c23, failures_since_green 0; no stage passed its exit test
- BLOCKED T801: plan gap reported: PLAN GAP: app/server/tests/web_chat_stops.rs:572 and :592 (owned by T645) assert a consent text queued to a chat-verified number that never texted in; fixing review finding (web_chat consent must never permit a text, CON-005/CON-025) requires changing them. Also app/server/tests/web_chat_after_code.rs:46,:319,:368 (T649) for the "We'll answer by text" wording. Smallest plan change: let T801 own web_chat_stops.rs (and web_chat_after_code.rs). Prepared fix patch at /tmp/T801-fix-round.patch. Reviewer confirmed the gap for web_chat_stops.rs. Proof: cargo test --workspace --exclude askhava-integra; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T801 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T723 (since 2026-11-24T00:30:33Z, no attempt used; a plan change fixes it): T723: measures 48% of the context window, over its 45% limit
- PLAN GAP T811 (since 2026-11-24T10:37:51Z, no attempt used; a plan change fixes it): T811: measures 48% of the context window, over its 45% limit
- Strict plan check red: 65 problem(s) on tasks still to run (+576 on DONE tasks); first: T402: stored context_pct 29% but it measures 33%; regenerate

## Last 24 hours
- no attempt ended in the last 24 hours

### Plan and harness merges on main
- 18:27Z Plan: test clock for the J1/J2 journeys, T222 as J1 core, T799's Spanish wording (D-5836, D-5837) (#1472)

## Progress (requirements verified)

**Requirements verified:** 66 of 735 (9.0%) · partial 661 · unstarted 8
**Tasks merged (throughput only):** 672 of 788

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 2 | 33 | 0 | 35 |
| ai | 0 | 26 | 3 | 29 |
| businesses | 0 | 57 | 1 | 58 |
| calls | 1 | 24 | 0 | 25 |
| consent | 1 | 34 | 0 | 35 |
| fees | 1 | 51 | 0 | 52 |
| intake | 0 | 43 | 0 | 43 |
| job-cards | 0 | 24 | 0 | 24 |
| licensing | 0 | 48 | 0 | 48 |
| lifecycle | 0 | 28 | 0 | 28 |
| messaging | 2 | 43 | 0 | 45 |
| ops-recovery | 18 | 35 | 1 | 54 |
| privacy | 2 | 29 | 0 | 31 |
| quiet-hours | 0 | 9 | 0 | 9 |
| quotes-pick | 1 | 55 | 0 | 56 |
| recurring | 3 | 8 | 0 | 11 |
| security | 2 | 50 | 3 | 55 |
| terms | 6 | 29 | 0 | 35 |
| website | 10 | 20 | 0 | 30 |
| wording | 17 | 15 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-26T18:19:06Z (week)): week: 7.3M used, 186.7M left, resets 2026-12-03T17:00:00Z; projected finish 2026-12-09T14:48:07Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 315 of 353 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T847 (lease until 2026-11-26T21:26:32Z)
- lane 2: T612 (lease until 2026-11-26T20:45:16Z)
- lane 3: T820 (lease until 2026-11-26T20:44:31Z)
- lane 4: T783 (lease until 2026-11-26T20:46:51Z)

## Last usage reading
- {"task": "T123", "at": "2026-11-25T01:33:41Z", "cache_creation_input_tokens": 98373, "input_tokens": 128, "cache_read_input_tokens": 2165304, "output_tokens": 23531}
