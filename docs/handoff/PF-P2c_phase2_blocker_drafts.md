# Laftly PF-P2c — Phase 2 blocker-fix review

**Date:** 2026-11-23  
**Basis:** `Laftly_PF-P2c_phase2_blocker_fixes.zip` (six task prompts, manifests, test-design excerpts, and selected source snapshots).  
**Nature of deliverable:** proposed **plan changes**, not implemented patches and not a green-test certification. The archive does not contain the complete repository, plan graph, locked specs, or current CI results.

## Executive disposition

| Task | Current obstacle | Recommended disposition | New task? |
|---|---|---|---|
| T382 | Sixth Verify check cannot occur through a five-try app cap | Correct TD-PICK-61 to test the cap separately from a provider 60202 returned on an **allowed** attempt | No |
| T394 | Writes across T299-owned dispatcher and risks DB connection held during provider await | Split AI-writing primitives from post-T299 dispatcher integration, with explicit transaction boundary | Yes, unless T299 can absorb all integration without a dependency cycle; not established by package |
| T685 | Existing T306 finished without an owner-facing toggle | Preserve T685 backend and add a small authorized portal UI task, or transfer to a still-queued owner of the actual settings page | Conditional; likely yes |
| T133 | Business identity code cannot read plaintext address to clear Resend suppression | Add messaging-owned unsuppression handler reached through a core task/event, no cross-layer private read | No, if T133 budget and crate ownership are amended |
| T171 | Disposable drill intentionally edits files excluded from normal ownership and locked specs | Run experimental changes **only in an isolated discarded worktree**, merge documentation fixes only | No |
| T392 | Worker has no Casey-held sandbox credentials to run a vendor inspection | Make T392 offline implementation and test; separate live run as Casey's Gate-C checkpoint | No build task; explicit owner checkpoint |

**Global guardrails:** no production vendor calls before Gate C; no weakening existing tests; cross-crate traffic through public `api.rs`; no AI provider await while a DB transaction or pooled connection is held; ≤600 changed lines for a build task and ≤800 for a harness task; no unearned statement that a live test passed.

---

## T382 — Verify attempt cap versus vendor error mapping

### Evidence / blocker

`tasks/T382/PROMPT.md:9` and `tasks/T382/TEST_DESIGN_ROWS.md:1` specify three wrong checks, a resend, two more wrong checks, **then a third post-resend check** to elicit Twilio 60202. That last check would be the sixth on a single verification. But `tasks/T382/cited/app/picks/src/rules.rs:13-15` explicitly specifies `CODE_TRIES = 5` and a 15-minute cooldown, and `tasks/T382/owned/app/portals/src/business_auth.rs:118-120` specifies five reserved sign-in checks. The test conflicts with the cap and can fail for the correct reason. Separately, `tasks/T382/owned/app/portals/src/business_auth.rs:642-670` sends notice-link mail without the server's admission call defined at `tasks/T382/owned/app/server/src/business_signin.rs:230-247`; T382's existing SEC-014 work must not be lost.

### Exact replacement for T382 goal's problematic test sentence

> Against the served T131 stand-in, verify **two independent cases** for each of the pick-code, identity-code and business sign-in flows. **Local cap case:** three wrong checks, one resend during the same 10-minute verification, then two wrong checks exhaust the five allowed attempts and enter the existing 15-minute cooldown/lockout. A later sixth user submission must not call the Verify adapter; it must return the established cooldown/lockout. **Vendor error case:** arrange the stand-in's provider-side attempt state (or deterministically inject its typed response) so that an otherwise **permitted** Verify check returns Twilio 60202. The caller must map it to the same cooldown/lockout rather than a generic error; also test typed 60203 on a permitted send. Do not increase `CODE_TRIES`, reset tries on resend or make a sixth provider check. Keep all other adapter errors' existing behavior.

Keep the original ownership and T131/T564/T121 ordering in T382. Preserve the notice-link per-IP and per-business rate-limit changes, with a concrete assembled-router test for the **actual production `SignInMailer` override**. Do not treat a trait's provided default of `true` as proof of enforcement. A future concrete mailer that forgets to override it would fail open; consider an explicit deny-by-default contract if changing existing test implementors can be scoped safely.

