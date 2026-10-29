# Ask Hava master plan: where we are and what happens before the build

**For:** Casey · **Date:** October 28, 2026 · **Status:** the plan is finished; the build is on hold at Gate P until the hand-run test (`docs/pilot/hand-run-test.md`) has run. Nothing is built and nothing is live.

This is the only to-do list. The same file is in the repository as `docs/PREBUILD.md`. Rule 9 at the end says how it stays that way.

---

## 1. Where we are

**Decided and written down**
- What Ask Hava is: a quote relay that packages the job. A homeowner asks once and answers a few pro-level questions, with photos if easy; she approves the job card; local businesses quote in the same format per job type; she sees every quote, unranked, with license numbers and Google and Yelp links, and picks one; the picked business pays a fixed introduction fee ($25, $75 or $199 by job size). Ask Hava does no work and promotes no business.
- The plan: 674 numbered requirements in 20 spec files (10 withdrawn), 482 logged decisions, 172 build tasks with their prompts, 8 legal drafts, a license map, a question bank for 37 trades linked to the Grok trade knowledge base, 40 worked fee examples. Every repo check passes.
- One rule for every trade: if a license is needed, only licensed businesses get its jobs; if not, any legitimate business with a city license can. Every enabled service category is open (D-459).
- Proof first (D-455, D-468): a 6 week hand-run test by email and web form, with no texts; weeks 1 to 3 free and helped, weeks 4 to 6 hands off and paid in the pushed trades; it comes before `approved to build`. Customer details are encrypted on the way to the business and wherever they're kept (D-454).
- No calls to businesses, ever. Businesses get everything by email and on the website.
- The look: dark plaster and brass, chosen Oct 28 (D-451). `docs/design/DESIGN_SPEC.md` sets colors, fonts, textures and buttons; four mock-ups show the home page (phone and desktop), her quote page and the pick step. Every page the build makes follows it.
- Built in Rust (D-453): the whole server is Rust, and the server builds the pages. The compiler, not just tests, stops three mistakes: an address released without a verified payment, a job status someone forgot to handle, and one part of the app reaching into another. Python stays only in the build tools, the fake vendor services and the twin.
- No AI smell (D-452): a scan fails the build on machine-sounding words ("seamless", slogans in threes, em dashes, exclamation marks) and page habits, and a second reviewer reads all new customer copy aloud before it's handed in. The test is whether a local person would say it to a neighbour.

**Built:** nothing. No app code exists. No account is open. Nothing has been sent to Twilio, Stripe or any agency.

**Verified vs assumed**

| Verified (checked against a source or by a test) | Assumed (only the real world can answer) |
|---|---|
| Fee math, the 4-month rule and every worked example (repo tests) | Whether Twilio approves homeowner texting |
| ROC class scopes (A.A.C. R4-9-103, R4-9-104, the ROC class list, checked Oct 28) | Whether Stripe accepts the model, and charges without the business present (auto-confirm) |
| Statute text for pest control (A.R.S. 3-3601, 3-3615) and RESPA (12 C.F.R. 1024.14) | Whether real businesses quote anonymous job cards and catch picks by email (the hand-run test answers this first) |
| First reply (consent text): GSM-7, 2 segments each (counted) | The real fee mix, and whether $25 site visits help |
| The build runner: kit steps K0 to K9 ran, one practice task passed, one failed on purpose, one resumed | Whether a merge starts the next runner session (one task an hour assumed) |
| Every simulation number is marked [sim] and came from AI stand-ins | Whether homeowners trust a text-only, unnamed service |

---

## 1b. What changed in the pre-build engineering audit (October 28, D-479 and D-480)

One pull request. Report: `docs/audit/prebuild-audit-2026-10-28.md`. Verdict: `docs/build-readiness.md`, **not ready**, with the ordered list of what comes before the first build task (its section 4 is now part of this checklist as items 2a to 2d). New standing files: `docs/governance.md`, `docs/ai-operating-manual.md`, `docs/risk-register.md`. The plan is fingerprinted (`kit/plan.lock`); any change to a plan file without a logged decision fails the build. Progress is counted in verified requirements (`scripts/progress.py`). 28 security and privacy requirements were added with tests and owning tasks. Two harness tasks were added (T172 twin gate, T173 journeys in CI) and T001 was absorbed. Five decisions are yours: section 4, items E-1 to E-5.

## 1a. What changed in fix round 16 (October 28, ChatGPT's stress test)

