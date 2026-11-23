# Ask Hava build status

Updated 2026-11-23T00:46:09Z by runner fbf6b9f5d0d8 (actions).

**Building: T645 in progress since 23:51Z; 617 of 774 done; week 69% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 617 of 774 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 2: T645 claimed 23:51Z, lease until 02:51Z; lane 3: T832 claimed 23:50Z, lease until 02:50Z; lane 4: T712 claimed 00:09Z, lease until 03:09Z
- **Week:** 69% used, 52.4M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 00:46Z, 0 min before this update
- **Keeper:** last ran 00:42Z, next wake 02:52Z

## Needs attention
- BLOCKED T121: plan gap reported: PLAN GAP: outside ownership: app/portals/src/update_channel.rs
T729 (owner) merged UpdatesForm/ConfirmForm without deny_unknown_fields and confirm_query ignores unknown keys; SEC-010 scan fails on them (F1 x2, F5 x1). Also: REFUSING_TRY_FROM lacks JobTypesForm (business_portal.rs), StepUpForm (business_step_up.rs), Posted<F> (address_changes.rs, business_guarded_changes.rs); CLASSIFIED lacks twilio_answer/vendor_error (adapters base.rs), run_help_notice, spanish_switch latest, photo_asks load; business_signup_route.rs posts roc_number/base_zip/radius/code names the templates lack, so per-route; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T121 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T803: Worker made no change. All 83 named mutants are killed; the Checks job timed out (40 min limit) because the Rust build took ~26 min and mutants.toml changes trigger all 83 mutants (cut off after mutant 72). Needs a harness fix (raise Checks timeout-minutes or run only changed mutants) or a faster CI build. Smoke did not finish in the worker's polls.
- PLAN GAP T121 (since 2026-11-23T00:03:26Z, no attempt used; a plan change fixes it): outside ownership: app/portals/src/update_channel.rs
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- PLAN GAP T837 (since 2026-11-23T00:09:54Z, no attempt used; a plan change fixes it): T837: prompt kit/prompts/T837.md is missing; run scripts/gen_prompts.py; T837: branch claude/task/T837-plan-health is named by its requirements, tests or reads, but neither it nor a dependency owns it and it does not exist yet
- Strict plan check red: 24 problem(s) on tasks still to run (+496 on DONE tasks); first: T624: stored context_pct 12% but it measures 18%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T725 | site: the home page shows the texting number with its consen | MERGE_PENDING | 4 | 49 min | 00:39Z | merging main into the reviewed head conflicts in app/server/src/main.rs; a resync merges it in on this attempt (D-891) |
| T675 | intake and server (harness): the dispatcher asks for photos  | MERGE_PENDING | 4 | 45 min | 00:33Z | merging main into the reviewed head conflicts in app/server/CLAUDE.md; a resync merges it in on this attempt (D-891) |
| T121 | Startup key checks, control router absent in live, supply-ch | BLOCKED | 2 | 12 min | 00:03Z | PLAN GAP: outside ownership: app/portals/src/update_channel.rs / T729 (owner) merged UpdatesForm/ConfirmForm without den |
| T675 | intake and server (harness): the dispatcher asks for photos  | resync | 4 | 81 min | 17:03Z |  |
| T590 | calls, picks and server: the who's-calling and missed-call t | check_failed | 1 | 118 min | 16:40Z | Ci: failure; Coverage: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / test  |
| T122 | docs/account-checklist.md: Part 1 test accounts, Part 2 busi | QUEUED | 1 | 88 min | 16:23Z | Reviewer after the findings round (head d65c4896894680459870359165740cf8b3a2bf57) still ended FINDINGS: / 1. tests/docs/ |
| T725 | site: the home page shows the texting number with its consen | resync | 4 | 82 min | 16:15Z |  |
| T831 | site: the /request start screen's words box sits fully insid | done | 1 | 107 min | 16:13Z |  |
| T729 | portals and privacy: the quote page offers updates by email  | done | 3 | 92 min | 16:10Z | Ci: failure; Coverage: failure / Coverage failed at step: Compare with tests/coverage_floor.json and refuse a lowered fl |
| T397 | server and portals: the owner approves or declines a member- | done | 2 | 2 h 0 min | 15:12Z | PLAN GAP: outside ownership: app/server/tests/door_matrix.rs:1182. owner_jar signs the owner in at key.now + 30s*k per p |
| T833 | pre-hand-in gate test (harness): the Ally-step test holds th | done | 1 | 65 min | 14:05Z |  |
| T117 | docs/runbook.md: outages, safe mode, refunds, disputes, comp | done | 4 | 33 min | 13:54Z | Reviewer after findings round still FINDINGS at 4f79a8b8c4195e68ccfba3a25eaab6d612560536: / 1. docs/runbook.md:139-142 a |
| T830 | server: `server rotate-keys` and `server retire-index-versio | done | 1 | 61 min | 12:46Z |  |
| T729 | portals and privacy: the quote page offers updates by email  | check_failed | 2 | 49 min | 12:26Z |  |
| T610 | site: the D-4246 polish on the Spanish pages, the legal page | done | 2 | 40 min | 12:19Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Crate:     basic |
| T589 | intake and portals: the pick review page is shown in her req | done | 2 | 87 min | 12:04Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Warning:   unmai |
| T675 | intake and server (harness): the dispatcher asks for photos  | BLOCKED | 3 | 2 h 50 min | 11:38Z |  |
| T805 | messaging (harness): the toll-free fallback is refused after | done | 1 | 50 min | 11:32Z |  |
| T829 | server tests (harness): the door matrix's owner sign-ins nev | done | 3 | 46 min | 11:10Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Crate:     basic |
| T725 | site: the home page shows the texting number with its consen | BLOCKED | 3 | 21 min | 10:52Z |  |
| T729 | portals and privacy: the quote page offers updates by email  | resync | 2 | 2 h 32 min | 10:40Z |  |
| T803 | messaging (harness): an opted-out number gets no HELP reply; | BLOCKED | 3 | 25 min | 10:25Z | Ci: failure; Checks: cancelled / Checks failed at step: Named mutants of the files this change touches (scripts/run_name |
| T589 | intake and portals: the pick review page is shown in her req | check_failed | 1 | 53 min | 10:04Z |  |
| T725 | site: the home page shows the texting number with its consen | check_failed | 2 | 57 min | 09:57Z |  |
| T610 | site: the D-4246 polish on the Spanish pages, the legal page | check_failed | 1 | 39 min | 09:00Z |  |
| T118 | Journey J3 (operator) keyboard-only, with closing checks | BLOCKED | 4 | 45 min | 08:54Z | PLAN GAP: outside ownership: app/site/templates/request.html, app/site/static/site.css / At 1280x900 the /request chat s |
| T829 | server tests (harness): the door matrix's owner sign-ins nev | check_failed | 2 | 48 min | 08:40Z |  |
| T609 | site (harness): the D-4246 polish on the footer, how it work | done | 3 | 65 min | 07:14Z | PLAN GAP: tests/e2e/web_chat.spec.ts:123 (owned by T489) uses strict locator main.locator('[lang="es"]'); the footer's E |
| T810 | portals and server: a shared-inbox Confirm press asks for it | done | 3 | 76 min | 06:46Z | PLAN GAP: outside ownership: app/server/tests/confirm_press_code.rs. TD-SEC-82 requires a direct portals.shared_inbox in |
| T118 | Journey J3 (operator) keyboard-only, with closing checks | BLOCKED | 3 | 2 h 17 min | 06:35Z |  |
| T827 | server and portals: the owner and homeowner forms refuse unk | done | 1 | 82 min | 06:12Z |  |
| T829 | server tests (harness): the door matrix's owner sign-ins nev | BLOCKED | 1 | 19 min | 06:03Z |  |
| T828 | ci (harness): gitleaks scans each push's commit range, with  | done | 1 | 84 min | 05:45Z |  |
| T616 | site: strong per-coding ETags with 304 revalidation, a one-y | done | 2 | 39 min | 05:32Z | Ci: failure; Coverage: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / test  |
| T803 | messaging (harness): an opted-out number gets no HELP reply; | check_failed | 2 | 63 min | 05:27Z |  |
| T675 | intake and server (harness): the dispatcher asks for photos  | BLOCKED | 2 | 50 min | 04:44Z |  |
| T542 | portals and server: notice and backup address changes are re | done | 2 | 2 h 42 min | 04:28Z | Reviewer after the findings round still ended REVIEW-VERDICT: FINDINGS at head 4de98e6fb74e1e5394629d7a1abe0694167723a2: |
| T729 | portals and privacy: the quote page offers updates by email  | BLOCKED | 1 | 2 min | 04:19Z |  |
| T609 | site (harness): the D-4246 polish on the footer, how it work | BLOCKED | 2 | 13 min | 04:15Z |  |
| T721 | businesses and portals: Casey counts a check-in PROBLEM repo | done | 1 | 76 min | 04:10Z |  |
| T616 | site: strong per-coding ETags with 304 revalidation, a one-y | check_failed | 1 | 96 min | 03:07Z |  |
| T118 | Journey J3 (operator) keyboard-only, with closing checks | check_failed | 2 | 2 h 19 min | 03:06Z |  |
| T810 | portals and server: a shared-inbox Confirm press asks for it | BLOCKED | 2 | 2 min | 03:05Z |  |
| T810 | portals and server: a shared-inbox Confirm press asks for it | QUEUED | 1 | 39 min | 02:57Z |  |
| T803 | messaging (harness): an opted-out number gets no HELP reply; | check_failed | 1 | 69 min | 02:44Z |  |
| T691 | intake (harness): START into an open request resends the con | done | 2 | 2 h 16 min | 02:20Z | session ended without handing in |
| T759 | intake: "Close which?" expires on a worded reply that is not | done | 1 | 28 min | 02:19Z |  |
| T397 | server and portals: the owner approves or declines a member- | BLOCKED | 1 | 27 min | 02:12Z |  |
| T121 | Startup key checks, control router absent in live, supply-ch | BLOCKED | 1 | 15 min | 01:45Z |  |
| T117 | docs/runbook.md: outages, safe mode, refunds, disputes, comp | BLOCKED | 3 | 49 min | 01:39Z |  |
| T542 | portals and server: notice and backup address changes are re | QUEUED | 1 | 2 h 37 min | 01:29Z |  |
| T725 | site: the home page shows the texting number with its consen | BLOCKED | 1 | 21 min | 01:24Z |  |
| T609 | site (harness): the D-4246 polish on the footer, how it work | BLOCKED | 1 | 2 min | 00:57Z |  |
| T117 | docs/runbook.md: outages, safe mode, refunds, disputes, comp | QUEUED | 2 | 106 min | 00:48Z |  |

### Plan and harness merges on main
- 00:07Z Plan: pre-flight fixes (D-5567); T837 keeper stage gate, job 32 and copy-flag fixes (D-5568) (#1368)
- 17:04Z Plan: unblock T121/T832/T499/T514 (D-5552); pre-flight fixes job 24 (D-5551) (#1363)
- 12:43Z Plan: pre-flight goal fixes for T666, T778, T820, T524, T825, T831 (D-5526) (#1354)
- 11:22Z Plan: unblock T725 (D-5518) and T803 via T832 Checks timeout 60 min (D-5519) (#1347)
- 11:05Z Plan: T831 fits the /request words box in the first screen; T118 depends on it (A11Y-005, D-5502) (#1343)

## Progress (requirements verified)

**Requirements verified:** 403 of 735 (54.8%) · partial 319 · unstarted 13
**Tasks merged (throughput only):** 617 of 774

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 25 | 10 | 0 | 35 |
| ai | 7 | 19 | 3 | 29 |
| businesses | 29 | 28 | 1 | 58 |
| calls | 17 | 8 | 0 | 25 |
| consent | 17 | 18 | 0 | 35 |
| fees | 27 | 25 | 0 | 52 |
| intake | 11 | 32 | 0 | 43 |
| job-cards | 16 | 8 | 0 | 24 |
| licensing | 36 | 12 | 0 | 48 |
| lifecycle | 21 | 7 | 0 | 28 |
| messaging | 26 | 19 | 0 | 45 |
| ops-recovery | 17 | 31 | 6 | 54 |
| privacy | 11 | 20 | 0 | 31 |
| quiet-hours | 4 | 5 | 0 | 9 |
| quotes-pick | 34 | 22 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 14 | 38 | 3 | 55 |
| terms | 30 | 5 | 0 | 35 |
| website | 22 | 8 | 0 | 30 |
| wording | 29 | 3 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-23T00:04:03Z (week)): week: 141.6M used, 52.4M left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-07T17:38:21Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 260 of 346 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 2: T645 (lease until 2026-11-23T02:51:34Z)
- lane 3: T832 (lease until 2026-11-23T02:50:02Z)
- lane 4: T712 (lease until 2026-11-23T03:09:54Z)

## Last usage reading
- {"task": "T725", "at": "2026-11-23T00:39:13Z", "cache_read_input_tokens": 2441130, "cache_creation_input_tokens": 89591, "input_tokens": 117, "output_tokens": 17451}
