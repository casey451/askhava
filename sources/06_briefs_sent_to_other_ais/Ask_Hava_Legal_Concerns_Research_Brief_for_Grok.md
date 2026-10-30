# Ask Hava: legal concerns research brief (for Grok)

**From:** Casey Solomon (owner), with Claude · **Date:** October 27, 2026
**Jurisdiction:** Lake Havasu City, Mohave County, Arizona, USA. Federal law where noted.

## What I'm asking

Below is every legal concern we've found for Ask Hava. For **each one**, go find the actual law and give one of three answers, with proof:

- **NO PROBLEM:** the law allows what we do. Cite the statute, rule, agency guidance or case that shows it.
- **CHANGE NEEDED:** the law requires something different. Cite it, quote the controlling words, and say exactly what to change.
- **UNCLEAR, ATTORNEY:** the sources conflict or don't answer it. Say what the attorney should be asked, and what you found on each side.

This is research for my attorney to review, not a substitute for one. A confident answer without a source is worth nothing to me. "I couldn't find it" is a fine answer.

## Rules for the research

1. **Primary sources first:** statutes (azleg.gov), the Arizona Administrative Code, federal statutes and the CFR, agency rulings and guidance (ROC, Dept of Agriculture, ADRE, FCC, FTC, CTIA), and court decisions. Secondary sources (law firm blogs, trade sites) only to point you to a primary source, and labeled as secondary.
2. **For every citation give:** the full cite, the URL, the date you read it, and a short quote of the words that decide the question.
3. **Check it's current.** Statutes and FCC rules have changed recently (e.g. the handyman limit bills of 2023–2025, the FCC consent-revocation rule). Say if a rule is pending, delayed or recently amended.
4. **Label every claim:** **[P]** primary source, **[S]** secondary source, **[I]** your inference from sources, **[U]** unverified.
5. **Don't stretch.** If a law is about telemarketing and our texts aren't sales texts, say whether it applies and why, with the definition.
6. Say plainly whether you could open each source, or are working from memory.

## The business in one page (the facts to test the law against)

- **What it does:** A homeowner (or tenant) in Lake Havasu City texts one local number or uses web chat on askhava.com and describes a job. Ask Hava sends an **anonymous job card** (town, kind of place, job description with names, phones and addresses removed) to **every** opted-in local business in that category that qualifies. Businesses reply with quotes. The homeowner sees every quote on a private web page, in arrival order, never ranked, each with the business's own wording, review links, and (where a license applies) "ROC # shows active on the state website as of [date]." The homeowner picks one by entering a code texted to their phone.
- **What happens after a pick:** The picked business confirms, its saved card is charged off-session through Stripe, and only then does it get the homeowner's first name and address. It calls the homeowner through a **masked Twilio number**; neither side sees the other's real phone. Losing businesses get "This job is no longer active."
- **Money:** Homeowners never pay. The picked business pays a fee based on the size of the chosen quote: **$25** under $500, **$75** for $500–$1,500, **$199** over $1,500. Ongoing services count as 4 months of their price. Ranges use the midpoint; trip-fee quotes use a set tier per category. Free first win for businesses joining in the first 90 days. Automatic refund if the homeowner can't be reached or cancels before they talk. Repeat work from the same customer is free.
- **What Ask Hava never does:** perform any work, recommend or rank a business, set or negotiate the job price, or take a share of what the homeowner pays the business.
- **What Ask Hava does do that may matter legally:**
  - checks state licenses (ROC daily lists plus a live lookup at each pick) and only sends a job card to a business whose license covers that job;
  - **refuses** some quotes (a handyman quote of $1,000 or more; any gas, permit-type, hardwired smoke alarm or sprinkler job to an unlicensed business);
  - reads quotes with AI and reformats them into a standard card;
  - controls the pick-and-confirm step.
- **Categories:** about 95 kinds of local service business. The launch set is:
  - **no state license needed:** cleaning, pool cleaning, gardening, detailing, junk removal, movers, pet services, home watch, etc.;
  - **ROC-licensed trades:** plumbing, HVAC, electrical, pool repair, roofing, painting, etc.;
  - **pest control.**

  Left out until later: childcare, home care, massage, mobile hair and nails, mobile vets, catering, towing, property management.
