# CLK verdicts: PF-T222 and PF-CLK checked against main 0f6466a (Nov 26, 2026)

Read-only check: grep plus line reads, nothing run. Both reviews used an older snapshot, so `app/server/src/lib.rs` lines have moved by about 45 (review 443/545/552/626/630/791/916 are now 482/591/598/672/676/837/962). In the lines below, C = CONFIRMED, P = PARTLY, W = WRONG.

## PF-T222 findings
- #1 C: the web binary builds TestControl but passes only its router (`app/server/src/main.rs:63-75`); `webhooks()` hard-codes `SystemClock` (`lib.rs:962`); the worker uses `control.clock()` (`bin/worker.rs:49-50`); DONE is judged at the webhook instant (`app/server/src/inbound_text.rs:350`, `app/intake/src/check_in_replies.rs:178-184`).
- #2 P: the `SystemClock` sites in `lib.rs` are 476, 482, 591, 598, 672, 676, 837 and 962, which is eight, not seven. It also misses `app/server/src/checkin.rs:19` and `app/server/src/release_sink.rs:492`. Its advice to inject the test clock into `owner_pages`/owner_help/digest (476, 482) and into `RefusalLog` (676) is W: owner sessions and security logs stay on real time (Fable §2), and DONE J3 depends on that (`tests/e2e/j3.ts:6-8,63-64`, D-5282).
- #3 C: the feature build refuses Test/Live (`app/core/src/test_control.rs:163-165`); the release build returns `SystemClock` (`:86-96`). No fallback is needed.
- #4 C: `tests/e2e/keyed.ts:85` seeds a business session with `now()+1 day` on the database clock, plus jobs, picks and licences at `now()`/`current_date` (`:108-152,225`; `tests/e2e/j2.ts:33-36,136`).
- #5 C (scope/budget opinion): it matches Fable's core/variant split.
- #6 C: the webhook builder pins `IntakeSettings::default()` (`lib.rs:969`), and the J1 baseline answers "No photo" (`tests/e2e/j1_homeowner.spec.ts:41,258`).
- #7 C (not re-read in detail): the helpers use fill/click.
- #8 C: pick_flow refuses OptedOut/NotTextable before it sends a code (`app/portals/src/pick_flow.rs:818-824`).
- #9 C (plausible; harness only, not re-traced).
- #11 C: test control serves only clock, advance, messages and perf/db (`app/core/src/test_control.rs:227-231`); `tests/a11y/pick_forms.spec.ts:112` holds the four phrases.
- Clock design step 1 C: 50 test files call `build_router(pool, Router::new(), …)` (e.g. `app/server/tests/action_links_route.rs:161,1267`), so the wrapper constructors must stay.
- Step 2 C: `VoiceHooks` clones the same `Webhooks` Arc (`lib.rs:993`), so voice, call status and SMS move together.
- Step 3 P: the same "inject RefusalLog/owner" error as #2.
- Step 4 C: the omitted routes had to be audited. That is done below.
- Acceptance tests C: the advance → signed inbound → received at ≈ `/__test/clock` test, and the route test with no webhook keys. Add a Stripe test: a webhook signed with real `Date.now` after a multi-day advance is still accepted.
- Coverage ledger C, by spot check: `j1_homeowner.spec.ts` has variants at 309-348, 911 order "is D-2032's gap" (`:328`), closing checks at `:349`, step 2 coverage right after the ZIP (`:243`); T099 drives day-90 and free-first-win (`tests/e2e/j2_business.spec.ts:42,64`).

## PF-CLK blocking findings (section 2)
- 1 C: also `app/server/src/checkin.rs:19`. Line drift: 546→591, 553→598, 627→672, 792→837, 917→962.
- 2 P: Twilio verification ignores `now` (`app/adapters/src/signatures.rs:211`, `twilio(key,&delivery)` has no timestamp check), so moving `Webhooks.clock` cannot break inbound SMS, voice or call-status signatures. The Stripe half is C: the 300 s tolerance shares `h.clock` with dispatch (`app/payments/src/routes.rs:182-183`), and J2 signs with real `Date.now` (`tests/e2e/j2.ts:187`).
- 3 C: `stripe_webhook` uses `SystemClock` (`release_sink.rs:492`) for both the sink (`:121,139`) and Confirm (`app/portals/src/confirm_pay.rs:1151,1465,1546`). The worker's Stripe hooks (`lib.rs:1144,1954-1971`) are also real; that is only the held release, which is safe-mode/J3 and not on the J1/J2 core.
- 4 P: the route constructors are independently `SystemClock` (C), but the session risk is overstated. `BUSINESS_SESSION_LIFE` and `HOMEOWNER_SESSION_LIFE` are 30 days (`app/portals/src/business_auth.rs:106`, `homeowner_portal.rs:56`), and `current_session` compares `expires_at > now` (`app/portals/src/session.rs:203-215`). A session issued on real time and read on the app clock (≥ real) stays valid for offsets under 30 days. The real risks are only:
  - the 1-day seeded session (`keyed.ts:85`);
  - the 30-minute text-Confirm session, issued by action links and read by Confirm (`app/portals/src/action_links.rs:54`). Moving action links and Confirm together fixes it.
