> ChatGPT draft PF-PLAN-T801 (queue job 1), received from Casey Nov 24, 2026 (about 12:30 pm MST). Unverified draft for the Thursday planning session: verify on `main`, assign real IDs (NEW-n here collides with the journeys draft), run the full validator. It splits Fable's five tasks into seven (C/D and E/F apart) for the 600-line and context limits, and proposes parking some non-SMS notices, which needs Fable.

# PF-PLAN-T801 — draft plan change

## 1. Verdict

**BLOCKED for requeue; Fable’s scope is implementable in principle, but the plan is not yet validator-certified or executable against the complete repository.** The existing T801's 700-line budget exceeds the requested 600; the old off-mode behavior directly queues consent (`app/intake/src/web_chat.rs:762–776`), browser UUID becomes SMS-like `TextIn` (`:390–405`), `contact_consented` accepts every channel (`app/intake/src/models.rs:348–359`), and ordered steps refuse without `may_text` (`app/intake/src/order_dispatch.rs:371,476,522`). The real site discards replies and says it will answer by text (`app/site/src/chat.rs:440–445`). The existing T645/T649/T103 assertions demonstrably encode the wrong transport. No migration is demonstrated necessary by `migrations/0053_intake_tables.sql:49–62`. Fable’s decisions a–e are retained. I split C+D and E+F, making seven tasks total (six additions including independent I, plus T801): the former spans reply helpers and a 70 KB dispatcher plus first-message handling, the latter has independently testable upload authorization and timer semantics. Keeping those combined would put each near or above 600 changed lines and risk exceeding the 45% context ceiling. These are estimates, not measured diffs.

**Unresolved implementation boundaries:** the existing web session only stores first message ID/language, not words (`web_chat.rs:579–597`), and the site reposts `said` (`chat.rs:230–234`); the authorized vault continuation is not supplied. The card timer requires an outbox UUID and watches delivery (`summary_step.rs:188–200,254–270,308–322`), so a real presentation hook and compatible timer payload are required; never fake a sent record. The current async reminder uses `card_allowed` instead of SMS evidence (`handlers.rs:350–374`). The complete messaging enqueue/sender, actual server composition, upload/pick portals and full graph are missing; request-scoped zero-row admission and the Verify exception cannot be certified with this archive. These are plan verification gates, not reasons to replace Fable's on-screen path with a handoff. T801 stays BLOCKED pending the full-repo checks below. No live operation, code implementation or runtime test was performed.

## 2. Draft task entries

Placeholders NEW-1…NEW-6 and D-XXXX must be replaced everywhere by overseer-assigned IDs, including branches, prompts, deps and decision fields. JSON is valid; the placeholder values are intentionally not validator-legal. The serial chain is NEW-1 → NEW-2 → NEW-3 → NEW-4 → NEW-5 → T801; NEW-6 is independent of it and a direct T119 prerequisite. Existing T801 prerequisites stay; direct DONE owners are included per archive. New tests only in predecessors except add-only consent_records; atomic activation owns old transport changes. Do not run the new tasks until ownership, stage/phase constraints and full validation are resolved. Budget numbers are planning ceilings, not measured context.

### ADD — full entries

