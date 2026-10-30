# Havasu Home Quotes: full live-business simulation brief

**For:** Grok (independent simulation and review)
**From:** Casey Solomon, owner, with Claude
**Date:** October 26, 2026
**Version of the business being tested:** Technical Blueprint v9.3, plus every change agreed on October 26, 2026 (Part 3).

---

## Part 0. Read this first

1. **This is a simulation only.** Every person, business, text, phone call and payment is invented. Do not contact anyone, sign up for anything, or use any real service. Label every number you report as simulated.
2. **Build and run it if you can execute code.** If you can't run code, say so plainly at the top of your report, reason through the scenarios by hand, and never present a made-up number as a measured result.
3. **Separate three kinds of numbers** everywhere in your report:
   - **Given:** a rule or price stated in this brief.
   - **Assumed:** a behavior rate you or this brief chose (for example, how often a business declines a $75 fee).
   - **Measured:** what your simulation produced.
4. **Never trust the app's own labels when judging it.** Judge every check from what the outside world saw: texts actually delivered, payments Stripe actually took, calls actually connected, and what each simulated person actually did. An app bug must not be able to hide itself.
5. **Use fixed random seeds** (1 to 50) so a re-run gives the same results, and print the seeds.
6. **Be blunt.** The goal is to find every problem before this goes live. A report that finds nothing is less useful than one that finds ten real problems.

---

## Part 1. What we want from you

1. **Build a full-business simulator** from the rules in Parts 2 to 8: homeowners, businesses, Casey, and fake versions of every vendor (Twilio texts, calls and Verify; Stripe; Claude API; the state license site; the city license list; email; the mail service). Every request should be a complete transaction from first text to Done or Closed.
2. **Run every scenario in Part 10,** 50 seeds each. Use 60-day runs for most of them and a 24-month run for the business-growth scenario.
3. **Test every function and feature** listed in Part 11. For each one, report pass or fail and give one example.
4. **Model how everyone reacts to price** (Part 9): which businesses join, stay, quote, confirm and pay, and which quit, at the fee structure in Part 6 and the alternatives listed there.
5. **Report the money** (Part 12): revenue, refunds, every vendor cost, fixed costs and profit, by month. Also report the requests per day needed to reach $20,000 and $50,000 profit a year.
6. **Report Casey's workload:** minutes per week, broken down by task, and what happens when he's on shift or away.
7. **Find problems.** Rank every broken promise, money error, privacy leak, compliance risk and stuck job by severity. Give a reproduction for each one: seed, day, and the sequence of events.
8. **Recommend fixes,** and say which open decisions (Part 13) your results favor.
9. **Use the report template** in Part 14.

---

## Part 2. The business in one paragraph

Residents of Lake Havasu City, Arizona (later Bullhead City, Fort Mohave and Kingman) text one local number or use web chat to say what service they need. That can be a repair, a recurring service like pool cleaning, or a one-time job like car detailing. The app sends an anonymous **job card** to every qualified local business that opted in to that type of job. Businesses send quotes. The homeowner sees **every quote** on one private quote page, with links to reviews and the state license record. The homeowner picks one by entering a code texted to their own phone. The business taps **Confirm** within 2 business hours, and only then pays a fee of **$25, $75 or $199** depending on the size of the chosen quote. Only after payment does the business get the homeowner's first name, address and photos. It calls through a **masked number**; the homeowner's phone number is never shared. **Homeowners never pay.** Positioning: "We gather quotes; you choose. We're just compiling the information." Owner: Casey Solomon, who has a full-time day job, so his available time is limited and comes in shifts.

---

## Part 3. Changes agreed on October 26, 2026 (on top of v9.3)

These are part of the system you are testing. Where v9.3 (Part 5) and this table disagree, **this table wins.**

