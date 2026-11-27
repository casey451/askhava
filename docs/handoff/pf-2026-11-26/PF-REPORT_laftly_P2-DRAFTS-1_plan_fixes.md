# Laftly P2-DRAFTS-1 — Phase 2 plan amendments

**11 tasks drafted; 6 checklist items; 5 open choices.**

**Disposition: plan revision required before Phase 2 execution.** All 16 verified findings (9 CONFIRMED, 7 PARTLY) are mapped below. Source of truth: `reviews/P2_verdicts.md` (verified tree `ed430909b`); vendor capabilities below are from the bundled VENDOR-1 report, not new online verification. This is paste-ready proposed text, not a patch, execution result, or approval. The excerpt has only 19 task entries; T127, T141, T511, T130 and several queued producers/renderer owners are absent. Conditional manifest deltas are expressly not safe to apply until the complete graph and owner checks pass.

### Applying the draft

Order below is a topological order of the affected tasks using the excerpt’s direct dependencies, not a claim about the full graph. Before T127 sends the checklist, the overseer must approve the checklist changes below as an explicit plan edit (T122 DONE, T141 QUEUED); if T141 cannot precede T127 without a cycle, do not add that dependency: authorize the narrow checklist edit in the plan session and have T127 use the revised file. Do not reopen T122 silently. Before queueing, run full task validation, check ownership, transitive deps and context budget, allocate decision number, update locked rows only by decision, regenerate prompts and plan lock under the repository rules. No existing test may be weakened.


## T499 — Docs tooling (harness): lossless compaction check and writing standard

**Verdicts:** 3a-7, 3a-11.

**Evidence:** `ci.yml:72,146-149`; `kit/prompts/T499.md:9`; `docs/code-docs.md` writing standard. No requirement IDs assigned to T499.

### Replacement `goal`