One decision, D-468. ChatGPT showed the hand-run test couldn't prove the fee, because "it would be worth it" isn't paying. Now: weeks 1 to 3 are helped and free; weeks 4 to 6 are hands off, and picked businesses in the pushed trades pay the real fee before getting the details. Requests are marked Seeded, Partner or Organic, and only Organic ones count as demand. The scorecard judges 2-or-more-quote choice, slow tails, active (quoting) businesses, real payments and your minutes per request. Results are labeled as a winter test. The LLC, EIN and Stripe now start with the test (item 8a). Gate P needs real payments.

## 2. What changed in fix round 15 (October 28, the Hormozi, Martell and Koerner review)

One pull request. Decisions D-455 to D-467. Source: the review doc "Ask Hava: Hormozi, Martell & Koerner Plan Review" of commit `6285ec6`, then your answer: "So do what you recommend and identify what needs to be done before the build starts". Your trade call: "We need to run all service buisnesses".

| Change | Where |
|---|---|
| A 4 to 6 week hand-run test comes before `approved to build`; scope cut and fee level wait for one re-plan pass after it; no new requirements until then | `docs/pilot/hand-run-test.md`, section 3 below (D-455) |
| Recruiting mostly self-serve: hand-written, disclosed Facebook group posts, a few visits; never texts or calls; D-181's launch-week sign-ups superseded | BIZ-048, OPS-041 (D-456) |
| Where people came from: an optional sign-up question and `g` link codes | BIZ-053, INT-047 (D-457) |
| Scorecard: requests per week, by source, paid introductions against 15 / 50 / 115 a month, net per introduction, intake completion, sign-up speed, recruiting minutes | OPS-039, section 7 (D-458) |
| Every enabled category open; marketing pushes one or two seasonal trades | LIC-046, `categories.yaml` (D-459) |
| Town asked right after the coverage line; card auto-sends after 30 minutes; no "text, email or both" question | intake order, INT-041, TXT-018, privacy policy (D-460) |
| "All quotes are in" text, and a closing note | TXT-042, JOB-012 (D-461) |
| Business first days: sample pick test moved to the first real card; open jobs on the done screen | BIZ-042, BIZ-031 (D-462) |
| Why a business leaves or pauses | BIZ-054 (D-463) |
| Crowding flag at 5 businesses and 4 quotes in a month | OPS-027 (D-464) |
| Price lock 6 months; refunds at Ask Hava's choice; save-card box at the free win | FEE-040, TERMS-026, FEE-036, business terms (D-465) |
| Share-my-words box, real numbers on the home page, contact card and "Know someone" line | JOB-017, WEB-028, PICK-054 (D-466) |
| AI receptionist "Pro" cut; Ruby declined; fixed costs capped near $150 a month; a "changing the live service" page in the handover; company setup starts when the test hits its quote line | D-467, `BUILD_BRIEF.md` section 15 |

**Not changed now, and why (D-467):** night job-card emails wait for the test's email-speed numbers; AI-drafted help replies wait for the AI switch; holding uncovered requests would add a job status, so uncovered demand feeds recruiting posts instead; the long fee summary at sign-up stays because the legal check holds its lines.

## 2a. What changed in fix round 14 (October 28, packaged jobs)

One pull request. Decisions D-429 to D-449. Plan: `docs/handoff/rework-plan.md`. Your answers: photos now, AI later; she OKs sharing at the pick; the ADOR ruling on the later list; the fire-truck list only; then "Do everything that you recommend." No simulations were run.

