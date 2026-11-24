# Ask Hava build status

Updated 2026-11-24T14:55:15Z by runner 234ae7f8fe8e (actions).

**Building: T741 in progress since 14:26Z; 661 of 780 done; week 93% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 661 of 780 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 1: T741 claimed 14:26Z, lease until 17:26Z; lane 3: T732 claimed 14:23Z, lease until 17:23Z; lane 4: T122 claimed 13:45Z, lease until 16:45Z; lane 5: T305 claimed 13:13Z, lease until 16:13Z
- **Week:** 93% used, 4.5M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 14:54Z, 0 min before this update
- **Keeper:** last ran 14:55Z, next wake 15:15Z

## Needs attention
- BLOCKED T222: Reviewer over 457819c6bc470ff6f69d271d7ab023fb3c0c25eb ended REVIEW-VERDICT: FINDINGS (hand_in_by too close for a fix round):
1. tests/e2e/j2_full.ts:190 closingChecks omits no-duplicate-message, audit-row-per-status-change and no-sentinel-outside-vault (TD-PRIV-02); j2_full.spec.ts:8 title claims closing checks; D-5641 not-driven list does not name them.
2. D-5641 falsely says no e2e file drives the web chat start (CON-025); tests/e2e/web_chat.spec.ts (T594) does. CON-025 is in T222's Goal and was left undone without valid reason.
3. Worker's PLAN GAP (day-3 DONE / masked call) REFUTED by reviewer: app/server/src/main.rs and lib.rs are shared paths T222 may edit, and a masked-call driver can live in owned tests/e2e/j1_full.ts. Only the 600-line budget is a real limit, which the worker did not argue. D-5642 guesses at the cause.
4. tests/e2e/j2_full.ts:4 cites D-5643, which does not exist in docs/decisions.md.
Reviewer could not run tests (no PostgreSQL there); worker reported smoke.sh and prehandin.sh green.
- BLOCKED T801: plan gap reported: PLAN GAP: app/server/tests/web_chat_stops.rs:572 and :592 (owned by T645) assert a consent text queued to a chat-verified number that never texted in; fixing review finding (web_chat consent must never permit a text, CON-005/CON-025) requires changing them. Also app/server/tests/web_chat_after_code.rs:46,:319,:368 (T649) for the "We'll answer by text" wording. Smallest plan change: let T801 own web_chat_stops.rs (and web_chat_after_code.rs). Prepared fix patch at /tmp/T801-fix-round.patch. Reviewer confirmed the gap for web_chat_stops.rs. Proof: cargo test --workspace --exclude askhava-integra; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T801 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T723 (since 2026-11-24T00:30:33Z, no attempt used; a plan change fixes it): T723: measures 47% of the context window, over its 45% limit
- PLAN GAP T811 (since 2026-11-24T10:37:51Z, no attempt used; a plan change fixes it): T811: measures 48% of the context window, over its 45% limit
- Strict plan check red: 62 problem(s) on tasks still to run (+561 on DONE tasks); first: T402: stored context_pct 29% but it measures 33%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T802 | site (harness): the coverage page says texts work with US nu | check_failed | 2 | 75 min | 14:28Z | Ci: failure; Checks: failure / Checks failed at step: Rust build (the one boundary build) and tests (all but SPLIT_TESTS |
| T732 | server: the business job page's "Withdraw your quote" posts  | resync | 2 | 48 min | 14:04Z | merging main into the reviewed head conflicts in docs/architecture.md; a resync merges it in on this attempt (D-891) |
| T733 | server: the business portal serves the service-area settings | done | 1 | 91 min | 13:29Z |  |
| T410 | portals: every signed-in business page shows the Get help li | done | 3 | 85 min | 13:17Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T732 | server: the business job page's "Withdraw your quote" posts  | check_failed | 1 | 63 min | 12:16Z |  |
| T624 | server: two failed in-process health checks in a row raise o | done | 2 | 95 min | 12:13Z | Ci: failure; Python: failure / Python failed at step: Unit, contract and drill tests (pytest) / T705: about 21877 tokens |
| T802 | site (harness): the coverage page says texts work with US nu | check_failed | 1 | 115 min | 12:06Z |  |
| T305 | server and portals: the scorecard, the intake report, the mo | BLOCKED | 2 | 34 min | 11:41Z | PLAN GAP: outside ownership: app/portals/templates/scorecard.html, app/portals/templates/intake_report.html, app/portals |
| T410 | portals: every signed-in business page shows the Get help li | check_failed | 2 | 101 min | 11:20Z |  |
| T801 | intake and site (harness): with texting on, the web chat onl | BLOCKED | 1 | 2 h 10 min | 10:59Z | PLAN GAP: app/server/tests/web_chat_stops.rs:572 and :592 (owned by T645) assert a consent text queued to a chat-verifie |
| T305 | server and portals: the scorecard, the intake report, the mo | check_failed | 1 | 73 min | 10:28Z |  |
| T814 | portals and server: a used action link pressed again shows i | done | 1 | 2 h 29 min | 10:07Z |  |
| T841 | e2e: seedPick's new businesses hold the fixture licence ROC1 | done | 1 | 97 min | 09:48Z |  |
| T804 | server and portals: a delete or copy request made in the por | done | 2 | 63 min | 08:35Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T410 | portals: every signed-in business page shows the Get help li | check_failed | 1 | 93 min | 07:32Z |  |
| T656 | site: a failed submit on the /request chat shows a focused e | done | 2 | 2 h 1 min | 07:12Z | Reviewer after the findings round still FINDINGS (head 18fdb4a27): app/site/templates/request.html lines 7-10: header co |
| T804 | server and portals: a delete or copy request made in the por | check_failed | 1 | 69 min | 06:37Z |  |
| T779 | ops, server and portals: alerts that do not push show in the | done | 1 | 83 min | 06:27Z |  |
| T728 | intake: a homeowner's "<n> YES" text from her phone picks th | done | 1 | 91 min | 05:32Z |  |
| T676 | tests (harness): J1 runs the real intake conversation instea | done | 1 | 2 h 44 min | 05:29Z |  |
| T656 | site: a failed submit on the /request chat shows a focused e | QUEUED | 1 | 17 min | 04:56Z |  |
| T544 | portals and server: the business team page with its role-cha | done | 1 | 102 min | 04:50Z |  |
| T757 | privacy and ops: "Delete my data" also deletes her kept mess | done | 1 | 43 min | 03:54Z |  |
| T524 | portals and server: the weekly digest and the owner-portal f | done | 1 | 2 h 24 min | 03:35Z |  |
| T222 | Journeys J1 and J2, the full variants: web chat, photos, que | BLOCKED | 3 | 2 h 22 min | 03:00Z | Reviewer after the fix round still ended REVIEW-VERDICT: FINDINGS over 0aedbd3769e83a983d24d694c7abe05f8773a57a: / 1. do |
| T649 | intake, site and server: after the code, the web chat's name | done | 2 | 62 min | 02:40Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T119 | Journey J4 (vendor outage), with closing checks | BLOCKED | 1 | 36 min | 02:36Z | PLAN GAP: outside ownership: tests/e2e/webserver.ts / keyedEnv (webserver.ts:143-177) sets no ASKHAVA_HOMEOWNER_NUMBER;  |
| T813 | businesses and portals (harness): a step-up counts only in t | done | 1 | 59 min | 02:35Z |  |
| T836 | ci and runner workflow (harness): the backup-runner gate ski | done | 2 | 25 min | 00:55Z | PLAN GAP: tests/test_runner_five_lanes.py:176 (test_no_workflow_a_hand_in_starts_cancels_another_lanes_run_when_five_han |
| T737 | portals, ai and server: the owner records the AI provider's  | done | 2 | 112 min | 00:42Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T738 | portals and server: owner settings refusals show an accessib | done | 1 | 85 min | 00:34Z |  |
| T716 | intake: the dispatcher's trade questions at step 5: "Which f | done | 4 | 114 min | 23:57Z | PLAN GAP: outside ownership: app/intake/src/conversation.rs (purge_child, ~348-362), app/intake/src/models.rs (purge_req |
| T649 | intake, site and server: after the code, the web chat's name | check_failed | 1 | 2 h 19 min | 22:52Z |  |
| T819 | portals: her feedback words are scrubbed before they are sto | done | 3 | 85 min | 22:43Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T816 | messaging: the data-request confirm link goes by email to th | done | 1 | 112 min | 22:39Z |  |
| T737 | portals, ai and server: the owner records the AI provider's  | check_failed | 1 | 65 min | 21:59Z |  |
| T745 | intake: a reader for a job's intake answers through `intake: | done | 1 | 2 h 19 min | 21:22Z |  |
| T836 | ci and runner workflow (harness): the backup-runner gate ski | BLOCKED | 1 | 11 min | 20:40Z |  |
| T668 | server: the business job page at /b/jobs/{j} offers "Not a f | done | 1 | 56 min | 20:07Z |  |
| T812 | businesses and portals: a passed step-up records the session | done | 1 | 89 min | 19:31Z |  |
| T730 | messaging: a homeowner who chose email gets each quote updat | done | 4 | 76 min | 19:13Z | PLAN GAP: outside ownership: app/messaging/src/quote_text_sent.rs / Evidence: last_quote_text (quote_text_sent.rs:45-52) |
| T222 | Journeys J1 and J2, the full variants: web chat, photos, que | QUEUED | 2 | 97 min | 17:54Z |  |
| T782 | server: the released job's page shows its real job number | done | 1 | 97 min | 16:56Z |  |
| T121 | Startup key checks, control router absent in live, supply-ch | done | 4 | 100 min | 16:52Z | PLAN GAP: outside ownership: app/portals/src/update_channel.rs / T729 (owner) merged UpdatesForm/ConfirmForm without den |
| T714 | portals: a Confirm link sent by text is marked text-started  | done | 4 | 88 min | 15:49Z | Ci: failure; Checks: failure / Checks failed at step: Rust dependency audit (cargo-audit, cargo-deny) / Warning:   unmai |
| T222 | Journeys J1 and J2, the full variants: web chat, photos, que | QUEUED | 1 | 67 min | 15:38Z |  |

### Plan and harness merges on main
- 13:12Z Plan: T305 owns its three owner-report templates for Ally; PF-T801 review filed (D-5792) (#1446)
- 07:37Z Plan: per-turn token trims, PF-P1d pre-flight fixes, J1 keyboard flake task T846, T119 unblock, hand-offs (D-5693, D-5709, D-5715, D-5716) (#1419)
- 23:24Z Plan: token efficiency and phase-2 pre-flight (D-5676); smoke throughput harness task T843, T836 unblocked (D-5682) (#1412)
- 20:12Z Plan: outside pre-flight fixes for 22 queued tasks, template a11y line, four test-design rows (D-5670) (#1410)
- 19:49Z Plan: pre-flight fixes for ten claimable tasks, J3 licence fixture T841 (D-5657); baseline audit gate deferred (D-5663); outside pre-flight fixes T744/T6
- 16:44Z Plan: T840 update-email entry point and first-quote resend (D-5640); six-task pre-flight fixes (D-5646) (#1402)

## Progress (requirements verified)

**Requirements verified:** 439 of 735 (59.7%) · partial 284 · unstarted 12
**Tasks merged (throughput only):** 661 of 780

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 27 | 8 | 0 | 35 |
| ai | 8 | 18 | 3 | 29 |
| businesses | 34 | 23 | 1 | 58 |
| calls | 17 | 8 | 0 | 25 |
| consent | 20 | 15 | 0 | 35 |
| fees | 28 | 24 | 0 | 52 |
| intake | 12 | 31 | 0 | 43 |
| job-cards | 16 | 8 | 0 | 24 |
| licensing | 40 | 8 | 0 | 48 |
| lifecycle | 22 | 6 | 0 | 28 |
| messaging | 26 | 19 | 0 | 45 |
| ops-recovery | 21 | 28 | 5 | 54 |
| privacy | 13 | 18 | 0 | 31 |
| quiet-hours | 4 | 5 | 0 | 9 |
| quotes-pick | 36 | 20 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 21 | 31 | 3 | 55 |
| terms | 30 | 5 | 0 | 35 |
| website | 24 | 6 | 0 | 30 |
| wording | 30 | 2 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-24T14:12:34Z (week)): week: 189.5M used, 4.5M left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-07T20:39:07Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 304 of 352 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- T741 (lease until 2026-11-24T17:26:45Z)
- lane 3: T732 (lease until 2026-11-24T17:23:32Z)
- lane 4: T122 (lease until 2026-11-24T16:45:59Z)
- lane 5: T305 (lease until 2026-11-24T16:13:43Z)

## Last usage reading
- {"task": "T802", "at": "2026-11-24T14:28:39Z", "cache_read_input_tokens": 907412, "cache_creation_input_tokens": 77863, "output_tokens": 10075, "input_tokens": 69}
