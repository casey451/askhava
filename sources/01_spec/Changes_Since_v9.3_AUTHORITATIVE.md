# Ask Hava: changes since Technical Blueprint v9.3 (authoritative delta)

**Owner:** Casey Solomon · **Compiled:** October 27, 2026 (updated after the ChatGPT build review and Grok legal research) · **Status:** decisions final unless marked "attorney" or "verify"

## How to read this file

Technical Blueprint v9.3 is the base spec. This file lists every change made since then. **Where the two disagree, this file wins.** Where this file and any other document in the package disagree, this file also wins. The known conflicts are:

| Older text | Says | Authoritative value |
|---|---|---|
| `03_business_and_design/All_Services_Expansion_and_Role_Play_Changes.md`, Fees section and its decision checklist | Ongoing services count as **2 months**; quote window and other items listed as open | Ongoing services count as **4 months** (section 3). All decisions are settled (section 3) |
| `03_business_and_design/Ask_Hava_Quotes_Brand_and_Copy_Brief.md` | Ask Hava is a directory plus quotes; invite ~690 directory listings; "Your first win is free" | The old directory is **deleted**. Ask Hava is the quote service only. Free first win only for businesses joining in the **first 90 days** |
| `03_business_and_design/Havasu_Home_Quotes_Business_Plan_and_Blueprint_v9.md` | "Up to 4 contractors per trade"; refund needs "2 attempts on 2 different days"; pick by replying YES; fees $45/$59/$149; brand "Havasu Home Quotes" | Every qualified opted-in business gets every matching card (no cap); 2 attempts at least 3 hours apart within 72 hours; pick by a code texted to the homeowner's phone (`2 YES` by text still works); fees in section 3; brand "Ask Hava" |
| `01_spec/hhq_twin/` (digital twin) | 5 trades, old fees, brand "Havasu Home Quotes" | Twin is updated in build Stage 1 to 25 categories, new fees and all changes here |
| Website prototype, brand brief, revision notes | "Every quote shown, never ranked"; "Insurance on file: Yes/No"; "skip tenant requests"; texting "cancel" to close a job | Wording and rules in section 7 win |
| Any copy of the build plan exported before the Oct 27 reviews (the copies in `02_build_plan/` are updated) | Sessions flip "passes"; K9 runs real app tasks; build 1–2 weeks | Runner-only completion, practice-task rehearsal, revised estimates (section 8) |
| Grok's review, "Conflicts" item 1 | Goal possibly $20k–$50k **a month** | Goal is **a year** |
| Grok's review, "Conflicts" item 2 | No homeowner details may go to any AI | Zero-retention AI is allowed, **after** a plain-code scrub (section 3) |

---

## 1. Brand and domain

- **Name:** Ask Hava (homeowners) and Ask Hava for Business. Legal entity: **Ask Hava LLC** (to be formed).
- **Domain:** askhava.com, already owned. Registrar GoDaddy (paid through May 7, 2029), DNS already on Cloudflare (checked Oct 26, 2026 by DNS/RDAP lookup).
- **The old askhava.com directory site is deleted.** Casey deletes the old Railway project (site, database, scrapers) and the old GitHub repository himself. Nothing is migrated. Google Search Console stays verified.
- **Stage 0.5 (before texting registration):** a simple Ask Hava page with what's coming, privacy policy, SMS terms and the opt-in description goes live on askhava.com, because Twilio's reviewers check it. Old directory URLs show this page instead of errors.
- **Hava is not a person.** All copy uses the service voice ("Ask Hava: …"). FAQ: "Ask Hava is a local service. Our texts are automated, and a real person reviews anything flagged."
- **Links inside texts stay on askhava.com** (for example askhava.com/help/roc-complaint), which then links onward. This keeps the texting registration's link domain consistent.

## 2. Services: any local service business can opt in