| # | Area | Rule now |
|---|---|---|
| 1 | First text | Also says the business you pick pays us a small fee. Homeowners never pay. |
| 2 | Quotes to homeowner | The homeowner is texted when the **first** quote arrives, with a note that more are coming. They can pick any quote at any time. |
| 3 | Quote window | Businesses get up to **24 hours** to quote. Open decision: count weekdays only, or every day. Test both (Part 13). |
| 4 | New-quote texts | At most one update text every few hours ("You now have 4 quotes"), not one per quote. The private page always shows every quote. |
| 5 | Losing businesses | Every business that quoted gets "This job is no longer active" when the homeowner picks someone else or cancels. |
| 6 | Emergencies | The safety text says **why** (for example, "A rotten-egg smell can mean a gas leak. Leave the area, don't flip switches, call 911 or the gas company."). Once the homeowner replies that they're safe, the app asks whether the repair is urgent. It explains that after-hours work usually costs more and lets them choose emergency or regular. Emergency jobs go only to businesses that ticked "after-hours emergencies." |
| 7 | After-hours businesses | Businesses can tick "after-hours emergencies" on their checklist. |
| 8 | Spanish | Every homeowner text goes out in Spanish. The job card says "Homeowner prefers Spanish." Businesses can tick "We speak Spanish," which shows on their quote card. Jobs are **not** routed by language. |
| 9 | Tenants | Tenant requests go out normally. The job card says "Requester is a tenant." Businesses can tick "skip tenant requests." |
| 10 | Business questions | Any free-text question from a business about a live pick ("Got the notice, what now?") gets a short reply with the Confirm link and the next step. |
| 11 | Complaints | Reply with an apology. Explain the business is independent and responsible for its own work. Send the business's own phone number, the job details and the state ROC complaint link (for licensed trades). A business is paused only after **3 complaints in 90 days**, pending Casey's review. |
| 12 | Business texts in | Messages like "How do I get on your list?" are recognized as a business and answered with the sign-up link. They are never filed as a repair request. |
| 13 | Identity check | Order of methods: a code texted to the phone on the state or city record; then an automated voice call reading the code (for landlines); then email to the address on record; then an automatically mailed letter as a last resort. |
| 14 | License at sign-up | The license class is read from the state record and read back to the business ("Your R-39 covers AC repair, heating and system replacement. Which do you want?"). Any extra job type is allowed only if that license covers it or it needs no license. |
| 15 | STOP, then coming back | The STOP confirmation says "To use us again, text START." Casey's call-back list is only for rare cases. |
| 16 | Services covered | **Any local service business** (Part 4), including recurring ones. |
| 17 | Prices | Part 6. |
| 18 | Business Plan v9 fixes | No cap on businesses per job or per trade (every qualified business that opted in gets every matching card). Unreachable-homeowner refund rule: 2 call attempts **at least 3 hours apart** within 72 hours. Picking uses a code texted to the homeowner's phone, or "2 YES" by text. |

---

## Part 4. Service categories, licenses and matching

### 4.1 Categories in this simulation

These are the 25 categories to simulate. **Share of picks** and **typical first-year value** are assumptions taken from Phoenix, national and local price research. Give each job a random spread around the typical value: a lognormal with sigma 0.6 is our assumption, and you may change it if you explain why.

| Category | Share of picks | Typical value (first year for recurring) | Recurring? | License rule |
|---|---|---|---|---|
| Plumbing repair | 13% | $300 | No | ROC R-37 (handyman under $1,000, no permit, no gas) |
| HVAC repair | 12% | $450 | No | ROC R-39 or R-39R |
| HVAC replacement | 2% | $9,000 | No | ROC R-39 or R-39R |
| Electrical | 7% | $350 | No | ROC R-11 |
| Pool repair | 4% | $500 | No | ROC R-6 or B-6 (gas pool heaters never handyman) |
| Handyman | 8% | $250 | No | None if under $1,000, no permit, no gas, no hardwired smoke alarms or sprinklers |
| Pool cleaning | 6% | $1,400 a year ($116 a month) | Yes | None (attorney to confirm) |
| Landscape maintenance | 5% | $2,000 a year | Yes | None (gardener exemption) |
| House cleaning (every 2 weeks) | 5% | $3,500 a year | Yes | None |
| Pest control (quarterly) | 4% | $500 a year | Yes | AZ Dept of Agriculture pest license |
| Dog waste pickup | 1% | $600 a year | Yes | None |
| Trash-can cleaning | 1% | $300 a year | Yes | None |
| Vacation-rental turnover cleaning | 2% | $4,000 a year | Yes | None |
| Junk removal | 3% | $250 | No | None |
| Car detailing | 3% | $250 | No | None |
| Boat detailing | 3% | $700 | No | None |
| Boat repair | 4% | $900 | No | None |
| Window cleaning | 2% | $175 | No | None |
| Pressure washing | 2% | $350 | No | None (sealing or painting is R-34) |
| Solar screens | 2% | $1,500 | No | Depends (attorney to confirm; window film is R-65) |
| Roof repair | 2% | $1,100 | No | ROC R-42 |
| Painting | 3% | $2,000 | No | ROC R-34 |
| Movers (local) | 2% | $1,200 | No | None in-state |
| Appliance repair | 2% | $250 | No | ROC R-63 or none (attorney to confirm); gas appliances never handyman |
| Garage doors | 2% | $350 | No | ROC (class unclear) |

**Not offered at launch** (requests are answered right away and logged as recruiting demand): childcare, home care, massage, mobile hair and nails, mobile vets, catering, towing, property management.

### 4.2 License and identity rules

