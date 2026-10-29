# Brain: what home-service businesses go through with paid job sources

- **Purpose:** what plumbers, pool techs, handymen and other home-service owners actually experience with Angi/HomeAdvisor, Thumbtack, Google Local Services Ads and similar sites, what they value and what makes them quit, and what that means for Ask Hava's business side.
- **Who reads it:** any session building or reviewing `businesses` (sign-up, portal, "Your numbers"), the quote form (PICK-040), the loss notice (PICK-026), fee and receipt copy (FEE-*, BIZ-030), business-page copy (WEB-018 to WEB-025), the recruiting posts (BIZ-048), or the hand-run test (`docs/pilot/hand-run-test.md`).
- **How to use it:** read sections 1 to 4 for the evidence, section 6 for the design implications, then run the checklist in section 8 on any task that touches the business side. Cite the tag (`[C1]`, list in section 9) when a choice rests on it.
- **Last verified:** 2026-10-29.
- **It never overrides `docs/spec/` or `docs/architecture.md`.** If this file and the spec disagree, build the spec and report the disagreement in your task report (and `docs/decisions.md` if your task owns that file). Tags: `[inferred]` = reasoning, not backed by a source; `[anecdotal]` = forum or community posts, real but unrepresentative.

**Wording note.** The industry calls a request a "lead". Ask Hava never does in customer copy (WORD-020, WORD-026). This file uses "lead" only when describing other companies.

## 1. The FTC's case against HomeAdvisor (the primary record)

The Federal Trade Commission is the US consumer-protection and competition regulator; its complaint, opinion and order are the best primary evidence of how a lead seller treated small businesses.

