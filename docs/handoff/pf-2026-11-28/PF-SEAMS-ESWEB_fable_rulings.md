# PF-SEAMS + PF-ES-WEB Fable rulings (Nov 28, 2026)

Read against origin/main 820c69ead, the two Opus verifications, PF-CHAIN2 rulings and the web update-choice ruling. Casey's delegation: "Have fable make any decisions". Casey's words on the feature: "Email or phone number and have option for both"; answers (a) to (c) stand. No hard rule is changed by anything below.

Every spec edit below is a `docs/spec` amendment with one lock rewrite and one `docs/decisions.md` entry (bundle them). `sources/` is not touched.

## 1. Step 11a never holds the card (SEAMS #15)

**Ruling: on the regular path the card is shown first, and the 11a question follows it in the same reply. 11a is settled by her next reply, whatever that reply is.** This replaces "before the card" in the earlier ruling and in INT-050.

Reason: before 11a existed, the card rendered in the reply to her address and its 30-minute timer started. A sequential 11a adds a new place where leaving stops the request for good. Treating other replies as SKIP does not fix leaving. The TXT-032 sentence is read from the stored channel when the cards go out, so it never needed the answer before the card.

Exact behaviour:

- **Regular path.** The reply to the stored name and address is: T860's card render (unchanged, timer starts at that render), then `web_updates_ask`. T878 appends the question and writes the once-per-request marker in that same transaction. T860's test (card follows address) stays green unchanged. T878's own test asserts: address, then one reply holding the card and then the question.
- **Emergency path.** Unchanged: the question comes with the reply to the step 11 answer, after the cards.
- **Order of checks on the next reply.** T858's hold lead, opt-out words, the 911 check, the age checks and LIST all run first, as on every page turn. Then 11a. An opt-out word or an age stop closes 11a and sets nothing.
- **Recognised 11a answers** (whole reply, trimmed, any case, trailing punctuation ignored): `1`, `2`, `3`, `SKIP`, or a reply that is only one well-formed email address (counts as 1 with that address, no second ask).
- **Any other reply** closes 11a silently, sets nothing, shows no 11a line and is handled as if 11a had never been asked (so SEND and EDIT act on the card as T860 planned). 11a is never asked again on that request.
- **Leaving** changes nothing: the card was rendered, so T860's timer sends it.
- **1 or 3:** show TXT-018's email question. The email prompt stays open for her next reply. A well-formed address is stored (ruling 6, #3) and the reply is `web_updates_saved`. A SEND or EDIT at the email prompt closes it with nothing stored and acts on the card. Any other reply gets `web_updates_email_bad` once; a second miss gets `web_updates_email_dropped` then `web_updates_skip`, nothing stored, channel not set.
- **2 while live:** `web_updates_text_how`, 11a closed. **2 while not live:** `web_updates_text_unavailable`, and 11a stays open for exactly one more reply (1 goes to the email question; anything else closes it as above).
- **3 (Both):** after the address is stored, `web_updates_saved`, then `web_updates_text_how` while live, else the new key `web_updates_texts_not_yet` ("Texts aren't available yet." / ES draft "Los mensajes de texto aún no están disponibles."). Channel is Both either way.
- **SKIP:** `web_updates_skip`, sets nothing.
- **2 and SKIP** add `web_updates_email_now` when her stored channel is Email or Both and the address is not bounced.
- **After any 11a outcome line**, if the card still waits for SEND or EDIT, the same reply re-shows the current card revision through T860's render path. That never resets or starts a timer.
- **An answer that arrives after the cards went out** (timer fired) is still taken and stored. The cards-out notice already shown keeps its no-email sentence. The first email she gets is then the first quote update, which carries the confirm link.
- Zero homeowner outbox rows on every branch.

Spec: amend INT-050 ("right after the first name and address, in the same reply as the card and after it; settled by her next reply; never holds the card") and the intake order table row 11a. Tests in T878: each branch above, plus SEAMS #16's cases (STOP, cancel and PARAR at the choice prompt and at the email prompt opt out with no channel set; gas gets the page INT-004 words; an under-13 answer purges, including a just-stored update address).

## 2. INT-004 by email on the web path (C1, N2)

**Ruling: no page INT-004 email. Log a spec change.** In the web chat the warning is the on-screen reply, it leads every page reply until SAFE (PF-CHAIN2 ruling 1), and the High call-back alert is raised as planned.

Reason: the 911 check runs on her first message, when a web request almost never has an address. The only address a returning homeowner might have is an update address, which TXT-018 limits to updates. A new safety template and send path would reach almost no one and adds a way for the hold to fail.

Exact behaviour: a page-warned hold writes `warned_by='page'`, zero outbox rows and zero email rows. T279's email for an inbound text on a paused line is unchanged.

Spec: amend TXT-034 from "see the INT-004 text on screen and by email" to "see the INT-004 text on screen in the web chat (an inbound text on a paused line gets it by email, T279)". Add the zero-email assertion to T873's page-hold test.

## 3. SKIP wording (ES-WEB rows 3, 6)

**Ruling: one neutral wording, no conditional variants.**