```json
[
  {
    "id": "NEW-1",
    "slug": "web-consent-carrier-boundary",
    "title": "intake/server: distinguish verified web evidence from genuine carrier consent and guard homeowner campaign enqueue",
    "goal": "A+B. First recheck full migration chain (0053_intake_tables.sql:49–62 has append-only rows, two channel checks, no conversation uniqueness), full carrier composition and all enqueue callers. In models.rs:296–359 add SMS-only contact evidence and conversation-specific SMS evidence; keep card_allowed (service.rs:878–880) as any valid record. A returning contact with prior SMS evidence must not authorize an unrelated web-only request. Separate verified web intake authorization from SMS permission; use request/job linkage for campaign sends, not just contact. In service.rs:220–245,285–326 and inbound_text.rs:402–419,527–541,771–842, introduce a trusted, signature-checked, persisted carrier entry, including raw/address, media, START/retry and LIST paths, distinct from browser TextIn (web_chat.rs:390–405,418–419,455–456); a browser UUID never proves carrier origin. Under homeowner lock (models.rs:605), re-read open request and its channel evidence; first genuine ordinary inbound SMS for a verified web-origin request appends exactly one SMS record with real inbox ID, actual rendered version and receipt time, queues exact consent once in the same transaction, then continues current step, not a second job. Preserve safety-first then consent (decision a; INT-002/043, CON-031); STOP/HELP, business-intent and suppression exceptions remain exactly as specified, no manufactured consent on those branches. Verify ordering at carrier delivery as well as queue creation. Decision c: consent_sent_at means consent presented, no migration, channel is provenance; audit web shown not sent, SMS audit unchanged; inventory every real-repo reader before changing behavior. Implement fail-closed request-scoped campaign admission in owned intake helpers and async handlers::tell (350–374), with explicit refusal rather than fabricated Queued IDs; retain genuine first-consent, safety/mandatory keyword and Verify exceptions only as specified. Audit actual messaging enqueue/send boundaries on main: no chat-only outbox row and no release from a previous request or after mode flip; if an unowned cross-crate producer or final enqueue guard is required, assign that file by plan amendment before activation (do not add messaging→intake crate dependency). Existing web compatibility path remains inactive with respect to new admission until T801 atomically activates it; do not expose a new public bypass. Add tests for new predicates/entry now; final activation owns the old test corrections. No migration or invented receipt. Add mixed-channel tests to consent_records.rs, keep existing lines 81–268,294–340 intact (CON-005/007/021/027, TERMS-033); add real signed fake-webhook server test, including replay, concurrent deliveries, returning consented contact, forged browser ID, STOP/HELP, raw/name/address, first MMS, zero unauthorized rows, and 911 before consent. T741 must merge first; inspect actual current code before applying. Plan change D-XXXX (supersedes the transport/test assumptions of D-5279 only; Fable scope 1, Nov 24). Fake data only; no real vendor or number. Preserve existing SMS behavior and security, suppression, SAFE, age/purge, rate-limit, raw-PII and deduplication guards; no test floor reduction or retry to hide failures. Two independent Opus review rounds for consent-sensitive application changes. If a required unowned file or budget overrun is discovered, stop and report a plan gap; do not edit it. Update shared crate notes/architecture facts only as applicable under existing shared-path rules; no locked spec rewrite.",
    "type": "harness",
    "phase": 1,
    "stage": "5",
    "module": "intake",
    "deps": [
      "T100",
      "T594",
      "T656",
      "T181",
      "T075",
      "T646",
      "T103",
      "T645",
      "T649",
      "T057",
      "T489",
      "T517",
      "T609",
      "T595",
      "T593",
      "T343",
      "T644",
      "T763",
      "T745",
      "T676",
      "T671",
      "T725",
      "T831",
      "T741",
      "T038",
      "T039",
      "T040",
      "T043",
      "T048",
      "T049",
      "T050",
      "T156",
      "T208",
      "T251",
      "T054",
      "T370",
      "T747",
      "T241",
      "T432",
      "T570",
      "T572",
      "T277",
      "T482",
      "T673",
      "T775",
      "T728",
      "T151",
      "T581",
      "T600",
      "T256",
      "T435",
      "T257",
      "T279",
      "T317",
      "T325",
      "T336",
      "T357",
      "T391",
      "T449",
      "T407",
      "T480",
      "T573",
      "T675"
    ],
    "model": "claude-opus-5-5",
    "effort": "high",
    "reviewer_model": "claude-opus-5-5",
    "review_areas": [
      "harness",
      "privacy",
      "security",
      "compliance"
    ],
    "owns": [
      "app/intake/src/models.rs",
      "app/intake/src/service.rs",
      "app/intake/src/replies.rs",
      "app/intake/src/handlers.rs",
      "app/server/src/inbound_text.rs",
      "tests/integration/tests/consent_records.rs",
      "app/server/tests/consent_channel_transition.rs"
    ],
    "reads": [
      "docs/spec/consent.md",
      "docs/spec/intake.md",
      "docs/spec/messaging.md",
      "docs/spec/website.md",
      "app/intake/src/web_chat.rs",
      "app/intake/src/conversation.rs",
      "migrations/0053_intake_tables.sql",
      "app/messaging/src/api.rs",
      "app/server/src/lib.rs",
      "docs/code-docs.md"
    ],
    "requirements": [
      "CON-005",
      "CON-007",
      "CON-021",
      "CON-025",
      "CON-027",
      "CON-031",
      "TXT-038"
    ],
    "tests": [],
    "decisions": [
      "D-5279",
      "D-XXXX"
    ],
    "cites": [],
    "budget": {
      "files": 7,
      "lines": 600,
      "context_pct": 42
    },
    "checks": [
      "Ci",
      "Twin",
      "Ally"
    ],
    "branch": "claude/task/NEW-1-web-consent-carrier-boundary",
    "prompt": "kit/prompts/NEW-1.md",
    "acceptance": "Additive primitives and trusted SMS transition tested through real fake carrier route, append-only evidence and exact consent ordering; all old consent and SMS tests pass unchanged before activation. Report full producer/reader inventory and any missing ownership; never mark task complete with a known admission gap.",
    "status": "QUEUED"
  },
  {
    "id": "NEW-2",
    "slug": "intake-screen-reply-primitives",
    "title": "intake: typed on-screen replies without treating them as SMS outbox rows",
    "goal": "C. Introduce explicit server-selected delivery context and a typed rendered reply separate from Queued (service.rs:66–81). Refactor common rendering and question/answer markers, not a second state machine. Implement screen variants in service.rs:308–326, conversation.rs:289–349,402–439,567–585, handlers.rs:379–398,439–472 and replies.rs:137–154,492–518: safety/SAFE, age, HELP/LIST, address, emergency/timing/choice, ordinary text and raw reply processing, without creating SMS outbox records or pretending delivery. Real SMS adapter retains exact render, subject IDs, dedupe and return IDs. Screen path cannot enqueue even when same contact has prior SMS evidence. No user-selected channel or posted provenance. Keep raw addresses in vault only, no PII or bearer links in client-carried said or logs (site/chat.rs:230–234,409–445). Do not flip the actual chat entry yet. Add only intake_reply_delivery.rs tests: screen/SMS state equivalence, exact rendered templates, age close/purge, LIST during address, HELP and STOP, SAFE, raw-vault failure, suppression and no outbox; existing test assertions unchanged (INT-027/038/043, CON-005/021/025, TXT-034). Plan change D-XXXX (supersedes the transport/test assumptions of D-5279 only; Fable scope 1, Nov 24). Fake data only; no real vendor or number. Preserve existing SMS behavior and security, suppression, SAFE, age/purge, rate-limit, raw-PII and deduplication guards; no test floor reduction or retry to hide failures. Two independent Opus review rounds for consent-sensitive application changes. If a required unowned file or budget overrun is discovered, stop and report a plan gap; do not edit it. Update shared crate notes/architecture facts only as applicable under existing shared-path rules; no locked spec rewrite.",
    "type": "harness",
    "phase": 1,
    "stage": "5",
    "module": "intake",
    "deps": [
      "NEW-1",
      "T038",
      "T181",
      "T039",
      "T048",
      "T049",
      "T050",
      "T054",
      "T103",
      "T208",
      "T370",
      "T747",
      "T040",
      "T650",
      "T432",
      "T255",
      "T572",
      "T162",
      "T212",
      "T745",
      "T662",
      "T716",
      "T151",
      "T581",
      "T600",
      "T241",
      "T570",
      "T277",
      "T482",
      "T671",
      "T673",
      "T775",
      "T728"
    ],
    "model": "claude-opus-5-5",
    "effort": "high",
    "reviewer_model": "claude-opus-5-5",
    "review_areas": [
      "harness",
      "privacy",
      "security",
      "compliance"
    ],
    "owns": [
      "app/intake/src/service.rs",
      "app/intake/src/conversation.rs",
      "app/intake/src/handlers.rs",
      "app/intake/src/replies.rs",
      "app/intake/src/reply_delivery.rs",
      "app/intake/src/lib.rs",
      "tests/integration/tests/intake_reply_delivery.rs"
    ],
    "reads": [
      "docs/spec/consent.md",
      "docs/spec/intake.md",
      "docs/spec/messaging.md",
      "docs/spec/website.md",
      "app/intake/src/models.rs",
      "app/intake/src/web_chat.rs",
      "docs/code-docs.md"
    ],
    "requirements": [
      "CON-005",
      "CON-021",
      "CON-025",
      "INT-027",
      "INT-038",
      "INT-043",
      "TXT-034"
    ],
    "tests": [],
    "decisions": [
      "D-5279",
      "D-XXXX"
    ],
    "cites": [],
    "budget": {
      "files": 7,
      "lines": 550,
      "context_pct": 39
    },
    "checks": [
      "Ci",
      "Twin",
      "Ally"
    ],
    "branch": "claude/task/NEW-2-intake-screen-reply-primitives",
    "prompt": "kit/prompts/NEW-2.md",
    "acceptance": "New reply tests pass; real queued result only for genuine SMS; state and rendered reply equivalence; zero campaign rows from web adapter; legacy callers and tests unchanged.",
    "status": "QUEUED"
  },
  {
    "id": "NEW-3",
    "slug": "web-ordered-intake-description",
    "title": "intake: ordered verified web intake with first description preserved once",
    "goal": "D. Implement additive verified web progression through shared intake, not a second flow. order_dispatch.rs:243–326,361–390,467–499,513–538 currently queues first questions and gates progression on may_text. In verified screen context use request authorization plus existing locking/SAFE/age/ownership checks, not SMS permission; SMS keeps may_text and all original guards. Route every downstream turn through NEW-2 typed replies: ZIP (zip_step.rs:122–149,253), trade/candidates, coverage, timing, numbered questions, split/ALSO, raw name/address, LIST and next question. Explicitly account for photo/card hooks completed by NEW-4/5; don't claim full journey yet. web_chat.rs:325–328,468–475,579–597,753–776 loses first words. Before verification preserve the scrubbed first job description using an existing authorized server-owned vault/continuation mechanism, bounded expiry, one-time consumption, session binding; never raw data in session SQL or authoritative client hidden said. If no suitable mechanism exists, stop for an owned vault/schema plan amendment; do not silently ask her to repeat it (Fable requires kept first description). After right code retain first description exactly once and immediately progress to correct first open question; replay/reload idempotent, never reinterpret ZIP/address as original description. Add web_intake_order.rs only: unique first description across retry, two jobs, one open request, identical step ordering, refusal and SAFE before card, no queue rows; keep genuine SMS step/golden/coverage/age guards unchanged (INT-001/027/032/043/045/047, PRIV-008, CON-007/025). Keep existing public path until activation; code may add shared API exports only as needed. Plan change D-XXXX (supersedes the transport/test assumptions of D-5279 only; Fable scope 1, Nov 24). Fake data only; no real vendor or number. Preserve existing SMS behavior and security, suppression, SAFE, age/purge, rate-limit, raw-PII and deduplication guards; no test floor reduction or retry to hide failures. Two independent Opus review rounds for consent-sensitive application changes. If a required unowned file or budget overrun is discovered, stop and report a plan gap; do not edit it. Update shared crate notes/architecture facts only as applicable under existing shared-path rules; no locked spec rewrite.",
    "type": "harness",
    "phase": 1,
    "stage": "5",
    "module": "intake",
    "deps": [
      "NEW-2",
      "T671",
      "T672",
      "T673",
      "T776",
      "T675",
      "T711",
      "T715",
      "T717",
      "T716",
      "T251",
      "T103",
      "T645",
      "T649",
      "T241",
      "T432",
      "T570",
      "T572",
      "T277",
      "T482",
      "T775",
      "T728",
      "T040",
      "T048",
      "T650",
      "T255",
      "T162",
      "T212",
      "T745",
      "T662"
    ],
    "model": "claude-opus-5-5",
    "effort": "high",
    "reviewer_model": "claude-opus-5-5",
    "review_areas": [
      "harness",
      "privacy",
      "security",
      "compliance"
    ],
    "owns": [
      "app/intake/src/order_dispatch.rs",
      "app/intake/src/zip_step.rs",
      "app/intake/src/web_chat.rs",
      "app/intake/src/replies.rs",
      "app/intake/src/conversation.rs",
      "tests/integration/tests/web_intake_order.rs"
    ],
    "reads": [
      "docs/spec/consent.md",
      "docs/spec/intake.md",
      "docs/spec/messaging.md",
      "docs/spec/website.md",
      "app/intake/src/reply_delivery.rs",
      "app/intake/src/models.rs",
      "app/intake/src/service.rs",
      "docs/spec/privacy.md",
      "docs/code-docs.md"
    ],
    "requirements": [
      "INT-001",
      "INT-027",
      "INT-032",
      "INT-043",
      "INT-045",
      "INT-047",
      "PRIV-008",
      "CON-007",
      "CON-025"
    ],
    "tests": [],
    "decisions": [
      "D-5279",
      "D-XXXX"
    ],
    "cites": [],
    "budget": {
      "files": 6,
      "lines": 550,
      "context_pct": 43
    },
    "checks": [
      "Ci",
      "Twin",
      "Ally"
    ],
    "branch": "claude/task/NEW-3-web-ordered-intake-description",
    "prompt": "kit/prompts/NEW-3.md",
    "acceptance": "Additive web intake order and first-description cases pass; no duplicated description/job, PII or SMS row; SMS ordering unchanged; no use of client transcript as proof.",
    "status": "QUEUED"
  },
  {
    "id": "NEW-4",
    "slug": "web-photo-continuation",
    "title": "intake: authorized web photo upload, confirm/remove and continuation",
    "goal": "E. photos.rs:217–236,285–309,358–409,524–565,627–664 and order_dispatch.rs:414–439: screen delivery renders existing authenticated photo upload route/link and question; do not create a text or fake outbox. Retain short lifetime, session/request ownership, exact job, confirm/remove/skip markers and card boundary; after photo flow advances through same order to next question/address. Actual portal route/auth implementation not in archive; verify it can serve existing verified session, and allocate any required portal route or adapter ownership by plan change first. No raw photo token in client-carried said, logs or URL beyond existing authorized route. Genuine carrier picture_text path remains SMS-only; no changes to its assertions. Add web_intake_photos.rs tests only: upload, optional skip, remove/confirm, expired token, cross-job denied, retry and post-card refusal, continuation, zero campaign rows. INT-044/045, CARD-013, PRIV-014, TXT-034. Plan change D-XXXX (supersedes the transport/test assumptions of D-5279 only; Fable scope 1, Nov 24). Fake data only; no real vendor or number. Preserve existing SMS behavior and security, suppression, SAFE, age/purge, rate-limit, raw-PII and deduplication guards; no test floor reduction or retry to hide failures. Two independent Opus review rounds for consent-sensitive application changes. If a required unowned file or budget overrun is discovered, stop and report a plan gap; do not edit it. Update shared crate notes/architecture facts only as applicable under existing shared-path rules; no locked spec rewrite.",
    "type": "harness",
    "phase": 1,
    "stage": "5",
    "module": "intake",
    "deps": [
      "NEW-3",
      "T212",
      "T674",
      "T675",
      "T671",
      "T672",
      "T673",
      "T776",
      "T711",
      "T715",
      "T717",
      "T716"
    ],
    "model": "claude-opus-5-5",
    "effort": "high",
    "reviewer_model": "claude-opus-5-5",
    "review_areas": [
      "harness",
      "privacy",
      "security",
      "compliance"
    ],
    "owns": [
      "app/intake/src/photos.rs",
      "app/intake/src/order_dispatch.rs",
      "tests/integration/tests/web_intake_photos.rs"
    ],
    "reads": [
      "docs/spec/consent.md",
      "docs/spec/intake.md",
      "docs/spec/messaging.md",
      "docs/spec/website.md",
      "docs/spec/privacy.md",
      "docs/spec/cards.md",
      "app/intake/src/reply_delivery.rs",
      "app/intake/src/web_chat.rs",
      "docs/code-docs.md"
    ],
    "requirements": [
      "INT-044",
      "INT-045",
      "CARD-013",
      "PRIV-014",
      "TXT-034"
    ],
    "tests": [],
    "decisions": [
      "D-5279",
      "D-XXXX"
    ],
    "cites": [],
    "budget": {
      "files": 3,
      "lines": 480,
      "context_pct": 30
    },
    "checks": [
      "Ci",
      "Twin",
      "Ally"
    ],
    "branch": "claude/task/NEW-4-web-photo-continuation",
    "prompt": "kit/prompts/NEW-4.md",
    "acceptance": "Authenticated screen photo path advances without SMS; ownership/expiry/confirmation tests pass; all existing SMS photo tests unchanged.",
    "status": "QUEUED"
  },
  {
    "id": "NEW-5",
    "slug": "web-card-presentation-timer",
    "title": "intake: card approval timer starts only at authenticated web presentation",
    "goal": "F. Decision d: summary_step.rs:155–200,254–323,440–449 and order_dispatch.rs:513–538: share card digest/revision, permission and approval state machine; screen renders the actual redacted card, SEND and EDIT on page, not an SMS link surrogate. Separate prepared from presented: only when server successfully renders that revision to its verified session, register presentation once under lock and queue normal wait from that instant; no render, no timer. If rendering fails no presentation marker or timer; no client assertion alone starts timer. Retry/refresh idempotent, no restart of old wait, new EDIT revision invalidates previous timer, SEND immediately approves exactly once; preserve SAFE, emergency, ownership and card_allowed. SMS still waits for real delivery_status, never changes 30-minute rules or existing legacy timer decoding; add backward-compatible web timer evidence instead of fake outbox UUID/delivery. Expose a narrow authenticated presentation hook for T801 to call in actual rendering path. Decision e: after dispatch show cards-out wording and existing authorized quote-page link on page even with no email, never text; use existing quote-link minting (replies.rs:563, order_dispatch.rs:533); no token in posted said; no unauthorized or duplicate link. Preserve email routing as T840 scope, do not implement G. Add web_card_approval.rs tests only: prepared/no render, render/expiry, reload, EDIT/SEND races, old payload, unauthenticated/other session, no consent, no email, zero campaign rows. Existing SMS preview and timer assertions unchanged (INT-041/043, CON-007/025, TXT-034). Plan change D-XXXX (supersedes the transport/test assumptions of D-5279 only; Fable scope 1, Nov 24). Fake data only; no real vendor or number. Preserve existing SMS behavior and security, suppression, SAFE, age/purge, rate-limit, raw-PII and deduplication guards; no test floor reduction or retry to hide failures. Two independent Opus review rounds for consent-sensitive application changes. If a required unowned file or budget overrun is discovered, stop and report a plan gap; do not edit it. Update shared crate notes/architecture facts only as applicable under existing shared-path rules; no locked spec rewrite.",
    "type": "harness",
    "phase": 1,
    "stage": "5",
    "module": "intake",
    "deps": [
      "NEW-4",
      "T208",
      "T600",
      "T671",
      "T672",
      "T673",
      "T776",
      "T675",
      "T711",
      "T715",
      "T717",
      "T716",
      "T241",
      "T432",
      "T570",
      "T572",
      "T277",
      "T482",
      "T775",
      "T728"
    ],
    "model": "claude-opus-5-5",
    "effort": "high",
    "reviewer_model": "claude-opus-5-5",
    "review_areas": [
      "harness",
      "privacy",
      "security",
      "compliance"
    ],
    "owns": [
      "app/intake/src/summary_step.rs",
      "app/intake/src/order_dispatch.rs",
      "app/intake/src/replies.rs",
      "tests/integration/tests/web_card_approval.rs"
    ],
    "reads": [
      "docs/spec/consent.md",
      "docs/spec/intake.md",
      "docs/spec/messaging.md",
      "docs/spec/website.md",
      "app/intake/src/models.rs",
      "app/intake/src/reply_delivery.rs",
      "app/intake/src/web_chat.rs",
      "docs/code-docs.md"
    ],
    "requirements": [
      "INT-041",
      "INT-043",
      "CON-007",
      "CON-025",
      "TXT-034"
    ],
    "tests": [],
    "decisions": [
      "D-5279",
      "D-XXXX"
    ],
    "cites": [],
    "budget": {
      "files": 4,
      "lines": 530,
      "context_pct": 34
    },
    "checks": [
      "Ci",
      "Twin",
      "Ally"
    ],
    "branch": "claude/task/NEW-5-web-card-presentation-timer",
    "prompt": "kit/prompts/NEW-5.md",
    "acceptance": "Presentation starts exactly one revision-bound timer; no render/no timer; SEND/EDIT races and legacy SMS delivery semantics proven; cards-out and quote link accessible without email/SMS in new screen adapter.",
    "status": "QUEUED"
  },
  {
    "id": "NEW-6",
    "slug": "j1-sent-text-observation",
    "title": "e2e: J1 observes cleared outbox bodies without changing SMS assertions",
    "goal": "I, independent of runtime chain and before T119 (D-5716). In tests/e2e/j1_homeowner.spec.ts:9–12,81–101,109,178–180,192–196,233–238,246–267,315 correct comments and observation plumbing. Old body string assumption, direct SQL consent body at 236 and quotePath regex at 179 -> nullable row read; when body NULL after real send, obtain the precisely correlated fake-carrier message from control.sentTexts(), by real recipient, row/template identity or demonstrably unambiguous ordered per-recipient observation; verify actual stand-in contract on main, fail on ambiguity, never select arbitrary latest body. Bounded polling only to observe the matching asynchronous send, not to retry a failed test or resend. Keep row-ID seen set, template sets, counts, exact CON-031 prefix/no reply, coverage/timing/address/card wording, SEND no text, quote token and existing closing/STOP/emergency/duplicate assertions 309–354 unchanged (TXT-011, CON-031, INT-043). Update async quotePath callers, not just old stale line 172. No fake carrier fallback in web_chat.spec.ts:159 (obsolete assertion removed by T801). Test both retained body and sent-cleared paths with fake data; no global worker switch here; T119 owns webserver.ts and later runs full projects with configured number. Plan change D-XXXX (supersedes the transport/test assumptions of D-5279 only; Fable scope 1, Nov 24). Fake data only; no real vendor or number. Preserve existing SMS behavior and security, suppression, SAFE, age/purge, rate-limit, raw-PII and deduplication guards; no test floor reduction or retry to hide failures. Two independent Opus review rounds for consent-sensitive application changes. If a required unowned file or budget overrun is discovered, stop and report a plan gap; do not edit it. Update shared crate notes/architecture facts only as applicable under existing shared-path rules; no locked spec rewrite.",
    "type": "harness",
    "phase": 1,
    "stage": "5",
    "module": null,
    "deps": [
      "T343",
      "T426",
      "T098",
      "T214",
      "T644",
      "T671",
      "T676"
    ],
    "model": "claude-sonnet-5-5",
    "effort": "medium",
    "reviewer_model": "claude-opus-5-5",
    "review_areas": [
      "harness",
      "privacy"
    ],
    "owns": [
      "tests/e2e/j1_homeowner.spec.ts"
    ],
    "reads": [
      "docs/spec/consent.md",
      "docs/spec/intake.md",
      "docs/spec/messaging.md",
      "tests/e2e/fixtures.ts",
      "tests/e2e/keyed.ts",
      "tests/e2e/webserver.ts",
      "stand_ins/twilio.py",
      "app/messaging/src/models.rs"
    ],
    "requirements": [
      "CON-031",
      "TXT-011",
      "INT-043"
    ],
    "tests": [],
    "decisions": [
      "D-5716",
      "D-XXXX"
    ],
    "cites": [],
    "budget": {
      "files": 1,
      "lines": 280,
      "context_pct": 17
    },
    "checks": [
      "Ci",
      "Twin",
      "Ally"
    ],
    "branch": "claude/task/NEW-6-j1-sent-text-observation",
    "prompt": "kit/prompts/NEW-6.md",
    "acceptance": "J1 body and quote-path assertions unchanged and pass with both queued body and cleared body; missing or ambiguous sent text fails; no other test or worker configuration changed.",
    "status": "QUEUED"
  }
]
```

