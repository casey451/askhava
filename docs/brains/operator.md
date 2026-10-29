# Brain: operator frameworks (Hormozi, Martell, Koerner)

- **Purpose:** the three operating frameworks Casey wants Ask Hava's business decisions judged by, summarized from sources, plus how the current decisions sit against them and a 10-question check before any business change.
- **Who reads it:** any session proposing or reviewing a business change: pricing and fees (FEE-*), offers and free wins, recruiting (BIZ-048, BIZ-053), intake friction (INT-*), the pilot scorecard (OPS-039, `docs/PREBUILD.md` section 7), the hand-run test, scope cuts, new costs, or the re-plan pass after the hand-run test (D-455).
- **How to use it:** run the 10 questions in section 7 on the change, name the framework point you relied on (tag in section 9) in the decision entry, and say where the frameworks disagree instead of picking the convenient one.
- **Last verified:** 2026-10-29.
- **It never overrides `docs/spec/` or `docs/architecture.md`.** Frameworks are lenses, not rules. If a framework points against the spec, build the spec and report the disagreement in your task report (and `docs/decisions.md` if your task owns that file). Tags: `[inferred]` = reasoning, not backed by a source read for this file.

**Provenance.** An earlier, fuller "operator pack" (Hormozi and Martell research, later extended with Koerner and Facebook-group recruiting; cited in D-456 to D-467 as "pack 01a", "01b", "02", "03", "09" and "10") lived outside this repository and was not available to this session. This file is a rebuilt, sourced condensation. Where a decision cites a pack line this session could not re-source, section 6 says so. The books themselves were not read for this file; the summaries below come from the authors' own published summaries and interviews where found, and otherwise from third-party book summaries, marked as such.

## 1. Alex Hormozi: offers and leads

**Who:** founder of Acquisition.com and author of *$100M Offers* (2021) and *$100M Leads* (2023), self-published through Acquisition.com Publishing [H5]. His books are widely read by small service-business owners [credibility note inferred].

### 1.1 The value equation (*$100M Offers*)
- Perceived value rises with the **dream outcome** and the **perceived likelihood of achievement**, and falls with the **time delay** and the **effort and sacrifice** it takes [H1].
- The two levers to cut are time delay and effort: "reducing the time required to experience results and minimizing the effort involved increases perceived value"; create early wins and remove obstacles [H1].
- Likelihood is built with credibility, proof and guarantees [H1].

