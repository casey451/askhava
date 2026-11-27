# Fable's decisions on the verified reviews (Nov 26, 2026; main 0f6466a)

Inputs: `/tmp/claude-0/verify/T801_verdicts.md`, `/tmp/claude-0/verify/CLK_verdicts.md`, my scope decision `docs/handoff/2026-11-24-fable-scope-T801-T222.md`. Read-only; nothing run. Spec lines below were read by me; code lines come from the Opus verdicts unless marked "(read)". Items marked "plan session checks" are not verified.

## A. T801 questions

### T1. Pick code by email in web-and-email mode (F-e): parked, or in E+F/H?
**Decision: neither. It leaves the parked list and becomes its own Opus task (PKE) outside the T801 chain, a dependency of T533. My design decision (e) is narrowed.**
- Reason: PICK-029 (`docs/spec/quotes-pick.md:33`) and TXT-038 (`docs/spec/messaging.md:42`) send the pick code by email when texting is switched off or suspended; TXT-034 (`docs/spec/messaging.md:38`) says the same for web-and-email mode. TD-TXT-15 (`docs/test-design.md:408`), which T533 owns, asserts "the pick code by email to the address given in the chat" with zero texts. T533 cannot pass while `pick_flow` sends a Verify SMS in every mode. It is pick-step code, not consent or intake, so it does not belong in E+F or H.
- Decision (e) narrowed: a Verify SMS pick code to her typed number is allowed only when texting is "not live" for a reason other than switched off, suspended or web-and-email mode (in practice: before the Gate C date). In those three modes the code goes by email only; the opt-out list blocks either way.
- Plan PR must: add PKE (wire `app/picks/src/email_code.rs` into `app/portals/src/pick_flow.rs` behind the T725 predicate and the opted-out case; bring the conversation email D-1612 says is missing, `docs/decisions.md:1188`); order it against T689 (owner of `pick_flow.rs`); add T533 → PKE. Plan session checks whether the web-chat column of the Intake order table has a step that collects her email; if not, PKE's plan logs how the chat gets "an address given in their own conversation" (INT-012, `docs/spec/intake.md:16`) without adding an order step.

### T2. SMS eligibility per number or per request (R2-12)?
**Decision: per request. A request may be texted only if an `sms` consent record exists for that request's conversation.**
- Reason: CON-005 makes the inbound text consent "about their own request" and says a web-chat record "never permits a text" (`docs/spec/consent.md:9`); CON-007 says a web-chat request "gets no texts and needs none" (`docs/spec/consent.md:11`); CON-021 texts a web-chat requester "only after she texts in" (`docs/spec/consent.md:25`). So a returning texter's web request is not textable until she texts in during it. The table already carries `conversation_id` on every record (`migrations/0053*.sql`, `intake.consent_record`, read), so no migration.
- Plan PR must (task A): one predicate, "an `sms` record for this conversation", used by `may_text`, `card_allowed`, the enqueue guard and as the input to `rules::plan` (replacing `open.is_some()`, F-c). Every text-opened request must get its own `sms` record from its first inbound text, whether or not the consent text is re-sent; a test proves a returning number's second text-opened request is textable. Her first SMS into a web-opened request gets the consent text and appends the `sms` record (my decision b). Callers with no request in hand fail closed.

### T3. TD-CON-36: rewrite in the plan PR or in H?
**Decision: in the T801 plan PR, with the one `kit/plan.lock` rewrite and one decision entry.**
- Reason: the row (`docs/test-design.md:867`) still says the chat "queues the same INT-038 text the text path queues" and treats the consent text as sent; D-5279's CON-005 wording (`docs/spec/consent.md:9`) already forbids any text on a `web_chat` record. The row is stale against the spec, so correcting it is a spec alignment, not a weakened test (CLAUDE.md rules 6 and 8). Doing it in the plan PR keeps H from owning a locked doc and puts the wording in front of both Opus review rounds.
- Plan PR must: reword TD-CON-36 (reply and consent text shown on the page, no text queued, TXT-041 line shown on the page); grep every other web-chat row for an asserted queued text (at least TD-CON-10, TD-INT-30, TD-INT-43) and fix them in the same lock rewrite; H then changes only the T645/T649/T103 tests to match.