- 5 C: per-IP admissions share `flow.clock` (`pick_flow.rs:853,863`; WebSecurity at `:427`); worker prune and sweep run on the app clock (`lib.rs:2964,2969`, review said 2898). Both reviews MISSED:
  - Confirm's own admissions and security cap (`confirm_pay.rs:500,574,999`; WebSecurity `:955`);
  - inbound SEC-014 new-request limit on the dispatch `now` (`inbound_text.rs:805,938-939`).
- 6 P:
  - C: `app/picks/src/settle.rs:114-115` stamps Connected with `Timestamp::now()`. `check_in_times(row.status_at, …)` (`check_in_replies.rs:178`) then makes day 3 due early by the offset left from earlier specs, and status-history times come out of order. This is in scope as Confirm/release time.
  - Not blocking: `settle.rs:164` and `app/intake/src/accepted_quote.rs:97` queue at real time, which is ≤ app now, so they only run early.
  - W: "sent_at starts the Confirm window" (`app/messaging/src/models.rs:289`). The window starts at the worker's `now` passed through `accept_by` (`app/messaging/src/business_notices.rs:455-471` → `lib.rs:2482`).
  - W: `app/quotes/src/intake.rs:692`. The `confirm_quote` wall wrapper has no production caller; quotes go through `submit_quote_with_facts` from `app/server/src/action_links.rs` on `links.clock`.
- 7 C: the floors are at `app/messaging/src/consent_purge.rs:45,101,120,139` and `app/ops/src/safe_mode.rs:67`. P for `app/intake/src/conversation.rs:531,539`: the GREATEST floor is at `:557-558`, and 539 is `insert_answer`.
- 8 C: `tests/e2e/webserver.ts:127` is the shared clock file.
- 9 C: `test_control.rs:355` keeps the known offset, `:375-377` and `:402-403` saturate to real.

## PF-CLK section 3 ledger (sampled)
- Counts (411 lines, 65 A|Y) unverified as totals. 87 X rows not checked.
- 65 A|Y rows: every file:line read on main matched its excerpt for:
  - `app/server/src/business_portal.rs`: 343, 368, 415, 419, 498, 814;
  - `app/server/src/action_links.rs`: 264, 602, 691, 734, 784, 1000, 1058, 1246, 1340, 1418;
  - `app/portals/src/business_auth.rs`: 593, 659, 722, 820, 872;
  - `app/portals/src/feedback.rs`: 258, 372;
  - `app/portals/src/checkin.rs`: 132, 214;
  - `app/portals/src/quote_page.rs`: 549, 566;
  - `app/server/src/business_signup.rs` raw `Timestamp::now()` at 539, 885, 1068, 1568, 1859, 1876, 1909;
  - `pick_flow.rs`, `confirm_pay.rs`, `release_sink.rs`.