| Change | Where |
|---|---|
| AI may word questions and write the job card she approves, inside fixed limits (no diagnosing, pricing or safety calls) | AI-006, AI-008, AI-010, AI-019, TXT-004, CARD-007 (D-430) |
| Pro-level questions per job type from the Grok knowledge base, up to 8; the base kept in `docs/catalog/trade_kb/` | INT-039, LIC-040, OPS-040 (D-431) |
| Photos from day one, checked on our own server; AI reads them only after zero retention | INT-044, CARD-013, CARD-014, AI-016, PRIV-013, PRIV-015 (D-432) |
| Design jobs: inspiration pictures and a labeled concept image (switch off until a provider is set up) | INT-045, INT-046 (D-433) |
| She approves the real card; one business job page | INT-041, CARD-023, JOB-010 (D-434) |
| Coverage told at once; not-covered requests email the owner inbox | INT-017, TXT-032, OPS-041 (D-435) |
| One 911 text for the fire-truck list; no heat, water, health or utility lines; safety-numbers file gone | INT-002, INT-004, INT-031, INT-034 to INT-037, INT-042, WEB-022 (D-436) |
| Own-or-rent and the landlord text removed | INT-028, TXT-028, CARD-011 (D-437) |
| Short texts linking to her page; license numbers and Google and Yelp links on quotes; "Free for you" said plainly | TXT-016, TXT-040, PICK-041, CON-003 (D-438) |
| Sign-up collects Google and Yelp links and license numbers | BIZ-052 (D-439) |
| No scheduling: no call window, no lockbox question | PICK-018, PICK-025, PICK-031, PICK-045, TXT-030 (D-440) |
| One quote format per job type; site-visit fees only on repairs that need a look | PICK-002, PICK-053, FEE-004, FEE-043, PICK-038 (D-441) |
| Screened notes; edit or withdraw; "Can't take it" | PICK-050 to PICK-052 (D-442, D-446) |
| "Share my first name and address" box at the pick; "introduction fee" wording, never "lead" | PICK-049, PICK-037, FEE-031, WORD-026 (D-443) |
| Sales tax: set-aside 8.6%; ADOR ruling on the company-setup list | FEE-046 (D-444) |
| AI wording in the privacy policy and the Twilio question | AI-018, TERMS-008 (D-445) |
| Walkthrough gaps G5 to G21 | LIC-048, TXT-018, PICK-016, OPS-028, OPS-033, PICK-047, BIZ-051, TXT-041, J1, J2 (D-446) |
| Voice: a sharp, experienced receptionist | WORD-024, `docs/voice.md` (D-447) |
| Tasks T162 to T165 added; T152 and T159 repurposed | `kit/tasks.json` (D-448) |

## 2b. What changed in fix round 13 (history: rows A4 summary step, A17's 7.6% and A19's safety numbers were later superseded by D-434, D-444 and D-436)

One pull request on GitHub (see the link in chat). Decisions D-391 to D-416. No simulations were run.

**From the deep dive's 19 fixes (all applied)**

