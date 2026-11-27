# PF-P2-3a / PF-P2-3b verdicts: Opus read-only verification

Tree: `/tmp/claude-0/wt-pf` at `ed430909b`. Every task named here is QUEUED (`origin/claude/kit-state`).

Tally: 16 findings, 9 CONFIRMED, 7 PARTLY, 0 WRONG. Several severities change.

| Report | CONFIRMED | PARTLY | WRONG |
|---|---|---|---|
| 3a (11 findings) | 6 | 5 | 0 |
| 3b (5 findings) | 3 | 2 | 0 |

## PF-P2-3a (vendors)

### 3a-1. T129, T131, T132, T133, T134: no credential path for the sandbox hand-in

**Verdict:** CONFIRMED in substance, BLOCKER. The report gives the wrong cause.

**Wrong premise.** The packet brief said keys live only in Railway test variables. The repo does not say that:
- `BUILD_BRIEF.md:62-65` (§2) allows **network secrets (build)**.
- `BUILD_BRIEF.md:184` says the checklist puts each test key in "either a network secret (with the host to lock it to) or a test-project variable".
- `BUILD_BRIEF.md:516` gives "sandbox network secrets in Phase 2" for the build VM.
- `docs/architecture.md:467` agrees.

So the brief is not inconsistent, and no brief change is needed.

**Real gap.** The checklist that T127 sends never puts any vendor key where the build VM can use it:
- `docs/account-checklist.md:33-34`: Twilio SID, token and Verify SID are "test-project variables".
- `:62-63`: Stripe, test-project variables.
- `:71-72`: Resend, test-project variables.
- `:73`: the AI key, test-project variable.
- Only storage (`:74`) is a network secret.
- **Lob is not in the checklist at all**, though T133 is `sandbox-resend-lob`.
- Twilio *test credentials* (the magic-number SID and token that T129 needs) are not listed.

Ordering and access make this worse:
- T129 to T134 run before T135 (`T135.deps` includes T134), so the Railway test project is not deployed yet.
- Claude holds no hosting key (T135 goal).
- CI has no keys (`BUILD_BRIEF.md:517`; `.github/workflows/ci.yml:554-559` runs `pytest tests` bare).
- Result: each "must pass before hand-in" `tests/contract/test_sandbox_t1xx.py` (for example `kit/prompts/T129.md:171-174`) can only skip or fail.

**Smallest fix** (plan change before T127, which is QUEUED and sends the checklist; `docs/account-checklist.md` owners: T122 DONE, T141 QUEUED; the file is not in `kit/plan.lock`):
1. Add a "Phase 2 build sandbox" block to Part 1. Each item is a **network secret locked to its API host**:
   - Twilio test credentials (`api.twilio.com`), plus the sub-account key where test credentials cannot do Verify or voice.
   - Stripe test restricted key (`api.stripe.com`).
   - Resend test key (`api.resend.com`).
   - **Lob test key (`api.lob.com`), a new account item.**
   - B2 test bucket keys.
2. In T129's sandbox mode, which all the later sandbox tasks reuse:
   - With no `CONTRACT_BASE_URL` or credentials, sandbox tests skip. That is the CI case.
   - At hand-in, a skipped or unsupported case is reported as **NOT RUN** in `docs/for-casey-review.md`, never as a pass. This copies T131's existing "reported as not run, never as passed" clause to T129, T132, T133 and T134.

No GitHub secret and no live key are involved.

### 3a-2. T132: `Catching` drops the description

**Verdict:** CONFIRMED, MAJOR.
- T132's goal gives `IntentStripe` a default-body method.
- `Catching` forwards only `create_intent` (`app/portals/src/confirm_pay.rs:860-872`). It is built for the press at `:1790` and goes through `charge.rs:222` `open_intent`.

**Fix:** T132's goal says `Catching` overrides the new method and forwards the text, and adds a test that creates the intent on the press path. The file is already owned by T132.

### 3a-3. T132: hand-in names only Python tests