### EDIT — changed fields only

```json
{
  "T801": {
    "title": "intake/site (harness): activate full zero-SMS web intake and live texting handoff",
    "goal": "H1+H2 activation; D-XXXX supersedes D-5279's promise of unchanged transport tests and D-5716's web-chat body fallback. Activate NEW-1–5 together: SMS-only may_text and request-scoped enqueue guard, server-owned verified web reply path, first description, photo and card continuation. No legacy web SMS fallback or synthetic outbox IDs. Decisions a–e apply: (a) emergency 911 first then consent for genuine SMS, STOP/HELP unchanged; (b) use only T725 businesses::api::homeowner_texting_number, not ad-hoc switch logic: while live, GET /request and home prefill/POST show number, exact disclosures, 18+ and “Text {number} from your phone to continue.” (copy panel WORD-028, brand data), no phone/code/Verify/request/job; typed 911/business intent stays on screen. Check live mode server-side on each GET, POST, send-code, check-code and request creation, including flip during Verify; unverified sessions immediately handoff (no successful stale code opens request). A verified open web request keeps screen transport even if live flips; genuine first inbound SMS appends evidence once and resumes same request by SMS; subsequent web turns still screen only. After close new request follows current mode; never trust hidden form. Fail closed on mode/read errors. Verify all real adapter callers on main (server lib and businesses implementation not supplied); if changes needed outside ownership, plan-fix first. (c) web consent version matches displayed disclosure, audit intake.consent_shown, consent_sent_at means presented not SMS proof; no migration. (d) site/chat.rs:435–445,491–515,535–576,590–596 and request.html actual render use typed replies and real presentation hook; only successfully rendered authorized exact card revision starts wait; no client-posted evidence or private tokens/raw PII in said (chat.rs:230–234). SEND/EDIT page path. (e) no email still shows cards-out and quote-page link; no campaign SMS; Verify pick code exception (CON-025/TXT-038) only with opt-out check, verify pick_flow.rs send_window on main; T840 handles async email; park only named missing notices under decision, never fall back to SMS. No H3; T533 owns full web journey. Keep one question, accessible status/error summary, focus and refill, origin/CSRF, no-store, vault and rate limits.\nAuthorized DONE-test changes under D-XXXX, exact source snapshot locations (only listed transport assertions and necessary fixture/API adaptation; everything else unchanged):\n• tests/integration/tests/web_chat.rs:357–373 queued [consent] and outbox body → zero queued/DB campaign rows plus actual screen disclosure equality to web_chat_consent_text; 374–388 exact channel/null inbox/session/version digest unchanged, now digest actual rendered disclosure; add SMS evidence false/card allowed true. 498–505 one first consent SMS → zero rows and first on-screen question; 506–515 exact first UUID and golden step order unchanged; assert preserved distinct description once. 552 Texted → typed successful screen result, 554–556 exact one description unchanged; 631 one queued → zero plus shown disclosure, distinct request/source/reverification remain. 193–219 genuine SMS business signup outbox guard stays. 279–325,400–465 and 598–740 pre-code/rate limit/opt-out/privacy/unique request guards remain (CON-005/007/021/025, TXT-034/038, INT-043, PRIV-008).\n• app/server/tests/web_chat_stops.rs:391 normal message ours empty → exact state-backed screen question/response (not opt-out), no list and open request unchanged; 497–505 exact underage screen words retained, queued.contains → zero campaign rows; 507–509 closure/no card unchanged. 572–578 [consent] and exact outbox Spanish body → empty queue and exact Spanish disclosure on actual rendered page; 592–607 [consent, Spanish-not-yet] and exact outbox bodies → zero queue and exact English consent and Spanish-off line on page in correct order; 579/608 language unchanged. verify helper 181–207 must observe actual rendered verification page for disclosure, not re-render in test. 365–386 STOP confirmation/suppression/time/no additional rows unchanged; 415–480 vault failure/pre-code stops, 541–559 purge unchanged (CON-008/010/025/035, INT-038, PRIV-007, TXT-002/041).\n• app/server/tests/web_chat_after_code.rs:217–235 fixture asks address by SMS → actual screen question, 319 TEXTED → exact screen next response or explicit no-new-question as dictated by current state, 325–330 one queued address question → one state-backed presentation and zero SMS, 368 TEXTED → exact on-screen LIST answer; 288–317,320–324,331–336,369–375,381–402 vault fields, PII scrub, no second answer, LIST keeps question open, age deletion unchanged (INT-027/038, TXT-031/034, PRIV-005/008).\n• tests/e2e/web_chat.spec.ts:8–10 false no-worker comment → worker may run; 131 test title and 154 OPEN notice preserve successful verified indication plus exact first question; 157–161 one consent outbox and body==page → zero rows AND zero carrier campaign sends for this number since start, actual fixed WEB-014 disclosure equals approved exact text and consent version; never use sent fallback for forbidden SMS. 143/151/179 wrong/no code zero records/rows, 155 web_chat,166 one open record,111–128 disclosures/age/AI/no agree and 169–220 CSRF/911 remain. Add isolated live/off mode and stale code tests; assert live no number-entry/code/request even via crafted POST, verified flip continuity, no email, fake Verify separate from carrier (CON-025, TXT-034/038, SEC-042, WEB-014).\n• app/server/tests/web_chat_route.rs:169–221 and home_request_box.rs:201–248,262–274,366–373,424–443 existing off-mode, prefill, origin/token, no-store, escaped input and no PII log guards unchanged; add live and flip variants, explicitly set off mode for old tests if necessary. tests/a11y/page_states.ts:544,965 add separate live state using grounded fixture; keep existing off-mode state and other fixtures, no global mode weakening (SEC-042, WEB-027, A11Y-001). Embedded site/chat.rs:764–771 successful code may now return richer screen response but preserve successful notice, wrong/refused/max tries; 851–879 progress status/refill TEXTED fixture → actual screen progress notice if old constant removed; keep same DOM/focus/refill assertions (A11Y-035). 722–758 fixed consent/AI/adapter/missing code unchanged. Existing T646 client-IP, T656 error summary, home_first_screen, SMS consent_records, genuine SMS orders, STOP/HELP/911 and brand tests unchanged. Run all web-chat callers and all original T801 checks; report any additional conflicting test with exact file/line as plan gap, don't edit without logged authority. Two Opus review rounds. Fake only; zero DB campaign rows is mandatory even without configured sender, and zero fake-carrier campaign sends with worker enabled. Plan change D-XXXX (supersedes the transport/test assumptions of D-5279 only; Fable scope 1, Nov 24). Fake data only; no real vendor or number. Preserve existing SMS behavior and security, suppression, SAFE, age/purge, rate-limit, raw-PII and deduplication guards; no test floor reduction or retry to hide failures. Two independent Opus review rounds for consent-sensitive application changes. If a required unowned file or budget overrun is discovered, stop and report a plan gap; do not edit it. Update shared crate notes/architecture facts only as applicable under existing shared-path rules; no locked spec rewrite.",
    "deps": [
      "T100",
      "T594",
      "T656",
      "T181",
      "T075",
      "T646",
      "T103",
      "T645",
      "T649",
      "T057",
      "T489",
      "T517",
      "T609",
      "T595",
      "T593",
      "T343",
      "T644",
      "T763",
      "T745",
      "T676",
      "T671",
      "T725",
      "T831",
      "NEW-5",
      "T802",
      "T038",
      "T039",
      "T040",
      "T043",
      "T048",
      "T049",
      "T050",
      "T156",
      "T208",
      "T251",
      "T054",
      "T370",
      "T747",
      "T267"
    ],
    "owns": [
      "app/intake/src/web_chat.rs",
      "app/intake/src/models.rs",
      "app/intake/src/service.rs",
      "app/site/src/chat.rs",
      "app/site/templates/request.html",
      "tests/integration/tests/web_chat.rs",
      "app/server/tests/web_chat_route.rs",
      "tests/e2e/web_chat.spec.ts",
      "app/server/tests/home_request_box.rs",
      "app/server/tests/web_chat_stops.rs",
      "app/server/tests/web_chat_after_code.rs",
      "tests/a11y/page_states.ts"
    ],
    "reads": [
      "docs/spec/consent.md",
      "docs/spec/intake.md",
      "docs/spec/messaging.md",
      "docs/spec/website.md",
      "docs/architecture.md",
      "docs/code-docs.md",
      "docs/spec/privacy.md",
      "app/intake/src/reply_delivery.rs",
      "app/intake/src/summary_step.rs",
      "app/intake/src/photos.rs",
      "app/server/src/lib.rs",
      "app/businesses/src/texting_switch.rs",
      "app/portals/src/pick_flow.rs",
      "tests/e2e/fixtures.ts"
    ],
    "requirements": [
      "CON-005",
      "CON-007",
      "CON-025",
      "TXT-021",
      "TXT-034",
      "TXT-038",
      "CON-021",
      "INT-041",
      "INT-043",
      "WEB-009"
    ],
    "decisions": [
      "D-5279",
      "D-470",
      "D-5716",
      "D-XXXX"
    ],
    "budget": {
      "files": 12,
      "lines": 600,
      "context_pct": 45
    },
    "acceptance": "All owned and affected guards pass, T645/T649/T103 transport-only corrections exactly as listed, zero campaign outbox rows and fake sends with worker enabled, valid off-mode full on-screen intake, photo/card/SEND/EDIT, no-email quote link, live handoff plus stale code refusal and verified grandfathering, genuine first SMS once with correct evidence and ordering; T840/T533 carry their own scopes. Ally and two Opus reviews pass. Requeue only after full graph, ownership, real budget and decision validated."
  },
  "T119": {
    "deps": [
      "T103",
      "T106",
      "T118",
      "T307",
      "T489",
      "T497",
      "T536",
      "T561",
      "T841",
      "T801",
      "NEW-6"
    ],
    "goal": "Journey J4 (Twilio failing and suspended, Stripe unreachable, the AI down, then recovery) runs end to end in Playwright, keyboard-only too, ending with the closing checks and a clean nightly reconcile (A11Y-021). Plan change D-3744: J4's quote link that \"appears in the portal\" (TXT-014) is the homeowner portal T307 serves, and \"Casey's call list fills\" (TXT-023) is read on the owner pages T310 serves, which this task reaches through T118 (T118 depends on T303 and T310), so it depends on T118 and T307 (D-3263 assumed T119 followed T118; until now it did not). It signs the owner in only through T303's shared `tests/e2e/owner_session.ts` helper, never running `bootstrap-owner` itself (D-3744). Its Resend email checks wait for the email-sender wiring task, as T118's do (D-3263). Plan change D-3844 (the overseer's pre-claim scout, Nov 15, 2026): J4's vendor failures come from T497's sticky stand-in faults (`control.setFaults`, cleared after every test), never from changing the app's settings. The app does not detect a suspension from a send refusal (Casey records it, `record_suspension`, `app/messaging/src/mode_state.rs:156`, D-3844), so J4 step 2 also has the owner, signed in through `owner_session.ts`, record the suspension on T108's owner home control, and step 5 records the restore and switches texting back on. The faults: Twilio sends failing and refused as from a suspended number (Twilio error 21606), Stripe unavailable or dropping the connection, the AI answering 529. No process builds an AI client before T299 (stage 6; `app/server/src/main.rs` and `bin/worker.rs` build none), and a phase-1 task cannot depend on it, so in this stage step 4 sets the AI fault and checks INT-021's AI-off path as the spec now words it (each step at once on the fixed question bank, no \"we have your message\" text and no 30-minute retry, D-492; `docs/test-design.md`'s J4 step 4 corrected to match) and the safety text for \"sparks\" within 5 minutes (INT-003); once T299 builds the client the same spec exercises the real outage, and T299's acceptance requires it to pass unchanged. Plan change D-3889 (Fable's pre-claim check, Nov 16, 2026): steps 1 and 3 are worker tasks, so this task depends on T536, which starts the suite's worker with the keyed values (Stripe, Twilio and vault, and T490's Resend values) and runs J3 and J4 after every other spec. J4 seeds from the app clock, never the database's `now()`. Shared steps live in `tests/e2e/j4.ts`. `control.setFaults` is on the e2e `test` only (T497), so `j4_keyboard.spec.ts` uses it with `test.use({ controlSecret: controlSecret() })`. An `afterAll` records the restore and switches texting back on, so a failure midway never leaves the shared database suspended. Plan change D-3893: this task's specs run in T536's late Playwright project, so a single-file run also runs the whole first project; add `--no-deps` while iterating on one spec, and the commands under \"Run your tests\" stay the hand-in check. Plan change D-5657 (pre-flight, checked on main 98e09a35; TD-TXT-15, TXT-034, TXT-021): TD-TXT-15's J4 half: steps 1 and 2 run By text, since the retries need attempted sends; after step 2 the owner sets the launch channel to web-and-email through `tests/e2e/owner_session.ts`; steps 3 to 5 and the closing checks assert that no outbound text row was created (queued) after the switch (texts queued in steps 1 and 2 may still go out when step 5 restores the number); the `afterAll` restores the launch channel and the homeowner-texting values it read at the start, not hard-set ones, as D-5285 did for J3. Plan change D-5716 (attempt 1's confirmed plan gap, verified on `main` 15a972ec): `keyedEnv` (`tests/e2e/webserver.ts:143` to `:170`) sets no homeowner number, so `homeowner_number()` (`app/server/src/lib.rs:2345`) is `None` in CI, the worker builds no homeowner sender (`app/server/src/lib.rs:2267` to `:2276`) and J4 steps 1 and 2 cannot run By text (D-5657). This task is now a harness task (Sonnet worker, Opus review kept) and owns `tests/e2e/webserver.ts` (every other owner is DONE) and adds only `ASKHAVA_HOMEOWNER_NUMBER: \"+15005550006\"` to `keyedEnv`, the stand-in's only accepted From (`stand_ins/twilio.py:48`) and the To J1 already uses, with a doc comment saying why. Step 1 sets the `twilio` `suspended` fault (a 4xx 21606 refusal, which `refused()` in `app/messaging/src/service.rs:335` to `:342` treats as not sent, so TXT-014's 3 retries run, `app/messaging/src/outbox_sender.rs:549` to `:569`); the `fail` fault's 500 is an unknown outcome that is never resent (TXT-011, D-558; `outbox_sender.rs:634` to `:674`), so it cannot show the retries. `stand_ins/` is unchanged. Sends now follow the quiet hours (8 a.m. to 8 p.m. in the stricter of Arizona and area-code time, by the app clock), so step 1 moves the app clock into sending hours before it sets the `suspended` fault, and its retry count never depends on when CI runs. Homeowner texts in every e2e spec now really go out: T841 (J3's zero-text count re-taken after the switch to web-and-email), NEW-6 (J1 body observation, including quote links) and T801 (web chat zero outbox rows and zero campaign sends; no fallback for forbidden text) land first for that (D-5716, D-XXXX). Run the full e2e `main` and late projects and `Ally` with the new line; if a spec this task does not own still goes red because texts now go out, report a plan gap and never edit that spec.",
    "decisions": [
      "D-1143",
      "D-3744",
      "D-3844",
      "D-3889",
      "D-3893",
      "D-5553",
      "D-5657",
      "D-5716",
      "D-XXXX"
    ]
  },
  "T533": {
    "deps": [
      "T222",
      "T536",
      "T801",
      "T840"
    ],
    "goal": "Plan change D-3889 (split from T222, Fable's pre-claim check, Nov 16, 2026; D-3263, D-3744): the web-and-email runs of J1 and J2 go in `tests/e2e/j1_j2_web_email.spec.ts`: signed in as the owner only through T303's shared `tests/e2e/owner_session.ts`, the spec sets the launch channel to web-and-email on the served owner home (T108, served by T310; `messaging::api::set_launch_channel`, `app/messaging/src/mode_state.rs:216`), then runs J1 and J2 with web-chat consent and intake, the safety text on screen, the pick code by Verify text (a Verify code is not a carrier message; read with the stand-in's `GET /_stand_in/twilio/code`, `stand_ins/server.py:446`; the code by email, PICK-029, is parked by D-3889's review round), no call-window or who-lets-in question anywhere (D-440), a portal quote, Confirm, HOLD, Away, PAUSE and BUSY in the business portal, and the stand-in carrier recording no new text from the journey's start to its end (its count before and after, and none to the journey's homeowner or business phones; earlier specs' texts stay in the log); J2's sign-up takes its identity code by Verify text to the record phone (BIZ-002's first step; a Verify code is not a carrier message, D-313, TXT-037; `tests/e2e/j2.ts:94`), then verifies its notice email by the emailed code; a mode-aware email-first identity code (TXT-034's \"by email or letter\") is parked post-freeze (TD-TXT-15's 'identity code by email or letter' clause, `docs/test-design.md:408`, is parked with it, D-5551); the fake voice provider records no call to a business phone (`tests/e2e/j2.ts:192`, `callsTo`) (TD-TXT-15, BIZ-046, BIZ-002). Web-and-email also switches homeowner texting off and switching back to SMS leaves it off (`app/messaging/src/mode_state.rs:222` to `:228`), and the outbox sender sends no homeowner text while it is off, so an `afterAll` switches the launch channel back to SMS and homeowner texting back on (`messaging::api::switch_on`, `mode_state.rs:126`, through the same owner page) and asserts both. TD-TXT-15's Resend email checks stay parked (D-3750). Fake data only. Update `docs/decisions.md`. Plan change D-3892 (Opus pre-claim check, Nov 16, 2026): without the keyed worker nothing is ever sent, so the no-text check would prove nothing; this task depends on T536, which starts the worker with the keyed values and runs `j1_j2_web_email*` in the e2e late project, after `j1_full` has moved the clock by days. Every seed takes the app clock (`GET /__test/clock`), never the database's `now()` (`tests/e2e/keyed.ts:85` uses `now()`). Shared steps are imported from T222's helper modules `tests/e2e/j1_full.ts` and `tests/e2e/j2_full.ts`, `tests/e2e/j2.ts` and `tests/e2e/keyed.ts`, never from a `.spec.ts` and never copied, within 500 lines; iterate with `--no-deps`. Plan change D-4258 (closed scan lists, Nov 17, 2026): \"no call-window or who-lets-in question anywhere\" is now a closed check, one definition for worker and reviewer. The phrases are exactly the four of `tests/a11y/pick_forms.spec.ts:112` (\"What time\", \"call window\", \"Who will let\", \"lockbox\"), matched as a case-insensitive substring (stricter than that spec's case-sensitive `not.toContainText`, so a capitalised \"Call window:\" heading is caught). Playwright refuses importing a `.spec.ts` and this task does not own that file, so the list is copied exactly into this spec, and a test here reads `tests/a11y/pick_forms.spec.ts`, takes the array on the line holding `for (const asked of [`, and asserts it equals the copy. \"Anywhere\" is closed as: the `main` element's text of every page each journey loads (each navigation and each form answer) and the web chat's transcript after each homeowner turn. Must fail (planted, a page set to each text): \"What time works for a call?\", \"Your call window\", \"Who will let the business in?\", \"Is there a lockbox?\", \"Call window: 9 a.m. to noon\". Must pass: \"They never see your number.\" and each journey page as served. Passing the closed scans is the wording bar; an off-list phrasing that no file in scope contains is a note under 'Scan list notes (not findings)', not a finding (D-4038). Plan change D-5664 (outside pre-flight): TD-TXT-15's pick-code-by-email (D-3889 review round (a)), identity-code-by-email-or-letter (D-5551) and Resend email clauses (D-3750) are out of scope; the reviewer checks only the remaining clauses. Plan change D-XXXX: this task is the full zero-text web-and-email journey (no separate H3). Drive verified on-screen intake, photo/card/SEND, quote page and no-email path with real fake worker; assert zero campaign outbox rows and zero carrier campaign sends, distinguish Verify; preserve closing checks and existing spec clauses. J1/J2 core dependencies from the separate clock plan are added by overseer; no SQL-seeded transition claimed as a journey.",
    "decisions": [
      "D-3889",
      "D-3892",
      "D-4258",
      "D-5551",
      "D-5664",
      "D-XXXX"
    ]
  }
}
```

## 3. Draft docs/decisions.md entry

- **D-XXXX** Question (plan change, November 24, 2026; number reserved by overseer; Fable's decision for Casey under D-305, `docs/handoff/2026-11-24-fable-scope-T801-T222.md` §1; D-5279, D-5716; CON-005/007/021/025, TXT-034/038, INT-041/043, WEB-009; IDs unchanged): T801's blocked plan assumes the web fallback can still use the SMS queue and that completed T645/T649/T103 tests stay unchanged, but the channel evidence is not SMS consent, the shared intake guards block progression and its replies currently queue SMS. Choice: keep the one SMS opt-in path and all substantive specs; make the off-mode verified web chat a full on-screen request with no campaign outbox rows or sends, using shared intake state and a typed screen reply; append genuine SMS consent once on the first qualifying carrier inbound and continue that request. Safety 911 before consent, STOP/HELP unchanged (a); live mid-chat unverified sessions immediately handoff and verified open web requests finish on screen unless the person texts in (b); consent_sent_at means consent presented, channel proves provenance and web audit is shown not sent, no migration (c); exact card revision starts its wait only at successful authenticated server rendering, SEND/EDIT on page (d); no email means cards-out and quote-page link on page, no campaign text, Verify pick code only with opt-out check (e). Split A+B, C, D, E, F, T801 H1+H2, and independent I (NEW-6, before T119); no G, T840 owns asynchronous email and T533 owns zero-text journey, no H3. The exact old-to-new assertions and guards in T801's goal in this plan are the only authorization to change DONE transport tests (D-5386 pattern): tests/integration/tests/web_chat.rs:357–373,498–505,552,631; app/server/tests/web_chat_stops.rs:391,502–505,572–578,592–607; app/server/tests/web_chat_after_code.rs:217–235,319,325–330,368; tests/e2e/web_chat.spec.ts:8–10,131,154,157–161; site/chat.rs:764–771,851–879 fixture only as necessary; route/home/a11y add-only plus explicit off-mode setup. Preserve exact disclosure/version, state, vault/age/purge/STOP, origin and genuine SMS guards. D-5716's web consent fallback is superseded by zero text; all J1 body reads remain exact, use correlated sent observation when NULL under NEW-6. Pending full-repo confirmation, park only non-SMS delivery of `intake` no-reply reminder/closed notice (INT-019, `handlers.rs:287–291,350–374`), and the proactive text-only `check_in_more`, `feedback_request`, `whos_calling`, `whos_calling_email`, `check_in_day3`, `check_in_day14`, `no_quotes_yet`, `emailed_question`, `closed_no_quotes`, `closed_no_pick`, `closed_no_quote_left`, `unreachable_closed` listed by `app/messaging/src/update_emails.rs:49–72` if their equivalent authorized web/email surfaces are missing. This is a draft explicit deferral of notice transport only, not removal of job-state transitions, mandatory journey steps or quote access; request_out/request_out_held/request_out_today/request_out_slow must have on-screen cards-out/link as applicable; first_quote/quote_update/all_quotes_in stay T840. The overseer must verify each item actually lacks a path, identify its spec/owner and obtain/record approval before parking; any mandatory non-SMS step required by TXT-034 remains a blocker, not a silently waived requirement. No live vendor use, no retries to conceal red tests. Kind: plan. Source: `docs/handoff/2026-11-24-fable-scope-T801-T222.md:7` to `:20`; `docs/spec/consent.md:9,11,25,29`; `docs/spec/intake.md:45,60–72`; `docs/spec/messaging.md:38`; `docs/spec/website.md:13`; `app/intake/src/models.rs:296–359`; `app/intake/src/web_chat.rs:390–405,579–597,730–776`; `app/intake/src/order_dispatch.rs:371,476,522`; `app/intake/src/summary_step.rs:155–200,254–322`; `app/site/src/chat.rs:440–445`; `tests/e2e/j1_homeowner.spec.ts:81–99,178–180,236–267`.

## 4. Open questions / mandatory overseer checks on main

### 4.1 Ownership and exact dependency edits not possible from this excerpt

Only T299's complete entry among colliding queued owners is supplied. It already depends directly on T801, so it needs no dependency edit for these shared files. T299 remains in phase 2/stage 6. The following are **append candidates to existing `deps` arrays**, not replacement arrays: the archive supplies statuses/owners, not the current `deps` for these tasks. Preserve every existing dependency; if a listed owner is already an ancestor, retain that order instead and re-read code; if the candidate would form a cycle or violate phase/stage, stop and resolve ordering on the full graph. Do not write fabricated complete deps arrays into the second JSON object. T741 is MERGE_PENDING, explicitly wait in NEW-1 and verify it is DONE first. T802 is DONE in the current package (not IN_PROGRESS as the earlier review says); direct dependency in T801. Shared paths excluded by validator still need coordination.

| Existing task | Append if not already transitively ordered | Shared files (short names) |
|---|---|---|
| T236 | T801 | service.rs |
| T299 | T801 (already T801-dependent) | inbound_text.rs, order_dispatch.rs, web_chat.rs, chat.rs |
| T394 | NEW-5 | summary_step.rs |
| T402 | NEW-1 | inbound_text.rs |
| T680 | NEW-3 | conversation.rs |
| T683 | NEW-1 | inbound_text.rs |
| T704 | T801 | service.rs, handlers.rs, conversation.rs |
| T718 | NEW-5 | order_dispatch.rs |
| T719 | NEW-5 | order_dispatch.rs |
| T723 | NEW-5 | order_dispatch.rs |
| T736 | NEW-5 | order_dispatch.rs |
| T742 | NEW-2 | handlers.rs |
| T760 | NEW-5 | order_dispatch.rs |
| T817 | NEW-1 | inbound_text.rs |
| T818 | NEW-3 | conversation.rs |
| T823 | NEW-5 | replies.rs, order_dispatch.rs |
| T825 | T801 | service.rs, order_dispatch.rs |
| T839 | NEW-3 | conversation.rs |

Important: T533's new direct T801/T840 deps are concrete draft edits based on its supplied entry, but its full core-J1/J2 dependencies are owned by the separate clock/journeys plan. T119 continues to depend on T801 and now NEW-6; no reverse edge. Check all downstream gates, T120, audits, T126, and finished/Gate C transitive coverage. Verify the proposed new test paths are unowned and discover every owner of missing runtime paths (including potential privacy-vault, messaging enqueue/sender, portal upload/pick flow, site route composition, copy docs and shared adapters). If H activation's actual diff exceeds 600 lines or generated context exceeds 45%, split live handoff into additive helper/UI work with no production flip; keep actual web transport switch + old DONE test corrections atomic in T801. Do not suppress tests or let intermediate code create a new unguarded web route.

### 4.2 Full-repo verification checklist

1. Run actual `scripts/validate_tasks.py` on the full graph after assigning real IDs, logging the decision, resolving every owner, checking dependencies and generating prompts/plan lock. Verify every named spec's path; the archive only has consent/intake/messaging/website and not the privacy/card specs or test design. Confirm path names such as `docs/spec/privacy.md`, `docs/spec/cards.md`, `app/portals/src/pick_flow.rs`, `stand_ins/twilio.py`, `app/server/src/lib.rs` before committing. Confirm no new crate cycle. Measure context with `scripts/gen_prompts.py`, do not merely trust estimates. Keep T801 status BLOCKED until plan-fix approved.
2. Confirm migrations and all consent timestamp readers, append privileges, real inbox provenance, producer/request linkage and opt-out/mode checks. Check every enqueue caller including quote_updates/job_texts, background, pick/check-ins, portal and carrier compositions; central guard must also protect old web-origin rows from later mode change/previous contact SMS evidence. Check how `may_text` is used by T840 (email-only fallback), no regression or dependency cycle. If absent implementation files require changes, allocate exact path and all owner edges before tasks claim; no unowned edits. The archive includes `update_emails.rs:155` `queue_update_email`, but not all consumers; T840 still owns async handling.
3. Confirm server-owned first-description continuation and privacy threat model, session authorization and expiry, rate-limit and mode-race transaction behavior, all `ChatKeys` implementations and API exports, real card rendering success semantics and backward-compatible timer data, existing signed upload and quote links. If existing data structures cannot represent these without additional files/migration, amend plan rather than fabricate values. Confirm pick Verify `send_window`/opt-out and no email branch; no live SMS/Verify or email in tests.
4. Confirm T645 test helper `verify` directly calls intake (`web_chat_stops.rs:181–207`), so change it to exercise actual screen before asserting Spanish disclosure; ensure state assertion at :391 covers actual ordinary response rather than requiring empty transcript. Check T649 fixture :217–235 for direct SMS ask. Search all other tests using `Checked::Verified`, `ChatAnswer::Texted`, `ChatAnswer::Replied`, `/request`, `may_text`, `consent_sent_at`, and outbox. If another DONE assertion conflicts, document its exact old/new and requirement in the decision before ownership reassignment. Do not change genuine SMS tests, consent purge, T646 client IP, T656 error summary, existing A11Y, code/terms/brand or safety assertions.
5. Confirm exact fake carrier correlation metadata and behavior, worker enabled vs disabled, J1's every body consumer and quote token renewal. Run targeted tests, all affected crates, full e2e projects with fake sender enabled and disabled as appropriate, `Ci`, `Twin`, `Ally`, and two Opus review rounds. Ensure zero campaign rows AND zero carrier sends on chat-only branch, even for a previously SMS-consented contact and after mode flip. T533 runs full zero-text journey and closing checks; no extra H3 or G. Confirm specific parked non-SMS notices and their actual spec IDs/owners with Fable; if TXT-034 mandatory completion requires one, keep blocker open rather than waive it.

### 4.3 Validation performed in this review

Parsed both JSON payloads, checked known dependency IDs and copied direct DONE owners. Imported supplied `validate_tasks.py` with a minimal empty `trace_spec` import shim (sibling module missing); ran its **per-task `check_task` only** after temporary substitution of unused T-shaped IDs and a D-shaped number, on all six additions plus edited T801/T119/T533. Result: no per-task errors. This is not the full validator: graph ancestry/cycles, protected/test ownership against missing entries, spec/decision references, gate/phase order, coverage and measured context cannot be certified. No changes were applied to `kit/tasks.json`; no Rust/Playwright execution.
