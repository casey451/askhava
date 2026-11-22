# Ask Hava build status

Updated 2026-11-22T03:10:30Z by runner b3e4c6cdedb5 (actions).

**Building: T542 in progress since 01:45Z; 599 of 763 done; week 49% used, resets Thu 2026-11-26 17:00Z (10:00 am Phoenix).**

- **Tasks done:** 599 of 763 build tasks (owner gates not counted)
- **Stage:** stage 5; stage proofs recorded: 1, 2, 3, 4
- **Lane:** lane 5: T542 claimed 01:45Z, lease until 04:45Z; lane 7: T721 claimed 02:54Z, lease until 05:54Z
- **Week:** 49% used, 91.3M tokens left; resets Thu 2026-11-26 17:00Z (10:00 am Phoenix)
- **Last run:** 03:10Z, 0 min before this update
- **Keeper:** last ran 03:10Z, next wake 03:30Z

## Needs attention
- BLOCKED T117: Reviewer after findings round still FINDINGS at 6670a168e2d4d62e56e23b50ed17253f85e6af47:
1. docs/runbook.md:180-181 (blocking, recovery): restore of an older export puts the old key version back only for the replay run, but step 8 starts worker (and step 10 web) with the host key list lacking v1, so they refuse to start (and VAULT_KEY v1 rows unreadable). Fix: in restore step 8 put the old version back on the host key list, re-run rotate_batch and rekey_suppression_batch to 0, call retire_index_version, then drop v1 again; the paper-copy clock restarts; test should assert it.
2. docs/runbook.md:173 and :169 (minor): retire_index_version deletes only messaging.suppression rows (suppression_rekey.rs:231-234); stale v1 phone digests are removed by a later rotate_batch after v1 is dropped from the key list (privacy/src/vault.rs:752,760-761). Rotation drill omits that clean-up run.
Nit: runbook.md:101, :174, :177 exceed wrap width.
- BLOCKED T121: plan gap reported: PLAN GAP: outside ownership: app/server/src/owner_portal.rs, app/server/src/owner_tools.rs, app/portals/src/owner_help.rs, app/server/src/homeowner_portal.rs
SEC-010 scan (D-4258) fails on ten form structs without deny_unknown_fields (owner_portal ChangeBody, BridgeBody, OptOutBody, StripeBody, SafetyBody, SpanishBody, BusinessBody, ReleaseBody; owner_tools EvidenceBody; owner_help HelpForm; homeowner_portal PhotoPost) and unclassified raw parses (owner_tools audit_log RawQuery, homeowner_portal upload_fields Multipart and csrf_checked), none owned by T121. Also business_signup.rs Asked::from_; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T121 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T397: plan gap reported: PLAN GAP: outside ownership: app/server/tests/door_matrix.rs:1182. owner_jar signs the owner in at key.now + 30s*k per probed Owner row; past ~60 rows expires_at < created_at, violating portals.session CHECK (expires_at > created_at), so POST /owner/help/close as Owner returns 500. Marking the three /b/phones rows yes in docs/architecture.md section 13 turns td_sec_61_every_served_door_row_over_http red (reproduced by worker and by reviewer: exit 101, door_matrix.rs:1533). Smallest fix: a harness task that bases owner_jar sign-in times in the past (e.g. key.now - OWNER_IDLE_LIMIT) or spaces TO; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T397 --plan-fix D-### --reason "..."` (D-2859)
- BLOCKED T810: plan gap reported: PLAN GAP: outside ownership: app/server/tests/confirm_press_code.rs. TD-SEC-82 requires a direct portals.shared_inbox insert (never set_inbox_shared/record_step_up, D-5319), but TD-SEC-10 (tests/integration/tests/architecture_migrations.rs check_app :737, check_sql_forms :660-677, schema_refs :295) scans every .rs under app/: macro form fails "`portals.` names another module's schema", plain sqlx::query fails "SQL outside the query macros: `query`". Reviewer confirmed. Smallest plan change: T810 owns tests/integration/tests/confirm_press_code.rs (not scanned; confirm_terms.rs:238 does the plai; fix the plan (the task's kit/tasks.json entry or prompt, the decision in its `decisions` list), then run `python3 -m runner --requeue T810 --plan-fix D-### --reason "..."` (D-2859)
- PLAN GAP T121 (since 2026-11-22T01:45:41Z, no attempt used; a plan change fixes it): outside ownership: app/server/src/owner_portal.rs, app/server/src/owner_tools.rs, app/portals/src/owner_help.rs, app/server/src/homeowner_portal.rs
- PLAN GAP T365 (since 2026-11-12T04:23:07Z, no attempt used; a plan change fixes it): T365: not in kit/tasks.json
- PLAN GAP T397 (since 2026-11-22T02:12:37Z, no attempt used; a plan change fixes it): outside ownership: app/server/tests/door_matrix.rs:1182. owner_jar signs the owner in at key.now + 30s*k per probed Owner row; past ~60 rows expires_at < created_at, violating portals.session CHECK (expires_at > created_at), so POST /owner/help/close as Owner returns 500. Marking the three /b/phones rows yes in docs/architecture.md section 13 turns td_sec_61_every_served_door_row_over_http red (reproduced by worker and by reviewer: exit 101, door_matrix.rs:1533). Smallest fix: a harness task that bases owner_jar sign-in times in the past (e.g. key.now - OWNER_IDLE_LIMIT) or spaces TOTP steps differently. Prompt forbids editing door_matrix.rs and forbids working around an unreachable route. Worker work is uncommitted in the tree.
- PLAN GAP T518 (since 2026-11-18T00:12:48Z, no attempt used; a plan change fixes it): T518: dependency T106 is DONE but its owned file app/server/src/healthz.rs is not on main; T518: dependency T106 is DONE but its owned file app/server/src/liveness.rs is not on main; T518: dependency T106 is DONE but its owned file tests/integration/tests/health.rs is not on main; T518: dependency T106 is DONE but its owned file app/core/src/queue_health.rs is not on main
- PLAN GAP T810 (since 2026-11-22T03:05:53Z, no attempt used; a plan change fixes it): outside ownership: app/server/tests/confirm_press_code.rs. TD-SEC-82 requires a direct portals.shared_inbox insert (never set_inbox_shared/record_step_up, D-5319), but TD-SEC-10 (tests/integration/tests/architecture_migrations.rs check_app :737, check_sql_forms :660-677, schema_refs :295) scans every .rs under app/: macro form fails "`portals.` names another module's schema", plain sqlx::query fails "SQL outside the query macros: `query`". Reviewer confirmed. Smallest plan change: T810 owns tests/integration/tests/confirm_press_code.rs (not scanned; confirm_terms.rs:238 does the plain insert there); update docs/test-design.md:939 and kit/tasks.json TD-SEC-82 paths and run command to `cargo test -p askhava-integration --test confirm_press_code`.
- Strict plan check red: 33 problem(s) on tasks still to run (+477 on DONE tasks); first: T624: stored context_pct 12% but it measures 18%; regenerate

## Last 24 hours
| Task | Title | Result | Attempt | Took | Ended | Retry reason |
|---|---|---|---|---|---|---|
| T616 | site: strong per-coding ETags with 304 revalidation, a one-y | MERGE_PENDING | 1 | 96 min | 03:07Z |  |
| T118 | Journey J3 (operator) keyboard-only, with closing checks | MERGE_PENDING | 2 | 2 h 19 min | 03:06Z | Reviewer after fix round (head ee01dda321ae54d4c6af6a1e464281c92fbdcf1d) still FINDINGS: / 1. tests/e2e/j3_operator.spec |
| T810 | portals and server: a shared-inbox Confirm press asks for it | BLOCKED | 2 | 2 min | 03:05Z | PLAN GAP: outside ownership: app/server/tests/confirm_press_code.rs. TD-SEC-82 requires a direct portals.shared_inbox in |
| T810 | portals and server: a shared-inbox Confirm press asks for it | QUEUED | 1 | 39 min | 02:57Z |  |
| T803 | messaging (harness): an opted-out number gets no HELP reply; | check_failed | 1 | 69 min | 02:44Z | Ci: failure; Checks: failure / Checks failed at step: Named mutants of the files this change touches (scripts/run_named_ |
| T691 | intake (harness): START into an open request resends the con | MERGE_PENDING | 2 | 2 h 16 min | 02:20Z | session ended without handing in |
| T759 | intake: "Close which?" expires on a worded reply that is not | done | 1 | 28 min | 02:19Z |  |
| T397 | server and portals: the owner approves or declines a member- | BLOCKED | 1 | 27 min | 02:12Z | PLAN GAP: outside ownership: app/server/tests/door_matrix.rs:1182. owner_jar signs the owner in at key.now + 30s*k per p |
| T121 | Startup key checks, control router absent in live, supply-ch | BLOCKED | 1 | 15 min | 01:45Z | PLAN GAP: outside ownership: app/server/src/owner_portal.rs, app/server/src/owner_tools.rs, app/portals/src/owner_help.r |
| T117 | docs/runbook.md: outages, safe mode, refunds, disputes, comp | BLOCKED | 3 | 49 min | 01:39Z | Reviewer after findings round still FINDINGS at 4f79a8b8c4195e68ccfba3a25eaab6d612560536: / 1. docs/runbook.md:139-142 a |
| T542 | portals and server: notice and backup address changes are re | QUEUED | 1 | 2 h 37 min | 01:29Z | Reviewer after the findings round still ended REVIEW-VERDICT: FINDINGS at head 4de98e6fb74e1e5394629d7a1abe0694167723a2: |
| T725 | site: the home page shows the texting number with its consen | BLOCKED | 1 | 21 min | 01:24Z | PLAN GAP: tests/docs/test_required_lines.py:139 (WORD-013, docs/spec/wording.md:17) contradicts kit/prompts/T725.md:9 (S |
| T609 | site (harness): the D-4246 polish on the footer, how it work | BLOCKED | 1 | 2 min | 00:57Z | PLAN GAP: tests/e2e/site_pages.spec.ts:26-28 (owned by T102) requires exactly one footer h2 "Homeowners" and exactly fou |
| T117 | docs/runbook.md: outages, safe mode, refunds, disputes, comp | QUEUED | 2 | 106 min | 00:48Z |  |
| T118 | Journey J3 (operator) keyboard-only, with closing checks | QUEUED | 1 | 2 h 35 min | 00:44Z |  |
| T800 | Legal docs and tests (harness): the Twilio campaign submissi | done | 1 | 46 min | 00:39Z |  |
| T608 | site and portals: the home page and the quote page in full V | done | 2 | 2 h 21 min | 23:38Z | Ci: failure; E2e: failure / E2e failed at step: The journeys (npx playwright test tests/e2e) / {"timestamp":"2026-11-21T |
| T600 | intake, businesses, privacy and server: four worker handlers | done | 1 | 45 min | 23:15Z |  |
| T798 | catalog: the photo asks' English wording loaded from the tra | done | 1 | 107 min | 23:03Z |  |
| T117 | docs/runbook.md: outages, safe mode, refunds, disputes, comp | QUEUED | 1 | 34 min | 22:56Z |  |
| T794 | adapters and server: the Twilio media delete and the product | done | 1 | 49 min | 22:17Z |  |
| T788 | Tooling (harness): check_pr_change.py fails a task branch th | done | 1 | 64 min | 21:30Z |  |
| T600 | intake, businesses, privacy and server: four worker handlers | resync | 1 | 57 min | 21:10Z |  |
| T565 | portals and server: the business team role change, removal a | done | 1 | 97 min | 20:57Z |  |
| T754 | portals and payments: a $0 Confirm after a PaymentIntent was | done | 4 | 113 min | 20:18Z | PLAN GAP: tests/integration/tests/stripe_webhook_dispatch.rs:420-430 (done test td_fee_43_a_database_error_is_resent_and |
| T707 | server: an emergency pick's address re-release waits while s | done | 3 | 97 min | 20:13Z | Reviewer after the findings round still ends REVIEW-VERDICT: FINDINGS (head 2f9bf610c3700bb46a4670ade27d67afda63591a). / |
| T690 | businesses and server: a sign-up waiting on "That number is  | done | 1 | 2 h 30 min | 19:27Z |  |
| T791 | Tests and templates (harness): three Spanish stay-on texts r | done | 1 | 36 min | 19:21Z |  |
| T675 | intake and server (harness): the dispatcher asks for photos  | BLOCKED | 1 | 4 min | 19:13Z | PLAN GAP: no production PictureTextMedia / Twilio media DELETE exists (only the test fake at tests/integration/tests/int |
| T790 | catalog, licensing and tests (harness): the termite report f | done | 1 | 57 min | 19:07Z |  |
| T608 | site and portals: the home page and the quote page in full V | check_failed | 1 | 2 h 46 min | 18:46Z |  |
| T615 | site and server: a styled 404 page for unknown paths, the fa | done | 4 | 68 min | 18:19Z | Ci: failure; Coverage: failure / Coverage failed at step: Branch coverage (cargo llvm-cov on the pinned nightly) / test  |
| T787 | intake: crate guide trimmed to 5,500 bytes, the detail moved | done | 1 | 68 min | 16:45Z |  |
| T786 | server: crate guide trimmed to 5,500 bytes, the detail moved | done | 1 | 34 min | 16:33Z |  |
| T655 | businesses: an unknown ROC license status warns at 24 hours, | done | 2 | 2 h 34 min | 16:05Z | model claude-opus-5-5 ran, task names claude-fable-5-2 |
| T789 | Tests (harness): TD-INT-47's no-contact check ignores the ra | done | 1 | 27 min | 15:38Z |  |
| T304 | server and portals: the owner refund review, the audit log v | done | 2 | 2 h 44 min | 15:18Z | PLAN GAP: app/portals/templates/owner_audit_log.html (owned by T205) and app/portals/templates/health.html (owned by T47 |
| T776 | intake: a move out of Arizona and a property-management requ | done | 3 | 99 min | 14:37Z | PLAN GAP: outside ownership: app/quotes/src/card_pool.rs (also docs/catalog/categories.yaml) / Worker: D-5160's three tr |
| T655 | businesses: an unknown ROC license status warns at 24 hours, | QUEUED | 1 | 2 h 36 min | 13:24Z |  |
| T707 | server: an emergency pick's address re-release waits while s | QUEUED | 2 | 77 min | 12:31Z |  |
| T581 | messaging, intake and server: a failed text's call-back is d | done | 1 | 76 min | 12:03Z |  |
| T615 | site and server: a styled 404 page for unknown paths, the fa | BLOCKED | 3 | 2 h 11 min | 11:53Z |  |
| T754 | portals and payments: a $0 Confirm after a PaymentIntent was | check_failed | 3 | 70 min | 10:58Z |  |
| T458 | businesses, calls and server: the email's "Link this phone"  | done | 1 | 112 min | 10:35Z |  |
| T654 | licensing and quotes: every ROC list import runs each active | done | 2 | 74 min | 10:15Z | PLAN GAP: outside ownership: app/quotes/src/service.rs, app/quotes/src/late_cards.rs, app/intake/src/cards_out.rs, app/i |
| T615 | site and server: a styled 404 page for unknown paths, the fa | check_failed | 2 | 92 min | 09:28Z |  |
| T776 | intake: a move out of Arizona and a property-management requ | BLOCKED | 2 | 11 min | 08:54Z |  |
| T674 | server: the homeowner's photo upload page and the short-live | done | 2 | 40 min | 08:06Z | merging main into the reviewed head conflicts in app/server/src/main.rs; a resync merges it in on this attempt (D-891) |
| T615 | site and server: a styled 404 page for unknown paths, the fa | check_failed | 1 | 55 min | 07:33Z |  |
| T674 | server: the homeowner's photo upload page and the short-live | resync | 2 | 2 h 38 min | 07:19Z |  |
| T754 | portals and payments: a $0 Confirm after a PaymentIntent was | BLOCKED | 2 | 46 min | 06:41Z |  |
| T755 | messaging: a sole opt-out word counts with any spacing ("STO | done | 1 | 66 min | 05:21Z |  |
| T777 | businesses: a sign-in code is texted only to a line whose cu | done | 2 | 39 min | 05:05Z | PLAN GAP: outside ownership: app/server/tests/business_signin_route.rs / T777's approved_at filter in sign_in_lines fail |
| T614 | site: the V6 header in base.html (links, the Menu in the pag | done | 3 | 53 min | 04:37Z | PLAN GAP: tests/integration/tests/quote_page.rs:172 (owner T052, protected) bans the word "summary" anywhere in the quot |
| T691 | intake (harness): START into an open request resends the con | QUEUED | 1 | 27 min | 04:35Z |  |
| T304 | server and portals: the owner refund review, the audit log v | BLOCKED | 1 | 91 min | 04:20Z |  |
| T457 | portals and server: the Confirm email link asks for a code w | done | 1 | 35 min | 04:16Z |  |
| T781 | recovery: a failed or timed-out nightly export raises one al | done | 1 | 116 min | 04:14Z |  |
| T776 | intake: a move out of Arizona and a property-management requ | BLOCKED | 1 | 11 min | 04:00Z |  |
| T684 | picks and server: the open-cards email goes at the times eac | done | 2 | 93 min | 03:50Z | PLAN GAP: app/quotes/src/models.rs:265 `card_sent_at` is pub(crate); T684 owns no app/quotes/src file. quotes::api has n |
| T780 | payments and portals: the scorecard's refund and dispute rat | done | 1 | 33 min | 03:33Z |  |

### Plan and harness merges on main
- none

## Progress (requirements verified)

**Requirements verified:** 0 of 735 (0.0%) · partial 716 · unstarted 19
**Tasks merged (throughput only):** 599 of 753

| Area | Verified | Partial | Unstarted | Total |
|---|---|---|---|---|
| accessibility | 0 | 35 | 0 | 35 |
| ai | 0 | 26 | 3 | 29 |
| businesses | 0 | 57 | 1 | 58 |
| calls | 0 | 25 | 0 | 25 |
| consent | 0 | 35 | 0 | 35 |
| fees | 0 | 52 | 0 | 52 |
| intake | 0 | 43 | 0 | 43 |
| job-cards | 0 | 24 | 0 | 24 |
| licensing | 0 | 48 | 0 | 48 |
| lifecycle | 0 | 28 | 0 | 28 |
| messaging | 0 | 45 | 0 | 45 |
| ops-recovery | 0 | 42 | 12 | 54 |
| privacy | 0 | 31 | 0 | 31 |
| quiet-hours | 0 | 9 | 0 | 9 |
| quotes-pick | 0 | 56 | 0 | 56 |
| recurring | 0 | 11 | 0 | 11 |
| security | 0 | 52 | 3 | 55 |
| terms | 0 | 35 | 0 | 35 |
| website | 0 | 30 | 0 | 30 |
| wording | 0 | 32 | 0 | 32 |

A requirement is verified only when every task naming it merged green, every test entry covering it is owned by a merged task, and the nightly full run on main was green afterwards.

## Pacing
- Pacing (session_reading 2026-11-22T02:58:08Z (week)): week: 102.7M used, 91.3M left, resets 2026-11-26T17:00:00Z; projected finish 2026-12-06T14:46:35Z.

## Tasks done (throughput)
- stage 0.5: 0 of 1 done
- stage 1: 11 of 11 done
- stage 2: 80 of 80 done
- stage 3: 29 of 29 done
- stage 4: 237 of 238 done
- stage 5: 242 of 335 done
- stage 6: 0 of 75 done
- stage 7: 0 of 2 done
- stage checkpoint: 0 of 1 done
- stage kit: 14 of 15 done

## Running
- nothing
- lane 5: T542 (lease until 2026-11-22T04:45:48Z)
- lane 7: T721 (lease until 2026-11-22T05:54:28Z)

## Last usage reading
- {"task": "T616", "at": "2026-11-22T03:07:30Z", "output_tokens": 47978, "cache_creation_input_tokens": 186252, "cache_read_input_tokens": 3538750, "input_tokens": 147}