- **City business license.** Every business inside Lake Havasu City needs one. The city publishes the active list as a monthly spreadsheet. The app checks the business name against it for **every** category, including unlicensed ones.
- **ROC (Arizona Registrar of Contractors).** The license class and status are read from the state record at sign-up, never typed in by the business. The record is re-checked twice a day, and again before a job card goes out if the last check is over 12 hours old. License end dates and bond cancellation dates pause a business on that date. A misread page never changes a verified status. If the site is down, shows a CAPTCHA or has changed its layout, the result is "unknown": the business stays as it was for up to 24 hours, then is paused.
- **Pest control.** Checked on the AZ Department of Agriculture license search. A request for "weed control" or "pre-emergent" goes **only** to licensed pest-control businesses, never to a gardener.
- **Handyman exemption** (A.R.S. 32-1121(A)(14)). The total job, labor plus materials, must be **under $1,000**. No permit can be required, and the job can't be split to get under the limit. **Gas work and hardwired smoke alarms or fire sprinklers are never exempt, at any price.** A handyman quote of $1,000 or more is refused with an explanation. A handyman quote on a gas job is refused.
- **One free first win per verified business,** checked by ROC number and business entity when the win is used, not only at sign-up.

### 4.3 How a job reaches businesses

1. The AI reads the request twice, and plain keyword rules give an independent second opinion on category and job type.
2. If the category is unclear, the homeowner is asked "Is this mainly 1) X or 2) Y?" Answers in words count too ("the AC one"). Allow one re-ask, then a fallback.
3. If the possible job types need different licenses, the homeowner is asked which fits best.
4. A job card goes to **every** active, verified business in that town whose license covers every remaining candidate job type and which ticked at least one of them. There is no cap.
5. Fallback: businesses that switched on "also send me jobs nobody else covers," if their license allows it.
6. If no qualified business covers the job, the homeowner hears so right away, and the request is logged as recruiting demand.
7. Neighboring towns follow the same rules.
8. Job cards never contain the homeowner's name, phone, street address, or photos showing faces, house numbers or documents. The last pass is plain code: anything shaped like a name, a contact phrase, a run of number words, or 7 or more digits removes the rest of that sentence.

---

## Part 5. The v9.3 system (unchanged parts)

### 5.1 Job lifecycle

Every request has exactly one status and can only move along these steps. Any other move is refused and logged as a bug.

```
Intake -> Open -> (No quotes yet) -> Quotes ready -> Awaiting confirm -> Payment pending -> Connected -> Done
   |        +-> Not covered
   +-> Closed early (emergency, out of area, category not open, duplicate)
Any status before Done -> Cancelled
```

| Status | Moves on when | Next |
|---|---|---|
| Intake | Consent given, questions answered, safety check, area check, duplicate check | Open, or Closed early |
| Open | First quote arrives (change 2) | Quotes ready; with no quote by 9 p.m. the next day, No quotes yet ("no quotes yet" text goes at 6 p.m.) |
| No quotes yet | A quote arrives | Quotes ready; after 5 days with none, Closed |
| Quotes ready | The homeowner picks on the page with the texted code, or replies "2 YES" | Awaiting confirm; after 3 days with no pick, Closed |
| Awaiting confirm | The business taps Confirm within 2 business hours; license re-checked at this moment | Payment pending; no confirm means back to Quotes ready (next quote offered) |
| Payment pending | Stripe confirms the charge, or it's a free first win | Connected; a declined card or no card within 1 business hour means back to Quotes ready |
| Connected | The homeowner replies DONE, NO or PROBLEM at day 3 or day 14 | Done or Closed; never called within 72 hours means back to Quotes ready |

- **Timers** run on the business clock (8 a.m. to 9 p.m.) and live in the database. Every timer checks the current status first and does nothing if the job has moved on.
- **One active job per homeowner, property and category.** A second message within 2 hours is the same job. After 2 hours, the same category counts as the same job only if the description mostly matches.
- **Messages of one or two words never open a job.**

### 5.2 Order of operations for money and details (enforced)

1. The homeowner confirms the pick: a Twilio Verify code entered on the page, or "2 YES" from their own phone.
2. The business taps Confirm within 2 business hours.
3. The license and account status are re-checked at the pick and again at Confirm.
4. Stripe charges the saved card, or the free first win is recorded.
5. Stripe's "payment succeeded" notice arrives, or the app asks Stripe directly after 30 minutes.
6. Only then are the first name, address and full photos released and the masked call line opened. The homeowner's phone number is never given out.

### 5.3 Texting rules

