> Outside review PF-T801, received from Casey Nov 24, 2026 (about 6:40 am MST). Unverified: verify every finding on `main` before acting. Input for planning T801 after the Thursday Nov 26 reset (Casey's decision), with two Opus review rounds.

# PF-T801 — Consent plan fix

**Verdict: BLOCKED — architectural and task-plan changes required before T801 is attempted again.**

| Review item | Assessment |
|---|---|
| Current task plan | Not ready |
| Recommended approach | Split the task |
| Review level | Opus — high effort |
| Priority | P0 — Consent and functionality |

## 1. Summary

The reported plan gap is valid, but the problem extends beyond the two previously completed test files. Restricting `contact_consented` to SMS consent without separating intake execution from message delivery will break the web-chat intake sequence. Additionally, the current SMS consent-recording logic does not establish SMS consent when a person first texts from a number already associated with an open web-chat request. The existing 10-file, 700-line T801 budget is inadequate for the complete correction.

**Do not send the existing T801 prompt back to the worker.** Correct the plan and its dependencies first.

Review basis: supplied repository snapshot `16fd7ee8`, T801 prompt, decision excerpts and specifications. This is a static design and source review, not a compiled implementation or executed test run.

## 2. Design

### 2.1 Separate intake authorization from SMS authorization

The principal architectural mistake is treating permission to communicate by SMS as permission to process a request. These must be independent decisions.

```text
Homeowner request
  ├── May process intake? (request authorization)
  │   └── SMS or verified web → execute intake steps, keep safety gates
  │       └── Web/email output where appropriate; no SMS outbox rows for web-only replies
  └── May send SMS? (SMS consent + mode)
      └── Actual inbound SMS required for consent; Verify code is not consent
          └── SMS delivery only when permitted
```

| State | Intake allowed | Homeowner campaign SMS |
|---|---|---|
| Unverified web session | No | No |
| Verified web-only request | Yes | No |
| Genuine SMS consent, texting disabled | Yes, through permitted channels | No |
| Genuine SMS consent, texting live | Yes | Subject to existing send restrictions |
| Texting live, new web visitor | No request opened | No |

Twilio Verify is the specific identity-verification exception permitted by CON-025 and TXT-038. It must remain separate from informational SMS consent and the campaign outbox.

**Required changes:**

- `app/intake/src/models.rs:contact_consented` — restrict the existing query to `r.channel = 'sms'` for the exact contact.
- `app/intake/src/service.rs:may_text` — retain its narrow meaning of SMS consent eligibility. It must not be used as the sole authorization for delivering a message.
- `app/intake/src/service.rs:card_allowed` — preserve request-level card authorization, including the permitted web-chat path under CON-007.
- Introduce request-level intake authorization that validates the request, contact and authenticated channel without granting SMS permission.

Do not simply rename `may_text` and continue calling it everywhere.

### 2.2 Introduce channel-aware intake replies

**Files:** `app/intake/src/service.rs`, `app/intake/src/order_dispatch.rs`, `app/intake/src/web_chat.rs`.

| Source | Problem |
|---|---|
| `service.rs:68–86` | `Queued` and `Handled` represent replies through outbox IDs. |
| `web_chat.rs:392` | `text_in` converts a web-chat turn into an apparent inbound text. |
| `web_chat.rs:448–475` | `text_turn` runs `receive_text` and returns `ChatAnswer::Texted`. |

The smallest safe architectural change is to keep one intake state machine but introduce an explicit channel-aware delivery mechanism.

Suggested design, not a finalized API signature:

```rust
enum ReplyChannel {
    InboundSms,
    VerifiedWebChat,
}
```

Rendered web replies must be returned as page content, rather than represented as successfully queued SMS messages.

Requirements:

- Reuse existing template rendering and substitution.
- Preserve the order of multiple replies.
- Render dynamic question values and choices.
- Preserve question markers, answer handling and transaction boundaries.
- Preserve deduplication and state transitions.
- Never create a fake outbox UUID for a web reply.
- Never enqueue an SMS and subsequently delete it to simulate web-only delivery.
- Never infer that a synthetic `TextIn` created by the web chat represents a genuine inbound SMS.

A second complete intake implementation for web chat would be unnecessary and would introduce long-term divergence.

### 2.3 Correct the intake step gates

**File:** `app/intake/src/order_dispatch.rs`.

| Function | Required change |
|---|---|
| `first_text`, :243 | Reuse the initial intake work for web requests without recording an inbound SMS. |
| `reply_step`, :361 | Replace the SMS-consent execution gate at :371 with request-level authorization. |
| `ask_open_again`, :467 | Replace the SMS gate at :476; preserve SAFE and open-question behavior. |
| `show_card_step`, :513 | Replace the SMS gate at :522; show the card on the authorized page. |
| `send_trade_ask`, :879 | Deliver the rendered question through the selected channel. |
| `ask_step`, :1327 | Present the actual next question through the selected channel. |
| `hold_911`, :1569 | Display the safety response without granting SMS permission. |

Keep every existing emergency hold, closed-request restriction, card-release restriction and question-order rule.

**Important additional finding:** merely removing the three `may_text` guards is unsafe. Downstream functions still directly invoke `queue_text`, `queue_body` and other SMS-specific helpers. Those delivery points must be corrected in the same implementation.

Several of those helpers are defined in files absent from the package, so their complete ownership requirements cannot yet be determined.

### 2.4 Change verified web-chat opening

**File:** `app/intake/src/web_chat.rs:open_verified`, lines 730–777.

Currently, this function explicitly queues the consent text before writing the `web_chat` consent record.

Replace that behavior with the following:

1. Validate the Verify code and existing admission requirements.
2. Open or safely associate the request.
3. Record consent with channel `web_chat`, session ID, time and exact displayed consent version.
4. Present the appropriate consent wording on the page.
5. Start the next applicable intake step, including the ZIP question when appropriate.
6. Return rendered page replies without creating a homeowner SMS outbox row.

The existing `intake.consent_sent` audit action must also be reviewed. A page disclosure must not be falsely reported as a sent SMS.

Preserve the request source, rate limits, consent hashing, language selection and existing transaction guarantees.

### 2.5 Replace every chat-delivered text with its proper presentation

| Existing text | Web-chat replacement | Governing requirement |
|---|---|---|
| Consent text | Existing disclosure above chat | CON-025, CON-031 |
| ZIP question | Actual question on the page | INT-049 |
| Emergency warning | Exact INT-004 warning on screen | INT-002, TXT-034 |
| Underage response | Exact INT-038 response on screen | INT-038 |
| Trade questions | Questions and choices in chat | INT-039, INT-043 |
| Name and address | Actual question in chat | INT-027 |
| Picture request | Upload link and optional-photo wording | INT-044 |
| Inspiration pictures | Page upload and design questions | INT-045 |
| Card approval | Card with EDIT/SEND actions | INT-041 |
| Quote updates | Authorized quote page and email | TXT-021, TXT-034 |
| LIST | Quote information on the authorized page | TXT-031 |
| Spanish unavailable notice | Existing Spanish notice on screen | TXT-039, TXT-041 |

For name and address, retain vault-only storage and the restrictions on passing personal information to the AI.

For photos, retain validation, metadata stripping, re-encoding, confirmation and the prohibition on photos during a 911-list situation.

### 2.6 Correct the site and route behavior

**Files:** `app/site/src/chat.rs`, `app/site/templates/request.html`, `app/server/src/lib.rs`.

Change:

- `ChatAnswer::Texted` and `ChatAnswer::Replied` handling at `chat.rs:445` to display the rendered responses.
- `after_check` at `chat.rs:592` to display the actual next intake question.
- The start-screen and POST handling to use T725's authoritative homeowner-texting availability predicate.
- The template to distinguish the live-texting start screen from the verified web-chat flow.

The availability decision must be enforced server-side for GET and POST. Hiding the phone and code controls alone is insufficient.

Test stale or crafted form submissions after the owner switches texting modes. A stale browser must not bypass the new consent boundary.

Preserve CSRF protection, escaped content, no-store responses, keyboard behavior, accessible status announcements and existing error handling.

**Privacy warning:** the current chat history is carried in a hidden `said` field (`chat.rs:230`, `request.html:55`). Do not add private quote tokens, unredacted name-and-address data or other sensitive response content to that field without reviewing its trust and privacy boundaries.

### 2.7 Separate background notifications from intake progression

These functions cannot simply be treated as interactive chat responses because they can run without a browser request.

| File/function | Required correction |
|---|---|
| `job_texts.rs:job_text_step`, :270 | Evaluate whether the notice is due independently of whether SMS is permitted. Route applicable notices through authorized email or page delivery. |
| `job_texts.rs:queue`, :521 | Preserve delivery idempotency, secure links and status checking. |
| `quote_updates.rs:quote_update_step`, :106 | Do not abandon all quote-update processing at the `may_text` check at :115. |
| `quote_updates.rs:send`, :259 | Support authorized non-SMS delivery and update the notification marker correctly. |
| `picture_text.rs:drive`, :339 | Separate authorized photo-processing progression from permission to reply by SMS. |

There is already relevant channel-routing functionality at `app/messaging/src/service.rs:insert`, lines 217–231, associated with D-5585.

It calls `update_emails::route` and can avoid creating an SMS row when the recipient's delivery method is email-only.

Reuse that implementation where applicable, rather than building a second email-routing system. However, its behavior cannot be fully verified because `app/messaging/src/update_emails.rs` is missing from the supplied archive.

Also distinguish actual inbound picture messages from pictures uploaded through the web page. A web upload must not be manufactured into an inbound SMS to advance the state machine.

### 2.8 Handle the first genuine inbound SMS after web chat

**Additional P0 finding not adequately covered by the current T801 prompt.**

In `app/intake/src/service.rs:handle`, lines 140–185, the system checks whether the contact already has an open conversation.

If one exists, it takes the existing-request reply path.

In `receive_answer`, lines 220–251, the SMS consent record is written only if the returned messages include a consent template.

Consequently, a homeowner who has already completed the web-chat verification process can later send her first real SMS and still fail to obtain an SMS consent record.

There is another related issue: `app/intake/CLAUDE.md:16` currently requires one consent record per request and none from a reply. That invariant must explicitly accommodate the first genuine SMS associated with an existing web request.

| Event | Result |
|---|---|
| Phone entered in web chat | No SMS consent |
| Verify code accepted | Web-chat consent only |
| Further web-chat messages | No SMS consent |
| First qualifying genuine inbound SMS | Establish SMS consent using its real inbound ID and time |
| Duplicate delivery of that SMS | No duplicate consent record or consent message |
| Subsequent ordinary SMS replies | No repeated consent message |
| STOP or HELP | Preserve existing keyword handling and exceptions |

The existing web-chat request should not be duplicated simply to establish SMS consent.

The exact representation of the additional consent event must be verified against the database constraints and the append-only consent invariant. Do not overwrite the original `web_chat` record or fabricate an inbound message ID.

**Missing dependency:** the consent table migration and its unique constraints are not included. A schema change may be required.

## 3. Test changes

The following are justified corrections to assertions that contradict the current specification. They must be authorized in the decision log before implementation.

### Governing specification excerpts

| Requirement | Exact relevant excerpt |
|---|---|
| CON-005 — `consent.md:9` | “a consent record it writes never permits a text” |
| CON-021 — `consent.md:25` | “a web-chat requester only after she texts in” |
| CON-025 — `consent.md:29` | “a phone number typed there gets only the Twilio Verify code” |
| TXT-034 — `messaging.md:38` | “answer intake there” |
| INT-043 — `intake.md:47` | “a builder or golden test (OPS-040) never invents another order” |

### T645 — `app/server/tests/web_chat_stops.rs`

| File:line | Old assertion | Required new assertion | Spec |
|---|---|---|---|
| :503–506 | INT-038 reply exists in SMS outbox | Exact INT-038 reply on page; zero campaign SMS rows | CON-005, INT-038 |
| :572 | One queued Spanish consent text | Zero SMS queued | CON-025 |
| :574–578 | Outbox contains Spanish consent body | Exact Spanish disclosure displayed; zero SMS rows | CON-025, CON-026 |
| :592–596 | Consent and Spanish fallback queued | Zero SMS queued | CON-005 |
| :599–603 | Spanish fallback in outbox | Exact Spanish fallback displayed | TXT-039, TXT-041 |
| :604–607 | English consent in outbox | Exact English disclosure displayed | CON-025 |

The brief's reference to line 485 for the underage queued-text assertion is stale. In the supplied snapshot, the relevant assertion is at lines 503–506.

**Keep unchanged:** underage closure, under-13 deletion, no cards, suppression records, opt-out confirmation on screen, no additional SMS after opt-out, language persistence, 911 ordering and the vault-digest failure safeguards.

### T649 — `app/server/tests/web_chat_after_code.rs`

| File:line | Old assertion | Required new assertion | Spec |
|---|---|---|---|
| :46 | `TEXTED` contains a promise to answer by text | Replace with page-response expectations | TXT-034 |
| :235 | Name/address question reported as asked | Preserve question-state assertion; additionally prove page delivery | INT-027 |
| :319 | Page contains `TEXTED` | Actual permitted reply or next question appears on page | TXT-034 |
| :325–330 | Exactly one name/address outbox entry | Zero SMS entries; prove the question was presented once and not reopened | CON-005, INT-027 |
| :368 | LIST answered by text | LIST result or proper empty-state response displayed on page | TXT-031, TXT-034 |

The name-and-address test must not be reduced to a simple zero-outbox check. Its original purpose—proving that the question is not asked twice—must remain intact.

**Keep unchanged:** vault storage assertions, redaction, absence of personal information in retained job descriptions, question closure, LIST not becoming an address answer, and under-13 data deletion.

### Other affected tests found in the package

| File:line | Old assertion | Required new assertion | Spec |
|---|---|---|---|
| `tests/integration/tests/web_chat.rs:361–373` | Queued consent template and body equal page wording | No campaign SMS; exact rendered disclosure and matching stored consent version | CON-025, CON-005 |
| `web_chat.rs:501–505` (integration test) | One first/only consent SMS | Zero SMS; correct first visible intake reply | CON-025, INT-043 |
| `web_chat.rs:631` (integration test) | Reverified request queues one SMS | No SMS; new request and web consent recorded | CON-025 |
| `tests/e2e/web_chat.spec.ts:154–161` | One consent outbox row matching page | Exact page disclosure, next question, web consent, zero campaign SMS rows | CON-025, TXT-034 |
| `app/site/src/chat.rs:765–771` | Successful verification produces `OPEN` notice | Preserve success status and next-step rendering | CON-025, A11Y-030 |
| `app/site/src/chat.rs:851–863` | OPEN/TEXTED status notices | Preserve accessibility checks for replacement responses | A11Y-030 |

For the integration consent-version test, compute the expected version from the actual displayed disclosure. Preserve the existing checks of channel, session ID, language, record count and digest.

### 3.1 D-5716 needs a separate correction

The current T801 prompt has stale line references and understates the impact of the e2e sender becoming active.

The supplied snapshot shows:

- `tests/e2e/j1_homeowner.spec.ts:236` reads the first SMS body directly from the outbox.
- `queuedFor`, at lines 85–91, reads all SMS bodies.
- `say`, at lines 94–99, uses those bodies for intake assertions.
- `quotePath`, at lines 178–181, extracts private quote-page links from message bodies.

Once a sent outbox row's body is cleared, all relevant consumers of `queuedFor` can become affected—not just the consent assertion.

The test harness must obtain the original message body from a reliably correlated fake-carrier record where necessary. Do not use an ambiguous recipient-only match when several messages to the same recipient have the same template.

The actual SMS journey must retain its exact consent, ZIP, question, card, quote-link and duplicate-delivery assertions.

Conversely, `web_chat.spec.ts:159` must no longer fall back to a fake-carrier consent SMS. That test should prove that no such SMS exists.

### 3.2 Required additional regression coverage

Acceptance test checklist (proposed tests — not yet executed):

- [ ] Live texting: number and disclosures shown; no request or Verify step.
- [ ] Live texting: stale or crafted number/code POST cannot bypass the restriction.
- [ ] Verified web chat: entire intake and card approval complete with zero campaign SMS.
- [ ] Web-only consent: may_text is false while card authorization remains valid.
- [ ] First genuine SMS after web chat records consent exactly once.
- [ ] Concurrent or duplicate inbound SMS cannot duplicate consent.
- [ ] Previously SMS-consented contact using web chat still follows web-only delivery.
- [ ] Paused, suspended, pre-launch and web-and-email modes tested independently.
- [ ] 911, SAFE, underage stops, STOP and HELP retain existing behavior.
- [ ] Spanish enabled and disabled paths show the correct words.
- [ ] Quote updates and photo uploads work without SMS.
- [ ] Mode changes do not release previously unauthorized SMS.
- [ ] One person's web consent cannot authorize another contact's number.
- [ ] All existing SMS intake and accessibility regression tests remain green.

## 4. Copy drafts

These are proposed replacements for the copy panel, not approved final copy.

| Location | Proposed wording |
|---|---|
| `OPEN` | Thanks, we've got your request. |
| `TEXTED` | Got it. |
| Live texting start | Text {number} from your phone to continue. |
| Web-chat continuation | Answer the next question here. |
| Photo request | A photo or two helps businesses quote. No photo is fine too. |
| Photo action | Upload photos |
| Quote page action | See your quotes |
| Email updates | What email should we use? We never share it with businesses. |

`TEXTED` should preferably be renamed internally to `RECEIVED`, if a generic acknowledgment is still necessary.

An acknowledgment alone is not an adequate replacement for a question. The actual rendered intake response must follow it.

Keep the existing fixed CON-031, INT-004, INT-038 and applicable Spanish wording unless a separate specification change explicitly authorizes changes.

In particular, CON-031 itself contains SMS-related wording. CON-025 explicitly requires that disclosure above the web chat, so it must not be silently rewritten by this task. Additional clarification can state:

> Using this chat does not sign you up for text updates.

All brand names, domains and public numbers must come from their existing authoritative data and configuration.

## 5. Plan change

### 5.1 Recommended task split

Do not attempt to force the complete fix into one oversized T801 PR. I recommend three coordinated implementation tasks, plus a separate e2e harness correction.

```text
Plan and schema verification — Correct D-5279 and allocate ownership
  ↓
A — Intake channel separation — Authorization, replies, consent provenance
  ↓
B — Web chat and T801 — Live screen, web-only intake, regression tests
  ↓
C — Background delivery — Email, quotes, photos, full journey
```

Dependency order: the completed chain must pass all consent and no-SMS journey tests before launch.

The split must not introduce an intermediate revision that enables unauthorized texting or disables working intake while claiming completion. Implement and merge the channel-boundary changes together where necessary.

#### Task A — Intake channel separation and consent provenance

**Classification:** Harness.

Proposed primary ownership:

```text
app/intake/src/models.rs
app/intake/src/service.rs
app/intake/src/order_dispatch.rs
app/intake/src/conversation.rs
app/intake/src/handlers.rs
app/intake/src/replies.rs
app/intake/src/summary_step.rs
app/intake/src/zip_step.rs
tests/integration/tests/consent_records.rs
```

Several of these files are referenced by imports but not included in the package. Confirm their existing ownership and exact responsibilities before locking this list.

Goal: separate intake execution, rendered responses and SMS consent. Correct the first genuine SMS after web chat, preserving consent-record integrity and the existing SMS journey.

#### Task B — Revised T801

**Classification:** Harness.

Proposed primary ownership:

```text
app/intake/src/web_chat.rs
app/site/src/chat.rs
app/site/templates/request.html
tests/integration/tests/web_chat.rs
app/server/tests/web_chat_route.rs
app/server/tests/web_chat_stops.rs
app/server/tests/web_chat_after_code.rs
app/server/tests/home_request_box.rs
tests/e2e/web_chat.spec.ts
```

Explicitly transfer the affected T645 and T649 test files under a logged specification correction, following the D-5386 pattern.

Use the existing shared-path rules for server composition, crate API exports, documentation and accessibility entries.

#### Task C — Non-SMS background delivery

**Classification:** Harness.

Initial ownership candidates:

```text
app/intake/src/job_texts.rs
app/intake/src/quote_updates.rs
app/intake/src/picture_text.rs
app/intake/src/photos.rs
app/messaging/src/update_emails.rs
```

Add the actual tests and email-routing implementation files after inspecting their current owners.

Goal: ensure quote updates, photo steps, applicable notifications and secure page access continue without unauthorized SMS.

#### Separate harness correction — D-5716

Move the `j1_homeowner.spec.ts` sent-body retention issue to a dedicated harness correction, coordinated with T119.

It is independent of changing web-chat consent semantics and deserves focused regression coverage.

### 5.2 Budget recommendations

The current budget at `prompts/T801.md:58` is 10 owned files, 700 changed lines and 39% context.

I recommend these provisional replacement ceilings:

| Task | Owned files | Changed lines | Context |
|---|---:|---:|---:|
| Existing T801 | 10 | 700 | 39% |
| A — Channel separation | 14 | 1,400 | 45% |
| B — Revised T801 | 10 | 1,100 | 40% |
| C — Background delivery | 12 | 1,000 | 40% |
| D-5716 harness correction | 4 | 300 | 25% |

These are planning estimates, not measured implementation sizes. Recalculate the context budgets using the complete source files and generated prompts before allocating the tasks.

**Model assignment:** retain Opus at high effort for the three consent and channel tasks. Retain Opus privacy/compliance review and the two review rounds. The isolated fake-carrier test correction is suitable for Sonnet with an Opus reviewer, once its behavior is fully specified.

### 5.3 Goal sentences to add

The following can replace the defective portions of T801's goal and be allocated across the new tasks.

**Revised goal — T801 consent and channel separation**

While homeowner texting is live, the web chat displays the configured homeowner number and required disclosures and opens no request. Neither the phone-number step nor the Verify-code step is available through the page or its POST handlers.

While homeowner texting is not live, a verified web-chat session may run the entire homeowner intake sequence, including the permitted card-release steps, without creating a homeowner campaign SMS outbox row.

Intake progression is authorized independently of permission to text. The same intake rules, question order, safety checks, privacy restrictions, answer markers and card-release rules apply regardless of delivery channel.

A `web_chat` consent record never authorizes SMS. A Verify code is identity verification only and never constitutes consent to informational texts.

Web-chat replies are rendered and returned to the authorized page. They are not created as SMS outbox messages, represented by fictitious outbox identifiers or removed from the outbox after insertion.

A person's first qualifying genuine inbound SMS can establish SMS consent when an existing web-chat request is open. It records the real inbound message ID, time and consent version without modifying the original web-chat consent record or opening an unnecessary duplicate request.

Quote updates and other applicable asynchronous communications use the authorized email and page mechanisms when SMS is unavailable. Picture uploads advance the same intake steps without manufacturing inbound picture texts.

All affected tests owned by completed tasks change only through the logged specification correction. Every existing safety, privacy, consent, ordering, accessibility and deduplication assertion not contradicted by the amended specification remains enforced.

Acceptance requires a complete web-chat intake and card-release journey with zero homeowner campaign SMS outbox rows, separate verification of the live-texting start screen, and passing regression tests for the existing SMS path.

### 5.4 Proposed decision entry

Use the next valid decision number assigned through the established runner process. Do not invent or reserve a number in this review.

**D-[NEXT] — T801 consent and web-chat delivery correction**

**Question:** T801's first attempt stopped with a confirmed plan gap. Review of main at `16fd7ee8` establishes that D-5279's statement that the completed T645 and T649 tests pass unchanged contradicts the amended consent requirements. The problem also extends to intake authorization, reply delivery, later inbound SMS consent and asynchronous homeowner notifications.

**Evidence:** CON-005 (`docs/spec/consent.md:9`) states that a web-chat consent record never permits a text. CON-021 (`:25`) permits texting a web-chat requester only after she texts in. CON-025 (`:29`) permits only the Verify code before web-chat verification and distinguishes web-chat consent from SMS consent. TXT-034 (`docs/spec/messaging.md:38`) requires the homeowner journey to operate without texts.

`app/intake/src/models.rs:348` currently treats any consent record as SMS permission. `app/intake/src/order_dispatch.rs:371`, `:476` and `:522` use SMS permission to gate intake execution. `app/intake/src/web_chat.rs:730–777` explicitly queues consent messages when a web-chat request is verified. Existing assertions in `app/server/tests/web_chat_stops.rs` and `app/server/tests/web_chat_after_code.rs` require texts that these specifications prohibit.

`app/intake/src/service.rs:220–251` also records SMS consent only when the handler returns a consent template, leaving a gap for the first genuine inbound SMS associated with an already-open web-chat request.

**Choice:** Supersede only D-5279's incorrect assertion that affected completed tests pass unchanged and its assumption that restricting `may_text` alone preserves the chat intake steps. The substantive CON-005, CON-021, CON-025 and TXT-034 requirements remain authoritative.

Separate request authorization, intake execution, SMS consent and reply delivery. Preserve verified web-chat intake with no homeowner campaign SMS outbox entries. Preserve the existing SMS journey, all safety restrictions and consent provenance.

Under `BUILD_BRIEF.md:69`, explicitly transfer the affected completed test files to the designated harness tasks. Replace contradictory SMS expectations with exact page-output, state-transition and zero-SMS assertions. No unrelated test assertion may be removed or weakened.

Verify the consent-record schema and the completed-task ownership map before locking the replacement tasks. Split T801 into appropriately bounded dependent tasks for the shared intake mechanism, web-chat integration and asynchronous delivery. Allocate the D-5716 fake-carrier body-retention correction separately.

Retain Opus workers and Opus privacy/compliance reviewers for consent-sensitive changes, with two review rounds. Completion requires passing targeted integration, server and browser tests, full smoke, prehandin and Ally checks.

## 6. Risks and open questions

| Severity | Outstanding issue | File needed to settle it |
|---|---|---|
| P0 | Existing one-consent-record-per-request constraint may prevent recording later genuine SMS consent. | Migration defining `intake.consent_record` |
| P0 | Shared reply helpers may enqueue SMS without going through the five identified consent gates. | `app/intake/src/conversation.rs`, `handlers.rs`, `replies.rs` |
| P0 | Actual SMS webhook dispatch may require changes to establish trusted inbound provenance. | Server inbound webhook and dispatch modules |
| P0 | Email-only routing may not cover a web requester without a confirmed email. | `app/messaging/src/update_emails.rs` |
| P0 | Card state and auto-send scheduling may depend on an SMS being queued. | `app/intake/src/summary_step.rs` |
| P1 | Exact photo-upload continuation and authorization need verification. | `app/intake/src/photos.rs` and upload handlers |
| P1 | First-message preservation: `first_message` stores ID and language, but not its description. The initial intake must not lose or double-process the job description. | Complete web-chat and intake dispatch modules |
| P1 | Behavior of a verified web session when texting becomes live during intake needs an explicit transition rule. | `docs/architecture.md`, T725 implementation |
| P1 | Existing tests explicitly enforce the older `may_text` execution invariant. | `tests/integration/tests/intake_order_corrections.rs` |
| P1 | T802 is concurrently editing accessibility states. | Current T802 prompt and branch diff |
| P1 | Existing test ownership and dependency ordering cannot be verified completely. | `docs/tasks.md` or authoritative task graph |
| P1 | Consent and notification architecture updates must be checked against locked blueprint sections. | `docs/architecture.md` |
| P2 | Other tests outside the supplied package may rely on outbox IDs, SMS bodies or the previous page wording. | Complete repository and test-design document |

### T802 coordination

The `/request` accessibility entries in the supplied `tests/a11y/page_states.ts` are around line 544, rather than the T801 prompt's reference to line 446.

Do not remove the existing web-only state to accommodate the new live-texting screen.

Add independently seeded accessibility coverage for the live-texting screen, and coordinate the shared edit with T802. Preserve the existing web-chat question, error, focus and keyboard checks.

### Additional tests likely to be affected

The supplied `app/intake/CLAUDE.md` identifies several guarding tests that warrant examination before the plan is locked:

- `tests/integration/tests/intake_order_corrections.rs`
- `tests/integration/tests/intake_order_area_trade.rs`
- `tests/integration/tests/intake_order_two_jobs.rs`
- `tests/integration/tests/pick_lapsed_email.rs`
- `tests/integration/tests/pick_moved_text.rs`
- `tests/integration/tests/help_answers.rs`
- `tests/integration/tests/consent_purge.rs`

Their contents were not supplied, so I cannot determine whether their assertions require changes. They must not be assumed incorrect or preemptively rewritten.

## Final assessment

| Item | Assessment |
|---|---|
| Problem identification | Confirmed |
| Existing T801 scope | Insufficient |
| Existing consent architecture | Correction required |
| Proposed design direction | Recommended |
| Ready for autonomous implementation | Conditional |

The most important correction is to stop treating SMS as the mechanism that drives intake. SMS should be one delivery channel for the intake engine, not a prerequisite for executing it.

Fixing only the consent query and the two contradictory test files would solve one compliance defect while introducing substantial functional regressions.

**Recommendation: keep T801 blocked until the consent schema, shared reply helpers and email-routing implementation have been inspected, ownership has been allocated, and the replacement prompts have been generated.**

No repository files were modified, no tests were weakened, and no build or test results are claimed.
