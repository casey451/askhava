# T801 review verification (PF-T801 round 1, PF-T801b round 2) on main 0f6466a

Verified by reading the code, read-only. No builds or test runs. Status comes from origin/claude/kit-state kit/state.json. Owner columns: A+B, C+D, E+F, H (=T801), I (J1 observation fix).

## Round 1 (PF-T801)
- R1-01 CONFIRMED: `contact_consented` counts consent records from any channel. Evidence: models.rs:348-359; `may_text` is that check (service.rs:890). Owner: A+B.
- R1-02 PARTLY: the gates at order_dispatch.rs:371/476/522 exist. But the web chat never reaches `reply_step` today (see V1), so making the check SMS-only does not break web intake. That intake does not exist on main. Owner: C+D.
- R1-03 CONFIRMED: `card_allowed` counts any consent record. Evidence: service.rs:878-880. Owner: A+B.
- R1-04 CONFIRMED: `Queued`/`Handled` carry outbox IDs (service.rs:66-86); `text_in` builds a `TextIn` from the chat (web_chat.rs:392); `text_turn` returns `Texted` (web_chat.rs:448-475). Owner: C+D.
- R1-05 CONFIRMED: every order_dispatch function line is right: 243, 361 (gate 371), 467 (476), 513 (522), 879, 1327, 1569. Owner: C+D; E+F for `show_card_step`.
- R1-06 CONFIRMED: `queue_text` (conversation.rs:585) and `queue_body` (handlers.rs:379) call `enqueue` with no consent check. Also used at zip_step.rs:253, replies.rs:518, photos.rs:236 and by `ask_name_address` (conversation.rs:420, no gate). Owner: C+D.
- R1-07 CONFIRMED: `open_verified` queues the consent text and the Spanish line (web_chat.rs:764-767), then writes the `web_chat` record (:772) and the `intake.consent_sent` audit row (:775). Owner: H.
- R1-08 CONFIRMED: TEXTED is at chat.rs:445 and OPEN at :592. OPEN itself also says "We just sent you a text." (chat.rs:153-156). Owner: H.
- R1-09 CONFIRMED: no texting-mode check anywhere in chat.rs or web_chat.rs. The predicate exists (see R2-08). Owner: H.
- R1-10 CONFIRMED: the hidden `said` field is built at chat.rs:230-233 and written at request.html:55. Privacy note only. Owner: H.
- R1-11 CONFIRMED: the background gates are job_texts.rs:270/282, quote_updates.rs:106/115, picture_text.rs:339/356, and the line numbers for `send` (:259) and `queue` (:521) are right. The quote-update templates are already covered by QUEUED T840, goal (3). The other job texts are parked by Fable's decision.
- R1-12 CONFIRMED: the D-5585 routing is in messaging/src/service.rs:212-232 (`insert` calls `update_emails::route`).
- R1-13 CONFIRMED (worse than stated): `handle` runs `plan(open.is_some(), …)` (service.rs:154, :182). With a web-opened request, her first real SMS gets neither the consent text nor an `sms` record, because `receive_answer` (:222-249) records only when a consent template was queued. Owner: A+B.
- R1-14 CONFIRMED: app/intake/CLAUDE.md:16 says "One consent record per request, none from a reply" (NOTES never 27). A+B must reword it to allow an appended `sms` record. The file is not locked.
- R1-15 NOT A DEFECT: no unique constraint on `consent_record` anywhere. Only 0053 (:49-62), 0054 and 0196 touch the table; it has only a nonunique homeowner index. No migration needed.
- R1-16 CONFIRMED lines, FLAG: T645 web_chat_stops.rs:502-506, :572-578 and :592-607 assert the SMS. The proposed change contradicts LOCKED docs/test-design.md:867 (TD-CON-36), which says "queues the same INT-038 text the text path queues", "makes the consent text … Spanish" and "the TXT-041 line follows the first reply". Owner: H, with a test-design.md edit, a plan.lock rewrite and a decision.
- R1-17 PARTLY: :46, :319, :325-330 and :368 are right. But :235 is in the fixture `ask()` (:216-235), which asks the address through the SMS API (round 2 is right). TD-INT-43 (test-design.md:875) does not name the transport, so no lock conflict. Owner: H.
- R1-18 CONFIRMED: integration web_chat.rs:360-373, :501-505, :552 (`Texted`) and :631. web_chat.rs:202-211 is the genuine SMS business-intent path and stays as a guard. Owner: H.
- R1-19 CONFIRMED: web_chat.spec.ts:154-161 checks the outbox body and the header at :8-10. Owner: H.
- R1-20 CONFIRMED: chat.rs unit tests at :769, :852 and :879 (OPEN/TEXTED). Owner: H (T299 also owns chat.rs and runs after T801).
- R1-21 CONFIRMED: j1_homeowner.spec.ts `queuedFor` (:87), `say` (:94), `quotePath` (:178) and the consent read (:236). The prompt's ":172" is stale. Bodies are set to NULL on send at messaging/src/models.rs:289. Owner: I.
- R1-22 WRONG: intake_order_corrections.rs:333-360 is a genuine-SMS STOP guard (TD-INT-52) and has no `may_text` invariant.
- R1-23 PARTLY: an unconfirmed address is emailed, with a confirm link (update_emails.rs:194-201). No address, or a bounced one, gives `None`.
- R1-24 CONFIRMED: the card timer holds the outbox ID and waits on `delivery_status`. Evidence: summary_step.rs:186-193, :309-310. Owner: E+F.
- R1-25 CONFIRMED: `first_message` stores only the ID and the language (web_chat.rs:581-597). Owner: C+D.
- R1-26 CONFIRMED: the `/request` a11y state is at page_states.ts:544 (not 446). T802 is DONE, so the file has no open owner. H must own it, add-only.

