# Ask Hava build status

Updated 2026-11-26T19:57:27Z by runner a508df213427 (actions).

**Building: T847 in progress since 18:26Z; 672 of 788 done; week 3% used, resets Thu 2026-12-03 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 672 of 788 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T847 claimed 18:26Z, lease until 21:26Z; lane 2: T848 claimed 19:56Z, lease until 22:56Z; lane 5: T840 claimed 19:09Z, lease until 22:09Z
- **Week:** 3% used, 187.2M tokens left; resets Thu 2026-12-03 17:00Z (10:00 am Phoenix)
- **Last run:** 19:32Z, 24 min before this update
- **Keeper:** last ran 19:32Z, next wake 21:29Z

## Needs attention
- BLOCKED T612: plan gap reported: PLAN GAP: app/site/static/site.css:96-99 (owner T613, not T612): the "DM Sans fallback" face names only local("Arial") and local("Roboto"); the CI image has Liberation Sans only, so with fonts blocked the 320x740 first-screen check in tests/e2e/site_speed.spec.ts fails (Get quotes bottom 771 of 740). Reviewer confirmed: with local("Liberation Sans") added in-browser the button bottom is 695 px. Smallest fix: add local("Liberation Sans") to that face's src with the same size-adjust figures. Reviewer also found: quote-page animation check at site_speed.spec.ts:321-329 reads getAnimations only af; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T612 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T801: plan gap reported: PLAN GAP: app/server/tests/web_chat_stops.rs:572 and :592 (owned by T645) assert a consent text queued to a chat-verified number that never texted in; fixing review finding (web_chat consent must never permit a text, CON-005/CON-025) requires changing them. Also app/server/tests/web_chat_after_code.rs:46,:319,:368 (T649) for the "We'll answer by text" wording. Smallest plan change: let T801 own web_chat_stops.rs (and web_chat_after_code.rs). Prepared fix patch at /tmp/T801-fix-round.patch. Reviewer confirmed the gap for web_chat_stops.rs. Proof: cargo test --workspace --exclude askhava-integra; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T801 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T723 (since 2026-11-24T00:30:33Z, no attempt used; a plan change fixes it): T723: measures 48% of the context window, over its 45% limit
- PLAN GAP T811 (since 2026-11-24T10:37:51Z, no attempt used; a plan change fixes it): T811: measures 48% of the context window, over its 45% limit
- Strict plan check red: 65 problem(s) on tasks still to run (+576 on DONE tasks); first: T402: stored context_pct 29% but it measures 33%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T612 | harness: the WEB-029 and WEB-030 runtime checks on every sit | BLOCKED | 1 | 50 min | 18:35Z | PLAN GAP: app/site/static/site.css:96-99 (owner T613, not T612): the "DM Sans fallback" face names only local("Arial") a |

### Plan and harness merges on main
- 18:27Z Plan: test clock for the J1/J2 journeys, T222 as J1 core, T799's Spanish wording (D-5836, D-5837) (#1472)

## Progress (requirements verified)

**Requirements verified:** 478 of 735 (65.0%) · partial 249 · unstarted 8
**Tasks merged (throughput only):** 672 of 788

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 28 | 7 | 0 | 35 |
| ai | 11 | 15 | 3 | 29 |
| businesses | 35 | 22 | 1 | 58 |
| calls | 17 | 8 | 0 | 25 |
| consent | 23 | 12 | 0 | 35 |
| fees | 27 | 25 | 0 | 52 |
| intake | 14 | 29 | 0 | 43 |
| job-cards | 17 | 7 | 0 | 24 |
| licensing | 41 | 7 | 0 | 48 |
| lifecycle | 22 | 6 | 0 | 28 |
| messaging | 25 | 20 | 0 | 45 |
| ops-recovery | 35 | 18 | 1 | 54 |
| privacy | 15 | 16 | 0 | 31 |
| quiet-hours | 4 | 5 | 0 | 9 |
| quotes-pick | 39 | 17 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 23 | 29 | 3 | 55 |
| terms | 35 | 0 | 0 | 35 |
| website | 27 | 3 | 0 | 30 |
| wording | 30 | 2 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-26T19:31:26Z (week)): week: 6.8M used, 187.2M left, resets 2026-12-03T17:00:00Z; projected finish 2026-12-09T20:54:31Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 315 of 360 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T847 (lease until 2026-11-26T21:26:32Z)
- lane 2: T848 (lease until 2026-11-26T22:56:55Z)
- lane 5: T840 (lease until 2026-11-26T22:09:53Z)

## Last usage reading
- {"task": "T612", "at": "2026-11-26T18:35:36Z", "cache_read_input_tokens": 6573358, "cache_creation_input_tokens": 212492, "input_tokens": 129, "output_tokens": 64089}