### T4. V1: replay the pre-code first description, or make her retype it?
**Decision: replay it from the page, never from the server. No retype in the normal case.**
- Reason: before the code the server keeps only the first message's ID and language, "never its words" (`app/intake/src/web_chat.rs:578-579`, read; D-4292). The page already carries the chat so far in the hidden `said` field (`app/site/src/chat.rs:230-233`, read). Nothing is in the vault to replay, and storing words before the code would break that rule.
- Plan PR must (task C): on the code POST, take her "you" lines from `said` as untrusted input: length cap, the same scrub as any chat message (vault name included), the 911 and INT-038 checks run again on them, then `keep_description` under `first_message_id` so a retried POST stores nothing twice (D-3189). If `said` is missing, empty or over the cap, ask for the description once on the page.

### T5. Mark TD-INT-30 (T103, DONE) as only partly proven?
**Decision: yes, in the plan PR's decision entry and in `docs/for-casey-review.md`.**
- Reason: TD-INT-30 (`docs/test-design.md:323`) claims the web-chat path "runs its steps in the table's order"; the verdict found its test checks only the static `web_chat_order` table and that no served step exists past the screen. CLAUDE.md rule 4 allows "tested" only when a run shows it.
- Plan PR must: state that the behavioural proof of TD-INT-30 moves to task D (integration) and T533 (end to end); add TD-INT-30 to D's `tests`. T103 stays DONE; nobody edits state.

## B. Clock questions

### C1. `Clock` default method over real-clock fields on route structs?
**Decision: approved, with one change: name it `real_now()`, not `wall_now()`.**
- Reason: a default method (`self.now()`) keeps `SystemClock` and `ManualClock` behaviour identical and avoids editing about 25 DONE test files with struct literals (rule 7). `app/core/src/clock.rs:89,104` (read) already use "wall-clock" for Arizona local time, so `wall_now` would mislead.
- Plan PR must (CLK-a): unit test that an advanced `OffsetClock` gives `real_now()` near system time while `now()` keeps the offset; a `///` on the trait method naming the rule (signatures, rate limits, security events, sessions).

### C2. Per-number daily code caps: app-time buckets in CI, or thread real time into `limits.rs`?
**Decision: accept app-time buckets for the code-send caps. This is the one named exception to "rate limits on real time". Per-IP admissions and security events still move to `real_now()`.**
- Reason: the cap, the cooldown and the code's life are one transaction on one instant, with the day lock keyed from it (D-3631, `docs/decisions.md:2066`). Splitting it changes `picks`/`portals` signatures used by DONE tests. Production is unchanged either way (`SystemClock`); in CI an advance can only loosen the cap.
- Plan PR must: log the exception by name in the decision; no journey may assert a per-number code cap across a clock advance.

