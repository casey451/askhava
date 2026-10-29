# Pre-build sweep, October 29, 2026 (D-491)

**What ran:** eight independent Opus reviewers each cold-read one batch of task prompts in the runner's order, as a worker would, and checked each against `kit/tasks.json`, `kit/layers.json`, the spec, the test design and the decisions; a ninth swept all 713 requirements against their test rows, the fixed texts, the settings and the journeys. Nothing was changed by the reviewers; every finding carries file-and-line evidence. The reports are summarised here; the full texts are in the session that ran them. **Decision:** D-491 (what was changed and why). **Follows:** `docs/audit/process-review-2026-10-29.md`.

## 1. Counts

| Batch | Prompts read | Findings | Blocker | High | Medium |
|---|---|---|---|---|---|
| 2 | 27 | 27 | 5 | 10 | 12 |
| 3 | 27 | 26 | 6 | 16 | 4 |
| 4 | 27 | 25 | 6 | 9 | 10 |
| 5 | 27 | 26 | 6 | 13 | 7 |
| 6 | 27 | 30 | 14 | 11 | 5 |
| 7 | 27 | 28 | 9 | 12 | 7 |
| 8 | 20 | 19 | 3 | 10 | 6 |
| 1 | 30 | 25 | 3 | 9 | 13 |
| Spec sweep | 713 requirements | 22 (8 contradictions, 9 gaps, 5 stale) | | | |
| **Total, first pass (all batches and the sweep)** | | **228** | **52** | **90** | **64** |
| v1 (second pass, repaired prompts T172 to T213) | 55 | 22 | 2 | 10 | 10 |
| v2 (second pass, T057 to T150) | 55 | 27 | 5 | 11 | 11 |
| v3 (second pass, T199 to T142) | 55 | 33 | 4 | 18 | 11 |
| **Total, first two passes** | | **310** | **63** | **129** | **96** |
| w1 (third pass, T172 to T062) | 46 | 23 | 4 | 13 | 6 |
| w2 (third pass, T189 to T157) | 46 | 32 | 2 | 18 | 12 |
| w3 (third pass, T209 to T142) | 45 | 20 | 1 | 11 | 8 |
| **Total, three passes** | | **385** | **70** | **171** | **122** |
| x1 (fourth pass, T172 to T183) | 38 | 24 | 3 | 11 | 10 |
| x2 (fourth pass, T173 to T085) | 38 | 24 | 3 | 14 | 7 |
| x3 (fourth pass, T162 to T142) | 37 | 21 | 2 | 8 | 11 |
| **Total, four passes** | | **454** | **78** | **204** | **150** |

A blocker is a task the worker could not finish as written; the plan's own validators (`validate_tasks.py`, `trace_spec.py`, `gen_prompts.py --check`) were green throughout, because none of these defects is visible to a structural check.

## 2. The eight causes (and the fix for each)

| # | Cause | Tasks hit | Fix (D-491) |
|---|---|---|---|
| 1 | Customer words: the template files were one task's | about 40 | The two template files and every `status.rs` are shared paths; a `copy` flag gives a task the customer-copy steps |
| 2 | The layer wall: pages and senders in `ops` (layer 1), tests in a crate that cannot see the module under test | 14 | Pages to `portals`, senders to `messaging`, the AI injection test to the integration crate, the intake limit to `intake`, the unreachable-homeowner decision to `picks`; `privacy` calls `adapters` |
| 3 | State with no table | 18 | Each task owns its migration; all migrations renumbered by serial position |
| 4 | Order: a task needing what a later task builds | 12 chains | T219 after T009; T044 after T153; T163 before T156 and T208; T065, T066, T064; J1 and J2 split into a stage-4 slice and T222 |
| 5 | A test row on a task that owns none of its files | about 60 rows | Rows moved to the owning tasks (the `kit/tasks.json` diff lists them) |
| 6 | Stale decisions pasted beside current rules; wrong test commands | 25 prompts | `gen_prompts.py` marks superseded decisions and pastes the successor (`kit/superseded_decisions.json`); `askhava-recovery-tests`; the Playwright config path |
| 7 | Spec contradictions, mostly between the D-480, D-485 and D-490 additions and older text | 20 requirements, 41 rows | Rewritten as section 3 lists |
| 8 | Missing pieces: team members, the legal pages, a contract-test sandbox mode, a voice stand-in, the NFKC crate, PDF and push with no crate | 7 | T220, T221, T223; T129 and T167 goals; `unicode-normalization`; PDF dropped from V1; push moved to Phase 2 |

