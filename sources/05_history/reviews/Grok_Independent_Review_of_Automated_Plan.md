# Havasu Home Quotes: Independent Review of the Automated Plan

Reviewer: Grok Bot, working for Casey Solomon. Subject: `Havasu_Home_Quotes_Review_Package.md` (Parts A–G). Vendor pages and laws were checked on the day this review was written. Tags: **[fact]** means a source is named, **[calc]** means arithmetic shown here or in the numbers check, and **[opinion]** means the reviewer's judgment. "Could not verify" means exactly that.

---

## 1. Verdict

**Verdict: validate demand a cheaper way first. Do not build the automated version yet. Confidence: high.**

Top three reasons:

1. **The plan loses money in every Base month, and removing Ask Hava makes that worse.** The package's own Base case loses $214 a month at month 24 and ends 24 months $11,794 down [fact: Part B, C5]. Break-even needs about 122 requests a month against a Base of 80 [fact: C6]. The package's demand figures were first estimated with an Ask Hava channel and were never lowered after Ask Hava was dropped [fact: workbook `Havasu_Unit_Economics_v2.xlsx`, Inputs I19: "estimate first made with an Ask Hava channel, dropped… values unchanged"]. With Ask Hava at zero, a realistic Base is about 40–60 requests a month at month 24, not 80. That means a loss of $315–$417 a month at month 24 and a 24-month cash position of about −$13,400 to −$14,900 [calc, §5.4]. If the income goal is $20,000 a month, the plan needs about 4,059 requests a month, roughly 50 times its own Base. No single city of this size can supply that [calc, §2.2].
2. **The core design breaks two rules Casey has already set.** An AI voice agent and an AI text desk handle every homeowner call and text, so homeowner names, phone numbers and addresses would pass through AI tools. That breaks the rule that no homeowner personal data goes into any AI tool. The go-live sequence also forms the LLC and files the texting registration before the attorney review. That breaks the decision that all legal and LLC work waits behind the hard gate [fact: Part E §8, §10; `test-kit/deferred-legal-and-llc.md`].
3. **The approved hand-run test answers the only question that matters, which is demand, for less money and with no build risk.** The hand-run test needs about $1,500–$2,300 in cash [fact: `test-kit/approved-decisions-2026-10-26.md`]. The automated plan needs $2,220–$4,220 plus about $800–$900 held back, and it carries $620 a month in fixed costs [fact: C3, C8]. At the package's own expected launch demand of 2.2 in-scope requests a week, there is about a 6% chance of clearing the kill line and about a 0.1% chance of reaching the continue line [calc, §5.5]. So the build would most likely be paid for and then shut down.

---

## 2. Conflicts with your current decisions

These six items are places where the package disagrees with decisions Casey has already made. Each one has to be settled before anything else in the package is relied on.

### 2.1 Ask Hava is out, but the package still depends on it

- **What the package says.** Part A calls Ask Hava (askhava.com) the "main free marketing channel." Part E §3 puts an ownership line ("Havasu Home Quotes and Ask Hava are owned by Casey Solomon") on the website and adds a "Get repair quotes" block to Ask Hava. Part F gives Ask Hava jobs to the recruiting assistant (reading its listings) and the marketing agent (updating its block). Gap 2 puts the inbound contractor page "on the site and Ask Hava." Phase 6 go-live uses the "Ask Hava page and rack cards" [fact: Parts A, E §3, E §10, F].
- **Casey's decision.** Ask Hava gets no traffic and will play no part in this business. Its contribution is zero [fact: Casey, via the parent agent; also the workbook Decisions Log B20].
- **Fix.** Remove every Ask Hava reference: the ownership line, the site block, the recruiting-assistant reads, the marketing-agent task, the contractor page link and the go-live step. None of these appear in this review's recommendations. Then re-estimate demand from the channels that remain. §5.4 does this: the Base ramp is likely 25–50% lower, month-24 profit falls from −$214 to −$315 to −$417, and break-even stays at 122 requests a month. Break-even therefore moves from 1.5× expected demand to about 2.0–3.0× [calc].

