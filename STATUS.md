# Ask Hava build status

Updated 2026-11-26T01:58:22Z by runner 0f6466a20c23 (actions).

**Waiting for tokens until 17:00Z; 672 of 781 done; week 97% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 672 of 781 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** idle since 01:43Z (no task in progress)
- **Week:** 97% used, 0.0M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 01:57Z, 0 min before this update
- **Keeper:** last ran 01:57Z, next wake 07:37Z

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
- no attempt ended in the last 24 hours

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 488 of 735 (66.4%) · partial 239 · unstarted 8
**Tasks merged (throughput only):** 672 of 781

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
| ops-recovery | 35 | 18 | 1 | 54 |
| privacy | 15 | 16 | 0 | 31 |
| quiet-hours | 4 | 5 | 0 | 9 |
| quotes-pick | 40 | 16 | 0 | 56 |
| recurring | 10 | 1 | 0 | 11 |
| security | 25 | 27 | 3 | 55 |
| terms | 35 | 0 | 0 | 35 |
| website | 27 | 3 | 0 | 30 |
| wording | 31 | 1 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-26T01:44:15Z (week)): week: 194.0M used, 0k left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-08T22:06:30Z; waiting until 2026-11-26T17:00:00Z.

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
- nothing

## Last usage reading
- {"task": "T123", "at": "2026-11-25T01:33:41Z", "cache_creation_input_tokens": 98373, "input_tokens": 128, "cache_read_input_tokens": 2165304, "output_tokens": 23531}