## Round 2 (PF-T801b)
- R2-01 CONFIRMED: no uniqueness on consent records, so no schema change (same as R1-15).
- R2-02 CONFIRMED that `tell` uses `card_allowed` as text permission (handlers.rs:350-377). On "receive_answer treats a queued consent as SMS proof": PARTLY, a real race. `chat_turn` checks the open request (web_chat.rs:333) before its transaction (:337). If the request closes in between, `receive_text` opens a new request and records `channel='sms'` with the browser message UUID as the inbox ID. Owner: A+B.
- R2-03 PARTLY: confirmed that `TextIn` is shared and that the browser `message_id` becomes `inbox_id` (web_chat.rs:392-406). But the cited lib.rs:599-636 is router merges; webhook dispatch is in inbound_text.rs:738-792. Owner: A+B.
- R2-04 CONFIRMED: `queue_update_email` covers 3 templates (update_emails.rs:43), writes no SMS row (:145-161), and `route` falls back to Text (:129-141). Already in QUEUED T840. Owner: T840, so no G.
- R2-05 CONFIRMED: same as R1-24. Owner: E+F.
- R2-06 PARTLY: confirmed that `photo_text` queues an SMS (photos.rs:217-236). "Portal upload handler absent" is WRONG: app/server/src/homeowner_portal.rs:233 has `PHOTO_UPLOAD_ROUTE`, session-based, owned by QUEUED T820. Owner: E+F, ordered against T820.
- R2-07 CONFIRMED: same as R1-25. Owner: C+D.
- R2-08 PARTLY: "T725 predicate absent" is WRONG; it exists at businesses/src/texting_switch.rs:37, re-exported at api.rs:183 and used at site/src/pages.rs:825. "No mode check in the chat" is CONFIRMED. Owner: H.
- R2-09 CONFIRMED: T645/T649 assert chat SMS, and the corrections STOP guard needs no change. It misses the TD-CON-36 lock (R1-16).
- R2-10 WRONG: T802 is DONE, not IN_PROGRESS. Also T839 (conversation.rs) and T741 (inbound_text.rs) are DONE, not queued owners. The other owner lists are right.
- R2-11 CONFIRMED: T119 depends on T801, so I must not depend on T119. T299, T549, T550, T126 and T127 also depend on T801.
- R2-12 CONFIRMED code, spec open: `contact_consented` is per contact across all requests. So a returning SMS-consented contact's web request is textable today. Whether that is right is a spec question (Q2). Owner: A+B.
- R2-13 CONFIRMED: 911 before consent is pinned (intake_order_area_trade.rs:299). The business reply records no consent (service.rs:254-268). Fable's (a) is consistent.
- R2-14 CONFIRMED: chat.rs:436-445 discards the result and shows TEXTED. Owner: H.
- R2-15 CONFIRMED: same as R1-21. Owner: I.
- R2-16 CONFIRMED: `consent_sent_at` is NOT NULL (0053:42) and set at opening (models.rs:255). Web writes the `intake.consent_sent` audit (web_chat.rs:775). Owner: H (renamed "shown" per Fable's c).
- R2-17 CONFIRMED: `messaging::may_text` is past-customer only (messaging/src/service.rs:124-135); `insert` has no consent check.
- R2-18 NOT A DEFECT: the under-13 test (web_chat_stops.rs:512-558) has no SMS assertion, so no change is needed.
- R2-19 CONFIRMED: the T649 fixture `ask()` uses the SMS API (web_chat_after_code.rs:216-235). Owner: H.
- R2-20 CONFIRMED: the H3 journey belongs to T533 (TD-TXT-15). But T533's deps are only T222 and T536, so the edge to the T801 chain is missing.

## Fable's checks
- F-c PARTLY: the column itself is safe. Nothing reads `intake.conversation.consent_sent_at`: writes only (models.rs:255, order.rs:666, api.rs:264). The `ConversationRow` field (models.rs:289) is never read, and twin, scripts and tests only insert it or age it. The `intake.consent_sent` audit has writers only (service.rs:303, web_chat.rs:775, resubscribe.rs:86). But "consent text was sent" is inferred from "a request is open" at service.rs:154/:182 (`rules::plan(consent_sent)`, rules.rs:93). After H, a web-opened request breaks CON-001 for her first SMS. resubscribe.rs:61 also reads `consent_template` to resend consent on START, which is genuine SMS and fine. Owner: A+B (the plan input must become "an sms record for this request").
- F-e PARTLY: `start_code` (portals/src/pick_flow.rs:787-838) has no consent or `may_text` gate, so a Verify pick code goes to her vaulted typed number. A paused job gets no code (:812-817). `OptedOut`/`NotTextable` give CANT_TEXT with no code (:819-824). Quiet hours (`Hold`) do not block. Behaviour CONFIRMED. Spec caveat: PICK-029 and TXT-038 (messaging.md:42) send the code by email when texting is switched off or suspended, and TXT-034 does the same in web-and-email mode. pick_flow has no mode check, and email_code.rs (T095, D-1612) is not wired. This is an existing parked gap, not T801's; QUEUED T689 owns pick_flow.rs.

## New findings
- V1 CONFIRMED (largest): after the code the web chat runs no INT-043 step past the screen. `text_turn` calls `service::receive_text` (911, SAFE and beep only; service.rs:370-440). `first_text`/`reply_step` run only from `replies::receive_raw_reply` (replies.rs:211-214), and the chat routes there only while the name-and-address question is open (web_chat.rs:338-342). `open_verified` asks no ZIP. TD-INT-30's test (integration web_chat.rs:471) checks only the static `web_chat_order` table. So C+D must build the chat's dispatch through the shared steps, not only swap the transport. T801's goal premise "a chat-run request keeps its steps" is false. Owner: C+D.
- V2 FLAG: TD-CON-36 is a locked row (test-design.md:867). See R1-16.
- V3 constraint: new OPEN copy must keep the substring "got your request" (web_chat_client_ip.rs:35, T646 DONE, untouchable) and "we've got your request" (web_chat.spec.ts:153). Owner: H.

## Changes to Fable's plan
1. C+D grows: it builds web-chat intake dispatch (ZIP after the code, the description replayed, then `reply_step`), which does not exist today (V1). Expect the split Fable allowed, C then D. Re-budget it.
2. H must edit locked docs/test-design.md TD-CON-36, with a plan.lock rewrite and a D-entry (CLAUDE.md rule 8), on top of the T645/T649/T103 transport changes. Add it to H's owned locked file, or do it in the plan PR itself.
3. A+B scope: the input to `rules::plan` changes from `open.is_some()` to an SMS record for this request (F-c); close the chat TOCTOU that can write `sms` with a browser UUID (R2-02); reword app/intake/CLAUDE.md:16 and NOTES never 27.
4. Dependency edges: T533 must depend on H (missing today). T820 (photo route) and T689 (pick_flow) need ordering against E+F. page_states.ts has no open owner, so H takes it add-only.
5. Drop as non-issues: the schema or uniqueness migration (R1-15), changes to intake_order_corrections (R1-22), the under-13 test change (R2-18), waiting on T802 (R2-10), and "T725 not implemented" (R2-08).

## Open questions for Fable
1. F-e: in web-and-email mode, or with texting switched off or suspended, PICK-029/TXT-034 want the pick code by email, but pick_flow sends a Verify SMS whatever the mode. Keep it parked (which decision?), or add it to E+F/H?
2. R2-12: is SMS eligibility per number (the code today, and CON-005's "number texted only after its own first inbound text") or per request (CON-005's "about their own request")? This decides whether a returning texter's web request may be texted.
3. TD-CON-36: rewrite the locked row in the T801 plan PR, or in H under its own decision?
4. V1: may C+D replay the pre-code first description from the vault, or must she retype it (PRIV-008: words are never stored before the code)?
5. Since web intake past consent was never built, should TD-INT-30 (T103, DONE) be marked as only partly proven in the decision?
