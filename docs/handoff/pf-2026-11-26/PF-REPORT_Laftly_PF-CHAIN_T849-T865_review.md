# Laftly — PF-CHAIN preflight review

**18 tasks checked; 5 blockers, 9 majors, 2 minors.**

**Disposition: FAIL — do not release the full chain.**  
D-5836 · D-5848

The largest problems are in the web-chat consent chain. Splitting the implementation into smaller tasks is reasonable, but the intermediate states introduce conflicting consent requirements, and several tasks assume that downstream functionality already exists.

This is a static preflight review of the supplied package, not an executed test run. Findings are limited to implementation feasibility, ownership, consent, clock handling and acceptance criteria.

## 1. Findings table

| # | Task | Severity | What breaks | Evidence | Smallest fix to the plan |
|---:|---|---|---|---|---|
| 1 | T856, T857, T865 | BLOCKER | The temporary chat exemption explicitly permits texting without SMS consent, contradicting the locked consent specification. | `docs/spec/consent.md:9`: “a consent record it writes never permits a text.” T856's goal nevertheless allows web-only consent to retain outbox delivery regardless of caller. | Keep legacy transport testing isolated from real delivery. Do not permit the exemption to authorize production SMS. |
| 2 | T857 → T801 | BLOCKER | T857 must prove the legacy exemption works. T801 must delete it, but does not own T857's regression test. | T857 acceptance: “the chat exemption keeps the old chat texts.” T801 removes `CHAT_LEGACY_TEXTS` but does not own `tests/integration/tests/guarded_enqueue.rs`. | Grant T801 ownership of the transitional test and explicitly authorize its replacement with a zero-SMS assertion under D-5848. |
| 3 | T857 | BLOCKER | The legacy web-chat bootstrap queues messages before the web-chat consent record exists. The proposed guard requires a record for its exemption. | `app/intake/src/web_chat.rs:764–773`: `queue_text(...)` executes before `record(...)`. The Spanish fallback is also queued before the record. | Establish the record before guarded delivery or use an isolated transitional test path consistent with finding 1. Preserve transactional rollback. |
| 4 | T858, T859 | BLOCKER | T858 claims the behavioral golden test for the ordered web-chat journey before T859 implements step 9. T858 already covers steps 10 and 11. | `docs/spec/intake.md:69–71`: step 9 is photos, step 10 design categories, step 11 name and address. Test-design line 323, TD-INT-30, requires a golden test that fails if any two steps swap. | Give T858 a partial-order test and transfer ownership of the complete TD-INT-30 golden test to the task completing the journey. |
| 5 | T862 | BLOCKER | The conversation email is required to be readable only by Picks, but the same task requires it in the homeowner's data download. Both requirements cannot literally hold simultaneously. | T862 acceptance: “read only by `picks` under the pick-code grant.” Its goal also requires `data_download.rs` to include the address. `app/privacy/src/models.rs:262–267` currently grants PickCode access only to `[Phone]`. | Define separate, narrowly scoped grants for pick-code delivery and the homeowner's own data export. Neither grant should expose other email fields. |
| 6 | T856 | MAJOR | Replacing the consent predicate alone does not implement first-inbound SMS consent for an existing web request. | `app/intake/src/service.rs:169–170` still sends existing requests to `screen_reply`; `:229–245` creates consent only when a consent template was queued. | Specify a separate transactional web-to-SMS transition that queues consent and records the carrier inbox ID exactly once before normal progression. |
| 7 | T856 | MAJOR | Browser messages still enter the carrier-oriented intake path. Changing its consent predicate risks treating a browser message as the first inbound SMS. | `app/intake/src/web_chat.rs:455–456`: `let text = text_in(contact, msg);` followed by `receive_text(...)`. | Require structurally separate carrier and browser entry points before changing consent detection. Test both directions of the race. |
| 8 | T857 | MAJOR | The unchanged queue signatures do not carry an explicit request ID. Their subject arrays represent different entities, not consistently a conversation. | `app/intake/src/handlers.rs:372`: `queue_body(..., &[wait.job], ...)`; `summary_step.rs:186–187`: `[p.job, card_id(&digest), show_id(show)]`; `conversation.rs:585–592` accepts only a generic subject array. | Define an unambiguous request-resolution contract. If signatures must change, grant ownership of affected callers and schedule the changes serially. Never fall back to contact-wide consent. |
| 9 | T856, T857 | MAJOR | Changing `may_text` immediately can suppress existing web-chat progression before the downstream guards are installed. | `app/intake/src/order_dispatch.rs:371`, `:476`, `:522` return empty replies when `may_text` is false. T856 changes that predicate; T857 owns the later gate changes. | Move the necessary compatibility changes into T856 or defer the predicate change until the callers are migrated. |
| 10 | T854 | MAJOR | Session validation and business-link expiry currently share one timestamp. A blanket substitution of `real_now()` would also move business-link expiry onto real time. | `app/server/src/action_links.rs:324`: `business_principal(tx, jar, now)`; `:329`: `use_action_link(..., now)`. | Carry separate real and business timestamps through `session_check` and its callers. Add an advanced-clock regression for both session validity and link expiry. |
| 11 | T852, T853 | MAJOR | The mandatory positive closing checks are incompatible with variants intentionally stopping before a charge or release. | Both goals require four closing checks with positive evidence first. T852's urgent variants stop at the urgent answer; T853 includes refused quotes and lapsed picks. | Define expected positive evidence and expected zero-side-effect assertions separately for each variant. |
| 12 | T861 | MAJOR | Matching sent messages by recipient and template kind is not directly supported by the carrier fixture's returned structure. Repeated templates could be matched incorrectly. | `tests/e2e/fixtures.ts:16–23` exposes `sid`, `to`, `from`, `body`, `status`, `direction`, but no template kind. `:124–129` supports filters by recipient and key. | Join the outbox entry to its specific carrier message using a verified stable key or SID. Prove repeated templates remain correctly distinguished. |
| 13 | T863 | MAJOR | Sending a code by email is insufficient unless verification also uses the same destination and channel. The goal does not explicitly require persisting the issued challenge's routing decision. | `app/portals/src/pick_flow.rs:1394`: `check_page_code(...)`; `app/picks/src/email_code.rs:163–175` implements a separate email check using the email destination. | Bind verification to the original code's channel and destination. Test a texting-mode or conversation-email change between sending and checking. |
| 14 | T863 | MAJOR | The served page still contains SMS-specific instructions even when the code is delivered by email. | `app/portals/src/pick_flow.rs:1112`: “We texted a 6-digit code to your phone.” `:1255`: “Enter the 6-digit code from our text.” | Explicitly include channel-correct success, retry and validation copy in the goal and acceptance tests. |
| 15 | T857 | MINOR | One referenced module is absent from the supplied package and may be a stale filename. | T857 cites `order.rs:399`; the supplied intake ordering implementation is `app/intake/src/order_dispatch.rs`. | Verify the reference against the repository and regenerate the prompt with the correct location. |
| 16 | T854 | MINOR | The generated test instructions do not identify the three requested advanced-clock regressions individually. | T854's “Run your tests” section says: “This task owns no test file. Run `bash scripts/smoke.sh` and whatever its acceptance line names.” | Name the three inline regression tests and their targeted Cargo commands explicitly. |