### 2.2 The income goal may be understated by 12 times

- The package states the goal as "$20,000–$50,000 a year" [fact: Part A; C6]. The goal on record is **$20,000–$50,000 a month**. If the record is correct, every goal figure in the package is 12 times too small.
- At the package's own Base contribution of $5.08 per request and $620 fixed costs [fact: C6]:
  - $20,000 a month needs (20,000 + 620) ÷ 5.08 ≈ **4,059 requests a month**, which is about 2,638 in scope and 453 paid introductions [calc].
  - That is about 48,700 requests a year, or 2.3 requests per owner household per year across the package's 20,800 owner households [calc; household count from C2]. That is not achievable [opinion].
  - $50,000 a month needs about 9,965 requests a month at Base [calc].
  - Even the Optimistic contribution ($11.90 per request) needs about 1,714 requests a month for $20,000 a month [calc].
  - About 453 paid introductions a month would also have to be absorbed by the 15–21 contractors the plan recruits, which is over 20 paid jobs each per month [calc; opinion that this is unrealistic].
- **Fix.** Casey should say which unit is right. If the goal is monthly, this business in this city cannot reach it at any plausible demand. It would only make sense as a small side income or a stepping stone, and the decision should be framed that way.

### 2.3 Homeowner personal data would go through AI tools

- **The rule.** Homeowner names, phone numbers and addresses never go into any AI tool, including Claude.
- **What the package does.** The design breaks this rule in many places [fact, Parts E and F]:
  - Retell's AI voice agent hears and transcribes every call, and recordings are kept 90 days (D2).
  - The AI text desk reads every inbound text.
  - The Receptionist agent reads "caller's past requests (by phone number)" and has write access to contact vault A.
  - The repeat-customer feature has the AI confirm a street address aloud (E, repeat-customer row).
  - The "talk to a person" emails include a callback number.
  - The QA auditor reads 10% of transcripts.
  - Photo screening is done by AI.
- **The package's privacy claim is also wrong.** Part E §4 says the AI provider "does not train on or keep the data (Anthropic's commercial API by default)." Anthropic's own page says API inputs and outputs are deleted "within 30 days." So data is kept for up to 30 days unless there is a zero-retention agreement, and Managed Agents session transcripts "persist until you delete them" and are outside zero-retention [fact: privacy.claude.com article 7996866; platform.claude.com "API and data retention"]. A rule that says "the AI and the console see request IDs" cannot hold when the AI is the thing answering the phone.
- **Fix: designs that keep AI away from personal data** [opinion; design proposals]:
  1. **Non-AI intake.** Use a plain web form plus a touch-tone phone menu with no speech transcription. Callers press keys for trade, ZIP code and urgency. A deterministic program, meaning ordinary code with no AI, stores the name, phone and address in an encrypted contact table and issues a request ID.
  2. **Deterministic vault and messaging.** Ordinary code holds contact details, sends status texts from fixed templates, and handles keyword replies (A/B/C, NONE, STATUS, STOP). No AI reads inbound texts.
  3. **Scrub before AI.** If AI is used at all, it receives only the request ID and a job description that has passed through a pattern-based scrubber that removes phone numbers, emails, street numbers and names. Anything the scrubber flags goes to Casey, not to the AI.
  4. **Contractor side only.** AI may read contractor quotes, which carry no homeowner personal data.
  5. **Photos.** Drop photos, or have ordinary code strip location and other metadata. No AI screening of photos.
  6. **No AI voice and no AI reading transcripts.** Callers who can't use the menu leave a voicemail that Casey handles by hand.
  7. **Or change the rule.** Casey can amend the rule knowingly, but only in writing and before any build.

### 2.4 Legal and LLC work is moved ahead of the hard gate