- But "Y" (direct path) ≠ must switch. The business_portal, job_page, signin/signup and business_auth rows are on the path yet safe on real time under the 30-day session rule. Fable's narrow scope correctly leaves them.
- 26 real-time/SQL rows spot-checked:
  - C (all security or rate-limit sites on the route's own clock, or DB floors), 21 rows: `business_portal.rs:298`, `app/server/src/cant_take_route.rs:115`, `app/server/src/business_step_up.rs:243`, `app/server/src/webhook_refusals.rs:70`, `app/server/src/chat_codes.rs:269,358`, `app/server/src/owner_signin.rs:131,317,410`, `homeowner_portal.rs:734`, `app/server/src/address_changes.rs:278`, `app/server/src/phone_approval.rs:159`, `safe_mode.rs:67`, `consent_purge.rs:45`, `app/privacy/src/vault.rs:733`, `app/portals/src/security.rs:266`, `app/intake/src/models.rs:819`, `app/businesses/src/links.rs:243,460`, `app/messaging/src/update_emails.rs:244`, `app/jobs/src/job_facts.rs:110`.
  - W (line drift): `app/server/src/job_page_route.rs:160` is HTML; the security site is elsewhere.
  - P: `conversation.rs:539`, see 7 above.
  - P: `app/messaging/src/models.rs:249,289` are real, but they don't feed the Confirm window (see 6).
  - C, moved line: `lib.rs:2898` → 2969.
- Not a ledger row, but every security event and cap shares the route clock (`security.rs:148-161,266`). One shared fix covers all of them (see the plan section).

## PF-CLK section 4 tasks
- CLK-01 C (composition with wrappers). Its "stage activation" is unnecessary if CLK-a below lands first.
- CLK-02 W as a task: no Twilio entry point (`inbound_text`, `inbound_voice`, `voice_status`, `call_status`) checks timestamp freshness.
- CLK-03 C in need. P in method: `StripeWebhook{…}`, `ConfirmRoute{…}` and `Webhooks{…}` struct literals appear in 9, 6 and 10+ DONE test files (e.g. `app/server/tests/confirm_route.rs:215`, `app/server/tests/release_recovery.rs:509`), so a new pub clock field means editing tests the clock task doesn't own.
- CLK-04 P: the per-IP split is needed; the session preflight is not needed for J1/J2 core.
- CLK-05 a-e: wider than Fable's scope. Not needed for J1/J2 core; leave on real time.
- CLK-06: only the `settle.rs:114-115` part is needed. The notice enqueues at `app/picks/src/handlers.rs:111`, `aftermath.rs:222`, `money_notices.rs:88,140` and `pick_withdrawn.rs:57` only run early, so they are harmless.
- CLK-07: 07a W (no caller); 07b not blocking.
- CLK-08: preflight only. sent_at W; the floors stay.
- CLK-09 C. It equals Fable's seeds task.
- CLK-10: fold into the clock task's tests plus the J1/J2 core acceptance.

## Must take the test-control clock (served J1/J2 business time)
1. `main.rs:63-75`: take `control.clock()` once and pass clones to `build_router_with_clock`, `webhooks_with_clock` and `stripe_webhook_with_clock`. Keep the old fns as `SystemClock` wrappers.
2. `lib.rs:962` `Webhooks.clock` covers:
   - inbound SMS dispatch `inbound_text.rs:350` (intake timers, the check-in DONE window `check_in_replies.rs:178-184`);
   - voice `app/server/src/inbound_voice.rs:160`, `app/server/src/voice_status.rs:80,130`, `app/server/src/call_status.rs:143` (shared Arc).
3. `lib.rs:672` pick flow: `pick_flow.rs:742,745,818,947,1392,1441` (send window, 30-day rule, token expiry, code issue/check, share/choose).
4. `lib.rs:837` quote page: `quote_page.rs:549,566`.
5. `lib.rs:591` action links: `app/server/src/action_links.rs:264-1418` (quote submit, HOLD/Away/BUSY, Can't take it link, email Confirm session issue).
6. `lib.rs:598` feedback: `feedback.rs:258,372`.
7. `checkin.rs:19` (merged at `lib.rs:599`): `app/portals/src/checkin.rs:132,214`.
8. `release_sink.rs:492` on web, which covers:
   - `PickReleaser` (`:121,139`);
   - Confirm (`confirm_pay.rs:1151,1465,1546`, `business_principal`/`mine`);
   - Stripe dispatch `now` (`app/payments/src/routes.rs:182,191`).
9. `settle.rs:114-115`: thread the release `at` instead of `Timestamp::now()` (Connected `status_at` feeds day 3).

## Must stay on real time
- Stripe signature freshness: `routes.rs:183` (J2 signs with real `Date.now`, `j2.ts:187`).
- Twilio `verify_webhook` `inbound_text.rs:312`: harmless either way, since Twilio has no timestamp; prefer real time for clarity.
- Security events and caps on switched routes:
  - `security.rs:266` (WebSecurity at `pick_flow.rs:427`, `confirm_pay.rs:955`);
  - `confirm_pay.rs:999`;
  - RefusalLog `lib.rs:676`, which stays `SystemClock`.
- Rate limits on switched routes:
  - `pick_flow.rs:853,863`;
  - `confirm_pay.rs:500,574`;
  - `inbound_text.rs:805/939` (SEC-014; missed by both reviews).
- Owner sessions, TOTP and owner pages:
  - `lib.rs:476,482`;
  - `owner_signin.rs:131`, `app/server/src/owner_portal.rs:319`, `app/server/src/owner_tools.rs:173`, `app/server/src/owner_reports.rs:173`, `app/server/src/owner_ai_routes.rs:115`;
  - `tests/e2e/owner_session.ts:129-135`.
- Database-time floors: `consent_purge.rs:45,101,120,139`, `conversation.rs:557-558`, `safe_mode.rs:67`, `vault.rs:733`.
- Health, liveness, off-db stream, recovery, CLI drills.
- Left real by scope (safe for offsets under 30 days):
  - `business_portal.rs:187`, `job_page_route.rs:125`, `homeowner_portal.rs:221`;
  - `app/server/src/business_signin.rs:124`, `business_signup.rs:417,1516` and its raw `Timestamp::now` calls;
  - `cant_take_route.rs:68`, step-up/guarded/address/phone/settings routes;
  - `app/site/src/chat.rs:410,480,509`.
- Worker prune and sweep `lib.rs:2964,2969` are already on the app clock. They are outside the narrow scope; see Q5.

## Overlap with QUEUED tasks (kit/tasks.json + kit-state)
- `app/server/src/lib.rs`: T718, T719.
- `inbound_text.rs`: T402, T299, T683, T817.
- `confirm_pay.rs`: T132, T746, T811.
- `app/payments/src/routes.rs`: T381, T785.
- `pick_flow.rs`: T689.
- `tests/e2e/keyed.ts`: T847 (D-5809 adds `seedPick(at)` there). The seeds task must depend on T847 or fold into it.
- Composition-only edits need no change to `app/server/src/action_links.rs` (owned by T678, T687, T731, T746, T811).
- Untouched: `main.rs`, `release_sink.rs`, `checkin.rs`, `security.rs`, `settle.rs`, `app/core/src/clock.rs` and `test_control.rs` have no non-DONE owner.
- T222 is BLOCKED; T532 and T533 are QUEUED.

## DONE tests and locked rows a change would contradict
- `tests/e2e/j3.ts:6-8,63-64` (D-5282): owner sessions must stay on the system clock. This rules out PF-T222's owner/RefusalLog injection.
- 50 files using `build_router(…, Router::new(), …)` plus the struct literals above: no signature or pub-field changes, so wrappers or a trait default are needed.
- `tests/e2e/j1_homeowner.spec.ts:183-204` (D-5717): the quiet-hours advance stays consistent. Rerun the T098, T099 and J3 specs unchanged.
- `tests/security/tests/replay.rs:93`: the tolerance unit test is unaffected.
- `docs/test-design.md:14` (time is injected) supports the fix. No locked row pins web `SystemClock`.

## Changes to Fable's plan
1. Split the clock task in two:
   - **CLK-a (real-time split, behaviour-neutral):** add `Clock::wall_now()` with default `self.now()` in `app/core/src/clock.rs`; override it in `OffsetClock` (`test_control.rs`) to return system time. Point the must-stay-real sites above at `clock.wall_now()`. `SystemClock` and `ManualClock` tests keep identical behaviour, and no struct or test edits are needed.
   - **CLK-b (composition):** `main.rs`, `lib.rs`, `checkin.rs`, `release_sink.rs`, and `settle.rs` at-threading, plus one new regression test. CLK-b depends on CLK-a; otherwise J2's Stripe webhook gets 403.
2. Drop PF-CLK CLK-02, CLK-04 (session part), CLK-05, CLK-06 (notices), CLK-07 and CLK-08. Keep only `settle.rs:114-115` from CLK-06.
3. Extend Fable's real-time list with Stripe signature freshness (the only timestamped signature on the path) and the SEC-014 inbound limit and Confirm/pick admissions.
4. Seeds task (= CLK-09) after T847, reusing its `seedPick(at)`. Add an app-clock `expires_at` for the seeded session (`created_at` stays `now()` per the D-5809 CHECK), and app-clock job/pick/licence/fee-terms options. `tests/e2e/j2.ts` belongs to DONE T536, so grant it to the seeds task explicitly.
5. Gate CLK-a/b on deps with the overlapping queued tasks (lib.rs T718/T719, inbound_text ×4, confirm_pay ×3, payments routes ×2, pick_flow T689), so the files aren't edited by two tasks at once.

## Open questions for Fable
1. Approve the `Clock::wall_now` default-method design in core over adding real-clock fields to route state structs (which forces edits to about 25 DONE test files)?
2. Pick and Confirm code per-number daily caps inside `send_page_code`/`admit_code_send` share one `now` with the code TTL. Accept app-time buckets in CI only, or thread wall time into `app/portals/src/limits.rs`?
3. Order against the 11 QUEUED overlapping tasks: clock first (critical path), or after?
4. Should the worker's session sweep and rate-limit prune (`lib.rs:2964,2969`) move to `wall_now` now? This is pre-existing; J3 works around it via D-5282.
5. Should the business portal and job page move to the app clock for J2 core away/month or photo-expiry assertions, or should J2 core avoid asserting them?
