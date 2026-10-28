# Ask Hava: how the finished business works (storyboard)

**Purpose:** one shared picture of the finished service, from three seats (homeowner, business, owner), before anything is built. Content only; slides are built from this.

**Source:** `casey451/askhava`, main at commit `823c40c` (PR #37, "consent by texting in, terms at the pick, updates question", D-419 to D-428, merged Oct 28, 2026 18:55 UTC). Requirement IDs in the speaker notes point to `docs/spec/*.md`.

**Consent change:** PR #37 merged before this was finalized, so everything below uses main: the first text in is the consent, the first reply carries the carrier lines and the first question follows as its own text, the homeowner is asked "text, email or both" after the request goes out, and the Terms (including the $100 cap, which stays inside the Terms) are accepted by picking beside "By picking, you agree to the Terms". Spanish conversations are off at launch (D-427).

**Labels used on mock screens:**
- **[exact]** wording is in the spec, word for word.
- **[paraphrase]** the spec says what the message must contain but not its exact words.

**Example used throughout (all fake):** homeowner "Maria", request/job **4821**, Lake Havasu City, a Tuesday in July. Businesses: **Desert Air HVAC** (the one picked), **Lakeview Comfort**, **Sunbelt Air**, plus one business that doesn't quote. Masked number (928) 555-0100. Address "100 Sample St".

**Count:** 42 slides. Opening (1), Homeowner (2–20), Business (21–30), Owner (31–36), AI (37–39), Gaps (40–42). The full ranked gap list follows slide 42.

---

## OPENING

### Slide 1: What Ask Hava is

- **One sentence:** Ask Hava is a quote-request service; the business the homeowner chooses pays Ask Hava a set introduction fee.
- A homeowner texts once. Local businesses quote in their own words. The homeowner sees every quote, unranked, and picks one.
- The picked business pays $25, $75 or $199 by job size. Homeowners pay nothing.
- **The standard statement [exact]:** "Ask Hava is not a contractor and does not perform, bid on or supervise work. We pass along your request and show you quotes from independent local businesses, as they wrote them. You choose, and you hire the business directly."
- Pilot: 6 trades (heating and cooling, electrical, handyman, pool and spa repair, weekly pool cleaning, pest control), 8–12 businesses, 8–12 weeks.

**Speaker notes:** One-sentence description is WORD-023 (D-377). Statement is WORD-002 (D-356). Fees FEE-001, FEE-009. Pilot trades LIC-046 and `docs/catalog/categories.yaml` `launch_open: true` (confirmed D-418). Pilot shape is PREBUILD section 7. No calls to businesses ever (BIZ-039). Plumbing is NOT a pilot trade, so burst pipes and water-heater jobs get a safety text where needed and then "not covered yet" (INT-017).

---

## HOMEOWNER JOURNEY (Maria, AC not cooling in July)

### Slide 2: Maria's first text

**Screen (Maria's phone, Tuesday 1:40 p.m.):**
> **Maria → Ask Hava:** Hi, our AC quit this afternoon and it's 92 in the house. Can someone come look?

- She found the number on askhava.com or a flyer. She can also start in the web chat on the site.
- Before anything else, code checks her words for danger (gas, smoke, sparks, flooding, heat).
- "AC" plus "92" is a heat match. That triggers the heat-safety text first.
- Her first text counts as her OK to get automated texts about this request. Nothing asks her to reply OK.

**Speaker notes:** INT-001 (text or web chat). INT-002 keyword check runs before anything. INT-031 heat match: AC/cooler word plus a heat word or 85°F+. CON-001 says safety/heat text goes before the consent text. CON-005 (D-419): first inbound text, or starting a web chat, is the consent; time, message ID, channel and consent-text version are stored. Web chat: a typed phone number gets texts only after a Verify code proves it (CON-025).

### Slide 3: The first two replies

**Screen (1:40 p.m., within 5 minutes):**
> **Ask Hava [exact]:** Ask Hava: No AC in this heat can be dangerous. If anyone is confused, very weak, fainting or collapses, call 911 now. Go somewhere with AC if you can. A fan alone isn't enough when it's over 90°F inside. Drink water and use cool showers or wet cloths. For heat-relief help, dial 211 (live help 9 a.m. to 7 p.m.). Ask Hava is not an emergency service.

> **Ask Hava [exact]:** Ask Hava: Got it. Next, a few quick questions. Ask Hava is not a contractor; you hire the business you pick directly. Automated texts about your request. Msg frequency varies. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms: askhava.com/terms Privacy: askhava.com/privacy

> **Ask Hava:** *(the first question follows at once, as its own text; slide 4)*

- Heat text never holds the request and is sent once per job.
- The second text carries every line carriers require. It asks for no reply.
- If she asks "is this a person?": "Ask Hava is a local service. Our texts are automated…" then the next question.
- **Not in any text anymore:** "Free for you" and the $25–$199 fee (see gap G1).

**Speaker notes:** Heat text INT-031 (211 line only 9 a.m.–7 p.m. and only if the number is verified, INT-036). First reply CON-031 (295 characters, 2 GSM-7 segments), CON-002, CON-004 (never "Reply OK"), CON-027. CON-003: no fee line in the first reply; fee lives in the SMS terms, how-it-works page and homeowner terms. The "18 or older" rule is in the Terms, web chat start and INT-038, not in the text path's first reply.

### Slide 4: "How soon?" and emergency or regular

**Screen:**
> **Ask Hava [exact]:** How soon do you need this? 1) Today, it's urgent 2) In the next few days 3) No rush 4) By a certain day
> **Maria:** 1
> **Ask Hava [partly exact]:** "Emergency: businesses get your request by email marked EMERGENCY, and the business you pick has 60 minutes to confirm. Businesses that get emergency requests at night may be able to come tonight. After-hours work usually costs more." *[paraphrase: Reply 1 for emergency or 2 for regular.]*
> **Maria:** 2

- Maria chooses regular. The business she picks gets 4 business hours to confirm instead of 60 minutes.
- She can still switch to emergency later, on the pick screen.

**Speaker notes:** INT-030 timing question (exact). INT-006 choice; only the emergency sentence is exact, the regular option and reply format are not written anywhere [paraphrase]. Category check comes first after consent (INT-016); for "AC quit" it's obvious, so no question. PICK-017 offers "Switch to emergency (60-minute confirm)". Ordering on the heat path is ambiguous across INT-031, INT-006, INT-016, INT-030 (gap G6).

### Slide 5: The trade questions (heating and cooling)

**Screen [exact questions, from the question bank]:**
> What kind of system is it? 1) Central AC with an outside unit 2) Package unit on the roof or ground 3) Swamp cooler 4) Mini-split 5) Furnace or heat pump (+ Not sure) → **1**
> What's happening? 1) Not cooling 2) Not heating 3) Won't turn on 4) Leaking water 5) Strange noise or smell 6) Thermostat blank or error → **1**
> About how old is the system? 1) Under 5 years 2) 5 to 10 years 3) 10 to 15 years 4) Over 15 years → **3**
> Is an error code or flashing light showing? 1) Yes 2) No → **2**
> Anything else? A brand or code helps. (No names or phone numbers.) → "It's a Trane I think"

- One question at a time. Numbered answers. "Not sure" always works.
- A question is skipped only when her own words already answer it clearly.
- Never more than 6.

**Speaker notes:** INT-039, LIC-040, `docs/catalog/question_bank.yaml` set `hvac_repair` (version 1, `confirmed_by: []`, still to be confirmed with 2–3 real businesses). The first four are card fields; "Anything else?" is NOT a card field, so the business never sees "Trane" (gap G3). "Strange noise or smell" as a numbered answer does not trigger the danger-word check (gap G13).

### Slide 6: Where, own or rent, name and address