| # | Fix | Where |
|---|---|---|
| A1 | Twilio question: "we never call businesses" (the stale "automated phone call" line removed) | for-casey-review |
| A2 | License map: dual (CR-), general (B, B-3, KB-1, KB-2), R-61, R-62, pool (B-5, B-6, KA-5, KA-6), subclassifications, and other agencies; unclear rows never inherited | `docs/catalog/license_map.yaml`, LIC-043, LIC-035 |
| A3 | Business setup: checklist pre-ticked from the license, untick, largest job, recurring yes/no, after-hours; checkboxes or one question a screen; re-checked when the license changes | BIZ-049, BIZ-050, INT-032 split |
| A4 | Expert intake: 3 to 6 questions per trade in English and Spanish, incl. boats, RVs, pools and vacation homes; routing with confidence; "Here's what we'll send" summary step; golden test requests; monthly accuracy report | `docs/catalog/question_bank.yaml`, INT-039 to INT-042, OPS-040 |
| A5 | Pilot cut: `launch_open` per trade; a trade opens at 2+ businesses; pilot scorecard page | LIC-046, OPS-039 |
| A6 | Honest count: N = emails the provider accepted; bounces drop out; "Not every business answers." | TXT-032, CARD-019 |
| A7 | Short consent texts: English 558 characters, Spanish 532, both 4 segments (replaced Oct 28: no reply OK, 2 segments each, D-419) | CON-031, SMS terms |
| A8 | Quote types: fixed, range, site visit needed, recurring; hourly needs estimated hours; setup charges never in the fee; site-visit fee $25, never above $75 | FEE-004, FEE-043, FEE-044, W34 to W40 |
| A9 | Homeowner backup choice at the pick | PICK-047 |
| A10 | Pest control on for Department licensees only; sale and refinance inspections fee-free | LIC-012, LIC-015, FEE-045 |
| A11 | Desk research: no ROC class names garage doors; junk-removal city code not readable online. Both stay off | D-404 |
| A12 | Pitch file fixed ("Text Ask Hava", no bidding wars, no phone help line, email-only picks, pest answer) | `outputs/gtm/` (outside the repo) |
| A13 | Voice guide as a wording rule, with 10 rewritten texts | `docs/voice.md`, WORD-024 |
| A14 | Homeowners see "request <n>", businesses "job <n>" | WORD-025 |
| A15 | Stripe question rewritten, with an auto-confirm question added | for-casey-review |
| A16 | "Who runs Ask Hava": LLC record link and city license number once they exist | WEB-021 |
| A17 | Money view: 7.6% sales-tax set-aside line and manual expenses | FEE-046 |
| A18 | Auto-confirm and pay if picked (only after Stripe's written yes); push web app built as an optional channel | PICK-048, BIZ-041 |
| A19 | Safety numbers verified by the build itself, re-checked every 90 days; not your task | INT-036, task T159 |

**Other open fixes found and applied (6):** masked-call caller ID steps against "Spam Likely" (CALL-024); R-37R and R-39R read by the subclassification on the state record (LIC-035); commercial-only licenses told why they get no jobs (LIC-044); R-61 and R-62 project limits (LIC-045); a swamp-cooler job type; the legal drafts updated to match (business terms, privacy policy, SMS terms, security program).

**Changed Oct 28 after the master plan (D-419 to D-428)**

Casey: "their reaching out is their consent".

| Change | Where |
|---|---|
| No "Reply OK" step: a homeowner's first text is the opt-in (for web chat, a mobile number plus its code, CON-005 and CON-025 as amended by D-470); the first reply answers, moves on and carries the disclosures in 2 segments ("Ask Hava: Got it. Next, a few quick questions. Ask Hava is not a contractor; you hire the business you pick directly. Automated texts about your request. Msg frequency varies. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms: askhava.com/terms Privacy: askhava.com/privacy"); the emergency check still runs first; the card goes out on its own after 30 minutes (INT-041, D-460) | CON-002 to CON-006, CON-025 to CON-027, CON-031, INT-019, INT-025, INT-026, INT-041, TXT-034, WORD-002 (D-419) |
| Terms accepted by picking: only "By picking, you agree to the Terms" (linked) beside the Pick button, logged with time, terms version and request number; the $100 cap stays in the Terms under "Limit of liability", not on the page | PICK-016, TERMS-001, TERMS-004, legal drafts (D-420, D-421) |
| Updates go by text by default, every update linking to the quote page; the "text, email or both" question was removed by D-460 | TXT-018 (D-422, D-460) |
| Stopping texts unchanged; the first reply says STOP | D-423 |
| Twilio question shortened to customer-care texts and two questions (item 6) | for-casey-review, `send_drafts/Twilio_question.md` (D-424) |
| B11 trademark skipped for now, B13 insurance later, B17 Spanish English-first with safety texts on | Section 4; TXT-039, TXT-002, CON-026, TERMS-018, BIZ-018 (D-425 to D-427) |

**Skipped, and why**
- Shortening the HELP reply (voice rewrite 6): carriers need its contact details, and the legal check uses its reply-time sentence.
- Cutting build tasks for the pilot (team members, monthly fee limit, live license lookup, full restore replay): held for the re-plan pass after the hand-run test (D-455).
- The persona file's facts (quiet hours, "Text Hava", refunds, calls): stale; only its speaking style was used.
- Status-report fixes 1 to 13: already merged in fix round 12 and the legal drafts.

---

## 3. Before-build checklist

Owner: **Claude**, **Casey**, or **automatic** (the build does it). Status: done, to do, or waiting (on an outside answer). Do them in this order; items 2 to 7 can run during the hand-run test.

| # | Item | Owner | Status | Done means |
|---|---|---|---|---|
| 1 | Fix round 15 merged | Claude | done when the PR merges | PR merged with Ci, Twin and Ally green |
| 2 | Pause the build runner while the build is held (section 6) | Casey | to do | "Ask Hava build runner" shows Paused; the old "(hourly)" one is deleted |
| 2a | Audit decisions E-1 to E-5 (`docs/for-casey-review.md`, top) | Casey | to do | Each answered in a GitHub issue or chat; decisions logged |
| 2b | Ruleset taps (E-2): `Governance` required, Actions source, up-to-date branches, `claude/kit-state` ruleset | Casey | to do | Ruleset JSON shows them |
| 2c | T174 (runner live entry point, red-`main` drill, stage exits) and P-1 (prompt text and budgets, task splits, validator checks) | Claude | to do | Both merged green; `docs/build-readiness.md` section 4 items 6 and 7 done |
| 2d | Re-issue `docs/build-readiness.md` as READY | Claude | waiting on 2a to 2c and 14 | Every Critical and High risk in `docs/risk-register.md` closed or accepted |
| 3 | Open a free Twilio account; send the short Twilio question (`docs/for-casey-review.md` top, D-424) | Casey | to do | Case number saved |
| 4 | Twilio's written answer | outside | waiting on item 3 | Answer saved; doesn't block the build |
| 5 | Open a Stripe account (test mode only); send the Stripe question | Casey | to do | Ticket sent |
| 6 | Stripe's written answer | outside | waiting on item 5 | Answer saved; auto-confirm stays hidden until it says yes |
| 7 | Save dated screenshots of today's askhava.com | Casey | to do | Screenshots in your files |
| 8 | Hand-run test setup: the rules tab, test inbox, request form, sign-up form, sheet, tracking links (`docs/pilot/hand-run-test.md` sections 3 and 4) | Casey | to do | The rules tab is filled in; both forms submit into the sheet; one test request and one test sign-up arrive |
| 8b | Dry run with fake businesses and homeowners: six scenarios, Stripe test mode, then reset (`docs/pilot/dry-run.md`, D-469) | Casey | to do | All six scenarios pass their checks; the tracker is cleared and set for week 1 |
| 8a | Start the LLC and EIN, then a live Stripe account with $25, $75 and $199 payment links, an 8.6% set-aside, the Lake Havasu City business license (and home-occupation form), a postal address for email footers, the published pilot privacy notice and the paid-pilot terms form (playbook sections 4 and 9, D-470) | Casey | to do | All in place by the end of week 3; every paid-week business has accepted the terms |
| 9 | Find the Havasu Facebook groups (about an hour, logged in on your phone): community, recommendations, buy and sell, trades and business-owner groups; note member counts and posting rules; message each admin | Casey | to do | A list of 8 or more groups, with each admin's answer |
| 10 | Line up practice requests from people you know (counted as Seeded, not as demand) and 3 to 5 property managers, rental hosts or realtors (Partner) | Casey | to do | Names on your list (not in the repo) |
| 11 | Run the hand-run test for 6 weeks: weeks 1 to 3 helped and free, weeks 4 to 6 hands off and paid in the pushed trades; the weekly scorecard every Sunday | Casey | to do | Six weekly rows in the sheet, with nudges logged |
| 12 | The first 5 to 10 business conversations, with the C1 to C13 questions (playbook section 9) | Casey | to do | Answers noted |
| 13 | Test decision: go, change the plan first, or stop (playbook section 8) | Casey | waiting on item 11 | You tell Claude which, with the scorecard |
| 14 | Re-plan pass: apply what the test showed, the held scope cut, and the fees from the paid weeks | Claude | waiting on item 13 | One PR merged green |
| 15 | Paper rehearsal of the build runner (section 6) | Claude | done Oct 28 (D-472) | Reached "Finished" with only the expected stops; a failing task blocked only its dependents; three ordering fixes applied |
| 16 | Optional: add the routine's merge trigger (section 6) | Casey | to do | The routine shows "Pull request closed, merged into main" |
| 17 | Gate P: reply `approved to build` on GitHub issue #24, only if the scorecard says go including real payments and `docs/build-readiness.md` says READY; then resume the runner | Casey | waiting on items 2d, 13 and 14 | Reply posted |
| 18 | Build Phase 1 (stages 1 to 5) | automatic | waiting on item 17 | Account checkpoint issue appears |

**Company setup now starts with the test (D-468):** the LLC, EIN and a live Stripe account are needed for the paid weeks (item 8a). The rest of section 8 (city license, DNS, Twilio registration) follows before Gate C. Before Spanish is switched on, the two-AI back-translation check of the Spanish safety texts (D-427), and a paid review (about $100).

**Done or dropped:** fix round 13 and 14 merged; your section 4 defaults answered ("do what you recommend", D-455 to D-467); Department of Agriculture letter dropped (D-417); safety-number websites dropped (D-436).

---

## 4. Your open decisions

**Defaults already in the plan (pending your OK).** Each is one setting; changing one is a logged settings change.

| # | Default in the plan | Recommended |
|---|---|---|
| B2 | Pilot trades: every enabled service category; marketing pushes one or two seasonal trades | Decided by Casey Oct 28: "We need to run all service buisnesses" (D-459; replaces D-418's six) |
| B3 | Pest control on for businesses that attest to a Department license; cards say it isn't independently verified; no letter | Decided by Casey Oct 28 (D-417) |
| B4 | Home inspections on; no fee for sale, purchase or refinance ones | Confirmed by Casey Oct 28 (D-418) |
| B5 | AI may read scrubbed text before a zero-retention agreement, once the provider's terms are recorded and the privacy policy says so; launch works without AI | Confirmed by Casey Oct 28 (D-418) |
| B6 | Backup choice on; auto-confirm only after Stripe says yes; push web app built as an optional channel | Confirmed by Casey Oct 28 (D-418) |
| B7 | Site-visit quotes pay $25, never more than $75 | Confirmed by Casey Oct 28 (D-418) |
| B8 | Fees stay $25 / $75 / $199 for the test; the re-plan pass sets them from the "worth the fee" answers; early joiners locked 6 months | Recommended and applied (D-455, D-465) |
| B9 | No arbitration for homeowners in the pilot | Confirmed by Casey Oct 28 (D-418) |
| B12 | No owner name online; LLC record link and city license number added once they exist | Confirmed by Casey Oct 28 (D-418) |
| B14 | Losing businesses told whether the picked quote was lower, about the same or higher | Confirmed by Casey Oct 28 (D-418) |
| B15 | Safety voice call off | Confirmed by Casey Oct 28 (D-418) |
| B11 | Trademark: skip for now; revisit if Ask Hava grows beyond Havasu or another "Ask Hava" appears | Decided by Casey Oct 28: "We can skip trademark for now" (D-425) |
| B13 | Insurance: later, after the business is fully working | Decided by Casey Oct 28: "We'll work all that out after we have a fully working business" (D-426) |
| B17 | Spanish: launch English-only for conversations; Spanish built and tested but switched off; Spanish safety texts on, checked before launch by back-translation through ChatGPT and Grok with matching meaning; full Spanish after a paid professional review (about $100) | Decided by Casey Oct 28: "your recommendation is going to have to be best" (D-427) |
| B16 | Short first reply | Replaced Oct 28: no reply OK; a 2-segment first reply (D-419) |

**Still open (1): B1. B10 is now decided, shown here for reference.**

| # | Decision | Recommended answer |
|---|---|---|
| B10 | Sales tax path | Decided Oct 28: set aside 8.6%; ask ADOR for a private ruling after the LLC exists, before real revenue (D-444) |
| B1 | Gate P | Approve only after the hand-run test says go and the re-plan pass is merged (section 3, items 13 to 17). |

**Applied from the review (D-455 to D-467), each your recommended answer:**

| # | Question | Answer applied |
|---|---|---|
| R1 | Hand-run test before `approved to build`? | Yes, 6 weeks, the last 3 paid in the pushed trades (D-455, D-468) |
| R2 | Post from your profile or the Ask Hava Page? | Your own profile, saying "I run Ask Hava", and the Page where a group allows Pages (D-456) |
| R3 | Which trades get the marketing push? | One or two that fit the launch month (playbook section 6); all trades stay open (D-459) |
| R4 | Visits only, or file the seller registration to allow calls? | Visits only for now; calls need A.R.S. 44-1272 and a new decision (D-374, D-456) |
| R5 | Price lock for early joiners | 6 months (D-465) |
| R6 | Is $50k for Havasu alone? | No: $20k a year is the Lake Havasu City target; $50k assumes Bullhead City and Kingman (D-467) |

**Scope cut:** held for the re-plan pass after the test (D-455): postpone team members, the monthly fee limit, the live license lookup, the full restore replay and the features that launch switched off.

---

## 5. Accounts to set up, in order

Rule: test or sandbox first. Never paste a key into chat, an issue, a file or an email. The build asks for keys only at the account checkpoint, in a GitHub issue titled "Your turn: set up accounts", with exact clicks.

**Where keys go** (`BUILD_BRIEF.md` section 2):
- **Build sessions:** secrets in the claude.ai cloud environment `ask-hava-build`, each locked to its vendor's host.
- **The running test app:** variables in the Railway **test** project.
- **GitHub Actions:** one secret only, `CLAUDE_CODE_OAUTH_TOKEN`, for the optional backup runner.
- **Live keys:** only you, only at Gate C, only in the Railway **live** project.

| When | Account | Key goes to | Why then |
|---|---|---|---|
| Now | Twilio (free) | No key yet | To send the eligibility question |
| Now | Stripe (test mode, don't activate) | No key yet | To send the support question |
| Now (optional) | Backup runner token (`claude setup-token`, at a computer) | GitHub Actions secret `CLAUDE_CODE_OAUTH_TOKEN` | Only if the scheduled runner fails twice |
| Account checkpoint (end of Phase 1) | Railway test project | Railway test variables (app keys) | Test site |
| Account checkpoint | Stripe test keys | Environment secret locked to api.stripe.com; Railway test variable | Stripe sandbox (T132) |
| Account checkpoint | Twilio test credentials ("Username and password" secret type) | Environment secret locked to api.twilio.com; Railway test variable | Twilio sandbox (T129, T131) |
| Account checkpoint | Resend | Environment secret locked to api.resend.com; Railway test variable | Email sandbox (T133) |
| Account checkpoint | Lob (test key) | Environment secret locked to api.lob.com; Railway test variable | Identity letters (T133) |
| Account checkpoint | Sentry | Railway test variable (DSN) | Error monitoring |
| Account checkpoint | UptimeRobot | No key; you add the test site's /healthz | Uptime alerts |
| Account checkpoint | Claude Console workspace (separate from your Claude plan) | Environment secret locked to api.anthropic.com; Railway test variable | Real-AI test on fake data (T137); its API terms are recorded for B5 |
| Account checkpoint (optional) | Concept-image provider (chosen then) | Environment secret locked to its host; Railway test variable | Concept images for design jobs (INT-046); its terms recorded; switch stays off without it |
| Account checkpoint | Storage bucket (chosen then, D-094) | Railway test variable | Audit stream and backups |
| At the start of the hand-run test (D-468) | LLC, EIN, live Stripe for the paid weeks; then city license, TPT path, Google Workspace, DNS (SPF, DKIM, DMARC); insurance later (D-426) | Not keys | Section 8 |
| Gate C | Twilio brand and campaign registration, Stripe activation, live keys | Railway live project only, by you | Going live |

**Two encryption checks at the account checkpoint (D-454):** the host you pick encrypts the database, its volumes and its backups at rest (Render documents this; confirm it for Railway in writing), and the Claude Console workspace has zero data retention in force before the AI switch goes on. Decide there too whether to add an outside key service (AWS or Google Cloud KMS); it is optional, about $1 a month for the key, and cleanest on Render Pro.

---

## 6. Build automation

**How it runs.** One scheduled task wakes a fresh Claude session every hour. Each session does one cycle: read state and your GitHub replies, finish the last task if its pull request merged green, then claim one task, build it with a worker on the task's model, review it, and push. GitHub checks (Ci, Twin, Ally) and auto-merge do the rest. State lives on the `claude/kit-state` branch; progress is in `STATUS.md` there.

**Where it stops for you (and only there)**

| Stop | What you do |
|---|---|
| Gate P (now) | Reply `approved to build` on issue #24 |
| A GitHub setting the build can't change | Follow the taps in the issue |
| Account checkpoint (end of Phase 1) | Create the test accounts, add keys where the issue says, reply `accounts done` |
| Stage 0.5 (the askhava.com page) | Reply `go`, whenever you want the page |
| Real texts to your phone (only if Twilio test credentials don't work) | Reply `yes` or `no` |
| "Finished: ready for your review" | Read the handover pack |
| Gate B | Your own click-through and screen-reader check |
| Gate C and the first live charge | Live keys and "go live", then the first-charge go |

**Risks found, and what handles them**

| Risk | Handling |
|---|---|
| Two runners firing at once | "Ask Hava build runner (hourly)" (7 past) is already paused (since 17:55 UTC today). Delete it. Keep "Ask Hava build runner" (50 past, fresh Sonnet sessions). |
| Usage spent while held | The kept runner fires 24 times a day and exits in about 25 seconds at Gate P. Small, but nothing to gain. **Pause it now; resume it right after you reply on issue #24** (or tap Run now). |
| Slow chain (one task an hour) | 154 tasks to build means about 6 to 7 days at best. Adding the merge trigger in the routine's settings lets the next task start right after a merge. |
| Runner loses races to the notify and inbox workflows writing to `claude/kit-state` | A lost race exits and retries next cycle; noted for K8; costs a cycle, not work |
| A task fails | One retry, then Opus; after 3 tries it's BLOCKED, only its dependents wait, and it goes on the end-of-build list |
| Usage limit | The task waits for the reset; never pay per use |
| Phase 2 sandbox calls blocked by the deny rules (noted at K0) | The sandbox tasks use the vendor SDKs; if one blocks, it's listed at the end, not asked mid-build |
| Safety-number verification can't reach city or utility sites | Unverified numbers are left out of texts; never a stop (the safety-number item was dropped, D-436) |
| Stale lines in `BUILD_BRIEF.md` (Part 2 lists "Arizona TPT license" and "zero retention"; Appendix A says 7 past, the live routine runs at 50 past) | Logged; the decisions (D-412, D-414) win; fix in a docs task, no stop |
| Older source lines that round 14 reversed (changes file :58, :86, :89, :196, :247, :248; `BUILD_BRIEF.md` :182, :340: photos off cards, heat and water safety lines, tenant line, TPT license, insurance first) | `docs/constitution.md` updated to match; the protected files stay as they are and are superseded by D-417, D-426, D-430, D-432, D-436, D-437, D-444 (D-450) |

**Paper rehearsal (run Oct 28 under your "do what you recommend", D-472: passed after three fixes)**
- **What:** a paper rehearsal. Claude runs the build's own runner code (`runner/cycle.py`) against the real task list with fake GitHub and fake workers, every task passing, then once with one task failing on purpose.
- **What it proves:** every gate opens in the right order (Gate P, account checkpoint, Stage 0.5, real texts, Finished, Gate B, Gate C, first charge); no task waits on something that never finishes; a blocked task holds only its dependents; the number of cycles to the end.
- **How long and how much usage:** one session, about 30 to 45 minutes, no worker sessions, a small share of one usage window.
- **Go means:** it reaches "Finished" with only the stops in the table above, no deadlock, and the failing task blocks only its dependents. Anything else becomes a fix before Gate P.
- **Then:** the first two real tasks after Gate P (T002 and T003) act as the live check: both merge green, and the next cycle starts on its own. They're real work, so nothing is wasted.

---

## 7. Pilot plan and stop/go scorecard

**Shape:** after the hand-run test passes. Every enabled category open, marketing pushing one or two seasonal trades; 15 to 25 businesses; 8 to 12 weeks; at least 60 started requests. A trade opens to homeowners only at 2 or more active businesses. Recruiting mostly self-serve from disclosed Facebook group posts, plus a few visits (BIZ-048). The owner portal's scorecard page shows these each week (OPS-039).

**The money ladder (D-458):** about $40 is left from a $50 average fee after the sales-tax set-aside, Stripe and refunds. 15 paid introductions a month is the pilot; about 50 a month is the $20,000 a year pace (Lake Havasu City); about 115 a month is the $50,000 pace, which needs Bullhead City and Kingman.

| Measure | Go | Stop or rethink |
|---|---|---|
| Requests with 1 or more quotes within 24 hours | 70% or more | under 50% after 4 weeks |
| Routable requests with 2 or more quotes within 24 hours | 60% or more | under 30% |
| 75th-percentile time to first quote | 12 hours or less | over 24 hours |
| Median time to first quote | 4 hours or less | over 12 hours |
| Picks per request with quotes | 40% or more | under 25% |
| Picks confirmed (not lapsed) | 85% or more | under 75% |
| Average fee per paid introduction | $50 or more | under $35 |
| Free share of introductions (leaving out each business's first win) | 20% or less | over 40% |
| Requests started per week (by week 8) | 15 or more | under 6 |
| Paid introductions this month (month 3) | 15 or more | under 5 |
| Net per paid introduction | $40 or more | under $28 |
| Started intakes that reach the card step | 80% or more | under 60% |
| Business sign-ups active within 15 minutes | 70% or more | under 40% |
| Refunds / bank disputes | 10% / 1% or less | over 20% / 2% |
| Businesses paying again within 60 days | 50% or more | under 30% |
| Your minutes per paid introduction (recruiting counted separately) | 20 or less | over 45 |
| Homeowner complaints | 1 per 20 jobs or fewer | more than 1 per 10 |

The scorecard decides nothing on its own. You decide stop or go at week 8 from real numbers.

---

## 8. Later

**At the start of the hand-run test (company setup, D-468; the LLC, EIN and Stripe first, for the paid weeks):** confirm with the Arizona Corporation Commission that an agent or mailbox address can replace a home address; form the LLC with a Maricopa or Pima County statutory agent; free EIN; city business license and home-occupation form; ADOR private taxpayer ruling request (retail, rental, prime contracting, marketplace, city licensing-for-use and city advertising; D-444) and an 8.6% set-aside account; SPF, DKIM and DMARC on askhava.com; Twilio registration (after the answer and the LLC); Stripe activation; Gate C; Resend paid plan at the first 70% alert.

**Running-cost rule:** fixed monthly costs stay near $150 or less, not counting the Claude plan; the Ruby answering service is declined (D-467).

**After the pilot:** fee review with the real mix; business texting (if Twilio says yes); AI photo reading (after zero retention); support bot (past about 15 help requests a week); owner finance assistant; "call and leave a message", then AI voice intake; new trades (pool construction, RV and mobile repair, garage doors once a class is confirmed, junk removal if the city allows); optional prepaid credit pack, spent only on introduction fees, with no auto-renew and no placement advantage (the AI receptionist plan is cut, D-467); arbitration review and Spanish legal documents; insurance once the business is fully working (D-426); trademark if Ask Hava grows beyond Havasu or another "Ask Hava" appears (D-425); the paid Spanish review, then Spanish conversations on (D-427).

---

## 9. How we avoid going in circles

1. **This file is the only to-do list.** Older lists (status report section 7, the deep dive's section 7, earlier review notes) are history.
2. **Every change updates this file** in the same pull request, with the decision number.
3. **A closed item stays closed** unless new evidence is named here: a written answer, a real number from the pilot, or a law or vendor rule that changed.
4. **No new research round, simulation or rehearsal starts without your OK**, and each one says up front what answer would change a decision.
5. **When unsure, the build decides and logs** (BUILD_BRIEF section 16); it never waits except at the stops in section 6.
