# Ask Hava build status

Updated 2026-11-20T13:05:41Z by runner c91fee70b2df (manual).

**Building: T670 in progress since 12:59Z; 536 of 710 done; week 31% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 536 of 710 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T670 claimed 12:59Z, lease until 15:59Z; lane 2: T496 claimed 11:44Z, lease until 14:44Z; lane 3: T727 claimed 11:55Z, lease until 14:55Z; lane 4: T667 claimed 12:55Z, lease until 15:55Z; lane 5: T767 claimed 12:53Z, lease until 15:53Z; lane 6: T530 claimed 10:55Z, lease until 13:55Z; lane 7: T672 claimed 12:39Z, lease until 15:39Z
- **Week:** 31% used, 125.6M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 13:05Z, 0 min before this update
- **Keeper:** last ran 12:31Z, next wake 12:51Z

## Needs attention
- BLOCKED T573: Ci: failure; Coverage: cancelled
Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly)
test pages::tests::web_009_the_request_box_posts_with_a_label_and_the_token ... ok
test pages::tests::waits_read_in_minutes_then_hours ... ok
test pages::tests::web_020_city_guide_reads_its_three_settings ... ok
test routes::tests::td_web_01_banner_hidden_by_default_and_shown_when_on ... ok
test routes::tests::td_web_01_site_page_needs_no_script ... ok
test routes::tests::another_method_is_405_with_allow ... ok
test tests::site_copy_blocks::d_3890_the_start_line_matches_its_chat_sentence_while_it_names_the_number ... ok
test routes::tests::the_stylesheet_is_served_and_unknown_names_are_not ... ok
test tests::site_copy_blocks::word_029_the_block_check_fails_planted_differences ... ok
test tests::site_copy_blocks::word_029_every_home_and_footer_block_shows_on_the_page ... ok
test chat::tests::ai_line_follows_the_switch_and_consent_comes_first ... ok
test chat::tests::number_step_without_the_part_sends_and_stores_nothing ... ok
test tests::who_runs::td_web_10_record_link_and_license_follow_their_settings ... ok
test result: ok. 41 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.29s
##[error]The operation was canceled.
Ci failed at step: All five jobs passed
Checks: success, Lint and slow tests: success, Python: success, Coverage: cancelled, E2e: success
##[error]Process completed with exit code 1.
- BLOCKED T614: plan gap reported: PLAN GAP: tests/a11y/reflow_spacing.spec.ts:49-54 (T053, protected) flags every TARGETS control with no box as "is hidden", but DESIGN_SPEC docs/design/DESIGN_SPEC.md:102 requires .site-nav hidden below 1024px and Menu hidden from 1024px, so the specified header fails at every width. Also: closed <details> Menu links keep boxes in Chromium (counted as Tab stops by j1_keyboard.spec.ts:56 and j2_keyboard.spec.ts:26) and the open Menu scrolls sideways at 320px (scrollWidth 375); both fixable only in app/site/static/site.css, not owned by T614 (CSP forbids inline style). Smallest plan change: a ta; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T614 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T206 (since 2026-11-20T12:53:17Z, no attempt used; a plan change fixes it): T206: dependency T648 is DONE but its owned file app/recovery/src/restore.rs is not on main; T206: dependency T648 is DONE but its owned file tests/recovery/tests/export_restores.rs is not on main
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- PLAN GAP T524 (since 2026-11-20T12:53:17Z, no attempt used; a plan change fixes it): T524: dependency T648 is DONE but its owned file app/recovery/src/restore.rs is not on main; T524: dependency T648 is DONE but its owned file tests/recovery/tests/export_restores.rs is not on main
- Strict plan check red: 39 problem(s) on tasks still to run (+406 on DONE tasks); first: T410: stored context_pct 15% but it measures 20%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T648 | recovery: the monthly test restore into a throwaway cluster, | done | 2 | 112 min | 12:29Z | PLAN GAP: app/recovery/src/restore.rs:44 include_str!("../../../scripts/bootstrap_roles.sql") is required by the goal, b |
| T573 | messaging and server: queued business texts are addressed to | check_failed | 3 | 70 min | 12:25Z | model claude-opus-5-5 ran, task names claude-fable-5-2 |
| T614 | site: the V6 header in base.html (links, the Menu in the pag | BLOCKED | 1 | 16 min | 11:39Z | PLAN GAP: tests/a11y/reflow_spacing.spec.ts:49-54 (T053, protected) flags every TARGETS control with no box as "is hidde |
| T463 | businesses and server: "Busy for <n> days" pauses job cards  | done | 1 | 58 min | 11:16Z |  |
| T768 | runner tests (harness): the seed test fakes free disk, so a  | done | 1 | 22 min | 10:38Z |  |
| T573 | messaging and server: queued business texts are addressed to | QUEUED | 2 | 53 min | 09:48Z |  |
| T530 | portals: the owner help page and its screened reply box (D-3 | QUEUED | 2 | 2 h 25 min | 09:46Z | model claude-opus-5-5 ran, task names claude-fable-5-2 |
| T727 | Runner (harness): a retry after a review failure or a plan-f | check_failed | 1 | 75 min | 09:38Z | Ci: failure; Coverage: cancelled / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / tes |
| T617 | site: DM Sans from one weight-axis variable file (smaller th | done | 2 | 2 h 16 min | 09:22Z | PLAN GAP: tests/a11y/design_tokens.spec.ts:221 and :242 (protected, not owned by T617) contradict the prompt's single DM |
| T766 | server tests (harness): a photo-link door and a {job} fill i | done | 1 | 53 min | 09:05Z |  |
| T116 | Stage 0.5 static page set and the DNS records file | done | 1 | 50 min | 08:48Z |  |
| T669 | picks: "Can't take it" and a business leaving end a pick in  | done | 2 | 80 min | 08:32Z | runner smoke red on c403ea4898ac (bash scripts/smoke.sh, D-3530): / line = cache.seed(tmp_path, smoke, debug, lambda arg |
| T672 | intake: the dispatcher asks "How soon?", the night count and | QUEUED | 2 | 89 min | 08:05Z | runner smoke red on 3e9315b7399c (bash scripts/smoke.sh, D-3530): / line = cache.seed(tmp_path, smoke, debug, lambda arg |
| T561 | server and portals: the homeowner's "Download my data" and h | done | 1 | 33 min | 07:51Z |  |
| T762 | runner and workflow (harness): fix-round workers commit with | done | 1 | 68 min | 07:44Z |  |
| T764 | ci and pre-hand-in gate (harness): scripts/bootstrap_roles.s | done | 1 | 54 min | 07:29Z |  |
| T669 | picks: "Can't take it" and a business leaving end a pick in  | QUEUED | 1 | 56 min | 07:07Z |  |
| T573 | messaging and server: queued business texts are addressed to | BLOCKED | 1 | 5 min | 06:27Z |  |
| T561 | server and portals: the homeowner's "Download my data" and h | resync | 1 | 60 min | 06:26Z |  |
| T765 | recovery (tests): the backup tests' fake binaries can no lon | done | 1 | 65 min | 06:11Z |  |
| T751 | stand-ins (harness): the Stripe stand-in detaches a payment  | done | 1 | 51 min | 05:02Z |  |
| T726 | Runner (harness): a failed attempt's unpushed work can be ke | done | 2 | 36 min | 05:00Z | merging main into the reviewed head conflicts in kit/plan.lock, kit/runner.json; a resync merges it in on this attempt ( |
| T543 | picks, businesses, portals and server: "Away until" is store | done | 1 | 64 min | 04:57Z |  |
| T434 | portals: the monthly intake accuracy report page and the mon | done | 1 | 112 min | 04:55Z |  |
| T110 | recovery: replay with derived IDs | done | 1 | 2 h 36 min | 04:55Z |  |
| T663 | picks, jobs and payments (harness): a confirmed pick moves i | done | 3 | 65 min | 04:54Z | Ci: failure; Coverage: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / test  |
| T556 | Decisions digest, piece 11: the next entries after the heade | done | 1 | 51 min | 04:50Z |  |
| T726 | Runner (harness): a failed attempt's unpushed work can be ke | resync | 2 | 32 min | 04:08Z |  |
| T747 | intake and jobs: the homeowner's INT-029 emergency choice ma | done | 1 | 71 min | 03:25Z |  |
| T726 | Runner (harness): a failed attempt's unpushed work can be ke | resync | 2 | 2 h 16 min | 03:22Z |  |
| T617 | site: DM Sans from one weight-axis variable file (smaller th | BLOCKED | 1 | 2 min | 03:04Z |  |
| T648 | recovery: the monthly test restore into a throwaway cluster, | BLOCKED | 1 | 21 min | 02:55Z |  |
| T763 | tests (harness): the web chat spec's fake numbers never coll | done | 1 | 25 min | 02:17Z |  |
| T672 | intake: the dispatcher asks "How soon?", the night count and | BLOCKED | 1 | 13 min | 02:12Z |  |
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
| T530 | portals: the owner help page and its screened reply box (D-3 | QUEUED | 1 | 117 min | 17:38Z |  |
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

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 254 of 735 (34.6%) · partial 460 · unstarted 21
**Tasks merged (throughput only):** 536 of 710

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 16 | 19 | 0 | 35 |
| ai | 7 | 19 | 3 | 29 |
| businesses | 20 | 37 | 1 | 58 |
| calls | 11 | 14 | 0 | 25 |
| consent | 11 | 24 | 0 | 35 |
| fees | 21 | 31 | 0 | 52 |
| intake | 7 | 36 | 0 | 43 |
| job-cards | 12 | 12 | 0 | 24 |
| licensing | 30 | 18 | 0 | 48 |
| lifecycle | 13 | 15 | 0 | 28 |
| messaging | 22 | 23 | 0 | 45 |
| ops-recovery | 6 | 34 | 14 | 54 |
| privacy | 8 | 23 | 0 | 31 |
| quiet-hours | 3 | 6 | 0 | 9 |
| quotes-pick | 22 | 34 | 0 | 56 |
| recurring | 9 | 2 | 0 | 11 |
| security | 10 | 42 | 3 | 55 |
| terms | 10 | 25 | 0 | 35 |
| website | 7 | 23 | 0 | 30 |
| wording | 9 | 23 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-20T12:30:43Z (week)): week: 68.4M used, 125.6M left, resets 2026-11-26T17:00:00Z; projected finish 2026-11-29T18:46:46Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 179 of 292 done
- stage 6: 0 of 65 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T670 (lease until 2026-11-20T15:59:44Z)
- lane 2: T496 (lease until 2026-11-20T14:44:53Z)
- lane 3: T727 (lease until 2026-11-20T14:55:28Z)
- lane 4: T667 (lease until 2026-11-20T15:55:37Z)
- lane 5: T767 (lease until 2026-11-20T15:53:17Z)
- lane 6: T530 (lease until 2026-11-20T13:55:17Z)
- lane 7: T672 (lease until 2026-11-20T15:39:52Z)

## Last usage reading
- {"task": "T648", "at": "2026-11-20T12:29:39Z", "cache_read_input_tokens": 9662071, "input_tokens": 207, "cache_creation_input_tokens": 385613, "output_tokens": 116104}
