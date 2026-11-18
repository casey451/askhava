# Ask Hava build status

Updated 2026-11-18T18:27:21Z by runner ac95475ba313 (actions).

**Building: T483 in progress since 18:25Z; 453 of 645 done; week 64% used, resets Thu 2026-11-19 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 453 of 645 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 3: T483 claimed 18:25Z, lease until 21:25Z; lane 7: T711 claimed 18:12Z, lease until 21:12Z
- **Week:** 64% used, 63.6M tokens left; resets Thu 2026-11-19 17:00Z (10:00 am Phoenix)
- **Last run:** 18:27Z, 0 min before this update
- **Keeper:** last ran 18:25Z, next wake 18:45Z

## Needs attention
- BLOCKED T205: plan gap reported: PLAN GAP: `payments` has no public reader that ties a dispute to its pick (or its charge, business or job). Without one, T205 cannot narrow `waiting_disputes` or refuse a press on another pick's dispute, so this has to go back as a `payments` follow-up.
Evidence (confirmed by a second reviewer): list_disputes (app/payments/src/disputes.rs:362-388) and DisputeView (app/payments/src/models.rs:621-634) carry no pick, payment-intent, business or job; refund_state (app/payments/src/refund_state.rs:39-45) gives only `disputed`, not the dp_ id; EvidencePack fields are pub(crate) (models.rs:571-580); ; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T205 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T408: plan gap reported: PLAN GAP: kit/prompts/T408.md:54 "Size budget: at most 6 files, 800 changed lines" cannot be met. The prompt's own plan changes (D-3144, D-3879, D-4026, D-4326) require edits to at least 13 files (release_sink.rs, new release_hold.rs, release_proof.rs, server lib.rs, payments api.rs, server tests/safe_mode_release.rs, integration safe_mode.rs, ops safe_mode.rs, three NOTES.md, architecture.md, decisions.md), and tests for every held_release money branch (Stripe fetch failure, pick ended while held, no fee row) plus address_in through its registered handler, which the first review required (95%; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T408 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T563: plan gap reported: PLAN GAP: app/messaging/src/optout.rs:311 (suppress/record, owned by T029 and later tasks; T563 owns no messaging file) and kit/prompts/T563.md:9 (CROSS_SCHEMA_CALLS fixed at exactly one entry, messaging.last_message_at(). The sweep (head d7eb5dc64, all TD-CON-30 tests green, smoke and prehandin green) checks suppression via messaging::api::send_window before its per-number transaction, then clears the vault phone inside it. Review finding (unresolved): an opt-out via messaging::api::suppress (portal/email/owner paths) or a texted STOP committing between that check and the clear lets the sweep; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T563 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T583: plan gap reported: PLAN GAP: kit/prompts/T583.md:51 (budget of 450 changed lines in 3 files) The smallest plan change: raise T583's budget to 650 changed lines, or move `tests/integration/tests/quote_update_texts.rs` into a test-only follow-up task with its own budget.
Evidence: `git diff --numstat origin/main...HEAD` on owned files at head a3034e2b1: app/intake/src/job_texts.rs 5/5, app/intake/src/quote_updates.rs 292/0, tests/integration/tests/quote_update_texts.rs 377/0 (679 lines). Fix-round worker's honest trim reached about 273 + 8 + ~350 = ~630 lines (test still failing clippy: a 73-line function needing ; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T583 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T645: plan gap reported: PLAN GAP: app/site/src/chat.rs:340 (owned by T593, then T646 and T649; T593 is not merged yet) calls `chat_message(db, &secret, &key, &msg, self.intake)` with no `Vault`, and the `ChatKeys` trait (chat.rs:62-66) only exposes secret/send/check. `messaging::api::suppress` (app/messaging/src/optout.rs:311-317) needs a `&Vault` and the phone, since digests come from `vault.blind_index` (optout.rs:441-443). `chat_message` (app/intake/src/web_chat.rs:247-253) takes no vault and its `Settings` is `Copy`, so T645 cannot list the session's number (step 1, TD-CON-36 "at once") without a signature change; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T645 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- Strict plan check red: 34 problem(s) on tasks still to run (+258 on DONE tasks); first: T402: measures 48% of the window, over its 45% limit; trim its reads or split it

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T624 | server: two failed in-process health checks in a row raise o | MERGE_PENDING | 1 | 96 min | 18:26Z | merging main into the reviewed head conflicts in app/server/src/bin/worker.rs; a resync merges it in on this attempt (D- |
| T205 | portals: owner refunds with a review step and the audit log  | BLOCKED | 1 | 58 min | 18:23Z | PLAN GAP: `payments` has no public reader that ties a dispute to its pick (or its charge, business or job). Without one, |
| T500 | Decisions digest, piece 1: from the first entry, at most 300 | done | 1 | 71 min | 18:18Z |  |
| T671 | intake (harness): a homeowner's texts after the consent text | done | 2 | 102 min | 17:25Z | PLAN GAP: three tests outside T671's scope fail because of the new behaviour (worker edited none of them): / 1. tests/in |
| T621 | portals: the scorecard's go and stop line settings and its p | done | 1 | 38 min | 17:03Z |  |
| T720 | Decision digest prerequisites (harness): the digest plan and | done | 1 | 67 min | 16:51Z |  |
| T624 | server: two failed in-process health checks in a row raise o | resync | 1 | 49 min | 16:38Z |  |
| T592 | jobs, quotes and site: the home page's numbers read every jo | done | 1 | 40 min | 16:19Z |  |
| T645 | intake and messaging: the web chat honours an opt-out word,  | BLOCKED | 1 | 4 min | 16:05Z | PLAN GAP: app/site/src/chat.rs:340 (owned by T593, then T646 and T649; T593 is not merged yet) calls `chat_message(db, & |
| T621 | portals: the scorecard's go and stop line settings and its p | resync | 1 | 55 min | 15:55Z |  |
| T665 | server tests (harness): the door matrix fills a business job | done | 1 | 81 min | 15:47Z |  |
| T525 | server and ops: the hourly vault-read check and the daily da | done | 1 | 102 min | 15:41Z |  |
| T200 | ops and portals: the owner help page with its screened reply | done | 2 | 2 h 25 min | 14:32Z | Review not approved after the one fix round (head 346acc872b66e2a8cd86b345b919fe4573f7db87). / Remaining finding (review |
| T671 | intake (harness): a homeowner's texts after the consent text | BLOCKED | 1 | 44 min | 13:58Z |  |
| T472 | server (harness): serve the "Can't take it" page and button  | done | 1 | 62 min | 12:45Z |  |
| T662 | jobs, intake, calls and server: a masked call line closes wh | done | 1 | 73 min | 12:39Z |  |
| T679 | quotes and businesses: a launch-open category takes requests | done | 1 | 37 min | 12:17Z |  |
| T583 | intake and server: the quote-count update text is sent, at m | BLOCKED | 1 | 53 min | 12:13Z | PLAN GAP: kit/prompts/T583.md:51 (budget of 450 changed lines in 3 files) The smallest plan change: raise T583's budget  |
| T700 | Runner (harness): a merge-line task whose turn ran out goes  | done | 1 | 29 min | 12:08Z |  |
| T200 | ops and portals: the owner help page with its screened reply | QUEUED | 1 | 2 h 57 min | 12:02Z |  |
| T408 | payments and server: a release waits while safe mode is on a | BLOCKED | 2 | 2 h 21 min | 11:34Z | PLAN GAP: kit/prompts/T408.md:54 "Size budget: at most 6 files, 800 changed lines" cannot be met. The prompt's own plan  |
| T563 | intake and privacy: the daily consent sweep removes consent  | BLOCKED | 1 | 85 min | 11:21Z | PLAN GAP: app/messaging/src/optout.rs:311 (suppress/record, owned by T029 and later tasks; T563 owns no messaging file)  |
| T658 | privacy: the scrubber removes lower-case Spanish addresses a | done | 2 | 51 min | 11:04Z | Fix-round review of head 18e4fd102f38620e11d1055ecb4d77813e2b2965 returned FINDINGS after the one allowed fix round (fir |
| T650 | intake: the under-13 stop also deletes the request's job pho | done | 1 | 90 min | 10:39Z |  |
| T641 | Runner (harness): merge train depth 2, the next branch's upd | done | 1 | 55 min | 10:29Z |  |
| T644 | tests (harness): the remaining Playwright specs and helpers  | done | 1 | 43 min | 09:48Z |  |
| T660 | quotes (harness): the quote-note AI screener takes scrubbed  | done | 1 | 56 min | 08:52Z |  |
| T658 | privacy: the scrubber removes lower-case Spanish addresses a | QUEUED | 1 | 89 min | 08:11Z |  |
| T640 | harness: Ci handles the merge train's throwaway refs and cou | done | 1 | 64 min | 08:02Z |  |
| T619 | quotes, picks and intake: the quote-median, lapsed-pick, win | done | 2 | 40 min | 08:02Z | T619 readers are correct per three Opus reviews, but the test file still misses wrong implementations; no reviewer appro |
| T482 | messaging and intake: the plain-word auto-answers after HELP | done | 1 | 2 h 29 min | 07:56Z |  |
| T643 | portals (harness): a real browser's same-origin form POST pa | done | 1 | 2 h 24 min | 07:49Z |  |
| T642 | Runner (harness): the keeper starts a run for each free lane | done | 1 | 83 min | 06:05Z |  |
| T601 | businesses and quotes: the card pool reads every service are | done | 1 | 50 min | 05:14Z |  |
| T523 | jobs, ops and intake: the check-in answer recorder and the c | done | 3 | 46 min | 05:06Z | Review not approved after the one findings round (head bd99856fca96fb81973c28181ad9fd6e39e7ac91; flag_inputs tests 5 pas |
| T638 | harness: Ci skips a compile its job already passed on a prov | done | 1 | 88 min | 04:35Z |  |
| T623 | payments: a Stripe key refusal (401 or 403) takes the Stripe | done | 1 | 63 min | 03:46Z |  |
| T151 | ops and messaging: HELP requests with one owner notification | done | 1 | 34 min | 03:40Z |  |
| T647 | payments: rebuild_charge reports Booked inside a caller's tr | done | 1 | 82 min | 03:35Z |  |
| T619 | quotes, picks and intake: the quote-median, lapsed-pick, win | QUEUED | 1 | 69 min | 03:31Z |  |
| T639 | Runner (harness): the smoke warm-up is skipped when the pre- | done | 1 | 76 min | 03:14Z |  |
| T108 | portals: owner tools for pauses, fee mode, categories and se | done | 1 | 2 h 45 min | 03:08Z |  |
| T151 | ops and messaging: HELP requests with one owner notification | resync | 1 | 2 h 14 min | 02:26Z |  |
| T618 | Harness: a 4-hourly advisory watch on main that alerts Casey | done | 1 | 28 min | 02:22Z |  |
| T604 | Dependencies: sentry without default features, so native-tls | done | 1 | 78 min | 01:49Z |  |
| T478 | jobs, intake, calls, messaging and businesses: the scorecard | done | 1 | 2 h 23 min | 01:47Z |  |
| T587 | payments and picks: the old Confirm fee writer is marked tes | done | 1 | 48 min | 00:26Z |  |
| T606 | harness: scripts/check_copy.py allows one aria-hidden arrow  | done | 1 | 62 min | 23:23Z |  |
| T203 | Wording scan: the required statements, business copy lines a | BLOCKED | 1 | 3 min | 23:08Z | PLAN GAP: `app/portals/templates/quote_page.html:101` (owned by DONE tasks T052, T095, T294, T308, T384, T388, T393, T44 |
| T115 | docs/a11y.md (plan, statement, VoiceOver script), statement  | done | 3 | 37 min | 22:46Z | PLAN GAP: `package-lock.json` (unchanged from main) fails `npm audit --audit-level=low` on GHSA-fc5q-39xc-vjpf; nothing  |
| T603 | Perf and twin (harness): the web vitals report records LCP a | done | 1 | 40 min | 22:05Z |  |
| T104 | Wording scan: brand, banned words and the not-a-contractor p | done | 4 | 44 min | 21:44Z | Review not approved after the one fix round (head 01543c2e001f3583c873583dd24865826cfa3ae5; tests/docs/test_wording_scan |
| T594 | site: the end-to-end spec of the web chat and request page a | done | 2 | 83 min | 21:03Z | PLAN GAP: app/server/src/chat_codes.rs:187 and app/site/templates/request.html, both owned by T489. Smallest plan change |
| T106 | ops: /healthz and the web and worker liveness exits | done | 1 | 77 min | 20:38Z |  |
| T602 | privacy: the inbound email signature check refuses an out-of | done | 1 | 30 min | 20:37Z |  |
| T597 | Runner (harness): the early merge-close step also pushes the | done | 1 | 26 min | 20:28Z |  |
| T582 | quotes and messaging (harness): a late read-back confirm is  | done | 1 | 36 min | 20:13Z |  |
| T115 | docs/a11y.md (plan, statement, VoiceOver script), statement  | BLOCKED | 2 | 68 min | 19:54Z |  |
| T104 | Wording scan: brand, banned words and the not-a-contractor p | BLOCKED | 3 | 46 min | 19:24Z |  |
| T605 | businesses, intake and picks: the leaving answers, not-cover | done | 1 | 46 min | 19:15Z |  |
| T528 | ops: the alert, audit-row and digest-row readers and writers | done | 2 | 37 min | 19:05Z | PLAN GAP: tests/integration/tests/business_leaving.rs:463 (owned by T228, also listed by T343) asserts that businesses/s |

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 305 of 735 (41.5%) · partial 380 · unstarted 50
**Tasks merged (throughput only):** 453 of 645

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 17 | 18 | 0 | 35 |
| ai | 9 | 17 | 3 | 29 |
| businesses | 19 | 38 | 1 | 58 |
| calls | 12 | 13 | 0 | 25 |
| consent | 14 | 21 | 0 | 35 |
| fees | 24 | 27 | 1 | 52 |
| intake | 14 | 29 | 0 | 43 |
| job-cards | 14 | 10 | 0 | 24 |
| licensing | 34 | 14 | 0 | 48 |
| lifecycle | 19 | 9 | 0 | 28 |
| messaging | 21 | 23 | 1 | 45 |
| ops-recovery | 4 | 31 | 19 | 54 |
| privacy | 9 | 21 | 1 | 31 |
| quiet-hours | 6 | 3 | 0 | 9 |
| quotes-pick | 27 | 29 | 0 | 56 |
| recurring | 7 | 2 | 2 | 11 |
| security | 9 | 41 | 5 | 55 |
| terms | 25 | 7 | 3 | 35 |
| website | 9 | 16 | 5 | 30 |
| wording | 12 | 11 | 9 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-18T18:24:43Z (week)): week: 130.4M used, 63.6M left, resets 2026-11-19T17:00:00Z; projected finish 2026-12-02T05:03:21Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 96 of 246 done
- stage 6: 0 of 46 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 3: T483 (lease until 2026-11-18T21:25:05Z)
- lane 7: T711 (lease until 2026-11-18T21:12:25Z)

## Last usage reading
- {"task": "T624", "at": "2026-11-18T18:26:13Z", "input_tokens": 114, "cache_creation_input_tokens": 120586, "output_tokens": 19745, "cache_read_input_tokens": 1676939}