- **Casey's decision.** All legal and LLC work waits until the project is built and working. It is a hard gate before any contractor signs, any live charge, or any real homeowner request. The deferred file lists the LLC only after the Phase 3 dry run and requires "a new legal review before any automation, 10DLC texting, AI voice" [fact: `deferred-legal-and-llc.md`; `legal-opinion.md` line 62].
- **What the package does.**
  - The D2 register says "The LLC and EIN are formed in build week 1."
  - E §8 step 6 says "Week 1 of the build (decided Oct 26)," and step 8 opens the bank account right after.
  - Decision 1 in E §11 is recorded as "Decided Oct 26: yes."
  - E §10, Phase 3, files the A2P brand and campaign registration. The attorney review (G5) comes in Phase 5. So the texting registration description goes to carriers before an attorney has seen it, even though G5 lists that description as something the attorney must review [fact].
  - Phase 5 combines the gate items with contractor visits and sign-ups. That invites a contractor signing before G1–G5 are complete [fact: E §10 row 5; opinion on the risk].
  - E §10 itself says the gate "stays last, except the LLC and EIN if Casey approves." Elsewhere the package records that approval as already given. Those two statements contradict each other [fact].
- **Fix.** Keep LLC, EIN, bank and A2P registration behind the gate, or have Casey explicitly reverse the deferral. Split Phase 5 so recruiting conversations are non-binding until G1–G5 are done. Rescope the attorney review: the $750 flat fee was set for a hand-run test, and AI voice, TCPA consent, 10DLC and recording now add work [opinion; fact for the $750, approved decision #8].

### 2.5 Calendar dates in the plan

The phase plan has no hard calendar dates, which is good. It does contain soft calendar-month targets [fact]:

- "go-live preferred in the November–March winter-resident season" (E §10)
- "recruit extra HVAC contractors before May"
- "honest wait message in July and August" (F risk 15)
- "city license (each January)"

**Fix.** Reword these by phase and season: "go-live in the cool season," "recruit extra HVAC before the hot season," "wait message during the peak cooling season," "license renewal at the annual renewal point." Statutory tax due dates and decision stamps such as "decided Oct 26" are factual and can stay.

### 2.6 The automated phone line reverses an approved decision

- Approved decision #6 says "No automated phone line or automated texting. Google Voice, by hand" [fact: `approved-decisions-2026-10-26.md`]. The new plan reverses it. That is Casey's call to make.
- **What the reversal costs** [fact: C3, C8, C8b, test plan; calc]:
  - **Start-up cash:** $2,220–$4,220 plus $800–$900 held back, against $1,500–$2,300 for the hand-run test. That is $720–$1,920 more before reserves.
  - **Monthly fixed costs:** $620 against roughly $199–$234 for the hand-run test ($25–$60 running plus about $174 test insurance). That is about $386–$421 more each month.
  - **Month-24 Base:** −$214 automated against −$301 hand-run, so automation is $87 a month better. At that rate the extra start-up money takes 8–22 months to pay back, and only if demand reaches Base.
  - **Owner time:** 6–10 hours a month instead of 22–34. This is the real benefit.
  - **New risks:** AI voice and texting compliance, 10DLC rejection, AI insurance exclusions, and build failure.

---

## 3. Top issues (ranked)

**1. Demand is too low to cover fixed costs, even before Ask Hava is removed.**
- *What's wrong.* Break-even is 122 requests a month, or 18.3 in-scope requests a week. Base is 80 a month [fact: C6]. Base never breaks even in 24 months [fact: Part G].
- *Why it matters.* The package's own downside cases stack up. Insurance $150 higher, the Claude plan staying on Max, and voice costing twice the estimate together give −$214 − 150 − 80 − 33 = **−$477 a month** at month 24 [calc from C6 sensitivity table].
- *Evidence.* C5, C6, Part G.
- *Fix.* Do not build until measured demand runs near the break-even pace. That means at least about 18 in-scope requests a week sustained, not the 4.8-a-week continue line.

**2. The demand inputs still assume Ask Hava, and the replacement channels are weaker than the package assumes.**
- *What's wrong.* The ramp values were not changed when Ask Hava was dropped [fact: workbook Inputs I19]. Base month 24 equals about 25% of Angi's national request rate for a brand-new local name [fact: same cell]. Google Business Profile, which the steering list includes, says "lead generation agents or companies" are not eligible, and it requires in-person contact with customers [fact: Google Business Profile Help 13763036]. That channel may not be available at all.
- *Why it matters.* At 40–60 requests a month, month 24 is −$315 to −$417, and the 24-month cash position is about −$13,400 to −$14,900 [calc].
- *Fix.* Use the reviewer's Base of about 50 requests a month at month 24 until test data exists, and fund channels as described in §5.4.

**3. AI intake conflicts with the no-personal-data-in-AI rule.**
- Covered in §2.3.
- *Why it matters.* The core product cannot launch as designed without breaking Casey's rule. A breach would involve recordings, transcripts and AI logs in three outside systems (Retell, Anthropic, Render) [opinion].
- *Fix.* Use the touch-tone menu, web form, deterministic vault and scrubber design in §2.3.

**4. Texting registration may be permanently rejected as lead generation.**
- *What's wrong.* Twilio error 30951 lists "collects consumer information… to… distribute to other businesses as sales leads" and "operates as an… intermediary, capturing consumer interest for… services offered by unrelated third parties." It also says the rejected campaign "is not eligible for resubmission" [fact: twilio.com/docs/api/errors/30951]. Havasu Home Quotes matches that description closely, even with masked relay numbers [opinion].
- *Why it matters.* The fallback is toll-free verification, which could not be verified here, and then a voice-only service. Voice-only means AI-voice readouts to homeowners. AI voices count as "artificial" voices under the TCPA and need prior express consent [fact: FCC 24-17 declaratory ruling, Feb 2024].
- *Fix.* Design so homeowners never need business texting. Use an email or web status page plus touch-tone callbacks recorded by a human. If texting is kept, get Twilio pre-review and the attorney's view before filing anything.

**5. The legal gate and LLC sequencing is inconsistent with Casey's deferral.**
- Covered in §2.4.
- *Why it matters.* Filings and a bank account would exist before the attorney review. A contractor could sign during Phase 5 [opinion].
- *Fix.* Reorder as described in §2.4.

**6. The income goal unit is unclear and may be 12 times too small.**
- Covered in §2.2. The fix is for Casey to confirm the unit.

**7. The build is too large for one non-coder to maintain.**
- *What's wrong.* The plan has 18 AI agents, a relay number pool, Stripe automation, auto-deploying fixes (D2), a single Render web service and database, and Claude Code run from Casey's consumer plan [fact: Parts E, F].
- *Why it matters.* An earlier review of a smaller version estimated 600–1,400 build hours [fact: `review/01_technical.md` and `Havasu_Referral_Complete_Kit.md`, an earlier estimate labeled as an estimate; opinion that the current scope is larger]. Consumer Claude plans are outside zero-retention [fact: platform.claude.com retention page]. Auto-deploy without human review can break the money or consent paths [opinion].
- *Fix.* If anything is built, build only a web form, Stripe Payment Links, a deterministic database and fixed templates. No auto-deploy.

**8. Some costs are missing or not quoted.**
- *What's wrong.* Insurance of $276 is "not a quote" [fact: C2/C3]. The ISO exclusions CG 40 47 01 26 and CG 40 48 01 26 exist and can remove generative-AI injury from general liability coverage [fact: Vorp Labs exclusions ledger; Verisk via IndependentAgent.com].
- Other costs that are missing or partly covered:
  - Retell add-ons: PII removal +$0.01 a minute and guardrails +$0.005 a minute [fact: retellai.com/pricing].
  - Relay-call minutes on both legs.
  - T-Mobile inbound text fees: $0.0025 per segment [fact: twilio.com SMS pricing].
  - Statutory agent, virtual mailbox and city license (prices could not be verified).
  - A larger attorney scope.
  - Owner pay and tax.
- *Fix.* Get a bound quote with an AI carve-back before trusting any profit line. Add a contingency of at least $100 a month [opinion].

**9. The test has little power at expected demand.**
- *What's wrong.* The kill line is 3.3 a week and the continue line is 4.8 a week. Expected launch demand is 2.2 a week [fact: C7].
- *Why it matters.* At 2.2 a week over the four-week scoring window, P(clear kill) ≈ 6.4% and P(reach continue) ≈ 0.1% [calc]. If the true rate is exactly at the kill line, the result is a coin flip (44.9%) [calc]. So the automated build is very likely to be spent on a test that fails.
- *Fix.* Run the cheap test first. Build only after a pass.

**10. Some privacy and data-handling claims are inaccurate.**
- *What's wrong.* The claim that Anthropic doesn't keep data is wrong, as covered in §2.3. Recordings are kept 90 days at Retell. ROC license checks use an AI browser on a site that "blocks automated server lookups" [fact: D2].
- *Why it matters.* The privacy policy would make false statements, which is a deceptive-practice risk [opinion]. Automating a site that blocks automation risks being blocked or breaching its terms (could not verify the site's terms).
- *Fix.* Correct the privacy text and do ROC checks by hand. They take minutes per contractor.

---

## 4. Numbers check

### 4.1 Base month-24 recomputation (C2–C4)

The Python calculation, inputs from C2 and C3:

```
requests 80 × 0.65 in scope = 52.0
× 0.85 qualified = 44.2 ; × 0.65 quoted = 28.73 ; × 0.50 chosen = 14.37
× 0.69 accept/pay = 9.91 ; × 0.90 after leakage = 8.92 paid introductions
gross     8.92 × $60.24                       = $537.4
refunds   10% of gross                        = −$53.7
disputes  1% × 8.92 × (50% × 60.24 + $15)     = −$4.0
Stripe    8.92 × (2.9% × 60.24 + $0.30)       = −$18.3
variable  80 × $0.6875                        = −$55.0
fixed                                         = −$620.0
operating profit                              = −$213.6
```

**The result is −$213.6 against the package's −$214. It matches** [calc].

### 4.2 Other checks

- **Contribution per request** is $5.08. 620 ÷ 5.08 = 122 break-even requests, and (20,000 + 620) ÷ 5.08 ≈ 4,059. The package's figure of 2,287 ÷ 5.08 = 450 for $20,000 a year also checks out [calc].
- **Stop rule ★1.**
  - Kill line: 79 ÷ 6.4 ÷ 4.345 × 1.15 = 3.27 a week.
  - Continue line: 79 ÷ 4.4 ÷ 4.345 × 1.15 = 4.75 a week.
  - Both match C7 [calc]. These lines are 1.5× and 2.2× expected demand. The package describes them as "2–3×," which is loose.
- **Part G CSV.** All 144 rows (two cost bases × three scenarios × 24 months) were recomputed:
  - profit = gross − refunds − disputes − Stripe − variable − fixed
  - cumulative = −start-up + running sum
  - in-scope = 0.65 × requests
  - gross = paid × fee
  - **Result: zero arithmetic errors.** Year totals and lowest-cash figures match C5 (Base year 1 −$5,332, year 2 −$3,262; designed Base month 24 −$429.63, cumulative −$16,978) [calc].
  - The only cosmetic difference: the CSV average fee is $60.24 while C2 says $60.25 (rounding).

### 4.3 Vendor prices checked

| Item | Package | Checked | Result |
|---|---|---|---|
| Retell voice | $0.13–0.31/min, $0.15 Base | $0.07–$0.31/min; Haiku-class LLM $0.0032/min; PII removal +$0.01/min | Plausible [fact: retellai.com/pricing] |
| Twilio 10DLC | $4.50 brand, $15 vetting, $1.50/mo | Same | Matches [fact: Twilio support 1260803965530] |
| Twilio number, text | $1.15/mo, $0.0083/segment | $1.15; $0.0083 plus carrier fees, T-Mobile inbound $0.0025 | Matches; inbound carrier fees understated [fact: twilio.com SMS pricing] |
| Render | $58/mo | Pro $25 + $7 + $7 + Postgres $19 | Matches [fact: render.com/pricing] |
| Claude Haiku 5.5 | $0.10/$0.50 per MTok | $0.10/$0.50 for prompts ≤100K tokens; $0.50/$2.50 above | Matches; keep prompts under 100K [fact: platform.claude.com pricing] |
| Claude Max 5x | $100 incl. $100 API credit | Confirmed | Matches [fact: claude.com/pricing] |
| Managed Agents | $0.08/session-hour | Confirmed | Matches |
| Stripe | 2.9% + 30¢; $15 dispute | Same; fees not returned on refunds | Matches [fact: stripe.com/pricing] |
| Stripe Financial Connections | $0.30 | Not shown | Could not verify |
| Twilio Conversations fees | Not costed | — | Could not verify |

---

## 5. Remaining questions

### 5.1 Assumptions (Question 4)

1. **Demand of 80 requests a month at month 24.** This is the **most wrong** assumption, and it is too high. It was estimated with Ask Hava and never lowered, and Google Business Profile may be unavailable. The reviewer's Base is about 50 [opinion].
2. **Funnel rates (quoted 65%, chosen 50%, pays 69%).** These are untested. They could be lower in the hot season if contractors are fully booked [opinion].
3. **Leakage of 10%.** Probably too low. Contractors meet homeowners on site, and the only remedy is the fee itself [opinion].
4. **Insurance of $276 and owner time of 6–10 hours.** Both are likely too low. The insurance is not a quote and AI exclusions exist. Real build-phase and first-year hours will be higher [opinion; fact on the exclusions].

### 5.2 Re-estimated demand without Ask Hava (per Casey's confirmation)

The remaining channels, judged by the reviewer [opinion unless tagged]:

- **Rack cards.** These are already in the package's $75-a-month marketing budget [fact: C3]. The printing price could not be verified.
- **Facebook and Nextdoor posts.** Posting from your own accounts is free. Many local groups restrict business posts (opinion; could not verify specific group rules).
- **Google Business Profile.** Free, but probably **not available**, because lead-generation companies are listed as ineligible and the business has no in-person customer contact [fact: Google support 13763036].
- **Partners.** Property managers, real-estate agents, home inspectors, HOA newsletters and hardware stores. These cost nothing but take time. Any referral fee paid to a partner comes out of a $45.55-per-introduction contribution.
- **Word of mouth.** It starts near zero and builds with completed jobs.

Re-estimate [calc using the package ramp × factor, $5.08 contribution, $620 fixed]:

| Case | Month-24 requests | Month-24 profit | 24-month cash | Break-even ÷ expected |
|---|---|---|---|---|
| Package Base (with Ask Hava assumption) | 80 | −$214 | −$11,794 | 1.5× |
| −25% | 60 | −$315 | ≈ −$13,400 | 2.0× |
| **Reviewer Base, −37.5%** | **50** | **−$366** | **≈ −$14,150** | **2.4×** |
| −50% | 40 | −$417 | ≈ −$14,900 | 3.0× |

Break-even stays at 122 requests a month, because fixed costs do not change. Expected launch demand drops to about 1.4 in-scope requests a week, against a kill line of 3.3 [calc: 2.2 × 0.625].

**Replacement channels and their costs.** The hard limit is the marginal contribution: about **$5 per request, or about $45 per paid introduction**. Any paid channel has to bring requests in below that just to break even on each one [calc].

- **USPS Every Door Direct Mail (EDDM).**
  - Postage is $0.26 per piece [fact: usps.com].
  - Printing is about $0.10–$0.22 per piece [fact, third-party: mailpro.org].
  - A 2,000-piece test costs about $720–$960 [calc]. It pays back only if it brings about 140–190 requests, a 7–9% response rate. That is implausible (opinion; could not verify typical response rates).
- **Paid Facebook or Nextdoor ads.** Cost per click and cost per lead could not be verified. If tried at all, cap a single test at a budget Casey chooses and judge it against $5 per request.
- **Google Local Services Ads.** Eligibility for a referral service could not be verified, and it is likely ineligible for the same reason as Google Business Profile [opinion].
- **Best value: free partner channels plus a website with plain local search content.** These cost time, not cash [opinion].

### 5.3 Legal (Question 5). AI analysis, not legal advice; for the attorney.

- **TCPA and consent.** Inbound calls from homeowners are not robocalls. Outbound AI-voice quote readouts are "artificial voice" calls and need prior express consent [fact: FCC 24-17]. Outbound texts need consent and must honor STOP. The current revocation-rule details and dates could not be verified here. Texts to contractors' cell phones also need consent at sign-up [opinion].
- **Twilio 10DLC lead-generation rejection.** See issue 4. The rejection is not eligible for resubmission [fact: Twilio 30951]. Twilio's onboarding guide also requires a statement that opt-in data is not shared with third parties [fact: Twilio help 11847054539547]. Passing a homeowner's first name and a relay to a contractor sits close to that line [opinion].
- **A.R.S. §32-1101.** The definition of "contractor" covers anyone who "offers to undertake… by or through others" and consultants who represent that they can "supervise or manage a construction project" [fact: azleg.gov §32-1101]. The counsel rulings addressed the fixed hand-run script. An AI that improvises wording ("we'll get it fixed for you") creates new exposure [opinion]. Lock homeowner-facing wording to fixed templates.
- **A.R.S. §44-1271–1278.** Section 44-1278(B) makes it unlawful for a seller conducting telephone solicitation to "intentionally make… any unsolicited telephone sales call to any mobile" device. It also bars artificial-voice calls to residential phones without prior express consent, and requires an unblocked caller ID [fact: azleg.gov §44-1278]. Outbound recruiting calls or texts that sell paid introductions to contractors could be "sales calls" [opinion]. The package's "no recruiting calls or texts" rule (D2) is right and should stay. §44-1271's definitions were not fetched (could not verify).
- **AI disclosure and call recording.** The package announces recording in the first sentence, which is prudent. Arizona's recording-consent statute (A.R.S. §13-3005) and the rules for out-of-state callers could not be verified here. With the touch-tone design there is no recording, so the issue goes away [opinion].
- **Privacy and breach.** The package cites A.R.S. §18-552 notice duties (not re-verified here). The automated plan spreads personal data across Retell recordings (90 days), Anthropic (up to 30 days, with Managed Agents transcripts kept until deleted [fact]) and Render. The privacy policy's "not kept" claim is wrong [fact].
- **Non-circumvention.** The 90-day, fee-only remedy is reasonable. Enforcing it automatically from call logs may read as a penalty if the logs are wrong. Keep a human decision on any charge [opinion].
- **AI insurance exclusions.** CG 40 47 01 26 (Coverage A and B), CG 40 48 01 26 (B only) and CG 35 08 01 26 exist as optional endorsements [fact: Vorp Labs ledger; IndependentAgent.com]. Require written confirmation that none is attached, or a carve-back, before binding.

### 5.4 Technical feasibility (Question 6)

- **Single points of failure.**
  - One Render web service and one database.
  - Retell, Twilio's number pool and A2P status, and Stripe.
  - Casey's phone for alerts and Casey's computer for ROC checks.
  - A maintenance agent that auto-deploys code.
  - Away mode applies defaults after 7 days of silence, which is long for live requests [fact: F risk 1; opinion].
- **What the stress test misses** [opinion]:
  - Real carrier filtering and delivery.
  - Prompt injection through caller speech or texts.
  - Spikes after a rack-card drop.
  - Model version changes and drift over months.
  - Contractor behavior: slow quotes, going around the fee.
  - Real disputes and chargebacks.
  - Seasonal surges.
- **Realistic for a non-coder?** The full design is not realistic. A web form, Stripe Payment Links, a spreadsheet or small database and fixed templates are realistic [opinion].

### 5.5 What's missing (Question 7)

- Demand re-estimated without Ask Hava (supplied here in §5.2).
- Google Business Profile eligibility.
- Owner pay and the value of Casey's time.
- Tax.
- A bound insurance quote.
- A shutdown and data-deletion plan.
- A breach-response runbook.
- Contractor willingness-to-pay evidence.
- Competitors' response (Angi, Thumbtack, contractors' own listings).
- Callers who can't use the system (Spanish speakers are only logged; accessibility).
- How second-home owners will be reached.
- A cap on total acceptable loss.

### 5.6 Cheaper and faster ways (Question 8)

1. **Run the approved hand-run test as designed**, with Google Voice by hand, the gate first, and $1,500–$2,300 in cash.
2. **Run a demand smoke test that collects no personal data**, before the gate. Rack cards and posts point to a "coming soon" page or a number with a recorded message. Count visits and calls only, and take no requests. Ask counsel whether this stays within the gate (opinion; it takes no homeowner request).
3. **Get non-binding contractor interest during Phase 2 visits.** Show mock job cards at $45/$59/$149 and record yes or no. Nothing is signed before the gate.
4. **If you automate later, automate only the deterministic parts**: form, payments and templates.

### 5.7 What should NOT be automated (Question 9)

- Anything that touches homeowner personal data with AI.
- Emergency triage: gas, sparks or flooding should get a fixed message telling the caller to call 911 or the utility.
- Refunds, appeals and disputes.
- The final decision on contractor licenses.
- Outbound AI-voice calls to homeowners.
- Contractor recruiting outreach.
- Production code deploys without review.
- Anything about money, consent or legal wording.

### 5.8 Questions for Casey (Question 10)

1. Is the goal $20,000–$50,000 a month or a year?
2. What is the most total cash you are willing to lose before stopping?
3. Will you keep the no-personal-data-in-AI rule, which rules out AI voice and AI text intake, or amend it in writing?
4. Do you still want the LLC and texting registration deferred behind the gate, given that the package records "decided: yes" for doing them early?
5. Will you run the hand-run test before any build?
6. Who covers if you are unavailable for a week?
7. If Twilio rejects texting permanently, is a no-text design acceptable?
8. How many hours can you spend during a build phase?
9. Are you willing to accept losing $300–$400 a month for a long ramp?
10. What measured weekly demand would make you stop for good?

---

## 6. Bottom line

The arithmetic in the package is correct. Base month 24 recomputes to −$213.6, and the CSV has no errors. But the business it models does not pay. Base never breaks even, and with Ask Hava at zero a realistic Base is about 50 requests a month, which loses about $366 a month at month 24 and about $14,000 over two years, against a break-even of 122 requests. If the goal is $20,000 a month rather than a year, it would need about 4,000 requests a month, which this city cannot supply. The automated design also breaks Casey's rule against putting homeowner data into AI, moves LLC and texting filings ahead of the legal gate, and depends on a texting registration that Twilio may permanently reject as lead generation. Do not build it. Run the approved hand-run test, or a cheaper no-data demand smoke test first, and keep homeowner contact details in plain code, away from any AI. Revisit automation only if measured demand runs near the break-even pace for several weeks.