**Screen:**
> **Ask Hava [paraphrase]:** What's the ZIP code for the job? → 86403
> **Ask Hava [exact]:** Do you own or rent this place? 1) Own 2) Rent 3) Own the home, rent the lot 4) Second or winter home → **1**
> **Ask Hava [partly exact]:** *[paraphrase: What's your first name and the street address for the job?]* "Only the business you pick gets this, after you pick it. We never ask about immigration status or ID."
> **Maria:** Maria, 100 Sample St

- ZIP checks the service area (Lake Havasu City and listed Arizona towns only).
- Renters get one extra text: their landlord may be responsible.
- Name and address go into a locked vault. Never to AI. Never on a job card.

**Speaker notes:** Area check INT-022 (ZIP question wording not in spec). Own/rent INT-028. Renter text TXT-028. Name/address INT-027, PRIV-001, PRIV-002. Danger-word check skips the address answer so "Smoketree Ave" isn't "smoke" (INT-002).

### Slide 7: Photos

- **The spec has no photo step.** Nothing says when Maria is invited to send a photo, or how (picture text or upload link).
- If she sends one anyway: location data is stripped, it's stored privately, and it goes **only to the business she picks**, after it pays.
- Photos never go on job cards until a zero-retention AI agreement is signed and photo screening runs.
- So in the pilot, **no business sees a photo before quoting.**

**Speaker notes:** PRIV-013 to PRIV-015 (optional, stripped, "avoid photos of people"), CARD-013 (never on cards until zero-retention), CARD-014 and AI-016 (two AI checks plus an on-server check), PICK-022 (full photos released after payment). No INT requirement asks for photos. Gap G2. For the deck, show this slide with a "Not decided" banner.

### Slide 8: "Here's what we'll send"

**Screen [exact template, example values]:**
> **Ask Hava:** Ask Hava: Here's what we'll send for request 4821: heating and cooling, Central AC with an outside unit, Not cooling, 10 to 15 years. It goes out in 2 hours. Reply EDIT to change it, or SEND to send it now.
> **Maria:** SEND

- EDIT asks which answer to change, then shows a new summary. SEND, OK or YES sends at once.
- No reply in 2 hours: it goes out as summarized. Nothing waits on her.
- Skipped on an emergency job (speed first).
- **Note:** the summary shows the trade and up to 3 answers. Businesses actually get more (her own words, timing, all card answers, the heat flag). See gap G3.

**Speaker notes:** INT-041 (D-395, wording changed by D-419). Built from fixed fields, no contact details. Card contents are CARD-007. No AI writes this. This step is the natural place to show Maria the whole card (gap G3) and is also where an AI-written card would be approved by her (slide 39).

### Slide 9: Request sent, and "text, email or both?"

**Screen (1:53 p.m.):**
> **Ask Hava [exact template]:** Ask Hava: Your request went to 4 local businesses that take heating and cooling jobs. Not every business answers. Quotes are open until 1:53 p.m. Wednesday, Arizona time. We'll text you when the first one comes in. You can choose as soon as the first quote arrives. If it can't wait, it's fine to call a company you know too. Businesses get requests by email, so the first quote can take a few hours.
> **Ask Hava [exact]:** Ask Hava: How should we send quote updates? 1) Text 2) Email 3) Both. Every update links to your private quote page.
> **Maria:** 3
> **Ask Hava [exact]:** Ask Hava: What email should we use? We never share it with businesses.
> **Maria:** maria@example.com

- No answer means text. Asked once, never again.
- With email only, safety texts, codes and direct replies still come by text.
- Each update email has an unsubscribe link. A bounced address switches her back to text.
- "4" counts only businesses whose email was accepted. Each business has 24 hours from its own card to quote.

**Speaker notes:** TXT-032 including the two urgent lines (her answer was "Today, it's urgent", chose regular). N per CARD-019 (D-397). Quote window PICK-001. Updates question TXT-018 (D-422); email checked for format only, never verified by a link (gap G7). On an emergency job the question comes after the INT-029 questions.

### Slide 10: Quotes arriving

**Screen (2:35 p.m.):**
> **Ask Hava [paraphrase; required parts exact]:** Ask Hava: Your first quote for request 4821 is in: site visit needed, $89 trip fee, earliest visit today, evening. More may come, and you can pick any time. Can't open links? Reply LIST to get your quotes by text. Or see them at: askhava.com/q/[private link]

**Later, if she replies LIST [exact format]:**
> 1) Lakeview Comfort, site visit needed, $89 trip fee, earliest today evening
> 2) Desert Air HVAC, price range $450 to $650, earliest today evening
> 3) Sunbelt Air, firm price $385, earliest tomorrow morning
> To choose, reply the number and YES, for example 2 YES.
> By picking, you agree to the Terms: askhava.com/terms

- At most one "you now have k quotes" text every 3 hours: "<k> of 4 businesses have quoted so far."
- She gets the same updates by email (she chose both).

**Speaker notes:** TXT-016 lists what the first-quote text must contain and puts the LIST line before the link; sentence wording is not fixed. Count updates TXT-017, TXT-032. LIST TXT-031 (max 10 per text) now ends with the Terms line (D-420), so a text pick is a Terms acceptance logged with channel "text" (PICK-016). A "2 YES" sent without a LIST or lapse text in front of it still picks (gap G8).

### Slide 11: The private quotes page

**Screen mock (askhava.com/q/…, hidden from search):**
> Ask Hava is not a contractor and does not perform, bid on or supervise work. We pass along your request and show you quotes from independent local businesses, as they wrote them. You choose, and you hire the business directly.
> Every eligible quote shown, in the order received. #1 means first received, not first choice.
> Ask Hava shows no reviews and doesn't rank businesses. To check this business yourself: look up its license, search its name, or ask it for local references.
>
> **#2 Desert Air HVAC** · Independent business, not affiliated with Ask Hava. · ROC #123456 showed Active on the ROC website when we checked on <date>. Check it yourself. · ROC license since 2014
> **Price range: $450 to $650** *(plain definition of "price range")*
> Business's own words: "Likely capacitor or fan motor. Range covers parts and labor. Can be there by 6."
> Trip fee counts toward the repair: No trip fee · Parts and materials: Included · Permit: Not needed · Price may change after seeing the job: Yes · Earliest visit: Today, evening · Speaks Spanish: No
> No reviews or ranking from Ask Hava; check it yourself.
> **[Choose]** or reply 2 YES

- Every quote, numbered by arrival. No sorting, no "best", no stars.
- Business names are shown. No phone numbers or addresses on either side.

**Speaker notes:** WORD-002, WORD-005, PICK-041 (top line and card line), WORD-014, LIC-026, LIC-037, PICK-040 (quote facts and order), PICK-008 to PICK-010, PICK-015. Layout itself is not specified [paraphrase]. "About us" line (BIZ-022) and "We speak Spanish" (BIZ-006) can also appear. Button label: PICK-009 says "Choose"; PICK-016 says "Pick" (gap G19).

### Slide 12: Picking (review step)

**Screen mock [exact lines]:**
> You're choosing one business for this request. If it falls through, you can choose another quote.
> You hire the business directly. Ask Hava isn't part of your agreement with it.
> If you choose now, Desert Air HVAC has until 7:30 p.m. to confirm by email.
> **If Desert Air HVAC doesn't confirm in time, go to:** ( ) No backup ( ) #1 Lakeview Comfort (•) #3 Sunbelt Air
> **How should they reach you?** (•) Call me through a masked number ( ) Email me at…
> **When can they call?** (•) Any time 8 a.m. to 8 p.m. ( ) Mornings 8 to 12 ( ) 12 to 1 ( ) After 5 p.m.
> By picking, you agree to the <u>Terms</u> **[Pick]** · [Go back]
> *Then:* a 6-digit code by text. "Your code to choose a quote is <code>. Only type it on askhava.com. We never ask for it by phone."

- Backup choice is hers. Ask Hava never picks it. "No backup" is the default.
- Offered: "Switch to emergency (60-minute confirm)".
- Only "By picking, you agree to the Terms" beside the button. The $100 cap is inside the Terms under "Limit of liability", not on the page.
- This tap is when she accepts the Terms (logged with time, version, request number).

**Speaker notes:** PICK-017 (review lines, end time, switch-to-emergency), PICK-047 backup (D-400), PICK-018 contact and window, PICK-016 (D-420, D-421: only the terms line, never the cap or a key-terms summary), PICK-013 code (10 minutes, 5 tries, voice only if she taps "Call me with the code instead"). TERMS-001, TERMS-004. Nothing on this screen says "free for you" or that the business pays a fee, though CON-003 says the fee "is shown again beside every pick" (gap G1). PICK-017 still says "You're choosing…" while the button says Pick (gap G19).