### 1.2 The grand slam offer and price (*$100M Offers*)
- Find "a starving crowd" with real pain and the means to pay; list their problems; turn each into a solution; pick delivery vehicles; trim the costly-but-low-value parts and stack the rest [H2].
- Enhance it with bonuses, guarantees, naming, scarcity and urgency [H2]. (Ask Hava's voice guide bans urgency and scarcity language in copy, so only honest, factual deadlines apply: `docs/voice.md` rule 7.)
- Commodity offers "compete on price"; a premium price funds a better product: "Charge a Premium to Create a Virtuous Cycle" [H2].

### 1.3 Leads (*$100M Leads*)
- An engaged lead is "someone who has taken an action indicating interest" [H3]. (Ask Hava's word is "request" or "sign-up", never "lead": WORD-020.)
- **The Core Four**, by who you reach (warm or cold) and how (one-to-one or one-to-many): warm outreach, posting free content, cold outreach, paid ads [H3][H4].
- **Lead getters**, once your own hours run out: customer referrals, employees, agencies and affiliates or partners [H3].
- **Rule of 100:** "100 units of primary lead-generating activity per day for 100 days", meant as a floor against under-investment [H3].
- **More, better, new:** do more of the channel that works, then make it convert better, then add a new one [H3].

## 2. Dan Martell: buy back your time

**Who:** author of *Buy Back Your Time* (2023), a coach of software founders [credibility note inferred]. His own Next Big Idea Club summary [M1] and an Entrepreneur interview [M2] are the primary sources here.

- **Buyback principle:** "we don't hire people to grow our business — we hire people to buy back time out of our calendar" [M2].
- **Buyback rate:** annual income divided by 2,000 hours, divided by four; at $100,000 that's $12.50 an hour, and "you shouldn't be doing anything in your calendar that you could pay another person $12.50 or less an hour to do" [M1][M2].
- **Buyback loop:** when growth hits "the pain line", audit your time and energy, transfer low-value tasks, and fill the freed time with high-value work [M1]. The **DRIP matrix** sorts tasks by money and energy: Delegate, Replace, Invest, Produce [M3, third-party summary].
- **Replacement ladder:** hire in order: admin, delivery, marketing, sales, leadership [M1].
- **10-80-10:** you do the first 10% (direction) and last 10% (finish); someone else does the middle 80%; "80% done by somebody else is 100% ... awesome" [M1][M2].
- **Playbooks:** record yourself doing the task (the "Camcorder Method"), then the person taking over writes the playbook: course, cadence and checklist [M3, third-party summary].

## 3. Chris Koerner: boring businesses, sell first, keep the day job

**Who:** host of *The Koerner Office* podcast [K6]; founded LCDcycle (sold 2019) and Texas Snax, among others; runs the CoFounders holding company; says he has started more than 20 companies and sold five [K1][K3]. Wikipedia flags that many of these claims are self-reported [K1].

- **Boring service businesses:** window washing, appliance repair, TV mounting, lawn care: "unsexy", cheap to start, can charge premium prices, hard for AI to replace [K2, secondary summary]. On his tree company: "that's just a boring old tree trimming business" [K3].
- **Sell before you build:** get a customer first, even unpaid; skip the LLC, business plan and logo until demand is proven, which he calls "playing business"; pre-sell before producing [K2, secondary summary]. He ran Facebook tests to settle form factor, price and flavor before ordering samples [K4].
- **Charge more:** premium pricing is a main draw of service businesses; take deposits or payment up front [K2, secondary summary].
- **Side-business rules:** "There's enough time in the day to do this, on the nights and weekends"; "I just don't want people to burn the boats too soon. There has to be a pathway"; "So many ways to really vet something out before quitting that we don't have to quit" [K4]. He also warns it "has a very low ceiling if we're only working on it after hours" [K4].
- **Speed:** "shorten the amount of time spent between having the idea and doing something about that idea"; "focus is overrated and momentum is underrated" [K4].
- **Stack assets:** reuse what one business built in the next: his newsletter repurposes podcast episodes his readers never heard [K5]; he carried Facebook-ad lessons from earlier businesses [K4]; CoFounders plants ideas with partners [K3]. "Stack assets" as a named rule is [inferred] from these examples.

## 4. The frameworks applied to Ask Hava's parts

### The value equation for each side [H1]

| Lever | Homeowner | Business | Where the spec delivers it |
|---|---|---|---|
| Dream outcome | The job done by someone local, without calling around | A paying customer who already chose them | INT-001 one text or chat; BIZ-030 "an introduction fee for a homeowner who chose you" |
| Perceived likelihood | Several real quotes, license shown, "Your number is never shared" | The card is real, in their trade and town, with an honest count | WORD-007, PICK-041, CARD-019, CARD-001 |
| Time delay | First quote in hours, not days | The first card soon after sign-up | WEB-028's median-time number, D-460, D-462 |
| Effort and sacrifice | A few questions, no account, free | A few taps to quote, nothing to pay until a pick | INT-043 intake order, PICK-040 "Same as last time", BIZ-001 |

### The Core Four and lead getters, mapped to channels [H3][H4]

| Channel | Ask Hava's version | Limits |
|---|---|---|
| Warm outreach | People Casey knows send real jobs ("Seeded" in the hand-run) | Tests the process, not demand; scored separately (hand-run section 3) |
| Free content | Hand-written, disclosed posts in local Facebook groups; one homeowner tip post a week | Facebook bans automated group posting and paid admin permission (BIZ-048, hand-run section 2) |
| Cold outreach | Recruiting email only to businesses that asked; a few in-person visits | No recruiting texts or calls at all (BIZ-048); phone recruiting needs A.R.S. 44-1272 work first (D-374) |
| Paid ads | None yet | Koerner calls Facebook ads "the infinite money glitch" [K4]; any ad test is variable spend and needs Casey's go (CLAUDE.md rule 1) |
| Lead getters | Homeowner referrals (PICK-054); property managers and hosts ("Partner" source class); businesses telling other businesses ("Another business" in BIZ-053) | Copy never says "partner" or "affiliate" (WORD-020); no paid referral rewards [inferred from D-466's no-reward rule] |

### Martell's replacement ladder, with the system as most rungs [M1][inferred]

| Rung | Who does it at Ask Hava | What Casey keeps |
|---|---|---|
| Admin | The app: routing, notices, receipts, digests (OPS-030) | Reading the weekly digest |
| Delivery | The app: intake, cards, quote page, picks | Help requests the app can't answer (OPS-036) |
| Marketing | Casey's own posts | The one manual rung; the first hand-off candidate once posts are templated |
| Sales | Self-serve sign-up (BIZ-021 to BIZ-049) | Needs-review sign-ups (BIZ-027) |
| Leadership | Casey at the gates and monthly review | Stop and go, fee changes, new towns |

## 5. Where the three agree and disagree

| Question | Hormozi | Martell | Koerner | For Ask Hava |
|---|---|---|---|---|
| Price | Charge a premium [H2] | (not addressed) | Charge more [K2] | Fixed, low fees plus a free first win (FEE-001, WEB-005). This is the main tension; the hand-run's real payments decide (D-465, D-468). |
| Focus | Starving crowd; more-better-new, one channel at a time [H2][H3] | Time on high-value work [M1] | "focus is overrated" [K4] | All trades open (D-459), but marketing pushes one or two. |
| Volume of outreach | Rule of 100 [H3] | Protect the calendar [M1] | Nights and weekends [K4] | Casey has 20 to 40 hours a month. The Rule of 100 doesn't fit; self-serve recruiting (BIZ-048) and the system have to do the volume. |
| Build or test | Proof raises likelihood [H1] | Playbook first, then hand it off [M3] | Sell before you build [K2] | Hand-run test before Gate P (D-455). |

## 6. Ask Hava's current decisions against the frameworks (D-455 to D-467)

| Decision | Fits | Strains | Notes |
|---|---|---|---|
| **D-455** Hand-run test before the build | Koerner, sell before building [K2]; Hormozi, proof raises likelihood [H1]; Martell, the hand-run doc is the first playbook [M3] | Koerner would skip the LLC until demand shows [K2]; the paid weeks need it for Stripe | The LLC starts only when the test hits its quote line (D-467 item 4). That's the "pathway" rather than "playing business" [inferred]. |
| **D-456** Self-serve recruiting from disclosed Facebook posts | Core Four: free content plus warm outreach [H3][H4]; Koerner's distribution focus [K4] | Rule of 100 volume [H3]; Facebook as a single channel | Nextdoor as backup (hand-run section 7) follows more-better-new [H3]. Phone outreach still needs A.R.S. 44-1272 work (D-374). |
| **D-457** Record where each request and sign-up came from | More-better-new needs per-channel counts [H3] | none | The decision also cites Martell's single-channel risk from the outside pack; not re-sourced here. |
| **D-458** Scorecard with a 15/50/115 paid-introduction ladder and Casey's minutes | Martell: measure time and money per task [M1] | none | Buyback check: $20,000 over 240 to 480 hours a year is about $42 to $83 an hour, so a buyback rate of about $10 to $21 an hour [inferred: Martell's formula applied to side-business income and hours]. Anything Casey does that a $15-an-hour helper or the system could do is a candidate to hand off. |
| **D-459** Every enabled trade open | Koerner's breadth and momentum [K4] | Hormozi's starving-crowd focus [H2] | Pushing one or two trades in marketing keeps focus where it costs Casey's time. |
| **D-460** Town check first, 30-minute auto-send, no extra channel question | Value equation: cut time delay and effort [H1] | none | D-460 quotes "The only thing that beats free is fast" from the outside pack; not re-sourced here. |
| **D-461** One "all quotes are in" text and one closing note | Effort and likelihood: the homeowner knows when to choose [H1] | none | Follow-up cited from the outside pack (03 M7, 09 heuristic 27); not re-sourced. |
| **D-462** First real job before setup steps; Casey's personal welcome | Time delay to first value [H1] | Martell would hand the welcome off later [M1] | The personal welcome is playbook-only; fine for the pilot, a hand-off candidate after. |
| **D-463** Ask why a business leaves or pauses | Hormozi, talk to unhappy customers (outside pack, not re-sourced) | none | Matches the quit reasons in `contractor-lead-experience.md` section 4. |
| **D-464** Crowding flag at 5 businesses and 4+ median quotes | Value for businesses: likelihood of winning [H1] | Hormozi's volume instinct | The decision cites Koerner's "10 bidders for every bid" complaint from the outside pack; not re-sourced. |
| **D-465** 6-month price lock, refund at Ask Hava's choice, save-card box on the free win | Guarantees raise likelihood [H1]; a low intro price while value is unknown | Charge a premium [H2][K2] | Koerner's low-intro-price point is from the outside pack (09 section 11); not re-sourced. Fees are set after the paid weeks. |
| **D-466** Real numbers and opt-in homeowner quotes as proof | Proof raises likelihood [H1] | none | Numbers computed, never typed (WEB-028); no rewards for reviews. |
| **D-467** Cut the Pro plan; fixed costs about $150 a month; credit pack without auto-renew; handover guide | Koerner: keep it boring and simple [K2]; Martell: playbooks so the work can be handed off [M3] | Hormozi's offer stacking would add value to the paid side [H2] | Fewer offers means fewer things for the only maintainer (Claude Code) to break. |

## 7. Ten questions before any business change

1. **Which scorecard number does it move** (OPS-039, PREBUILD section 7, hand-run section 10), and how will we know within 4 weeks?
2. **Value equation:** for the homeowner or the business, does it raise the outcome or the likelihood, or cut the wait or the effort? Name which one [H1].
3. **Does it add a step before first value** for either side? If so, what does it remove to pay for it [H1]?
4. **Can it be tested by hand first** (a form, an email, a post) before anyone builds it [K2]?
5. **How many of Casey's hours a month does it take**, and is any of it work worth less than his buyback rate that the system or a helper should do instead [M1]?
6. **Does it add a fixed monthly cost?** Fixed running costs stay at about $150 a month, not counting the Claude plan (D-467).
7. **Does it keep the business boring and simple** for a maintainer who is only Claude Code (D-467 item 3)? What new failure can it cause at 2 a.m. with nobody watching?
8. **Which channel does it feed**, and is that the channel that's working (more-better-new) [H3]?
9. **Does it stay inside the guardrails:** not a contractor, no ranking, no "lead" wording, no urgency, no marketing texts, no calls to businesses, real counts only (WORD-*, TXT-008, BIZ-039, BIZ-048)? A framework never justifies crossing one.
10. **Who decides and can it be undone?** If it's a Casey stop point (CLAUDE.md, BUILD_BRIEF section 16), ask through `outbox/`; otherwise decide, log it in `docs/decisions.md` with the framework point used, and say how to reverse it.

## 8. How to use the frameworks without bending the product

- Hormozi's scarcity and urgency become plain, true deadlines (`docs/voice.md` rule 7), never pressure.
- "Charge more" is tested with real payments, not argued (D-468).
- The Rule of 100 is a reminder that recruiting is a volume game, not a quota for Casey: the system and self-serve sign-ups carry it [inferred].
- Martell's ladder maps onto Ask Hava with AI in most rungs: admin and delivery are the system, marketing is Casey's posts (the one manual rung), sales is self-serve sign-up, leadership is Casey at the gates [inferred].
- 10-80-10 is how this build already runs: Casey sets direction and approves gates; Claude does the middle [inferred].

## 9. Sources

Alex Hormozi:
- [H1] SuperSummary, *$100M Offers* Section 3, Chapter 6 "The Value Equation" (third-party summary): https://www.supersummary.com/100m-offers/section-3-summary/
- [H2] SuperSummary, *$100M Offers* summary (third-party): https://www.supersummary.com/100m-offers/summary/
- [H3] PulseRevOps, *$100M Leads* summary (third-party): https://pulserevops.com/sales-book-summaries/bs308
- [H4] Charl Laubscher's notes, "The Core Four" from *$100M Leads* (third-party): https://publish.obsidian.md/charllaubscher/05+Love+%2B+Money/03+Process/Processes+for+Partnerships/Sales/The+Core+Four
- [H5] Acquisition.com shop, the $100M series books (publisher page; located, not read): https://shop.acquisition.com/products/100m-trilogy-bundle

Dan Martell:
- [M1] Dan Martell, "Buy Back Your Time" Book Bite, Next Big Idea Club (author's own summary): https://nextbigideaclub.com/magazine/buy-back-time-get-unstuck-reclaim-freedom-build-empire-bookbite/39964/
- [M2] Entrepreneur, Jason Feifer interviews Dan Martell, 30 Jan 2023: https://www.entrepreneur.com/living/how-much-should-pay-other-people-to-take-work-off-your/443766
- [M3] SuperSummary, *Buy Back Your Time* summary (third-party): https://www.supersummary.com/buy-back-your-time/summary/

Chris Koerner:
- [K1] Wikipedia, "Chris J. Koerner" (flagged for self-reported sources): https://en.wikipedia.org/wiki/Chris_J._Koerner
- [K2] Shortform summary of *The School of Greatness* episode with Chris Koerner, July 2026 (secondary): https://www.shortform.com/podcast/episode/the-school-of-greatness-2026-07-08-episode-summary-the-fastest-way-to-make-10000-a-month-chris-koerner
- [K3] *Millennial Investing* MI334, "Confessions of an Entrepreneurial Addict", 11 Mar 2024: https://www.theinvestorspodcast.com/millennial-investing/confessions-entrepreneurial-addict/
- [K4] *The Diary of a CEO* with Chris Koerner, transcript (Singju Post; first 100,000 characters read): https://singjupost.com/serial-entrepreneur-chris-koerner-on-doac-podcast-transcript/
- [K5] beehiiv case study, The Koerner Office newsletter: https://www.beehiiv.com/case-studies/koerner-office.md
- [K6] *The Koerner Office* podcast feed: https://www.buzzsprout.com/2241079

Not reachable for this session: the earlier operator pack outside the repository (see Provenance), and the three books themselves.