## 3. Text changed

### (1) Requirements whose text changed (20, each tagged D-491)
- **TXT-005**: a full askhava.com address; the https:// scheme is optional; never a shortener.
- **TXT-024**: the safety call comes from a non-main number and is gated by CALL-022.
- **TXT-044**: Spanish UCS-2 is reported, never failed, except the CON-031 Spanish text (GSM-7, 2 segments).
- **TXT-045**: one-click unsubscribe is a per-address job-card opt-out, never a business PAUSE.
- **CON-018**: adds the full Spanish HELP reply, the away sentence and the LISTA line, marked as a Spanish draft.
- **FEE-017**: drops "including retries"; the check runs before every charge attempt.
- **FEE-028**: adds the ledger entry types countering-fee, countering-fee-return and processing-fee.
- **FEE-047**: the PaymentIntent is created on the first Confirm-page load (for an auto-confirm quote, at the pick).
- **FEE-052**: the warned, unreleased charge is refunded; the pick ends as fraud-refunded; the job returns to Quotes ready.
- **JOB-014**: adds the early-fraud-warning trigger for Payment pending going back to Quotes ready.
- **INT-022**: a later message joins or opens a job as INT-010 says.
- **OPS-024**: a Confirm press during safe mode is refused ("try again later"); only releases resume.
- **OPS-047**: quote minutes come from the optional PICK-040 field "minutes this quote took".
- **PICK-040**: adds the optional field "minutes this quote took", used only by the scorecard and never shown on a card.
- **SEC-007**: recovery codes are kept on paper, apart from the password manager (OPS-042).
- **SEC-049**: the Stripe IP ranges apply in live and sandbox modes; stand-in mode accepts only the stand-in's host.
- **SEC-050**: Postmaster is checked in Casey's monthly review (the FEE-033/OPS-027 flags review that TD-OPS-08 tests).
- **A11Y-015**: the time-limit list adds the code windows, the 5-minute fresh-code windows and the owner idle timeout.
- **BIZ-015**: statements are CSV only in V1; no PDF.
- **BIZ-041**: the push web app is deferred to the later app phase; no service worker until then.