```text
Plan change D-3856 (Casey, Nov 15, 2026: at the end of phase 1 every document needed for upkeep is "simplified and organized in a way that's most efficient", nothing left out, nothing "wordy for no reason"). Following `scripts/split_crate_doc.py --verify` (D-3474) and `scripts/split_arch_notes.py --verify` (D-3620). (1) `scripts/check_doc_compaction.py --map FILE [--base REF]`, standard-library Python. The map records `base` (the SHA it was made against, the default REF). For every file the map names, each line present at the base and gone or changed now must be listed (by SHA-256) with targets `path:start-end` that exist now. Each fact token of the old line must appear inside those ranges, not merely somewhere in the file. Fact tokens are requirement, TD-, D- and task IDs, paths, backticked names, numbers and amounts, quoted text, and the words never, not, no, only, must, may, unless, except, before and after. A line with no fact tokens needs a target range sharing at least 60% of its content words. A removed line that is blank, or that occurs as a whole line, byte for byte, inside its target range, passes without the token or word test (so blank lines and byte-identical section moves pass); map entries are a list, so identical removed lines are each listed or share one entry with several targets (D-3892). A byte-identical move (`moves`: old path to new path) passes when the bytes match. A map entry for a line that was not removed fails. The map may carry a `left` list (work deferred to T142), which the check ignores; the script's docstring states the map schema (`base`, the removed-line entries, `moves`, `left`), because T507 to T514 do not read the script. Across the whole tree, any backticked `path:N` or `path:N-M` cited outside `docs/decisions.md` (spec `Source:` lines, `kit/tasks.json`, crate `CLAUDE.md`, `docs/test-design.md` and the rest) whose text at those lines differs from the base fails; a citation of a path listed in `moves` is compared at its new path, and a cited line missing at both the base and now is not a difference (stale citations past a file's end exist, for example `app/jobs/CLAUDE.md:44`, a 29-line file). (2) The decision-digest check is T720's (D-4447). (3) Tests in `tests/test_check_doc_compaction.py`: each failure above, including a tokenless deletion, a token found only outside its range and a changed cited line, plus passing cases (among them a section with blank and repeated lines moved byte-identical to a new file), one of them a branch that merged a cited-line change from `main`, and one that runs the step on a merged commit on `main` carrying a map. (4) A `Ci` step runs `check_doc_compaction.py` on every map under `docs/reviews/doc-maps/` that the pull request adds or changes. A retry branch merges `main` in, so `Ci` takes the base as `git merge-base origin/main HEAD`, requires the map's recorded base to be its ancestor, and compares only the changes between that merge base and HEAD. Lines or cited code changed on `main` since the recorded base are not flagged, and a merged map never breaks later runs. On a run whose HEAD equals origin/main (a run on main: the nightly calls `Ci` through `workflow_call`, and `ci.yml` pushes only `claude/**` and `dependabot/**`), the base is HEAD^1, the commit before the merge. If no map changed in that commit, the step is skipped. (5) `docs/code-docs.md` gains a short writing standard: say a fact once, in the shortest clear form, where its reader looks first; link, don't repeat; keep IDs and cited lines stable; respect the size budgets T514 adds. Rewrite `kit/plan.lock`; the new scripts are covered by its `scripts/*.py` glob. Plan change D-4447 (Casey, Nov 18, 2026, about 7:40 am MST, asked "Start the digest early?", answered "Yes, start it now (Recommended)"): the digest check, its tests and the digest plan moved to T720 so the digest pieces run during stage 5; this task keeps the compaction check, its `Ci` step and the writing standard after T126. Plan change D-5676 (token efficiency and phase-2 pre-flight, checked on `main` 025855b2): Before the ancestor check, if the recorded base is not in the fetched history, `git merge-base` fails, or ancestry cannot be proved, the step deepens `origin/main` with `git fetch --unshallow` (or repeated `--deepen=200`), using the same `GH_TOKEN` URL as the "Fetch main" step (`.github/workflows/ci.yml:146` to `:149`), because `Ci` fetches 200 commits (`:72` checkout, `:149` fetch); the map records no date, so `--shallow-since` is not used. A test covers a base outside a 200-commit checkout. Retry merge-base and the ancestor proof after deepening; if the proof still fails, fail the check explicitly, not skip or compare against an unproved base. Add a shallow-history fixture where the recorded base is present but ancestry cannot initially be proved, and a case that remains unprovable.
```

### Replacement `acceptance`

```text
All original compaction tests pass; both shallow-history fixtures recover the correct base when possible; an unprovable base fails with a clear diagnostic. No changed map or cited line escapes checking.
```

### Manifest changes

The JSON below is a delta, not a new manifest schema: append `add` values uniquely to the existing array; remove only `remove` values. Keep all other entries, requirements, tests and decisions. For entries without an existing `acceptance`, add the proposed field only if the current validator permits it; otherwise put its exact text into the goal and generated hand-in section.

```json
{
  "owns": {
    "add": [],
    "remove": []
  },
  "reads": {
    "add": [],
    "remove": []
  },
  "deps": {
    "add": [],
    "remove": []
  }
}
```

### Test design

No locked row change. Retain every existing case.




## T129 — Twilio messaging sandbox behind the adapter; contract tests against it

**Verdicts:** 3a-1, 3a-8.

**Evidence:** `BUILD_BRIEF.md:62-65,184,189-191,516-517`; `docs/account-checklist.md:33-34`; `app/adapters/src/sms.rs:72-81`; `app/adapters/src/tests/adapter_config.rs:359-364`; vendor report Twilio section.

### Replacement `goal`

```text
Phase 2 step 1-2: Twilio test credentials and magic numbers; any difference fixed in the adapter and stand-in together. This harness task also gives `tests/contract/` a sandbox mode (`CONTRACT_BASE_URL`, credentials from the environment, never printed) that the later sandbox tasks use (D-491). The sandbox check covers T275's `SmsBodyDelete` (D-1841). Nov 11 vendor-double review batch 4, F2 (D-2761): in sandbox mode, a Messages create carrying T377's `StatusCallback` is accepted by Twilio's test credentials, and the adapter sends it on every create from a client composed with a public base URL; the result is recorded in the decision. Existing legacy Twilio test credentials, if available, prove only simulated Messages creation and magic-number errors, including acceptance of the StatusCallback parameter; they cannot prove callback delivery or message-body update/redaction. Retain the T275 redaction and callback behavior in the stand-in tests, and record those real-service cases NOT RUN unless an owner-authorized test-project run through T130 supplies evidence. Do not substitute regular credentials for missing magic-number credentials. Sandbox mode is opt-in with CONTRACT_BASE_URL and the appropriate host-locked build network secret; without either, mark the vendor cases skipped (normal keyless CI). Keep the offline stand-in contract tests running and passing separately. At hand-in enumerate each actual vendor case with PASS, FAIL or NOT RUN (credentials / endpoint / vendor limitation / needs owner) in docs/for-casey-review.md; a skipped case or stand-in pass never proves a real vendor case. A supported case with credentials and endpoint that fails is a failure, not NOT RUN. Never print credentials or use live keys/endpoints. Real texts, calls or Verify deliveries to a real phone are Casey-only stops through T130; no worker acceptance sends one.
```

### Replacement `acceptance`

```text
All existing offline checks and applicable named TD cases pass unchanged; each supported, authorized vendor case actually run passes; all remaining vendor cases are individually NOT RUN with reason and owner follow-up, never counted as passed. StatusCallback is present on every applicable create; omission for a client composed without a public base URL remains valid.
```

### Manifest changes

The JSON below is a delta, not a new manifest schema: append `add` values uniquely to the existing array; remove only `remove` values. Keep all other entries, requirements, tests and decisions. For entries without an existing `acceptance`, add the proposed field only if the current validator permits it; otherwise put its exact text into the goal and generated hand-in section.

```json
{
  "owns": {
    "add": [],
    "remove": []
  },
  "reads": {
    "add": [
      "docs/account-checklist.md"
    ],
    "remove": []
  },
  "deps": {
    "add": [],
    "remove": []
  }
}
```

### Test design

No locked row change. Retain every existing case.




## T131 — Twilio voice and Verify sandbox behind the adapters

**Verdicts:** 3a-1; preserves PF-P2c T382.

**Evidence:** `BUILD_BRIEF.md:191`; `docs/spec/calls.md:5-21`; `docs/account-checklist.md:31-58`; `docs/handoff/PF-P2c_phase2_blocker_drafts.md` T382; vendor report Twilio section.

### Replacement `goal`

```text
The Twilio voice and Verify sandbox clients sit behind the same adapter traits as the stand-ins, so masked calls from the fixed pool (never recorded, never buying numbers) and Verify codes are covered by offline contract tests, with actual vendor cases run only where the authorized test service supports them (CALL-001, CALL-002, CALL-006, CALL-017). Nov 11 vendor-double review batch 4, F3 and F10 (D-2762, D-2769): (1) in sandbox mode, a Calls create with T378's script, `StatusCallback`, `StatusCallbackEvent` and detection parameters is accepted and one with none of `Url`, `Twiml`, `ApplicationSid` is refused, both recorded in the decision; (2) Verify fidelity (https://twilio.com/docs/verify/api): `stand_ins/twilio.py`'s `start_verification` (`stand_ins/twilio.py:367`) answers a send to the same `To` and channel while a pending verification is under 10 minutes old with that verification (same SID, same code, check attempts kept), refuses a sixth send of one verification with 60203 and a sixth check with 60202 (both 429), and refuses the email channel unless the stand-in's Verify service has an email integration (on by default, a control turns it off; the control goes in `stand_ins/server.py`'s `ROUTES` table, owned here because T133 and T134 also add routes there, D-2821); `app/adapters/src/verify.rs` maps 60202 and 60203 to a typed too-many-tries error that T382's callers turn into a cooldown (any other error unchanged); `tests/contract/test_sandbox_t131.py` records what the real Verify service does on a resend within 10 minutes and on the email channel without and, once Casey has set it up (T122's item), with the SendGrid integration; a case that cannot run yet is reported as not run, never as passed, and listed in `docs/for-casey-review.md`. Plan change D-5676 (token efficiency and phase-2 pre-flight, checked on `main` 025855b2): `AdapterError` (`app/adapters/src/base.rs:61` to `:91`) has no too-many-tries variant and `base.rs` is not this task's, so the typed check is a method: `verify.rs` adds an inherent `pub fn is_too_many_tries(&self) -> bool` on `AdapterError` (true for `Vendor` code 60202 or 60203), which callers reach through the existing `AdapterError` export in `app/adapters/src/api.rs`; `base.rs` and its variants are unchanged, and T382's callers test the error with it. The `start_verification` cite is refreshed. Legacy Twilio test credentials simulate Calls creation parameter acceptance/refusal only, not actual AMD, TwiML execution or masked bridging, and cannot use Verify. Verify SMS, voice and email behavior (including resend and integration checks) needs the isolated real test sub-account and owner-authorized deliveries through T130; record NOT RUN otherwise. Keep all deterministic stand-in tests and typed errors for T382; T382 exercises the five-try local limit separately from an allowed provider check returning 60202, never a sixth real check. Sandbox mode is opt-in with CONTRACT_BASE_URL and the appropriate host-locked build network secret; without either, mark the vendor cases skipped (normal keyless CI). Keep the offline stand-in contract tests running and passing separately. At hand-in enumerate each actual vendor case with PASS, FAIL or NOT RUN (credentials / endpoint / vendor limitation / needs owner) in docs/for-casey-review.md; a skipped case or stand-in pass never proves a real vendor case. A supported case with credentials and endpoint that fails is a failure, not NOT RUN. Never print credentials or use live keys/endpoints. Real texts, calls or Verify deliveries to a real phone are Casey-only stops through T130; no worker acceptance sends one.
```

### Replacement `acceptance`

```text
All existing offline checks and applicable named TD cases pass unchanged; each supported, authorized vendor case actually run passes; all remaining vendor cases are individually NOT RUN with reason and owner follow-up, never counted as passed. Stand-in retains attempts across resends and maps typed 60202/60203; no actual call, Verify code or email to a real recipient is required of the worker.
```

### Manifest changes

The JSON below is a delta, not a new manifest schema: append `add` values uniquely to the existing array; remove only `remove` values. Keep all other entries, requirements, tests and decisions. For entries without an existing `acceptance`, add the proposed field only if the current validator permits it; otherwise put its exact text into the goal and generated hand-in section.

```json
{
  "owns": {
    "add": [],
    "remove": []
  },
  "reads": {
    "add": [
      "docs/account-checklist.md"
    ],
    "remove": []
  },
  "deps": {
    "add": [],
    "remove": []
  }
}
```

### Test design

No locked row change. Retain every existing case.




## T132 — Stripe test mode behind the adapter; contract and webhook tests against it

**Verdicts:** 3a-1, 3a-2, 3a-3.

**Evidence:** `app/portals/src/confirm_pay.rs:860-872,1790`; `app/payments/src/charge.rs:222`; `docs/spec/fees.md:53` FEE-049; `kit/prompts/T132.md:205-212`; verified report cites `tests/integration/tests/confirm_and_pay.rs:273-312` (omitted from archive).

### Replacement `goal`

```text
The Stripe test-mode client sits behind the payments adapter, and applicable real test-mode contract and webhook cases are run with authorized test credentials and endpoints: cards only through Elements and the authorization stored, signed webhooks processed once, idempotency keys on every call, and the ledger reconciling to Stripe test mode (FEE-014, FEE-015, FEE-028 to FEE-030, FEE-032, FEE-036, SEC-016 to SEC-018). `PaymentsAdapter::create_refund` (ID only) is not retired here: it stays in the trait beside `send_refund` (T258's typed refund), once the Stripe test-mode refund shapes are checked (D-1753, D-5676). On `main` 025855b2 no production code calls `create_refund` except `send_refund`'s provided body (`app/adapters/src/payments.rs:381`) and the forwarding impls (`app/payments/src/off_session.rs:79`, `app/portals/src/confirm_pay.rs:844`; `app/server/src/business_signup.rs:1467` implements it returning `NotSent`), which stay while the method is required. Leave them, confirm with `git grep -n '\.create_refund(' app/` that no other caller exists, change nothing outside the owned files for it, and reword the "T132 removes it" lines (`app/adapters/src/payments.rs:11`, `:374`; `app/adapters/NOTES.md:26`; `app/payments/NOTES.md:227`) to say a later cleanup task removes it (D-5676). Nov 10 code review, payments F4, and the plan review's M1 (D-2624, D-1476; FEE-031, FEE-049): the PaymentIntent is created with no description and no statement-descriptor suffix (`create_unconfirmed_intent`, `app/adapters/src/payments.rs:440`), so the card statement shows no job number; D-1476 gave this to T132 and its goal never said so. `payments` holds no job number and may not depend on `jobs` (`kit/layers.json`), so the number comes from the Confirm route: `charge` gains `open_intent_described` (and `confirm_and_pay` a matching entry) taking `payments::api::IntentText`, the existing `open_intent` and `confirm_and_pay` keep their signatures and behaviour for the done tests, and `app/portals/src/confirm_pay.rs` (T261's, which runs first) builds `IntentText::for_job` from the job number it already shows and calls the described entry; `IntentStripe` gains a method with a default body carrying the text, so the done tests' fakes compile unchanged, and `intent_http.rs` overrides it; `Catching` in `app/portals/src/confirm_pay.rs:860-872`, used by the Confirm press at :1790, also overrides the described method and forwards the same IntentText unchanged to its inner Stripe transport; retain the default for existing fakes; the create form sends `description` from `IntentText::description()` and `statement_descriptor_suffix` from `IntentText::statement_suffix()`, never a brand literal in the adapter (T295 makes brand literals fail); TD-FEE-38's stripe-mock case checks both fields, and the contract run also confirms stripe-mock returns `latest_charge` on a succeeded intent (T352's field). Nov 11 vendor-double review batch 4, F7 (D-2766): `confirm_payment_intent` (`app/adapters/src/payments.rs:616`, through `confirm_with`, `:646`) confirms with `payment_method` only, while the page hands a `requires_action` intent to Stripe.js `handleNextAction` (`app/portals/static/confirm_pay.ts:112`), which needs the `use_stripe_sdk` next action that a server-side confirm gets only when it sends `use_stripe_sdk=true`; the fake answers `use_stripe_sdk` whatever is sent (`stand_ins/stripe.py:292` to `:293`). The confirm sends `use_stripe_sdk=true`: `confirm_with` adds it to every on-session confirm (both `confirm_payment_intent` and `confirm_payment_intent_saving`, `app/adapters/src/payments.rs:632`, used by `app/server/src/lib.rs:381` and `:396`), never to an off-session confirm (PICK-048's `confirm_off_session`), and the adapter test covers both; the stand-in reads the form field as the HTTP string `"true"` and its case sends it so; the fake answers a 3-D Secure confirm without it with `next_action.type` `redirect_to_url`, so the adapter test fails before the fix; and in sandbox mode `tests/contract/test_sandbox_t132.py` confirms with a 3DS2 test card (https://docs.stripe.com/testing#regulatory-cards) and asserts `requires_action` with `next_action.type` `use_stripe_sdk`; any other answer is fixed in the adapter and the fake together and recorded in the decision. D-4349: T664 also changes `app/portals/src/confirm_pay.rs` (the Confirm press's PICK-020 re-check, its card and charging-pause checks and its move to Payment pending), so this task runs after it and keeps those calls. Plan change D-5676 (token efficiency and phase-2 pre-flight, checked on `main` 025855b2): `create_refund` is a required `PaymentsAdapter` method (`app/adapters/src/payments.rs:366`; `send_refund`'s provided body `:376` to `:392` calls it) with about 60 implementors, nearly all in test files this task does not own, so it is not retired here. As above, the production forwarding impls stay and only the "T132 removes it" lines are reworded; `create_refund` stays in the trait; its removal and the implementor clean-up go to a later cleanup task that owns those files. This task also owns `tests/contract/test_stripe_confirm.py` (T260's, done): its 3-D Secure case (`test_the_3ds_card_asks_for_approval_then_the_bank_answer_settles_it`, the `next_action` assert at `:111`, through the `confirm` helper at `:36` to `:37`) confirms without `use_stripe_sdk` and asserts `next_action.type` `use_stripe_sdk`; once the fake answers `redirect_to_url` without the flag, that case confirms with `use_stripe_sdk=true` and keeps its assertion, and a new case asserts `redirect_to_url` for a 3-D Secure confirm without the flag. The cites above are refreshed (`create_unconfirmed_intent`, `confirm_payment_intent` and `confirm_with`, `handleNextAction`, the fake's `use_stripe_sdk` answer). Add a Rust regression in the already-owned `app/portals/src/confirm_pay.rs` (inline test module or extend an owned test module) that creates an intent through the actual Confirm press/Catching path, asserting the job-number text survives into the transport. Add a Rust adapter regression in the owned `app/adapters/src/payments.rs` that captures the outgoing form and asserts both `description` and `statement_descriptor_suffix`, using the real adapter form builder and a local fake HTTP receiver. Add the touched crate tests to the generated Run your tests commands, alongside the two existing Python commands; run `cargo test -p askhava-adapters` and `cargo test -p askhava-portals` (confirm package names against Cargo.toml). Preserve TD-FEE-38 and the existing integration tests. Stripe-mock is a stateless schema/form check, not proof of lifecycle, ledger reconciliation or genuine webhook delivery; test those against Stripe test mode separately. A stripe-mock response missing latest_charge is reported as a mock limitation, not proof of real vendor behavior; preserve the field assertion in deterministic tests and test-mode verification. Sandbox mode is opt-in with CONTRACT_BASE_URL and the appropriate host-locked build network secret; without either, mark the vendor cases skipped (normal keyless CI). Keep the offline stand-in contract tests running and passing separately. At hand-in enumerate each actual vendor case with PASS, FAIL or NOT RUN (credentials / endpoint / vendor limitation / needs owner) in docs/for-casey-review.md; a skipped case or stand-in pass never proves a real vendor case. A supported case with credentials and endpoint that fails is a failure, not NOT RUN. Never print credentials or use live keys/endpoints. Real texts, calls or Verify deliveries to a real phone are Casey-only stops through T130; no worker acceptance sends one.
```

### Replacement `acceptance`

```text
All existing offline checks and applicable named TD cases pass unchanged; each supported, authorized vendor case actually run passes; all remaining vendor cases are individually NOT RUN with reason and owner follow-up, never counted as passed. Confirm press forwards IntentText; captured Rust create form has both fields; TD-FEE-38 remains green and actual test-mode lifecycle evidence is distinct from stripe-mock.
```

### Manifest changes

The JSON below is a delta, not a new manifest schema: append `add` values uniquely to the existing array; remove only `remove` values. Keep all other entries, requirements, tests and decisions. For entries without an existing `acceptance`, add the proposed field only if the current validator permits it; otherwise put its exact text into the goal and generated hand-in section.

```json
{
  "owns": {
    "add": [],
    "remove": []
  },
  "reads": {
    "add": [
      "docs/account-checklist.md"
    ],
    "remove": []
  },
  "deps": {
    "add": [],
    "remove": []
  }
}
```

No new owned files: use inline tests in the owned modules. Do not edit the DONE confirm_and_pay integration file. Retain the logged T260 Python fixture change and new negative case. Update generated `Run your tests` with the two Rust crate commands (not just prose).

### Test design

No locked row change. Retain every existing case.




## T133 — Resend and Lob test mode behind the adapters

**Verdicts:** 3a-1, 3a-4, 3a-9; folds PF-P2c T133.

**Evidence:** `docs/spec/businesses.md:60` BIZ-056; `app/adapters/src/email.rs:90-101`; `app/adapters/src/tests/ssrf_guard.rs:427-439`; `docs/handoff/PF-P2c_phase2_blocker_drafts.md` T133; vendor report Resend/Lob sections.

### Replacement `goal`

```text
The Resend and Lob test-mode clients sit behind their adapters with signed webhooks (Svix for Resend) and the signed inbound-email route into `privacy`, and the identity chain's email and letter steps and the email fallback for failed texts are exercised against them where the authorized test service supports those cases (BIZ-001 to BIZ-003, BIZ-016, TXT-014, TXT-015, SEC-039, PRIV-026). The fake Resend gains the suppression-list endpoints (list, and delete for one address), routed with this task's controls in `stand_ins/server.py`'s `ROUTES` table (owned here, after T131 and before T134, which also add routes there, D-2821), and BIZ-056 re-verification requests suppression removal before the verification email can be sent; BIZ-056 moved here from T149 because the fake endpoint must exist first and T149 runs before this task (TD-BIZ-36, D-492). The opaque request is added to `app/businesses/src/email_verification.rs` (T196's, extended here) and tested in `tests/integration/tests/email_reverify.rs` (D-493). Through a durable core task/event keyed by opaque mailbox ID and verification attempt, the messaging-owned `app/messaging/src/unsuppress.rs` handler reads the address through authorized privacy::api, calls EmailAdapter::remove_suppression with no DB transaction held, and writes a success audit row only after provider acknowledgment. Businesses never reads plaintext or calls messaging/adapters directly. Gate the verification send on successful removal or recorded manual resolution; deduplicate retries. A provider error keeps work pending/retryable, with one High owner alert and no success audit, and never exposes an address in logs. Keep T584 transactional settle intact. Nov 11 vendor-double review batch 4, F6 and the Lob note (D-2765, D-2770): (1) the fake Resend keeps a team-wide suppression list (`stand_ins/resend.py:110` has none): a send to a listed address is accepted (an ID returned) and emits `email.suppressed`, never `email.delivered`; a hard bounce lists the address; a control lists one; and a control makes a send emit `email.failed`; (2) in sandbox mode `tests/contract/test_sandbox_t133.py` records whether Resend's API offers removing one suppressed address, which BIZ-056's `DELETE /suppressions/{address}` (`app/adapters/src/email.rs:90-101`) assumes; the bundled vendor fact check documents removal by ID or email, so exercise the actual documented API in an isolated test team; if the endpoint is actually unsupported, only the real client (`resend_real.rs`) returns a typed unsupported error; retain the stand-in ResendEmail DELETE and the DONE `ssrf_guard.rs:427-439` test unchanged, BIZ-056's re-verification raises one High alert asking Casey to remove the address in Resend's dashboard and writes no removal audit row it did not make, and the gap goes to `docs/for-casey-review.md`; (3) it records the unit of Lob's `Lob-Signature-Timestamp` (`app/adapters/src/signatures.rs` reads unix seconds) and, if Lob sends another unit, fixes the reader and the fake Lob together with a test that fails before the fix. T379 counts suppressed and failed emails as not reached. Plan change D-3155: its inbound-email route hands each email `privacy` has read to T151's help-request and opt-out entry point (CON-035's detector, `RevokeChannel::Email` or `EmailReply`, confirmed by email). Plan change D-4052: T584 runs first and changes `email_verification::confirm` so the sign-up settle runs inside the code's transaction; keep that. Actual authorization and provider errors are not unsupported success; keep a distinct failure path. Lob webhook timestamp units must be observed from a genuine subscribed test webhook, not a debugger event with its separate signing key. Test-mode rendering is not postal delivery or real address verification; absent a public test endpoint record those cases NOT RUN. Sandbox mode is opt-in with CONTRACT_BASE_URL and the appropriate host-locked build network secret; without either, mark the vendor cases skipped (normal keyless CI). Keep the offline stand-in contract tests running and passing separately. At hand-in enumerate each actual vendor case with PASS, FAIL or NOT RUN (credentials / endpoint / vendor limitation / needs owner) in docs/for-casey-review.md; a skipped case or stand-in pass never proves a real vendor case. A supported case with credentials and endpoint that fails is a failure, not NOT RUN. Never print credentials or use live keys/endpoints. Real texts, calls or Verify deliveries to a real phone are Casey-only stops through T130; no worker acceptance sends one.
```

### Replacement `acceptance`

```text
All existing offline checks and applicable named TD cases pass unchanged; each supported, authorized vendor case actually run passes; all remaining vendor cases are individually NOT RUN with reason and owner follow-up, never counted as passed. TD-BIZ-36 passes with acknowledged removal before email dispatch, one success audit, and unsupported/failure cases with no success audit and one High owner alert. Existing ResendEmail DELETE and SSRF tests pass unchanged.
```

### Manifest changes

The JSON below is a delta, not a new manifest schema: append `add` values uniquely to the existing array; remove only `remove` values. Keep all other entries, requirements, tests and decisions. For entries without an existing `acceptance`, add the proposed field only if the current validator permits it; otherwise put its exact text into the goal and generated hand-in section.

```json
{
  "owns": {
    "add": [
      "app/messaging/src/unsuppress.rs"
    ],
    "remove": []
  },
  "reads": {
    "add": [
      "docs/account-checklist.md"
    ],
    "remove": []
  },
  "deps": {
    "add": [
      "T176",
      "T196"
    ],
    "remove": []
  }
}
```

Budget delta: `{"budget":{"files":11,"lines":520}}` (preserve context_pct pending remeasurement). Earlier handoff counted nine owned paths; this excerpt already has ten. Shared exports, module declarations and worker registration only if permitted by the current shared-path rules. Verify ordering against the actual messaging worker/event owner; if an unshared file is required, transfer ownership explicitly or split the messaging handler into a predecessor. Do not use direct cross-layer calls.

### Test design

`docs/test-design.md:658` (original line in excerpt).

**Old:**

```text
658: | TD-BIZ-36 | B | In, C | BIZ-056 | A re-verification of an address the provider suppressed removes the suppression through the email adapter first, against the fake Resend's suppression endpoint, and writes an audit row (moved from TD-BIZ-33) (D-492) |
```

**New:**

```text
| TD-BIZ-36 | B | In, C | BIZ-056 | A re-verification of an address the provider suppressed emits a durable opaque mailbox operation. The messaging handler reads the address via privacy::api, removes the suppression through the email adapter against fake Resend before allowing verification email dispatch, and writes one success audit row after acknowledgment. Unsupported or failed removal keeps work pending, raises one High owner alert and writes no success audit row. No plaintext reaches businesses or logs (moved from TD-BIZ-33) (D-492) |
```

Reason: Preserve the success assertion and add privacy and failure coverage; the earlier PF-P2c decision requires the messaging boundary.




## T134 — License sources and object storage behind the adapters

**Verdicts:** 3a-1.

**Evidence:** `docs/spec/privacy.md:33` PRIV-029; `BUILD_BRIEF.md:191`; `docs/account-checklist.md:74`; vendor report B2 section.

### Replacement `goal`

```text
The real license-source and object-storage clients sit behind their adapters: daily lists and the throttled lookup work against the real sources for verification only, and the audit stream and exports land in the bucket through a write-only, prefix-scoped token (CARD-015, LIC-006, LIC-007, LIC-010, LIC-027, OPS-010, OPS-011, PRIV-029). The real store also implements T275's `StorageDelete` (D-1841). The `photos/` prefix uses its own key with read, list and delete; PRIV-029's write-only, prefix-scoped token, versioning and object lock stay in force for the audit-stream and export (backup) prefixes (D-1841). Plan change D-2954 (`sources/01_spec/Changes_Since_v9.3_AUTHORITATIVE.md:250`): the real client also implements T385's `LicenseRecords::lookup_record` and answers in T385's `RECORD_HEADER` format, one record per call, only for a signing-up business's own license, nothing kept (LIC-010): for `roc`, that license's row of the ROC's posted download list or, where that lacks the contact, the ROC's public detail of that one license (a script-built page keyed by an internal license ID, not the six-digit license number, so the client maps the number to it), mapping the ROC's six-digit number format to and from the normalised number; for `city`, that number's row of the latest city spreadsheet; a field the source does not publish (an email, the phone kind, a DBA, a street address) is left empty; a missing license is a 404. Plan change D-5554: like `LocalStorage::read_object` after T779, the real store's read maps only a not-found answer to `Vendor { status: 404, code: None }`; every other read failure is `OutcomeUnknown`, never a 404. Use an isolated fake-data B2 test bucket, not production storage. Verify prefix-scoped audit/export writers cannot read, list or permanently delete, and cannot hide/overwrite evidence contrary to the Object Lock policy; check actual retention/versioning/Object Lock separately, not merely key capabilities. Verify the separately scoped photos credential performs the required deletion without audit/export access. Never substitute a broad key. Sandbox mode is opt-in with CONTRACT_BASE_URL and the appropriate host-locked build network secret; without either, mark the vendor cases skipped (normal keyless CI). Keep the offline stand-in contract tests running and passing separately. At hand-in enumerate each actual vendor case with PASS, FAIL or NOT RUN (credentials / endpoint / vendor limitation / needs owner) in docs/for-casey-review.md; a skipped case or stand-in pass never proves a real vendor case. A supported case with credentials and endpoint that fails is a failure, not NOT RUN. Never print credentials or use live keys/endpoints. Real texts, calls or Verify deliveries to a real phone are Casey-only stops through T130; no worker acceptance sends one.
```

### Replacement `acceptance`

```text
All existing offline checks and applicable named TD cases pass unchanged; each supported, authorized vendor case actually run passes; all remaining vendor cases are individually NOT RUN with reason and owner follow-up, never counted as passed. Real source verification and isolated B2 checks are distinguished from fixture results; PRIV-029 and photo isolation negative tests remain required.
```

### Manifest changes

The JSON below is a delta, not a new manifest schema: append `add` values uniquely to the existing array; remove only `remove` values. Keep all other entries, requirements, tests and decisions. For entries without an existing `acceptance`, add the proposed field only if the current validator permits it; otherwise put its exact text into the goal and generated hand-in section.

```json
{
  "owns": {
    "add": [],
    "remove": []
  },
  "reads": {
    "add": [
      "docs/account-checklist.md"
    ],
    "remove": []
  },
  "deps": {
    "add": [],
    "remove": []
  }
}
```

### Test design

No locked row change. Retain every existing case.




## T236 — AI-call budgets per conversation and per job, and no AI call before the area check

**Verdicts:** 3b-1, 3b-3.

**Evidence:** `app/ai/src/service.rs:52-61`; `app/ai/src/tests/adapter_calls.rs:184`; verified verdict lists three DONE integration literals, including inbound_scrub_context; `app/ai/src/switch.rs:243-248`; `docs/spec/ai.md:25,32-33`.

### Replacement `goal`

```text
Credit-drain defense for the war games (D-1154): AI-021's global daily cap stays, and below it AI-028's per-step budgets apply: no AI call before the consent text and the area check; 2 readings per inbound message, 1 reword per bank question (at most 8), 1 follow-up, card and project file 1 each plus 1 per EDIT (at most 3), 4 concept prompts, 2 screening calls a photo for at most 3 photos; backstops of 40 calls per conversation per Arizona day and 60 per job, with the card call always reserved so no budget lands on it; a verified web-chat session shares its phone's budget and an unverified one makes no call; quote reads 3 per business per job, outside the job's budget; only the first 1,600 characters of a scrubbed message are sent, while the AI-off card keeps her full scrubbed words. Past a budget the step runs on the AI-off path (INT-021) with nothing said and an `ai.call_stat` row with outcome `budget`; `models::calls_today` (`app/ai/src/models.rs`) excludes `budget` rows as it excludes `daily_cap` rows, so refused calls never use up the AI-021 cap. Keep `IntakeCall` and its existing field set unchanged. Add a separate typed budget context (conversation, job, step and for quote reads business) to the new pool-taking entry; the caller in `intake` passes that context, and the existing connection-taking compatibility wrapper passes no context; no production path may use the wrapper or bypass reservations; the budgets are settings in `AiConfig`; the migration adds the `budget` outcome and the conversation, job, step and business columns to `ai.call_stat`. The global daily cap (AI-021, D-811) becomes an atomic admission: before the provider call, one statement reserves a slot for this Arizona day (a counter row per day updated with `... WHERE used < cap RETURNING`, or an equivalent single-statement reservation) on its own short transaction, so concurrent calls past the cap never reach the transport; a retry of one logical call uses its reservation, an abandoned or timed-out reservation still counts for that day, and the `ai.daily_cap` alert is raised once a day under the same race (D-1757). No database connection or open transaction is held across an AI provider await (AI-029, D-2038): the entry the app calls takes the pool and reads the AI-008 switch; in one short atomic reservation transaction, check global cap availability (without consuming a slot), reserve all applicable step budgets and reserve the global daily admission last, committing only if all succeed. If any reservation refuses, roll back all attempted reservations and record the correct refusal outcome separately; the global cap takes precedence when already exhausted, as TD-AI-18 requires. Release the connection, awaits the provider (up to AI-023's timeout, retries and backoff included) with no connection held, then takes a connection again to record the call. Within the short admission transaction, every AI-028 budget check is a single-statement reservation like the AI-021 admission (only while the budget has room), so concurrent readings of one message (a duplicate or replayed webhook) cannot pass its 2-reading limit, the 1-reword limit or the per-day and per-job backstops together; a refused reservation makes no call and records outcome `budget`. Intake has no caller that applies a reading yet (`app/intake/src/question_bank.rs` calls only `accept_question`), so this task builds the first one in `app/intake/src/service.rs`: it reads one inbound message through the gated `read_intake` and applies the reading only if the step is still current on the new connection, which means the conversation has no inbound message and no `intake.intake_answer` row newer than the message the call was made for; otherwise the reading is recorded and discarded. Calling that function from the text and web-chat paths is T299's (IC-23, D-2065); the AI writing calls are IC-53's (no owner yet). `app/ai/src/switch.rs` (the gated `read_intake`) is this task's for that change. Other tasks' tests call `read_intake` with a `&mut PgConnection` (`app/ai/src/tests/adapter_calls.rs`, `tests/integration/tests/ai_zero_retention.rs`, `tests/integration/tests/prop_prompt_injection.rs`, `tests/integration/tests/inbound_scrub_context.rs`) and are not this task's to change: keep that form compiling for them as a thin wrapper that no app code calls. AI-029's other half, the inbound-text webhook committing before any AI call, is T299's, which wires the AI reading into the text path (IC-23, D-2065); that path makes no AI call yet. Ordering only: runs after T370, which changes one call in `app/intake/src/service.rs` (D-2703); its migration is `0210_ai_call_budget.sql` (V9, D-2707). Plan change D-3675: the migration is numbered 0209 (it was 0202; D-3881 moved it up one more, so T527's 0208 fits before it; D-3675 made it 0204, D-3722 moved it up one more, D-3750 one more, after T470's `0207_messaging_email_lease.sql`, D-3753 one more, after T480's `0208_messaging_spanish_switch.sql`, and D-3779 one more, because T486 took `0190_catalog_switched_on.sql` and every queued migration from 0190 up moved up one) because T456's `0205_picks_hold_away.sql` and `0206_businesses_away_busy.sql` run before this task in the serial order (`scripts/validate_tasks.py` V9). Plan change D-3883: this task's migration moved up one, to `0210_ai_call_budget.sql`, for T529's `0197_payments_refund_owner.sql` (V9); earlier numbers in this goal are history. Plan change D-4292: this task's migration moves to `migrations/0212_ai_call_budget.sql` for T645's `0210_messaging_web_chat_optout.sql` and `0211_intake_web_chat_language.sql`, which run earlier (V9); earlier numbers in this goal are history. Plan change D-4310: this task's migration moves to `migrations/0213_ai_call_budget.sql` for T653's `0212_licensing_btr_typed_unchecked.sql`, which runs earlier (V9); earlier numbers in this goal are history. Plan change D-4727: this task's migration moves to `migrations/0216_ai_call_budget.sql` for T729's `0215_update_email_choice.sql`, which runs earlier (V9); earlier numbers in this goal are history. Plan change D-5314: this task's migration moves to `migrations/0217_ai_call_budget.sql` for T812's `0216_businesses_step_up_session.sql`, which runs earlier (V9); earlier numbers in this goal are history. Plan change D-5336: this task's migration moves to `migrations/0219_ai_call_budget.sql` for T804's `0216_portals_data_request_link.sql`, which runs earlier (V9); earlier numbers in this goal are history. Plan change D-5676 (token efficiency and phase-2 pre-flight, checked on `main` 025855b2): The line budget is a guide, not a stop (as D-5331); do not stop for size or cut a case, and say the final count in the hand-in. Plan change D-5848: the migration is `0219_ai_call_budget.sql` (T862 takes `0218`); earlier numbers are history. `weekly_counts` in `app/ai/src/switch.rs:243-248` excludes `budget` outcomes from both calls and ended counts, as `daily_cap` is excluded. Add a concurrent mixed admission test: step-budget-exhausted requests racing eligible ones with one global slot left cannot consume the slot; exactly one eligible logical call reaches the transport, refusal rows do not enter daily or weekly call/ended totals, and an admitted retry uses its original reservation. Rollback must also undo any earlier step reservations if a later one refuses. Maintain the existing once-per-day alert and no-held-connection tests. T299 passes the new context via the public pool entry; T394 uses the same reservation primitive, including its separate dispatcher integration under the prior handoff. This changes AI-029 reservation transaction wording only by the decision below; no provider await in a transaction.
```

### Replacement `acceptance`

```text
TD-AI-18, TD-AI-20 and TD-AI-21 pass; with a pool of one connection, a slow stand-in AI call in flight through the entry the app calls leaves that connection free for another task; 20 concurrent readings of one message make exactly 2 transport calls; no app code calls a `read_intake` form that takes a held connection. Existing IntakeCall literals compile without edits; mixed concurrent refusal cannot consume the last global slot; weekly totals exclude budget and daily_cap; all reservations are atomic and no connection survives the provider await.
```

### Manifest changes

The JSON below is a delta, not a new manifest schema: append `add` values uniquely to the existing array; remove only `remove` values. Keep all other entries, requirements, tests and decisions. For entries without an existing `acceptance`, add the proposed field only if the current validator permits it; otherwise put its exact text into the goal and generated hand-in section.

```json
{
  "owns": {
    "add": [],
    "remove": []
  },
  "reads": {
    "add": [],
    "remove": []
  },
  "deps": {
    "add": [],
    "remove": []
  }
}
```

No ownership changes. Migration stays `0219_ai_call_budget.sql`. This is the atomic-transaction option from 3b-3; it requires a logged narrow AI-029 wording amendment (below), rather than silently ignoring its per-check transaction wording. Update the T299/T394 generated prompts to describe the new typed context without editing their owned files here.

### Test design

`docs/test-design.md:610` (original line in excerpt).

**Old:**

```text
610: | TD-AI-21 | F | In | AI-029 | In `tests/integration/tests/ai_budget.rs`, against PostgreSQL with a pool of exactly one connection, the AI-008 switch on and a test transport that holds each call until the test releases it (no sleep, section 1 rule 6): while a reading is in flight through the entry the app's intake calls, the test takes the pool's one connection within 1 second and commits a write; the same holds during an AI-023 retry (a 429 with `retry-after: 0`, then a success); after release the call is recorded once in `ai.call_stat`; 20 concurrent readings of one inbound message on separate tasks → exactly 2 reach the transport and the other 18 are recorded with outcome `budget` (AI-028); a reading whose conversation step the test moves on while the call is held is recorded and not applied, and the step's current state is unchanged (D-2038) |
```

**New:**

```text
| TD-AI-21 | F | In | AI-029 | In `tests/integration/tests/ai_budget.rs`, against PostgreSQL with a pool of exactly one connection, the AI-008 switch on and a test transport that holds each call until the test releases it (no sleep, section 1 rule 6): while a reading is in flight through the entry the app's intake calls, the test takes the pool's one connection within 1 second and commits a write; the same holds during an AI-023 retry (a 429 with `retry-after: 0`, then a success); after release the call is recorded once in `ai.call_stat`; 20 concurrent readings of one inbound message on separate tasks → exactly 2 reach the transport and the other 18 are recorded with outcome `budget` (AI-028); a reading whose conversation step the test moves on while the call is held is recorded and not applied, and the step's current state is unchanged (D-2038) |; additionally, with one global slot left, race step-budget-refused requests against eligible requests: exactly one eligible call reaches transport, rejected requests consume no global slot or partial step reservation, and budget/daily_cap rows are excluded from weekly call and ended counts
```

Reason: Adds coverage without replacing any existing assertion; TD-AI-18 and TD-AI-20 unchanged.




## T298 — adapters: the AI endpoint settings and the real Claude client, the stand-in by default

**Verdicts:** 3b-4.

**Evidence:** `app/ai/src/models.rs:274-283`; `docs/spec/ai.md:27` AI-023; verified D-810 `docs/decisions.md:816` (not in archive).

### Replacement `goal`

```text
IC-23's first half (D-2065): the app has no AI endpoint setting (the only AI keys are `ASKHAVA_AI_MODEL`, `ASKHAVA_AI_TIMEOUT_SECONDS` and `ASKHAVA_AI_DAILY_CAP`, `app/core/src/settings.rs:156` to `:158`) and its only AI client, `StandInAi` (`app/adapters/src/ai.rs:35`), takes only an `http` base URL on a loopback IP literal and sends the fixed non-key `stand-in`, so the Railway test project cannot reach the real Claude API (AI-009, `BUILD_BRIEF.md:194`). In `app/adapters/src/ai.rs` add, beside `StandInAi`, the AI endpoint settings and one client for the real Claude Messages API, behind the existing `AiAdapter` trait so `ai::api::AdapterTransport` wraps either client unchanged. Settings, read from the same pairs `Settings::from_pairs` took, each key added to `KEYS` in `app/core/src/settings.rs` (shared): `ASKHAVA_ADAPTER_MODE_AI` (`stand_in` or `api`; default `stand_in` in dev, ci and test, `api` in live), `ASKHAVA_AI_BASE_URL` (default `http://127.0.0.1:4010/anthropic`, the stand-in server, in `stand_in` mode and `https://api.anthropic.com` in `api` mode) and `ASKHAVA_AI_API_KEY`. Rules: `stand_in` is refused in live (SEC-032) and takes only an `http` base URL on a loopback IP literal, as `StandInAi::new` does, so T297's fault proxy on a loopback port can stand in front of the stand-in; `api` is refused in dev and ci (AI-009: no real AI key before the Phase 2 Railway test project) and takes only `https://api.anthropic.com` (that scheme and host, port 443, no userinfo, path or query), so no setting can send scrubbed text to another host (AI-001); `api` needs the key and `stand_in` refuses one, except that in live a missing key never stops start-up: the settings then give an AI-off endpoint that names `ASKHAVA_AI_API_KEY` (no client, never a value), so an AI-off launch at Gate C starts and T299 runs every step on the AI-off path with a high alert (overseer's decision, D-2065); so a real key is never configured where only the stand-in may be reached (the stand-in also rejects any `sk-ant-` key, `stand_ins/server.py:92`). The key is a `SecretString` read from the app host's secret variables (SEC-001, `BUILD_BRIEF.md:64`): it is sent only as the `x-api-key` header to that one host through `VendorHttp` (no redirects, no proxy from the environment) and never appears in `Debug`, `Display`, an error, a log line, a span or Sentry; every error names the key, never the value. The real client posts only to `/v1/messages`, with the same `anthropic-version` header and idempotency key as `StandInAi` (AI-001, SEC-018). Keep `StandInAi::new` and the tests that call it (`app/adapters/src/tests/ssrf_guard.rs`, not this task's) working unchanged. No test reaches the real API or any non-loopback host (AI-009): the `api` mode is tested through its settings rules, the request it would send checked without leaving the machine (with a fake key sentinel that matches no secret-scanner rule), and the key's redaction. Nothing in the app calls the new client yet: T299 builds it at start-up and calls it from intake (IC-23's second half, D-2065). Update `app/adapters/CLAUDE.md` and the `ai` row of the adapter table in `docs/architecture.md` (`grep -n '(Claude API)' docs/architecture.md`; both shared). The schema it sends is T376's (D-2760). Plan change D-5676 (token efficiency and phase-2 pre-flight, checked on `main` 025855b2): the AI settings-key cite is refreshed (`KEYS`, `app/core/src/settings.rs:156` to `:158`). Retain the logged D-810 behavior in the real client: a 429 that reaches the existing models error mapping with retry_after None takes the AI-off path and raises the existing owner alert. Do not edit models.rs here or claim actual retry-after support. Record the real-provider alert-noise tradeoff in the decision; no delay outside AI-023 budget. A later queued models.rs owner may implement header mapping only under a separate decision, not as a prerequisite for Gate C.
```

### Replacement `acceptance`

```text
TD-AI-23 passes; with no AI keys set, the dev settings give the stand-in client at `http://127.0.0.1:4010/anthropic`; `ASKHAVA_ADAPTER_MODE_AI=api` stops start-up in dev and ci naming the key; live with no key set gives the AI-off endpoint naming `ASKHAVA_AI_API_KEY`, not an error; no test opens a connection to a non-loopback host; every existing `adapters` and `ai` test passes unchanged. Preserve D-810 behavior; real 429 limitation explicitly recorded and no unsupported retry claim.
```

### Manifest changes

The JSON below is a delta, not a new manifest schema: append `add` values uniquely to the existing array; remove only `remove` values. Keep all other entries, requirements, tests and decisions. For entries without an existing `acceptance`, add the proposed field only if the current validator permits it; otherwise put its exact text into the goal and generated hand-in section.

```json
{
  "owns": {
    "add": [],
    "remove": []
  },
  "reads": {
    "add": [],
    "remove": []
  },
  "deps": {
    "add": [],
    "remove": []
  }
}
```

### Test design

No locked row change. Retain every existing case.




## T379 — messaging (harness): an email the provider suppressed or failed counts as not reached, and the notice log says so

**Verdicts:** 3a-5, 3a-6, 3a-10.

**Evidence:** `app/messaging/src/email_timing.rs:71-105,249-280`; `app/messaging/src/business_notices.rs:579-590`; `tests/integration/tests/text_retries.rs:55-77`; `docs/spec/businesses.md:49` BIZ-045; `docs/spec/job-cards.md:23` CARD-019.

### Replacement `goal`

```text
Nov 11 vendor-double review batch 4, F6 (counting half; D-2765; CARD-019, BIZ-045, TXT-014). Resend skips a send to an address on its team-wide suppression list and reports `email.suppressed`, and reports `email.failed` for a send it could not make (`docs/brains/email-deliverability.md:58`, `:62`); `receive_email_event` stores both (`app/messaging/src/email.rs`), but `card_notice_accepted` (`app/messaging/src/business_notices.rs:579`) leaves out only `email.bounced`, so after one hard bounce every later card notice to that address counts as reached toward CARD-019's N, and `ProviderStatus::from_event` (`app/messaging/src/email_timing.rs:85`) maps neither, so BIZ-045's log shows nothing for them. After T133 (the fake Resend's suppression list) and T313 (owner of `business_notices.rs`): (1) `card_notice_accepted` treats an email with an `email.suppressed` or `email.failed` event like a bounced one (not reached); (2) extend the notice_log SQL latest-event allowlist at `email_timing.rs:255-259` with email.suppressed and email.failed, regenerate shared .sqlx metadata, and `ProviderStatus` gains `Suppressed` and `Failed`, each with a BIZ-045 log line in English and Spanish that passes `scripts/check_copy.py` (for example "Not sent: this address is blocked after an earlier bounce or spam report."), exhaustively matched; (3) the test `FakeMail` (`tests/integration/tests/text_retries.rs:55`) keeps a suppression list and returns a successful stable provider ID for a send to a listed address, then the test separately injects the matching persisted provider event through the normal event-store path (also for failed and delivered). Tests in `tests/integration/tests/email_suppressed.rs`: a business whose only card email was suppressed, or failed, is not counted as reached (fails before the fix); a delivered one still is; the notice log shows the new lines. Update the `messaging` `CLAUDE.md` and `docs/decisions.md`. Plan change D-3878: T520 first moves `business_notices.rs`'s link colour to the `evergreen` token; this task keeps it. Plan change D-5568 (Grok pre-flight finding, verified on main bd3e4e9c): this task writes words a homeowner or business reads, so it is flagged `copy` and its prompt carries the copy steps (`scripts/check_copy.py`, `scripts/check_ai_smell.py`, the cold read and the copy panel) instead of "Customer copy: none changed ... stop and report". Preserve ProviderStatus::line() for existing English callers and add a language-aware entry for the two new statuses using the existing language representation; test the latest persisted status and both rendered languages. The actual portal notice-log renderer must call that entry with the business language: locate its real caller and queued owner before queueing. If outside T379, amend that owner task (ordered after T379) or transfer the renderer file to T379 after its preceding owner, with a served-page test. Do not claim the bilingual acceptance is closed by enum tests alone. No new copy bypass; preserve copy panel.
```

### Replacement `acceptance`

```text
TD-TXT-37 passes with stored events and actual notice-log rendering in English and Spanish; suppressed/failed are not reached, delivered is reached; SQLx offline checks and all existing tests pass; copy checks pass. Do not close TD-TXT-37 until the actual renderer is wired.
```

### Manifest changes

The JSON below is a delta, not a new manifest schema: append `add` values uniquely to the existing array; remove only `remove` values. Keep all other entries, requirements, tests and decisions. For entries without an existing `acceptance`, add the proposed field only if the current validator permits it; otherwise put its exact text into the goal and generated hand-in section.

```json
{
  "owns": {
    "add": [],
    "remove": []
  },
  "reads": {
    "add": [],
    "remove": []
  },
  "deps": {
    "add": [],
    "remove": []
  }
}
```

Shared `.sqlx` regeneration; renderer owner/path is not present in this archive (the included `confirm_pay.rs:411` reads log to mark opening, not to render). The zero-path delta is the known-safe core; a renderer delta must be filled and graph-checked by the overseer before queueing. See choice 3.

### Test design

No change to `docs/test-design.md:430` TD-TXT-37; it already demands both languages. Add the persistence and real rendering assertions within that requirement.




## T659 — server and messaging: the signed Resend webhook route marks bounced and complained business addresses returned

**Verdicts:** 3b-5.

**Evidence:** `app/messaging/src/email.rs:915-916,954-955`; `docs/spec/messaging.md:22` TXT-018; `docs/spec/security.md:55` SEC-051; `docs/test-design.md:882`.

### Replacement `goal`

```text
Plan change D-4327 (night spot review of outgoing email on `main` e64d06e02, re-verified on `main` d662af7e5; BIZ-040, BIZ-055, SEC-025, SEC-050, SEC-051; D-1381). `messaging::api::receive_email_event` (`app/messaging/src/email.rs:723`, D-1381) and `businesses::api::mark_returned` (`app/businesses/src/email_verification.rs:512`) have no caller outside tests, and no server route verifies `WebhookVendor::Resend` (`app/adapters/src/signatures.rs:59`); `app/businesses/NOTES.md:222` says the webhook's call to `mark_returned` is not wired, D-1381 left it to a later task and none had it (T133 builds the signed adapters and the fake Resend's events, T490 the mailers, T379 only counts suppressed card emails). So a hard-bouncing or complaining business address keeps getting notices, BIZ-040's banner never shows, and once emails go out `check_webhook_silence` (`email.rs:811`, scheduled hourly) raises the High `messaging.email_webhooks_silent` alert 24 hours later and keeps it open for good. (1) `app/server/src/resend_webhook.rs` (new): `POST /webhooks/resend`, mounted in the shared `app/server/src/lib.rs` beside the other webhook routes and outside the CSRF layer as they are, reads the raw body and the `svix-id`, `svix-timestamp` and `svix-signature` headers and passes `adapters::api::verify_webhook(WebhookVendor::Resend, …)` with the signing secret from a new setting `ASKHAVA_RESEND_WEBHOOK_SECRET` (the shared `app/core/src/settings.rs`; a secret, never logged or printed); unset, the route is not mounted in Dev and CI (they answer 404) and the server refuses to start in Test and Live, as `stripe_webhook`'s secret does (`app/server/src/release_sink.rs:298` to `:302`); a refused delivery (bad signature, missing header, timestamp outside 5 minutes) answers `adapters::api::refuse(WebhookVendor::Resend, e)`'s 403 and stores nothing, and the router is mounted through the `refused(...)` wrapper (`app/server/src/lib.rs:404` to `:415`), so each 403 records one capped `webhook_signature_failed` security event (`app/server/src/webhook_refusals.rs`; SEC-016, SEC-048); every other failure answers 200 or 500, never 403, as that layer requires. A verified delivery is handled in one transaction the route opens and commits once: it passes `&mut *tx` to `receive_email_event` (which dedupes by the Svix ID, stores the event and raises SEC-050's complaint alert, D-1381), then to the returns lookup, `mark_returned` and `acknowledge_cleared` below, so each of their own `begin()` calls is a savepoint and a failure anywhere rolls the event back too; `Recorded`, `Repeat` and `Ignored` answer 200 after the commit and any database error answers 500 with nothing kept, so Resend's retry is a first delivery again, never a `Repeat` that skips the address (SEC-051: acknowledged only once stored). (2) For a `Recorded` event of type `email.bounced`, `email.complained` or `email.suppressed`, after the store, a new `messaging::api` lookup in `app/messaging/src/email_returns.rs` (new; not `models.rs`, which T470, T573 and T580 own) maps the event's email ID through `messaging.email_outbox.provider_id` to the row's `recipient_kind` and `recipient_id` (`migrations/0089_messaging_retries_email.sql:39` to `:50`); for a `business` row it finds the mailbox's businesses through `businesses::api::businesses_of_mailbox` and calls `businesses::api::mark_returned` once for each, at the event's signed `created_at` (read from the verified body, never the wall clock, so SEC-051's event-time order holds and BIZ-055's span is measured between events), with `Returned::Complaint` for `email.complained`, `Returned::Hard` for `email.suppressed` and for an `email.bounced` whose `data.bounce.type` is `"Permanent"`, and `Returned::Soft` for any other bounce (`"Transient"`, `"Undetermined"` or no type); the body fields are read in `email_returns.rs` (BIZ-055's 14-day rule is `mark_returned`'s, which queues `RETURNED_KIND`); a homeowner row, an unknown email ID or another event type makes no business-mailbox-return change and queues no returned task; preserve TXT-018 update-address bounce fallback to text inside receive_email_event, and a `Repeat` calls nothing. (3) A stored event of any type clears the webhook-silence alert: `messaging.email_webhooks_silent` joins `CLEARING_KINDS` (`app/ops/src/alerts.rs:396`) and the route calls `ops::api::acknowledge_cleared` (`:418`, OPS-023) with it after the store, so the alert resolves once webhooks arrive and a later silence opens a new one (`raise_kind_once`, D-3144). (4) Before Gate C only the stand-in and test secrets are used: no live Resend key, endpoint or webhook is created or registered (`BUILD_BRIEF.md:68`); registering the endpoint and its secret at Resend is a Gate C step, listed for Casey in `docs/for-casey-review.md`. (5) Tests in `app/server/tests/resend_webhook.rs` (TD-SEC-76), fake data only, through the assembled router with a test signing secret and bodies built by hand in Resend's event shape (the fake Resend's events, `stand_ins/resend.py:84` to `:85`, carry no bounce type, and `stand_ins/` is protected): a bad signature, a missing header and a timestamp 6 minutes old each answer 403 with nothing stored and exactly one `webhook_signature_failed` security event; a hard bounce, a complaint and `email.suppressed` for a sent business notice each mark its mailbox returned with one `RETURNED_KIND` task (the complaint also one complaint alert), and a replay of the same Svix ID changes nothing; two transient bounces whose signed `created_at` are 13 days apart do not mark it and 14 days apart do (BIZ-055); a bounce with an undetermined or missing bounce type counts as soft; a failure injected after the event store (for example a `mark_returned` error from a planted constraint) answers 500 and keeps no event row, and the same delivery sent again then marks the address returned; a bounce of an ordinary homeowner notice (not her current update address), an unknown email ID and a `contact.created` event answer 200, mark no business mailbox returned and queue no returned task; preserve TXT-018 update-address bounce behavior and its existing tests; a database failure answers 500; an open `messaging.email_webhooks_silent` alert is resolved by the next stored event; with no secret set the route is not served in CI, and building the composition for Test or Live with no secret is refused. Ordered after T133, T490 and T469 (the adapters, the fake's events and the provider wiring) and after T652, the other queued owner of `app/ops/src/alerts.rs`. Update `docs/architecture.md`, `app/server/NOTES.md`, `app/messaging/NOTES.md`, `app/businesses/NOTES.md` (its "Not wired yet" line), `app/ops/NOTES.md` and `docs/decisions.md`. Resend test webhook registration is only on the isolated test team and test endpoint when Casey authorizes it; live registration remains Gate C. Do not weaken the event-store, replay, transaction or complaint assertions.
```

### Replacement `acceptance`

```text
TD-SEC-76 passes against PostgreSQL through the assembled router; `tests/integration/tests/email_verification.rs`, the email and webhook tests and `tests/security` pass unchanged; no live Resend key or endpoint is used. Homeowner fixture is an ordinary notice; no business mailbox returned and no returned task; TXT-018 update-address bounce fallback remains unchanged.
```

### Manifest changes

The JSON below is a delta, not a new manifest schema: append `add` values uniquely to the existing array; remove only `remove` values. Keep all other entries, requirements, tests and decisions. For entries without an existing `acceptance`, add the proposed field only if the current validator permits it; otherwise put its exact text into the goal and generated hand-in section.

```json
{
  "owns": {
    "add": [],
    "remove": []
  },
  "reads": {
    "add": [],
    "remove": []
  },
  "deps": {
    "add": [],
    "remove": []
  }
}
```

### Test design

`docs/test-design.md:882` (original line in excerpt).

**Old:**

```text
882: | TD-SEC-76 | F | S, In | SEC-051, SEC-050, SEC-025, BIZ-040, BIZ-055, OPS-023 | In `app/server/tests/resend_webhook.rs` (T659, D-4327), fake data only, through the assembled router with a test Svix secret and bodies built by hand in Resend's shape: a bad signature, a missing header and a 6-minute-old timestamp each answer 403, store nothing and record exactly one `webhook_signature_failed` security event; a hard bounce, a complaint and `email.suppressed` for a sent business notice each mark its mailbox returned with one returned task (the complaint also one complaint alert), a replayed Svix ID changes nothing, transient bounces whose signed `created_at` are 13 days apart do not mark it while 14 days apart do, and an undetermined or missing bounce type counts as soft; a failure injected after the event store answers 500 and keeps no event, and the resent delivery then marks the address; a homeowner email's bounce, an unknown email ID and a non-email event answer 200 and change nothing; a database failure answers 500; the next stored event resolves an open webhook-silence alert; with no secret set the route is not served in CI and a Test or Live composition is refused |
```

**New:**

```text
| TD-SEC-76 | F | S, In | SEC-051, SEC-050, SEC-025, BIZ-040, BIZ-055, OPS-023 | In `app/server/tests/resend_webhook.rs` (T659, D-4327), fake data only, through the assembled router with a test Svix secret and bodies built by hand in Resend's shape: a bad signature, a missing header and a 6-minute-old timestamp each answer 403, store nothing and record exactly one `webhook_signature_failed` security event; a hard bounce, a complaint and `email.suppressed` for a sent business notice each mark its mailbox returned with one returned task (the complaint also one complaint alert), a replayed Svix ID changes nothing, transient bounces whose signed `created_at` are 13 days apart do not mark it while 14 days apart do, and an undetermined or missing bounce type counts as soft; a failure injected after the event store answers 500 and keeps no event, and the resent delivery then marks the address; an ordinary homeowner notice's bounce (not her current update address), an unknown email ID and a non-email event answer 200, mark no business mailbox returned and queue no returned task; TXT-018 update-address bounce fallback remains unchanged; a database failure answers 500; the next stored event resolves an open webhook-silence alert; with no secret set the route is not served in CI and a Test or Live composition is refused |
```

Reason: Clarifies scope rather than disabling an existing homeowner side effect; T511 owns the row; overseer logs and applies the row change before T659’s prompt is regenerated.




## T704 — intake: an emergency job's quote texts, the INT-029 questions and intake's direct replies are queued with their QH-004 fact (D-4419)

**Verdicts:** 3b-2.

**Evidence:** `app/intake/src/order_dispatch.rs:1536-1551`; `app/intake/src/handlers.rs:379-395`; verified verdict cites `replies.rs:518`, `zip_step.rs:253`, `photos.rs:236`, `cards_out.rs:582` (not supplied); `docs/spec/quiet-hours.md:5,8`; `docs/spec/intake.md:33` INT-029.

### Replacement `goal`

```text
Plan change D-4419 (T402 measured 47% of the context window, over the 45% build limit, from its nine owned files): D-3106 is split by module. This task does D-3106 for every eligible producer in its owned intake files, including the actual order_dispatch path and creates `tests/integration/tests/quiet_exempt_producers.rs` (TD-QH-07) with the cases for the texts those producers queue: an emergency job's first-quote text and an INT-029 question go at once at 23:10 and the same texts on a regular job wait until 08:00; an `intake` reply queued within 15 minutes of her own inbound text goes at once and one queued later waits until 08:00. T402 follows this task and does the same for `app/picks/src/service.rs` and `app/server/src/inbound_text.rs`, with its cases (the pick-step texts, the server's replies, the 911 text and a HELP reply, now TD-QH-09) in its own `tests/integration/tests/quiet_exempt_pick_replies.rs`; a text those two files queue is not changed here. The quote-count update text named below is queued by `app/intake/src/quote_updates.rs` (T583's), and its QH-004 exemption is T708's, which this task follows; it is not changed here. D-3106 as written for T402: Plan change D-3106 (QH-004, QH-001; D-2628, D-3055): after T356 the outbox sender holds a row outside 8 a.m. to 8 p.m. unless its `quiet_exempt_until` lets it go; plain `messaging::api::enqueue` sets that itself for the any-hour kinds (the 911 text, keyword replies, pick codes, the after-hours notice), but an emergency job's texts and a direct reply are exempt only through `enqueue_exempt`, which no producer calls, so at night they wait for 8 a.m. Each producer here that queues such a text passes its fact through `enqueue_exempt`: an emergency job's quote texts (the first-quote text and quote-count updates), the questions INT-029 asks after the cards, and the homeowner's pick, confirm and connect texts (including the INT-033 texts; the who's-calling and missed-call texts are T403's) carry the job's emergency choice; a reply to her own inbound text carries that text's received time, so it goes at any hour only within 15 minutes of it. The facts are read where the producer already has them (the job row, the inbound delivery), never guessed from words. A producer whose texts are never exempt by a fact keeps plain enqueue and is not changed. Own order_dispatch.rs and audit every queue_body call in replies.rs, zip_step.rs, photos.rs and cards_out.rs as well as the original six files. In each eligible producer pass the persisted emergency choice or actual inbound received time explicitly through a consent-safe helper to enqueue_exempt; preserve T857 consent guards and outbox dedupe. Keep existing queue_body signature for unaffected callers, or use a separate exemption-aware helper. Never infer eligibility from template name, message words, wall clock or recipient alone. The INT-029 bank question must use the actual dispatch path at order_dispatch.rs:1536-1551, not a test-only bypass. All other direct-reply paths receive the same bounded treatment, only if QH-004 permits it. Tests in `tests/integration/tests/quiet_exempt_producers.rs` (TD-QH-07). Every TD-QH, TD-TXT and TD-INT case passes unchanged. Update the `intake` CLAUDE.md and docs/decisions.md only; T402 owns the picks/server work. Plan change D-3999: it follows T564, which also edits `app/picks/src/service.rs` and `app/intake/src/job_texts.rs`. Plan change D-4338: ordered after T662, which first makes both cancel paths in `app/intake/src/closing.rs` and `conversation.rs` queue the masked line's close (CALL-009, CALL-010); keep those calls. Update the `intake` `CLAUDE.md` (not the `picks` or `server` ones, T402's) and `docs/decisions.md`. Plan change D-4602: `log_demand` calls `order_dispatch::demand_month_count` (T723); the private `month_count` is removed, unless T126 already merged them; then check `log_demand` uses the one body. Run after the queued owners identified by the verified review (T299, T823, T825, T857, T859, T860, T865), preserving T857 consent checks and any earlier T394 dispatcher integration. If the expanded ownership breaches the context cap, split by producer into a predecessor or successor with explicit dependencies and keep TD-QH-07 assigned to the final real-path integration task; do not queue a task with unresolved ownership or claim closure before all producers are wired.
```

### Replacement `acceptance`

```text
TD-QH-07 passes through actual intake dispatch and the worker with stand-in Twilio at 23:10: emergency first-quote and INT-029 question immediate; regular equivalents held until 08:00; each eligible direct reply within 15 minutes immediate, later reply held. Consent guard remains effective, no out-of-scope exemption, every TD-QH/TD-TXT/TD-INT case passes unchanged; T402 and T708 scopes preserved.
```

### Manifest changes

The JSON below is a delta, not a new manifest schema: append `add` values uniquely to the existing array; remove only `remove` values. Keep all other entries, requirements, tests and decisions. For entries without an existing `acceptance`, add the proposed field only if the current validator permits it; otherwise put its exact text into the goal and generated hand-in section.

```json
{
  "owns": {
    "add": [
      "app/intake/src/order_dispatch.rs",
      "app/intake/src/replies.rs",
      "app/intake/src/zip_step.rs",
      "app/intake/src/photos.rs",
      "app/intake/src/cards_out.rs"
    ],
    "remove": []
  },
  "reads": {
    "add": [],
    "remove": []
  },
  "deps": {
    "add": [
      "T823",
      "T825",
      "T857",
      "T859",
      "T860",
      "T865"
    ],
    "remove": []
  }
}
```

**Conditional delta, not validated:** T299 already owns order_dispatch and is a dependency; the other prior queued owners are the set identified in the verified verdict, not a proven one-to-one mapping. Check full owner map and cycles first; keep only actual eligible producer files if audit proves others do not qualify. Proposed maximum file count 12; previous context estimate 38% was for seven files, so remeasure and split if >45% build limit. Do not simply raise the cap. If T394 dispatcher integration is approved and owns order_dispatch, add its allocated task ID as predecessor too. T402 must remain after final T704 producer integration. See choice 4.

### Test design

No change to `docs/test-design.md:386` TD-QH-07 or `:388` TD-QH-09; the original worker-path assertions remain. Expand fixtures for each actual eligible producer and consent negative cases.




## Account checklist — six paste-ready items

**Placement:** Part 1 before `## Part 2: business accounts` in `docs/account-checklist.md`. Preserve existing Railway variables and secret inventory, including AI (`:75`) and SendGrid (`:48-58`); this adds a separate build-network-secret path, not a replacement for deployment settings. T122 DONE / T141 QUEUED; narrow plan edit before T127 as described above. No actual account or secret is created by this draft. Vendor links are copied from the bundled VENDOR-1 report.

```markdown
### Phase 2 build sandbox (before T127 sends the account checklist)

Casey alone provisions these accounts and keys. Add each build credential as a network secret locked to its exact vendor API host; never put values in this file, a prompt, ordinary environment file, log, commit or GitHub secret. Preserve the separate sealed Railway test-project variables. CI remains keyless for these vendor suites. Use fake data only and no production key or endpoint before Gate C. Missing credentials or endpoint means NOT RUN, not pass. Actual texts, calls and Verify deliveries to a real phone require Casey's explicit T130 hands-on stop.

- [ ] **Twilio simulated Messaging/Calls:** check if legacy test Account SID and token are available; new console issuance is not assumed. If available install as build network secrets locked to `api.twilio.com`. Simulated create/magic-number tests only; no callbacks, redaction, actual calls or Verify proof. If unavailable record NOT RUN. [Test credentials](https://www.twilio.com/docs/iam/test-credentials).
- [ ] **Twilio isolated service test project:** provision separate restricted sub-account credentials and Verify service SID; test-project variables as above, and only appropriate non-delivery build credentials host-locked to `api.twilio.com` or `verify.twilio.com` as applicable. Do not expose unrestricted delivery credentials to workers; any delivery/spending is Casey's hands-on T130 stop, including Verify SMS/voice/email, callbacks and redaction of actual messages. Set up SendGrid Verify email integration as above. A real test-service project is not magic-number simulation. [Unsupported test resources](https://www.twilio.com/docs/api/errors/20008), [testing guidance](https://help.twilio.com/articles/223183808-Why-aren-t-my-test-credentials-working-), [public webhook endpoint](https://static0.twilio.com/docs/usage/webhooks/webhooks-faq), [Verify pricing](https://www.twilio.com/en-us/verify/pricing).
- [ ] **Stripe sandbox:** restricted test secret as build network secret locked to `api.stripe.com`, with the existing test-project variables and test webhook signing secret. Only test payment methods. Use approved test webhook forwarding or public test endpoint, with secret in the approved test-project boundary; no real charge or live key. [Testing](https://docs.stripe.com/testing), [webhooks](https://docs.stripe.com/webhooks).
- [ ] **Resend isolated test team:** test API key as build network secret locked to `api.resend.com`; retain sealed test-project key and webhook secret. Use vendor test addresses only; suppression operations affect the whole team, so no production team key. Record unsupported nondeterministic events NOT RUN; actual test webhooks need a public authorized endpoint. [Test emails](https://resend.com/docs/dashboard/emails/send-test-emails), [remove suppression](https://resend.com/docs/api-reference/suppressions/remove-suppression), [CLI](https://resend.com/docs/cli).
- [ ] **Lob test account (new):** Secret Test API Key as build network secret locked to `api.lob.com`; sealed Railway test-project API key and normal subscribed test webhook signing secret. Configure a separate test webhook subscription to the approved public HTTPS test route after it exists; distinguish its signing secret from the debugger key. Test digital proofs and supported events, no physical mail or production key; no real USPS tracking proof. [API keys](https://help.lob.com/account-management/api-keys), [test environments](https://docs.lob.com/#section/Test-and-Live-Environments), [webhooks](https://help.lob.com/print-and-mail/getting-data-and-results/using-webhooks).
- [ ] **B2 isolated test bucket:** separate prefix-scoped application keys for audit/export writers and photos. Lock build network secrets to the actual configured B2 API host(s), recorded in the secret inventory after endpoint verification; no broad wildcard or unrestricted key. Seal test-project variables as needed. Audit/export keys must not read/list/delete permanent objects and must satisfy versioning/Object Lock retention; verify hide behavior separately. Photos key may read/list/delete only its own prefix. Fake data only; no production bucket. [Application keys](https://www.backblaze.com/docs/cloud-storage-application-keys), [capabilities](https://www.backblaze.com/docs/cloud-storage-application-key-capabilities), [create keys](https://www.backblaze.com/docs/cloud-storage-create-and-manage-app-keys).
```

For all webhook cases deferred until public deployment: have Casey configure the Railway test endpoint when available under T135; perform only authorized test runs, attach results to the appropriate original task's review record and the T130 owner-stop record where real phone delivery is involved. A missing endpoint does not make an earlier task's vendor check green. No change to `BUILD_BRIEF.md` credential policy is needed (`:62-65,184,516-517`). Only the AI Actions secret already authorized by the brief is allowed; no new vendor GitHub secrets.

## Decision entry draft

- **D-[allocate through runner]** Question (plan change, November 26, 2026; decider: **Fable / overseer**; PF-P2-3a / PF-P2-3b verified on `ed430909b`, VENDOR-1 vendor report and earlier PF-P2c handoff; 9 confirmed, 7 partly): which changes are needed before the first Phase 2 tasks can be queued without weakening tests or bypassing owner-held credentials? Choice: before T127, revise `docs/account-checklist.md` Part 1 (T122 DONE, T141 QUEUED) by explicit plan authorization with host-locked build network secrets for Twilio, Stripe, Resend, Lob and B2; T129/T131/T132/T133/T134 keep keyless CI and report unsupported or unavailable vendor cases NOT RUN, never pass, with T130 owner stop for real phone operations; no change to the brief's credential policy. T129's callback parameter is required only for clients composed with a public base URL; T131 separates simulated Calls creation from real Verify/AMD/callbacks, retains typed errors and T382's independent local-cap/provider-error cases. T132 forwards IntentText through Catching, asserts the Confirm press path and outgoing Rust form, and separates stripe-mock shape proof from actual lifecycle; T133 keeps stand-in DELETE and its DONE SSRF test, routes suppression removal through messaging/privacy via an opaque durable operation, and treats unsupported as real-client-only; T134 verifies isolated B2 prefix/capability boundaries. T379 extends the SQL allowlist and language-aware renderer, keeps successful fake send IDs and injects asynchronous events; T499 deepens on missing or unprovable history, then fails clearly, and refreshes citations. T236 leaves IntakeCall intact, supplies separate budget context at the pool boundary and excludes budget from weekly counts; reserve steps and global admission in one short atomic transaction, global slot last, no partial reservations or held connection during provider await, with global cap priority when already exhausted. T298 retains D-810 for real 429s and records owner-alert noise. T659 distinguishes ordinary homeowner bounce from TXT-018 update-address fallback; T704 owns or explicitly transfers real INT-029/direct-reply producers after their queued owners, preserves consent gates and all existing QH cases; no test-only workaround. Reason: source evidence and exact task goals above; no live operation, new feature or unrelated refactor. Source lines: `BUILD_BRIEF.md:62-65,184,516-517`; `docs/account-checklist.md:33-34,62-75`; `app/adapters/src/sms.rs:72-81`, `email.rs:90-101`, `tests/ssrf_guard.rs:427-439`; `app/portals/src/confirm_pay.rs:860-872,1790`; `app/messaging/src/email_timing.rs:95-105,255-280`, `business_notices.rs:579`, `email.rs:915-916,954-955`; `app/ai/src/service.rs:52-61`, `switch.rs:243-248`, `models.rs:274-283`; `app/intake/src/order_dispatch.rs:1536-1551`, `handlers.rs:379-395`; `.github/workflows/ci.yml:72,146-149`; `tests/integration/tests/text_retries.rs:55-77`. Requirements implemented/clarified: SEC-016/017/018, TXT-011/012/013/014/015/018/038, CALL-001/002/006/017/022, FEE-014/015/028/029/030/031/032/036/049, BIZ-001/002/003/016/045/056, SEC-039, PRIV-026/029, CARD-015/019, LIC-006/007/010/027, OPS-010/011, AI-021/023/028/029, QH-001/004, INT-029, SEC-051/050/025, BIZ-040/055, OPS-023. No requirement IDs removed. Only AI-029 transaction wording changes (below); D-810 exception retained, not a fresh AI-023 policy. Locked rows TD-BIZ-36 (`:658`), TD-AI-21 (`:610`), TD-SEC-76 (`:882`) change exactly as quoted above, with no lost assertions; TD-QH-07 (`:386`), TD-TXT-37 (`:430`), TD-FEE-38 (`:179`) remain intact. T511 or authorized plan session updates locked rows; no silent worker edits. Prior PF-P2c decisions for T382/T394/T685/T171/T392 remain separate and are not superseded by this entry. Conditional ownership and gates below require full-graph verification before approval; do not mark this proposed decision as decided until Fable / overseer approves.

### Narrow AI-029 specification amendment (only if atomic choice approved)

`docs/spec/ai.md:33` old clause: “the AI-021 admission and the AI-028 budgets, each on its own short transaction”. New clause: “the AI-021 admission and the AI-028 budgets, reserved together in one short atomic transaction, with the global admission consumed only after all applicable step reservations succeed; a refused reservation commits none of them”. Preserve the rest of AI-029, including the durable inbound commit and stale reading discard. No new policy on caps or retries. Record this exact locked-spec wording through the plan decision process. If not approved, use the step-first short-transaction option in choice 2 with equivalent refusal/no-leak proof.

## Open choices (five)

1. **Checklist ownership/order.** A: overseer authorizes a narrow plan-time edit now before T127; preserve T141's later work. B: use T141 as owner and add T141 → T127 ordering if the full graph permits. **Recommend A**, since neither full entry is supplied and delaying the checkpoint or introducing a cycle is unnecessary. T127 must not send old checklist.
2. **T236 reservation transaction.** A: atomic short transaction (draft above), update AI-029 clause by decision. B: retain separate short transactions, step reservations first/global last with durable compensation of partial step reservations, including crash/replay and cap refusal cases. **Recommend A**: fewer partial-reservation failure paths and straightforward race proof. This is a specification wording choice, not a change to cap values.
3. **T379 bilingual renderer.** A: add verified renderer file(s) to T379 after current queued owner(s). B: amend existing queued portal owner, ordered after T379, assigning actual served bilingual assertion to that task; defer TD-TXT-37 closure until both complete. **Recommend B** if an eligible queued owner exists; otherwise A with measured budget. Neither exact owner nor renderer path is in packet.
4. **T704 producer ownership/context.** A: expand T704 as drafted, with full graph proof and measured context within limit. B: split into narrow producer tasks after each current owner and before final TD-QH-07 integration (T402 waits on final); allocate actual IDs and paths from full graph. **Recommend B if >45% or if cycles require reordering; otherwise A.** The existing 38% estimate excludes five added files; do not assume it still fits. Do not weaken TD-QH-07 or transfer consent logic out of T857 without a decision.
5. **Actual unsupported/deferred vendor proof.** A: leave each unavailable case NOT RUN, attach owner/test-project follow-up to T130/T135 and do not treat it as real proof. B: Casey explicitly approves isolated test-project setup and hands-on execution now wherever possible; supported cases become PASS/FAIL only with evidence; the remaining still NOT RUN. **Recommend A for worker completion, B for the owner checkpoint where needed**; no silent substitution or authorization to use a production key. This is not permission to waive a launch requirement.

## Can't tell

- Full `kit/tasks.json`, `kit/plan.lock`, `kit/layers.json`, validator and ownership/status graph are not supplied. T127/T141/T511/T130 entries are absent; cannot give full replacement goals or safe exact graph deltas for them. The 11 full goal drafts above are for supplied affected entries. Before queueing, verify `acceptance` field schema for the older entries, actual file counts, shared paths, migration order and transitive dependencies.
- T379 actual portal renderer, language selection and current owner; no exact path or owner invented. T704 omitted producer files and full current owners/cycles; proposed conservative owner set is from verified verdict, not independently revalidated. T394 prior split needs its actual allocated dispatcher task ID. T704 must wait for that integration where it touches the same dispatcher.
- Exact T133 task/event worker registration, layer constraints and integration owner; the earlier PF-P2c path and architecture are proposed. Confirm no unowned private read. Exact T132 Cargo package names and real press fixture location must be checked; the DONE integration target is absent. T132 original logged Python test adjustment remains subject to decision.
- Other prior PF-P2c drafts (T382 separate cases, T394 core/dispatcher split, T685 settings UI owner, T171 isolated discarded worktree and T392 offline scanner/Casey Gate C run) remain in force as prior decisions/drafts; this packet does not establish whether they were applied to the current plan. Do not replace or close them based on this document. In particular T392 NOT OBSERVABLE is not proof of absence; owner Gate C policy remains to be confirmed under that prior decision.
- No real credentials, vendor responses, webhook capture, full repository or CI artifacts supplied. No vendor capability was re-queried and no tests were run. Exact Lob timestamp unit, Twilio legacy credential availability, B2 endpoint host(s), test-team permissions and real vendor case results remain unknown. All vendor URLs above come from the package.

## Coverage index

| Finding | Draft |
|---|---|
| 3a-1 | T129/T131/T132/T133/T134 + checklist |
| 3a-2 / 3a-3 | T132 |
| 3a-4 / 3a-9 | T133 |
| 3a-5 / 3a-6 / 3a-10 | T379 |
| 3a-7 / 3a-11 | T499 |
| 3a-8 | T129 |
| 3b-1 / 3b-3 | T236 |
| 3b-2 | T704 |
| 3b-4 | T298 |
| 3b-5 | T659 |