- **Filed:** an administrative complaint on 11 March 2022 (Docket 9407) [C2][C5]. Conduct alleged "since at least mid-2014" [C1].
- **Costs alleged:** providers generally paid "an annual membership fee of $287.99" plus a fee per lead [C2 ¶5]. An optional one-month mHelpDesk software subscription added $59.99 and "automatically renewed at $59.99 per month unless service providers cancelled it" [C2 ¶7, ¶18]; sales agents allegedly called it free [C1]; the $347.98 charge often showed as one line, so providers missed the add-on [C2 ¶59].
- **Mismatched leads:** HomeAdvisor said providers "only will receive leads matching the types of services they provide and their preferred geographic area," but "many of them do not" [C1]; many leads "do not concern the type of work that the service providers perform" [C2 ¶39].
- **Not ready to hire:** leads "do not concern people who intend to hire a service provider soon," including people who chose "Planning & Budgeting" or "Timing is flexible" [C2 ¶31-32].
- **People who never asked:** HomeAdvisor resold leads bought from affiliates, whose forms often didn't say the data would go to HomeAdvisor, so many leads concerned "individuals who did not knowingly seek HomeAdvisor's assistance" [C2 ¶14, ¶45].
- **Win rates:** "HomeAdvisor often tells service providers that its leads result in jobs at rates much higher than it can substantiate" [C1]. In the Commission's September 2022 opinion, Complaint Counsel's figure from HomeAdvisor's own calculations was roughly 15 to 25 percent, and the average lead price about $30 (July 2014 to September 2019); both figures are marked as confidential-but-shown in the public text and were disputed [C3].
- **Refunds:** "HomeAdvisor does not generally provide refunds for leads"; credits are the remedy, and it denied many credit requests (number redacted) [C2 ¶21-22]. Providers spent time chasing credits, often without success [C2 ¶8].
- **Outcome:** a consent order announced 23 January 2023 (4-0 vote): up to $7.2 million in redress, and a ban on misleading claims about leads, including that they concern people "who are ready to hire a service provider", on calling things free that aren't, and on unsubstantiated conversion-rate claims [C1]. The FTC sent over $3 million in refunds in November 2023 [C4].
- **Private suits:** eight contractors sued in 2018 calling the leads "overwhelmingly bogus" [C6]; in January 2024 a federal court certified a nationwide class on a narrower claim (misuse of the plaintiffs' personal information) and refused one on deceptive practices [C7].

**What the parent company itself says.** Angi's 2024 annual report states that consumer-connection fees are paid "regardless of whether the professional ultimately provides the requested service," that professionals who don't get "an attractive return" may leave, and that 2025 offerings include "double opt-in leads" [C8]. Angi reported about 168,000 transacting professionals in Q4 2024 [C8].

## 2. How the main sources charge, and what businesses say

Primary pricing pages were found only for Google LSA and Angi Leads' help center; Thumbtack, Yelp, Porch and Networx don't publish per-lead prices that this session could reach. Figures marked "reported" come from software vendors' guides or agencies and are estimates.

| Source | How it charges | What a business can do about a bad one | Evidence |
|---|---|---|---|
| **Angi Leads (formerly HomeAdvisor Pro)** | "You'll be charged for each new lead as soon as it's sent"; "Leads are not guaranteed jobs" [H1]. Reported $15 to $85 per lead, about $300 a year membership; Angi "does not publish official pricing" [A1]. HomeAdvisor says it sends a lead to "no more than three additional service professionals" [A3]. | A credit (not a refund) for duplicates, wrong ZIP, wrong task, competitor tests or bad phone numbers, only for leads 24 hours to 30 days old, and only if you used call tracking, called within 24 hours and tried on several days [H2]. | primary [H1][H2][C8]; reported [A1][A3] |
| **Thumbtack** | Charged "when you access or are matched with a lead, not when you win the job"; weekly budget; "Opportunities" charge only if the customer replies; reported $10 to over $100 per lead [T1]. | "If a lead doesn't turn into a job, you don't get your money back" [T1]. | secondary [T1]; anecdotal [T2] |
| **Google Local Services Ads** | Charged for answered calls, missed business-hours calls over 20 seconds, messages, voicemails and bookings; repeat contacts within 15 days are free; a weekly budget sets a monthly maximum [G1]. Reported average cost per lead from one agency's 100+ clients (Aug 2025): roofer $162, HVAC $80, fencing $71, plumbing $69, painter $40, landscaper $39, handyperson drywall $34, locksmith $34 [G3]. | Google reassesses leads for 30 days and auto-credits poor ones; feedback "may occasionally credit" [G1]. Since July 2024 manual disputes were replaced by automated review, and wrong-service or wrong-area calls are no longer credited, per one agency [G2]. | primary [G1]; agency data [G2][G3] |
| **Yelp Ads, Porch, Networx** | No primary price page reached in this session. | Unknown here. | gap |

**What owners say** [anecdotal]:
- A Thumbtack pro paid $83.25 for a furniture-assembly lead shared with three pros, bid $200 and lost: "paying 50% of revenue to Thumbtack doesn't work" [T2].
- On Angi: "a lot of lookie-loos" (a pest-control owner); leads sold to three to five businesses at once, turning into a race to call first and pushing prices down; annual contracts with early-termination fees around 35% of the remaining value [A2].
- An HVAC company was billed by LSA for calls from its own customers searching its name for the phone number [G4].
- A contractor spending about $20,000 a year on HomeAdvisor credited his results to answering immediately [A3].

**Speed.** A 2011 Harvard Business Review study of online sales leads found "most companies are not responding nearly fast enough" [S1]. Shared leads turn that into a race: the business that calls first usually gets the conversation [A2][A3; anecdotal]. Ask Hava removes the race: the homeowner sees every quote and picks (WORD-005), so a slower but better quote can still win [inferred].

**Where jobs really come from.** A 2009 Pro Remodeler survey found repeats and referrals made up nearly 80% of remodelers' leads [S2]. It is old and covers remodelers only, but it matches what the copy panel already assumes: a paid source is a top-up, and businesses compare it to free word of mouth [inferred].

## 3. What businesses value

Each line pairs the evidence with Ask Hava's answer.

| They value | Evidence | Ask Hava today |
|---|---|---|
| Pay only when there's real intent | Angi charges "regardless of whether the professional ultimately provides the requested service" and is moving to "double opt-in" [C8]; Thumbtack's "Opportunities" charge only on a reply [T1] | Pay only on Confirm of a pick the homeowner made (BIZ-030, FEE-036); refund if unreachable (PICK-021) |
| Jobs that match trade, area and size | FTC mismatch allegations [C1][C2 ¶39] | License map and checklist routing (CARD-001, LIC-043), towns (BIZ-010), largest-job answer (BIZ-049) |
| Honest timing | "Planning & Budgeting" leads sold as ready to hire [C2 ¶31-32] | The card carries the homeowner's timing answer, including "Just getting prices" (CARD-007, INT-030) |
| Real people who asked | Affiliate leads from people who didn't know [C2 ¶14, ¶45] | Every request starts with the homeowner texting or chatting with Ask Hava (INT-001); no bought requests (see P-BIZ-1) |
| Knowing the competition | Shared with three to five [A2][A3] | "Sent to <N> businesses." on every card (CARD-019); no cap (CARD-003); crowding flag (OPS-027) |
| Full detail before spending anything | Pay-on-match models charge before the pro sees enough to judge [H1][T1] | The whole anonymous card and screened photos before quoting; quoting is free |
| Easy, real refunds | Credits not refunds, strict conditions, denials [C2 ¶21-22][H2] | Automatic refund under a stated rule, "without you asking" (BIZ-030, TERMS-026) |
| No contracts, no add-ons, no auto-renew | $287.99 membership plus a hidden auto-renewing add-on [C2 ¶5, ¶18, ¶59]; contracts with exit fees [A2] | No card or fee to join (BIZ-001); leave any time (BIZ-051); no Pro plan; any later credit pack never auto-renews (D-467) |
| Control of spend | Weekly budgets on LSA and Thumbtack [G1][T1] | Nothing is charged without the business's own Confirm, except opt-in auto-confirm (PICK-048); the monthly fee limit is held for the re-plan pass |
| Honest claims | FTC order bans unsubstantiated win-rate and "ready to hire" claims [C1] | Counts from the ledger only (CARD-019, WEB-028); hand-run posts never claim how often a card becomes a job |

## 4. How a business judges a new source in week one, and why it quits

Week one, in the order an owner meets it [inferred from sections 1 to 3]:
1. **Did signing up cost anything or ask for a card?** (Burned by memberships and add-ons [C2].)
2. **Did a real job arrive fast, in my trade and town?** (Mismatch is the first complaint [C1].)
3. **Could I quote it from what I saw, quickly?** (A quote that takes 45 minutes and wins 1 in 10 is not worth it; hand-run section 11.)
4. **How many others got it?** (Shared-lead fatigue [A2][T2].)
5. **Did I get picked, and was any charge one I expected?** (Surprise charges and denied credits [C2 ¶21-22, ¶59].)
6. **When I lost, did I learn anything?** [inferred]

Why they quit, from the evidence: paying for contacts who never answer or never meant to hire [C2 ¶31][A1][A2]; jobs outside their trade or area [C1]; refund fights [C2 ¶8, ¶22][H2]; contracts, memberships and auto-renewals [C2 ¶18][A2]; too many competitors and a race to the lowest price [A2][T2]; charges they can't trace to a job [C2 ¶59][G4]; dispute rights taken away [G2]. Ask Hava's BIZ-054 leave-reason list ("Too few jobs", "Jobs weren't a fit", "The fee", "Too many businesses quoting") matches these well.

## 5. Ask Hava's fixed fee next to the market

| | Typical market cost | Ask Hava |
|---|---|---|
| Unit charged | A contact, whether or not you win: about $30 average at HomeAdvisor 2014 to 2019 [C3]; $15 to $85 reported at Angi [A1]; $10 to over $100 reported at Thumbtack [T1]; $34 to $162 by trade on LSA, one agency's data [G3] | A pick the homeowner made after seeing your price: $25 (under $500), $75 ($500 to $1,500), $199 (over $1,500) (FEE-001) |
| Cost per won job | Contact price divided by win rate. At HomeAdvisor's disputed 15 to 25% [C3], a $30 lead works out to about $120 to $200 per won job [inferred arithmetic] | The fee itself, refunded if unreachable; a pick is not a signed job, because the homeowner can still decide not to hire [inferred] |
| First job | Paid | Free first win within the launch window (WEB-005) |
| Joining | Membership or none | Free (BIZ-001) |

Read this carefully:
- The win rates are rough and disputed, LSA figures are one agency's, and Ask Hava's own pick-to-job rate is unknown until the pilot. Don't present the right-hand column as cheaper per job until the ledger shows it [inferred].
- Any cost comparison shown to businesses needs a source cited on the page and must never call a request a "lead" (WORD-012).
- The free first win removes the "is this real?" risk at the moment of first value; Koerner's advice of a low introductory price while value is unknown (D-465) points the same way [inferred link]. Its cost is a free share of picks that must stay small (PREBUILD section 7: 20% or less, leaving out first wins).

## 6. Design implications for Ask Hava

**Sign-up friction (BIZ-*).**
- Lead with money in numbers before anything else: BIZ-030's "Free to join. Pay only when you confirm a pick: $25 / $75 / $199 by job size. Refund if you can't reach them." is the right first line, because burned owners look for the catch first (copy panel section 2) [C2].
- No card, no membership, no add-on, no trial that converts (BIZ-001, D-467). Never call anything free that later costs money: that is exactly what the FTC order bans [C1].
- Get to a real job fast: BIZ-031's done screen leading with open jobs (D-462) and BIZ-042's sample card. Each extra step before the first card costs sign-ups [inferred].

**The job card and quote form (CARD-*, PICK-040).**
- Show everything a pro needs to price it once: job type, size band, timing (including "Just getting prices"), screened photos, town, and "Sent to <N> businesses." Honest timing protects Ask Hava from the "not ready to hire" problem [C2 ¶31].
- Keep quoting under a few minutes: PICK-040's "Same as last time" pre-fill matters more than any copy. Measure quote minutes in the hand-run (C10).

**The loss notice (PICK-026).**
- Tell every losing business, once, that the job went elsewhere, how many quoted, and whether the picked quote was lower, about the same or higher. That's the feedback pay-per-lead sites never give [inferred]. Keep it short, no blame (voice guide rule 3). C11 in the hand-run asks whether "the picked quote was lower" makes them quote again or quit: let that answer set the wording.

**Fee messaging (FEE-031, WORD-026).**
- One charge, one job, one line: "Ask Hava introduction service, job <n>" (FEE-031). Never bundle, never add a line the business didn't press Confirm for; the FTC record shows what a hidden bundled line does to trust [C2 ¶59].
- The "Why was I charged?" page answers in this order: you were picked for job <n> on <date>; you pressed Confirm at <time>; your quote was $<x>, so the fee is $<fee> (tier and how the basis was set); the refund rule and whether it applies; how to ask. Call it an "introduction fee for a homeowner who chose you" (BIZ-030), never a lead, purchase or "unlock" (WORD-026).
- Never state or imply a pick-to-job or quote-to-win rate that isn't computed from the ledger with its date range [C1].

**The hand-run test scorecard (`docs/pilot/hand-run-test.md` section 10).**
- Real payment beats stated willingness: section 10's real-payment line is right [inferred; D-468].
- Add per active business: median quote minutes, quotes sent per pick, and whether it stopped quoting (section 11 already asks; make it a scorecard line).
- Count cards a business says weren't its trade, town or size. Mismatch is the first thing burned owners check [C1].
- Count refund requests and their reasons in the paid weeks.

**Answering the burned owner (recruiting posts, business page, help).** Each objection below comes from the evidence in sections 1 to 4; each answer must be true under the spec today, and is worded in Ask Hava's terms, never "lead".

| What a burned owner thinks | The true answer | Backed by |
|---|---|---|
| "Another site that sells my number to five guys." | Every card says how many businesses got it. You pay nothing unless the homeowner picks your quote. | CARD-019, BIZ-030 |
| "I'll pay for people who never answer." | You pay when you confirm a pick. If you call twice in her window and can't reach her, the fee comes back without you asking. | PICK-021, BIZ-030 |
| "They'll send me jobs I don't do." | You get only job types your license covers and you ticked, in the towns you picked. | CARD-001, BIZ-004, BIZ-010 |
| "There's a membership or a contract." | Free to join, no card at sign-up, leave any time. | BIZ-001, BIZ-051 |
| "The first month is 'free' and then it isn't." | The launch offer is one free win; after that every fee is shown on the Confirm page before you press it. | WEB-005, FEE-036 |
| "I'll never know why I lost." | When another quote is picked, you're told how many quoted and whether the picked quote was lower, about the same or higher. | PICK-026 |
| "Is this person even serious?" | Ask Hava doesn't promise that. The card shows her timing answer, including "Just getting prices", so you decide. | CARD-007, P-BIZ-2 |
| "Who runs this?" | Ask Hava LLC, a Lake Havasu City business; its public record and city license are linked. | WEB-021 |

Never answer an objection with a claim about how often cards turn into jobs: the FTC order bars exactly that kind of unsupported claim for HomeAdvisor [C1], and Ask Hava has no such number yet.

**Proposed requirement sentences** (for Casey's decision; not yet in the spec):
- **P-BIZ-1** Ask Hava never buys, imports, rents or resells requests from another site or list; every job card comes from a homeowner who contacted Ask Hava directly by text or web chat.
- **P-BIZ-2** No template, page, post or document for businesses says "ready to hire", "serious homeowners", "qualified", "pre-qualified", "high-intent" or "guaranteed jobs", or states a pick, hire or win rate that is not computed from the ledger and shown with its date range and count, and the WORD-003 scan checks for them.
- **P-BIZ-3** Every job card page offers the business a one-tap "Not a fit for us" with the reasons not my trade, not my town, too big or too small, and other; the answer changes no routing or fee, and the monthly count per trade and reason appears on the pilot scorecard (OPS-039).
- **P-BIZ-4** Every charge to a business is for exactly one confirmed pick and appears as its own line on the receipt and statement; there are no bundled charges, add-ons, memberships or automatic renewals.
- **P-BIZ-5** The hand-run scorecard adds, per pushed trade, the median minutes a business spends on a quote (from the business's own estimate), quotes per pick per business, the count of "not a fit" cards, and refund requests with reasons.
- **P-BIZ-6** After the re-plan pass, a business may set an optional monthly fee limit in the portal; once reached, its quotes are marked to the homeowner as unavailable until the next month or until the owner raises the limit (restores the held "monthly fee limit" as the one spend control).

## 7. Things this file could not settle

- Thumbtack's official lead-price rules and refund policy (only secondary and community sources were reachable).
- Yelp Ads, Porch and Networx pricing.
- A representative survey of home-service owners on lead sources (only the 2009 remodeler survey was found).
- Ask Hava's own pick-to-job rate: the hand-run test and pilot must measure it.

## 8. Checklist for any task touching the business side

- [ ] Does any copy say lead, purchase, unlock, ready to hire, qualified or exclusive? (WORD-020, WORD-026, P-BIZ-2)
- [ ] Is every number shown computed from the ledger, with its date range? (CARD-019, WEB-028)
- [ ] Can a business see the whole card, the count sent and the timing before spending a minute or a dollar?
- [ ] Does any charge happen without the business's own Confirm (other than opted-in PICK-048)?
- [ ] Is every charge one job, one line, with a "Why was I charged?" page (FEE-031)?
- [ ] Is the refund automatic under the stated rule, with no conditions beyond BIZ-030's?
- [ ] Does sign-up ask for a card, a membership or a renewal? It must not.
- [ ] Does the loss notice tell them how many quoted and how their price compared (PICK-026)?
- [ ] Does anything call something free that can later cost money?
- [ ] Is quoting still a few taps for a repeat job type (PICK-040)?
- [ ] Does a business leaving or pausing get asked why, once (BIZ-054)?

## 9. Sources

FTC and court records (primary):
- [C1] FTC press release, "FTC Order Requires HomeAdvisor to Pay $7.2 Million and Stop Deceptively Marketing its Leads for Home Improvement Projects", 23 Jan 2023: https://ftc.gov/news-events/news/press-releases/2023/01/ftc-order-requires-homeadvisor-pay-72-million-stop-deceptively-marketing-its-leads-home-improvement
- [C2] FTC administrative complaint, In the Matter of HomeAdvisor, Inc., Docket 9407 (public redacted): https://Www.ftc.gov/system/files/ftc_gov/pdf/Home%20Advisor%20Part%20III%20Complaint%20Public%20Redacted_0.pdf
- [C3] FTC Commission opinion following order denying summary decision, 9 Sep 2022: https://www.ftc.gov/system/files/ftc_gov/pdf/HomeAdvisor%20Summary%20Decision%20Opinion%20Redacted.pdf
- [C4] FTC refunds announcement, 29 Nov 2023 (reposted by the New Hampshire Banking Department): https://www.banking.nh.gov/news-and-media/ftc-returns-more-3-million-businesses-paid-homeadvisor-memberships
- [C5] Venable LLP summary of the complaint, March 2022 (secondary): https://www.venable.com/insights/blogs/2022/03/burning-down-the-house-ftc-accuses-homeadvisor-inc
- [C6] BusinessDen, contractors' 2018 class action (secondary): https://businessden.com/?p=27495
- [C7] Bloomberg Law, class certification, Jan 2024 (secondary): https://news.bloomberglaw.com/litigation/homeadvisor-will-face-nationwide-class-in-suit-over-sham-leads
- [C8] Angi Inc. Form 10-K for 2024 (SEC filing): https://www.sec.gov/Archives/edgar/data/1705110/000170511025000018/angi-20241231.htm

Company help pages (primary):
- [H1] Angi Leads pro help, "How Leads Work": https://prohelp.homeadvisor.com/how-leads-work
- [H2] Angi Leads pro help, "How to request lead credits": https://prohelp.homeadvisor.com/how-to-request-lead-credits
- [G1] Google Local Services Ads help, "How leads work": https://support.google.com/localservices/answer/7195435

Reported prices and agency data (secondary):
- [G2] PaperStreet, automated LSA lead reviews (Aug 2024): https://www.paperstreet.com/blog/automated-lead-reviews-for-local-services-ads-progress-or-problem/
- [G3] The Media Captain, LSA cost per lead for 11 industries (Aug 2025, agency client data): https://themediacaptain.com/google-local-service-ad-statistics/
- [T1] Housecall Pro, "What is Thumbtack and how does it work": https://www.housecallpro.com/resources/what-is-thumbtack-how-it-works/
- [A1] Housecall Pro, "What is Angi's List and how does Angi work": https://www.housecallpro.com/resources/what-is-angis-list-how-angi-works/
- [A3] ClockShark, "Everything contractors need to know about HomeAdvisor": https://clockshark.com/blog/contractors-need-to-know-home-advisor

Anecdotal (owner forums):
- [T2] Thumbtack Community, "How Leads are priced": https://community.thumbtack.com/discussion/2397/how-leads-are-priced
- [A2] Jobber Community, "Is Angi worth it": https://community.getjobber.com/discussions/marketing-forum/angi/13803
- [G4] Local Search Forum, HVAC company billed for branded searches (2023): https://localsearchforum.com/threads/why-is-google-local-services-showing-ads-for-a-different-location-when-customers-search-for-our-company-name.59817

Research:
- [S1] Oldroyd, McElheran and Elkington, "The Short Life of Online Sales Leads", Harvard Business Review 89(3), 2011 (abstract only read): https://scholarsarchive.byu.edu/facpub/9711
- [S2] Pro Remodeler, Business Results Survey: repeats and referrals (2009): https://proremodeler.com/business-results-survey-repeats-and-referrals

Not legal advice; no lawyer reviewed this file (D-367).
