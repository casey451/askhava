# Ask Hava build status

Updated 2026-11-26T21:40:09Z by runner 0f6466a20c23 (manual).

**Building: T657 in progress since 20:24Z; 673 of 788 done; week 4% used, resets Thu 2026-12-03 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 673 of 788 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T657 claimed 20:24Z, lease until 23:24Z; lane 3: T820 claimed 20:04Z, lease until 23:04Z; lane 4: T309 claimed 20:18Z, lease until 23:18Z
- **Week:** 4% used, 183.9M tokens left; resets Thu 2026-12-03 17:00Z (10:00 am Phoenix)
- **Last run:** 21:22Z, 17 min before this update
- **Keeper:** last ran 21:21Z, next wake 22:12Z

## Needs attention
- CLAIMS STOPPED: nightly success on a508df213427, failures_since_green 0; no stage passed its exit test
- BLOCKED T612: plan gap reported: PLAN GAP: app/site/static/site.css:96-99 (owner T613, not T612): the "DM Sans fallback" face names only local("Arial") and local("Roboto"); the CI image has Liberation Sans only, so with fonts blocked the 320x740 first-screen check in tests/e2e/site_speed.spec.ts fails (Get quotes bottom 771 of 740). Reviewer confirmed: with local("Liberation Sans") added in-browser the button bottom is 695 px. Smallest fix: add local("Liberation Sans") to that face's src with the same size-adjust figures. Reviewer also found: quote-page animation check at site_speed.spec.ts:321-329 reads getAnimations only af; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T612 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T801: plan gap reported: PLAN GAP: app/server/tests/web_chat_stops.rs:572 and :592 (owned by T645) assert a consent text queued to a chat-verified number that never texted in; fixing review finding (web_chat consent must never permit a text, CON-005/CON-025) requires changing them. Also app/server/tests/web_chat_after_code.rs:46,:319,:368 (T649) for the "We'll answer by text" wording. Smallest plan change: let T801 own web_chat_stops.rs (and web_chat_after_code.rs). Prepared fix patch at /tmp/T801-fix-round.patch. Reviewer confirmed the gap for web_chat_stops.rs. Proof: cargo test --workspace --exclude askhava-integra; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T801 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T723 (since 2026-11-24T00:30:33Z, no attempt used; a plan change fixes it): T723: measures 48% of the context window, over its 45% limit
- PLAN GAP T811 (since 2026-11-24T10:37:51Z, no attempt used; a plan change fixes it): T811: measures 48% of the context window, over its 45% limit
- Strict plan check red: 67 problem(s) on tasks still to run (+576 on DONE tasks); first: T402: stored context_pct 29% but it measures 33%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T840 | intake and messaging: quote updates reach Email and web-and- | MERGE_PENDING | 1 | 2 h 10 min | 21:20Z |  |
| T848 | core, payments, inbound text, security: `Clock::real_now()`  | MERGE_PENDING | 1 | 75 min | 21:12Z |  |
| T847 | e2e: J3 step 5 seeds its refused pick at the app clock, so t | done | 1 | 94 min | 20:01Z |  |
| T612 | harness: the WEB-029 and WEB-030 runtime checks on every sit | BLOCKED | 1 | 50 min | 18:35Z | PLAN GAP: app/site/static/site.css:96-99 (owner T613, not T612): the "DM Sans fallback" face names only local("Arial") a |

### Plan and harness merges on main
- 18:27Z Plan: test clock for the J1/J2 journeys, T222 as J1 core, T799's Spanish wording (D-5836, D-5837) (#1472)

## Progress (requirements verified)

**Requirements verified:** 67 of 735 (9.1%) · partial 660 · unstarted 8
**Tasks merged (throughput only):** 673 of 781

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
| wording | 18 | 14 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-26T20:09:31Z (week)): week: 10.1M used, 183.9M left, resets 2026-12-03T17:00:00Z; projected finish 2026-12-09T21:31:54Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 316 of 360 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T657 (lease until 2026-11-26T23:24:45Z)
- lane 3: T820 (lease until 2026-11-26T23:04:34Z)
- lane 4: T309 (lease until 2026-11-26T23:18:50Z)

## Last usage reading
- {"task": "T840", "at": "2026-11-26T21:20:42Z", "cache_read_input_tokens": 11247178, "output_tokens": 85596, "input_tokens": 292, "cache_creation_input_tokens": 281379}