**Verdict:** PARTLY, minor.
- `kit/prompts/T132.md` "Run your tests" lists only `test_sandbox_t132.py` and `test_stripe_confirm.py`.
- `prehandin` and smoke still run the workspace's cargo tests.
- The TD-FEE-38 Rust cases live in `tests/integration/tests/confirm_and_pay.rs:273-312`.

The real hole is that nothing requires a Rust assertion that the adapter's create form carries `description` and `statement_descriptor_suffix`.

**Fix:** add one Rust test in `app/adapters` (T132 owns `payments.rs`) that checks the outgoing form, and add it to "Run your tests".

### 3a-4. T133: an unconditional "unsupported" branch would break `ssrf_guard.rs:427-439`

**Verdict:** PARTLY, minor.
- T133 owns a separate `app/adapters/src/resend_real.rs`, so the natural place for the branch is the real client.
- The stand-in `ResendEmail` DELETE, pinned by the DONE test, stays as it is.

**Fix:** one clause: "the unsupported error is the real client's only; `ResendEmail`'s DELETE and `ssrf_guard.rs` stay".

### 3a-5. T379: the notice-log SQL allowlist drops the new events

**Verdict:** CONFIRMED, MAJOR.
- `app/messaging/src/email_timing.rs:255-259` passes only `email.delivered`, `email.delivery_delayed`, `email.bounced` and `email.complained`.
- `from_event` runs after that, at `:280`.

**Fix:** T379's goal names the SQL allowlist, plus `.sqlx` regeneration (shared path). The file is already owned.

### 3a-6. T379: FakeMail and Spanish lines

**Verdict:** PARTLY, MAJOR on the language half.
- `ProviderStatus::line()` returns English only (`email_timing.rs:95-105`). No Spanish notice-log line exists anywhere, so "in English and Spanish" needs a new language-aware entry, and its caller is not named.
- The FakeMail half is a wording issue: `send_email` returns `Result<String, AdapterError>` (`tests/integration/tests/text_retries.rs:55-77`), so "answers … with `email.suppressed`" must mean that the fake send succeeds and the test then stores or injects the event.

**Fix:** state both points, and name the renderer's caller if it lies outside T379's files.

### 3a-7. T499: shallow history

**Verdict:** PARTLY, minor. The goal deepens only "if the recorded base is not in the fetched history" (`kit/prompts/T499.md:9`). The case where the base is present but `merge-base` or the ancestry proof fails is narrow, but real.

**Fix:** "deepen also when `git merge-base` fails or the ancestry check cannot be proved; then fail clearly".

### 3a-8. T129: "every create" against `sms.rs:72-81`

**Verdict:** PARTLY, nit.
- `status_callback: None` sends none, and the stand-in test builds the client without it (`app/adapters/src/tests/adapter_config.rs:359-364`).

**Fix:** "every create from a client composed with a public base URL".

### 3a-9. T133: stale cite

**Verdict:** CONFIRMED, minor. `app/adapters/src/email.rs` has 110 lines, and `["suppressions", address]` is at `:98`.

**Fix:** cite `email.rs:90-101`.

### 3a-10. T379: stale cites

**Verdict:** CONFIRMED, minor.
- `card_notice_accepted` is at `business_notices.rs:579`, not `:497`.
- `struct FakeMail` is at `text_retries.rs:55`, not `:60`.

### 3a-11. T499: stale ci.yml anchors

**Verdict:** CONFIRMED, minor.
- The checkout `fetch-depth: 200` is at `ci.yml:72`, not `:66`.
- `GH_TOKEN` is at `:146` and the fetch is at `:149`, not `:143`.

## PF-P2-3b

### 3b-1. T236: required fields on `IntakeCall` break tests T236 does not own

**Verdict:** CONFIRMED, BLOCKER.
- The struct is at `app/ai/src/service.rs:52-61`.
- Four struct literals sit in files T236 does not own:
  - `app/ai/src/tests/adapter_calls.rs:184` (T296, QUEUED).
  - `tests/integration/tests/prop_prompt_injection.rs:271` (DONE).
  - `tests/integration/tests/ai_zero_retention.rs:60` (DONE).
  - `tests/integration/tests/inbound_scrub_context.rs:164` (DONE). The goal does not mention this one.