### Slide 13: Right after the pick

**Screen (3:30 p.m.):**
> **Ask Hava [exact]:** Ask Hava: Done. Desert Air HVAC has your pick by email and has until 7:30 p.m. Arizona time to confirm. Businesses check email at different times. Nothing else to do for now. You can still choose another quote before then.
> **Ask Hava [exact]:** Who will let the business in? 1) Me 2) Someone else 3) A lockbox
> **Maria:** 1

- If Desert Air asks for 2 more hours: "Ask Hava: The business you picked is confirming. You'll hear by <time>. You can still choose another quote on your page." [exact]
- If Desert Air is on a job ("Away"): "Ask Hava: Desert Air HVAC is on a job until <time>. You'll hear by <time>. You can still choose another quote on your page." [exact]
- If it doesn't confirm in time, her backup (Sunbelt Air) gets the pick automatically.

**Speaker notes:** PICK-045, PICK-025 (for 2: name and number go to vault, "Should the business call them first? Reply FIRST."; for 3: "Tell the business the code on the call. Please don't text us a code."), PICK-043 HOLD, BIZ-034 Away, PICK-047 backup text "Ask Hava: <Business> didn't confirm in time, so your backup choice, <Business 2>, has your pick now. They have until <time> to confirm."

### Slide 14: The business confirms, and calls

**Screen (4:05 p.m.):**
> **Ask Hava [paraphrase; two sentences exact]:** Ask Hava: Desert Air HVAC confirmed request 4821. They'll call you from (928) 555-0100, any time 8 a.m. to 8 p.m. Arizona time. If you miss it, call them back on this number. It works for 30 days. Ask for the price and any changes in writing before work starts. Don't pay in full up front.

**If she misses their call [exact]:**
> Ask Hava: Desert Air HVAC tried to call you about request 4821. Call them back on (928) 555-0100. Reply TIMES to change when they call.

- The masked number rings through to the business. Neither side sees the other's real number. Calls are never recorded.
- She gets a copy of the accepted quote by email.
- Only now does Desert Air get "Maria" and "100 Sample St".

**Speaker notes:** TXT-027 (who's-calling), TXT-030 (missed call, TIMES), CALL-001 to CALL-007, CALL-006 (no recording), PICK-022 (release only after payment succeeds), PICK-023, PICK-042 (copy of quote). Email-contact version: "Ask Hava: <Business> confirmed request <n>. They'll email you at the address you chose. Ask for the price and any changes in writing before work starts. Don't pay in full up front." [exact].

### Slide 15: Check-in, feedback, problems

**Screen (day 3):**
> **Ask Hava [paraphrase]:** Ask Hava: How did request 4821 go? Reply DONE if the work is done, PROBLEM if something went wrong, or NO if they never reached you. Not done yet? Reply LATER. More answers: reply MORE.
> **Maria:** DONE
> **Ask Hava [paraphrase]:** a private feedback request (never shown publicly, never used to order quotes).

**If PROBLEM [exact]:**
> Ask Hava: Thanks. A person reads every problem report within 7 days. Ask Hava can't settle disputes about work or price. Licensed trades: you can also file with the state at askhava.com/help/roc-complaint.

- LATER moves the check to day 14. OTHER means she hired someone else.
- No reply by 7 days after day 14: the job just closes.

**Speaker notes:** JOB-015 (check-in timing; only the email-contact first line is exact), JOB-017 (private feedback, wording not specified), JOB-026, JOB-028, TXT-029, BIZ-011 (complaint response), OPS-005 (safety words in a PROBLEM push to the owner). No fee changes because of a PROBLEM (JOB-016).

### Slide 16: STOP and HELP, any time

**STOP [exact]:**
> Ask Hava: You're unsubscribed. You won't get more texts or automated calls from Ask Hava at this number. To use Ask Hava again, text START.

**HELP [assembled from exact parts]:**
> Ask Hava: help@askhava.com, askhava.com/help. Tell us what's wrong in one text. The Ask Hava owner gets HELP messages right away and usually replies within 2 days, or within 24 hours if your job is urgent. This number is text-only. If anyone is in danger, call 911. Can't open links? Reply LIST to get your quotes by text. Canadian numbers may pay roaming charges. Reply STOP to opt out.

- STOP never closes her request. It continues by email and on her quote page.
- "Please stop texting me" counts. "Can stop by at 3" doesn't.
- After HELP, her next message is matched to common answers (where are my quotes, how to close, does it cost anything) or goes to the owner.
- Calling the Ask Hava number gets a recording: "This is Ask Hava LLC. We work by text…" No voicemail.

**Speaker notes:** CON-008 to CON-013, CON-028 (open-request notice by email only), CON-035 (one suppression list), CON-018 (HELP), CON-034 (common answers), OPS-036 (help request to owner), CALL-020 (recorded greeting). CLOSE 4821 closes a request (JOB-019); "cancel" alone never does (JOB-020).

### Slide 17: Emergency path (gas smell)

**Example: a January night, a different homeowner texts "rotten egg smell by the furnace".**
> **Ask Hava [exact core]:** …a rotten-egg smell can mean a gas leak. If you smell gas or hear gas hissing, leave now. Don't use switches, flames or phones near it. From outside, call 911 and your gas company (UniSource natural gas: <number>). Don't try to turn the gas meter or valves. Ask Hava is not an emergency service. Call your gas company now, before getting quotes. Keep the appliance off until a licensed pro checks it. Reply SAFE when you are safe.
> *(then the first reply with the carrier lines, CON-031)*
> **Homeowner:** SAFE
> **Ask Hava [exact]:** Ask Hava: Has your gas company checked it? 1) Yes 2) Not yet

- **No job card goes out until they reply SAFE.**
- Then emergency or regular. If emergency: cards go out at once, marked EMERGENCY. No "Here's what we'll send" step.
- Picked business has **60 minutes, any hour**, to confirm. One reminder at 30 minutes.
- Carbon monoxide, electrical and flooding each add their own lines. A chirping smoke detector gets a "new battery" text instead.

**Speaker notes:** INT-002 to INT-005, INT-025 (SAFE), INT-035 (gas company question; answer goes on the card), INT-006, INT-029 (cards first, then own/rent and address), CARD-021 (EMERGENCY subject), PICK-044 (60 minutes), INT-034 (electrical, water), INT-037 (chirp). Numbers come only from the verified safety file (INT-036). Flood/burst pipe and water-heater gas are plumbing, which is not a pilot trade: they get the safety text, then "not covered yet" (INT-017).

### Slide 18: After hours (8 p.m. to 8 a.m.)