- **First text to a homeowner (consent prompt):** "Hi, this is Havasu Home Quotes. We gather quotes so you don't have to make the calls. It's free for you, and the business you choose pays us a small fee. Msg frequency varies by request. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms and privacy: hhq.example/terms. Reply OK to start." Consent accepts OK, Yes, Sure and Sí, and the record keeps the time, the exact reply and the prompt version.
- **HELP / INFO:** brand, help email, phone and the STOP instruction, answered within 5 minutes.
- **Opt-out words:** STOP, CANCEL, END, QUIT and UNSUBSCRIBE stop all texts, close jobs that haven't reached a business, and get exactly one confirmation. A connected job with no call yet is closed and refunded. "Please stop texting me" and similar wording count as opt-outs too. A message that merely contains "stop" ("can stop by at 3") does not.
- **Closing a job:** texts never say "reply CANCEL." They say "Reply CLOSE 1042." "Please cancel" in a sentence closes the newest open request and says so.
- **Quiet hours** (9 p.m. to 8 a.m.): only safety messages, STOP and HELP replies, pick codes, and direct replies within 15 minutes of the person's own text. Retries follow the same rules. Held texts are dropped if the job closed overnight.
- **Retries:** important texts are retried up to 3 times over 30 minutes, then sent by email and shown in the portal. Carrier error 21610 (recipient opted out) is never retried.
- **No marketing at all,** and links only on our own domain.
- **No backup texting number** during a suspension. Twilio warns that moving traffic to another campaign can get the account suspended or terminated. Instead: email anyone with an email on file, and put everyone else on Casey's call list.
- **Opted-out homeowner texts again:** the carrier blocks our replies until they send START. The STOP confirmation tells them how to come back (change 15). Email them if possible; otherwise add them to Casey's call-back list. A possible emergency from an opted-out number is a critical alert.
- **Staff phones:** a blocked call triggers "Reply LINK 4321 to add this phone." If nobody has got through 45 minutes later, the business is asked to call from its main phone.

### 5.4 AI rules

The AI does five narrow jobs with fixed outputs:
- reading the request (category, job type, emergency, urgent, language, rental, major job);
- scrubbing the description;
- reading quotes (type, amount, earliest arrival, confidence);
- screening photos;
- a weekly summary built from counts only.

It never writes texts to homeowners, never moves money, and never releases details. A quote read with less than 80% confidence goes back to the business to confirm. A quote containing phone numbers, links or spelled-out numbers bounces back. A plain keyword list (smell, burning, alarm, smoke, sparks, flooding, dizzy, head hurts, plus Spanish equivalents) runs on **every** message before anything else and triggers the safety message whatever the AI says. Prompt injection is treated as data: the app reads only fixed output fields.

### 5.5 Payments and refunds

- **Business payment setup:** a Stripe customer is created at sign-up, and no card is needed until after the free first win. At the second win, the business adds a card or bank account; if that fails, it has 1 business hour before the homeowner is offered the next quote. Charges are made off-session, with a unique key for each attempt (pick-1042-try1, try2). A bank that needs approval (3-D Secure) gets 1 business hour.
- **Charges happen only while the job is live.** Before every attempt, the app checks the job is still waiting for payment. If Stripe is unreachable for 4 hours, the pick is released. Money that arrives for a closed job is refunded automatically, and Casey is told.
- **Automatic refunds, each exactly once:**
  - homeowner unreachable: no call that a person answered and that lasted 45 seconds or more, after 2 attempts at least 3 hours apart within 72 hours; decided at 96 hours;
  - homeowner cancels before any call;
  - payment lands after the job moved on.
- **Disputes:** a dispute alerts Casey and costs the $15 Stripe fee. A refund on a disputed charge is blocked and flagged.
- **Failed refunds** are retried hourly, with an alert.
- **The ledger** is append-only and checked against Stripe every night. The owner and business money views must match Stripe to the cent.
- **Repeat work** from the same customer through "Request this business again" is never charged.

### 5.6 Portals

- **Homeowner:** sign in with phone and texted code. Screens: my jobs, job detail with every quote card, past jobs, cancel, ask a question, report a problem, my info, delete my data.
- **Business:**
  - Today: cards to quote, picks to confirm with a countdown, homeowners to call.
  - Jobs and quotes.
  - Billing, with statements.
  - Stats.
  - Profile and compliance.
  - Settings: pause, towns, job-type checklist, after-hours, Spanish, skip tenants, maximum cards per week.
- **Owner (Casey):**
  - Home numbers and a "Needs you" list.
  - Jobs and businesses.
  - Money and funnel views.
  - Pause switches: all, town, category, charging, texting.
  - System health.
  - Audit log.
  - A weekly spot-check of 10 job cards.
  - The flag for quotes that cluster just under $500 or $1,500.

### 5.7 Monitoring and backup

- **Alerts:**
  - text delivery below 95% for 30 minutes;
  - Twilio suspension;
  - Stripe or webhook errors lasting more than 30 minutes;
  - app down;
  - any ledger-versus-Stripe difference;
  - any complaint, safety report or dispute;
  - a category or town with fewer than 2 active businesses.
- **Escalation:** a high alert unanswered for 24 hours triggers safe mode and texts the backup person.
- **Database restore:** after a point-in-time restore, the lost window is replayed in time order from the vendor records (Twilio, Stripe events, web request log, audit stream, AI readings). IDs are derived from what they identify, so links, pick IDs and Stripe keys are recreated exactly, and nothing is sent twice.

---

## Part 6. Prices

### 6.1 The structure being tested (Casey's choice)

| Tier | Chosen quote | Fee to the business |
|---|---|---|
| Small | Under $500 | $25 |
| Medium | $500 to $1,500 | $75 |
| Large | Over $1,500 | $199 |