### C3. Order against the 11 overlapping queued tasks?
**Decision: clock first.**
- Reason: T299 depends on T801 (kit/tasks.json, read) and T683 on T299, so "clock after" would put the journeys behind the whole T801 chain. CLK-a and CLK-b are small and depend on nothing queued.
- Plan PR must: add a dep on CLK-a or CLK-b to the root of each overlapping chain (T381, T811, T689, T402, T817 for CLK-a's files; T718 for `app/server/src/lib.rs`), since `scripts/validate_tasks.py` refuses two concurrent owners of one file (`docs/governance.md:36`). If one of the 11 is in flight when the plan PR is written, flip that single edge (clock after it), unless it sits downstream of T801.

### C4. Move the worker's session sweep and rate-limit prune to real time now?
**Decision: no. Leave both on the app clock.**
- Reason: pre-existing, outside the J1/J2 path, and J3 already passes with it (D-5282). With C5's session rule the sweep can only remove a row early in CI, which no journey depends on. Not worth a change to worker code that recovery drills cover.
- Plan PR must: name it in the decision as a known CI-only looseness, to revisit only if a security test flakes.

### C5. Business portal and job page on the app clock?
**Decision: business portal yes, for business-time reads only; job page no. And one rule replaces the verdict's "move issuer and reader together": session issue and session validity use `real_now()` on every switched route, Confirm included.**
- Reason: once Confirm stamps the charge on the app clock, the portal's "this month" statement (`app/server/src/business_portal.rs:366-369`, read) must use the same clock, or J2 step 5 "the statement CSV equals the ledger" (`docs/test-design.md:93`) and the unchanged T099 spec fail whenever the offset crosses a month end. Sessions are security time: reading a real-issued session on the app clock fails after a 30-day offset, and a single run advances further than that. The J2 steps (`docs/test-design.md:89-93`) assert no photo expiry, so the job page stays on real time.
- Plan PR must: CLK-a points session reads and issue on the switched routes (Confirm's principal, the portal's principal, the action-link Confirm session) at `real_now()`; CLK-b injects the clock into the portal; acceptance test: after a 100-day advance, a session issued before it still opens the portal and Confirm, and the charge shows in the statement. Plan session checks each site takes `now` as its own argument; where one does not, fall back to the verdict's approach for that site and seed an app-clock expiry. J2 core must not assert away or month text beyond step 5.

## C. Does the verification change my scope decision?

Yes, in four ways. The spec reading and the "never cut" list stand.

1. **Re-split and re-budget the T801 chain: seven tasks, not five.** V1 means the web chat has no intake after the code, so C+D builds it. Chain: A → C → D → E → F → H (T801), with I independent. Rough total 190-300M tokens (was 70-130M). Finish moves to about Dec 14-18.
2. **Design direction for C, to keep it off the dispatcher queue.** Eight queued tasks own `app/intake/src/order_dispatch.rs` (T299, T718, T719, T723, T736, T760, T823, T825). C should make the seam the guarded enqueue (`queue_text`, `queue_body` and the four other callers in R1-06), not the steps: an `sms` request goes to the outbox; a web request's reply goes to the page and never to the outbox; anything else fails closed. Web dispatch lives in a new file that calls the shared steps. Then C and D need only small edits to `order_dispatch.rs`, later step tasks inherit the web path, and the chain does not wait for those eight. The plan session may choose another seam if this one does not hold, and must say why.
3. **Dependency edges.** T533 → H, PKE, both cores, T840. E is ordered against T820 only if it edits `app/server/src/homeowner_portal.rs` (prefer not). I must not depend on T119 and takes `tests/e2e/j1_homeowner.spec.ts` from T801's owns. H takes `tests/a11y/page_states.ts` add-only and must keep "got your request" / "we've got your request" in OPEN (V3). A is ordered before the queued owners of `service.rs`, `rules.rs`, `conversation.rs`, `handlers.rs` (T704, T236, T823, T825, T680, T742, T818).
4. **Dropped, as the verdicts say:** the uniqueness migration, changes to `intake_order_corrections`, the under-13 test change, waiting on T802, "T725 missing", PF-CLK's CLK-02, CLK-05, CLK-07, CLK-08, and the owner/RefusalLog clock injection.

Unchanged: build order (T801 chain is the long path; the clock lane runs beside it; clock plan PR first), variants last, nothing for Casey.

## D. Plan order

Model rule applied: Opus for money, privacy, security, consent and recovery application code; Sonnet for test-only journeys and harness tasks without such code (Opus review; retries go to Opus, D-2474). Budgets are rough, one clean attempt plus review.

### 1. Clock plan PR (first, small)

| # | Task | Owns (broad) | Deps | Model | Budget |
|---|---|---|---|---|---|
| 1 | CLK-a real-time split, behaviour-neutral | `app/core/src/clock.rs`, `test_control.rs`; the stay-real sites in `app/payments/src/routes.rs`, `app/portals/src/security.rs`, `pick_flow.rs`, `confirm_pay.rs`, `app/server/src/inbound_text.rs`; session sites (C5) | none; T381, T811, T689, T402, T817 gain a dep on it | Opus | 20-30M |
| 2 | CLK-b composition | `app/server/src/main.rs`, `lib.rs` (`*_with_clock` wrappers, old fns kept), `checkin.rs`, `release_sink.rs`, `business_portal.rs` (injection only), `app/picks/src/settle.rs` (thread `at`); regression tests (advance then signed inbound; real-signed Stripe webhook after a multi-day advance; C5's 100-day test). T098, T099, J3 specs rerun unchanged | CLK-a; T718 gains a dep on it | Opus | 25-40M |
| 3 | SEEDS clock-aware seed options, old defaults | `tests/e2e/keyed.ts`, `tests/e2e/j2.ts` (granted from DONE T536) | T847, CLK-b | Sonnet (harness) | 10-20M |
| 4 | T222 → J1 full core through day-3 DONE | `tests/e2e/j1_full.spec.ts`, `j1_full.ts` | CLK-b, SEEDS, I | Sonnet | 20-40M |
| 5 | New: J2 full core | `tests/e2e/j2_full.spec.ts`, `j2_full.ts` | CLK-b, SEEDS | Sonnet | 20-40M |
| 6 | T532 keyboard-only | as today | tasks 4, 5 | Sonnet | 15-30M |
| 7 | T533 web-and-email J1/J2 | `tests/e2e/j1_j2_web_email.spec.ts` | tasks 4, 5; H; PKE; T840 | Sonnet | 20-40M |
| 8, 9 | J1 variants, J2 variants (late, off the critical path; no STOP or 911; web-chat variant is T533's) | new spec files | tasks 4, 5 | Sonnet | 15-30M each |

Lane total about 160-300M.

### 2. T801 plan PR (after the clock PR; two Opus review rounds; carries the TD-CON-36 rewrite, the lock rewrite, and the decision with T1-T5)

| # | Task | Owns (broad) | Deps | Model | Budget |
|---|---|---|---|---|---|
| A | Consent primitives: per-request `sms` predicate; `card_allowed`; `rules::plan` input; every text-opened request gets its record; chat TOCTOU (R2-02); guarded enqueue that fails closed; reword `app/intake/CLAUDE.md:16` | `app/intake/src/models.rs`, `service.rs`, `rules.rs`, `conversation.rs`, `handlers.rs`, consent tests | none new; T704, T236, T823, T825, T680, T742, T818 after it | Opus | 30-45M |
| C | Web dispatch after the code: ZIP, description from `said` (T4), shared steps to the coverage line, replies on the page, no outbox ID in chat results | new `app/intake/src/web_dispatch.rs`, `web_chat.rs`, enqueue seam | A | Opus | 40-60M |
| D | Remaining question steps on the page in table order (how soon, urgent, night, trade questions, name and address); real TD-INT-30 golden test | web dispatch, small `order_dispatch.rs` edits, integration tests | C; ordered with the `order_dispatch.rs` owners | Opus | 30-50M |
| E | Photo upload on the page (step 9); no `photo_text` SMS for a web request | `app/intake/src/photos.rs`, chat route and template | D; T820 only if it touches `homeowner_portal.rs` | Opus | 25-40M |
| F | Card on the page (step 12): timer starts at render to her verified session, SEND and EDIT typed; no delivery-status wait | `app/intake/src/summary_step.rs` (after T394), chat page | D (E and F may run side by side if their files are disjoint) | Opus | 30-45M |
| H | T801: live handoff screen (T725 predicate), mid-chat switch-over, "shown" audit event, T645/T649/T103/e2e transport assertions under the logged decision, `page_states.ts` add-only | `app/site/src/chat.rs`, `request.html`, `web_chat.rs`, the listed tests | A, C, D, E, F; T299 and T119 keep their dep on it | Opus | 35-50M |
| I | J1 observation fix (`queuedFor`, `say`, `quotePath`, consent read) | `tests/e2e/j1_homeowner.spec.ts` | none; before T119 and task 4 | Sonnet | 10-20M |
| PKE | Pick code by email when texting is off, suspended or web-and-email (T1) | `app/portals/src/pick_flow.rs`, `app/picks/src/email_code.rs` wiring, privacy grant, tests | T689 order; C if the email comes from the chat | Opus | 30-50M |

Chain total about 230-360M with PKE. No task G, no H3 journey. Still nothing for Casey: none of this is one of his stops (`BUILD_BRIEF.md:546-552`).