- About 95 kinds of service business in 10 groups (pool/spa/water, yard/outdoor, home systems/repair, interior/remodel, cleaning/property care, vehicle, boat/lake, pets, moving/hauling, events/tech). The financial model uses **25 launch categories** (45% repairs, 24% recurring, 31% other one-time).
- **Launch set:** no-license categories, ROC categories, plus pest control. **Left out until later:** childcare, home care, massage, mobile hair and nails, mobile vets, catering, towing, property management.
- **A category catalog replaces the 5-trade list.** Each category stores: job types, license rule (none / ROC class / other state agency), recurring yes/no, intake questions, fee rule (and a set tier for trip-fee quotes), after-hours emergencies yes/no, keyword list. Adding a category is a settings change, not new code.
- **License rule per category:**
  - **No state license:** city business license only. The app checks the business name against the city's published active-license spreadsheet (lhcaz.gov, XLSX), plus the identity chain (section 4, Grok fix 6).
  - **ROC class:** e.g. plumbing R-37, HVAC R-39, electrical and solar R-11, pool repair R-6, hardscape and irrigation R-21, roofing R-42, painting R-34, flooring R-8, tile R-48, window film R-65, water softeners R-54, appliances R-63, low-voltage R-67. **Checked from the ROC's daily downloadable lists, plus a live lookup at each pick.**
  - **Other state agency:** pest control and weed spraying (AZ Dept of Agriculture), alarm systems and home inspectors (Board of Technical Registration), etc. Each needs its own lookup.
- **Handyman rules (A.R.S. 32-1121(A)(14) and (D)):**
  - Handyman quotes must be **under $1,000 total** (labor plus materials, top of any range), with **no permit** and no job splitting.
  - **Gas work is never handyman work at any price**; neither are hardwired smoke alarms or fire sprinklers. A gas keyword list overrides the AI's reading.
  - **Permit-type jobs** (water heater replacement, panels, new circuits, re-roofs and similar) are never sent to handymen. (Grok finding 8; adopted.)
  - Every handyman quote card shows: **"Not a licensed contractor. Only for jobs under $1,000 total with no permit."**
  - The $1,000 limit applies only to construction-type work. A $1,200 car detail or move is not refused.
- **Weed spraying:** "pre-emergent" or "weed control" requests go only to licensed pest-control businesses (A.R.S. 3-3613). A gardener may spray incidentally on the same visit.
- **Gardeners** are ROC-exempt under 32-1121(A)(16).
- **Insurance** for no-license categories: shown on the quote card as **"Insurance on file: Yes / No."** Not required.
- **Location isn't always a house:** boats, cars and RVs may be at a marina, storage lot or ramp. Cards show town and kind of place only.
- Requests that fit no open category get an immediate reply and are logged as recruiting demand.
- **Texting registration** describes local services in general (use case Customer Care), with new sample messages. See section 7 for the lead-generation determination and the campaign wording.

## 3. Final decisions (settled October 27, 2026)