### (2) Test-design rows changed (40) or added (1), each ending in D-491
- **TD-FEE-23**: dispute auto-submit happens only under the FEE-050 release-plus-call-or-3DS condition; otherwise it waits for Casey.
- **TD-FEE-25**: no `off_session=true` confirm except the PICK-048 path; the intent is created on the first Confirm-page load.
- **TD-FEE-33**: an early fraud warning also ends the pick as fraud-refunded.
- **TD-LIC-08**: adds LIC-025: a confirmed breach ends the account; an unconfirmed complaint changes nothing.
- **TD-CARD-01**: adds the CARD-009 boundary: 4 words left gives the fallback card, 5 words are kept.
- **TD-CARD-02**: every SHAFT category is removed; the log holds the list entry, never the removed text.
- **TD-CARD-09**: the emergency pick subject now uses PICK-037's "picked … Accept by" wording.
- **TD-CARD-10**: adds the two-times-a-day case (8 a.m. and 5 p.m.); a third time can't be chosen; cites D-380.
- **TD-INT-02**: INT-010 boundary: 50% word overlap joins (5 of 10), 49 of 100 opens a new job; the 2:00:00 case.
- **TD-INT-21**: INT-039 skip threshold: confidence 0.90 skips, 0.89 asks.
- **TD-INT-22**: "every" candidate job type, not "any"; never a business covering only part of the job.
- **TD-INT-27**: INT-044 re-ask boundary: 319 px re-asks, 320 px does not.
- **TD-CON-15**: quotes the full Spanish HELP block, identical to CON-018.
- **TD-TXT-09**: the safety call comes from a non-main number, with or without an email on file, gated by CALL-022.
- **TD-TXT-16**: the switch is refused while any CALL-022 condition is unmet; the call comes from a non-main number.
- **TD-TXT-18**: drops "the pick-call script" from the sentinel scan.
- **TD-TXT-25**: per-sentence word limits (15, never over 20) with the TXT-016 fee/LIST/Terms exception.
- **TD-TXT-26**: a planted UCS-2 character or third segment in the CON-031 Spanish text fails.
- **TD-TXT-27**: the unsubscribe stops one address only; the business stays unpaused; its other addresses still get cards.
- **TD-PICK-15**: CONFIRM by text gives the Confirm link only and never charges, whoever's phone sends it.
- **TD-PICK-18**: the optional minutes field can be left empty and never shows on a quote card.
- **TD-PICK-25**: the last-reminder wording matches PICK-037 exactly.
- **TD-CALL-05**: the owner and a "Confirm and billing" member link staff phones; the self-contradiction is removed.
- **TD-CALL-06**: adds the CALL-013 boundary: digits at 9 seconds are taken; at 10 seconds the prompt repeats once, then the call ends.
- **TD-CALL-08**: "replies YES or taps Approve" (BIZ-033).
- **TD-BIZ-01**: drops "then voice": record phone, then email, then letter; never voice.
- **TD-BIZ-03**: complaints are digest lines; only a safety word alerts at once.
- **TD-BIZ-04**: CSV statements equal the ledger; no PDF in V1.
- **TD-BIZ-07**: the one launch notice goes by email, as BIZ-031 says (batch5 #23).
- **TD-BIZ-11**: a member's Confirm sends the owner an email, plus a text only when the business chose text.
- **TD-BIZ-31**: until the later app phase there is no service worker, manifest or push; the app-phase cases are deferred.
- **TD-WORD-02**: adds the WORD-013 Spanish-versus-English page comparison docs test.
- **TD-WEB-01**: "Is Ask Hava a person?" answer.
- **TD-WEB-03**: pest control is listed plainly with its sample card and never advertised.
- **TD-WEB-04**: the coverage page lists pest control plainly and never advertises it.
- **TD-TERMS-01**: no arbitration clause or class waiver in effect (an OPTIONAL, OFF section is allowed).
- **TD-TERMS-02**: the new TERMS-011 wording, six samples, and the one allowed sign-up-reply exception.
- **TD-SEC-04**: allows Stripe.js on the Confirm page only, because FEE-014 requires Stripe Elements (batch4 #9).
- **TD-SEC-27**: tests both sandbox ranges and stand-in-host-only; the monthly Postmaster review is listed.
- **TD-OPS-06**: a Confirm press in safe mode is refused with no Stripe call; no charge starts when safe mode ends.
- **TD-WORD-31 (new)**: covers WORD-029. Kind E, layers U and D. It tests the `check_copy.py` limits: 21 versus 20 words, reading grade 8/9 for homeowner/business copy, superlatives, and the exemption for sentences marked "fixed". It sits after TD-WORD-30.

Other test-design text that changed:
- **Line 74**: the control router is compiled out under SEC-035's `test-control` feature, and both TD-SEC-05 and TD-SEC-15 check it.
- **J3 step 3**: the refund needs a fresh TOTP (the SEC-043 step-up).
- **J3 step 4**: the fee-mode change needs a TOTP entered within the last 5 minutes.
- **J3 step 5**: a Confirm press during safe mode is refused ("try again later"), Casey signs in again after the 30-minute idle timeout, and acknowledging resumes only the paused release, with the business's next press charging once. The step-up and re-sign-in steps come from batch7 #25.
- **Section 6 A11Y-015 row**: the time-limit list is widened the same way as the requirement; the row's first words are unchanged.


### (3) Legal, catalog, threat-model and architecture edits
- **`docs/legal/privacy-policy.md`**:
  - added the subheading "### When you pick a business" above the TERMS-025 sharing paragraph, and "### Who sees what at each step" above the table;
  - added the TERMS-031 never-shared sentence as its own paragraph in section 5 (texting);
  - added "website delivery and security (Cloudflare)" to the vendor list.
- **`docs/legal/business-terms.md`**: section 3 gains the BIZ-057 sentence: Ask Hava never buys, imports, rents or resells requests from another site or list.
- **`docs/legal/sms-terms.md`**: the HELP reply is reordered so the CON-018 sentences run as one block before the help links. No other wording changed.
- **`docs/catalog/question_bank.yaml`**: line 1 no longer lists INT-042, and the stale trailing INT-042 safety-prompt comment at the end of the file is removed. Line 24 already says it was withdrawn; no data changed.
- **`docs/threat-model.md`**: I13 now cites D-440 instead of the withdrawn PICK-025.
- **`docs/architecture.md`**:
  - §6 diagram: "fraud warning before release" added to the back-to-Quotes-ready reasons;
  - §6 Payment pending row: the FEE-052 trigger, and "JOB-014, FEE-052" in the requirement column;
  - §6 pick status list: adds "fraud-refunded";
  - §7 line 226: one PaymentIntent per pick, created on the first Confirm-page load, with PICK-048 named as the only off-session exception;
  - §8 `calls` reader: the CALL-005 read-back of first name and street only, under the same audited-read rule (PRIV-003).
  - §3 needed nothing: line 138 already says there is no service worker.


## 4. The second pass

After the first repair, three reviewers re-read the 165 prompts it had changed. Their 82 findings were of the same eight kinds plus one new one: repairs that moved a test row to a task which still could not write it. The second repair (the same decision, D-491) added T224 (the Confirm-and-pay page) and T225 (the ledger table first), removed T160 and T211, moved the email pipeline to the start of stage 3, let `site` read the figures its pages show, started the shared layout early, split or corrected 31 test rows and added 16, and fixed the photo-check requirements the first pass had left contradicting the allow-list. The second Rust trial (`docs/audit/rust-trial-2-2026-10-29.md`) ran alongside and its twenty deviations are in the same decision.

## 5. The third pass

Three reviewers re-read the 137 prompts the second round changed and found 75 more (7 blockers), almost all of two shapes: a task told to change a page, template or struct another task owns, and a test case on a task whose layer cannot reach the module or whose builder runs later. The third repair (the same decision) added T227 and T228, moved the quote-page, fee-preview, notice-recipient and digest cases to their owners, and added 10 rows while correcting 68. Blockers per pass: 63, 11, 7.

## 5a. The fourth pass and the V11 rule

Three reviewers cold-read all 113 prompts the third round had changed and found 69 more (8 blockers), still of the same two shapes. The recurring one, a test case on a task that cannot write it, is now a validator rule: V11 in `scripts/validate_tasks.py` fails any test row whose case for requirement R sits on a task that does not own R, has no ancestor owning R and shares the row with no task owning R. Run against the tree before the fourth repair, V11 flagged 106 rows the three human passes had not reached; the fourth repair (D-492) resolved every flag by moving or splitting rows, so this defect shape can no longer enter the graph silently. The other recurring shape (a task told to change a page, template or struct another task owns) remains a reading matter: the runner's fresh-eyes reviewer reads each prompt before the diff, and risk R-20 says when a further pass runs. Blockers per pass: 63, 11, 7, 8. The fourth repair's other changes are in D-492: the `app/site` crate exists from T051, the carrier-restricted word list moves to `catalog`, the intake tables are created by T038, the provider's opt-out configuration file is T029's, the business job page is rendered by `portals`, `businesses::api::notice_addresses` has a creator, the Stripe webhook's security rules sit on the task that owns the route, and the shared-path sentence in every prompt names the template, status, askama, worker and prompt paths D-491 made shared.

## 6. Left for the re-plan after the hand-run test

From the three expertise files added today (`docs/brains/ai-intake.md`, `legal-terms-privacy.md`, `deploy-operations.md`), the proposals D-491 did not adopt were adopted by D-492 the same day as 22 new requirements and 29 rewrites, except these, which change the offer, the scope or a Casey decision and wait for the re-plan: the $100 business liability floor, dropping the EIN at sign-up, where the Stage 0.5 pages are hosted, the AI line in texts (the two-segment consent text), and the INT-040 calibration. For the record, the list D-491 left open was: TERMS-001 says the terms are never accepted by a text reply while TXT-031 and PICK-046 accept a pick by "2 YES" (legal brain 1); PICK-049's share box starts ticked (FTC dark-pattern note, legal brain 4); the incident plan notifies by text, which A.R.S. 18-552 does not list (legal brain 10); Railway's Config as Code files stop being read on 2026-12-01 and T135 owns `railway.toml` (deploy brain 1); Railway does not watch the health check after deploy, so OPS-033's restart assumption is wrong (deploy brain 2); a rollback restores the old variables (deploy brain 3); Cloudflare's strictest TLS mode does not work with Railway and Bot Fight Mode can challenge webhooks (deploy brain 4); no documented Railway proxy ranges for SEC-040 (deploy brain 5); R2 has no write-only prefix-scoped key for PRIV-029 (deploy brain 8); the migration credential sits in the web service (deploy brain 3 proposal); the AI brain's INT-021 disagreement (fall back to the AI-off path instead of a 30-minute hold), the retention nuance that flagged text may be kept up to 2 years, and the measured-threshold rule for INT-039.