- **Unlicensed handymen** may quote jobs under $1,000 with no permit. Their quote card says "Not a licensed contractor. Only for jobs under $1,000 total with no permit."
- **Texting:**
  - Twilio, registered as A2P 10DLC (Low-Volume Standard, use case Customer Care) under Ask Hava LLC.
  - Every first text: "Hi, this is Ask Hava. We gather quotes so you don't have to make the calls. It's free for you, and the business you choose pays us a small fee. Msg frequency varies by request. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms and privacy: askhava.com/terms. Reply OK to start."
  - STOP is honored in English and Spanish. Quiet hours run 8 a.m.–9 p.m. in the stricter of Arizona time and the recipient's area-code time zone.
  - Texts are automated; a person reviews anything flagged.
- **Business identity check:** a code is sent by text, then by an automated voice call that reads the code (for landlines), then by email to the address on the state record, then by a mailed letter (Lob).
- **AI:** The Claude API (Anthropic) reads requests and quotes, under a zero-data-retention agreement. A plain-code scrubber removes names, phones, addresses and emails before any AI call.
- **Tags on job cards:** "Homeowner prefers Spanish" and "Requester is a tenant." Businesses may tick "skip tenant requests." Businesses may tick "We speak Spanish," which shows on their quote card. There is no routing by language.
- **Emergencies:** For words like gas smell, sparking or flooding, Ask Hava first texts a fixed safety message (for example "a rotten-egg smell can mean a gas leak; leave the house and call 911 or the gas company"), then continues.
- **Complaints:** We apologize, explain the business is independent, and send the business's phone, the job details and the ROC complaint link. A business is paused after 3 complaints in 90 days.
- **Owner:** Casey Solomon has a full-time day job. Ask Hava is a side business, run as Ask Hava LLC (to be formed).
- **Website:** English and Spanish, built to WCAG 2.2 AA.

---

## The concerns

Each item has an ID. Please answer every ID.

### A. Contractor licensing (Arizona Registrar of Contractors)

**A1. Is Ask Hava itself "contracting" without a license?**
- *Concern:* A.R.S. 32-1101 defines a contractor to include anyone who, "by or through others," undertakes, offers or submits a bid to do construction work. Ask Hava receives and reformats quotes, refuses some, and controls the pick. Is that offering or bidding "through others"?
- *Find:* the definition's exact text; ROC substantive policy statements; any Arizona case or ROC action against a referral service, lead platform or "contractor matching" site (Angi, HomeAdvisor, Thumbtack or similar) in Arizona.
- *No problem looks like:* authority that a referral or lead service that doesn't perform, contract for or supervise the work isn't a contractor.

**A2. Does a fee that rises with quote size look like a commission on the contract?**
- *Concern:* $25/$75/$199 tied to the chosen quote's size may look like a share of the contract price, strengthening A1.
- *Find:* any ROC rule or case treating referral-fee structure as evidence of contracting; whether Arizona restricts contractors paying referral fees or commissions to unlicensed persons; how Angi/HomeAdvisor/Thumbtack structure fees in Arizona.
- *Change needed looks like:* authority that a size-based fee crosses the line where a flat per-introduction fee wouldn't.

**A3. Advertising unlicensed contracting.**
- *Concern:* Arizona restricts advertising contracting work by unlicensed persons. Ask Hava's website advertises categories like plumbing and roofing.
- *Find:* A.R.S. 32-1164 and related advertising rules; whether a platform advertising "get quotes for plumbing" must hold a license or carry a disclaimer.

