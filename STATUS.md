# Ask Hava build status

Updated 2026-11-25T01:35:13Z by runner c4d284e73197 (actions).

**Waiting for tokens until 17:00Z; 671 of 781 done; week 97% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 671 of 781 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** idle since 01:33Z (no task in progress)
- **Week:** 97% used, 0.0M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 01:34Z, 0 min before this update
- **Keeper:** last ran 01:31Z, next wake 03:09Z

## Needs attention
- BLOCKED T222: Reviewer over 457819c6bc470ff6f69d271d7ab023fb3c0c25eb ended REVIEW-VERDICT: FINDINGS (hand_in_by too close for a fix round):
1. tests/e2e/j2_full.ts:190 closingChecks omits no-duplicate-message, audit-row-per-status-change and no-sentinel-outside-vault (TD-PRIV-02); j2_full.spec.ts:8 title claims closing checks; D-5641 not-driven list does not name them.
2. D-5641 falsely says no e2e file drives the web chat start (CON-025); tests/e2e/web_chat.spec.ts (T594) does. CON-025 is in T222's Goal and was left undone without valid reason.
3. Worker's PLAN GAP (day-3 DONE / masked call) REFUTED by reviewer: app/server/src/main.rs and lib.rs are shared paths T222 may edit, and a masked-call driver can live in owned tests/e2e/j1_full.ts. Only the 600-line budget is a real limit, which the worker did not argue. D-5642 guesses at the cause.
4. tests/e2e/j2_full.ts:4 cites D-5643, which does not exist in docs/decisions.md.
Reviewer could not run tests (no PostgreSQL there); worker reported smoke.sh and prehandin.sh green.
- BLOCKED T801: plan gap reported: PLAN GAP: app/server/tests/web_chat_stops.rs:572 and :592 (owned by T645) assert a consent text queued to a chat-verified number that never texted in; fixing review finding (web_chat consent must never permit a text, CON-005/CON-025) requires changing them. Also app/server/tests/web_chat_after_code.rs:46,:319,:368 (T649) for the "We'll answer by text" wording. Smallest plan change: let T801 own web_chat_stops.rs (and web_chat_after_code.rs). Prepared fix patch at /tmp/T801-fix-round.patch. Reviewer confirmed the gap for web_chat_stops.rs. Proof: cargo test --workspace --exclude askhava-integra; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T801 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T799: plan gap reported: PLAN GAP: contradiction: D-5401 requires es.json text "dentro de 72 horas de recibir los datos del cliente" (5 keys), but tests/docs/test_introduction_fee_wording.py:60 forbids the pattern `\bdatos de(l)? clientes?\b` (WORD-026); test_td_word_10[app/messaging/templates/es.json] fails with 4 hits (the negated use in unreachable_no_attempts is allowed). That test is protected and not owned by T799. Reviewer (a4e70d1f2bbf42da0) confirmed on head a34a900bb2a5. Fix: change the Spanish phrase (e.g. "datos del propietario", recount segments) or add an exemption via a harness task. All other changes m; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T799 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T723 (since 2026-11-24T00:30:33Z, no attempt used; a plan change fixes it): T723: measures 48% of the context window, over its 45% limit
- PLAN GAP T811 (since 2026-11-24T10:37:51Z, no attempt used; a plan change fixes it): T811: measures 48% of the context window, over its 45% limit
- Strict plan check red: 67 problem(s) on tasks still to run (+576 on DONE tasks); first: T402: stored context_pct 29% but it measures 33%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T123 | docs/owner-guide.md: how everything works, daily and weekly  | MERGE_PENDING | 2 | 86 min | 01:33Z | Reviewer after the findings round (head abc739335b85acdcead6902a51518ce39a82a4bd) still ended FINDINGS: / 1. docs/owner- |
| T820 | server and portals: "Download my data" also holds withdrawn  | QUEUED | 2 | 2 h 28 min | 01:30Z | Hand-in time passed. Worker added unit tests in app/portals/src/data_download.rs (commit f4a102dca); portals-only covera |
| T123 | docs/owner-guide.md: how everything works, daily and weekly  | QUEUED | 1 | 53 min | 00:00Z |  |
| T826 | site: Spanish page fixes (ranking line, launch offer, city l | done | 2 | 2 h 30 min | 22:57Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T815 | portals: the owner portal serves the weekly digest view at / | done | 2 | 89 min | 21:22Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T839 | intake, privacy and messaging: an under-13 purge also delete | done | 1 | 93 min | 20:15Z |  |
| T820 | server and portals: "Download my data" also holds withdrawn  | check_failed | 1 | 71 min | 19:49Z |  |
| T815 | portals: the owner portal serves the weekly digest view at / | check_failed | 1 | 81 min | 19:28Z |  |
| T846 | portals and a11y tests (harness): the quote card a "Go back" | done | 1 | 29 min | 19:15Z |  |
| T826 | site: Spanish page fixes (ranking line, launch offer, city l | check_failed | 1 | 43 min | 18:56Z |  |
| T799 | Templates and tests (harness): copy review fixes (refund att | BLOCKED | 1 | 9 min | 18:29Z | PLAN GAP: contradiction: D-5401 requires es.json text "dentro de 72 horas de recibir los datos del cliente" (5 keys), bu |
| T741 | server and intake: an opted-out homeowner's "<n> YES" picks  | done | 2 | 73 min | 17:54Z | Ci: failure; E2e: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / {"timestamp":"2026-11-24T |
| T590 | calls, picks and server: the who's-calling and missed-call t | done | 3 | 23 min | 17:28Z | Ci: failure; E2e: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / {"timestamp":"2026-11-24T |
| T590 | calls, picks and server: the who's-calling and missed-call t | check_failed | 2 | 74 min | 16:23Z |  |
| T741 | server and intake: an opted-out homeowner's "<n> YES" picks  | check_failed | 1 | 78 min | 15:45Z |  |
| T802 | site (harness): the coverage page says texts work with US nu | done | 3 | 23 min | 15:26Z | Ci: failure; Checks: failure / Checks failed at step: Rust build (the one boundary build) and tests (all but SPLIT_TESTS |
| T732 | server: the business job page's "Withdraw your quote" posts  | done | 2 | 49 min | 15:13Z | merging main into the reviewed head conflicts in docs/architecture.md; a resync merges it in on this attempt (D-891) |
| T122 | docs/account-checklist.md: Part 1 test accounts, Part 2 busi | done | 2 | 74 min | 15:00Z | Reviewer after the findings round (head d65c4896894680459870359165740cf8b3a2bf57) still ended FINDINGS: / 1. tests/docs/ |
| T305 | server and portals: the scorecard, the intake report, the mo | done | 3 | 102 min | 14:55Z | PLAN GAP: outside ownership: app/portals/templates/scorecard.html, app/portals/templates/intake_report.html, app/portals |
| T802 | site (harness): the coverage page says texts work with US nu | check_failed | 2 | 75 min | 14:28Z |  |
| T732 | server: the business job page's "Withdraw your quote" posts  | resync | 2 | 48 min | 14:04Z |  |
| T733 | server: the business portal serves the service-area settings | done | 1 | 91 min | 13:29Z |  |
| T410 | portals: every signed-in business page shows the Get help li | done | 3 | 85 min | 13:17Z | Ally: failure / Ally failed at step: The accessibility suite (axe at 320, 768 and 1280 px, keyboard, reflow, spacing) /  |
| T732 | server: the business job page's "Withdraw your quote" posts  | check_failed | 1 | 63 min | 12:16Z |  |
| T624 | server: two failed in-process health checks in a row raise o | done | 2 | 95 min | 12:13Z | Ci: failure; Python: failure / Python failed at step: Unit, contract and drill tests (pytest) / T705: about 21877 tokens |
| T802 | site (harness): the coverage page says texts work with US nu | check_failed | 1 | 115 min | 12:06Z |  |
| T305 | server and portals: the scorecard, the intake report, the mo | BLOCKED | 2 | 34 min | 11:41Z |  |
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

### Plan and harness merges on main
- 22:18Z Plan: harness task T847 for the J3 step-5 race (D-5809); Sonnet for T612 (D-5810); hand-off, ChatGPT queue and plan drafts (#1458)
- 17:12Z Plan: PF-Q1 pre-flight fixes for T817 and T843; PF-Q1, PF-T222, PF-T801b, PF-CLK reports and Fable's scope decision filed (D-5793) (#1448)
- 13:12Z Plan: T305 owns its three owner-report templates for Ally; PF-T801 review filed (D-5792) (#1446)
- 07:37Z Plan: per-turn token trims, PF-P1d pre-flight fixes, J1 keyboard flake task T846, T119 unblock, hand-offs (D-5693, D-5709, D-5715, D-5716) (#1419)

## Progress (requirements verified)

**Requirements verified:** 483 of 735 (65.7%) · partial 244 · unstarted 8
**Tasks merged (throughput only):** 671 of 781

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 28 | 7 | 0 | 35 |
| ai | 11 | 15 | 3 | 29 |
| businesses | 36 | 21 | 1 | 58 |
| calls | 17 | 8 | 0 | 25 |
| consent | 23 | 12 | 0 | 35 |
| fees | 29 | 23 | 0 | 52 |
| intake | 14 | 29 | 0 | 43 |
| job-cards | 17 | 7 | 0 | 24 |
| licensing | 41 | 7 | 0 | 48 |
| lifecycle | 23 | 5 | 0 | 28 |
| messaging | 27 | 18 | 0 | 45 |
| ops-recovery | 33 | 20 | 1 | 54 |
| privacy | 15 | 16 | 0 | 31 |
| quiet-hours | 4 | 5 | 0 | 9 |
| quotes-pick | 40 | 16 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 25 | 27 | 3 | 55 |
| terms | 35 | 0 | 0 | 35 |
| website | 25 | 5 | 0 | 30 |
| wording | 30 | 2 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-25T00:22:15Z (week)): week: 194.0M used, 33k left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-07T21:33:35Z; waiting until 2026-11-26T17:00:00Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 314 of 353 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing

## Last usage reading
- {"task": "T123", "at": "2026-11-25T01:33:41Z", "cache_creation_input_tokens": 98373, "input_tokens": 128, "cache_read_input_tokens": 2165304, "output_tokens": 23531}