- `web_updates_ask` EN: "How should we send your quote updates? 1. Email 2. Text 3. Both. Or type SKIP. You can see your quotes on your quote page either way."
- `web_updates_ask` ES (draft): "¿Cómo quiere recibir avisos sobre sus cotizaciones? 1. Correo electrónico 2. Mensaje de texto 3. Ambos. O escriba SKIP. De todos modos, puede ver sus cotizaciones en su página de cotizaciones."
- `web_updates_text_unavailable` EN: "Texts aren't available yet. We can email you instead: type 1 for email, or SKIP to go on." ES as Opus's row 6.

Reason: it is true for every homeowner, needs no state branch and no extra keys. Each sentence is within WORD-024's 15 words.

Spec: amend both quoted texts in INT-050 and T878's goal. All other lines are adopted as in Opus's "Recommended final lines" table (rows 1, 2, 4, 5, 7 to 13 and 9b), Spanish `draft: true`, "avisos" not "novedades", "página de cotizaciones". `web_updates_email_now` keeps its English.

## 4. Web-only pick with no conversation email (ES-WEB)

**Ruling: 11a stays skippable and gets no warning line. The pick step must never dead-end: when the homeowner cannot be texted because texting is off (web-and-email mode or a switched-off line), she has not opted out, and she has no conversation address, the pick page offers PICK-013's "Call me with the code instead" as the way to get her code.**

Reason: Casey asked for an option, not a required email, and D-050 forbids taking a code address on the quote page. The voice code already exists, goes only to her verified phone, only on her own tap, and is not a text.

Exact behaviour:

- Condition: texting unavailable for a reason other than her opt-out, and no conversation address on file. The page shows the call button as the code channel, with the existing PICK-013 limits (10 minutes, 5 tries, cooldown) and PICK-055 caps and hours.
- When the call cannot be placed (cap reached or the 9 p.m. to 8 a.m. rule), the page says so and points to laftly.com/help. It never asks for an email there.
- With a conversation address on file, PICK-029's email code is unchanged. An opted-out homeowner keeps PICK-029 and PICK-030 unchanged (no voice fallback added for her by this ruling).
- No text is sent on any branch.

Spec: amend PICK-029 (the voice fallback for this one case) and TXT-034 (pick code "by email to an address given in their own conversation, or by the PICK-013 call she taps when she gave none"). Placement: the task that owns the PICK-029 page path if one is still open, else one new small task (portals pick page plus `picks` code channel). Acceptance: a TD row where a web-only homeowner who skipped 11a completes a pick with zero texts and zero emails to her.

Not verified by me: whether the pick page already shows the call button when texting is off, and which open task owns that page. The builder checks both before sizing.

## 5. TXT-018 "We never share it with businesses"

**Ruling: keep the English line as it is. Make the spec-only clarification** in TXT-018: replace "never given to a business unless the homeowner chooses email contact at the pick (PICK-018)" with "never given to a business; a business gets only an address she types at the pick (PICK-018)".

Reason: the line is true as built (three separate stores; release reads only the pick-typed address). The lock is being rewritten for this batch anyway, so the clarification is free and removes a reading that would make the homeowner-facing line false. No code task. The reviewer's 16-word replacement is rejected (WORD-024).

## 6. Placements confirmed

- **#11 page card shown state, page EDIT and page SEND:** T860 (it owns `summary_step.rs`). T875 keeps building and storing the card revision without `Shown::Texted`, the refusal check through T865's page variant of `routing::apply`, and the stored-address outcome.
- **#3 bare address at the email step:** a new public entry point in T862 (`keep_conversation_address(tx, conversation, &UpdateAddress)` or equivalent; same seal, audit and purge, no new reader). T878 calls it in the same transaction as T877's store and `set_update_channel`, and drops "T862 captures it". No new task. Tests: a bare address at the 11a choice prompt and at the email prompt is kept; a bare address anywhere else is not.
- **#2 page age stop:** T874 (owns `conversation.rs`). Queues nothing, returns the page outcome, text signature unchanged. T875 calls it.
- **#1 LIST:** drop "a LIST reply" from T858's hold test; add `web_dispatch.rs` to T875's owns for the LIST branch only.
- **#7 page INT-037 question and answer, #5 `hold_911` text-path refusal, #8 inbox-ID dedupe key:** T873.
- **#9 marker wording:** T874, with the two re-ask tests in T875.
- **#10 `source = EXCLUDED.source`:** T877.
- **#4, #12, #13, #14 and V3:** T876 (and T860 for #12). "On file" means `update_status` not bounced with `update_channel` Email or Both; never `read_update_address` outside the send path. Gate on the stored channel, not this request's answer. Queue inside the cards-out transaction. Confirm link only while unconfirmed. Pass the quote-page link directly, never through `route`. TD-TXT-40 reworded to match, in the same D entry.
- **N1:** set `"copy": true` on T875, T876 and T878 and regenerate their prompts.
- **V1 and V2:** T878 adds `web_updates_email_dropped` and `web_updates_texts_not_yet`. T878 then has nine keys; update its acceptance count.
- **Row 13 `web_existing_text_request`:** T801.
- **#6 and #17:** no change (rejected).

## Supersedes

Ruling 1 here replaces design point 1 of `web_update_choice_ruling.md` only as to placement ("before the card") and the two quoted lines changed in ruling 3. Everything else in that ruling and in PF-CHAIN2 stands.