**A4. The handyman exemption, A.R.S. 32-1121(A)(14) and (D).**
- Confirm the current limit (we have: under $1,000 total including labor and materials, no permit, not part of a larger job, no splitting). Did the 2023, 2024 or 2025 bills raising it fail, and has anything passed in 2026?
- Confirm the exclusions: gas work at any price, hardwired smoke alarms, fire sprinklers. Are there others (e.g. electrical, plumbing beyond minor repairs)?
- Confirm the required disclosure: the exact words handymen must use ("not a licensed contractor") and where (ads, contracts, bids). Does our quote-card line satisfy it, or must the business include it in its own quote?
- Does a platform that sends small jobs to unlicensed handymen carry liability if a handyman exceeds the exemption?
- Should handyman-type businesses be capped at $1,000 even in no-license categories (e.g. a $2,408 pressure-wash quote from a handyman business)?

**A5. Which jobs need which license.** For each, find the controlling rule:
- Weekly pool cleaning: outside ROC contracting? Where does a "small pool repair" cross into R-6?
- The gardener exemption (32-1121(A)(16)): does it cover tree removal, palm trimming, planting, sod, irrigation repair?
- The ROC class for garage doors and for solar screens.
- Repairing a freestanding appliance: R-63 or no license?
- Water softener and RO install (R-54?), window film (R-65), low-voltage cameras (R-67), epoxy garage floors, pavers, misting systems, holiday lights, dryer vent cleaning, gutters, insulation.
- Permit-type jobs: which common home jobs need a Lake Havasu City permit (water heater replacement, panels, new circuits, re-roofs, block walls, patio covers)? Find the city's permit-required list.

**A6. Using ROC data.** May we download the ROC's posted lists daily and make automated lookups on the ROC contractor search? Check the ROC website terms of use. Is "ROC # shows active on the state website as of [date]" an accurate and safe way to display it?

**A7. Residential contract requirements.** A.R.S. 32-1158 and related rules require certain disclosures in residential contracts. Do any of them fall on Ask Hava because it displays the quote, or only on the business?

### B. Other state licenses and regulated categories

**B1. Pest control and weed spraying.** Does referring or advertising pest-control services require an Arizona Dept of Agriculture license or a qualifying party? Confirm the scope of the incidental weed-control exemption, A.R.S. 3-3613 (pre-emergent, advertised service, amounts).