### TD-PICK-61: old → new

- **Old:** “three wrong tries, a resend and two more wrong tries: the third check after the resend (Twilio's sixth attempt, 60202) gives the cooldown, not an error...” (`tasks/T382/TEST_DESIGN_ROWS.md:1`).
- **New:** “For each of pick, identity and business sign-in, three wrong checks, resend within ten minutes and two more wrong checks exhaust the five app checks; a sixth user attempt is locally blocked with no Verify adapter check and returns cooldown/lockout. In a separate stand-in setup with provider state already exhausted, an **allowed** adapter check returning typed 60202 enters the same cooldown/lockout; a permitted send returning typed 60203 is handled as specified, with no generic error page. Other errors retain prior handling.”

Retain **TD-SEC-78** unchanged (`tasks/T382/TEST_DESIGN_ROWS.md:2`). **New task:** none. **Budget:** retain T382's original ≤7 owned files / **≤450 changed lines** (`tasks/T382/PROMPT.md:55`). **Must not break:** five-try security cap, attempts across resends, 15-minute cooldown, T131 typed errors, notice-link and sign-in-form rate limits, all pre-existing tests.

**Can't tell / need:** exact T131 adapter error type and stand-in injection controls; production trait-implementor inventory; latest `kit/tasks.json` dependency graph; whether the existing identity path treats `60203` from resend identically to the pick path.

**Casey decision:** none for the logical contradiction. Any change to the actual five-try security policy would be a separate, explicit Casey decision and is **not recommended**.

---

## T394 — AI-writing core and dispatcher integration

### Evidence / blocker

`tasks/T394/PROMPT.md:9` asks T394 to inject writing calls into T299's intake turn, but the dispatcher hook is in T299's owned `app/intake/src/order_dispatch.rs` (identified in `README_REVIEW_BRIEF.md:17`; **not included** in the ZIP). T394's ownership (`tasks/T394/PROMPT.md:35-45`) omits that file. The same goal mandates AI-029, yet `tasks/T394/TEST_DESIGN_ROWS.md:1` requires a one-connection pool to remain available while writing is in flight. `tasks/T394/owned/app/ai/src/models.rs:95` presently registers only `INTAKE_READING`; `tasks/T394/cited/app/ai/src/writer.rs:63-71` provides the pure acceptance functions, not the live dispatcher integration. T672's question-ID replay and T103's INT-020 phrase classifier are additional explicit prerequisites in the goal.

### Proposed split: exact revised T394 goal

> **T394 — Implement the reusable AI-writing core.** Add the four versioned writing instructions, scrubbed input builders, tagged data escaping, schema validation, acceptance screening, AI-008/AI-021/AI-023/AI-028 gating and bounded fallbacks behind the public `ai::api`. Update the deterministic stand-in to cover valid output, bad schema, extra fact, link, screened word, timeout and budget refusal. Write focused unit tests, including no unnecessary provider calls when a budget is exhausted. Do **not** edit T299's `order_dispatch.rs` or claim that a complete homeowner conversation or IC-53 is closed. Return a typed writing result/fallback suitable for a later dispatcher integration. Any transaction used to reserve an AI budget or fetch request state must complete and return its connection **before** awaiting the provider. Keep the writing input scrubbed and free of contact data.

**T394 owned files:** `app/ai/src/writing.rs`, `app/ai/src/models.rs`, `app/ai/src/tests/writing.rs`, `stand_ins/ai.py`, `schemas/intake_reading.json` (5, plus normal allowed shared export/guide paths). **Dependencies:** retain AI substrate T163 and gating T236 and schema/scrubbing prerequisites; remove T299 only if its code is genuinely not used in this core. **Proposed budget:** ≤400 changed lines, 5 owned files (a harness task, well inside 800).

### New task required to avoid cross-task ownership: `TNEW-AI-WRITING-DISPATCH`

**Title:** “Hook screened AI writing into T299's intake dispatcher and card step without holding a DB connection.”

**Exact goal to add:**

> After T299, T394, T672 and T103, wire the public `ai::api` writing entry into `app/intake/src/order_dispatch.rs` and existing intake turn/card flow. First commit or release the DB work that records and reads answers, question IDs, write budgets and conversation state; build a `ScrubbedText` request; **drop the transaction and pooled connection**; await the stand-in or provider with a bounded timeout; then reacquire a connection and revalidate the turn/version before accepting the result. Never write stale AI output over a newer reply. Rewords and follow-ups keep distinct stable question IDs across T672's replay. The INT-020 second opinion is called only for a message that would otherwise open a new request. Screen every response through `accept_question` / `accept_document`, preserve every fallback and EDIT cap, and show an AI-written card to its homeowner before approval/auto-send. With AI off, refused, timed out or over budget, use the AI-off response immediately, without user-visible failure. Close IC-53 only after the complete TD-AI-29 integration test and the one-connection AI-029 check pass.

**Owned files:** `app/intake/src/order_dispatch.rs` (transfer exclusively **after** T299), `app/intake/src/ai_turn.rs`, `app/intake/src/question_bank.rs`, `app/intake/src/summary_step.rs`, `tests/integration/tests/ai_writing.rs` (5, plus allowed shared paths). **Dependencies:** T299, T394, T672, T103, and any existing related T236 prerequisite. **Downstream dependency:** T237 war games must wait for this new integration task, **not merely** the now-core-only T394. **Proposed budget:** ≤450 changed lines, 5 owned files, harness ≤800 ceiling.

### TD-AI-29: old → new

- **Old owner:** T394, full PostgreSQL/stand-in roundtrip across questions, follow-up, card and project file plus one-connection test (`tasks/T394/TEST_DESIGN_ROWS.md:1`).
- **New owner:** `TNEW-AI-WRITING-DISPATCH` for the **same full checks**, unchanged pass criteria and file `tests/integration/tests/ai_writing.rs`; still run `app/ai/src/tests/writing.rs` as T394's core tests. Add a **new core row** `TD-AI-29-CORE`, owned T394, for schema/scrubbing/screening/limits/typed fallbacks through public API, with no live intake dispatcher claim. The full TD-AI-29 additionally verifies T672 replay IDs, INT-020's restricted invocation and a pool of size 1 available during an in-flight writing call.

**Must not break:** AI-004/005/006/008/019/021/023/025/028/029, CARD-010, INT-021/039, original AI-on feature scope, budget reservation, tests for `ai`, `intake`, `ai_budget`, `ai_wiring`, and all pre-existing tests. No new external AI request with raw contact data.

**Can't tell / need:** current T299 `order_dispatch.rs` and T299 status, T672 replay implementation, AI budget API, `kit/tasks.json` DAG, `app/ai/src/api.rs`, integration test fixtures. These are not in the archive. If T299 is queued and can implement the hook with no cycle, folding **only the dispatcher hook** into T299 is the alternative; do not presume it.

**Casey decision:** approve adding one small task and transferring dispatcher ownership; any choice to reduce AI-writing launch scope would also be Casey's, but is **not proposed**.

---

## T685 — Monthly missed-cards notices require a usable portal switch

### Evidence / blocker

`tasks/T685/PROMPT.md:9` explicitly leaves the portal toggle to T306, while `README_REVIEW_BRIEF.md:18` reports T306 is DONE and never implemented it. T685's owned list (`tasks/T685/MANIFEST.md`) has the DB switch and monthly job, not the portal page. The current rule `tasks/T685/read/app/messaging/src/missed_cards.rs:52-55` can correctly skip a business only **after** its switch is stored. `tasks/T685/TEST_DESIGN_ROWS.md:1` checks a pre-set “switch off” record, but not whether an owner can actually change it. The current migration number is `0219_...` (`tasks/T685/PROMPT.md:9`, final D-5336), not the historical 0214/0217/0218 numbers earlier in the goal.

### Exact text to append to T685 goal

> T685 implements and tests the persistent default-on switch and its public read/write API; it does **not** claim the owner-facing portal control exists. The route and visual control are a separate dependency after T685 because T306 is complete and did not build them. The monthly runner must consult the persisted switch at decision time; disabled owners receive no email, even after a first-of-month retry or delayed bulk-email release. Do not silently mark BIZ-044 fully satisfied until the portal control passes its own authenticated test.

**Ownership / deps:** T685 retains `migrations/0219_businesses_missed_cards_switch.sql`, `app/businesses/src/missed_cards_switch.rs`, `app/quotes/src/month_cards.rs`, `app/picks/src/missed_cards_run.rs`, `app/messaging/src/email.rs`, `tests/integration/tests/missed_cards_email.rs`; preserve T730/T804/T812/T236 migration ordering and the `copy` requirement. **Budget:** keep existing 6-file/510-line limit.

### Add small UI task, unless a queued settings-page owner is confirmed

**Provisional ID:** `TNEW-MISSED-CARDS-TOGGLE-UI` (use the next real, collision-free ID from `kit/tasks.json`, not the literal placeholder).  
**Title:** “Expose authenticated monthly missed-cards email on/off switch in business settings.”

**Exact goal:**

> Add a labeled, accessible toggle on the **business owner's existing settings page** for BIZ-044 monthly missed-cards notices, defaulting to on and reading the persisted value from `businesses::api`. Only a member authorized to manage the business's notification settings may POST a change; require the portal's existing CSRF/session rules, reject other businesses' IDs, persist the chosen value through `businesses::api` and show the saved result. Reopening the page reflects the stored value. Do not affect job-card notices or other account email settings. Add a router-level regression test that a real authorized owner can turn it off and no monthly email is queued, that an unauthorized actor cannot change it, and that a later turn-on works.

**Owned files:** existing business settings-page Rust handler and matching template (**paths must be established from current repo before the task is queued**); new `app/portals/src/missed_cards_toggle.rs` if a separate action handler is needed; new `app/server/tests/missed_cards_toggle.rs`. Shared exports/route registration, crate guides, and English/Spanish template keys may be used only under their established shared-path rules. **Deps:** T685 and existing portal settings owner T306; order it after any *queued* owner of the page. **Budget:** ≤3 owned files and ≤250 changed lines, with existing shared-path exceptions. **Launch gate:** Gate C must wait for this task.

**Test design:** retain **TD-BIZ-57** intact for job rules. Add **`TD-BIZ-57-UI`**: assembled-router test of signed-in authorized owner OFF → persisted → monthly run queues none; owner ON → subsequent eligible run queues once; CSRF failure, unauthorized role and cross-business attempt do not change the value; English/Spanish labels remain accessible.

**Must not break:** BIZ-025, BIZ-044, CON-019, default-on migration, monthly Arizona date, threshold math, deduplication and OPS-038 Bulk behavior, unchanged portal tests, existing GET-page a11y coverage and customer-copy review.

**Can't tell / need:** the actual existing portal settings handler/template path, its owner and any still-queued owner task, portal authorization/CSRF API, `kit/tasks.json`, live migration graph. Since those exact files were omitted, **do not invent an existing path**. If another genuinely queued task already owns those files, append the UI acceptance to that task and **do not create** the placeholder task.

**Casey decision:** whether to create the small UI task or amend a verified queued settings-page task. The option to launch without the off-switch would weaken the explicit BIZ-044 promise and should not be assumed.

---

## T133 — Suppression removal must be executed by messaging/privacy

### Evidence / blocker

`tasks/T133/PROMPT.md:9` assigns Resend suppression removal to BIZ-056 in `app/businesses/src/email_verification.rs`; the adapter currently accepts a plaintext `address: &str` in `tasks/T133/owned/app/adapters/src/email.rs:42-55,90-101`. But the `businesses` identity module holds mailbox IDs and works within a DB transaction (`tasks/T133/owned/app/businesses/src/email_verification.rs:436-499`). The package's brief explicitly says only `messaging`/`privacy` may read the plaintext email (`README_REVIEW_BRIEF.md:19`). A direct `businesses → adapters.remove_suppression(plaintext)` would either violate the vault boundary or require an unowned privacy read; placing an external provider call inside `confirm` is also the wrong transaction boundary. The assumption that `DELETE /suppressions/{address}` is supported is not confirmed until T133's sandbox contract check.

### Exact amendment to T133 goal

> Keep the adapter and stand-in Resend suppression endpoints in T133, but move **the handling of a plaintext recipient and the call to `EmailAdapter::remove_suppression`** into a new messaging-owned module `app/messaging/src/unsuppress.rs`, exposed solely by `messaging::api`. The business-verification code uses the existing opaque mailbox ID / durable `core.task` event to ask the messaging worker for removal; **businesses does not call messaging or the adapter directly and does not read plaintext mail**. `messaging` obtains the address through its authorized `privacy::api` vault read, sends only that address to Resend, waits outside a DB transaction, and writes the provider-removal audit row only after acknowledged success. The request is idempotent and deduplicated by mailbox and re-verification attempt. The re-verification email may not be dispatched as successfully delivered while the address remains suppressed; gate the send behind successful removal or a recorded manual-resolution state. On a typed unsupported endpoint or provider failure, create one High owner alert for Casey, keep the re-verification work pending/retryable, **do not write a success audit row** and expose no address in logs or owner alert. Keep `email_verification::confirm`'s T584 transactional settle intact. The real endpoint's support remains a contract-test finding, not an assumption.

**Ownership:** retain current 9 T133 owned paths **plus** `app/messaging/src/unsuppress.rs` as the tenth owned path. Shared paths permit `app/messaging/src/api.rs` public export, `src/lib.rs` module declaration and `app/server/src/bin/worker.rs` handler registration. **Dependencies:** existing T131/T176/T196/T584 and the worker's normal messaging infrastructure must precede this amendment. **Budget amendment:** from 400 to **≤520 changed lines**, ≤10 owned files (build ceiling 600). If updated line estimates truly exceed 600, split the messaging implementation into its own short predecessor task instead of ignoring the limit.

**TD-BIZ-36: old → new:**

- **Old:** “A re-verification of an address the provider suppressed removes the suppression through the email adapter first, against the fake Resend's suppression endpoint, and writes an audit row” (`tasks/T133/PROMPT.md:82`).
- **New:** “Against fake Resend, a re-verification request emits an opaque mailbox operation; the messaging-owned handler reads the address through `privacy::api`, clears the suppression before allowing the re-verification send to proceed, and logs a successful removal exactly once **after** the adapter acknowledges it. No plaintext reaches `businesses`, a log, a job card or a test failure message. A provider unsupported/error response keeps delivery pending, creates one High Casey alert and does **not** write a removal-success audit. A later successful retry clears it once.”

**TD-CON-17** (email help and STOP intake) and Resend/Lob contracts remain unchanged; the scoped email route still passes the already-read incoming mail to T151's help/opt-out entry point. **No new task required under the 520-line budget.**

**Must not break:** PRIV-001/026 and vault reader restrictions, BIZ-056, BIZ-040, T584 transactional settle, signed inbound webhook routing, `T482` STOP handling, idempotency and SEC-018, no live operations before Gate C, non-delivery semantics for suppressed email.

**Can't tell / need:** the actual `privacy::api` reader type for a business mailbox, `core.task` durable message schema, `messaging` worker wiring and dependencies, `app/businesses/src/api.rs`, provider sandbox DELETE behavior, existing messaging module owner; none of these are complete in the ZIP. Verify no `businesses → messaging` import or crate cycle is introduced.

**Casey decision:** if Resend cannot remove a suppressed recipient through the approved API, only Casey can authorize/perform the provider-dashboard removal and acknowledge the manual checkpoint. Never pretend an automatic API deletion occurred.

---

## T171 — Safe cold-start drill using disposable branch only

### Evidence / blocker

`tasks/T171/PROMPT.md:9` directs a new Sonnet session to modify the fee tier, job category/license rule and copy/templates on a throwaway branch. These are intentionally outside T171's mergeable owned list at `tasks/T171/PROMPT.md:33-37`, which contains only `docs/audits/cold-start-drill.md`, root `CLAUDE.md` and `docs/README.md`. The ordinary task rule (“change only these”) would prohibit the very experiment the goal requests. The fee/spec changes are not authorized product decisions; T171 is measuring whether the docs can guide a hypothetical change. `docs/reviews/doc-efficiency-plan.md`, required for baseline comparison, was not included (`tasks/T171/MANIFEST.md`).

### Exact replacement for T171 goal / scope exception

> This is a documentation-readiness **experiment**, not a product-code change. Start a clean disposable worktree from a named main commit and a fresh Sonnet session with only the root `CLAUDE.md` and repository. Allow that **one disposable worktree only** to edit any code, spec, migration, test, language template or locked plan file necessary to attempt the three hypothetical changes (fee boundary $1,500 → $1,600; a category job type with questions, qualification and tests; one English line with corresponding Spanish). Do not copy or merge any experimental source/spec/template edits into the task delivery, and never rewrite the real plan lock as a side effect of the drill. Run the targeted tests to record their **actual** outcomes; record time to first edit, tokens before first edit, files read in full, wide searches, guesses, missing documentation and where the worker got stuck. Discard the experimental branch/worktree. On the normal T171 delivery branch, create only `docs/audits/cold-start-drill.md`, `docs/README.md`, allowed root `CLAUDE.md` guidance and other explicitly approved **documentation-only shared-path** improvements. Compare measured tokens/time with the stated `doc-efficiency-plan.md` baseline when available; mark the metric unavailable otherwise. Verify the delivery diff has **zero** fee, license, question, test, migration, spec or template changes. The drill is not a Casey approval to change pricing.

**Owned files / mergeable budget:** retain 3 owned files and 300 changed lines plus existing allowed shared documentation paths. **Exception:** temporary scratch files are outside owned-file/locked-file checks **only while in the disposable worktree**; no exception to production branch checks. **Dependencies:** retain current documentation-readiness predecessors; do not require an additional task. **Test-design row:** none currently (`tasks/T171/TEST_DESIGN_ROWS.md` is empty). Add a *task acceptance check*, not a production code test: `git diff` of deliverable has no experimental product changes; report records three attempted edits, outcomes, measurements and unknown baseline honestly.

**Must not break:** append-only decisions, locked specs on the delivered branch, price tiers, legal qualification, Spanish copy, existing CI tests, no unauthorized spec change. Run final smoke/prehandin on **delivery docs branch**, and keep scratch test failures reported as observations, not merged.

**Can't tell / need:** `docs/reviews/doc-efficiency-plan.md` baseline, current price/category source and test paths, `docs/governance.md` branch/lock rules, current `kit/tasks.json`. Do not invent benchmark numbers.

**Casey decision:** none required to perform three *hypothetical* isolated experiments; any real fee-tier or category/spec change needs a separate, explicit Casey product decision.

---

## T392 — Offline sentinel tooling plus owner-run live vendor checkpoint

### Evidence / blocker

`tasks/T392/PROMPT.md:9-11` bundles implementing the offline sentinel test **and** performing a real Railway test-project run with every vendor. It itself states Claude never has Casey's sandbox credentials and must stop for those steps; `tasks/T392/TEST_DESIGN_ROWS.md:1` says the live run never belongs in CI. T392's owned files (`tasks/T392/MANIFEST.md`) include the script and a report document, not access to vendor consoles. Requiring the build worker to perform that live run makes the task impossible to finish on its own. Importantly, **“not observable” is not proof of absence**; the task prompt acknowledges this.

### Exact revised T392 goal

> Implement a **fully offline, deterministic sentinel scanner** that accepts documented vendor-export/fixture inputs, uses unique non-real marker kinds and an allowlist of permitted locations, never emits marker values in findings, and tests prohibited occurrences in app logs, Sentry, redacted Twilio bodies, Resend logs and AI request inputs. A missing or unexpectedly empty sink must be labeled `not observable` or failed, never `absent`. Publish an operator runbook / blank result template at `docs/phase2/sentinel-trace.md` listing the Railway **test** project, all seven named sink groups, collection instructions, allowed places, result fields, evidence references, and criteria for a `Verdict: clean` versus `Verdict: blocked`. Pass TD-PRIV-26 **offline** in CI. Do **not** call vendor APIs, run J1, require credentials, fill fake live results or mark the live vendor check completed in the worker's hand-in. The live evidence run is a separate **Casey owner-run Gate C checkpoint** after T136's journeys, T137's AI run and T276's Twilio redaction, with fake markers and sandbox-only credentials. Casey (or an authorized hands-on operator under Casey's control) runs the documented commands/exports, records actual sink-by-sink evidence, and appends the real verdict. Any forbidden marker yields a High finding, Gate C blocked and a specific fix/re-run plan.

**Owned files:** unchanged `tests/sandbox_sentinel/sentinel_trace.py`, `tests/sandbox_sentinel/test_sentinel_trace.py`, `docs/phase2/sentinel-trace.md`. **Dependencies:** leave prerequisites for scanner build alone; add a **Gate C owner-action dependency** on completion of T136/T137/T276 and this offline T392, plus the real sandbox audit and any required rerun. **Budget:** existing 3 files/450 changed lines. **No extra code task** solely to own Casey's credentials.

### TD-PRIV-26: old → new

- **Old:** offline fake sink/export rules and “The live run ... recorded in ... sentinel-trace.md and never runs in CI” (`tasks/T392/TEST_DESIGN_ROWS.md:1`).
- **New:** retain **every offline condition and negative fixture**; clarify **T392 passes on scanner/tests/runbook alone**, while the actual live `sentinel-trace.md` result is **Casey's Gate C owner-action acceptance**, not a T392 worker acceptance. Add a fixture that ensures `not observable` is not silently serialized as `absent` and a report with real evidence cannot be replaced by a sample template.

**Important unresolved gating rule:** the original text permits labeling a sink “not observable” while potentially issuing `Verdict: clean` solely because no observed forbidden markers were found. For security, the safer default is `Verdict: blocked` when any **required** sink has no usable evidence; if exceptions are ever allowed, they require a documented Casey risk decision. Do not silently change this release policy without a spec/decision amendment.

**Must not break:** fake-data-only requirement, no credentials in code/logs, no live APIs in CI, PRIV-006/016/017/027 and AI-004, sink-by-sink evidence, no values in alerts, High finding / rerun / Gate C block semantics.

**Can't tell / need:** current `docs/phase2/` and Gate C owner-checklist format, `docs/for-casey-review.md`, T136/T137/T276 completion status, sandbox vendor export permissions/API support, live credentials accessible **only to Casey**, true meaning and policy for unobservable mandatory sinks.

**Casey decisions:** (1) perform or authorize the hands-on sandbox inspection and sign its actual outcome; (2) decide whether **any** unobservable mandatory sink can be accepted at Gate C, documented as residual risk. Credentials must never be passed into an AI worker.

---

## Required plan bookkeeping before Phase 2

1. Update the real task goals, owned-path assignments, dependencies and budgets in `kit/tasks.json` / plan source. Use collision-free real numbers for any `TNEW-*` tasks. Do not edit locked source without following its governance and decision-log process.
2. Update the affected exact rows in `docs/test-design.md` (TD-PICK-61, TD-AI-29, TD-BIZ-36, TD-PRIV-26), add `TD-AI-29-CORE` and `TD-BIZ-57-UI` if the proposed split/UI task is approved, and **leave TD-SEC-78 and TD-BIZ-57's backend checks intact**.
3. Append decision entries documenting the ownership transfers, owner checkpoint, migration number `0219`, any launch-gate change, and any Casey-specific approval. Regenerate `kit/prompts/` via `scripts/gen_prompts.py` and rewrite the plan lock only under the repository's stated process.
4. Validate ownership and dependency DAG (no T299/T394 cycle), budgets and migration ordering, then use ordinary smoke/prehandin/CI on the *actual changed repository*. None was run here, because the ZIP contains only snapshots and plans.

## Decision list for Casey

- **Approve** a short T394 dispatcher-integration task and post-T299 ownership transfer (or verify that queued T299 can absorb the hook without a cycle).
- **Approve** a small BIZ-044 portal switch task, unless a presently queued task can be demonstrated to own the settings page and absorb it.
- **Perform** the T392 sandbox vendor leak inspection at Gate C and record each sink's actual observability; explicitly rule on any mandatory unobservable sink.
- **If necessary**, manually clear Resend suppression in the provider dashboard only after the T133 contract test confirms an unsupported API, and confirm the re-verification outcome. Do not grant the AI worker a live key.

**Final evaluation:** The six blockers are fixable at the plan level. T382, T133, T171 and the build portion of T392 can be corrected without a separate build task; T394 deserves a narrow post-T299 integration task; T685 requires a UI owner, preferably an existing queued one if verified, otherwise a small new task. **This is a review of a supplied package, not proof of repo implementation or test success.**