## 2. Clean list

The following tasks have no confirmed plan gaps in the portions inspected:

| Task | Scope checked |
|---|---|
| T849 | Clock composition, settling, session sweep and rate-limit pruning |
| T850 | Clock-aware seed options and preservation of existing defaults |
| T222 | Full J1 core journey and dependency structure |
| T851 | Full J2 core journey and separation of Confirm and decline paths |
| T864 | ZIP and coverage dispatch, verified description replay |
| T859 | Photo upload route and session-bound design, apart from the ordering dependency in finding 4 |
| T860 | Card rendering, timer initiation and page delivery |

Additional checks that passed:

- No overlapping owned files were found among the 18 tasks that are unordered relative to one another in the supplied dependency graph.
- The serial consent-chain dependencies are present.
- The new integration and server test filenames match their targeted Cargo test commands.
- The new J1 and J2 Playwright filenames match the existing `late` project configuration.
- The existing `settle` wrapper can be preserved while introducing `settle_at`.
- The actual worker prompts permit module declarations and public API re-exports as shared edits. Those are therefore not ownership blockers.

## 3. Can't tell list

The following require additional repository files or executable verification before approval.

| Area | Required verification |
|---|---|
| Stripe signature freshness | Inspect the actual Stripe webhook verification implementation to confirm that it never receives the advanced business clock. |
| Twilio webhook admission | Inspect `inbound_text.rs` and the voice/status webhook implementations for security-related clock reads. |
| Consent database constraints | Inspect the migration defining `intake.consent_record`. Confirm it allows one web-chat record and one SMS record for the same conversation without weakening append-only protection. |
| Legacy queued SMS | Inspect the sender and outbox suppression paths to establish whether texts queued before T801 could still be delivered after the exemption is deleted. |
| T847 seed contract | The supplied `keyed.ts:137` does not yet expose the planned `seedPick(at)` option. Verify T847's completed implementation before starting T850. |
| Conversation email schema | Verify migration numbering, foreign keys, encryption binding, database permissions and purge coverage. |
| T864 description replay | Verify the complete `keep_description` implementation and its uniqueness constraints. Confirm repeated code submissions cannot duplicate stored words. |
| T801 cutover | Inspect every implementation of `ChatKeys`, all chat-related tests and the live texting predicate's composition. The package does not establish complete caller coverage. |
| Architecture and test design | The full locked architecture and test-design documents were not supplied. Cross-references and omitted requirements cannot be completely verified. |
| Build gates | Cargo manifests, complete migrations, runner scripts and the full workspace are needed to compile and execute acceptance tests. |

**Review disposition: FAIL — plan corrections required.**

Prioritize T856, T857, T858, T862 and the T801 cutover before assigning their workers. The clock chain is substantially better structured, but T854 needs its two-clock handling clarified before T849 changes the served routes.

The counts represent distinct static plan findings, not reproduced compiler or test failures. The partial repository prevents full implementation certification.