**B2. Movers.** Does Arizona license or register intrastate household-goods movers? Do any rules apply to a service that collects mover quotes (federal broker rules apply only to interstate moves; confirm we're outside them if we only handle in-state moves)?

**B3. Septic pumping.** Who licenses septage haulers in Mohave County: ADEQ or the county?

**B4. Junk and waste hauling.** Does Lake Havasu City code limit who may haul solid waste or junk (franchise hauler rules)?

**B5. Home watch, vacation-rental turnover cleaning and "property care."** At what point does this become property management needing an ADRE license? Check the exemption for rentals of 31 days or fewer as it applies to vacation-rental managers.

**B6. Alarm and security camera installers; home inspectors** (Board of Technical Registration). Confirm the certificate requirements and that low-voltage camera install needs an ROC class.

**B7. Deferred categories** (only if quick): what licenses would childcare, non-medical home care, massage, mobile hair and nails, mobile vets, catering, towing and property management need?

### C. Texting, calls and consent (federal and Arizona)

**C1. TCPA (47 U.S.C. 227) and FCC rules for our texts.**
- The homeowner texts first and replies OK to the consent text. Is that prior express consent for informational texts about their request? Do any of our texts count as telemarketing or advertising (needing prior express **written** consent)?
- The day-30 text "Still using Desert Pool Care? Reply YES or NO" is a candidate. So are the "new quote" updates.
- Texts to businesses (job cards, pick notices): is consent given at sign-up enough?

**C2. FCC consent-revocation rule (2024 order).** Revocation "by any reasonable means." What's the current status and effective date of each part? We honor STOP, Spanish opt-outs and natural phrases like "stop texting me." Is that enough? What's the deadline to stop after a revocation?

**C3. Quiet hours.** The TCPA's 8 a.m.–9 p.m. rule applies to telephone solicitations. Does it apply to our informational texts? Do state laws apply to out-of-state area codes (snowbirds), e.g. Florida's FTSA or Oklahoma's law? Is our "stricter of Arizona and the recipient's zone" rule enough?

**C4. Arizona's telephone solicitation law (A.R.S. 44-1271 et seq.).** Does it now cover text messages? Does it apply to non-sales texts? Is any registration required?

**C5. Automated voice call reading an ID code to a business's landline.**
- Is an artificial or prerecorded voice call to a business number that the business gave us at sign-up allowed under the TCPA?
- Does the FCC's 2024 ruling that AI-generated voices are "artificial" affect this?

**C6. CTIA guidelines and A2P 10DLC.**
- Does our first text and opt-in flow meet the current CTIA Messaging Principles and Best Practices and Twilio's campaign requirements?
- Is anything missing, such as the privacy policy wording, the statement that we don't share mobile numbers for marketing, or HELP response content?
- Is the "Customer Care" use case right for this model?

**C7. Masked calls.** Does using a Twilio proxy number as caller ID comply with the Truth in Caller ID Act and STIR/SHAKEN? If calls are ever recorded:
- Arizona is one-party consent;
- what applies when a party is in a two-party-consent state (e.g. California)?

(Current design: calls are not recorded. Say if that's the safer default.)

**C8. Record-keeping.** How long should consent and opt-out records be kept, given the TCPA's statute of limitations?

### D. Consumer protection, advertising and disclosures

**D1. FTC Act Section 5 and the Arizona Consumer Fraud Act (A.R.S. 44-1521 et seq.).** Are any of our claims deceptive or in need of qualification? Check each:
- "Free for you."
- "Your number is never shared."
- "Every quote shown, never ranked."
- "Independent business, not affiliated with Ask Hava."
- "ROC # shows active as of [date]."
- "Insurance on file: Yes/No."
- The business-side claims, such as "$25 is about what one online lead costs."

**D2. Disclosing who pays.** Is "the business you choose pays us a small fee" enough, or must the fee amount be disclosed to homeowners? Does the FTC Endorsement Guides' material-connection rule apply? We don't rank, so is it irrelevant?

**D3. Bots and AI disclosure.** Our texts are automated. Does any law require saying a bot is talking, e.g. California's B.O.T. Act (Bus. & Prof. Code 17940) for California residents, Utah's AI disclosure law, or anything in Arizona? Is "Ask Hava is a local service. Our texts are automated" enough?

**D4. Safety messages.** Does texting a fixed gas-leak or electrical-safety message create a duty or liability if it's wrong or late? What disclaimer language, if any, is standard?

**D5. Refund promises.** Our automatic-refund rules are promises to businesses. Is anything needed in the business terms so they are enforceable and clear?

### E. Payments

**E1. Off-session charges to businesses.** What authorization must a business give at sign-up so a card can be charged later when it confirms a pick?
- Consider card-network rules for merchant-initiated transactions, and Stripe's requirements for saving cards and charging off-session.
- Does an Arizona or federal rule (e.g. EFTA, if bank debits are ever used) apply?

**E2. Stripe acceptability.** Is a lead-generation or referral-fee business on Stripe's restricted or prohibited business list? Does it need extra review?

**E3. Arizona transaction privilege tax (TPT) and Lake Havasu City tax.** Are fees charged to businesses for introductions taxable in Arizona or by the city? Does Ask Hava need a TPT license?

### F. Privacy and data

**F1. Privacy law.** Does Arizona have a comprehensive consumer privacy law in force as of October 2026? Do any out-of-state laws reach us through visitors' or residents' locations (e.g. California's CCPA thresholds, CalOPPA's privacy-policy requirement for sites California residents visit)? What must the privacy policy say?

**F2. Data breach.** What do A.R.S. 18-551 and 18-552 require (definitions, notice timing, who to notify)? Do names plus addresses plus phones count as "personal information"?

**F3. Sending homeowner details to an AI vendor.** With a zero-data-retention agreement, and code scrubbing before every call, does any law require disclosing this in the privacy policy, or getting consent? Are photos (which may show house numbers, faces or children) a special risk?

**F4. Children.** Should the terms say users must be 18 or older? Does COPPA matter if a minor texts?

**F5. Email to businesses.** Do CAN-SPAM rules apply to our transactional emails to businesses, and what must they include?

### G. Discrimination

**G1. "Requester is a tenant" plus "skip tenant requests."** Is tenant status, or anything it may stand in for (source of income, familial status, race through disparate impact), protected under the federal Fair Housing Act, the Arizona Fair Housing Act or Arizona civil-rights law? Are we liable for letting businesses filter on it? Note that home repair services, not housing sales or rentals, are involved.

**G2. "Homeowner prefers Spanish."** Showing a language preference could let a business decline based on national origin. Does 42 U.S.C. 1981, Title II, Arizona public-accommodation law or the FTC's guidance create risk? Is it safer to show the tag only to businesses that ticked "We speak Spanish," or not at all?

**G3. Business-side terms.** Do we need a non-discrimination clause in the business terms?

### H. Liability and insurance

**H1. Negligent referral.** If a picked business damages a home, injures someone or commits a crime, could Ask Hava be liable for negligent selection or referral? We check licenses and identity but don't vet. Look for Arizona cases on referral-service liability, and on Angi/HomeAdvisor/Thumbtack liability.

**H2. Section 230.** Does 47 U.S.C. 230 protect us for business-written quote content we reformat with AI? Does reformatting make us a co-developer of the content?

**H3. Terms of service.** For both homeowner and business terms, what's standard and enforceable in Arizona? Consider limitation of liability, disclaimer of warranties, independent-business statement, arbitration and class waiver, governing law, and clickwrap versus text-message acceptance.

**H4. Insurance.** What coverage do comparable referral platforms carry (general liability, professional/E&O, cyber)? Is any coverage legally required for this business? (We'll confirm prices with a broker; I only need what's required or standard.)

### I. Website accessibility

**I1.** Does ADA Title III apply to a web-only service like ours in the Ninth Circuit (Robles v. Domino's and later cases)? Is WCAG 2.2 AA the right target? Does the 2024 DOJ Title II rule matter to a private business (we believe it doesn't)? Any Arizona accessibility law?

### J. Business formation, name and local rules

**J1. LLC formation.** Does Arizona still require publication of LLC Articles in Mohave County? If so, within what time and how many times? Statutory agent requirements?

**J2. Trademark.** Is "Ask Hava" or a confusingly similar mark for referral, directory or quote services registered or pending at the USPTO? Is "Hava" used by anyone in a related class? Is an Arizona trade name registration needed if the LLC is named "Ask Hava LLC"?

**J3. Lake Havasu City business license and home occupation.** Does Ask Hava need a city business license? If run from home, does it need a home-occupation permit, and does it meet the limits?

**J4. Using the city's active business license spreadsheet** to check businesses: any restriction on use?

**J5. Secondary employment.** The owner has a full-time day job. Without giving legal advice on his specific employer's policy, list the general rules that usually apply to side businesses run by full-time employees in Arizona:
- typical secondary-employment approval requirements;
- conflicts of interest;
- no use of official position, title or databases;
- whether mentioning his role in marketing is typically restricted.

He will check his own employer's policy; this is only so he knows what to ask.

### K. Anything we missed

**K1.** List any other law, rule or licensing issue that applies to this model and isn't covered above, with sources.

---

## Report format

Return one markdown file named `Ask_Hava_Legal_Research_Findings_Grok.md`:

```
# Ask Hava legal research findings (Grok, <date>)

## 0. How I researched
- Could I open live web sources? Which ones failed?
- Anything I answered from memory (labeled [U])

## 1. Summary
- CHANGE NEEDED items, most serious first (one line each)
- UNCLEAR, ATTORNEY items (one line each)
- Count of NO PROBLEM items

## 2. Findings
| ID | Verdict (NO PROBLEM / CHANGE NEEDED / UNCLEAR, ATTORNEY) | Controlling source (cite + URL + date read) | Deciding words (short quote) | Label [P/S/I/U] | What to change (if any) | Confidence (high/med/low) |

## 3. Questions for the attorney
- Numbered, each tied to an ID, each phrased so a yes/no or short answer is possible

## 4. Anything we missed (K1)
```

Please don't skip an ID. If one takes too long, mark it UNCLEAR, ATTORNEY and say what you'd search next.
