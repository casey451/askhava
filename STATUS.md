# Ask Hava build status

Updated 2026-11-20T05:05:52Z by runner 0a0541d7d211 (actions).

**Idle since 05:02Z; 519 of 707 done; week 20% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 519 of 707 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** idle since 05:02Z (no task in progress)
- **Week:** 20% used, 148.6M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 05:05Z, 0 min before this update
- **Keeper:** last ran 05:05Z, next wake 05:25Z

## Needs attention
- BLOCKED T648: plan gap reported: PLAN GAP: app/recovery/src/restore.rs:44 include_str!("../../../scripts/bootstrap_roles.sql") is required by the goal, but scripts/check_include_paths.py (D-1867, run in the smoke by tests/test_check_include_paths.py::test_td_ops_32_the_real_repository_passes) fails: scripts/bootstrap_roles.sql is not matched by the Checks and Coverage rules in .github/workflows/ci.yml (protected, harness-only) nor by RUST_PATHS in scripts/prehandin.sh:77 (locked in kit/plan.lock). T648 owns neither. Fix: a harness task adds scripts/bootstrap_roles\.sql to those three rules and rewrites kit/plan.lock. Reviewer; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T648 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- Strict plan check red: 75 problem(s) on tasks still to run (+380 on DONE tasks); first: T402: stored context_pct 25% but it measures 29%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T751 | stand-ins (harness): the Stripe stand-in detaches a payment  | MERGE_PENDING | 1 | 51 min | 05:02Z |  |
| T726 | Runner (harness): a failed attempt's unpushed work can be ke | MERGE_PENDING | 2 | 36 min | 05:00Z | merging main into the reviewed head conflicts in kit/plan.lock, kit/runner.json; a resync merges it in on this attempt ( |
| T543 | picks, businesses, portals and server: "Away until" is store | MERGE_PENDING | 1 | 64 min | 04:57Z |  |
| T434 | portals: the monthly intake accuracy report page and the mon | MERGE_PENDING | 1 | 112 min | 04:55Z |  |
| T110 | recovery: replay with derived IDs | MERGE_PENDING | 1 | 2 h 36 min | 04:55Z |  |
| T663 | picks, jobs and payments (harness): a confirmed pick moves i | MERGE_PENDING | 3 | 65 min | 04:54Z | Ci: failure; Coverage: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / test  |
| T556 | Decisions digest, piece 11: the next entries after the heade | done | 1 | 51 min | 04:50Z |  |
| T726 | Runner (harness): a failed attempt's unpushed work can be ke | resync | 2 | 32 min | 04:08Z |  |
| T747 | intake and jobs: the homeowner's INT-029 emergency choice ma | done | 1 | 71 min | 03:25Z |  |
| T726 | Runner (harness): a failed attempt's unpushed work can be ke | resync | 2 | 2 h 16 min | 03:22Z |  |
| T617 | site: DM Sans from one weight-axis variable file (smaller th | BLOCKED | 1 | 2 min | 03:04Z | PLAN GAP: tests/a11y/design_tokens.spec.ts:221 and :242 (protected, not owned by T617) contradict the prompt's single DM |
| T648 | recovery: the monthly test restore into a throwaway cluster, | BLOCKED | 1 | 21 min | 02:55Z | PLAN GAP: app/recovery/src/restore.rs:44 include_str!("../../../scripts/bootstrap_roles.sql") is required by the goal, b |
| T763 | tests (harness): the web chat spec's fake numbers never coll | done | 1 | 25 min | 02:17Z |  |
| T672 | intake: the dispatcher asks "How soon?", the night count and | BLOCKED | 1 | 13 min | 02:12Z | PLAN GAP: tests/integration/tests/intake_order_two_jobs.rs:278 (owned by T711 and T717, not T672) also pins the old orde |
| T218 | Wording and template scans from the brains: sales phrases, n | done | 1 | 74 min | 01:46Z |  |
| T663 | picks, jobs and payments (harness): a confirmed pick moves i | check_failed | 2 | 2 h 7 min | 01:40Z |  |
| T527 | messaging, portals and server: the business-texting switch t | done | 1 | 93 min | 01:39Z |  |
| T726 | Runner (harness): a failed attempt's unpushed work can be ke | QUEUED | 1 | 56 min | 01:00Z |  |
| T613 | site: the D-4246 polish layer in site.css (tokens, layered s | done | 1 | 85 min | 00:47Z |  |
| T758 | intake: Spanish gas and burning reports reach the 911 list,  | done | 1 | 90 min | 00:38Z |  |
| T109 | recovery: backup checks and the nightly encrypted export | done | 2 | 32 min | 00:37Z | merging main into the reviewed head conflicts in docs/for-casey-review.md; a resync merges it in on this attempt (D-891) |
| T541 | payments, portals and adapters: a card saved or changed from | done | 3 | 25 min | 00:00Z | Ci: failure; E2e: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / {"timestamp":"2026-11-19T |
| T726 | Runner (harness): a failed attempt's unpushed work can be ke | resync | 1 | 84 min | 23:31Z |  |
| T109 | recovery: backup checks and the nightly encrypted export | resync | 2 | 93 min | 23:26Z |  |
| T707 | server: an emergency pick's address re-release waits while s | check_failed | 1 | 80 min | 22:59Z | Ci: failure; Coverage: cancelled / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / tes |
| T541 | payments, portals and adapters: a card saved or changed from | check_failed | 2 | 91 min | 22:55Z |  |
| T480 | messaging, server and portals: the stored "Spanish conversat | done | 1 | 104 min | 22:23Z |  |
| T717 | intake (harness): corrections to T711's two jobs: two_jobs a | done | 1 | 2 h 11 min | 22:21Z |  |
| T607 | site: site.css brought up to the V6 reference (laftly-v6.css | done | 2 | 41 min | 21:57Z | review_missing on 2c1fc315ebe4 (D-3728): not proven reviewed or worker-made / (b) 5cb49e3317bc T607: NOTES entry in the  |
| T663 | picks, jobs and payments (harness): a confirmed pick moves i | BLOCKED | 1 | 30 min | 21:32Z |  |
| T753 | runner (harness): `fable` is a model the plan can name, a ta | done | 1 | 112 min | 20:59Z |  |
| T307 | server and portals: the homeowner portal's sign-in, home pag | done | 2 | 2 h 45 min | 20:33Z | PLAN GAP: tests/security/tests/vault_readers.rs:23 lets only `portals` call `read_own_contact` (line 19 lets only `messa |
| T748 | portals: the help-page form's confirmation promises no reply | done | 1 | 41 min | 20:08Z |  |
| T161 | wording: the WORD-024 voice rule's pressure-word scan, the r | done | 2 | 2 h 1 min | 20:00Z | PLAN GAP: `tests/docs/test_site_copy_blocks.py` must check that the home page and footer show the blocks in `docs/copy/s |
| T607 | site: site.css brought up to the V6 reference (laftly-v6.css | QUEUED | 1 | 2 h 5 min | 19:21Z |  |
| T555 | Decisions digest, piece 10: the next entries after the heade | done | 1 | 2 h 7 min | 17:51Z |  |
| T715 | intake (harness): corrections to T671's walker: alarm answer | done | 3 | 113 min | 17:43Z | PLAN GAP: one TD-INT-52 sentence can't be met by intake as planned: "a two-word first text naming a trade opens its job  |
| T530 | portals: the owner help page and its screened reply box (D-3 | QUEUED | 1 | 117 min | 17:38Z | Review round 2 on head 9cae918e901f6738321e0450589f137ab939da2f: REVIEW-VERDICT: FINDINGS (the one fix round is used). / |
| T724 | harness: Ci's Rust-test jobs, the worker job and the nightly | done | 1 | 50 min | 17:09Z |  |
| T210 | payments: the money view's sales-tax set-aside and manual ex | done | 1 | 53 min | 16:29Z |  |
| T407 | calls, portals and server: the owner-portal safety voice cal | done | 1 | 102 min | 16:01Z |  |
| T541 | payments, portals and adapters: a card saved or changed from | check_failed | 1 | 2 h 16 min | 15:25Z |  |
| T470 | messaging and server: the worker task that sends queued emai | done | 3 | 2 h 18 min | 15:24Z | Fix-round review (after the one allowed findings round) still found a problem at head 1830747e4; no reviewer approved a  |
| T593 | site and server: the home hero in D-3331's wording with the  | done | 2 | 75 min | 15:11Z | Ci: failure; Coverage: cancelled / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / tes |
| T554 | Decisions digest, piece 9: the next entries after the header | done | 1 | 110 min | 15:03Z |  |
| T652 | ops: safe mode starts only on OPS-001's alert kinds, and a d | done | 2 | 112 min | 15:03Z | PLAN GAP: tests/integration/tests/safe_mode.rs:246,281,320,378,407 (owned by T487 and T408; T652's prompt :11 and :47 sa |
| T158 | portals: the owner's pilot scorecard page with go and stop l | done | 1 | 2 h 57 min | 14:37Z |  |
| T540 | portals and server: the owner's release button re-sends an e | done | 1 | 53 min | 13:47Z |  |
| T470 | messaging and server: the worker task that sends queued emai | QUEUED | 2 | 2 h 21 min | 13:03Z |  |
| T593 | site and server: the home hero in D-3331's wording with the  | check_failed | 1 | 95 min | 12:35Z |  |
| T536 | E2e harness: clock-advancing journeys last, a keyed worker,  | done | 2 | 107 min | 12:33Z | PLAN GAP: tests/e2e/j2.ts:150 (owned by T099, `e2e-j2-business`; T644 also lists it). Smallest plan change: make the `no |
| T109 | recovery: backup checks and the nightly encrypted export | BLOCKED | 1 | 20 min | 11:47Z |  |
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
| T161 | wording: the WORD-024 voice rule's pressure-word scan, the r | BLOCKED | 1 | 4 min | 08:41Z |  |
| T456 | picks and server: "Hold for 2 more hours" is stored, honoure | done | 1 | 97 min | 08:35Z |  |
| T216 | site: the hero photo and mark (photography assets) | done | 1 | 97 min | 08:10Z |  |
| T310 | server and portals: the owner home and settings pages and th | done | 1 | 74 min | 07:19Z |  |
| T562 | server and portals: "Why was I charged?" is served with the  | done | 1 | 2 h 43 min | 07:09Z |  |
| T229 | Legal drafts: reading-grade check and plain-language rewrite | done | 1 | 2 h 17 min | 06:59Z |  |
| T506 | Decisions digest, piece 7: the next entries after the header | done | 1 | 102 min | 06:55Z |  |
| T408 | server: a release waits while safe mode is on and goes ahead | done | 3 | 2 h 48 min | 06:17Z | PLAN GAP: kit/prompts/T408.md:54 "Size budget: at most 6 files, 800 changed lines" cannot be met. The prompt's own plan  |

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 324 of 735 (44.1%) · partial 384 · unstarted 27
**Tasks merged (throughput only):** 519 of 707

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 18 | 17 | 0 | 35 |
| ai | 8 | 18 | 3 | 29 |
| businesses | 21 | 36 | 1 | 58 |
| calls | 14 | 11 | 0 | 25 |
| consent | 16 | 19 | 0 | 35 |
| fees | 21 | 31 | 0 | 52 |
| intake | 8 | 35 | 0 | 43 |
| job-cards | 12 | 12 | 0 | 24 |
| licensing | 33 | 15 | 0 | 48 |
| lifecycle | 16 | 12 | 0 | 28 |
| messaging | 23 | 22 | 0 | 45 |
| ops-recovery | 9 | 29 | 16 | 54 |
| privacy | 8 | 23 | 0 | 31 |
| quiet-hours | 3 | 6 | 0 | 9 |
| quotes-pick | 25 | 31 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 11 | 39 | 5 | 55 |
| terms | 27 | 8 | 0 | 35 |
| website | 18 | 10 | 2 | 30 |
| wording | 23 | 9 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-20T04:14:08Z (week)): week: 45.4M used, 148.6M left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-05T04:19:31Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 162 of 289 done
- stage 6: 0 of 65 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing

## Last usage reading
- {"task": "T751", "at": "2026-11-20T05:02:45Z", "input_tokens": 68, "cache_creation_input_tokens": 75100, "cache_read_input_tokens": 1459100, "output_tokens": 16773}
