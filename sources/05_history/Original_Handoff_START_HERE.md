# Havasu Home Quotes — START HERE (handoff for a fresh session)

**Owner:** Casey Solomon, Lake Havasu City, AZ (full-time day job; also owns Ask Hava, askhava.com)
**Prepared:** Oct 26, 2026, end of a long planning session
**Read this first, then the files it points to.** Everything the old conversation decided is in this zip.

---

## 1. What to do first in the new session

Casey wants a **role-play** before continuing, to make sure we're on the same page about every feature:

1. **Claude plays the customer** (a homeowner texting in, later a contractor). Casey answers as the business owner would by hand, as a human.
2. After each exchange, Claude checks whether the **technical design (v9.3)** does what Casey just described automatically, in the background. Flag every place where Casey's human handling and the design differ. Then ask Casey which one he wants, and note the change.
3. Then **switch roles**: Casey plays the client or contractor, and Claude plays the business (the app's texts and replies) using the exact v9.3 rules and wording.
4. Cover:
   - a normal job;
   - an emergency;
   - a Spanish speaker;
   - a renter or tenant;
   - "please cancel";
   - a contractor confirming, paying and calling;
   - a no-show and refund;
   - a complaint;
   - a contractor signing up (license type and job-type checklist);
   - a homeowner who texts STOP and comes back later.
5. Keep Casey's preferences in mind: **non-technical, plain language, step-by-step**, rigorous sourcing.

---

## 2. The business in one paragraph

Homeowners in Lake Havasu City (later Bullhead City, Fort Mohave, Kingman) text one number or use web chat to describe a home repair. The app sends an anonymous **job card** to every qualified local contractor who asked for that type of job. Contractors send quotes, and the homeowner sees **every quote** on a private quote page, with links to reviews and the state license record. The homeowner picks one by entering a code texted to their own phone. The contractor taps **Confirm** within 2 business hours and only then pays a fee: **$29 handyman / $45 plumbing, electrical, pool / $59 HVAC / $149 major jobs**. The first win is free (one per business), and a card is needed only at that first win. Only after payment does the contractor get the homeowner's first name, address and photos. They call through a **masked number**; the homeowner's phone number is never shared. **Homeowners never pay.** Positioning: "We gather quotes; you choose. We're just compiling the information." Wording: "ROC # shows active as of [date]."

---

## 3. Decisions Casey made (don't reopen without asking)

- **Goal:** $20k–$50k profit per **year**. **Hand-run test first,** then build. **LLC and EIN at the start of the build.**
- **Text and web only,** no AI voice line (the AI phone receptionist is a later "Pro" feature, on hold). Calls happen only through the masked line after a pick.
- **Every quote is shown,** not just 3. The private quote page replaced the old "reply A or B" texts.
- **Recruiting:** simple ads and Facebook group posts only, with no time spent calling contractors. No paid ads at the start ("nothing for ads").
- **License and insurance checks** are done by AI (Cowork) on the Arizona ROC site. It never guesses.
- **Matching (Casey's rule):** each business's license type comes from the state record at sign-up, plus a checklist of job types it wants. **Every qualified business that opted in gets the job card.** No cap of 4 per job, and no cap on businesses per trade.
- **Compliance is a must,** so Twilio has no reason to suspend us. There is **no backup texting number**, because Twilio forbids moving traffic to another campaign during a suspension.
- **Data rule:** zero-retention AI only. Homeowner details live in a plain-code contact vault; request IDs are used everywhere else.

## 4. Standing guardrails (from Casey's master file)

- No sign-ups, purchases, outreach, publishing, live messages, live charges or deployments without Casey's explicit go. AI never creates accounts or accepts terms for him.
- Never paste or store secrets (passwords, API keys) in chat or plain files.
- Never put homeowner names, phones, addresses or photos into an AI tool that keeps data; use request IDs.
- Don't claim something was tested unless it actually was. Describe the service truthfully everywhere.

---

## 5. Where things stand (v9.3)

- **Technical Blueprint v9.3** is the current, complete technical design (file below). It covers:
  - architecture and vendors;
  - data model;
  - how jobs reach businesses;
  - job lifecycle;
  - integrations;
  - texting compliance;
  - AI design;
  - payments;
  - portals;
  - security and monitoring;
  - backups and restore replay;
  - the digital twin;
  - problems found and fixed in 4 rounds;
  - the build plan.
- **The digital twin (simulator kit, `hhq_twin/`):**
  - It runs the whole business against fake Twilio, Stripe, Claude and ROC services.
  - 21 scenarios, 56 automatic checks, 19 step-by-step tests.
  - v9.3 passes all 19 tests; v9.1 fails 13.
  - Results over 60 days, 5 runs per scenario:
    - about 0–1 broken promises per run outside matching (original v9: 500–2,400);
    - 99% of qualified businesses reached per job, and 0.4% of job cards outside a license;
    - about 73 paid introductions per 60 days at 5 requests a day, and 212 at 16 a day;
    - vendor usage about 33–38¢ per request (about 70¢ with 15 businesses per trade).
  - A 30-day Twilio suspension would cost Casey about 50 hours of personal calls, so prevention matters most.
- **Running costs:** about **$157 a month fixed** (insurance about $90, database $30, Claude $25, agent $8, number and other $4), plus about 38–42¢ per request and Stripe 2.9% + 30¢.
- **Business plan v9 financials** (simulated):

  | Scenario | Year 1 | Year 2 | Month 24 |
  |---|---|---|---|
  | 5 requests a day | $10,725 | $19,347 | $2,040/mo |
  | Slow build | $2,115 | $13,034 | |
  | 2.5 a day | $4,344 | $8,800 | |

  About 5 booked jobs a day (about 16 requests a day) is roughly $6,600 a month, about $79k a year. These are from the business-level simulator (`hhq_sim/`), before the v9.3 matching and compliance changes.

## 6. Open items before launch (need outside sign-off)

- **Attorney:**
  - the job-type-to-license table;
  - the terms and privacy policy;
  - the texting registration package: use case Customer Care/Mixed, opt-in wording, sample messages, privacy clause "we don't share or sell mobile numbers or SMS opt-in data."
- **Twilio support:** describe the matching model and get their read before registering. Their lead-generation rules (error 30951) don't directly address this model.
- **Native Spanish speaker:** review `hhq_twin/templates_es.json`.
- **ROC site terms:** confirm automated lookups (about 35 a day) are allowed.
- **Render:** confirm point-in-time recovery on the chosen plan.
- **The real build must:**
  - lock each job's record while changing it;
  - write the off-database logs (audit stream, web request log, AI readings) so restores can be replayed.

---

## 7. Files in this zip

**1_current/ (use these)**

| File | What it is |
|---|---|
| `Havasu_Home_Quotes_Technical_Blueprint_v9.3.md` | The current technical design. Start here for how everything works. |
| `Havasu_Home_Quotes_Business_Plan_and_Blueprint_v9.md` | The current business plan: pitch, offer, fees, costs, financials, legal gate, launch steps. The technical blueprint v9.3 updates its technical parts. |
| `hhq_twin/` | The digital twin simulator kit, v9.3. Read its `README.md` to run it: `python3 twin.py`, `python3 review_tests.py`. Includes `templates_es.json` (Spanish drafts) and the latest results. |

**2_history/ (background; older versions)**

| File | What it is |
|---|---|
| `Havasu_Referral_COMPLETE_MASTER_FILE_2026-10-26.md` | Casey's master file from before this session: all earlier decisions and guardrails |
| `Havasu_Home_Quotes_Business_Plan_and_Blueprint_v8.md` | Previous business plan |
| `Havasu_Home_Quotes_Technical_Blueprint_v9.2.md` | Previous technical blueprint |
| `hhq_sim/` | Business-level financial simulator (v6 to v9 scenarios) behind the plan's financials |
| `Havasu_Home_Quotes_Simulation_for_Grok.md`, `..._v7_for_Grok.md` | Self-contained simulator packages sent to Grok for review |
| `reviews/` | Outside reviews Casey brought in: Grok reviews, the independent review findings, and the v8 independent stress-test report |

---

## 8. One-paragraph prompt to paste into the new session

> I'm Casey Solomon. Attached is a zip with everything for my Havasu Home Quotes business. Read START_HERE.md first, then the Technical Blueprint v9.3 and the Business Plan v9. Before we change anything, I want to role-play: you play a homeowner texting in, and I'll tell you how I'd handle it as a human. After each exchange, check whether the v9.3 design does that automatically, and flag any differences. Then we'll switch roles. Keep it plain-language and step by step.