| Decision | Choice |
|---|---|
| Profit goal | $20k–$50k **per year** (Grok's sim: about 4.7 requests/day for $20k/yr, 14.3/day for $50k/yr, simulated) |
| Homeowner details and the AI | Allowed only to the **zero-retention** Claude API (Haiku 5.5). A plain-code scrubber runs **before every AI call** (names after "this is / my name is", street-number patterns, 7+ digit runs, emails); the final text gate still runs on every card. Until the zero-retention agreement is signed, photos stay off job cards |
| Fees | **$25** if the chosen quote is under $500; **$75** for $500–$1,500; **$199** over $1,500. No percentage cap |
| Recurring services | Counted as **4 months** of their price (a $150/month pool account counts as $600, so the fee is $75) |
| Ranges and trip fees | A range uses its midpoint. A trip-fee-plus-estimate quote uses the set tier for its category |
| Tier-line shading | Owner portal flags businesses whose quotes keep landing just under $500 or $1,500. The day-3 PROBLEM reply catches a final bill well above the quote |
| Legal check on tiered fees | Kept as a fixed **introduction fee by job size**, never a percentage (section 7.1, item 2). A flat fee per job type is a ready settings switch |
| Insurance (no-license categories) | Shown yes/no, not required |
| License checks | ROC daily lists plus a live lookup at each pick |
| Quote window | 24 hours, counted **every day** (not weekdays only) |
| Free first win | Only for businesses that join in the **first 90 days** after launch |
| Early-cancel partial refund for recurring | **Not adopted** (Grok's sim: not favoured on money; revisit only if the hand-run test shows early stops hurt trust) |
| Repeat work from the same customer | Always free |
| Build timing | Build in test mode alongside the hand-run test |
| Usage limits | Pause and wait for the reset; no pay-per-use overflow |
| App host | **Railway Pro** if its Postgres Backups tab shows point-in-time recovery; otherwise **Render** |
| Backup person | None for now. Alerts wait for Casey; safe mode pauses risky actions after 24 hours without him |
| Website design | ChatGPT's "Clearwater Modern" prototype is the design reference. No more ChatGPT rounds; the build applies the 8 revision notes itself (section 6) |
| Backup Twilio number | **Rejected.** Instead: a site status banner and web chat as the fallback channel |

## 4. Role-play changes (16) carried into the spec

From the role-play against v9.3 (full table in the expansion doc, "Role-play changes"):

1. First text also says the business you pick pays us a small fee.
2. Homeowner is texted at the **first** quote, with a note more may come; can pick any time.
3. Businesses get up to **24 hours** to quote (every day).
4. One update text every few hours ("You now have 4 quotes"), not one per quote. **The gap is counted from delivery time.**
5. Losing businesses get "This job is no longer active." **Sent to every quoter whenever the job ends**, including anyone who quoted after the pick. Quotes stop being accepted once a pick is confirmed.
6. Emergencies: the safety text says why ("a rotten-egg smell can mean a gas leak") and is sent **before** the consent text. Once they reply they're safe, ask if the repair is urgent, mention after-hours usually costs more, and let them choose emergency or regular.
7. Businesses can tick "after-hours emergencies."
8. Spanish: Spanish texts; businesses can tick "We speak Spanish" (shown on their quote card). **"Homeowner prefers Spanish" is shown only to businesses that ticked "We speak Spanish"** (section 7); no business can filter or skip on language. **Every new text kind gets a Spanish template.**
9. Tenants: requests go out normally; the card says **"Landlord approval may be needed."** The "skip tenant requests" option is **dropped** (section 7).
10. Business questions about a live pick get a short reply with the Confirm link and what happens next.
11. Complaints: apologize; explain the business is independent; send the business's own phone, job details and the ROC complaint link (via an askhava.com redirect). Pause the business only after 3 complaints in 90 days.
12. Business messages ("get on your list") are detected and answered with the sign-up link. **Intent phrase list widened, with an AI second opinion on scrubbed text.**
13. Identity check chain: text code → voice call reading the code (landlines) → email on the state record → automatically mailed letter (Lob) as last resort.
14. License read-back at sign-up ("Your R-39 covers AC repair, heating and system replacement. Which do you want?"). Extras allowed only if the license covers them or they need no license.
15. STOP confirmation says "To use us again, text START." A STOP returner who texts again is treated as a new request.
16. Any local service, including recurring ones (section 2).

**Already matched v9.3, no change:** cancel refunds a business only if they haven't talked to the homeowner; confirm → pay → masked call; automatic refund when the homeowner can't be reached (2 attempts ≥3 hours apart within 72 hours); private quote page that keeps adding quotes, each with a Choose button.

Plus two more counted in the "18 changes": the category catalog with licenses (section 2) and the fee tiers (section 3).

## 5. Grok v9.3 simulation: our verdicts

Grok ran its own simulator against v9.3 (report and code in `04_reviews/grok_v93_simulation/`). We checked each finding against v9.3's text and the twin's code. **Grok's results are simulated, not real-world.** Several of its failures came from v9.3 fixes that its simulator left switched off by default.

**Real gaps: fixed in the spec (9)**

| # | Gap | Fix now in spec | Grok ref |
|---|---|---|---|
| G1 | Masked calls route to the most recent job and ignore the PIN | A number from a pool per active job, plus PIN; with more than one job, route only by PIN; after 2 bad PINs read back first name and street before connecting | 5 |
| G2 | Jobs left in Intake forever when the homeowner stops answering | Reminder at 24 h, close at 72 h ("Text us any time to restart"). Same timeout pattern on every waiting state | 7 |
| G3 | "Never mind" not treated as cancel; wrong job closed when several are open; cancel-like message opens a new job | Cancel-intent phrases in English and Spanish (never mind, no longer need, ya no, "please cancel my request"); with more than one open job ask "Close which? 1) … 2) … 3) both"; never open a new job from a cancel-like message; refund anything charged after a cancel. **A text that is only one opt-out word, including "cancel," is an opt-out, not a job cancel** (section 7) | 4 |
| G4 | A 1–2 word first message gets a reply without the required disclosures | The first text to any new number is always the full consent text, on every path | 9 (T1) |
| G5 | Opt-out words are English only | Spanish opt-outs (PARAR, ALTO, CANCELAR, BAJA, etc.) honored like STOP | — (found in our code review) |
| G6 | Owner approval can activate a business with no completed identity chain | Approval never activates on its own; the identity code is still required. Unlicensed businesses also need a city-license-list name match | 3 |
| G7 | Second free first win through a second name | Free-win entity key = ROC # **or** city-list entity **or** Stripe card fingerprint, checked when the free win is used | 12 |
| G8 | Complaint text links straight to the ROC site | Link goes through askhava.com/help/roc-complaint | — (registration link-domain consistency) |
| G9 | Nightly reconcile alerts on an unknown Stripe charge but books nothing | Reconcile from Stripe's event list; auto-match to a job or auto-refund (refund if the job is closed); keep the alert | 13 |

**Also adopted (cheap, from Grok):**
- Quiet hours applied in the **stricter** of Arizona time and the recipient's area-code time zone (Grok 9, T2).
- Permit-type job list never sent to handymen; gas keywords override the AI (Grok 8). See section 2.
- Quote list numbers stay in **arrival order** so "1 YES" can't change meaning between texts (Grok 15).
- Details template prints the first name once (Grok 17). **Verify** against v9.3 templates in K3.

**Clarifications written into the spec (6):** losers notified at job end (role-play 5); update gap counted from delivery (role-play 4); tenant and Spanish tags on cards; business-intent detection widened; any business question about a live pick answered; license read-back; Spanish templates for every new text kind.

**Not real gaps in v9.3 (Grok's sim had the fix off):**
- Grok 2, "(ask for Ruth)" surviving on a card: v9.3's final text gate already removes "ask for Name." Keep the weekly 10-card spot check.
- Grok 10, late safety text: v9.3's emergency word list already includes "sparking" and sends the safety text before consent. Keep stem matching in the rebuild.
- Grok 11, restore re-sends texts: v9.3's replay already checks what was sent and uses derived IDs. Keep the restore drill.
- Grok 1 / Conflict 2 (AI sees raw text): resolved by the zero-retention decision plus the scrub-first rule (section 3).

**Rejected:** backup Twilio number on a second provider (Grok 6). Instead: a hidden-by-default site status banner ("Texting is temporarily unavailable. You can still use the chat on this site and your quote page."), web chat on askhava.com, email fallback, and strict 10DLC compliance.

**Grok's legal flags:** the tiered fee as a possible commission, and the handyman cap across categories. Both are decided in section 7.1 (items 2 and 5).

## 6. Website: Clearwater Modern prototype plus 8 revisions

The prototype in `03_business_and_design/website_prototype/` (34 pages, English and Spanish, style guide, docs) is the design reference. axe-core 4.10 found no violations at 1280px; one issue at 320px (item 7). The build applies these changes (detail in `Ask_Hava_Prototype_Revision_Notes_for_ChatGPT.md`):

1. Remove all directory, events and chat links and the "More than quotes" section; footer column becomes "Homeowners"; tagline "Local quotes. Your choice."
2. Pricing copy: ongoing = 4 months (pool example moves to the $75 row); "Launch offer: your first win is free when you join in the first 90 days."
3. Quote cards: handyman disclosure line; "Insurance on file: Yes/No" for no-license categories; ROC line and "Check it yourself" link for licensed trades.
4. Service voice, no first-person Hava.
5. Spanish consent draft added (marked for native-speaker review), English kept.
6. Links inside texts stay on askhava.com.
7. 320px fix: wide-table scroll containers get `tabindex="0"`, `role="region"`, `aria-label`.
8. Status-banner component (hidden by default) and the "Is Hava a person?" FAQ.

**Target:** WCAG 2.2 AA. Screen-reader, 200% zoom and Lighthouse checks have **not** been run yet.

## 7. Legal and texting changes (October 27, 2026)

From Grok's legal research (`04_reviews/Grok_Legal_Research_Findings.md`), checked by Claude where the sources could be reached. These are research notes, not legal advice. The 18 questions that were open have working answers in section 7.1; `07_legal/Legal_Decisions_Record.md` lists them for an optional later attorney review. Every item below is built in; items marked **gate** must also be cleared before any business signs, any live charge or any real homeowner request.

**Texting**
- **Opt-out words (FCC 26-67, published Oct 26, 2026, effective 30 days later):** a text that is only one of *stop, quit, end, revoke, opt out, cancel, unsubscribe* (any case, plus the Spanish words already listed) is an opt-out from all texts. Twilio also stops delivery to that number automatically. The reply confirms and says "To use Ask Hava again, text START." Honored at once (the rule allows up to 10 business days).
- **Closing a job:** "CLOSE 1042," the close button on the quote page, or a longer cancel-intent message ("please cancel my request," "never mind") closes a job. The word "cancel" alone never does.
- **Twilio "lead generation" rule: Ask Hava does not violate it (decided Oct 27; no pre-clearance request).** Twilio's rule and its rejection notice (error 30951) target four things:
  1. collecting consumer information to sell or distribute "to other businesses as sales leads";
  2. acting "as an affiliate or intermediary" for unrelated third parties;
  3. shared or resold consent;
  4. a terms or privacy policy that mentions sharing or selling opt-in information.

  Ask Hava does none of these:
  - consent is given only to Ask Hava, for Ask Hava's own texts about the person's own request;
  - no phone number or opt-in is shared, sold or transferred;
  - businesses never text homeowners on Ask Hava's campaign, and calls go through masked numbers;
  - the homeowner, not Ask Hava, chooses which single business gets a first name and address, after asking for that business.

  **The remaining risk is a reviewer misreading the model**, and a rejected campaign can't be resubmitted. So:
  1. **Register only when the full site is live**, including the request page with its consent text, the privacy policy and the SMS terms, so the reviewer sees exactly how opt-in works.
  2. **The campaign description** uses the wording below and never uses the words "leads," "lead generation," "referral network," "marketplace of providers" or "connect you with."
  3. **The sample messages** are real Ask Hava texts: the consent text, "your first quote is in," the pick confirmation and the STOP reply.
  4. **The privacy policy and terms** must never say numbers or opt-ins are shared, sold or transferred.
  5. **Fallback if Twilio ever objects:** the service still works through the website. Requests come in by web chat, quotes appear on the private page, and updates go by email. The app keeps texting behind a switch so it can run web-and-email only.

  **Campaign description (draft):** "Ask Hava is a local service in Lake Havasu City, AZ. People text us, or start on askhava.com, to ask for quotes on a home or local service they need. They opt in by texting us first and replying OK to our consent message, which states message frequency, rates, HELP and STOP. We text them only about their own request: confirmations, when quotes arrive, and the business they choose. We never share, sell or transfer phone numbers or opt-in consent, and businesses never send messages on this campaign. Use case: Customer Care, Low-Volume Standard."
- **Privacy policy must include:** "No mobile information will be shared with third parties or affiliates for marketing or promotional purposes."
- **Business consent (gate):** business sign-up includes a checkbox giving written (E-SIGN) consent to job-card texts and automated calls at the number they enter. Version and timestamp stored. Homeowner texts stay purely about their request: no promotions, no cross-selling.
- **Identity voice call** opens with "This is Ask Hava LLC" and gives a callback number (47 CFR 64.1200(b)).
- **Records:** consent, opt-in wording version, STOP/HELP/START events and message logs kept at least 4 years (5 for margin) after the last message to a number.
- **Quiet hours** (optional hardening, adopted): no non-urgent texts after 8 p.m. in the stricter of Arizona time and the recipient's area-code zone.
- **Content filter:** homeowner job descriptions are screened for carrier-restricted content before they go on a job card.

**Copy and claims**
- The site and ads never say or imply Ask Hava does the work: "Get quotes from licensed plumbers. Ask Hava is not a contractor and does not perform, bid on or supervise work." (A.R.S. 32-1165, checked.)
- "Every quote shown" becomes **"Every eligible quote shown, in the order received."**
- "Insurance on file: Yes/No" becomes **"Insurance (business-reported): Yes/No."**
- "Your number is never shared" stays, with: "Your first name and address go only to the business you pick."
- License line: "ROC #___ showed Active on the ROC website when we checked on [date]. Check it yourself."
- Business-side claims such as "$25 is about what one online lead costs" are dropped unless a source is cited on the page.
- Never say "vetted" or "background-checked." Say exactly what's checked.
- Key terms (fee, STOP/HELP, privacy) also appear in Spanish wherever Spanish copy markets the service.
- Safety text stays generic: "If you smell gas, leave now and call 911 and your gas company from outside. Ask Hava is not an emergency service."
- Bot disclosure: "Ask Hava is a local service. Our texts are automated." If anyone asks whether they're talking to a person, the answer is truthful.

**Quote cards and AI**
- The quote card shows the business's own wording next to any AI-formatted summary, and the AI may not add claims (price, scope, warranty) that aren't in the original.

**Matching and routing (category list)**
- Pest control, and any weed job involving pre-emergents or advertised weed control, goes only to Arizona Dept of Agriculture pest-management licensees (license number shown).
- Alarm installs and monitoring go only to Board-certified alarm businesses that also hold an ROC low-voltage class. Camera installs go to R-67 (or broader) licensees, or to handymen under $1,000 with no permit. Home inspections go only to Board-certified inspectors.
- Septic pumping goes only to haulers with an ADEQ septage-hauler vehicle license.
- Moves leaving Arizona are refused (interstate broker rules).
- Home watch and turnover cleaning never include rent collection, finding tenants or leasing.
- **Handyman:** a split-job check blocks a second handyman job from the same business at the same address with overlapping scope inside a short window. The $1,000 cap applies to construction-type work in any category; it does **not** apply to non-construction services such as detailing or moving (Grok suggested a broader cap; not adopted).
- Class mapping from Grok (to confirm in K2b): softener, RO and misting R-54; window film and solar screens R-65; cameras R-67; gutters R-45; insulation R-40; pavers R-21; epoxy floors R-8 or R-34. The previously unresolved jobs (garage doors, holiday lights, dryer vents, freestanding appliances, junk removal) are decided in section 7.1, items 4 and 13.

**Terms, payments and privacy**
- **Business terms:** each automatic-refund trigger, timing and method exactly as coded (a test checks they match); a non-discrimination clause; the card-charge authorization ("I authorize Ask Hava to charge this card $25/$75/$199 by quote tier, ongoing services counted at 4 months, each time I confirm a homeowner's pick; first win free if I join in the first 90 days; refunds per the Terms"), with version and timestamp stored. Stripe SetupIntent for off-session use.
- **Homeowner terms:** linked in the first text; "Reply OK to agree to the Terms and start"; the OK is stored; the quote page also says "By choosing, you agree to the Terms." Users must be 18 or older. No arbitration clause or class waiver.
- **Privacy policy (CalOPPA plus CTIA):**
  - what is collected;
  - who it's shared with: the picked business, and the texting, payment, AI, mail and email providers;
  - that the AI provider works under a zero-retention agreement;
  - how to review or change data;
  - how changes are announced, and the effective date;
  - Do Not Track;
  - the no-sharing statement above.
- **Photos:** location data stripped; optional; "avoid photos of people"; not sent to the AI unless needed.
- Never collect SSNs, driver's licenses or tax ID images from businesses; use EIN or ROC number. Keep a short incident plan.
- Stripe account described as "local service marketplace: introduction fees charged to service businesses."

**Business setup (gate)**
- LLC: use a commercial registered-agent service with a Maricopa County address (no newspaper publication; home address stays private). See 7.1, item 17.
- Lake Havasu City business license; home-occupation application if run from home; never publish the home address (use a mailbox address).
- USPTO search for "Ask Hava" in Classes 35 and 42 before spending on the brand.

### 7.1 Working answers to the 18 legal questions (decided October 27, 2026)

Casey's instruction: work out the best solution for each open question so nothing waits on outside approval. Each answer below is the **safest workable choice** and is built in now. They are research-based judgments, not legal advice. An attorney can still review them later, and each has a fallback the code is built to switch to without a rebuild.

| # | Question | Decision (in effect) | Built-in fallback |
|---|---|---|---|
| 1 | Is Ask Hava a contractor (A.R.S. 32-1101)? | **No, and the design keeps it that way.** Every quote is the business's own offer, and the contract is between the business and the homeowner. Ask Hava never prices, bids, negotiates, schedules, supervises or guarantees work. Quote refusals are worded as **eligibility rules** ("this business isn't eligible for this job type"), never as Ask Hava judging the work. Terms, cards and site: "Ask Hava is not a contractor and does not perform, bid on or supervise work." | — |
| 2 | Does a size-tiered fee look like a commission? | **Keep $25 / $75 / $199, framed as a fixed introduction fee by job size.** It's published in advance and is never a percentage. It's charged when the business confirms the introduction, not when the job is done or paid, and isn't refunded or adjusted based on the final bill. Terms and site say "introduction fee," never "commission" or "share." | A settings switch to a flat fee per job type, with no code change |
| 3 | Does advertising pest-control quotes make Ask Hava a "pest management" business (A.R.S. 3-3615)? | **Pest control stays available but is never advertised.** No pest page, ad, tagline or seasonal banner. A homeowner who asks gets routed, and only Dept of Agriculture licensees receive the card. | Remove the category |
| 4 | Unresolved license classes | **Garage doors:** off at launch. **Dryer-vent cleaning:** allowed as cleaning (no license); rerouting or replacing a vent is a handyman job under the $1,000 cap. **Holiday lights:** plug-in temporary lights allowed (no license); any new outlet, circuit or hardwiring goes to R-11 electricians. **Freestanding appliance repair:** R-63 licensees, or handymen under $1,000; never gas, plumbing or electrical lines. **Weekly pool cleaning:** no license; any repair or equipment work goes to R-6 or a handyman under $1,000; gas heaters only to licensed businesses. | Each is one row in the category list |
| 5 | Handyman liability (split jobs, the cap) | Split-job check; the $1,000 cap on construction-type work; the "Not a licensed contractor…" line on every handyman card. Business terms make handymen **certify each quote** is a full-job price under $1,000 with no permit and no gas, smoke-alarm or sprinkler work. Breaking that removes them. | — |
| 6 | Twilio "lead generation" | Not a violation (section 7, Texting); register carefully once the full site is live | Web-and-email mode |
| 7 | Do job-card texts to businesses need written consent? | **Get it anyway:** the sign-up checkbox with E-SIGN consent, version and timestamp stored | — |
| 8 | Is "Reply OK" enforceable for homeowner terms? | **Two steps:** "Reply OK to agree to the Terms," and also "By choosing, you agree to the Terms" with a link on the quote page before any pick. **No arbitration clause and no class waiver**, which avoids enforceability fights. Arizona law. | — |
| 9 | Section 230 and AI summaries | The business's **original wording is always shown** with the summary. The AI may only shorten or format, never add a price, scope, warranty or claim. A business can view its quote card as homeowners see it. | — |
| 10 | Safety-text liability | Generic text only, no monitoring promises: "If you smell gas, leave now and call 911 and your gas company from outside. Ask Hava is not an emergency service." Terms repeat that Ask Hava isn't an emergency service. | — |
| 11 | Negligent-referral liability | Say exactly what's checked, never "vetted" or "background-checked." Terms include the independent-business statement and a warranty disclaimer, and limit liability to fees paid (homeowners pay nothing). Complaint-pause rule. **General liability plus tech E&O insurance** before Gate C. | — |
| 12 | Sales tax (TPT) on fees | **Get an Arizona TPT license** before the first live charge (it covers state and city), and have a tax preparer confirm the classification at the first filing. Fees are set **tax-included**: if any tax applies, Ask Hava pays it from the fee, so businesses' prices never change. | — |
| 13 | City trash franchise and junk removal | **Junk removal is off at launch.** Before switching it on, Casey (or a browser session) reads Lake Havasu City Code ch. 8.04 or asks the city. **Estate cleanouts and donation pickup** stay on, because they aren't refuse collection. | One row in the category list |
| 14 | ROC lists and the city spreadsheet | **Verification only, never recruiting.** Recruiting is through Facebook group posts. Use the ROC's posted download lists daily and a throttled single lookup at each pick. Before launch, read the ROC website terms in a browser; if a commercial-purpose statement (A.R.S. 39-121.03) is required, file it. | — |
| 15 | Tenant tag | "Landlord approval may be needed"; the "skip tenant requests" option is dropped | — |
| 16 | Card authorization wording | As drafted in section 7. Stripe SetupIntent for off-session use; agreement version and time stored; a receipt is emailed for every charge; the charge description names the request number | — |
| 17 | Statutory agent | **A commercial registered-agent service with a Maricopa County address.** No newspaper publication, and Casey's home address stays off the public record. | — |
| 18 | Trademark | **Search USPTO for "Ask Hava" and "Hava" (Classes 35 and 42) before any brand spending.** File a federal application in Class 35 after the LLC exists and the search is clear. | Rebrand before launch if the search finds a conflict |

## 8. Build-process changes (from ChatGPT's build stress test)

Detail is in `02_build_plan/`; summary:
- A real runner moves the queue: starts each next task when a pull request merges (the scheduled task's GitHub trigger), with an hourly safety net (superseded detail: see 8.2 and BUILD_BRIEF.md §7); leases tasks; merges one at a time; marks done only from GitHub's tests on the exact commit; retries after usage resets; blocks dependents on failure.
- The twin must fail the test run on any failed check. Its existing "business left out" and "outside license" warnings are explained before Stage 1.
- K9 rehearses practice tasks, not app code.
- A stand-in AI during the build (hidden keys never reach api.anthropic.com).
- Fake data only, with a scanner that fails the build on real-looking personal data or keys.
- Test and live environments fully separate.
- The Railway restore drill includes switching the app to the restored copy (a restore creates a new database service).
- STATUS.md in GitHub is the official status record.
- The askhava.com page (Stage 0.5) needs Casey's separate go.
- Revised estimates: kit about 1–3 weeks; app build about 2–8 weeks; public launch about 6–12+ weeks, gated mostly by the LLC and EIN, then Twilio's registration approval.

### 8.1 Build order (decided October 27, 2026)

Casey's instruction: build as much as possible first, then prompt him to set up accounts before the final build and testing.

- **Before the build:** only GitHub (private repo plus the Claude GitHub App) and the Claude Max plan with its cloud environment.
- **Phase 1:** the kit and stages 1–5 are built and tested against stand-ins:
  - the twin's fake Twilio;
  - Stripe's mock server;
  - fake Resend and Lob;
  - a stand-in AI;
  - a local Postgres.

  The website, documents, Twilio campaign wording, DNS records and a step-by-step account checklist are produced too.
- **Account checkpoint:** the runner stops, opens a "Your turn: set up accounts" GitHub issue (GitHub emails and notifies Casey) and puts the checklist at the top of STATUS.md.
  - **Part 1 (test accounts, no LLC needed):** Railway, Stripe test mode, Twilio trial, Resend, Lob, Sentry and UptimeRobot, plus a Claude Console workspace.
  - **Part 2 (business accounts, for Gate C only):** LLC, EIN, Google Workspace, TPT license, insurance, Twilio registration, Stripe activation and zero retention.
- **Phase 2:** after "accounts done," each stand-in is swapped for the real sandbox and the test site is deployed on Railway. The real AI, the restore drill and fixes follow. Then Gate B, then Gate C.

### 8.2 Build brief, stack and runner (decided October 27, 2026)

`BUILD_BRIEF.md` (package root; also the repo root) is how Claude Code builds Ask Hava. It decides:

- **The stack:** Python 3.12, FastAPI with server-rendered Jinja pages, PostgreSQL 16 and a Postgres job queue.
- **The runner:**
  - one task at a time, with no parallel building;
  - a scheduled task with a GitHub trigger on merged pull requests, plus an hourly run;
  - auto-merge only when the required checks (`ci`, `twin`, `security`, `a11y`) pass on an up-to-date branch;
  - runner state on a separate `kit-state` branch, using git's own push rules as the lock;
  - work done by subagents on the model named in each task.

  This replaces the API-token design, because network secrets are never sent to api.anthropic.com.
- **Security, accessibility, compliance, testing, quality, environments and retention:** see the brief, sections 8–14.

## 9. Build and hosting facts the spec depends on

- Hosting: Railway Pro (see decision table). The test service auto-deploys from GitHub; the live service has auto-deploy off and Casey presses Deploy at Gate C. **Claude never holds a hosting key.**
- Accounts and order: see `02_build_plan/Build_Plan.md`, "Accounts to create."
- Two Anthropic accounts: Casey's Claude Max plan runs the build (cloud sessions); a separate Claude Console organization runs the app's AI with zero data retention. (Cloud sessions are not available inside a zero-retention organization.)
- Guardrails (from the original handoff, still in force):
  - No sign-ups, purchases, outreach, publishing, live messages, live charges or deployments without Casey's explicit go. AI never creates accounts or accepts terms for him.
  - Never paste or store secrets in chat or plain files.
  - Never put homeowner names, phones, addresses or photos into an AI tool that keeps data; use request IDs.
  - Don't claim something was tested unless it actually was.