**Same AC text, sent at 9:30 p.m.:**
> Heat-safety text (without the 211 line: 211's live help ends at 7 p.m.).
> **Ask Hava [exact]:** Ask Hava: 2 heating and cooling businesses on Ask Hava get emergency requests at night.
> *(then the emergency-or-regular choice)*

**If none take night emergencies [exact]:**
> Ask Hava: No heating and cooling business on Ask Hava gets emergency requests at night yet. Your request goes to businesses at 8 a.m. If anyone feels unwell or it isn't safe, call 911.

- Regular texts and cards wait until 8 a.m. Safety texts never wait.
- Night emergency: only businesses that ticked after-hours (and passed a phone-alert test) get it now; everyone else at 8 a.m.
- No quote 60 minutes after a night emergency card [exact]: "Ask Hava: No after-hours business has quoted yet. Your request also goes to every heating and cooling business at 8 a.m. If anyone feels unwell or it isn't safe, call 911."

**Speaker notes:** QH-001 to QH-007 (8–8 in Arizona and the phone's area-code zone), QH-004 exceptions, INT-031 (211 hours), INT-033 (night count, N=0 text, 60-minute text), BIZ-007 (after-hours tick needs a person with "Emergency emails at any hour" and a current test), PICK-044 (night pick of a non-night business held to 8 a.m.).

### Slide 19: When no quotes come, or a pick falls through

**No quotes by 6 p.m. the next day [exact]:**
> Ask Hava: No quotes yet for request 4821. Small or unusual jobs can take longer. A few more details often help a business price it. Reply 1 to add details, 2 to keep waiting, or 3 to close.

**Pick not confirmed, no backup [exact]:**
> Ask Hava: Desert Air HVAC didn't confirm in time. Businesses get picks by email and sometimes miss the window. You still have other quotes: *(LIST lines)* To choose, reply the number and YES, for example 2 YES. Or see them at: <link> By picking, you agree to the Terms: askhava.com/terms

- 5 days with no quote: closed, and told she can text any time to try again.
- Daytime emergency with no quote in 60 minutes [exact]: "Ask Hava: No business has quoted yet. If it can't wait, it's fine to call a company you know. If anyone feels unwell or it isn't safe, call 911."
- Only 1 or 2 businesses got it: the first text warns "quotes may be slow or may not come."

**Speaker notes:** JOB-010 ("1" adds her next message to "the job page", which is undefined, gap G3), JOB-011, JOB-012 (3 days with quotes but no pick closes), JOB-025, INT-033, PICK-046, TXT-032 (1–2 business warning). A trade opens only with 2+ active businesses (LIC-046).

### Slide 20: Spanish

**At launch, conversations are English-only.** A setting, "Spanish conversations", is off.

| Stays in Spanish at launch | English at launch (even if she writes in Spanish) |
|---|---|
| Safety, heat-safety and emergency texts | First reply, intake questions, updates |
| Trade safety prompts (boat fuel, pool shock, sewage) | Quote page, pick, check-ins, emails |
| The AYUDA (help) reply and the Spanish opt-out confirmation | Business notices |

- PARAR, BAJA, AYUDA and other Spanish keywords always work.
- Before launch, each Spanish text that stays on is back-translated by two separate AIs (ChatGPT and Grok); both must match the English meaning.
- The setting turns on only after a paid professional review (about $100) of every Spanish template is recorded.
- When it's on: everything from Spanish templates, "Homeowner prefers Spanish" on cards for Spanish-speaking businesses, "Palabras del negocio" on the quote page (the business's words stay in English).

**Speaker notes:** TXT-039 (D-427), TXT-002, CON-026, CON-008, CON-018, TERMS-018, BIZ-018, CARD-012, PICK-003, PICK-040, WORD-011. Spanish first reply (CON-031, 303 characters) is built and tested but sent only while the setting is on. A Spanish-only texter at launch gets English questions after a Spanish safety text; there's no fixed line telling her why (gap G15).

---

## BUSINESS JOURNEY (Desert Air HVAC)

### Slide 21: How a business hears about Ask Hava

- In person, a post, the website or an email. **Never a recruiting text or call.**
- Business page: a sample job card per open trade, "How you're charged", and "Who runs Ask Hava" (an LLC, owner lives in Havasu, has a day job, no personal name).
- [exact] "Job requests and picks come by email. We don't text or call businesses."
- [exact] "You quote once. No bidding rounds, no paid placement." and "Every card says how many businesses got it."

**Speaker notes:** BIZ-048 (no recruiting texts/calls; recruiting emails need an honest subject, postal address and stop link), WEB-020, WEB-021, WEB-024, WORD-017, BIZ-030. ROC and city lists are used to verify, never to recruit (LIC-010).

### Slide 22: Sign-up, part 1 (what you do)

**Screens:**
1. **Pick your trades:** each marked "Open now" or "Coming later". Coming-later only → waitlist, join date kept.
2. **License number:** Ask Hava looks up the ROC record. Class comes from the record, never typed. Read-back: "Your R-39 license covers…"
3. **Job types:** checklist **pre-ticked** from the license (AC not cooling, thermostat, duct repair, furnace repair, swamp cooler repair). Untick what you don't do.
4. **Four questions:** towns served · largest job you'll quote (no limit, $1,500, $5,000 or an amount) · recurring work yes/no · after-hours emergencies yes/no.
5. Same steps as one checkbox page or one question per screen. Summary reads it all back.

- Commercial-only license: [exact] "Your license is commercial-only. Ask Hava sends home jobs only, so it can't send you job requests…"

**Speaker notes:** BIZ-021, LIC-006, LIC-027, BIZ-049 (D-393), LIC-043 and `docs/catalog/license_map.yaml` (R-39, CR-39, R-39R subclassifications), LIC-044, BIZ-004. No card at sign-up (BIZ-001, FEE-036).

### Slide 23: Sign-up, part 2 (proving it's you, and making sure email works)

1. **Identity code** to a contact on the **public record** (ROC phone by text, else ROC email, else a mailed letter). Never a call. "Someone else filling this in? Ask the owner to read you the 6 digits, or forward the text."
2. **Verify your email.**
3. **Sample card email**: "Did it arrive in your inbox?" Inbox / Spam / Promotions / Not yet → steps for that mail app. **Gmail:** Not spam, add sender to contacts, filter "Never send it to Spam", **move to Primary** (Gmail only alerts for Primary).
4. "Reply OK to this email, star it and add us to your contacts…"
5. **Sample pick** with Do Not Disturb on: "Did your phone alert you?" "Skip this and you may miss picks."
6. If after-hours: **test emergency email** with Sleep/DND on. Yes lasts 90 days.
7. Plain-words fee summary, E-SIGN box (unticked), Terms.
8. Done: [exact] "You're one of <n> businesses in heating and cooling so far. We'll tell you once, by email, when requests in your trade start."

- Activation is **automatic** when the code and license check pass with no flags. Welcome email includes a contact card "Ask Hava homeowner calls" with the masked numbers.

**Speaker notes:** BIZ-002, BIZ-024, BIZ-026, BIZ-040, BIZ-042 (all the inbox steps), BIZ-007, BIZ-037, BIZ-030, CON-019, BIZ-031, BIZ-016 (auto-activation; flagged ones wait for the owner, target 4 days BIZ-027), CON-020, BIZ-046. Backup notice address BIZ-043. Test journey J2 still says the owner approves every sign-up (gap G9). Pest businesses can't follow step 1 as written (gap G5).

### Slide 24: A job card arrives (exact content, layout is a mock)

**Email subject [exact template]:** Ask Hava: New Heating and cooling repair job in Lake Havasu City. Quote by 1:53 p.m. Wed.

**Body mock:**
> **Job 4821** · Heating and cooling repair · Lake Havasu City · House
> How soon: Today, it's urgent
> "Hi, our AC quit this afternoon and it's 92 in the house. Can someone come look?"
> What kind of system: Central AC with an outside unit
> What's happening: Not cooling
> About how old: 10 to 15 years
> Error code or flashing light: No
> Safety text sent: heat
> Sent to 4 businesses.
> **[Quote this job]** (works until the quote window ends)
> Ask Hava is not a contractor. It passes along this request and the homeowner's choice. If the homeowner picks you, they hire you directly; Ask Hava doesn't direct, schedule, supervise or pay for the work.

- **Not on the card:** name, phone, email, street, photos, and her "It's a Trane I think".
- Open-cards reminder email up to twice a day: "Ask Hava: Your job requests still open".

**Speaker notes:** CARD-020 (subject, statement, Quote link), CARD-005 to CARD-008 (contents and final scrub), CARD-019 (count), CARD-006 (town and kind of place), CARD-022 (open-cards email), BIZ-038 (Quote link works without sign-in, single purpose). Who gets it: CARD-001 (license covers the job type, ticked it, serves the town, largest-job answer not below the size band). Rental: "Landlord approval may be needed." (CARD-011). Spanish: "Homeowner prefers Spanish" (CARD-012). Exact layout is not specified.

### Slide 25: Quoting

**Quote form mock:**
- **Quote type:** Firm price · Price range · Site visit needed (trip fee plus an estimate on site) · Recurring (per visit/week/month/quarter/year, plus any setup charge) · Hourly (**must give estimated hours**, or it counts as site visit)
- **Amount:** $450 to $650
- **Your words:** "Likely capacitor or fan motor. Range covers parts and labor. Can be there by 6."
- **Quote facts** (pre-filled, "Same as last time"): trip fee counts toward repair · parts · permit · price may change · earliest visit · speaks Spanish
- **Fee preview [exact pattern]:** "If picked, this quote's fee is $75 (range midpoint $550)."
- Bounced back if it has a phone number, link, spelled-out number or "call me".
- Warning before sending a quote above your largest-job answer.
- Later, once Stripe says yes: an unticked "Auto-confirm and pay if picked" box.

**Speaker notes:** PICK-002 (types), FEE-043 (hourly), FEE-044 (setup charge), REC-010 (recurring must state a period), PICK-040 (facts), PICK-039 (fee preview), PICK-007 (bounce), BIZ-049 (largest-job warning), LIC-019/LIC-036 (handyman $1,000 limit and certification), PICK-048 (auto-confirm hidden until the Stripe answer is filed). No way to withdraw or correct a quote after sending (gap G14).

### Slide 26: Not picked

**Email to Sunbelt Air [exact template]:**
> Job 4821 went to another business this time. Several businesses quoted; the picked quote was higher than yours. No action needed. Thanks for quoting.

**Email to Lakeview Comfort (site-visit quote):** compared as "different quote types" [wording not fixed].

**When the job ends [exact]:** "This job is no longer active."

- Amounts and names are never shared. "About the same" means within 10%.
- "Several" replaces the count when the card went to 4 or fewer businesses.

**Speaker notes:** PICK-026, PICK-038 (D-193). B14 confirmed (D-418). The "different quote types" result doesn't fit the PICK-026 sentence (gap G17). Business's own numbers panel: BIZ-025.

### Slide 27: Picked: confirm and pay in one step

**Pick email [exact]:**
> Subject: Ask Hava: A homeowner chose your quote for job 4821. Confirm by 7:30 p.m.
> A homeowner picked your quote for job 4821 (AC not cooling, Lake Havasu City). One step left: tap Confirm and pay $75 by 7:30 p.m. Then you get their first name and address.
> Refund if you can't reach them: 2 call attempts at least 3 hours apart, inside their window, within 72 hours. They can be on different days.
> [Hold for 2 more hours]

- **4 business hours** (8 a.m.–8 p.m., every day). Reminders every business hour; last one: "Ask Hava: Last reminder: job 4821 has 1 hour to confirm. After 7:30 p.m. the homeowner can choose another quote."
- **Confirm page:** job number, fee, tier, how the fee basis was set, refund line, one button **"Confirm and pay $75"**. Card typed into Stripe or saved card. Counts only when payment succeeds.
- **Fees:** under $500 → $25 · $500–$1,500 → $75 · over $1,500 → $199. Range = midpoint. Site visit = $25. Recurring = 4 months, capped at $75. Tax included.
- **First win free** if you joined in the first 90 days (or within 90 days of your trade opening). "Confirm" alone.
- **Repeat picks by the same homeowner are free** for 180 days after the job closes.
- HOLD: once, +2 business hours (not on emergencies). Emergency: 60 minutes, any hour.

**Speaker notes:** PICK-037 (email, reminders, Hold link), PICK-019, PICK-021, PICK-043, PICK-044, FEE-001, FEE-003, FEE-004, FEE-036, FEE-010 to FEE-013, FEE-039, FEE-008, FEE-015 (authorization), BIZ-038 (Confirm link needs a code to that person's phone if no session), SEC-004. Only owner or "Confirm and billing" members can confirm (PICK-033). There is no "decline" button (gap G12). Card declined: 1 business hour to fix (FEE-020).

### Slide 28: After confirming

**Release email [exact]:** "Ask Hava: Job 4821 is confirmed. Sign in to see the homeowner's details." No name or address in the email (D-454).

**Release view in the portal, after sign-in [paraphrase; two lines exact]:**
> Job 4821 is confirmed. Homeowner: Maria, 100 Sample St, Lake Havasu City. Contact: call through (928) 555-0100. Window: any time 8 a.m. to 8 p.m. Arizona time. "Call only inside the homeowner's window." "Tip: on your first call, ask which number to text."

- Never the homeowner's real phone number. Never by text.
- Calls from your linked phones go through the masked line. An unlinked phone hears a PIN prompt; the owner gets a "Link this phone" email.
- No call yet at 24 and 48 hours [exact]: "Ask Hava: Job 4821: no call yet. Call within 72 hours of the release, or the fee can't be refunded."
- **Automatic refund** (decided at 96 hours) if: 2 attempts, 3+ hours apart, inside her window, within 72 hours, nobody answered. Never called = no refund, recorded as a no-show.
- Portal call log shows each attempt and whether it was inside the window.

**Speaker notes:** PICK-024, PICK-022, PICK-023, CALL-002 to CALL-005, CALL-011, CALL-012, TXT-026, FEE-022, FEE-023 (homeowner cancels before talking: refund), FEE-035, BIZ-035, TERMS-024 (use details only for this job). Receipt and "Why was I charged?" page: FEE-031.

### Slide 29: Charges, disputes, complaints

- **Question about a charge:** "Ask about a charge" form on the receipt and billing page, or reply to any email. Instant reply with the charge details; owner usually replies within 2 days.
- **Our mistake:** refunded in full.
- **Bank dispute:** owner alerted; evidence pack sent to Stripe automatically 2 days before the due date.
- **Homeowner complaint:** shows in your portal (no homeowner details) with a reply box open 5 days. Paused only after 3 complaints in 90 days.
- Lapsed license or bond: paused that day. License site down: up to 72 hours as you were, but you can't confirm while unknown.

**Speaker notes:** BIZ-032, BIZ-047, TERMS-022, FEE-026, FEE-027, FEE-042 (evidence record), BIZ-029, BIZ-012, OPS-005, LIC-008, LIC-009, BIZ-050 (license class change re-applies the map).

### Slide 30: Running your account

- **Away until <time>** (up to 4 business hours): a pick's clock starts when you're back. Homeowner is told.
- **Pause job cards / Resume / Busy for <n> days** (1–60), from links in any email.
- **Team members:** each with own sign-in email, roles "Cards and quotes" and/or "Confirm and billing". Owner approves phones they link.
- **Backup address** per person (for the person who checks email more often).
- **Monthly fee limit:** optional. A Confirm over it isn't charged and that pick ends.
- **"Your numbers" panel** and a monthly "you missed cards" email with the inbox fix.
- **Leaving:** the business terms say "leave at any time in the portal", but **no requirement defines leaving** (gap G16).

**Speaker notes:** BIZ-034, BIZ-028, BIZ-038, BIZ-023, BIZ-033, CALL-012, BIZ-043, FEE-041, BIZ-025, BIZ-044, `docs/legal/business-terms.md` section 11. Optional push web app comes in the last build stage (BIZ-041). Business texting stays off until Twilio says yes in writing (TXT-035).

---

## OWNER JOURNEY (Casey)

### Slide 31: The owner's week

- **Target:** your minutes per paid introduction ≤ 20 (stop line: over 45). You enter minutes weekly on the scorecard.
- **Old estimate:** one 30-minute weekly window plus a 10-minute mid-week check, about 25–35 minutes a week [sim]. **This predates HELP messages pinging you right away**, so it needs redoing (gap G11).
- **What you do:**
  - Answer HELP requests (homeowners and businesses): promise is 2 days, 24 hours if urgent.
  - Review flagged sign-ups (target 4 days).
  - Urgent call-backs to homeowners who can't get texts or email.
  - Read the weekly digest; monthly reviews (near-tier-boundary quotes, crowded trades, lapsed picks, no-quote report, 10 random cards).
  - Recruit: 2+ businesses per pilot trade.

**Speaker notes:** OPS-039, PREBUILD section 7, `docs/for-casey-review.md` line ~361 (estimate, marked [sim], written before D-348), OPS-036, CON-018, BIZ-027, INT-023, OPS-030, FEE-033, OPS-027, OPS-031, OPS-034, OPS-040, OPS-009, OPS-033 (windows never more than 4 days apart). No backup person (OPS-002).

### Slide 32: What reaches your phone, and how often

| Pushes to your phone | How |
|---|---|
| Safety alert the system couldn't handle (texting down, or an opted-out number reports danger) | Email push **plus an automated call that breaks Do Not Disturb** |
| Each HELP request | Email push, at once, any hour (after 10 in a day, they wait for the next 9/3 push) |
| 9 a.m. and 3 p.m. list (only if something's open): urgent call-backs, help due within 24 h, safety-word complaints, Twilio restored, license list failing 24 h | Email push |
| App still down 15 min after auto-restart | Email push: "press Restart" |
| Twilio number suspended | Email push |
| Money mismatch still there next night | Email push |
| Email plan at 70% of limit | Email push, max once a day |

- Everything else waits in the owner portal and the **weekly digest**.
- Never texts from the campaign number. How often: **no real number yet**; only the pilot will tell.

**Speaker notes:** OPS-029, OPS-032, OPS-035, OPS-036, OPS-004, OPS-030. On an unanswered critical safety alert, you place "one call from the Ask Hava number reading the generic safety script" (OPS-028); the spec has no tool for calling from that number (gap G10).

### Slide 33: HELP requests and safe mode

**HELP notification [exact subject pattern]:** "Ask Hava HELP: homeowner 4821, reply by <time>"
- Holds: job number, status, trade, town, urgent or not, the message (contact details removed), the last automated message they got, one link.
- Owner portal page: conversation and timeline, one-tap fixes (resend link or code, close/reopen, change contact choice, refund or pause by the rules), reply box.
- **You type the reply** (never AI), up to 300 characters, sent as "Ask Hava: Reply from the Ask Hava owner: <text>".

**Safe mode:**
- Starts when a High alert that pushed to your phone sits **unacknowledged 24 hours**.
- Pauses **charges and releases only**. Texts, safety texts, STOP/HELP and pick codes keep going.
- Ends when the alerts are acknowledged (by you or automatically). Paused charges re-check before going ahead.

**Speaker notes:** OPS-036 (D-348), WORD-021 (no personal name), OPS-001, OPS-023, OPS-024, FEE-038 (payment timers pause too), OPS-026 ("away until <date>" changes promises, not timers).

### Slide 34: The money view

| Line | What it is |
|---|---|
| Collected | Fees from the ledger, matches Stripe to the cent |
| **Sales tax set-aside 7.6%** | While the tax question (B10) is open |
| Expenses | You type them in (date, vendor, amount, category, note); never in the ledger |
| Left | Collected − set aside − expenses |

- Monthly CSV. Refunds, free wins ($0 with the waived amount), fee-free jobs and disputes each have their own ledger line.
- Nightly check against Stripe; any difference alerts you.
- No weekly money task.

**Speaker notes:** FEE-046 (D-410, 7.6% still in spec), FEE-028, FEE-029, FEE-030, FEE-012, FEE-045, FEE-027, OPS-009. Sales tax path still open (PREBUILD B10). Fees are tax-included (FEE-008).

### Slide 35: The pilot scorecard

| Measure | Go | Stop or rethink |
|---|---|---|
| Requests with 1+ quote in 24 h | ≥70% | <50% after 4 weeks |
| Median time to first quote | ≤4 h | >12 h |
| Picks per request with quotes | ≥40% | <25% |
| Picks confirmed (not lapsed) | ≥85% | <75% |
| Average fee per paid introduction | ≥$50 | <$35 |
| Free share of introductions | ≤30% after month 2 | >50% |
| Refunds / bank disputes | ≤10% / ≤1% | >20% / >2% |
| Businesses paying again within 60 days | ≥50% | <30% |
| Your minutes per paid introduction | ≤20 | >45 |
| Homeowner complaints | ≤1 per 20 jobs | >1 per 10 |

- Green/amber/red per trade and total. Also masked calls answered.
- **It decides nothing.** You decide at week 8.

**Speaker notes:** OPS-039 (D-396), PREBUILD section 7, CALL-024. Site-visit quotes at $25 pull the average fee down (gap G4).

### Slide 36: What's automatic, and what only you can do

**Fully automatic:** intake and safety texts · routing to licensed businesses · job cards · reminders · quote page · pick codes · fee math · charging on Confirm · release of details · masked calls · refunds by the rules · license checks daily · business activation when clean · bank-dispute evidence · nightly money check · backups · safe mode.

**Only you:**
- Approve flagged sign-ups; pause or remove a business.
- Type HELP replies; make urgent call-backs; the emergency safety call when everything else failed.
- Flip switches: trades open, pause all/town/trade/charging/texting, AI on scrubbed text, AI quote summaries, safety voice call, business texting, auto-confirm (after Stripe's written yes), Spanish conversations (after the paid review).
- Turn texting back on after a Twilio suspension.
- Buy masked numbers; upgrade the email plan.
- Gate C: live keys, press Deploy, approve the first live charge.

**Speaker notes:** BIZ-016, SEC-006 (only the owner portal can refund/pause/release), OPS-003, LIC-046, AI-008, PICK-003, CALL-019, TXT-035, PICK-048, TXT-039, TXT-024, CALL-017, SEC-025, OPS-022. Insurance is now "later, after the business is fully working" (D-426), so paid introductions may start with no E&O or cyber policy (gap G23).

---

## HOW THE AI WORKS

### Slide 37: What the AI does today (in the plan)

**At launch the app runs with no AI at all.** One switch can turn on a small amount.

| AI does NOT | AI may (only if the switch "AI on scrubbed text" is on) |
|---|---|
| Write any text or email anyone reads | Read the scrubbed request (names, phones, emails, street numbers removed) |
| Write job cards or summaries of the request | Guess the trade and job type, with a confidence level |
| Summarize quotes for homeowners (off at launch) | Pick which fixed question to skip or ask next |
| See photos (until zero-retention is signed) | Give a second opinion on "is this a business, not a homeowner?" |
| Guess a license class | |
| Override a danger keyword | |
| Touch money, fees or who gets details | |

- Every text is a **fixed template**. Every intake question is a **fixed question** from the bank (3–6 per trade).
- Keyword rules give an independent second opinion. If unsure: ask the homeowner.
- Privacy policy says, word for word: "No AI reads your messages today. Our texts are automated and come from fixed templates."

**Speaker notes:** AI-001 to AI-018. AI-006 (never writes texts; while summaries are off it writes nothing anyone reads), AI-008 (switch, needs provider terms recorded and matching privacy wording that day; D-412, confirmed D-418), AI-010 (fixed output fields only), AI-011, AI-012 (gas list overrides), AI-016 and CARD-013 (photos), AI-017, AI-018, TXT-004, CARD-007 (cards from fixed fields and the scrubbed description, never free text), PICK-003 (quote summaries off), INT-040 (confidence), INT-039. Golden test set target 90% (OPS-040).

### Slide 38: What you asked for, and what would have to change

**Your ask (as relayed):** "an expert assistant that understands any request like a licensed pro, reads photos like a pro, and packages the request into a short card a business can quote from."

| Piece of the ask | Today | What changes in the spec |
|---|---|---|
| Understands any request like a pro | Allowed in part: AI may classify and choose the next fixed question (switch) | Mostly a setting: turn on AI-008. To let AI ask its *own* follow-up questions: change AI-006, TXT-004, INT-039 (AI would write text homeowners read) |
| Reads photos like a pro | Not allowed: photos off cards, no photo step | Sign zero-retention (AI-001, CARD-013); add a photo step (new INT row); build screening (AI-016, CARD-014); let AI read photos for the card, not just screen them (AI-010) |
| Packages a short card | Not allowed: card = fixed fields + her scrubbed words | Change CARD-007 ("never free text"), AI-005/AI-010 (fixed fields only), AI-006; make INT-041 show the full AI card for her OK |
| Disclosures | "No AI reads your messages today." | New AI-018 wording; privacy policy, data pages, help page, Twilio campaign all updated the same day (TERMS-008, TERMS-016) |

**Speaker notes:** Each row cites the requirement that blocks it today. D-080, D-373, D-394, D-395, D-412. Zero-retention is an agreement with the AI provider that isn't signed; no date in the plan. Build tasks T156 (question-bank intake) and T137 (real-AI test, fake data) would need new rows in `docs/test-design.md`.

### Slide 39: The trade-offs, plainly

**Two very different kinds of AI summary:**

| | AI writes the **homeowner's request** into a card for businesses | AI summarizes **businesses' quotes** for the homeowner |
|---|---|---|
| Who checks it | **The homeowner approves it** at "Here's what we'll send" before it goes | Nobody before the homeowner reads it |
| Risk | Low to moderate: it's her own job, in her words, OK'd by her | Higher: why it was turned off (D-373) |
| Legal reasons | Must not diagnose or price ("probably a bad capacitor, about $300") or it reads as Ask Hava advising on the work | Ask Hava's own words about a price can be a deception claim (FTC Act s.5, A.R.S. 44-1522); Section 230 likely doesn't cover text Ask Hava's software writes; "never edits, rounds or converts a quote" (WORD-011) |
| Status | Not in spec; doable with the changes on slide 38 | In spec as a switch, **off**; turning on needs a recorded decision and new privacy wording that day |

**Other trade-offs:**
- **Privacy:** AI on scrubbed text before zero-retention is allowed (D-412). Photos are not: a photo can show faces, house numbers, cars. Zero-retention first.
- **Safety:** AI must never downgrade a danger word. Keywords always run first (INT-002).
- **Trust:** a pro-sounding card the homeowner didn't really read could promise things she didn't say. Her approval step matters.
- **Cost and time:** small per request, but it adds build tasks and tests. The plan launches fine without it.

**Speaker notes:** D-373 (Memo 4 Findings 2 and 13), AI-018, PICK-003, PICK-004 (a summary adding a claim is rejected), WORD-011, WORD-022 (no "recommends work"), INT-041 (D-395), D-412, CARD-013. Recommendation for discussion: AI that writes the request card with homeowner approval is the safer first step; quote summaries stay off.

---

## WHERE THE PLAN ISN'T BULLETPROOF YET

### Slide 40: Fix before the build (top 6)

1. **Homeowners are never told it's free, or that the business pays a fee** (G1).
2. **Photos have no flow** (G2).
3. **"Here's what we'll send" isn't what's sent**, and the business "job page" is undefined (G3).
4. **$25 site-visit quotes** can undercut the fee model on big jobs (G4).
5. **Pest self-attestation** isn't wired into sign-up, activation or the daily license check (G5).
6. **No single ordered intake script**, so the heat path's order is a guess (G6).

**Speaker notes:** Details, files and fixes for every gap are in the ranked list below. Checked against main at `823c40c` (after PR #37, D-419 to D-428). PR #37 already fixed: the 4-segment first reply (now 2), terms acceptance on text picks via LIST and lapse texts, the "Reply OK" leftovers, TD-CON-18, and the Spanish go-live question (TXT-039).

### Slide 41: Fix in the same pass (next 7)

7. Updates by email: address never verified; which texts still come by text is unclear (G7).
8. Small holes in Terms acceptance on text picks; "18 or older" never said in the text path (G8).
9. Test journeys J1 and J2 still describe old steps (G9).
10. The owner's emergency call has no tool (G10).
11. Owner time estimate is stale (G11).
12. No "can't take it" button for businesses; backup doesn't cover every way a pick ends (G12).
13. "Strange noise or smell" answer skips the danger check (G13).

### Slide 42: Clean-ups and risks (lower)

14. No way to withdraw or fix a sent quote (G14).
15. Spanish-only texters get English with no explanation (G15).
16. Leaving Ask Hava isn't defined (G16).
17. "Different quote types" doesn't fit the not-picked email (G17).
18. Seven requirements still say "default pending Casey" (G18).
19. "Choose" vs "Pick" wording (G19).
20. Old fallback questions in the category file (G20).
21. OPS-033 points at a "weekly window" OPS-029 doesn't define (G21).
22. Risk: pick lapses at pilot start (G22).
23. Risk: paid introductions before any insurance (G23).

---

# Ranked gap list (detail)

Checked against main at `823c40c`. Each: where · why it matters · suggested fix.

### G1. Homeowners are never told it's free, or that the business pays a fee
- **Where:** CON-003 (D-419) removed the fee line from the first reply and says "the fee is shown again beside every pick (PICK-016)". PICK-016 (D-421) now allows **only** "By picking, you agree to the Terms" beside the button and "never … a key-terms summary". PICK-017's review lines don't mention the fee. The fee now lives only in the SMS terms, how-it-works page and homeowner terms (WEB-026), and the HELP common answer if she asks (CON-034).
- **Why:** two requirements contradict each other, so the builder will guess. A homeowner who texts in, gets quotes and picks may never learn that the businesses she's choosing between pay Ask Hava. That's the core of "honest relay" and was a deliberate fix before (D-371: no "small fee"). It also matters for the deception rules the legal memos raised.
- **Fix:** keep the cap off the page (Casey's call), but put one plain line on the quote page top or the review step: "Free for you. The business you pick pays Ask Hava a set fee ($25, $75 or $199)." Then make CON-003 and PICK-016 say the same thing.

### G2. Photos: no step, no channel, no use in the pilot
- **Where:** no INT requirement; PRIV-013 to PRIV-015, CARD-013, CARD-014, AI-016, PICK-022, TXT-036.
- **Why:** homeowners will send photos anyway (picture texts). Nothing says whether picture texts are accepted, how they reach the vault, or what the homeowner is told. Businesses can't see photos before quoting until zero-retention and AI screening exist, and AI is off. Casey's "reads photos like a pro" depends on this.
- **Fix:** decide now: (a) the pilot takes no photos: one fixed reply, "Thanks. Photos aren't shared with businesses yet; the business you pick can ask on the call"; or (b) an upload link after the summary, photos going only to the picked business after it pays. Write it as a new INT row.

### G3. The homeowner approves a summary that isn't the card; the business "job page" is undefined
- **Where:** INT-041 (trade + up to 3 answers) vs CARD-007 (scrubbed description, all card fields, timing, safety flags). CARD-007 says "never free text" but includes the scrubbed description, which is free text. Question-bank "details" answers are `card_field: false` (`question_bank.yaml`, e.g. hvac_repair `details`), so they go nowhere. JOB-010 ("1 adds the homeowner's next message to the job page") and CARD-009 ("details on the job page") point to a page no requirement defines.
- **Why:** her approval doesn't cover what businesses see; the final scrub (CARD-008) may cut part of her sentence silently; the "Anything else?" question collects answers nobody reads; businesses miss useful details ("It's a Trane").
- **Fix:** add a CARD requirement for the business job page (card + non-card answers through the CARD-008 gate + added details); make INT-041 show the full card text (by text if it fits, else a link); fix CARD-007's "never free text" wording.

### G4. Site-visit quotes pay $25 on any job size
- **Where:** FEE-004, FEE-043 (hourly with no estimate becomes site visit), `categories.yaml` `trip_fee_tier: 25` on all 39 categories, including hvac_replacement. B7 confirmed (D-418).
- **Why:** a business can quote "site visit needed" on a $9,000 AC replacement and pay $25 instead of $199. This attacks the pilot's $50 average-fee go line (PREBUILD section 7). FEE-033 only flags quotes near $500 and $1,500.
- **Fix:** add a scorecard line "share of picks that were site-visit quotes" per trade and per business, and a monthly flag like FEE-033. Consider $75 (the allowed maximum) for replacement categories.

### G5. Pest self-attestation isn't wired through
- **Where:** LIC-012 and LIC-047 (self-attested, D-417) vs CARD-015 (card only if the latest daily license check showed Active; "every license is checked daily"), LIC-006 (class "never typed in by the business"), BIZ-002 (identity code only to a public-record contact), BIZ-016 (activation needs a license or city-list match), LIC-043 (termite to a licensee "whose record shows" the category).
- **Why:** as written, a pest business can't activate or never gets a card, or the build guesses. Legally, D-402's reasoning leaned on cards going only to real Department licensees; self-attestation with no check weakens that, and pest is still launch-open.
- **Fix:** add one row: pest identity = city business-license list match plus the attestation; CARD-015's daily check doesn't apply to pest; termite and weed by attestation. Consider keeping pest "Coming later" until a lawyer looks at A.R.S. 3-3615.

### G6. No single ordered intake script
- **Where:** INT-016 (category check first), INT-030 (timing), INT-031 (heat text "followed by the INT-006 choice"), INT-006 (choice after SAFE or a "Today" answer), INT-039, INT-022 (area check; no ZIP question wording anywhere), INT-028, INT-027 ("last question"), INT-041, TXT-018.
- **Why:** on the heat path (the most common July request) it's unclear whether the emergency choice comes before the timing question. The builder will pick an order, and the golden tests (OPS-040) will lock it in.
- **Fix:** one table in `docs/spec/intake.md` with the exact order and wording for each path: regular, heat, gas/CO/electrical/water, night, web chat.

### G7. Updates by email: unverified address, unclear scope
- **Where:** TXT-018 (D-422): email "checked for format", a hard bounce switches back to text; "with email only, safety texts, codes and direct replies … still go by text"; PICK-029 sends pick codes to an email from her conversation when she can't be texted.
- **Why:** a mistyped address that belongs to someone else gets her quote page link and, if she later opts out of texts, her pick codes. With "email only", it's unclear whether the who's-calling (TXT-027) and missed-call (TXT-030) texts, which carry the masked number, still go by text.
- **Fix:** confirm the address with a one-tap link before it's used for codes; list exactly which texts "email only" keeps (recommend: who's-calling and missed-call stay texts).

### G8. Small holes in Terms acceptance on text picks
- **Where:** PICK-016 puts the Terms line on texts that *offer* a text pick (TXT-031, PICK-046), but PICK-015 accepts "2 YES" from her phone at any time, and the quote page shows "or reply 2 YES" beside each button. The first reply no longer says "18 or older" (D-419); that rule is only in the Terms, the web chat start and INT-038.
- **Why:** a "2 YES" sent after only the first-quote text is a pick with no Terms line in front of it. The record of acceptance is weaker for exactly the picks that skip the page.
- **Fix:** if a "<n> YES" arrives and no text with the Terms line was sent for that request, reply once with the line and "Reply 2 YES again to pick." Add "Terms (18+)" to the first-quote text's link line.

### G9. Test journeys still describe old steps
- **Where:** `docs/test-design.md` section 5. J1 step 3 ("ZIP 86403; the job opens") skips the timing question, question bank and summary step (INT-030, INT-039, INT-041). J2 step 2 says "Casey approves in the owner portal; activation needs all three", but BIZ-016 activates automatically unless flagged.
- **Why:** tasks are built against these journeys; wrong journeys mean wrong code or red builds.
- **Fix:** refresh J1 and J2 in the next docs pass.

### G10. The owner's emergency call has no tool
- **Where:** OPS-028 ("places one call from the Ask Hava number"), CALL-019 (automated safety call off, B15).
- **Why:** with the automated call off, every unhandled safety alert lands on the owner, who has no way to call from the Ask Hava number; he'd use his own phone and expose his number.
- **Fix:** add an owner-portal "Call from the Ask Hava number" button that rings the owner's phone and bridges to the homeowner, or change OPS-028 to an email or web message.

### G11. Owner time estimate is stale
- **Where:** `docs/for-casey-review.md` (~lines 355–361: "phone only buzzes for four things", 30-minute window, 25–35 min/week [sim]); OPS-036 and OPS-029 now push every HELP at once (D-348) plus the 9 a.m./3 p.m. push; OPS-033 still schedules a "weekly window and mid-week check".
- **Why:** the 2-day and 24-hour promises (CON-018, BIZ-047) assume he can answer around a full-time day job with no backup (OPS-002).
- **Fix:** re-estimate by counting message types × expected volume (no simulation needed), and confirm the urgent 24-hour promise before launch.

### G12. Businesses can't decline a pick; backup doesn't cover every ending
- **Where:** PICK-047 says the backup applies when the business "declines", but no requirement gives a decline action. Endings the backup doesn't name: FEE-041 (fee limit ends the pick), FEE-020/JOB-014 (card not fixed in 1 hour), FEE-018 (Stripe down 4 hours), CON-029 (homeowner's CANCEL withdraws a pick).
- **Why:** a booked-up business makes the homeowner wait 4 business hours; the backup may not fire when it should.
- **Fix:** add a "Can't take it" button on the Confirm page (no fee, ends the pick at once); say the backup applies to every pick that ends without a Confirm.

### G13. A numbered "smell" answer skips the danger check
- **Where:** `question_bank.yaml` hvac_repair symptom option 5 "Strange noise or smell"; INT-002 matches words in the message (a reply of "5" has none); INT-042 safety prompts exist only for boats, pools and septic.
- **Why:** a furnace or AC "smell" can be gas or burning wiring. Picking 5 gets no safety text.
- **Fix:** add a follow-up: "What kind of smell? 1) Gas or rotten egg 2) Burning or electrical 3) Musty or other", with 1 and 2 sending the INT-004 or INT-034 text.

### G14. No way to withdraw or correct a quote
- **Where:** PICK-002 stores an "expiry" no rule uses; WORD-011 says "only the business can change its quote" but there's no edit action.
- **Why:** a $45 typo for $450 sets a $25 fee and misleads the homeowner; a business that books up can't pull out.
- **Fix:** allow edit or withdraw before any pick, shown on the quote page as "updated" or "withdrawn"; define expiry or drop it.

### G15. Spanish-only texters get English with no explanation
- **Where:** TXT-039 (D-427): Spanish conversations off at launch; Spanish safety texts stay on.
- **Why:** a homeowner who writes in Spanish gets a Spanish safety text, then English questions she may not understand, with no word about why.
- **Fix:** one fixed Spanish line (back-translation checked like the safety texts) after the first reply: "Por ahora seguimos en inglés. Responda AYUDA si necesita ayuda."

### G16. Leaving Ask Hava isn't defined
- **Where:** `docs/legal/business-terms.md` section 11 ("leave at any time in the portal"); no BIZ row; TERMS-021 retention days still undecided (D-390).
- **Why:** open quotes, waiting picks, saved cards and team sign-ins need a rule.
- **Fix:** a BIZ row: leaving withdraws open quotes, ends waiting picks (backup applies), removes saved cards, ends sessions, keeps the ledger 7 years.

### G17. "Different quote types" doesn't fit the not-picked email
- **Where:** PICK-038 vs PICK-026's sentence "the picked quote was <lower than / about the same as / higher than> yours".
- **Fix:** add the exact sentence: "…the picked quote was a different type of quote from yours."

### G18. Seven requirements still say "default pending Casey"
- **Where:** AI-008, BIZ-041, FEE-004, LIC-012, LIC-015, LIC-046, PICK-048; all confirmed in D-418.
- **Fix:** remove the tag and cite D-418.

### G19. "Choose" vs "Pick"
- **Where:** PICK-009 ("Choose button"), PICK-015 ("or reply <n> YES" beside every Choose button), PICK-017 ("You're choosing one business…"), TXT-031 ("To choose, reply…" then "By picking…"), PICK-047; PICK-016 now says "Pick button".
- **Fix:** one verb for the button, the review step and the texts.

### G20. Two sources of intake questions
- **Where:** `docs/catalog/categories.yaml` `intake_questions` ("short fallback prompts kept from K2b") vs `question_bank.yaml`.
- **Fix:** delete them or say exactly when a fallback is used.

### G21. OPS-033 cites an undefined "weekly window and mid-week check (OPS-029)"
- **Fix:** define the windows in OPS-033 itself, or drop them now that HELP pushes at once.

### G22. Risk: pick lapses at pilot start
- **Where:** PREBUILD section 1 "assumed"; D-400 (17% lapses [sim]); PICK-048 (auto-confirm waits on Stripe's written yes); BIZ-041 (push app is the last build stage).
- **Why:** on day one the only lapse protections are email, hourly reminders and the backup choice. The "picks confirmed ≥85%" go line is at risk.
- **Fix:** send the Stripe question now (PREBUILD item 8); during recruiting, walk each business through the sample-pick alert test in person.

### G23. Risk: paid introductions before any insurance
- **Where:** D-426 (insurance "later, after the business is fully working"); PREBUILD section 4 no longer lists insurance before the first paid introduction.
- **Why:** the earlier recommendation was tech E&O plus cyber bound before the first paid introduction; texting lawsuits and a data breach are the risks the legal memos flagged.
- **Fix:** none needed in the spec; just a conscious yes from Casey that the pilot runs uninsured, or a quote before Gate C.