- The goal keeps only the `read_intake(&mut PgConnection …)` form.

**Fix:** `IntakeCall` stays as it is. The budget context (conversation, job, step, business) is a separate type or argument on the new pool entry, and the thin wrapper passes none.

### 3b-2. T704: the INT-029 producer and the direct-reply producers sit outside T704's files

**Verdict:** CONFIRMED, BLOCKER, and broader than reported.
- The bank-question send is at `app/intake/src/order_dispatch.rs:1536-1551`, through `queue_body` (`handlers.rs:379-395`).
- `queue_body` producers also sit in `replies.rs:518`, `zip_step.rs:253`, `photos.rs:236` and `cards_out.rs:582`.
- T704 owns none of these files. Its goal says "a producer found outside these files is reported, not worked around", so TD-QH-07's 23:10 INT-029 case cannot pass.
- T704 already runs after T299, the first queued owner of `order_dispatch.rs`.
- T857 (plan branch) puts `queue_body` behind the consent guard, which interacts with the same helper.

**Fix:** T704 owns `order_dispatch.rs`, plus whichever of `replies.rs`, `zip_step.rs`, `photos.rs` and `cards_out.rs` queue exempt texts, ordered after their queued owners (T299, T823, T825, T857, T859, T860, T865). Or `queue_body` gains an exemption parameter, with all callers transferred under one logged decision. TD-QH-07 stays on the real dispatch path.

### 3b-3. T236: reservation order can leak the cap; the weekly count includes `budget` rows

**Verdict:** CONFIRMED, MAJOR.
- The goal makes the AI-021 admission and each AI-028 reservation separate short connections. If the global slot is taken first and a step budget then refuses, the day counter is spent with no call, which breaks "refused calls never use up the AI-021 cap".
- `weekly_counts` (`app/ai/src/switch.rs:243-248`) excludes only `daily_cap`, so `budget` rows count as "calls" and as "ended".

**Fix:**
- Take the step reservations first and the global admission last, so a budget refusal never touches the day counter. A single transaction also works.
- `weekly_counts` excludes `budget`.
- Add one concurrent test.

All files are owned by T236.

### 3b-4. T298: a real 429's `retry-after` is lost

**Verdict:** PARTLY, minor.
- `app/ai/src/models.rs:274-283` sets `retry_after: None` on purpose under **D-810** (`docs/decisions.md:816`), failing toward the owner alert. It is a logged deviation from AI-023 (`docs/spec/ai.md:27`), not a new gap.
- With a 3-second budget, waiting out a provider `retry-after` rarely fits anyway.
- The effect with the real API is an owner alert on every 429, not wrong behaviour.
- T298 does not run after T236, and `models.rs` is owned by T236, T296 and T394.

**Fix:** log the decision to accept D-810 for the real client (owner-alert noise is noted). Or a later owner of `models.rs` maps a header that the adapter error carries. Not needed before Gate C.

### 3b-5. T659: homeowner bounce "change nothing"

**Verdict:** PARTLY, minor.
- `kit/prompts/T659.md:9` and TD-SEC-76 (`docs/test-design.md:882`) say a homeowner email bounce "changes nothing".
- `app/messaging/src/email.rs:915-916` and `:954-955` (TXT-018, D-5442) switch her back to text on an update-address bounce.
- The two collide only if the fixture uses her current update address.

**Fix:** reword to "marks no business mailbox returned and queues no returned task". The fixture is an ordinary homeowner notice. This is a test-design change by decision (`docs/test-design.md` owner T511, QUEUED).

## Top items, in order

1. **3a-1:** the checklist gives the build VM no Twilio test, Stripe, Resend or Lob key, and Lob is missing entirely. Fix it before T127 sends the checklist, and add the NOT RUN rule.
2. **3b-2:** T704 cannot reach the INT-029 and direct-reply producers.
3. **3b-1:** T236's `IntakeCall` change breaks 3 DONE tests and T296's test.
4. **3b-3:** T236's reservation order and weekly count.
5. **3a-2:** T132's `Catching` forwarding.
6. **3a-5 and 3a-6:** T379's SQL allowlist and Spanish line entry.