- **Recurring services** count as **2 months** of their price. A $150-a-month pool client counts as a $300 quote, so the fee is $25.
- **Price range quotes** use the midpoint.
- **Trip fee + estimate on site quotes** use a set tier per category (for example, small for most repairs, large for HVAC replacement), so a business can't drop to $25 by quoting only a trip charge.
- **No percentage cap.** The fee changes only at $500 and $1,500.
- **Free first win:** one per verified business. Variant to test: free first win only for businesses that join in the first 90 days.
- Card fees are 2.9% + 30¢ per charge (bank payments 0.8%, capped at $5). The dispute fee is $15. Stripe keeps its fee on refunds.
- Homeowners never pay.

### 6.2 Alternatives to compare against

1. $19 / $59 / $199
2. $25 / $75 / $300
3. Flat $25 for everything
4. 5% of the chosen quote, $15 minimum, $250 maximum
5. $25 / $75 / $199 with recurring counted at 4 months instead of 2

### 6.3 What businesses pay elsewhere (for the price-reaction model)

These are published third-party figures, not official platform prices. Treat them as rough.

| Trade | Price per lead elsewhere (won or lost) | Cost to win one customer |
|---|---|---|
| HVAC | Google Local Services $51–$80; Google Ads $85–$149; shared leads $35–$110 | about $288 (2026 survey of 760 businesses) |
| Plumbing | Google Local $57–$69; Google Ads $76–$183; shared $30–$100 | about $263 |
| Electrical | Google Local $39; shared $25–$80 | about $218 |
| Roofing | Google Local $162; Google Ads $228 | about $369 (Google Local price ÷ 43.9% booking rate) |
| Handyman | Google Local $34; shared $8–$30 | about $77 (same method) |
| Landscaping | Google Local $39; Angi $15–$40; Thumbtack $10–$20 | about $89 |
| Pool service | Google Ads $45; maintenance leads $20–$80 | about $150 (one owner's report) |
| House cleaning | Google Ads $47; Thumbtack $10–$50 | not found |
| Pest control | Google Local $20–$30; Google Ads $40–$150 | about $46–$68 |
| Junk removal | Google Ads about $90–$120 (proxy) | not found |

Shared-lead platforms usually send each lead to 3–5 businesses. Businesses typically win 5–15% of shared leads and book about 40–45% of Google Local Services calls.

---

## Part 7. Vendors to fake, with prices and failure behavior

| Vendor | Price used | Failure behavior to simulate (assumed rates; vary them in the chaos scenario) |
|---|---|---|
| Twilio texts (one local 928 number) | $0.0083 per segment + about $0.004 carrier fee; MMS $0.022; number $1.15 a month | 1.5% fail, 1% delayed, 2% duplicate status notices, 0.5% lost notices. STOP handled at the carrier. Possible suspension (Part 10). |
| Twilio texting registration (A2P 10DLC) | Brand $4.50; campaign vetting $15 one-time; $1.50 a month | Registration can be rejected or delayed. Opt-out and complaint rates affect suspension risk. |
| Twilio masked calls | $0.0085 a minute in + $0.014 a minute out | Calls from unlinked phones need the 4-digit job PIN. A call counts only if a person answered and it lasted 45 seconds or more. Voicemail and call screening happen. |
| Twilio Verify (pick codes, sign-in, identity) | $0.05 per success + $0.0083 per text; voice-call codes cost more (use about $0.06) | 5 tries, then a 15-minute lockout. Some phones on the state record are landlines. |
| Stripe | 2.9% + 30¢; ACH 0.8% capped at $5; disputes $15 | 3% card declines, bank approvals needed, lost and late webhooks, outages up to several hours, refunds that fail, disputes |
| Claude API (Haiku 5.5, zero retention) | $0.10 / $0.50 per million tokens in / out; about 2¢ per request | Misreads category 4% of the time and job type 6%. Misses indirect emergencies 30% of the time. Falls for prompt injection 30% of the time. Misses 10% of identifying photos. Misreads quotes 3% of the time. **A re-read usually repeats the same mistake.** Outages. |
| Arizona ROC license site (automated browser checks) | Included in the Claude plan | Down 2% of the time, CAPTCHA 5%, layout change 1%. Licenses lapse mid-run; borrowed license numbers; outdated phone on the record. |
| AZ Dept of Agriculture pest license search | Same | Same failure rates as ROC |
| Lake Havasu City business license list (monthly XLSX) | Free | Up to a month out of date; name mismatches (DBA vs legal name) |
| Resend email | Free to 3,000 a month | Bounces |
| Mailed identity letter (Lob or PostGrid type) | About $1 a letter (assumption, not verified) | 3–7 day delivery; some never arrive |
| Render (app and database) | $7 + $19 a month | Outages; one point-in-time restore losing 3 hours (Part 10) |
| Sentry, UptimeRobot | Free | — |

**Fixed costs: about $157 a month.** That's insurance about $90, hosting and database about $30, Claude plan about $25, statutory agent about $8, and number, registration and domain about $4. No paid ads.

---

## Part 8. The people to simulate

### 8.1 Demand (Lake Havasu City facts, then assumptions)

**Facts:**
- 57,144 residents and 35,410 housing units at the 2020 Census.
- About 26% of homes had no full-time household (part-time owners and snowbirds).
- About 2,000 vacation rentals, about 1,200 of them registered.
- Median age about 56.
- Summer highs average 105–109°F from June to August.
- Boating season runs March to September.
- The city issued more pool permits than new-house permits in FY 2025/26.

**Assumptions:**

| Item | Assumption |
|---|---|
| Requests per day | Scenario-dependent: 2.5, 5, 10, 16 or 25 |
| Hours | Mostly 7 a.m. to 9 p.m.; about 5% overnight |
| Seasons | HVAC repairs triple from June to September. Boat work doubles from March to September. Cleaning, home watch and turnover cleaning rise during October–April (snowbirds) and in summer weekends (rentals). |
| Message includes ZIP code | 60% |
| Sends a photo | 50% |
| Repeat homeowner within 60 days | 10% |
| Gives an email when asked | 30% |

**Homeowner personas** (share of requests):

| Persona | Share | Behavior |
|---|---|---|
| Normal | 52% | Answers questions, picks within a day |
| Impatient | 6% | Texts "any quotes yet?" repeatedly; picks the first quote |
| No links | 6% | Never opens links; uses LIST and "2 YES" |
| Spanish speaker | 4% | Writes in Spanish only |
| Tenant | 5% | Rents; landlord may or may not be involved |
| Rental owner / absent owner | 5% | Someone else meets the business |
| Emergency (real or worded indirectly) | 3% | "Pilot light smells funny," "breaker is hot" |
| Over-sharer | 6% | Puts name, phone and address in the description |
| Link forwarder | 4% | Forwards the quote page to a spouse who tries to pick |
| Call screener | 4% | Doesn't answer unknown numbers |
| Canceller | 3% | "Never mind, neighbor fixed it" at a random point |
| Prompt injector | 1% | Tries to get phone numbers out of the AI |
| STOP-then-returns | 1% | Texts STOP, comes back months later |

### 8.2 Businesses

**Pool of businesses in town** (from local directory counts; assumption: 15–30% of them opt in during the first year):

| Category group | Businesses in town |
|---|---|
| Plumbing | about 22 |
| HVAC | about 23 |
| Electrical | about 31 |
| Pool (cleaning and repair) | about 19–50 |
| Landscaping | about 25–47 |
| Cleaning | about 25 |
| Pest control | about 12–19 |
| Boat repair | about 36–57 |
| Boat and car detailing | about 25 |
| Handyman | about 7 |
| Roofing | about 18 |
| Movers and junk | 2–5 each |
| Trash-can cleaning | 1 |
| Dog waste pickup | 0 found |

**Business personas** (share of businesses):

| Persona | Share | Behavior |
|---|---|---|
| Responsive | 30% | Quotes within an hour or two, confirms fast |
| Busy | 15% | Quotes late or not at all |
| Link-averse | 10% | Quotes by text ("150 THU AM") |
| Fee bypasser | 5% | Tries to slip a phone number into a quote, or contacts the homeowner off-platform |
| No-show | 4% | Pays, then never calls |
| License lapsing | 4% | License or bond lapses mid-run |
| Insurance lapsing | 4% | Certificate expires |
| Multi-staff | 8% | Techs call from unlinked phones |
| "Stop by" texter | 3% | Writes "can stop by at 3" |
| Outdated state record | 6% | Phone on record is old or a landline |
| Tier-line shader | 5% | Quotes $480 instead of $520, or $1,450 instead of $1,600, then adds on site |
| Trip-fee gamer | 3% | Quotes only a trip fee on big jobs |
| Small-job decliner | 3% | Turns down picks under about $200 |
| Fraud / fake business | 2 per run | Borrowed license, fake identity |

### 8.3 Casey

- Someone with a full-time job on rotating shifts, available about 1 hour a day in two windows. Model shift weeks, including a 2-week stretch away.
- Approves flagged sign-ups. Assumed: approves 95% of legitimate ones; fooled by a fake 15% of the time, or 3% with the checklist (call the phone on the state record, compare ID and license card).
- Handles complaints, disputes, call-back lists and alerts. Each task takes time; report minutes.
- A backup person can pause and approve, but can't change bank details.

---

## Part 9. How everyone reacts to price (model this explicitly)

1. **Confirm and pay.** When picked, a business confirms and pays with probability `1 / (1 + (s / s50)^3)`, where `s` is the fee as a share of the job's value (first-year value for recurring services). Run three settings: **s50 = 10%** (price-sensitive), **15%** (middle) and **20%** (less sensitive). These are assumptions; say if you think the curve is wrong and test your own.
2. **Joining.** A business joins if the fee looks better than what it pays elsewhere (Part 6.3) for its category. Use your judgment and state your rule.
3. **Leaving.** A business quits or unticks job types after a run of fees it feels were too high relative to the job, or after too few wins. Report churn by category.
4. **Quoting.** Businesses quote fewer jobs in categories where their win rate is low, because more businesses compete for each card.
5. **Gaming.** Tier-line shaders and trip-fee gamers (Part 8.2) respond to the tier lines. Measure how much fee they avoid, and whether the owner-portal flag and the day-3 PROBLEM reply catch them.
6. **Homeowners** never pay, but measure whether more quotes per job, faster first quotes, or fewer declined picks change how often they pick and how often they come back.

For each fee structure in Part 6, report: profit, share of picks confirmed, businesses joined and kept, quotes per job, and homeowner pick rate.

---

## Part 10. Scenarios (50 seeds each)

| # | Scenario | What's special |
|---|---|---|
| 1 | Normal | 5 requests a day, 60 days |
| 2 | Growth | Starts at 1 a day, rises to 10 a day by month 12 and 16 by month 24; 24 months; businesses join over time |
| 3 | Busy summer | 16 a day, HVAC-heavy, 110°F heat wave week |
| 4 | Snowbird season | October–April mix: more cleaning, home watch, rental turnovers |
| 5 | Boat season | March–September, boat work doubled |
| 6 | Vendor chaos | Every failure rate in Part 7 tripled; a 4-hour Stripe outage; an AI outage |
| 7 | Texting suspended | Twilio suspends the number on day 30 for 30 days |
| 8 | Adversarial | Fee bypassers, fake businesses, prompt injectors, tier-line shaders and trip-fee gamers all doubled |
| 9 | Casey away | 2 weeks unavailable; backup person only |
| 10 | Dispute wave | 5 disputes in one week |
| 11 | Same-minute collisions | 16 a day, every event snapped to the minute, ties in random order, each tie run both ways |
| 12 | Database restore | 3 hours of data lost on day 30, then replay |
| 13 | Crowded market | 15 businesses per category |
| 14 | Thin market | 1 business in half the new categories, 0 in some (dog waste, junk removal outside Havasu) |
| 15 | Spanish-heavy | 20% Spanish speakers |
| 16 | Price sensitivity | Scenario 1 at each fee structure in Part 6, at s50 = 10%, 15% and 20% |
| 17 | Your choice | At least 5 scenarios you think are most likely to break the business |

---

## Part 11. Checks: every function and feature (report pass or fail with an example)

**Money**
- A charge happens only after the homeowner's own phone said yes and the business confirmed in time, only while the job is live, exactly once, and only for a real introduction.
- The fee matches the tier for the chosen quote (including the recurring 2-month rule, ranges and trip-fee quotes).
- The free first win is used only once per business entity.
- The owner books and every business statement match Stripe to the cent.
- Refunds follow the published rules and happen exactly once. Disputes are handled. Failed refunds are retried.
- Money arriving for a closed job is refunded.

**Privacy**
- Contact details reach only the confirmed, paid, licensed business.
- Job cards contain no names, numbers, addresses or identifying photos.
- The AI never receives homeowner details unless zero retention is in place.
- The homeowner's phone number is never given out.

**Texting compliance**
- The first text has every disclosure. Only people who contacted us are texted.
- Quiet hours are respected.
- STOP, natural-language opt-outs and CANCEL work, with exactly one confirmation.
- HELP is answered within 5 minutes.
- No marketing. Links only on our domain.
- Spanish speakers get Spanish texts.
- Opt-out rate by audience against the internal alarms (3% homeowners, 5% businesses).

**Safety**
- An emergency gets the safety text, with the reason, within 5 minutes.
- No job card before the homeowner says they're safe.
- Emergency cards go only to after-hours businesses.

**Matching**
- Every card goes to a license that covers the job, and to every qualified business that opted in.
- Weed control goes only to pest licensees.
- Gas work never goes to a handyman.
- Handyman quotes of $1,000 or more are refused.
- Tenant and Spanish tags appear on cards. "Skip tenant requests" is honored.

**Homeowner experience**
- First-quote text arrives.
- At most one update text every few hours.
- The 9 p.m. promise is kept.
- Every new request hears back within 24 hours.
- Forwarded links can't pick.
- "Please cancel" closes the job.
- Losing businesses get "no longer active."

**Businesses**
- Sign-up recognized from a text.
- License read back.
- Identity check falls through text → voice → email → letter.
- Questions about a pick get answered.
- Staff phones can link.
- Complaints pause only at 3 in 90 days.
- Tier-line shading is flagged.

**Calls**
- Masked calls reach the right person.
- The job PIN works.
- Lines close on cancel, no-show or complaint.

**Nothing stuck**
- Every job and payment moves forward or ends.
- No timer fires on a job that has moved on.

**Restore**
- Job numbers are never reused.
- Every kept charge matches a pick.
- Nothing is sent twice.

**Casey**
- Minutes per week.
- Alerts unanswered for 24 hours go to the backup and safe mode.

---

## Part 12. Money report

For each scenario and fee structure, report by month:

- requests, picks, paid introductions, free first wins and refunds;
- revenue charged, refunds, Stripe fees and dispute fees;
- vendor costs: texts, MMS, Verify, calls, AI, email and letters;
- fixed costs ($157 a month);
- net profit;
- cost per request;
- average fee per paid introduction.

Also report:
- requests per day needed for **$20,000** and **$50,000** profit a year;
- the month the growth scenario reaches each of those paces;
- the lowest cash point, assuming $1,000 operating cash plus a $500 Stripe reserve at launch;
- the cost of a 30-day texting suspension, in money and in Casey's hours.

---

## Part 13. Open decisions (tell us which your results favor)

1. The 24-hour quote window counted on weekdays only, or every day.
2. Free first win for every business, or only during the first 90 days.
3. Recurring services counted at 2 months or 4 months.
4. $25 / $75 / $199, or one of the alternatives in Part 6.2.
5. Whether recurring services should get a partial refund if the homeowner stops within 30 days (needs a day-30 "Still using them?" text).
6. Unlicensed categories: require an insurance certificate, or only show "insurance on file: yes / no."
7. License data: live ROC lookups, or the ROC's downloadable lists checked daily with a live lookup only at the pick.

---

## Part 14. Report template

1. **Could you run code?** Yes or no. If no, what you did instead.
2. **Summary:** the 5 most important findings, one line each.
3. **Problems found,** ranked by severity:

   | # | Severity | Problem | Scenario, seed, day | What happened, step by step | Suggested fix |
   |---|---|---|---|---|---|

4. **Checks table:** every check in Part 11, with pass or fail, how many failures per run, and one example.
5. **Money:** the tables from Part 12, with given, assumed and measured numbers labeled.
6. **Price reaction:** results from Part 9 for every fee structure, at every s50 setting.
7. **Casey's workload:** minutes per week by task, per scenario.
8. **Vendor behavior:** how Twilio, Stripe, the AI and the license sites behaved, and what broke.
9. **Open decisions:** your recommendation on each item in Part 13, with the numbers behind it.
10. **What the model can't tell us,** and what the hand-run test must measure to replace the assumptions.
11. **Your code,** in full, so we can re-run it.

---

## Sources behind the numbers

- **Arizona law:**
  - [A.R.S. 32-1121 exemptions (current text shown in HB2120, 2025)](https://www.azleg.gov/legtext/57leg/1R/bills/hb2120p.htm)
  - [ROC license classes, A.A.C. R4-9-103](https://www.law.cornell.edu/regulations/arizona/Ariz-Admin-Code-SS-R4-9-103)
  - [Weed control exemption, A.R.S. 3-3613](https://law.justia.com/codes/arizona/title-3/section-3-3613/)
  - [Lake Havasu City business license](https://www.lhcaz.gov/175/Business-License)
- **Local demand:**
  - [Lake Havasu City (2020 Census figures)](https://en.wikipedia.org/wiki/Lake_Havasu_City,_Arizona)
  - [Mohave County Housing Study 2023](https://www.mohave.gov/media/ybcmgt44/mohavecountyhousingreport0927234db9d898-fa49-41c6-bb79-12395dae97fe.pdf)
  - [City permit report, Feb 2026](https://www.lhcaz.gov/DocumentCenter/View/5282/February-2026-PDF)
  - [Lake Havasu pool route listing](https://www.businessbroker.net/business-for-sale/pool-service-route-in-lake-havasu-for-sale-havasu-city-arizona/589638.aspx)
- **Lead prices:**
  - [The Media Captain: Google Local Services lead costs](https://themediacaptain.com/google-local-service-ad-statistics/)
  - [Pipelineon: 2026 home service benchmarks](https://pipelineon.com/blog/home-service-marketing-benchmarks-2026/)
  - [Web Tonic: pest control](https://www.webtonic.io/blog/pest-control-google-ads-statistics)
  - [Pipelineon: pool service](https://pipelineon.com/blog/pool-service-marketing/)
- **Vendor prices:**
  - [Twilio SMS](https://www.twilio.com/en-us/sms/pricing/us)
  - [Twilio Voice](https://www.twilio.com/en-us/voice/pricing/us)
  - [Twilio Verify](https://www.twilio.com/en-us/verify/pricing)
  - [Stripe](https://stripe.com/pricing)
  - [Claude API](https://platform.claude.com/docs/en/about-claude/pricing)
  - [Render](https://render.com/pricing)
  - [Resend](https://www.resend.com/pricing)
- **Twilio rules:**
  - [Error 30951 (lead generation)](https://www.twilio.com/docs/api/errors/30951)
  - [Error 30033 (moving traffic during a suspension)](https://www.twilio.com/docs/api/errors/30033.md)
